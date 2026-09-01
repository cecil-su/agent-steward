use std::path::Path;

use rusqlite::{TransactionBehavior, params};
use serde_json::{Value, json};
use storage_sqlite::now;

use crate::db::{check_version, insert_history, load_task};
use crate::{AppError, AppResult, Outcome, Service};

impl Service {
    pub fn worktree_status(&self, task_id: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task = load_task(&connection, task_id)?;
        let status = match (
            task.repository_path.as_deref(),
            task.repository_common_dir.as_deref(),
            task.repository_branch.as_deref(),
            task.worktree_path.as_deref(),
        ) {
            (Some(repository), Some(common), Some(branch), Some(path)) => {
                git_adapter::observe_status(repository, common, path, branch)
                    .map_err(|error| AppError::from_git(error, Some(path)))?
            }
            (None, None, None, None) => git_adapter::empty_worktree_status(),
            _ => {
                return Err(AppError::constraint(
                    "tasks.worktree_references.all_or_none",
                ));
            }
        };
        Ok(Outcome::new(json!({"worktreeStatus": status})))
    }

    pub fn worktree_create(
        &self,
        task_id: &str,
        expected: i64,
        repo: &Path,
        branch: &str,
        path: &Path,
    ) -> AppResult<Outcome> {
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            task_id,
            self.lock_root_override.as_deref(),
        )
        .map_err(|error| lock_error(error, task_id, "create"))?;
        let connection = self.connection()?;
        let task = load_task(&connection, task_id)?;
        check_version(&task, expected)?;
        if task.repository_path.is_some()
            || task.repository_common_dir.is_some()
            || task.repository_branch.is_some()
            || task.worktree_path.is_some()
        {
            return Err(AppError::worktree_safety(
                "task already has registered repository/worktree references",
                task.worktree_path.as_deref(),
            ));
        }
        let repo_info = git_adapter::repository_info(repo)
            .map_err(|error| AppError::from_git(error, repo.to_str()))?;
        if !git_adapter::local_branch_exists(&repo_info.repository_path, branch)
            .map_err(|error| AppError::from_git(error, repo.to_str()))?
        {
            return Err(AppError::worktree_safety(
                "the requested local branch does not exist",
                path.to_str(),
            ));
        }
        let target = git_adapter::canonicalize_target(path)
            .map_err(|error| AppError::from_git(error, path.to_str()))?;
        if target.exists()
            || git_adapter::find_worktree(&repo_info.repository_path, &target)
                .map_err(|error| AppError::from_git(error, path.to_str()))?
                .is_some()
        {
            return Err(AppError::worktree_safety(
                "target path is already present or registered",
                target.to_str(),
            ));
        }
        let invocation =
            git_adapter::invoke_worktree_add(&repo_info.repository_path, &target, branch);
        let observed = git_adapter::find_worktree(&repo_info.repository_path, &target);
        let created = match observed {
            Ok(Some(item)) if item.path == target && item.branch.as_deref() == Some(branch) => item,
            Ok(None) if !target.exists() => {
                return Err(AppError::from_git(
                    invocation.command_error(),
                    target.to_str(),
                ));
            }
            Ok(other) => {
                return Err(AppError::partial(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!({"observed": format!("{other:?}"), "gitExitSuccess": invocation.succeeded()}),
                    "taskctl doctor",
                ));
            }
            Err(_) => {
                return Err(AppError::partial(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    "taskctl doctor",
                ));
            }
        };
        drop(connection);
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| {
                AppError::partial(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!({"created": true, "branch": branch}),
                    &format!(
                        "taskctl worktree adopt {task_id} --repo {} --path {} --if-version {expected}",
                        repo_info.repository_path.display(),
                        target.display()
                    ),
                )
            })?;
        let current = load_task(&tx, task_id).map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                "taskctl doctor",
            )
        })?;
        if current.version != expected || current.worktree_path.is_some() {
            return Err(AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                "taskctl doctor",
            ));
        }
        let changed = tx
            .execute(
                "UPDATE tasks SET repository_path=?3,repository_common_dir=?4,repository_branch=?5,
                    worktree_path=?6,version=version+1,updated_at=?7 WHERE id=?1 AND version=?2",
                params![
                    task_id,
                    expected,
                    repo_info.repository_path.to_string_lossy(),
                    repo_info.common_dir.to_string_lossy(),
                    branch,
                    target.to_string_lossy(),
                    timestamp
                ],
            )
            .map_err(|_| {
                AppError::partial(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!({"created": true, "branch": branch}),
                    "taskctl doctor",
                )
            })?;
        if changed != 1 {
            return Err(AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                "taskctl doctor",
            ));
        }
        insert_history(
            &tx,
            task_id,
            "worktree.created",
            current.current_session_id.as_deref(),
            "worktree created",
            worktree_payload(&repo_info, &target, branch),
            &timestamp,
        )
        .map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                "taskctl doctor",
            )
        })?;
        tx.commit().map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                "taskctl doctor",
            )
        })?;
        let status = git_adapter::observe_status(
            &repo_info.repository_path.to_string_lossy(),
            &repo_info.common_dir.to_string_lossy(),
            &target.to_string_lossy(),
            created.branch.as_deref().unwrap_or(branch),
        )
        .map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!("unknown"),
                "taskctl doctor",
            )
        })?;
        Ok(Outcome::new(
            json!({"task": load_task(&connection, task_id)?, "worktreeStatus": status}),
        ))
    }

    pub fn worktree_remove(&self, task_id: &str, expected: i64) -> AppResult<Outcome> {
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            task_id,
            self.lock_root_override.as_deref(),
        )
        .map_err(|error| lock_error(error, task_id, "remove"))?;
        let connection = self.connection()?;
        let task = load_task(&connection, task_id)?;
        check_version(&task, expected)?;
        let (repo, common, branch, path) = registered_refs(&task)?;
        let info = git_adapter::repository_info(Path::new(repo))
            .map_err(|error| AppError::from_git(error, Some(repo)))?;
        if info.common_dir.to_string_lossy() != common {
            return Err(AppError::worktree_safety(
                "repository identity no longer matches the registered common directory",
                Some(path),
            ));
        }
        let status = git_adapter::observe_status(repo, common, path, branch)
            .map_err(|error| AppError::from_git(error, Some(path)))?;
        if !status.exists {
            return Err(AppError::worktree_safety(
                "registered worktree is already missing; use detach",
                Some(path),
            ));
        }
        if status
            .staged
            .as_ref()
            .is_some_and(|items| !items.is_empty())
            || status
                .unstaged
                .as_ref()
                .is_some_and(|items| !items.is_empty())
            || status
                .untracked
                .as_ref()
                .is_some_and(|items| !items.is_empty())
        {
            return Err(AppError::worktree_safety(
                "worktree contains staged, unstaged, or untracked files",
                Some(path),
            ));
        }
        let target = Path::new(path);
        let invocation = git_adapter::invoke_worktree_remove(Path::new(repo), target);
        let listed = git_adapter::find_worktree(Path::new(repo), target);
        match listed {
            Ok(None) if !target.exists() => {}
            Ok(Some(_)) if target.exists() => {
                return Err(AppError::from_git(invocation.command_error(), Some(path)));
            }
            Ok(_) | Err(_) => {
                return Err(AppError::partial(
                    Some(repo),
                    Some(path),
                    json!("unknown"),
                    "taskctl doctor",
                ));
            }
        }
        drop(connection);
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| {
                AppError::partial(
                    Some(repo),
                    Some(path),
                    json!({"removed": true}),
                    "taskctl doctor",
                )
            })?;
        let current = load_task(&tx, task_id).map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                "taskctl doctor",
            )
        })?;
        if current.version != expected || current.worktree_path.as_deref() != Some(path) {
            return Err(AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                "taskctl doctor",
            ));
        }
        tx.execute(
            "UPDATE tasks SET repository_path=NULL,repository_common_dir=NULL,repository_branch=NULL,
                worktree_path=NULL,version=version+1,updated_at=?3 WHERE id=?1 AND version=?2",
            params![task_id, expected, timestamp],
        )
        .map_err(|_| AppError::partial(Some(repo), Some(path), json!({"removed": true}), "taskctl doctor"))?;
        insert_history(
            &tx,
            task_id,
            "worktree.removed",
            current.current_session_id.as_deref(),
            "worktree removed",
            json!({"repositoryPath":repo,"repositoryCommonDir":common,"worktreePath":path,"branch":branch}),
            &timestamp,
        )
        .map_err(|_| AppError::partial(Some(repo), Some(path), json!({"removed": true}), "taskctl doctor"))?;
        tx.commit().map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                "taskctl doctor",
            )
        })?;
        Ok(Outcome::new(json!({
            "task": load_task(&connection, task_id)?,
            "worktreeStatus": git_adapter::empty_worktree_status(),
        })))
    }

    pub fn worktree_adopt(
        &self,
        task_id: &str,
        expected: i64,
        repo: &Path,
        path: &Path,
    ) -> AppResult<Outcome> {
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            task_id,
            self.lock_root_override.as_deref(),
        )
        .map_err(|error| lock_error(error, task_id, "adopt"))?;
        let info = git_adapter::repository_info(repo)
            .map_err(|error| AppError::from_git(error, repo.to_str()))?;
        let target = git_adapter::canonicalize_existing(path)
            .map_err(|error| AppError::from_git(error, path.to_str()))?;
        let observed = git_adapter::find_worktree(&info.repository_path, &target)
            .map_err(|error| AppError::from_git(error, path.to_str()))?
            .ok_or_else(|| {
                AppError::worktree_safety(
                    "Git does not register the requested worktree",
                    target.to_str(),
                )
            })?;
        let branch = observed.branch.ok_or_else(|| {
            AppError::worktree_safety(
                "detached worktrees cannot be adopted in V0",
                target.to_str(),
            )
        })?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let task = load_task(&tx, task_id)?;
        check_version(&task, expected)?;
        if task.worktree_path.is_some()
            || task.repository_path.is_some()
            || task.repository_common_dir.is_some()
            || task.repository_branch.is_some()
        {
            return Err(AppError::worktree_safety(
                "task already has repository/worktree references",
                task.worktree_path.as_deref(),
            ));
        }
        tx.execute(
            "UPDATE tasks SET repository_path=?3,repository_common_dir=?4,repository_branch=?5,
                worktree_path=?6,version=version+1,updated_at=?7 WHERE id=?1 AND version=?2",
            params![
                task_id,
                expected,
                info.repository_path.to_string_lossy(),
                info.common_dir.to_string_lossy(),
                branch,
                target.to_string_lossy(),
                timestamp
            ],
        )
        .map_err(AppError::from_sqlite)?;
        insert_history(
            &tx,
            task_id,
            "worktree.adopted",
            task.current_session_id.as_deref(),
            "worktree adopted",
            worktree_payload(&info, &target, &branch),
            &timestamp,
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        let status = git_adapter::observe_status(
            &info.repository_path.to_string_lossy(),
            &info.common_dir.to_string_lossy(),
            &target.to_string_lossy(),
            &branch,
        )
        .map_err(|error| AppError::from_git(error, target.to_str()))?;
        Ok(Outcome::new(
            json!({"task":load_task(&connection,task_id)?,"worktreeStatus":status}),
        ))
    }

    pub fn worktree_detach(
        &self,
        task_id: &str,
        expected: i64,
        expected_path: &Path,
    ) -> AppResult<Outcome> {
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            task_id,
            self.lock_root_override.as_deref(),
        )
        .map_err(|error| lock_error(error, task_id, "detach"))?;
        let connection = self.connection()?;
        let task = load_task(&connection, task_id)?;
        check_version(&task, expected)?;
        let (repo, common, branch, registered_path) = registered_refs(&task)?;
        let expected_canonical = git_adapter::canonicalize_target(expected_path)
            .map_err(|error| AppError::from_git(error, expected_path.to_str()))?;
        let registered_canonical = git_adapter::canonicalize_target(Path::new(registered_path))
            .map_err(|error| AppError::from_git(error, Some(registered_path)))?;
        if expected_canonical != registered_canonical {
            return Err(AppError::worktree_safety(
                "expected path does not match the registered path",
                expected_canonical.to_str(),
            ));
        }
        if expected_canonical.exists() {
            return Err(AppError::worktree_safety(
                "worktree path still exists",
                expected_canonical.to_str(),
            ));
        }
        if git_adapter::find_worktree(Path::new(repo), &expected_canonical)
            .map_err(|error| AppError::from_git(error, Some(registered_path)))?
            .is_some()
        {
            return Err(AppError::worktree_safety(
                "Git still registers the worktree",
                expected_canonical.to_str(),
            ));
        }
        drop(connection);
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let current = load_task(&tx, task_id)?;
        check_version(&current, expected)?;
        if current.worktree_path.as_deref() != Some(registered_path) {
            return Err(AppError::worktree_safety(
                "registered path changed before detach",
                current.worktree_path.as_deref(),
            ));
        }
        tx.execute(
            "UPDATE tasks SET repository_path=NULL,repository_common_dir=NULL,repository_branch=NULL,
                worktree_path=NULL,version=version+1,updated_at=?3 WHERE id=?1 AND version=?2",
            params![task_id, expected, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        insert_history(
            &tx,
            task_id,
            "worktree.detached",
            current.current_session_id.as_deref(),
            "stale worktree reference detached",
            json!({"repositoryPath":repo,"repositoryCommonDir":common,"worktreePath":registered_path,"branch":branch}),
            &timestamp,
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({
            "task":load_task(&connection,task_id)?,
            "worktreeStatus":git_adapter::empty_worktree_status(),
        })))
    }
}

fn lock_error(error: git_adapter::GitError, task_id: &str, operation: &str) -> AppError {
    if matches!(error, git_adapter::GitError::OperationBusy) {
        AppError::new(
            "WORKTREE_OPERATION_BUSY",
            "another worktree operation is already running for this task",
            true,
            json!({"taskId":task_id,"operation":operation}),
            4,
        )
    } else {
        AppError::from_git(error, None)
    }
}

fn registered_refs(task: &steward_core::TaskView) -> AppResult<(&str, &str, &str, &str)> {
    match (
        task.repository_path.as_deref(),
        task.repository_common_dir.as_deref(),
        task.repository_branch.as_deref(),
        task.worktree_path.as_deref(),
    ) {
        (Some(repo), Some(common), Some(branch), Some(path)) => Ok((repo, common, branch, path)),
        (None, None, None, None) => Err(AppError::worktree_safety(
            "task has no registered worktree",
            None,
        )),
        _ => Err(AppError::constraint(
            "tasks.worktree_references.all_or_none",
        )),
    }
}

fn worktree_payload(info: &git_adapter::RepositoryInfo, target: &Path, branch: &str) -> Value {
    json!({
        "repositoryPath": info.repository_path.to_string_lossy(),
        "repositoryCommonDir": info.common_dir.to_string_lossy(),
        "worktreePath": target.to_string_lossy(),
        "branch": branch,
    })
}
