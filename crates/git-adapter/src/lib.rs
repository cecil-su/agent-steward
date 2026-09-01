use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

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
}

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

    pub fn command_error(&self) -> GitError {
        match &self.output {
            Ok(output) => GitError::CommandFailed {
                operation: self.operation.clone(),
                exit_status: output.status.code(),
                summary: summarize_stderr(&output.stderr),
            },
            Err(error) => GitError::CommandFailed {
                operation: self.operation.clone(),
                exit_status: None,
                summary: error.to_string(),
            },
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
    set_private_dir(&lock_root)?;
    let database = if database_path.exists() {
        canonicalize_existing(database_path)?
    } else {
        absolute_clean(database_path)?
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
        .open(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(WorktreeLock { file }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            Err(GitError::OperationBusy)
        }
        Err(error) => Err(GitError::Io(error)),
    }
}

pub fn absolute_clean(path: &Path) -> Result<PathBuf, GitError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut cleaned = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => cleaned.push(prefix.as_os_str()),
            Component::RootDir => cleaned.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => {
                if !cleaned.pop() {
                    return Err(GitError::PathIdentity(format!(
                        "path escapes filesystem root: {}",
                        path.display()
                    )));
                }
            }
            Component::Normal(part) => cleaned.push(part),
        }
    }
    Ok(cleaned)
}

pub fn canonicalize_existing(path: &Path) -> Result<PathBuf, GitError> {
    let absolute = absolute_clean(path)?;
    fs::canonicalize(&absolute)
        .map_err(|error| GitError::PathIdentity(format!("{}: {error}", absolute.display())))
}

pub fn canonicalize_target(path: &Path) -> Result<PathBuf, GitError> {
    let absolute = absolute_clean(path)?;
    if absolute.exists() {
        return canonicalize_existing(&absolute);
    }
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
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
    let mut result = fs::canonicalize(ancestor)?;
    for component in suffix.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}

pub fn repository_info(repo: &Path) -> Result<RepositoryInfo, GitError> {
    let repository_path = canonicalize_existing(repo)?;
    let output = git_output(
        &repository_path,
        ["rev-parse", "--git-common-dir"],
        "repository identity",
    )?;
    let raw = string_output(&output.stdout);
    let common = PathBuf::from(raw.trim());
    let common = if common.is_absolute() {
        common
    } else {
        repository_path.join(common)
    };
    let common_dir = canonicalize_existing(&common)?;
    Ok(RepositoryInfo {
        repository_path,
        common_dir,
    })
}

pub fn local_branch_exists(repo: &Path, branch: &str) -> Result<bool, GitError> {
    if branch.trim().is_empty() || branch.starts_with('-') {
        return Err(GitError::SafetyRefused("invalid local branch name".into()));
    }
    let ref_name = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
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
        let text = String::from_utf8_lossy(field);
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
    Ok(list_worktrees(repo)?
        .into_iter()
        .find(|item| item.path == target))
}

pub fn invoke_worktree_add(repo: &Path, path: &Path, branch: &str) -> GitInvocation {
    GitInvocation {
        operation: "worktree.add".into(),
        output: Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["worktree", "add"])
            .arg(path)
            .arg(branch)
            .output(),
    }
}

pub fn invoke_worktree_remove(repo: &Path, path: &Path) -> GitInvocation {
    GitInvocation {
        operation: "worktree.remove".into(),
        output: Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["worktree", "remove"])
            .arg(path)
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
    if !target.exists() {
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
            observed_at,
        });
    }
    let canonical = canonicalize_existing(target)?;
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
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        "worktree status",
    )?;
    let head_output = git_output(&canonical, ["rev-parse", "HEAD"], "worktree head")?;
    let mut staged = BTreeSet::new();
    let mut unstaged = BTreeSet::new();
    let mut untracked = BTreeSet::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.len() < 4 {
            continue;
        }
        let x = entry[0] as char;
        let y = entry[1] as char;
        let file = String::from_utf8_lossy(&entry[3..]).into_owned();
        if x == '?' && y == '?' {
            untracked.insert(file);
        } else {
            if x != ' ' {
                staged.insert(file.clone());
            }
            if y != ' ' {
                unstaged.insert(file);
            }
        }
    }
    Ok(WorktreeStatus {
        registered: true,
        repository_path: Some(repository_path.to_owned()),
        repository_common_dir: Some(common_dir.to_owned()),
        path: Some(path.to_owned()),
        exists: true,
        branch: Some(registered_branch.to_owned()),
        head: Some(string_output(&head_output.stdout).trim().to_owned()),
        staged: Some(staged.into_iter().collect()),
        unstaged: Some(unstaged.into_iter().collect()),
        untracked: Some(untracked.into_iter().collect()),
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
        observed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

fn git_output<const N: usize>(
    cwd: &Path,
    args: [&str; N],
    operation: &str,
) -> Result<Output, GitError> {
    let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;
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

fn string_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn summarize_stderr(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars()
        .take(2_000)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_dir(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_nonexistent_target_from_existing_parent() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("a/../b/worktree");
        let canonical = canonicalize_target(&target).unwrap();
        assert_eq!(
            canonical,
            fs::canonicalize(temp.path()).unwrap().join("b/worktree")
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
}
