use super::*;

/// Lexical validation only: never canonicalize caller-provided HTTP paths here.
/// Network/device prefixes, ADS, and parent traversal are not checkout selections.
pub fn local_worktree_path(path: &Path) -> Result<PathBuf, GitError> {
    use std::path::Component;
    let invalid = || {
        GitError::PathIdentity(
            "expected a local absolute worktree path without parent traversal".into(),
        )
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

/// Only paths advertised by the registered repository are eligible. In particular,
/// do NOT use list_worktrees(), which canonicalizes every advertised path.
pub fn registered_worktree_path(common_dir: &Path, requested: &Path) -> Result<PathBuf, GitError> {
    let key = local_worktree_path(requested)?;
    local_worktree_path(common_dir)?;
    let output = git_output(
        common_dir,
        ["worktree", "list", "--porcelain", "-z"],
        "registered worktree candidates",
    )?;
    for field in output.stdout.split(|byte| *byte == 0) {
        if let Some(bytes) = field.strip_prefix(b"worktree ") {
            let path = Path::new(string_output(bytes)?);
            if local_worktree_path(path).is_ok_and(|candidate| candidate == key) {
                return Ok(path.to_owned());
            }
        }
    }
    Err(GitError::PathIdentity(
        "worktree is not an advertised checkout of the registered repository".into(),
    ))
}

/// One porcelain-v2 observation, including detached and unborn HEADs.
/// Status bytes are evidence only: unchanged status does not prove unchanged file contents.
#[derive(Debug, PartialEq, Eq)]
pub struct ContextGitState {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub has_ignored: bool,
    pub status: Vec<u8>,
}

pub fn context_git_state(info: &RepositoryInfo) -> Result<ContextGitState, GitError> {
    verify_repository_identity(info)?;
    let output = crate::read_process::output(
        git_command(&info.repository_path)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.untrackedCache=false",
                "-c",
                "status.renames=false",
                "status",
                "--porcelain=v2",
                "--branch",
                "--no-ahead-behind",
                "-z",
                "--untracked-files=all",
                "--ignored=matching",
                "--ignore-submodules=none",
            ]),
    )?;
    if !output.status.success() {
        return Err(GitError::CommandFailed {
            operation: "context Git state".into(),
            exit_status: output.status.code(),
            summary: summarize_stderr(&output.stderr),
        });
    }
    let mut oid = None;
    let mut branch = None;
    let mut dirty = false;
    let mut has_ignored = false;
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            oid = Some(string_output(value)?.to_owned());
        } else if let Some(value) = record.strip_prefix(b"# branch.head ") {
            branch = Some(string_output(value)?.to_owned());
        } else if record.starts_with(b"! ") {
            has_ignored = true;
        } else if !record.starts_with(b"# ") {
            dirty = true;
        }
    }
    let oid =
        oid.ok_or_else(|| GitError::PathIdentity("Git status omitted HEAD evidence".into()))?;
    let branch = branch
        .ok_or_else(|| GitError::PathIdentity("Git status omitted branch evidence".into()))?;
    verify_repository_identity(info)?;
    Ok(ContextGitState {
        head: (oid != "(initial)").then_some(oid),
        branch: (branch != "(detached)").then_some(branch),
        dirty,
        has_ignored,
        status: output.stdout,
    })
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
) -> Result<ExistingPathIdentity, GitError> {
    let current = identify_existing(&record.canonical_path)?;
    verify_same_identity("recorded directory", record, &current.record())?;
    Ok(current)
}

impl RepositoryInfo {
    pub fn common_identity(&self) -> &ExistingPathIdentity {
        &self.common_dir_identity
    }
}

pub fn verify_existing_identity(identity: &ExistingPathIdentity) -> Result<(), GitError> {
    verify_same_identity(
        "registered directory",
        identity,
        &identify_existing(&identity.canonical_path)?,
    )
}

/// Resolve a non-bare checkout from any directory within it. No Git writes.
pub fn checkout_info(directory: &Path) -> Result<RepositoryInfo, GitError> {
    let directory = identify_existing(directory)?;
    if !directory.canonical_path.is_dir() {
        return Err(GitError::PathIdentity("expected a directory".into()));
    }
    let output = git_output(
        &directory.canonical_path,
        ["rev-parse", "--show-toplevel"],
        "checkout root",
    )?;
    let root = string_output(&output.stdout)?.trim_end_matches(['\r', '\n']);
    let info = repository_info(Path::new(root))?;
    if !directory.canonical_path.starts_with(&info.repository_path) {
        return Err(GitError::PathIdentity(
            "directory is outside the resolved checkout".into(),
        ));
    }
    verify_repository_identity(&info)?;
    Ok(info)
}

/// Explicit directory-only sources must not silently include a Git checkout.
/// Check markers without relying on localized Git error messages.
pub fn directory_has_git_marker(directory: &Path) -> Result<bool, GitError> {
    let directory = canonicalize_existing(directory)?;
    for ancestor in directory.ancestors() {
        match fs::symlink_metadata(ancestor.join(".git")) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(GitError::Io(error)),
        }
        if ancestor.join("HEAD").try_exists()? && ancestor.join("objects").is_dir() {
            return Ok(true); // Bare repositories are not directory-only sources either.
        }
    }
    Ok(false)
}
