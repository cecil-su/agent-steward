use std::path::Path;

use rusqlite::params;
use serde_json::{Value, json};
use storage_sqlite::now;

use crate::db::{
    check_version, insert_history, load_task, load_task_by_reference, resolve_task_id,
};
use crate::{AppError, AppResult, Outcome, PartialDatabaseState, RecoveryCommand, Service};

impl Service {
    pub fn worktree_status(&self, task_reference: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task = load_task_by_reference(&connection, task_reference)?;
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
        task_reference: &str,
        expected: i64,
        repo: &Path,
        branch: &str,
        path: &Path,
    ) -> AppResult<Outcome> {
        let lookup = self.connection()?;
        let task_id = resolve_task_id(&lookup, task_reference)?;
        drop(lookup);
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            &task_id.to_string(),
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
        let target_identity = git_adapter::identify_target(path)
            .map_err(|error| AppError::from_git(error, path.to_str()))?;
        let target = target_identity.canonical_path.clone();
        git_adapter::worktree_path_key(&target)
            .map_err(|error| AppError::from_git(error, target.to_str()))?;
        if let Some(owner) = worktree_path_owner(&connection, &target)? {
            return Err(AppError::worktree_safety(
                format!("worktree path is already registered by task #{owner}"),
                target.to_str(),
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
        if git_adapter::path_exists(&target)
            .map_err(|error| AppError::from_git(error, path.to_str()))?
            || git_adapter::find_worktree(&repo_info.repository_path, &target)
                .map_err(|error| AppError::from_git(error, path.to_str()))?
                .is_some()
        {
            return Err(AppError::worktree_safety(
                "target path is already present or registered",
                target.to_str(),
            ));
        }
        git_adapter::verify_repository_identity(&repo_info)
            .map_err(|error| AppError::from_git(error, repo.to_str()))?;
        git_adapter::verify_target_identity(&target_identity)
            .map_err(|error| AppError::from_git(error, path.to_str()))?;
        let invocation =
            git_adapter::invoke_worktree_add(&repo_info.repository_path, &target, branch);
        let observed = git_adapter::find_worktree(&repo_info.repository_path, &target);
        let target_exists = git_adapter::path_exists(&target);
        let created = match (observed, target_exists) {
            (Ok(Some(item)), Ok(true))
                if item.path == target && item.branch.as_deref() == Some(branch) =>
            {
                item
            }
            (Ok(None), Ok(false)) => {
                if !invocation.succeeded()
                    && git_adapter::verify_repository_identity(&repo_info).is_ok()
                    && git_adapter::verify_target_identity(&target_identity).is_ok()
                {
                    return Err(AppError::from_git(
                        invocation.command_error(),
                        target.to_str(),
                    ));
                }
                return Err(AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    observed_git_state(None, false),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    git_invocation_diagnostics(&invocation),
                ));
            }
            (Ok(other), Ok(path_exists)) => {
                return Err(AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    observed_git_state(other.as_ref(), path_exists),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    git_invocation_diagnostics(&invocation),
                ));
            }
            (observed, path_exists) => {
                return Err(AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({
                        "observationError": post_git_observation_error(&observed, &path_exists),
                        "gitInvocation": git_invocation_diagnostics(&invocation),
                    }),
                ));
            }
        };
        if let Err(error) = git_adapter::verify_repository_identity(&repo_info) {
            return Err(AppError::partial_with_diagnostics(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!("unknown"),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
                json!({
                    "phase": "beforeDatabaseCommit",
                    "observationError": error.to_string(),
                    "gitInvocation": git_invocation_diagnostics(&invocation),
                }),
            ));
        }
        match git_adapter::observe_status(
            &repo_info.repository_path.to_string_lossy(),
            &repo_info.common_dir.to_string_lossy(),
            &target.to_string_lossy(),
            branch,
        ) {
            Ok(status) if status.exists => {}
            Ok(status) => {
                return Err(AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    observed_git_state(Some(&created), status.exists),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({
                        "phase": "beforeDatabaseCommit",
                        "observedStatus": status,
                        "gitInvocation": git_invocation_diagnostics(&invocation),
                    }),
                ));
            }
            Err(error) => {
                return Err(AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({
                        "phase": "beforeDatabaseCommit",
                        "observationError": error.to_string(),
                        "gitInvocation": git_invocation_diagnostics(&invocation),
                    }),
                ));
            }
        }
        drop(connection);
        let timestamp = now();
        let mut connection = self.connection().map_err(|error| {
            created_worktree_database_failure(
                self,
                task_id,
                &repo_info.repository_path,
                &created,
                expected,
                "databaseReconnect",
                json!(error.body),
            )
        })?;
        let tx = storage_sqlite::write_transaction(&mut connection).map_err(|error| {
            let error = AppError::from_storage(error);
            created_worktree_database_failure(
                self,
                task_id,
                &repo_info.repository_path,
                &created,
                expected,
                "databaseTransaction",
                json!(error.body),
            )
        })?;
        let current = load_task(&tx, task_id).map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            )
        })?;
        if current.version != expected || current.worktree_path.is_some() {
            return Err(AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unchanged,
                adopt_recovery_for_created_worktree(
                    self,
                    task_id,
                    &repo_info.repository_path,
                    &repo_info.common_dir,
                    &target,
                ),
            ));
        }
        let target_key = git_adapter::worktree_path_key(&target).map_err(|error| {
            AppError::partial_with_diagnostics(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
                json!({"phase": "databaseTransaction", "pathKeyError": error.to_string()}),
            )
        })?;
        if !matches!(worktree_path_owner(&tx, &target), Ok(None)) {
            return Err(AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            ));
        }
        let changed = tx
            .execute(
                "UPDATE tasks SET repository_path=?3,repository_common_dir=?4,repository_branch=?5,
                    worktree_path=?6,worktree_path_key=?7,version=version+1,updated_at=?8
                 WHERE id=?1 AND version=?2",
                params![
                    task_id,
                    expected,
                    repo_info
                        .repository_path
                        .to_str()
                        .expect("validated UTF-8 path"),
                    repo_info.common_dir.to_str().expect("validated UTF-8 path"),
                    branch,
                    target.to_str().expect("validated UTF-8 path"),
                    target_key,
                    timestamp
                ],
            )
            .map_err(|_| {
                AppError::partial(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!({"created": true, "branch": branch}),
                    PartialDatabaseState::Unchanged,
                    adopt_recovery_for_created_worktree(
                        self,
                        task_id,
                        &repo_info.repository_path,
                        &repo_info.common_dir,
                        &target,
                    ),
                )
            })?;
        if changed != 1 {
            return Err(AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unchanged,
                adopt_recovery_for_created_worktree(
                    self,
                    task_id,
                    &repo_info.repository_path,
                    &repo_info.common_dir,
                    &target,
                ),
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
                PartialDatabaseState::Unchanged,
                adopt_recovery_for_created_worktree(
                    self,
                    task_id,
                    &repo_info.repository_path,
                    &repo_info.common_dir,
                    &target,
                ),
            )
        })?;
        let response_task = load_task(&tx, task_id).map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            )
        })?;
        tx.commit().map_err(|_| {
            AppError::partial(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!({"created": true, "branch": branch}),
                PartialDatabaseState::Unknown,
                self.recovery_command(["doctor"]),
            )
        })?;
        let status = git_adapter::observe_status(
            &repo_info.repository_path.to_string_lossy(),
            &repo_info.common_dir.to_string_lossy(),
            &target.to_string_lossy(),
            created.branch.as_deref().unwrap_or(branch),
        )
        .map_err(|error| {
            AppError::partial_with_diagnostics(
                repo_info.repository_path.to_str(),
                target.to_str(),
                json!("unknown"),
                PartialDatabaseState::Updated,
                self.recovery_command(["doctor"]),
                json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
            )
        })?;
        if !status.exists {
            let registered_by_git = git_adapter::find_worktree_registration(
                &repo_info.repository_path,
                &target,
            )
            .map_err(|error| {
                AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
            return Err(if registered_by_git.is_some() {
                AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!({"pathExists": false, "registeredByGit": true}),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({
                        "phase": "afterDatabaseCommit",
                        "message": "the worktree directory is missing but Git still registers it; prune the stale Git registration or restore the directory before detaching"
                    }),
                )
            } else {
                AppError::partial_with_diagnostics(
                    repo_info.repository_path.to_str(),
                    target.to_str(),
                    json!({"pathExists": false, "registeredByGit": false}),
                    PartialDatabaseState::Updated,
                    detach_recovery_after_update(
                        self,
                        task_id,
                        &repo_info.repository_path,
                        &repo_info.common_dir,
                        branch,
                        &target,
                    ),
                    json!({"phase": "afterDatabaseCommit"}),
                )
            });
        }
        Ok(Outcome::new(
            json!({"task": response_task, "worktreeStatus": status}),
        ))
    }

    pub fn worktree_remove(&self, task_reference: &str, expected: i64) -> AppResult<Outcome> {
        let lookup = self.connection()?;
        let task_id = resolve_task_id(&lookup, task_reference)?;
        drop(lookup);
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            &task_id.to_string(),
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
            let listed =
                git_adapter::find_worktree_registration(&info.repository_path, Path::new(path))
                    .map_err(|error| AppError::from_git(error, Some(path)))?;
            git_adapter::verify_repository_identity(&info)
                .map_err(|error| AppError::from_git(error, Some(repo)))?;
            if listed.is_some() {
                return Err(AppError::worktree_safety(
                    "worktree directory is missing but Git still registers it; restore the directory or clean the stale Git registration, then run taskctl doctor",
                    Some(path),
                ));
            }
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
            || status
                .ignored
                .as_ref()
                .is_some_and(|items| !items.is_empty())
        {
            return Err(AppError::worktree_safety(
                "worktree contains staged, unstaged, untracked, or ignored files",
                Some(path),
            ));
        }
        let target_identity = git_adapter::identify_target(Path::new(path))
            .map_err(|error| AppError::from_git(error, Some(path)))?;
        let target = target_identity.canonical_path.as_path();
        git_adapter::verify_repository_identity(&info)
            .map_err(|error| AppError::from_git(error, Some(repo)))?;
        git_adapter::verify_target_identity(&target_identity)
            .map_err(|error| AppError::from_git(error, Some(path)))?;
        let invocation = git_adapter::invoke_worktree_remove(&info.repository_path, target);
        let listed = git_adapter::find_worktree_registration(Path::new(repo), target);
        let target_exists = git_adapter::path_exists(target);
        match (listed, target_exists) {
            (Ok(None), Ok(false)) => {}
            (Ok(Some(item)), Ok(true)) => {
                let post_failure_status = if invocation.succeeded() {
                    None
                } else {
                    Some(git_adapter::observe_status(repo, common, path, branch))
                };
                let unchanged = post_failure_status.as_ref().is_some_and(|result| {
                    result.as_ref().is_ok_and(|after| {
                        git_adapter::verify_repository_identity(&info).is_ok()
                            && git_adapter::verify_target_identity(&target_identity).is_ok()
                            && worktree_status_state_eq(&status, after)
                    })
                });
                if !invocation.succeeded() && unchanged {
                    return Err(AppError::from_git(invocation.command_error(), Some(path)));
                }
                return Err(AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(path),
                    observed_git_state(Some(&item), true),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    removal_failure_diagnostics(&invocation, post_failure_status.as_ref()),
                ));
            }
            (Ok(other), Ok(path_exists)) => {
                return Err(AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(path),
                    observed_git_state(other.as_ref(), path_exists),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    git_invocation_diagnostics(&invocation),
                ));
            }
            (listed, path_exists) => {
                return Err(AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(path),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({
                        "observationError": post_git_observation_error(&listed, &path_exists),
                        "gitInvocation": git_invocation_diagnostics(&invocation),
                    }),
                ));
            }
        }
        if let Err(error) = git_adapter::verify_repository_identity(&info) {
            return Err(AppError::partial_with_diagnostics(
                Some(repo),
                Some(path),
                json!("unknown"),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
                json!({
                    "phase": "beforeDatabaseCommit",
                    "observationError": error.to_string(),
                    "gitInvocation": git_invocation_diagnostics(&invocation),
                }),
            ));
        }
        drop(connection);
        let timestamp = now();
        let mut connection = self.connection().map_err(|error| {
            removed_worktree_database_failure(
                self,
                task_id,
                Path::new(repo),
                Path::new(path),
                expected,
                "databaseReconnect",
                json!(error.body),
            )
        })?;
        let tx = storage_sqlite::write_transaction(&mut connection).map_err(|error| {
            let error = AppError::from_storage(error);
            removed_worktree_database_failure(
                self,
                task_id,
                Path::new(repo),
                Path::new(path),
                expected,
                "databaseTransaction",
                json!(error.body),
            )
        })?;
        let current = load_task(&tx, task_id).map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            )
        })?;
        if current.version != expected || current.worktree_path.as_deref() != Some(path) {
            return Err(AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            ));
        }
        tx.execute(
            "UPDATE tasks SET repository_path=NULL,repository_common_dir=NULL,repository_branch=NULL,
                worktree_path=NULL,worktree_path_key=NULL,version=version+1,updated_at=?3
             WHERE id=?1 AND version=?2",
            params![task_id, expected, timestamp],
        )
        .map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            )
        })?;
        insert_history(
            &tx,
            task_id,
            "worktree.removed",
            current.current_session_id.as_deref(),
            "worktree removed",
            json!({"repositoryPath":repo,"repositoryCommonDir":common,"worktreePath":path,"branch":branch}),
            &timestamp,
        )
        .map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            )
        })?;
        let response_task = load_task(&tx, task_id).map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
            )
        })?;
        tx.commit().map_err(|_| {
            AppError::partial(
                Some(repo),
                Some(path),
                json!({"removed": true}),
                PartialDatabaseState::Unknown,
                self.recovery_command(["doctor"]),
            )
        })?;
        let listed_after_commit = git_adapter::find_worktree_registration(Path::new(repo), target)
            .map_err(|error| {
                AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(path),
                    json!("unknown"),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
        let target_exists = git_adapter::path_exists(target).map_err(|error| {
            AppError::partial_with_diagnostics(
                Some(repo),
                Some(path),
                json!("unknown"),
                PartialDatabaseState::Updated,
                self.recovery_command(["doctor"]),
                json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
            )
        })?;
        if target_exists || listed_after_commit.is_some() {
            let recommendation = validated_adopt_recovery(
                self,
                task_id,
                Path::new(repo),
                Path::new(common),
                target,
                listed_after_commit.as_ref(),
            );
            return Err(AppError::partial_with_diagnostics(
                Some(repo),
                Some(path),
                json!({
                    "pathExists": target_exists,
                    "registeredByGit": listed_after_commit.is_some(),
                }),
                PartialDatabaseState::Updated,
                recommendation,
                json!({
                    "phase": "afterDatabaseCommit",
                    "observed": format!("{listed_after_commit:?}"),
                }),
            ));
        }
        Ok(Outcome::new(json!({
            "task": response_task,
            "worktreeStatus": git_adapter::empty_worktree_status(),
        })))
    }

    pub fn worktree_adopt(
        &self,
        task_reference: &str,
        expected: i64,
        repo: &Path,
        path: &Path,
    ) -> AppResult<Outcome> {
        let lookup = self.connection()?;
        let task_id = resolve_task_id(&lookup, task_reference)?;
        drop(lookup);
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            &task_id.to_string(),
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
        let connection = self.connection()?;
        let task = load_task(&connection, task_id)?;
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
        if let Some(owner) = worktree_path_owner(&connection, &target)? {
            return Err(AppError::worktree_safety(
                format!("worktree path is already registered by task #{owner}"),
                target.to_str(),
            ));
        }
        drop(connection);
        match git_adapter::observe_status(
            &info.repository_path.to_string_lossy(),
            &info.common_dir.to_string_lossy(),
            &target.to_string_lossy(),
            &branch,
        ) {
            Ok(status) if status.exists => {}
            Ok(status) => {
                return Err(AppError::partial_with_diagnostics(
                    info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "beforeDatabaseCommit", "observed": status}),
                ));
            }
            Err(error) => {
                return Err(AppError::partial_with_diagnostics(
                    info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "beforeDatabaseCommit", "observationError": error.to_string()}),
                ));
            }
        }
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let current = load_task(&tx, task_id)?;
        check_version(&current, expected)?;
        if current.worktree_path.is_some()
            || current.repository_path.is_some()
            || current.repository_common_dir.is_some()
            || current.repository_branch.is_some()
        {
            return Err(AppError::worktree_safety(
                "task already has repository/worktree references",
                current.worktree_path.as_deref(),
            ));
        }
        let target_key = git_adapter::worktree_path_key(&target)
            .map_err(|error| AppError::from_git(error, target.to_str()))?;
        if let Some(owner) = worktree_path_owner(&tx, &target)? {
            return Err(AppError::worktree_safety(
                format!("worktree path is already registered by task #{owner}"),
                target.to_str(),
            ));
        }
        let changed = tx
            .execute(
                "UPDATE tasks SET repository_path=?3,repository_common_dir=?4,repository_branch=?5,
                    worktree_path=?6,worktree_path_key=?7,version=version+1,updated_at=?8
                 WHERE id=?1 AND version=?2",
                params![
                    task_id,
                    expected,
                    info.repository_path.to_str().expect("validated UTF-8 path"),
                    info.common_dir.to_str().expect("validated UTF-8 path"),
                    branch,
                    target.to_str().expect("validated UTF-8 path"),
                    target_key,
                    timestamp
                ],
            )
            .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(
                expected,
                load_task(&tx, task_id)?.version,
            ));
        }
        insert_history(
            &tx,
            task_id,
            "worktree.adopted",
            current.current_session_id.as_deref(),
            "worktree adopted",
            worktree_payload(&info, &target, &branch),
            &timestamp,
        )?;
        let response_task = load_task(&tx, task_id)?;
        tx.commit().map_err(|error| {
            database_commit_unknown(
                self,
                info.repository_path.to_str(),
                target.to_str(),
                json!({
                    "pathExists": true,
                    "registeredByGit": true,
                    "branch": branch,
                }),
                &error,
            )
        })?;
        let status = git_adapter::observe_status(
            &info.repository_path.to_string_lossy(),
            &info.common_dir.to_string_lossy(),
            &target.to_string_lossy(),
            &branch,
        )
        .map_err(|error| {
            AppError::partial_with_diagnostics(
                info.repository_path.to_str(),
                target.to_str(),
                json!("unknown"),
                PartialDatabaseState::Updated,
                self.recovery_command(["doctor"]),
                json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
            )
        })?;
        if !status.exists {
            let registered_by_git = git_adapter::find_worktree_registration(
                &info.repository_path,
                &target,
            )
            .map_err(|error| {
                AppError::partial_with_diagnostics(
                    info.repository_path.to_str(),
                    target.to_str(),
                    json!("unknown"),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
            return Err(if registered_by_git.is_some() {
                AppError::partial_with_diagnostics(
                    info.repository_path.to_str(),
                    target.to_str(),
                    json!({"pathExists": false, "registeredByGit": true}),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({
                        "phase": "afterDatabaseCommit",
                        "message": "the worktree directory is missing but Git still registers it; prune the stale Git registration or restore the directory before detaching"
                    }),
                )
            } else {
                AppError::partial_with_diagnostics(
                    info.repository_path.to_str(),
                    target.to_str(),
                    json!({"pathExists": false, "registeredByGit": false}),
                    PartialDatabaseState::Updated,
                    detach_recovery_after_update(
                        self,
                        task_id,
                        &info.repository_path,
                        &info.common_dir,
                        &branch,
                        &target,
                    ),
                    json!({"phase": "afterDatabaseCommit"}),
                )
            });
        }
        Ok(Outcome::new(
            json!({"task":response_task,"worktreeStatus":status}),
        ))
    }

    pub fn worktree_detach(
        &self,
        task_reference: &str,
        expected: i64,
        expected_path: &Path,
    ) -> AppResult<Outcome> {
        let lookup = self.connection()?;
        let task_id = resolve_task_id(&lookup, task_reference)?;
        drop(lookup);
        let _lock = git_adapter::acquire_worktree_lock(
            &self.database_path,
            &task_id.to_string(),
            self.lock_root_override.as_deref(),
        )
        .map_err(|error| lock_error(error, task_id, "detach"))?;
        let connection = self.connection()?;
        let task = load_task(&connection, task_id)?;
        check_version(&task, expected)?;
        let (repo, common, branch, registered_path) = registered_refs(&task)?;
        let info = git_adapter::repository_info(Path::new(repo))
            .map_err(|error| AppError::from_git(error, Some(repo)))?;
        if info.common_dir.to_string_lossy() != common {
            return Err(AppError::worktree_safety(
                "repository identity no longer matches the registered common directory",
                Some(registered_path),
            ));
        }
        let expected_canonical = git_adapter::canonicalize_target(expected_path)
            .map_err(|error| AppError::from_git(error, expected_path.to_str()))?;
        let registered_canonical = git_adapter::canonicalize_target(Path::new(registered_path))
            .map_err(|error| AppError::from_git(error, Some(registered_path)))?;
        if !git_adapter::paths_equivalent(&expected_canonical, &registered_canonical)
            .map_err(|error| AppError::from_git(error, expected_canonical.to_str()))?
        {
            return Err(AppError::worktree_safety(
                "expected path does not match the registered path",
                expected_canonical.to_str(),
            ));
        }
        if git_adapter::path_exists(&registered_canonical)
            .map_err(|error| AppError::from_git(error, Some(registered_path)))?
        {
            return Err(AppError::worktree_safety(
                "worktree path still exists",
                registered_canonical.to_str(),
            ));
        }
        if git_adapter::find_worktree_registration(&info.repository_path, &registered_canonical)
            .map_err(|error| AppError::from_git(error, Some(registered_path)))?
            .is_some()
        {
            return Err(AppError::worktree_safety(
                "Git still registers the worktree",
                registered_canonical.to_str(),
            ));
        }
        drop(connection);
        git_adapter::verify_repository_identity(&info)
            .map_err(|error| AppError::from_git(error, Some(repo)))?;
        let listed_before_commit =
            git_adapter::find_worktree_registration(&info.repository_path, &registered_canonical)
                .map_err(|error| {
                AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(registered_path),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "beforeDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
        let path_exists_before_commit =
            git_adapter::path_exists(&registered_canonical).map_err(|error| {
                AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(registered_path),
                    json!("unknown"),
                    PartialDatabaseState::Unchanged,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "beforeDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
        if path_exists_before_commit || listed_before_commit.is_some() {
            return Err(AppError::partial_with_diagnostics(
                Some(repo),
                Some(registered_path),
                json!({
                    "pathExists": path_exists_before_commit,
                    "registeredByGit": listed_before_commit.is_some(),
                }),
                PartialDatabaseState::Unchanged,
                self.recovery_command(["doctor"]),
                json!({
                    "phase": "beforeDatabaseCommit",
                    "observed": format!("{listed_before_commit:?}"),
                }),
            ));
        }
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let current = load_task(&tx, task_id)?;
        check_version(&current, expected)?;
        if current.worktree_path.as_deref() != Some(registered_path) {
            return Err(AppError::worktree_safety(
                "registered path changed before detach",
                current.worktree_path.as_deref(),
            ));
        }
        let changed = tx.execute(
            "UPDATE tasks SET repository_path=NULL,repository_common_dir=NULL,repository_branch=NULL,
                worktree_path=NULL,worktree_path_key=NULL,version=version+1,updated_at=?3
             WHERE id=?1 AND version=?2",
            params![task_id, expected, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(
                expected,
                load_task(&tx, task_id)?.version,
            ));
        }
        insert_history(
            &tx,
            task_id,
            "worktree.detached",
            current.current_session_id.as_deref(),
            "stale worktree reference detached",
            json!({"repositoryPath":repo,"repositoryCommonDir":common,"worktreePath":registered_path,"branch":branch}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, task_id)?;
        tx.commit().map_err(|error| {
            database_commit_unknown(
                self,
                Some(repo),
                Some(registered_path),
                json!({"pathExists": false, "registeredByGit": false}),
                &error,
            )
        })?;
        let listed_after_commit =
            git_adapter::find_worktree_registration(&info.repository_path, &registered_canonical)
                .map_err(|error| {
                AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(registered_path),
                    json!("unknown"),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
        let path_exists_after_commit =
            git_adapter::path_exists(&registered_canonical).map_err(|error| {
                AppError::partial_with_diagnostics(
                    Some(repo),
                    Some(registered_path),
                    json!("unknown"),
                    PartialDatabaseState::Updated,
                    self.recovery_command(["doctor"]),
                    json!({"phase": "afterDatabaseCommit", "observationError": error.to_string()}),
                )
            })?;
        if path_exists_after_commit || listed_after_commit.is_some() {
            let recommendation = validated_adopt_recovery(
                self,
                task_id,
                Path::new(repo),
                Path::new(common),
                &registered_canonical,
                listed_after_commit.as_ref(),
            );
            return Err(AppError::partial_with_diagnostics(
                Some(repo),
                Some(registered_path),
                json!({
                    "pathExists": path_exists_after_commit,
                    "registeredByGit": listed_after_commit.is_some(),
                }),
                PartialDatabaseState::Updated,
                recommendation,
                json!({
                    "phase": "afterDatabaseCommit",
                    "observed": format!("{listed_after_commit:?}"),
                }),
            ));
        }
        Ok(Outcome::new(json!({
            "task":response_task,
            "worktreeStatus":git_adapter::empty_worktree_status(),
        })))
    }
}

fn lock_error(error: git_adapter::GitError, task_id: i64, operation: &str) -> AppError {
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

fn created_worktree_database_failure(
    service: &Service,
    task_id: i64,
    repository: &Path,
    created: &git_adapter::ObservedWorktree,
    expected: i64,
    phase: &str,
    database_error: Value,
) -> AppError {
    AppError::partial_with_diagnostics(
        repository.to_str(),
        created.path.to_str(),
        observed_git_state(Some(created), true),
        PartialDatabaseState::Unchanged,
        adopt_recovery_command(service, task_id, repository, &created.path, expected),
        json!({
            "phase": phase,
            "databaseError": database_error,
        }),
    )
}

fn removed_worktree_database_failure(
    service: &Service,
    task_id: i64,
    repository: &Path,
    worktree: &Path,
    expected: i64,
    phase: &str,
    database_error: Value,
) -> AppError {
    AppError::partial_with_diagnostics(
        repository.to_str(),
        worktree.to_str(),
        observed_git_state(None, false),
        PartialDatabaseState::Unchanged,
        detach_recovery_command(service, task_id, worktree, expected),
        json!({
            "phase": phase,
            "databaseError": database_error,
        }),
    )
}

fn adopt_recovery_command(
    service: &Service,
    task_id: i64,
    repository: &Path,
    worktree: &Path,
    expected: i64,
) -> RecoveryCommand {
    service.recovery_command(vec![
        "worktree".to_owned(),
        "adopt".to_owned(),
        task_id.to_string(),
        "--repo".to_owned(),
        repository.to_string_lossy().into_owned(),
        "--path".to_owned(),
        worktree.to_string_lossy().into_owned(),
        "--if-version".to_owned(),
        expected.to_string(),
    ])
}

fn detach_recovery_command(
    service: &Service,
    task_id: i64,
    worktree: &Path,
    expected: i64,
) -> RecoveryCommand {
    service.recovery_command(vec![
        "worktree".to_owned(),
        "detach".to_owned(),
        task_id.to_string(),
        "--expected-path".to_owned(),
        worktree.to_string_lossy().into_owned(),
        "--if-version".to_owned(),
        expected.to_string(),
    ])
}

fn adopt_recovery_for_created_worktree(
    service: &Service,
    task_id: i64,
    repository: &Path,
    common_dir: &Path,
    worktree: &Path,
) -> RecoveryCommand {
    let Ok(listed) = git_adapter::find_worktree(repository, worktree) else {
        return service.recovery_command(["doctor"]);
    };
    validated_adopt_recovery(
        service,
        task_id,
        repository,
        common_dir,
        worktree,
        listed.as_ref(),
    )
}

fn task_without_worktree_references(task: &steward_core::TaskView) -> bool {
    task.repository_path.is_none()
        && task.repository_common_dir.is_none()
        && task.repository_branch.is_none()
        && task.worktree_path.is_none()
}

fn worktree_path_owner(
    connection: &rusqlite::Connection,
    worktree: &Path,
) -> AppResult<Option<i64>> {
    let registered = {
        let mut statement = connection
            .prepare(
                "SELECT id,worktree_path FROM tasks
                 WHERE worktree_path IS NOT NULL ORDER BY id",
            )
            .map_err(AppError::from_sqlite)?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?
    };
    for (task_id, registered_path) in registered {
        if git_adapter::paths_equivalent(Path::new(&registered_path), worktree)
            .map_err(|error| AppError::from_git(error, worktree.to_str()))?
        {
            return Ok(Some(task_id));
        }
    }
    Ok(None)
}

fn detach_recovery_after_update(
    service: &Service,
    task_id: i64,
    repository: &Path,
    common_dir: &Path,
    branch: &str,
    worktree: &Path,
) -> RecoveryCommand {
    let task = service
        .connection()
        .ok()
        .and_then(|connection| load_task(&connection, task_id).ok());
    let references_match = task.as_ref().is_some_and(|task| {
        task.repository_path.as_deref() == repository.to_str()
            && task.repository_common_dir.as_deref() == common_dir.to_str()
            && task.repository_branch.as_deref() == Some(branch)
            && task.worktree_path.as_deref().is_some_and(|path| {
                git_adapter::paths_equivalent(Path::new(path), worktree).unwrap_or(false)
            })
    });
    match task.filter(|_| references_match) {
        Some(task) => service.recovery_command(vec![
            "worktree".to_owned(),
            "detach".to_owned(),
            task_id.to_string(),
            "--expected-path".to_owned(),
            worktree.to_string_lossy().into_owned(),
            "--if-version".to_owned(),
            task.version.to_string(),
        ]),
        None => service.recovery_command(["doctor"]),
    }
}

fn validated_adopt_recovery(
    service: &Service,
    task_id: i64,
    repository: &Path,
    common_dir: &Path,
    worktree: &Path,
    listed: Option<&git_adapter::ObservedWorktree>,
) -> RecoveryCommand {
    let Some(branch) = listed.and_then(|item| item.branch.as_deref()) else {
        return service.recovery_command(["doctor"]);
    };
    if !git_adapter::path_exists(worktree).unwrap_or(false) {
        return service.recovery_command(["doctor"]);
    }
    let Ok(info) = git_adapter::repository_info(repository) else {
        return service.recovery_command(["doctor"]);
    };
    let Ok(expected_common) = git_adapter::canonicalize_existing(common_dir) else {
        return service.recovery_command(["doctor"]);
    };
    if info.common_dir != expected_common {
        return service.recovery_command(["doctor"]);
    }
    let status = git_adapter::observe_status(
        &info.repository_path.to_string_lossy(),
        &info.common_dir.to_string_lossy(),
        &worktree.to_string_lossy(),
        branch,
    );
    if !status.is_ok_and(|status| status.exists) {
        return service.recovery_command(["doctor"]);
    }
    let task = service.connection().ok().and_then(|connection| {
        if worktree_path_owner(&connection, worktree).ok()?.is_some() {
            return None;
        }
        load_task(&connection, task_id)
            .ok()
            .filter(task_without_worktree_references)
    });
    match task {
        Some(task) => service.recovery_command(vec![
            "worktree".to_owned(),
            "adopt".to_owned(),
            task_id.to_string(),
            "--repo".to_owned(),
            info.repository_path.to_string_lossy().into_owned(),
            "--path".to_owned(),
            worktree.to_string_lossy().into_owned(),
            "--if-version".to_owned(),
            task.version.to_string(),
        ]),
        None => service.recovery_command(["doctor"]),
    }
}

fn post_git_observation_error(
    registration: &Result<Option<git_adapter::ObservedWorktree>, git_adapter::GitError>,
    path_exists: &Result<bool, git_adapter::GitError>,
) -> String {
    let mut errors = Vec::new();
    if let Err(error) = registration {
        errors.push(format!("Git registration: {error}"));
    }
    if let Err(error) = path_exists {
        errors.push(format!("path metadata: {error}"));
    }
    errors.join("; ")
}

fn worktree_status_state_eq(
    before: &steward_core::WorktreeStatus,
    after: &steward_core::WorktreeStatus,
) -> bool {
    before.registered == after.registered
        && before.repository_path == after.repository_path
        && before.repository_common_dir == after.repository_common_dir
        && before.path == after.path
        && before.exists == after.exists
        && before.branch == after.branch
        && before.head == after.head
        && before.staged == after.staged
        && before.unstaged == after.unstaged
        && before.untracked == after.untracked
        && before.ignored == after.ignored
}

fn removal_failure_diagnostics(
    invocation: &git_adapter::GitInvocation,
    status: Option<&Result<steward_core::WorktreeStatus, git_adapter::GitError>>,
) -> Value {
    match status {
        Some(Ok(status)) => json!({
            "gitInvocation": git_invocation_diagnostics(invocation),
            "postFailureStatus": status,
        }),
        Some(Err(error)) => json!({
            "gitInvocation": git_invocation_diagnostics(invocation),
            "postFailureObservationError": error.to_string(),
        }),
        None => git_invocation_diagnostics(invocation),
    }
}

fn observed_git_state(
    observed: Option<&git_adapter::ObservedWorktree>,
    path_exists: bool,
) -> Value {
    json!({
        "pathExists": path_exists,
        "registeredByGit": observed.is_some(),
        "branch": observed.and_then(|item| item.branch.as_deref()),
        "head": observed.and_then(|item| item.head.as_deref()),
    })
}

fn git_invocation_diagnostics(invocation: &git_adapter::GitInvocation) -> Value {
    json!({
        "gitExitSuccess": invocation.succeeded(),
        "gitExitStatus": invocation.exit_status(),
        "stderrSummary": invocation.diagnostic_summary(),
    })
}

fn database_commit_unknown(
    service: &Service,
    repository_path: Option<&str>,
    worktree_path: Option<&str>,
    git_state: Value,
    error: &rusqlite::Error,
) -> AppError {
    AppError::partial_with_diagnostics(
        repository_path,
        worktree_path,
        git_state,
        PartialDatabaseState::Unknown,
        service.recovery_command(["doctor"]),
        json!({"phase": "databaseCommit", "databaseError": error.to_string()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn complete_status_comparison_detects_partial_file_deletion() {
        let mut before = git_adapter::empty_worktree_status();
        before.registered = true;
        before.exists = true;
        before.head = Some("head".into());
        before.staged = Some(Vec::new());
        before.unstaged = Some(Vec::new());
        before.untracked = Some(Vec::new());
        before.ignored = Some(Vec::new());
        let mut after = before.clone();
        after.observed_at = "later".into();
        assert!(worktree_status_state_eq(&before, &after));

        after.unstaged = Some(vec!["deleted-file.txt".into()]);
        assert!(!worktree_status_state_eq(&before, &after));
    }

    #[test]
    fn post_git_database_failures_report_partial_state_and_targeted_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("state.sqlite"));
        let repository = temp.path().join("repository");
        let worktree = temp.path().join("worktree");
        let created = git_adapter::ObservedWorktree {
            path: worktree.clone(),
            branch: Some("feature".to_owned()),
            head: Some("0123456789abcdef".to_owned()),
        };
        let cause = json!({
            "code": "UNSUPPORTED_SCHEMA_VERSION",
            "message": "database schema is newer than this taskctl build",
            "retryable": false,
            "details": {"databaseVersion": 99, "maxSupportedVersion": 6},
        });

        let create_error = created_worktree_database_failure(
            &service,
            1,
            &repository,
            &created,
            7,
            "databaseReconnect",
            cause.clone(),
        );
        assert_eq!(create_error.body.code, "PARTIAL_EXTERNAL_STATE");
        assert_eq!(
            create_error.body.details["worktreePath"],
            worktree.to_string_lossy().as_ref()
        );
        assert_eq!(create_error.body.details["gitState"]["pathExists"], true);
        assert_eq!(
            create_error.body.details["gitState"]["registeredByGit"],
            true
        );
        assert_eq!(create_error.body.details["databaseState"], "unchanged");
        assert_eq!(
            create_error.body.details["diagnostics"]["databaseError"]["code"],
            "UNSUPPORTED_SCHEMA_VERSION"
        );
        assert_eq!(
            create_error.body.details["diagnostics"]["databaseError"]["details"]["databaseVersion"],
            99
        );
        assert_eq!(create_error.body.details["recommendedArgs"][4], "adopt");
        assert_eq!(
            create_error.body.details["recommendedArgs"]
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            "7"
        );

        let remove_error = removed_worktree_database_failure(
            &service,
            1,
            &repository,
            &worktree,
            8,
            "databaseReconnect",
            cause,
        );
        assert_eq!(remove_error.body.code, "PARTIAL_EXTERNAL_STATE");
        assert_eq!(
            remove_error.body.details["worktreePath"],
            worktree.to_string_lossy().as_ref()
        );
        assert_eq!(remove_error.body.details["gitState"]["pathExists"], false);
        assert_eq!(
            remove_error.body.details["gitState"]["registeredByGit"],
            false
        );
        assert_eq!(remove_error.body.details["databaseState"], "unchanged");
        assert_eq!(remove_error.body.details["recommendedArgs"][4], "detach");
        assert_eq!(
            remove_error.body.details["recommendedArgs"]
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            "8"
        );
    }

    #[test]
    fn owner_lookup_revalidates_live_paths_inside_the_write_transaction() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("state.sqlite"));
        service
            .task_create(
                "TASK OWNER",
                r#"{
                    "title":"0904｜功能｜Owner",
                    "goal":"Protect the live Worktree",
                    "scope":"Test",
                    "acceptanceCriteria":"A stale path key cannot hide the owner"
                }"#,
            )
            .unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir(&worktree).unwrap();
        let owner_id = task_numeric_id(&service, "TASK OWNER");
        service
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET repository_path='repo',repository_common_dir='common',
                    repository_branch='feature',worktree_path=?2,worktree_path_key='stale-key'
                 WHERE id=?1",
                params![owner_id, worktree.to_str().unwrap()],
            )
            .unwrap();

        let mut connection = service.connection().unwrap();
        let tx = storage_sqlite::write_transaction(&mut connection).unwrap();
        assert_eq!(worktree_path_owner(&tx, &worktree).unwrap(), Some(owner_id));
    }

    #[test]
    fn orphaned_create_recovery_uses_doctor_without_live_git_state() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("state.sqlite"));
        service
            .task_create(
                "TASK RECOVERY",
                r#"{
                    "title":"0904｜功能｜Initial",
                    "goal":"Recover the worktree",
                    "scope":"Test",
                    "acceptanceCriteria":"Current version is used"
                }"#,
            )
            .unwrap();
        service
            .task_update("TASK RECOVERY", 1, r#"{"title":"0904｜优化｜Updated"}"#)
            .unwrap();

        let recovery = adopt_recovery_for_created_worktree(
            &service,
            task_numeric_id(&service, "TASK RECOVERY"),
            Path::new("repository"),
            Path::new("common"),
            Path::new("worktree with spaces"),
        );
        assert_eq!(recovery.args.last().unwrap(), "doctor");
    }

    #[test]
    fn detach_recovery_uses_the_current_task_version() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("state.sqlite"));
        service
            .task_create(
                "TASK DETACH RECOVERY",
                r#"{
                    "title":"0904｜功能｜Initial",
                    "goal":"Recover the database reference",
                    "scope":"Test",
                    "acceptanceCriteria":"Current version is used"
                }"#,
            )
            .unwrap();
        let repository = temp.path().join("repository");
        let common_dir = temp.path().join("common");
        let worktree = temp.path().join("worktree");
        let worktree_key = git_adapter::worktree_path_key(&worktree).unwrap();
        service
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET repository_path=?2,repository_common_dir=?3,
                    repository_branch='feature',worktree_path=?4,worktree_path_key=?5,
                    version=2 WHERE id=?1",
                params![
                    task_numeric_id(&service, "TASK DETACH RECOVERY"),
                    repository.to_string_lossy(),
                    common_dir.to_string_lossy(),
                    worktree.to_string_lossy(),
                    worktree_key,
                ],
            )
            .unwrap();
        service
            .task_note("TASK DETACH RECOVERY", 2, "progress", "Concurrent update")
            .unwrap();

        let recovery = detach_recovery_after_update(
            &service,
            task_numeric_id(&service, "TASK DETACH RECOVERY"),
            &repository,
            &common_dir,
            "feature",
            &worktree,
        );

        assert_eq!(recovery.args.last().unwrap(), "3");
    }

    #[test]
    fn orphaned_create_recovery_uses_doctor_when_another_task_owns_the_path() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("state.sqlite"));
        for task_id in ["TASK OWNER", "TASK RECOVERY"] {
            service
                .task_create(
                    task_id,
                    &format!(
                        r#"{{
                            "title":"0904｜功能｜{task_id}",
                            "goal":"Protect Worktree ownership",
                            "scope":"Test",
                            "acceptanceCriteria":"Conflicting adoption is not recommended"
                        }}"#
                    ),
                )
                .unwrap();
        }
        let repository = temp.path().join("repository");
        fs::create_dir(&repository).unwrap();
        git(&repository, ["init", "-b", "main"]);
        git(
            &repository,
            ["config", "user.email", "tests@example.invalid"],
        );
        git(&repository, ["config", "user.name", "Agent Steward Tests"]);
        fs::write(repository.join("README.md"), "fixture\n").unwrap();
        git(&repository, ["add", "README.md"]);
        git(&repository, ["commit", "-m", "fixture"]);
        git(&repository, ["branch", "feature"]);
        let worktree = temp.path().join("worktree");
        git(
            &repository,
            ["worktree", "add", worktree.to_str().unwrap(), "feature"],
        );
        let common_dir = git_adapter::repository_info(&repository)
            .unwrap()
            .common_dir;
        let worktree_key = git_adapter::worktree_path_key(&worktree).unwrap();
        service
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET repository_path=?2,repository_common_dir=?3,
                    repository_branch='feature',worktree_path=?4,worktree_path_key=?5,
                    version=2 WHERE id=?1",
                params![
                    task_numeric_id(&service, "TASK OWNER"),
                    repository.to_string_lossy(),
                    common_dir.to_string_lossy(),
                    worktree.to_string_lossy(),
                    worktree_key,
                ],
            )
            .unwrap();

        let recovery = adopt_recovery_for_created_worktree(
            &service,
            task_numeric_id(&service, "TASK RECOVERY"),
            &repository,
            &common_dir,
            &worktree,
        );

        assert_eq!(recovery.args.last().unwrap(), "doctor");
    }

    #[test]
    fn adopt_recovery_requires_a_live_branched_worktree_and_current_version() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        fs::create_dir(&repository).unwrap();
        git(&repository, ["init", "-b", "main"]);
        git(
            &repository,
            ["config", "user.email", "tests@example.invalid"],
        );
        git(&repository, ["config", "user.name", "Agent Steward Tests"]);
        fs::write(repository.join("README.md"), "fixture\n").unwrap();
        git(&repository, ["add", "README.md"]);
        git(&repository, ["commit", "-m", "fixture"]);

        let live = temp.path().join("live");
        git(&repository, ["branch", "feature"]);
        git(
            &repository,
            ["worktree", "add", live.to_str().unwrap(), "feature"],
        );
        let detached = temp.path().join("detached");
        git(
            &repository,
            [
                "worktree",
                "add",
                "--detach",
                detached.to_str().unwrap(),
                "HEAD",
            ],
        );
        let stale = temp.path().join("stale");
        git(&repository, ["branch", "stale"]);
        git(
            &repository,
            ["worktree", "add", stale.to_str().unwrap(), "stale"],
        );
        fs::remove_file(stale.join(".git")).unwrap();

        let service = Service::new(temp.path().join("state.sqlite"));
        service
            .task_create(
                "TASK ADOPT RECOVERY",
                r#"{
                    "title":"0904｜功能｜Initial",
                    "goal":"Recover a valid worktree",
                    "scope":"Test",
                    "acceptanceCriteria":"Unsafe adoption is not recommended"
                }"#,
            )
            .unwrap();
        service
            .task_update(
                "TASK ADOPT RECOVERY",
                1,
                r#"{"title":"0904｜优化｜Updated"}"#,
            )
            .unwrap();
        service
            .task_note("TASK ADOPT RECOVERY", 2, "progress", "Concurrent update")
            .unwrap();
        let task_id = task_numeric_id(&service, "TASK ADOPT RECOVERY");

        let info = git_adapter::repository_info(&repository).unwrap();
        let live_item = git_adapter::find_worktree(&repository, &live).unwrap();
        let recovery = validated_adopt_recovery(
            &service,
            task_id,
            &repository,
            &info.common_dir,
            &live,
            live_item.as_ref(),
        );
        assert_eq!(recovery.args.last().unwrap(), "3");
        let recovery = adopt_recovery_for_created_worktree(
            &service,
            task_id,
            &repository,
            &info.common_dir,
            &live,
        );
        assert_eq!(recovery.args.last().unwrap(), "3");

        let detached_item = git_adapter::find_worktree(&repository, &detached).unwrap();
        let recovery = validated_adopt_recovery(
            &service,
            task_id,
            &repository,
            &info.common_dir,
            &detached,
            detached_item.as_ref(),
        );
        assert_eq!(recovery.args.last().unwrap(), "doctor");
        let recovery = adopt_recovery_for_created_worktree(
            &service,
            task_id,
            &repository,
            &info.common_dir,
            &detached,
        );
        assert_eq!(recovery.args.last().unwrap(), "doctor");

        let stale_item = git_adapter::find_worktree(&repository, &stale).unwrap();
        let recovery = validated_adopt_recovery(
            &service,
            task_id,
            &repository,
            &info.common_dir,
            &stale,
            stale_item.as_ref(),
        );
        assert_eq!(recovery.args.last().unwrap(), "doctor");
        let recovery = adopt_recovery_for_created_worktree(
            &service,
            task_id,
            &repository,
            &info.common_dir,
            &stale,
        );
        assert_eq!(recovery.args.last().unwrap(), "doctor");

        let other_common = temp.path().join("other-common");
        fs::create_dir(&other_common).unwrap();
        let recovery = adopt_recovery_for_created_worktree(
            &service,
            task_id,
            &repository,
            &other_common,
            &live,
        );
        assert_eq!(recovery.args.last().unwrap(), "doctor");
    }

    #[test]
    fn detach_refuses_a_repository_with_a_different_common_directory() {
        let temp = tempfile::tempdir().unwrap();
        let registered_repository = temp.path().join("registered-repository");
        let current_repository = temp.path().join("current-repository");
        fs::create_dir(&registered_repository).unwrap();
        fs::create_dir(&current_repository).unwrap();
        git(&registered_repository, ["init", "-b", "main"]);
        git(&current_repository, ["init", "-b", "main"]);
        let registered_info = git_adapter::repository_info(&registered_repository).unwrap();
        let current_info = git_adapter::repository_info(&current_repository).unwrap();
        let missing_worktree = temp.path().join("missing-worktree");
        let worktree_key = git_adapter::worktree_path_key(&missing_worktree).unwrap();

        let service = Service::new(temp.path().join("state.sqlite"))
            .with_lock_root(temp.path().join("locks"));
        service
            .task_create(
                "TASK COMMON DIR",
                r#"{
                    "title":"0904｜功能｜Initial",
                    "goal":"Protect repository identity",
                    "scope":"Test",
                    "acceptanceCriteria":"Mismatched common directories are refused"
                }"#,
            )
            .unwrap();
        service
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET repository_path=?2,repository_common_dir=?3,
                    repository_branch='main',worktree_path=?4,worktree_path_key=?5,
                    version=2 WHERE id=?1",
                params![
                    task_numeric_id(&service, "TASK COMMON DIR"),
                    current_info.repository_path.to_string_lossy(),
                    registered_info.common_dir.to_string_lossy(),
                    missing_worktree.to_string_lossy(),
                    worktree_key,
                ],
            )
            .unwrap();

        let error = service
            .worktree_detach("TASK COMMON DIR", 2, &missing_worktree)
            .unwrap_err();
        assert_eq!(error.body.code, "WORKTREE_SAFETY_REFUSED");
        assert_eq!(
            error.body.details["reason"],
            "repository identity no longer matches the registered common directory"
        );
    }

    #[test]
    fn uncertain_database_commit_requires_doctor_reconciliation() {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("state.sqlite"));
        let error = database_commit_unknown(
            &service,
            Some("repository"),
            Some("worktree"),
            json!({"pathExists": true, "registeredByGit": true}),
            &rusqlite::Error::InvalidQuery,
        );

        assert_eq!(error.body.code, "PARTIAL_EXTERNAL_STATE");
        assert_eq!(error.body.details["databaseState"], "unknown");
        assert_eq!(
            error.body.details["gitState"],
            json!({"pathExists": true, "registeredByGit": true})
        );
        assert_eq!(
            error.body.details["recommendedArgs"]
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            "doctor"
        );
        assert_eq!(error.body.details["diagnostics"]["phase"], "databaseCommit");
    }

    fn task_numeric_id(service: &Service, reference: &str) -> i64 {
        service.task_show(reference).unwrap().data["task"]["id"]
            .as_i64()
            .unwrap()
    }

    fn git<const N: usize>(repository: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
