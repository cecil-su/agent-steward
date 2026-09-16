//! Non-Git filesystem identity and local path validation.
//! Persisted records compare identifiers; they do not prove continuity between processes.
use std::ffi::OsStr;
use std::fs;
#[cfg(target_os = "linux")]
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("path identity cannot be established: {0}")]
    PathIdentity(String),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

/// A live observation. Linux clones share a metadata-only pin; never deserialize a live handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingPathIdentity {
    pub canonical_path: PathBuf,
    object: FileObjectIdentity,
}

/// A persisted comparison record, NOT proof of continuity since a prior process/observation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExistingPathIdentityRecord {
    pub canonical_path: PathBuf,
    object: FileObjectIdentityRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileObjectIdentityRecord {
    first: u64,
    second: u64,
}

// Preserve the existing on-disk/hash representation, without serializing process resources.
impl serde::Serialize for ExistingPathIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.record(), serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPathIdentity {
    pub canonical_path: PathBuf,
    existing_ancestor: ExistingPathIdentity,
}

#[derive(Debug, Clone)]
struct FileObjectIdentity {
    first: u64,
    second: u64,
    // Pin the object until the last identity snapshot is dropped. Without this,
    // Linux can reuse an unlinked inode before the path is checked again.
    #[cfg(target_os = "linux")]
    _handle: std::sync::Arc<File>,
}

impl PartialEq for FileObjectIdentity {
    fn eq(&self, other: &Self) -> bool {
        (self.first, self.second) == (other.first, other.second)
    }
}

impl Eq for FileObjectIdentity {}

/// Make absolute without lexical security validation; use `local_path` for that.
pub fn absolute_clean(path: &Path) -> Result<PathBuf, PathError> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

pub fn canonicalize_existing(path: &Path) -> Result<PathBuf, PathError> {
    Ok(identify_existing(path)?.canonical_path)
}

pub fn canonicalize_target(path: &Path) -> Result<PathBuf, PathError> {
    Ok(identify_target(path)?.canonical_path)
}

pub fn identify_existing(path: &Path) -> Result<ExistingPathIdentity, PathError> {
    let absolute = absolute_clean(path)?;
    require_unicode_path(&absolute)?;
    let canonical_path = fs::canonicalize(&absolute)
        .map_err(|error| PathError::PathIdentity(format!("{}: {error}", absolute.display())))?;
    require_unicode_path(&canonical_path)?;
    let object = file_object_identity(&canonical_path)?;
    Ok(ExistingPathIdentity {
        canonical_path,
        object,
    })
}

pub fn identify_target(path: &Path) -> Result<TargetPathIdentity, PathError> {
    let absolute = absolute_clean(path)?;
    require_unicode_path(&absolute)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.try_exists().map_err(|error| {
        PathError::PathIdentity(format!("cannot inspect {}: {error}", ancestor.display()))
    })? {
        let name = ancestor.file_name().ok_or_else(|| {
            PathError::PathIdentity(format!("no existing ancestor for {}", absolute.display()))
        })?;
        if name.is_empty() || name == OsStr::new(".") || name == OsStr::new("..") {
            return Err(PathError::PathIdentity("invalid target component".into()));
        }
        suffix.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            PathError::PathIdentity(format!("no existing ancestor for {}", absolute.display()))
        })?;
    }
    let existing_ancestor = identify_existing(ancestor)?;
    let mut canonical_path = existing_ancestor.canonical_path.clone();
    for component in suffix.into_iter().rev() {
        canonical_path.push(component);
    }
    require_unicode_path(&canonical_path)?;
    Ok(TargetPathIdentity {
        canonical_path,
        existing_ancestor,
    })
}

pub fn path_exists(path: &Path) -> Result<bool, PathError> {
    path.try_exists().map_err(|error| {
        PathError::PathIdentity(format!("cannot inspect {}: {error}", path.display()))
    })
}

pub fn paths_equivalent(left: &Path, right: &Path) -> Result<bool, PathError> {
    let left_target = canonicalize_target(left)?;
    let right_target = canonicalize_target(right)?;
    if left_target == right_target {
        return Ok(true);
    }
    let left_exists = path_exists(left)?;
    let right_exists = path_exists(right)?;
    if left_exists && right_exists {
        return Ok(identify_existing(left)?.same_object(&identify_existing(right)?));
    }
    // Missing paths have no object identity: only the exact canonical target
    // spelling above can establish equality. Never probe or emulate name rules.
    Ok(false)
}

fn require_unicode_path(path: &Path) -> Result<(), PathError> {
    if path.to_str().is_some() {
        Ok(())
    } else {
        Err(PathError::PathIdentity(format!(
            "path is not valid UTF-8 and cannot be represented by the V0 contract: {}",
            path.display()
        )))
    }
}

pub fn verify_target_identity(identity: &TargetPathIdentity) -> Result<(), PathError> {
    let current = identify_target(&identity.canonical_path)?;
    verify_same_identity("target path", identity, &current)
}

fn verify_same_identity<T: PartialEq>(
    label: &str,
    expected: &T,
    current: &T,
) -> Result<(), PathError> {
    if current == expected {
        Ok(())
    } else {
        Err(PathError::PathIdentity(format!(
            "{label} changed after it was checked"
        )))
    }
}

#[cfg(unix)]
fn file_object_identity(path: &Path) -> Result<FileObjectIdentity, PathError> {
    use std::os::unix::fs::MetadataExt;

    #[cfg(target_os = "linux")]
    let handle = {
        use std::os::unix::fs::OpenOptionsExt;
        // O_PATH pins metadata without opening a FIFO/device for I/O or requiring
        // read permission. Derive dev/ino from this handle, not another path lookup.
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_CLOEXEC)
            .open(path)
            .map_err(|error| {
                PathError::PathIdentity(format!("cannot pin {}: {error}", path.display()))
            })?
    };
    #[cfg(target_os = "linux")]
    let metadata = handle.metadata().map_err(|error| {
        PathError::PathIdentity(format!("cannot inspect {}: {error}", path.display()))
    })?;
    #[cfg(not(target_os = "linux"))]
    let metadata = fs::metadata(path).map_err(|error| {
        PathError::PathIdentity(format!("cannot inspect {}: {error}", path.display()))
    })?;
    Ok(FileObjectIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
        #[cfg(target_os = "linux")]
        _handle: std::sync::Arc::new(handle),
    })
}

#[cfg(windows)]
fn file_object_identity(path: &Path) -> Result<FileObjectIdentity, PathError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
        OPEN_EXISTING,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = std::io::Error::last_os_error();
        return Err(PathError::PathIdentity(format!(
            "cannot open {} for identity: {error}",
            path.display()
        )));
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let succeeded = unsafe { GetFileInformationByHandle(handle, &mut information) } != 0;
    let result = if succeeded {
        Ok(FileObjectIdentity {
            first: u64::from(information.dwVolumeSerialNumber),
            second: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
    } else {
        let error = std::io::Error::last_os_error();
        Err(PathError::PathIdentity(format!(
            "cannot read identity for {}: {error}",
            path.display()
        )))
    };
    unsafe {
        CloseHandle(handle);
    }
    result
}

#[cfg(not(any(unix, windows)))]
fn file_object_identity(path: &Path) -> Result<FileObjectIdentity, PathError> {
    Err(PathError::PathIdentity(format!(
        "file object identity is unsupported on this platform: {}",
        path.display()
    )))
}

/// Lexical validation only: never canonicalize caller-provided HTTP paths here.
/// Network/device prefixes, ADS, and parent traversal are not local path selections.
pub fn local_path(path: &Path) -> Result<PathBuf, PathError> {
    use std::path::Component;
    let invalid = || {
        PathError::PathIdentity("expected a local absolute path without parent traversal".into())
    };
    let text = path.to_str().ok_or_else(invalid)?;
    if !path.is_absolute() || text.starts_with("//") || text.contains('\0') {
        return Err(invalid());
    }
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            #[cfg(windows)]
            Component::Prefix(prefix) => match prefix.kind() {
                std::path::Prefix::Disk(drive) | std::path::Prefix::VerbatimDisk(drive) => {
                    result.push(format!("{}:\\", drive.to_ascii_uppercase() as char))
                }
                _ => return Err(invalid()),
            },
            Component::RootDir => {
                if result.as_os_str().is_empty() {
                    result.push(std::path::MAIN_SEPARATOR.to_string());
                }
            }
            Component::CurDir => {}
            Component::Normal(name) => {
                #[cfg(windows)]
                if name.to_string_lossy().contains(':') {
                    return Err(invalid());
                }
                result.push(name);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(result)
}

impl ExistingPathIdentity {
    pub fn record(&self) -> ExistingPathIdentityRecord {
        ExistingPathIdentityRecord {
            canonical_path: self.canonical_path.clone(),
            object: FileObjectIdentityRecord {
                first: self.object.first,
                second: self.object.second,
            },
        }
    }

    /// Native object identity comparison; never a case-folded path comparison key.
    pub fn same_object(&self, other: &Self) -> bool {
        self.object == other.object
    }
}

impl ExistingPathIdentityRecord {
    /// Current identifier agreement only; a record cannot retain a prior process's inode pin.
    pub fn same_object(&self, current: &ExistingPathIdentity) -> bool {
        self.object.first == current.object.first && self.object.second == current.object.second
    }
}

/// Acquire a NEW live observation that agrees with a stored record. Does not authenticate
/// continuity before this call (e.g. inode reuse after the original snapshot was dropped).
pub fn observe_recorded_identity(
    record: &ExistingPathIdentityRecord,
) -> Result<ExistingPathIdentity, PathError> {
    let current = identify_existing(&record.canonical_path)?;
    verify_same_identity("recorded directory", record, &current.record())?;
    Ok(current)
}

pub fn verify_existing_identity(identity: &ExistingPathIdentity) -> Result<(), PathError> {
    verify_same_identity(
        "registered directory",
        identity,
        &identify_existing(&identity.canonical_path)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn record_preserves_wire_format_but_never_deserializes_a_live_handle() {
        let temp = tempfile::tempdir().unwrap();
        let live = identify_existing(temp.path()).unwrap();
        let wire = serde_json::to_value(&live).unwrap();
        assert_eq!(
            wire,
            json!({"canonical_path":live.canonical_path,"object":{"first":live.object.first,"second":live.object.second}})
        );
        let record: ExistingPathIdentityRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(record, live.record());
        assert_eq!(wire, serde_json::to_value(&record).unwrap());
        assert_eq!(
            serde_json::to_vec(&live).unwrap(),
            serde_json::to_vec(&record).unwrap()
        );
        assert_eq!(wire, serde_json::to_value(live.clone()).unwrap());
        assert!(record.same_object(&live));
        assert_eq!(observe_recorded_identity(&record).unwrap(), live);
        let mut forged = wire.clone();
        forged["object"]["_handle"] = json!(42);
        assert!(serde_json::from_value::<ExistingPathIdentityRecord>(forged).is_err());
        let mut forged = wire;
        forged["pinned"] = json!(true);
        assert!(serde_json::from_value::<ExistingPathIdentityRecord>(forged).is_err());
    }

    #[test]
    fn recorded_identity_requires_a_fresh_matching_observation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source");
        fs::create_dir(&path).unwrap();
        let record = identify_existing(&path).unwrap().record();
        fs::rename(&path, temp.path().join("old-source")).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(!record.same_object(&identify_existing(&path).unwrap()));
        assert!(matches!(
            observe_recorded_identity(&record),
            Err(PathError::PathIdentity(_))
        ));
    }

    #[test]
    fn target_ancestor_replacement_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("missing/file");
        let identity = identify_target(&target).unwrap();
        assert_eq!(
            canonicalize_target(&target).unwrap(),
            identity.canonical_path
        );
        verify_target_identity(&identity).unwrap();
        fs::rename(&parent, temp.path().join("held")).unwrap();
        fs::create_dir(&parent).unwrap();
        assert!(verify_target_identity(&identity).is_err());
    }

    #[test]
    fn existing_identity_replacement_and_hard_link_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file");
        fs::write(&path, b"data").unwrap();
        let live = identify_existing(&path).unwrap();
        assert_eq!(canonicalize_existing(&path).unwrap(), live.canonical_path);
        verify_existing_identity(&live).unwrap();
        let alias = temp.path().join("alias");
        fs::hard_link(&path, &alias).unwrap();
        assert!(paths_equivalent(&path, &alias).unwrap());
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"data").unwrap();
        assert!(verify_existing_identity(&live).is_err());
        assert!(!paths_equivalent(&path, &alias).unwrap());
    }

    #[test]
    fn local_path_validation_is_lexical_and_rejects_unsafe_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let absent = temp.path().join("not-created/file");
        assert!(local_path(&absent).is_ok());
        assert!(!absent.exists());
        for path in [
            Path::new("relative/file"),
            Path::new("//server/share"),
            Path::new("/bad\0name"),
        ] {
            assert!(local_path(path).is_err());
        }
        assert!(local_path(&temp.path().join("../escape")).is_err());
        #[cfg(windows)]
        for path in [
            r"C:\data\file:stream",
            r"\\server\share\file",
            r"\\.\PhysicalDrive0",
            r"\\?\UNC\server\share",
        ] {
            assert!(local_path(Path::new(path)).is_err());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn serialized_records_do_not_retain_pins_and_clones_release_the_last_pin() {
        let temp = tempfile::tempdir().unwrap();
        let live = identify_existing(temp.path()).unwrap();
        let weak = std::sync::Arc::downgrade(&live.object._handle);
        let clone = live.clone();
        let text = serde_json::to_string(&live).unwrap();
        let record: ExistingPathIdentityRecord = serde_json::from_str(&text).unwrap();
        assert_eq!(weak.strong_count(), 2);
        drop(live);
        assert_eq!(weak.strong_count(), 1);
        drop(clone);
        assert!(weak.upgrade().is_none());
        // Re-observation opens a different handle, not a resurrection from JSON.
        let fresh = observe_recorded_identity(&record).unwrap();
        assert!(record.same_object(&fresh));
        assert!(weak.upgrade().is_none());
    }
}
