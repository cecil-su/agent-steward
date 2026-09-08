use std::borrow::Cow;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(windows)]
use std::path::Component;

use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use steward_core::WorktreeStatus;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("path identity cannot be established: {0}")]
    PathIdentity(String),
    #[error("git command failed during {operation}: {summary}")]
    CommandFailed {
        operation: String,
        exit_status: Option<i32>,
        summary: String,
    },
    #[error("worktree safety check refused: {0}")]
    SafetyRefused(String),
    #[error("worktree operation is already running")]
    OperationBusy,
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct RepositoryInfo {
    pub repository_path: PathBuf,
    pub common_dir: PathBuf,
    repository_identity: ExistingPathIdentity,
    common_dir_identity: ExistingPathIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingPathIdentity {
    pub canonical_path: PathBuf,
    object: FileObjectIdentity,
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

#[derive(Debug, Clone)]
pub struct ObservedWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug)]
pub struct GitInvocation {
    pub operation: String,
    pub output: Result<Output, std::io::Error>,
}

impl GitInvocation {
    pub fn succeeded(&self) -> bool {
        self.output
            .as_ref()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn exit_status(&self) -> Option<i32> {
        self.output
            .as_ref()
            .ok()
            .and_then(|output| output.status.code())
    }

    pub fn diagnostic_summary(&self) -> String {
        match &self.output {
            Ok(output) => summarize_stderr(&output.stderr),
            Err(error) => error.to_string(),
        }
    }

    pub fn command_error(&self) -> GitError {
        GitError::CommandFailed {
            operation: self.operation.clone(),
            exit_status: self.exit_status(),
            summary: self.diagnostic_summary(),
        }
    }
}

pub struct WorktreeLock {
    file: File,
}

impl Drop for WorktreeLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn acquire_worktree_lock(
    database_path: &Path,
    task_id: &str,
    lock_root_override: Option<&Path>,
) -> Result<WorktreeLock, GitError> {
    let lock_root = match lock_root_override {
        Some(path) => path.to_path_buf(),
        None => steward_core::default_data_dir()
            .ok_or_else(|| GitError::PathIdentity("user data directory is unavailable".into()))?
            .join("locks"),
    };
    fs::create_dir_all(&lock_root)?;
    steward_core::set_private_dir(&lock_root)?;
    let database = if path_exists(database_path)? {
        canonicalize_existing(database_path)?
    } else {
        canonicalize_target(database_path)?
    };
    let mut hash = Sha256::new();
    hash.update(database.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(task_id.as_bytes());
    let path = lock_root.join(format!("{}.lock", hex::encode(hash.finalize())));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    steward_core::set_private_file(&path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(WorktreeLock { file }),
        Err(error) if is_lock_contention(&error) => Err(GitError::OperationBusy),
        Err(error) => Err(GitError::Io(error)),
    }
}

fn is_lock_contention(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    {
        // LockFileEx reports these raw Win32 errors instead of WouldBlock.
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        matches!(
            error.raw_os_error(),
            Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
        )
    }
    #[cfg(not(windows))]
    false
}

pub fn absolute_clean(path: &Path) -> Result<PathBuf, GitError> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

pub fn canonicalize_existing(path: &Path) -> Result<PathBuf, GitError> {
    Ok(identify_existing(path)?.canonical_path)
}

pub fn canonicalize_target(path: &Path) -> Result<PathBuf, GitError> {
    Ok(identify_target(path)?.canonical_path)
}

pub fn identify_existing(path: &Path) -> Result<ExistingPathIdentity, GitError> {
    let absolute = absolute_clean(path)?;
    require_unicode_path(&absolute)?;
    let canonical_path = fs::canonicalize(&absolute)
        .map_err(|error| GitError::PathIdentity(format!("{}: {error}", absolute.display())))?;
    require_unicode_path(&canonical_path)?;
    let object = file_object_identity(&canonical_path)?;
    Ok(ExistingPathIdentity {
        canonical_path,
        object,
    })
}

pub fn identify_target(path: &Path) -> Result<TargetPathIdentity, GitError> {
    let absolute = absolute_clean(path)?;
    require_unicode_path(&absolute)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.try_exists().map_err(|error| {
        GitError::PathIdentity(format!("cannot inspect {}: {error}", ancestor.display()))
    })? {
        let name = ancestor.file_name().ok_or_else(|| {
            GitError::PathIdentity(format!("no existing ancestor for {}", absolute.display()))
        })?;
        if name.is_empty() || name == OsStr::new(".") || name == OsStr::new("..") {
            return Err(GitError::PathIdentity("invalid target component".into()));
        }
        suffix.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            GitError::PathIdentity(format!("no existing ancestor for {}", absolute.display()))
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

pub fn path_exists(path: &Path) -> Result<bool, GitError> {
    path.try_exists().map_err(|error| {
        GitError::PathIdentity(format!("cannot inspect {}: {error}", path.display()))
    })
}

pub fn paths_equivalent(left: &Path, right: &Path) -> Result<bool, GitError> {
    let left_target = canonicalize_target(left)?;
    let right_target = canonicalize_target(right)?;
    if left_target == right_target {
        return Ok(true);
    }
    let left_exists = path_exists(left)?;
    let right_exists = path_exists(right)?;
    if left_exists && right_exists {
        return Ok(identify_existing(left)? == identify_existing(right)?);
    }
    // Missing paths have no object identity: only the exact canonical target
    // spelling above can establish equality. Never probe or emulate name rules.
    Ok(false)
}

fn require_unicode_path(path: &Path) -> Result<(), GitError> {
    if path.to_str().is_some() {
        Ok(())
    } else {
        Err(GitError::PathIdentity(format!(
            "path is not valid UTF-8 and cannot be represented by the V0 contract: {}",
            path.display()
        )))
    }
}

pub fn verify_target_identity(identity: &TargetPathIdentity) -> Result<(), GitError> {
    let current = identify_target(&identity.canonical_path)?;
    verify_same_identity("target path", identity, &current)
}

pub fn verify_repository_identity(info: &RepositoryInfo) -> Result<(), GitError> {
    let repository = identify_existing(&info.repository_path)?;
    verify_same_identity("repository", &info.repository_identity, &repository)?;
    let common_dir = identify_existing(&info.common_dir)?;
    verify_same_identity(
        "Git common directory",
        &info.common_dir_identity,
        &common_dir,
    )?;
    let current_common_dir =
        resolve_common_dir_identity(&info.repository_path, "repository identity verification")?;
    verify_same_identity(
        "repository Git common directory association",
        &info.common_dir_identity,
        &current_common_dir,
    )
}

fn verify_same_identity<T: PartialEq>(
    label: &str,
    expected: &T,
    current: &T,
) -> Result<(), GitError> {
    if current == expected {
        Ok(())
    } else {
        Err(GitError::PathIdentity(format!(
            "{label} changed after it was checked"
        )))
    }
}

#[cfg(unix)]
fn file_object_identity(path: &Path) -> Result<FileObjectIdentity, GitError> {
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
                GitError::PathIdentity(format!("cannot pin {}: {error}", path.display()))
            })?
    };
    #[cfg(target_os = "linux")]
    let metadata = handle.metadata().map_err(|error| {
        GitError::PathIdentity(format!("cannot inspect {}: {error}", path.display()))
    })?;
    #[cfg(not(target_os = "linux"))]
    let metadata = fs::metadata(path).map_err(|error| {
        GitError::PathIdentity(format!("cannot inspect {}: {error}", path.display()))
    })?;
    Ok(FileObjectIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
        #[cfg(target_os = "linux")]
        _handle: std::sync::Arc::new(handle),
    })
}

#[cfg(windows)]
fn file_object_identity(path: &Path) -> Result<FileObjectIdentity, GitError> {
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
        return Err(GitError::PathIdentity(format!(
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
        Err(GitError::PathIdentity(format!(
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
fn file_object_identity(path: &Path) -> Result<FileObjectIdentity, GitError> {
    Err(GitError::PathIdentity(format!(
        "file object identity is unsupported on this platform: {}",
        path.display()
    )))
}

pub fn repository_info(repo: &Path) -> Result<RepositoryInfo, GitError> {
    let repository_identity = identify_existing(repo)?;
    let repository_path = repository_identity.canonical_path.clone();
    verify_same_identity(
        "repository",
        &repository_identity,
        &identify_existing(&repository_path)?,
    )?;
    let common_dir_identity = resolve_common_dir_identity(&repository_path, "repository identity")?;
    let common_dir = common_dir_identity.canonical_path.clone();
    Ok(RepositoryInfo {
        repository_path,
        common_dir,
        repository_identity,
        common_dir_identity,
    })
}

fn resolve_common_dir_identity(
    repository_path: &Path,
    operation: &str,
) -> Result<ExistingPathIdentity, GitError> {
    let output = git_output(
        repository_path,
        ["rev-parse", "--git-common-dir"],
        operation,
    )?;
    let raw = string_output(&output.stdout)?;
    let common = PathBuf::from(raw.trim());
    let common = if common.is_absolute() {
        common
    } else {
        repository_path.join(common)
    };
    identify_existing(&common)
}

pub fn local_branch_exists(repo: &Path, branch: &str) -> Result<bool, GitError> {
    if branch.trim().is_empty() || branch.starts_with('-') {
        return Err(GitError::SafetyRefused("invalid local branch name".into()));
    }
    let ref_name = format!("refs/heads/{branch}");
    let output = git_command(repo)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(ref_name)
        .output()?;
    Ok(output.status.success())
}

pub fn list_worktrees(repo: &Path) -> Result<Vec<ObservedWorktree>, GitError> {
    let output = git_output(
        repo,
        ["worktree", "list", "--porcelain", "-z"],
        "worktree list",
    )?;
    let mut result = Vec::new();
    let mut current: Option<ObservedWorktree> = None;
    for field in output.stdout.split(|byte| *byte == 0) {
        if field.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(field).map_err(|_| {
            GitError::PathIdentity("Git reported a non-UTF-8 worktree entry".into())
        })?;
        if let Some(path) = text.strip_prefix("worktree ") {
            if let Some(item) = current.take() {
                result.push(item);
            }
            current = Some(ObservedWorktree {
                path: canonicalize_target(Path::new(path))?,
                branch: None,
                head: None,
            });
        } else if let Some(head) = text.strip_prefix("HEAD ") {
            if let Some(item) = current.as_mut() {
                item.head = Some(head.to_owned());
            }
        } else if let Some(branch) = text.strip_prefix("branch refs/heads/")
            && let Some(item) = current.as_mut()
        {
            item.branch = Some(branch.to_owned());
        }
    }
    if let Some(item) = current {
        result.push(item);
    }
    Ok(result)
}

pub fn find_worktree(repo: &Path, path: &Path) -> Result<Option<ObservedWorktree>, GitError> {
    let target = canonicalize_target(path)?;
    let worktrees = list_worktrees(repo)?;
    if let Some(item) = worktrees.iter().find(|item| item.path == target) {
        return Ok(Some(item.clone()));
    }
    for item in worktrees {
        if paths_equivalent(&item.path, &target)? {
            return Ok(Some(item));
        }
    }
    Ok(None)
}

pub fn find_worktree_registration(
    repo: &Path,
    path: &Path,
) -> Result<Option<ObservedWorktree>, GitError> {
    let target = canonicalize_target(path)?;
    Ok(list_worktrees(repo)?
        .into_iter()
        .find(|item| item.path == target))
}

pub fn invoke_worktree_add(repo: &Path, path: &Path, branch: &str) -> GitInvocation {
    GitInvocation {
        operation: "worktree.add".into(),
        output: git_command(repo)
            .args(["worktree", "add"])
            .arg(git_path_argument(path).as_ref())
            .arg(branch)
            .output(),
    }
}

pub fn invoke_worktree_remove(repo: &Path, path: &Path) -> GitInvocation {
    GitInvocation {
        operation: "worktree.remove".into(),
        output: git_command(repo)
            .args(["worktree", "remove"])
            .arg(git_path_argument(path).as_ref())
            .output(),
    }
}

pub fn observe_status(
    repository_path: &str,
    common_dir: &str,
    path: &str,
    registered_branch: &str,
) -> Result<WorktreeStatus, GitError> {
    let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let target = Path::new(path);
    if !path_exists(target)? {
        return Ok(WorktreeStatus {
            registered: true,
            repository_path: Some(repository_path.to_owned()),
            repository_common_dir: Some(common_dir.to_owned()),
            path: Some(path.to_owned()),
            exists: false,
            branch: Some(registered_branch.to_owned()),
            head: None,
            staged: None,
            unstaged: None,
            untracked: None,
            ignored: None,
            observed_at,
        });
    }
    let canonical = canonicalize_existing(target)?;
    let root_output = git_output(
        &canonical,
        ["rev-parse", "--show-toplevel"],
        "worktree identity",
    )?;
    let actual_root = canonicalize_existing(Path::new(string_output(&root_output.stdout)?.trim()))
        .map_err(|error| GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: error.to_string(),
        })?;
    if actual_root != canonical {
        return Err(GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: "the live path is not the root of the registered worktree".into(),
        });
    }
    let actual_repository =
        repository_info(&canonical).map_err(|error| GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: error.to_string(),
        })?;
    let expected_common =
        canonicalize_existing(Path::new(common_dir)).map_err(|error| GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: error.to_string(),
        })?;
    if actual_repository.common_dir != expected_common {
        return Err(GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: "the live Git common directory does not match the registered repository"
                .into(),
        });
    }
    let registered = find_worktree(Path::new(repository_path), &canonical)?.ok_or_else(|| {
        GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: "the path exists but Git does not register it as a worktree".into(),
        }
    })?;
    if registered.branch.as_deref() != Some(registered_branch) {
        return Err(GitError::CommandFailed {
            operation: "worktree identity".into(),
            exit_status: None,
            summary: "the live branch does not match the registered branch".into(),
        });
    }
    let output = git_output(
        &canonical,
        [
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored",
        ],
        "worktree status",
    )?;
    let head_output = git_output(&canonical, ["rev-parse", "HEAD"], "worktree head")?;
    let mut staged = BTreeSet::new();
    let mut unstaged = BTreeSet::new();
    let mut untracked = BTreeSet::new();
    let mut ignored = BTreeSet::new();
    let mut entries = output.stdout.split(|byte| *byte == 0);
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let x = entry[0] as char;
        let y = entry[1] as char;
        let file = std::str::from_utf8(&entry[3..])
            .map_err(|_| GitError::PathIdentity("Git reported a non-UTF-8 status path".into()))?
            .to_owned();
        if x == '?' && y == '?' {
            untracked.insert(file);
        } else if x == '!' && y == '!' {
            ignored.insert(file);
        } else {
            if x != ' ' {
                staged.insert(file.clone());
            }
            if y != ' ' {
                unstaged.insert(file);
            }
        }
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            // In porcelain v1 -z, rename/copy entries contain a second NUL-delimited
            // source path. It has no XY prefix and is not a separate status entry.
            let _ = entries.next();
        }
    }
    Ok(WorktreeStatus {
        registered: true,
        repository_path: Some(repository_path.to_owned()),
        repository_common_dir: Some(common_dir.to_owned()),
        path: Some(path.to_owned()),
        exists: true,
        branch: Some(registered_branch.to_owned()),
        head: Some(string_output(&head_output.stdout)?.trim().to_owned()),
        staged: Some(staged.into_iter().collect()),
        unstaged: Some(unstaged.into_iter().collect()),
        untracked: Some(untracked.into_iter().collect()),
        ignored: Some(ignored.into_iter().collect()),
        observed_at,
    })
}

pub fn empty_worktree_status() -> WorktreeStatus {
    WorktreeStatus {
        registered: false,
        repository_path: None,
        repository_common_dir: None,
        path: None,
        exists: false,
        branch: None,
        head: None,
        staged: None,
        unstaged: None,
        untracked: None,
        ignored: None,
        observed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

fn git_output<const N: usize>(
    cwd: &Path,
    args: [&str; N],
    operation: &str,
) -> Result<Output, GitError> {
    let output = git_command(cwd).args(args).output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitError::CommandFailed {
            operation: operation.to_owned(),
            exit_status: output.status.code(),
            summary: summarize_stderr(&output.stderr),
        })
    }
}

fn git_command(repo: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(git_path_argument(repo).as_ref());
    command
}

#[cfg(windows)]
fn git_path_argument(path: &Path) -> Cow<'_, Path> {
    use std::path::Prefix;

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Cow::Borrowed(path);
    };
    match prefix.kind() {
        Prefix::VerbatimDisk(drive) => {
            let mut simplified = PathBuf::from(format!("{}:", drive as char));
            simplified.extend(components);
            Cow::Owned(simplified)
        }
        Prefix::VerbatimUNC(server, share) => {
            let mut simplified = PathBuf::from(r"\\");
            simplified.push(server);
            simplified.push(share);
            simplified.extend(components);
            Cow::Owned(simplified)
        }
        _ => Cow::Borrowed(path),
    }
}

#[cfg(not(windows))]
fn git_path_argument(path: &Path) -> Cow<'_, Path> {
    Cow::Borrowed(path)
}

fn string_output(bytes: &[u8]) -> Result<&str, GitError> {
    std::str::from_utf8(bytes)
        .map_err(|_| GitError::PathIdentity("Git reported non-UTF-8 path data".into()))
}

fn summarize_stderr(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars()
        .take(2_000)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_paths() {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().unwrap();
        let existing = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"repository-\xff".to_vec()));
        assert!(matches!(
            identify_existing(&existing),
            Err(GitError::PathIdentity(_))
        ));
        let target = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"worktree-\xff".to_vec()));
        assert!(matches!(
            identify_target(&target),
            Err(GitError::PathIdentity(_))
        ));
        assert!(matches!(
            canonicalize_target(&target),
            Err(GitError::PathIdentity(_))
        ));
    }

    #[test]
    fn rejects_non_utf8_git_path_output() {
        assert!(matches!(
            string_output(b"worktree /tmp/path-\xff"),
            Err(GitError::PathIdentity(_))
        ));
    }

    #[test]
    fn canonicalizes_nonexistent_target_from_existing_parent() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("a")).unwrap();
        let target = temp.path().join("a/../b/worktree");
        let canonical = canonicalize_target(&target).unwrap();
        assert_eq!(
            canonical,
            fs::canonicalize(temp.path()).unwrap().join("b/worktree")
        );
    }

    #[test]
    fn exact_missing_unicode_paths_do_not_require_a_comparison_key() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        fs::create_dir(&repository).unwrap();
        git_output(&repository, ["init", "-b", "main"], "test setup").unwrap();
        let missing = temp.path().join("工作树");

        assert!(paths_equivalent(&missing, &missing).unwrap());
        assert!(
            find_worktree_registration(&repository, &missing)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn detects_replaced_target_ancestor_by_file_object_identity() {
        let temp = tempfile::tempdir().unwrap();
        let ancestor = temp.path().join("ancestor");
        fs::create_dir(&ancestor).unwrap();
        let identity = identify_target(&ancestor.join("worktree")).unwrap();

        // Keep the old object alive so the fixture never relies on inode allocation.
        fs::rename(&ancestor, temp.path().join("original-ancestor")).unwrap();
        fs::create_dir(&ancestor).unwrap();

        assert!(matches!(
            verify_target_identity(&identity),
            Err(GitError::PathIdentity(_))
        ));
    }

    #[test]
    fn target_identity_allows_changes_inside_same_ancestor() {
        let temp = tempfile::tempdir().unwrap();
        let identity = identify_target(&temp.path().join("worktree")).unwrap();
        fs::write(temp.path().join("unrelated-file"), "updated").unwrap();
        verify_target_identity(&identity).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn deleted_ancestor_cannot_reuse_a_cloned_identity_inode() {
        let temp = tempfile::tempdir().unwrap();
        let ancestor = temp.path().join("ancestor");
        fs::create_dir(&ancestor).unwrap();
        let original = identify_target(&ancestor.join("worktree")).unwrap();
        let identity = original.clone();
        drop(original);

        for _ in 0..128 {
            fs::remove_dir(&ancestor).unwrap();
            fs::create_dir(&ancestor).unwrap();
            let current = identify_target(&ancestor.join("worktree")).unwrap();
            assert_ne!(identity, current, "replacement reused the pinned inode");
            assert!(matches!(
                verify_target_identity(&identity),
                Err(GitError::PathIdentity(_))
            ));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn identity_handles_are_metadata_only_and_close_on_exec() {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("unreadable");
        fs::write(&path, "private").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let identity = identify_existing(&path).unwrap();
        let fd = identity.object._handle.as_raw_fd();
        // The borrowed descriptor remains owned by identity for both fcntl calls.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(flags >= 0 && flags & libc::O_PATH != 0);
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
    }

    #[test]
    fn detects_replaced_repository_by_file_object_identity() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        fs::create_dir(&repository).unwrap();
        git_output(&repository, ["init", "-b", "main"], "test setup").unwrap();
        let info = repository_info(&repository).unwrap();

        fs::rename(&repository, temp.path().join("original-repository")).unwrap();
        fs::create_dir(&repository).unwrap();
        git_output(&repository, ["init", "-b", "main"], "test setup").unwrap();

        assert!(matches!(
            verify_repository_identity(&info),
            Err(GitError::PathIdentity(_))
        ));
    }

    #[test]
    fn detects_changed_repository_common_dir_association() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        let other_repository = temp.path().join("other-repository");
        let first_common = temp.path().join("first-common");
        let second_common = temp.path().join("second-common");
        for (worktree, common) in [
            (&repository, &first_common),
            (&other_repository, &second_common),
        ] {
            let output = Command::new("git")
                .arg("init")
                .arg("--separate-git-dir")
                .arg(git_path_argument(common).as_ref())
                .arg(git_path_argument(worktree).as_ref())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                summarize_stderr(&output.stderr)
            );
        }
        let info = repository_info(&repository).unwrap();

        fs::write(
            repository.join(".git"),
            format!("gitdir: {}\n", second_common.display()),
        )
        .unwrap();

        assert!(matches!(
            verify_repository_identity(&info),
            Err(GitError::PathIdentity(_))
        ));
        assert_ne!(
            repository_info(&repository).unwrap().common_dir,
            info.common_dir
        );
    }

    #[test]
    fn second_task_lock_is_busy() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("db.sqlite");
        let _first = acquire_worktree_lock(&database, "T-1", Some(temp.path())).unwrap();
        assert!(matches!(
            acquire_worktree_lock(&database, "T-1", Some(temp.path())),
            Err(GitError::OperationBusy)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn converts_verbatim_paths_only_for_git_arguments() {
        assert_eq!(
            git_path_argument(Path::new(r"\\?\C:\repo\worktree")).as_ref(),
            Path::new(r"C:\repo\worktree")
        );
        assert_eq!(
            git_path_argument(Path::new(r"\\?\UNC\server\share\repo")).as_ref(),
            Path::new(r"\\server\share\repo")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_symlink_aliases_to_one_target_identity() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        fs::create_dir(&real).unwrap();
        let alias = temp.path().join("alias");
        symlink(&real, &alias).unwrap();
        assert_eq!(
            canonicalize_target(&alias.join("worktree")).unwrap(),
            canonicalize_target(&real.join("worktree")).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_parent_component_after_following_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("base");
        let other = temp.path().join("other");
        fs::create_dir(&base).unwrap();
        fs::create_dir(&other).unwrap();
        fs::create_dir(other.join("child")).unwrap();
        symlink(other.join("child"), base.join("link")).unwrap();

        assert_eq!(
            canonicalize_target(&base.join("link/../worktree")).unwrap(),
            fs::canonicalize(&other).unwrap().join("worktree")
        );
    }
}
