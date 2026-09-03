use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
struct FilesystemPathRules {
    case_sensitive: bool,
    normalization_sensitive: bool,
}

pub fn filesystem_path_key(path: &Path) -> io::Result<String> {
    if path.to_str().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not valid UTF-8",
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    absolute.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no final component")
    })?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.try_exists()? {
        let component = ancestor.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no existing ancestor")
        })?;
        if component.is_empty() || component == OsStr::new(".") || component == OsStr::new("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains an invalid target component",
            ));
        }
        suffix.push(component.to_owned());
        ancestor = ancestor.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no existing ancestor")
        })?;
    }
    let canonical_ancestor = fs::canonicalize(ancestor)?;
    let mut key = existing_path_key(&canonical_ancestor)?;
    if !suffix.is_empty() {
        let rules = directory_path_rules(&canonical_ancestor)?;
        for component in suffix.into_iter().rev() {
            key.push(missing_component_key(&component, rules)?);
        }
    }
    key.into_os_string().into_string().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "canonical path is not valid UTF-8",
        )
    })
}

pub fn filesystem_paths_equal(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(filesystem_path_key(left)? == filesystem_path_key(right)?)
}

fn directory_path_rules(directory: &Path) -> io::Result<FilesystemPathRules> {
    Ok(FilesystemPathRules {
        case_sensitive: !directory_names_alias(directory, "Aa", "aA", "case sensitivity")?,
        normalization_sensitive: !directory_names_alias(
            directory,
            "\u{e9}",
            "e\u{301}",
            "Unicode normalization",
        )?,
    })
}

fn existing_path_key(path: &Path) -> io::Result<PathBuf> {
    let mut actual = PathBuf::new();
    let mut key = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                actual.push(prefix.as_os_str());
                key.push(prefix.as_os_str());
            }
            Component::RootDir => {
                actual.push(component.as_os_str());
                key.push(component.as_os_str());
            }
            Component::Normal(name) => {
                key.push(existing_component_key(&actual, name)?);
                actual.push(name);
            }
            Component::CurDir | Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "canonical path contains a relative component",
                ));
            }
        }
    }
    Ok(key)
}

fn existing_component_key(parent: &Path, name: &OsStr) -> io::Result<String> {
    let name = name
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
    if !name.is_ascii() {
        // The component was returned by canonicalization, so retaining its on-disk
        // spelling neither approximates nor collapses the filesystem's Unicode rules.
        return Ok(name.to_owned());
    }
    let alias = name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_lowercase() {
                byte.to_ascii_uppercase()
            } else {
                byte.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>();
    let alias = String::from_utf8(alias).expect("ASCII case conversion remains UTF-8");
    if alias == name {
        return Ok(name.to_owned());
    }
    let original_path = parent.join(name);
    let alias_path = parent.join(alias);
    if !alias_path.try_exists()? {
        return Ok(name.to_owned());
    }
    let original_type = fs::symlink_metadata(&original_path)?.file_type();
    let alias_type = fs::symlink_metadata(&alias_path)?.file_type();
    let same_entry_kind = original_type.is_symlink() == alias_type.is_symlink()
        && original_type.is_dir() == alias_type.is_dir()
        && original_type.is_file() == alias_type.is_file();
    if same_entry_kind && fs::canonicalize(original_path)? == fs::canonicalize(alias_path)? {
        Ok(name.to_ascii_lowercase())
    } else {
        Ok(name.to_owned())
    }
}

fn missing_component_key(name: &OsStr, rules: FilesystemPathRules) -> io::Result<String> {
    let name = name
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
    if !name.is_ascii() && (!rules.case_sensitive || !rules.normalization_sensitive) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "non-ASCII target components are unsupported when the filesystem does not use exact name comparison",
        ));
    }
    Ok(if rules.case_sensitive {
        name.to_owned()
    } else {
        name.to_ascii_lowercase()
    })
}

fn directory_names_alias(
    directory: &Path,
    original_suffix: &str,
    alias_suffix: &str,
    rule: &str,
) -> io::Result<bool> {
    static NEXT_PROBE: AtomicU64 = AtomicU64::new(1);

    for _ in 0..16 {
        let id = NEXT_PROBE.fetch_add(1, Ordering::Relaxed);
        let prefix = format!(".taskctl-path-probe-{}-{id}", std::process::id());
        let original = directory.join(format!("{prefix}-{original_suffix}"));
        let alias = directory.join(format!("{prefix}-{alias_suffix}"));
        match fs::create_dir(&original) {
            Ok(()) => {
                let observed = alias.try_exists();
                let removed = fs::remove_dir(&original);
                if let Err(error) = removed {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("cannot remove filesystem {rule} probe: {error}"),
                    ));
                }
                return observed;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("cannot determine filesystem {rule}: {error}"),
                ));
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("cannot allocate a filesystem {rule} probe"),
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Blocked,
    Closed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
        }
    }
}

impl TryFrom<&str> for TaskStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "open" => Ok(Self::Open),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("unknown task status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub version: i64,
    pub goal: String,
    pub scope: String,
    pub acceptance_criteria: String,
    pub next_step: Option<String>,
    pub block_reason: Option<String>,
    pub block_recovery: Option<String>,
    pub current_session_id: Option<String>,
    pub repository_path: Option<String>,
    pub repository_common_dir: Option<String>,
    pub repository_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub latest_checkpoint_id: Option<String>,
    pub closure_outcome: Option<String>,
    pub closure_reason: Option<String>,
    pub closed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub task_id: String,
    pub source: Option<String>,
    pub external_session_id: Option<String>,
    pub continued_from: Option<String>,
    pub record_path: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointView {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub summary: String,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub pending: Vec<String>,
    pub next_step: String,
    pub risks: Vec<String>,
    pub git_head: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskNoteView {
    pub id: i64,
    pub task_id: String,
    pub session_id: Option<String>,
    pub note_type: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionImportView {
    pub id: String,
    pub session_id: String,
    pub source_path: String,
    pub media_type: Option<String>,
    pub sha256: String,
    pub size_bytes: i64,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub registered: bool,
    pub repository_path: Option<String>,
    pub repository_common_dir: Option<String>,
    pub path: Option<String>,
    pub exists: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub staged: Option<Vec<String>>,
    pub unstaged: Option<Vec<String>>,
    pub untracked: Option<Vec<String>>,
    pub ignored: Option<Vec<String>>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub task_id: String,
    pub sequence: i64,
    pub change_type: String,
    pub session_id: Option<String>,
    pub occurred_at: String,
    pub summary: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCreateInput {
    pub title: String,
    pub goal: String,
    pub scope: String,
    pub acceptance_criteria: String,
    #[serde(default)]
    pub next_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointInput {
    pub summary: String,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub pending: Vec<String>,
    pub next_step: String,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub code: String,
    pub message: String,
    pub details: Value,
}

pub fn require_non_empty(field: &str, value: &str) -> Result<String, (String, String)> {
    let value = value.trim();
    if value.is_empty() {
        Err((field.to_owned(), "must be a non-empty string".to_owned()))
    } else {
        Ok(value.to_owned())
    }
}

pub fn validate_string_array(field: &str, values: &[String]) -> Result<(), (String, String)> {
    if values.iter().any(|value| value.trim().is_empty()) {
        Err((
            field.to_owned(),
            "items must be non-empty strings".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/agent-steward"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|root| root.join("agent-steward"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(root).join("agent-steward"));
        }
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| home.join(".local/share/agent-steward"))
    }
}

pub fn set_private_dir(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        windows_permissions::set_private_acl(path, true)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

pub fn set_private_file(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(windows)]
    {
        windows_permissions::set_private_acl(path, false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(windows)]
pub fn private_acl_is_protected(path: &Path) -> Result<bool, std::io::Error> {
    windows_permissions::private_acl_is_protected(path)
}

#[cfg(test)]
mod path_key_tests {
    use super::*;

    #[test]
    fn path_key_is_stable_when_case_varied_parent_disappears() {
        let temp = tempfile::tempdir().unwrap();
        let aliases_case = directory_names_alias(temp.path(), "Aa", "aA", "test").unwrap();
        let parent = temp.path().join("MixedParent");
        fs::create_dir(&parent).unwrap();
        let before = filesystem_path_key(&parent.join("Worktree")).unwrap();

        fs::remove_dir(&parent).unwrap();
        let after = filesystem_path_key(&temp.path().join("mixedparent/worktree")).unwrap();

        assert_eq!(before == after, aliases_case);
    }

    #[test]
    fn path_key_refuses_a_missing_unicode_component_when_comparison_is_not_exact() {
        let temp = tempfile::tempdir().unwrap();
        let rules = directory_path_rules(temp.path()).unwrap();
        let parent = temp.path().join("Caf\u{e9}Parent");
        fs::create_dir(&parent).unwrap();
        let before = filesystem_path_key(&parent.join("Worktree")).unwrap();

        fs::remove_dir(&parent).unwrap();
        let after = filesystem_path_key(&temp.path().join("Cafe\u{301}Parent/Worktree"));

        if rules.case_sensitive && rules.normalization_sensitive {
            assert_ne!(before, after.unwrap());
        } else {
            assert_eq!(after.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn unicode_case_mapping_is_never_used_for_a_missing_component() {
        let temp = tempfile::tempdir().unwrap();
        let rules = directory_path_rules(temp.path()).unwrap();
        let dotted_upper = filesystem_path_key(&temp.path().join("\u{130}"));
        let dotted_lower = filesystem_path_key(&temp.path().join("i\u{307}"));

        if rules.case_sensitive && rules.normalization_sensitive {
            assert_ne!(dotted_upper.unwrap(), dotted_lower.unwrap());
        } else {
            for error in [dotted_upper.unwrap_err(), dotted_lower.unwrap_err()] {
                assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
                assert!(error.to_string().contains("non-ASCII target components"));
            }
        }
    }

    #[test]
    fn existing_unicode_final_component_uses_its_canonical_spelling() {
        let temp = tempfile::tempdir().unwrap();
        let worktree = temp.path().join("工作树");
        fs::create_dir(&worktree).unwrap();

        assert!(filesystem_path_key(&worktree).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn insensitive_children_do_not_fold_names_from_a_sensitive_parent() {
        let temp = tempfile::tempdir().unwrap();
        let sensitive = temp.path().join("sensitive");
        fs::create_dir(&sensitive).unwrap();
        if !set_windows_case_sensitivity(&sensitive, true) {
            eprintln!("skipped: per-directory case sensitivity is unavailable");
            return;
        }
        let upper = sensitive.join("Foo");
        let lower = sensitive.join("foo");
        fs::create_dir(&upper).unwrap();
        fs::create_dir(&lower).unwrap();
        if !set_windows_case_sensitivity(&upper, false)
            || !set_windows_case_sensitivity(&lower, false)
        {
            eprintln!("skipped: inherited case sensitivity cannot be disabled");
            return;
        }

        assert_ne!(
            filesystem_path_key(&upper.join("Worktree")).unwrap(),
            filesystem_path_key(&lower.join("Worktree")).unwrap()
        );
    }

    #[cfg(windows)]
    fn set_windows_case_sensitivity(path: &Path, enabled: bool) -> bool {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FILE_WRITE_ATTRIBUTES, FileCaseSensitiveInfo, OPEN_EXISTING,
            SetFileInformationByHandle,
        };
        use windows_sys::Win32::System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR;

        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let information = FILE_CASE_SENSITIVE_INFO {
            Flags: if enabled {
                FILE_CS_FLAG_CASE_SENSITIVE_DIR
            } else {
                0
            },
        };
        let succeeded = unsafe {
            SetFileInformationByHandle(
                handle,
                FileCaseSensitiveInfo,
                std::ptr::addr_of!(information).cast(),
                std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
            )
        } != 0;
        unsafe {
            CloseHandle(handle);
        }
        succeeded
    }
}

#[cfg(windows)]
mod windows_permissions {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::{io, mem, ptr, slice};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, ACCESS_DENIED_CALLBACK_ACE_TYPE,
        ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_DENIED_OBJECT_ACE_TYPE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    pub(super) fn set_private_acl(path: &Path, directory: bool) -> io::Result<()> {
        let sid = current_user_sid()?;
        let flags = if directory { "OICI" } else { "" };
        apply_dacl_sddl(
            path,
            &format!("D:P(A;{flags};FA;;;{sid})(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)"),
        )
    }

    fn apply_dacl_sddl(path: &Path, sddl: &str) -> io::Result<()> {
        let sddl_wide = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor);
        let path_wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        } == 0
            || dacl_present == 0
            || dacl.is_null()
        {
            return Err(io::Error::last_os_error());
        }
        let error = unsafe {
            SetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(())
    }

    pub(super) fn private_acl_is_protected(path: &Path) -> io::Result<bool> {
        let path_wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        let error = unsafe {
            GetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let _descriptor = LocalAllocation(descriptor);
        let mut control = 0_u16;
        let mut revision = 0_u32;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl.is_null() || control & SE_DACL_PROTECTED == 0 {
            return Ok(false);
        }

        dacl_allows_only_approved_principals(dacl, &current_user_sid()?)
    }

    fn dacl_allows_only_approved_principals(
        dacl: *const ACL,
        current_user: &str,
    ) -> io::Result<bool> {
        let allowed_sids = [
            current_user,
            "S-1-5-18",     // LocalSystem
            "S-1-5-32-544", // Builtin Administrators
        ];
        let mut information = ACL_SIZE_INFORMATION::default();
        if unsafe {
            GetAclInformation(
                dacl,
                ptr::addr_of_mut!(information).cast(),
                mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        for index in 0..information.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            match header.AceType as u32 {
                ACCESS_ALLOWED_ACE_TYPE => {
                    if usize::from(header.AceSize) < mem::size_of::<ACCESS_ALLOWED_ACE>() {
                        return Ok(false);
                    }
                    let allowed_ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                    let sid_pointer = unsafe { ptr::addr_of!((*allowed_ace).SidStart) }
                        .cast_mut()
                        .cast::<c_void>();
                    let sid = sid_to_string(sid_pointer)?;
                    if !allowed_sids.contains(&sid.as_str()) {
                        return Ok(false);
                    }
                }
                ACCESS_DENIED_ACE_TYPE
                | ACCESS_DENIED_OBJECT_ACE_TYPE
                | ACCESS_DENIED_CALLBACK_ACE_TYPE
                | ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn test_sddl_is_private(sddl: &str) -> io::Result<bool> {
        let sddl_wide = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor);
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut control = 0_u16;
        let mut revision = 0_u32;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl_present == 0 || dacl.is_null() || control & SE_DACL_PROTECTED == 0 {
            return Ok(false);
        }
        dacl_allows_only_approved_principals(dacl, &current_user_sid()?)
    }

    pub(super) fn current_user_sid() -> io::Result<String> {
        let mut raw_token: HANDLE = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(raw_token);
        let mut required = 0_u32;
        unsafe {
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required);
        }
        if required == 0 {
            return Err(io::Error::last_os_error());
        }
        let word_size = mem::size_of::<usize>();
        let mut buffer = vec![0_usize; (required as usize).div_ceil(word_size)];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let token_user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        sid_to_string(token_user.User.Sid)
    }

    fn sid_to_string(sid: *mut c_void) -> io::Result<String> {
        let mut sid_string = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut sid_string) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _sid_string = LocalAllocation(sid_string.cast());
        let mut length = 0;
        while unsafe { *sid_string.add(length) } != 0 {
            length += 1;
        }
        String::from_utf16(unsafe { slice::from_raw_parts(sid_string, length) })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

#[cfg(all(test, windows))]
mod windows_permission_tests {
    use super::*;

    #[test]
    fn protected_acl_that_allows_everyone_is_not_private() {
        assert!(!windows_permissions::test_sddl_is_private("D:P(A;OICI;FA;;;WD)").unwrap());
    }

    #[test]
    fn protected_acl_with_an_unknown_user_is_not_private() {
        assert!(
            !windows_permissions::test_sddl_is_private("D:P(A;OICI;FA;;;S-1-5-21-999-999-999-999)")
                .unwrap()
        );
    }

    #[test]
    fn unprotected_acl_is_not_private() {
        assert!(!windows_permissions::test_sddl_is_private("D:(A;;FA;;;WD)").unwrap());
    }

    #[test]
    fn applied_private_acl_is_private() {
        let user = windows_permissions::current_user_sid().unwrap();
        let sddl = format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
        assert!(windows_permissions::test_sddl_is_private(&sddl).unwrap());
    }
}
