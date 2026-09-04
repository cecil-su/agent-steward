use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

use rusqlite::{OptionalExtension, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use steward_core::{SessionImportView, TaskStatus};
use storage_sqlite::now;
use uuid::Uuid;

use crate::db::{
    bump_task, check_version, checkpoint_from_row, history_from_row, import_from_row,
    insert_history, load_session, load_task, load_task_by_reference, resolve_task_id,
    session_from_row,
};
use crate::{AppError, AppResult, Outcome, RecoveryCommand, Service, warning};

const MAX_IMPORT_BYTES: u64 = 16 * 1024 * 1024;
const IMPORT_READ_BUFFER_BYTES: usize = 64 * 1024;

impl Service {
    pub fn session_show(&self, id: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let session = load_session(&connection, id)?;
        let mut outcome = Outcome::new(json!({"session": session}));
        if let Some(path) = outcome.data["session"]["recordPath"].as_str()
            && !Path::new(path).exists()
        {
            outcome.warnings.push(warning(
                "RECORD_PATH_MISSING",
                "the referenced session record path does not exist",
                json!({"sessionId": id, "recordPath": path}),
            ));
        }
        Ok(outcome)
    }

    pub fn session_list(&self, task_reference: Option<&str>) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task_id = task_reference
            .map(|reference| resolve_task_id(&connection, reference))
            .transpose()?;
        let mut statement = if task_id.is_some() {
            connection
                .prepare(
                    "SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at
                     FROM sessions WHERE task_id=?1 ORDER BY started_at ASC,id ASC",
                )
                .map_err(AppError::from_sqlite)?
        } else {
            connection
                .prepare(
                    "SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at
                     FROM sessions ORDER BY started_at ASC,id ASC",
                )
                .map_err(AppError::from_sqlite)?
        };
        let sessions = if let Some(task_id) = task_id {
            statement
                .query_map([task_id], session_from_row)
                .map_err(AppError::from_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from_sqlite)?
        } else {
            statement
                .query_map([], session_from_row)
                .map_err(AppError::from_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from_sqlite)?
        };
        let mut outcome = Outcome::new(json!({"sessions": sessions}));
        for session in outcome.data["sessions"].as_array().into_iter().flatten() {
            if let (Some(id), Some(path)) = (session["id"].as_str(), session["recordPath"].as_str())
                && !Path::new(path).exists()
            {
                outcome.warnings.push(warning(
                    "RECORD_PATH_MISSING",
                    "the referenced session record path does not exist",
                    json!({"sessionId": id, "recordPath": path}),
                ));
            }
        }
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn session_attach(
        &self,
        task_reference: &str,
        expected: i64,
        session_id: &str,
        source: Option<&str>,
        external_session_id: Option<&str>,
        record_path: Option<&Path>,
    ) -> AppResult<Outcome> {
        if external_session_id.is_some() && source.is_none_or(|value| value.trim().is_empty()) {
            return Err(AppError::invalid(
                "source",
                "required when externalSessionId is present",
            ));
        }
        let source = source.map(str::trim).filter(|value| !value.is_empty());
        let external_session_id = external_session_id
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (record_path, missing) = match record_path {
            Some(path) if path.exists() => (
                Some(
                    git_adapter::canonicalize_existing(path)
                        .map_err(|error| AppError::from_git(error, path.to_str()))?
                        .to_string_lossy()
                        .into_owned(),
                ),
                false,
            ),
            Some(path) => (
                Some(
                    git_adapter::canonicalize_target(path)
                        .map_err(|error| AppError::from_git(error, path.to_str()))?
                        .to_string_lossy()
                        .into_owned(),
                ),
                true,
            ),
            None => (None, false),
        };
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, task_reference)?;
        let task_id = task.id;
        check_version(&task, expected)?;
        if task.status == TaskStatus::Closed {
            return Err(AppError::constraint("session.attach.closed_task"));
        }
        let existing = tx
            .query_row(
                "SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at
                 FROM sessions WHERE id=?1",
                [session_id],
                session_from_row,
            )
            .optional()
            .map_err(AppError::from_sqlite)?;
        if let Some(existing) = existing {
            if existing.task_id == task_id
                && existing.source.as_deref() == source
                && existing.external_session_id.as_deref() == external_session_id
                && existing.record_path == record_path
            {
                let mut outcome = Outcome::new(json!({"task": task, "session": existing}));
                if missing {
                    outcome.warnings.push(warning(
                        "RECORD_PATH_MISSING",
                        "the referenced session record path does not exist",
                        json!({"sessionId": session_id, "recordPath": record_path}),
                    ));
                }
                return Ok(outcome);
            }
            return Err(AppError::session(
                task.current_session_id.as_deref(),
                session_id,
            ));
        }
        tx.execute(
            "INSERT INTO sessions(id,task_id,source,external_session_id,record_path,started_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                session_id,
                task_id,
                source,
                external_session_id,
                record_path,
                timestamp
            ],
        )
        .map_err(AppError::from_sqlite)?;
        bump_task(&tx, task_id, expected, &timestamp)?;
        insert_history(
            &tx,
            task_id,
            "session.attached",
            Some(session_id),
            "session attached",
            json!({"sessionId": session_id, "source": source, "externalSessionId": external_session_id}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, task_id)?;
        let response_session = load_session(&tx, session_id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        let mut outcome = Outcome::new(json!({
            "task": response_task,
            "session": response_session,
        }));
        if missing {
            outcome.warnings.push(warning(
                "RECORD_PATH_MISSING",
                "the referenced session record path does not exist",
                json!({"sessionId": session_id, "recordPath": record_path}),
            ));
        }
        Ok(outcome)
    }

    pub fn session_close(&self, session_id: &str, expected: i64) -> AppResult<Outcome> {
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let session = load_session(&tx, session_id)?;
        let task = load_task(&tx, session.task_id)?;
        check_version(&task, expected)?;
        if session.ended_at.is_some() {
            return Ok(Outcome::new(json!({"task": task, "session": session})));
        }
        let was_current = task.current_session_id.as_deref() == Some(session_id);
        tx.execute(
            "UPDATE sessions SET ended_at=?2 WHERE id=?1 AND ended_at IS NULL",
            params![session_id, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        if was_current {
            tx.execute(
                "UPDATE tasks SET current_session_id=NULL,version=version+1,updated_at=?3
                 WHERE id=?1 AND version=?2",
                params![session.task_id, expected, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        } else {
            bump_task(&tx, session.task_id, expected, &timestamp)?;
        }
        insert_history(
            &tx,
            session.task_id,
            "session.closed",
            Some(session_id),
            "session closed",
            json!({"sessionId": session_id, "wasCurrent": was_current}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, session.task_id)?;
        let response_session = load_session(&tx, session_id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({
            "task": response_task,
            "session": response_session,
        })))
    }

    pub fn task_resume(
        &self,
        task_reference: &str,
        expected: i64,
        new_session_id: &str,
        from_session: Option<&str>,
        take_over: bool,
    ) -> AppResult<Outcome> {
        let initial = self.connection()?;
        let initial_task = load_task_by_reference(&initial, task_reference)?;
        let task_id = initial_task.id;
        check_version(&initial_task, expected)?;
        let _ = load_latest_checkpoint(&initial, &initial_task)?;
        let worktree_status = if initial_task.worktree_path.is_some() {
            Some(self.worktree_status(&task_id.to_string())?.data["worktreeStatus"].clone())
        } else {
            None
        };
        drop(initial);
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task(&tx, task_id)?;
        check_version(&task, expected)?;
        if !matches!(task.status, TaskStatus::InProgress | TaskStatus::Blocked) {
            return Err(AppError::constraint("task.resume.requires_active_task"));
        }
        let source_id = from_session
            .map(ToOwned::to_owned)
            .or_else(|| task.current_session_id.clone())
            .ok_or_else(|| AppError::invalid("fromSession", "resume source is required"))?;
        if source_id == new_session_id {
            return Err(AppError::invalid(
                "session",
                "must differ from resume source",
            ));
        }
        let source = load_session(&tx, &source_id)?;
        if source.task_id != task_id {
            return Err(AppError::constraint("session.resume.source_cross_task"));
        }
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
                [new_session_id],
                |row| row.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        if exists {
            return Err(AppError::constraint("sessions.id.unique"));
        }
        if task.current_session_id.is_some() && !take_over {
            return Err(AppError::session(
                task.current_session_id.as_deref(),
                new_session_id,
            ));
        }
        tx.execute(
            "INSERT INTO sessions(id,task_id,continued_from,started_at) VALUES (?1,?2,?3,?4)",
            params![new_session_id, task_id, source_id, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        let changed = tx
            .execute(
                "UPDATE tasks SET current_session_id=?3,version=version+1,updated_at=?4
                 WHERE id=?1 AND version=?2",
                params![task_id, expected, new_session_id, timestamp],
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
            "session.resumed",
            Some(new_session_id),
            "task resumed in a new session",
            json!({
                "sessionId": new_session_id,
                "continuedFrom": source_id,
                "displacedSessionId": task.current_session_id,
            }),
            &timestamp,
        )?;
        let response_task = load_task(&tx, task_id)?;
        let response_checkpoint = load_latest_checkpoint(&tx, &response_task)?;
        let response_sessions = list_sessions_for_task(&tx, task_id)?;
        let next_step = response_task.next_step.clone();
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({
            "task": response_task,
            "checkpoint": response_checkpoint,
            "sessions": response_sessions,
            "worktreeStatus": worktree_status,
            "nextStep": next_step,
        })))
    }

    pub fn session_import_add(
        &self,
        task_reference: &str,
        session_id: &str,
        expected: i64,
        file_path: &Path,
        sensitive_content_reviewed: bool,
    ) -> AppResult<Outcome> {
        if !sensitive_content_reviewed {
            return Err(AppError::invalid(
                "confirmSensitiveContentReviewed",
                "sensitive content review must be confirmed before importing",
            ));
        }
        let canonical = git_adapter::canonicalize_existing(file_path)
            .map_err(|error| AppError::from_git(error, file_path.to_str()))?;
        let path_metadata = fs::metadata(&canonical)
            .map_err(|error| AppError::invalid("file", error.to_string()))?;
        if !path_metadata.file_type().is_file() {
            return Err(AppError::invalid("file", "must be a regular file"));
        }
        if path_metadata.len() > MAX_IMPORT_BYTES {
            return Err(AppError::invalid("file", "file exceeds the 16 MiB limit"));
        }
        let mut file = open_import_file(&canonical).map_err(|error| {
            AppError::invalid("file", format!("cannot open import file: {error}"))
        })?;
        let metadata = file
            .metadata()
            .map_err(|error| AppError::invalid("file", error.to_string()))?;
        if !metadata.file_type().is_file() {
            return Err(AppError::invalid("file", "must be a regular file"));
        }
        if metadata.len() > MAX_IMPORT_BYTES {
            return Err(AppError::invalid("file", "file exceeds the 16 MiB limit"));
        }
        let mut content = Vec::with_capacity(metadata.len() as usize);
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; IMPORT_READ_BUFFER_BYTES];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| AppError::invalid("file", error.to_string()))?;
            if read == 0 {
                break;
            }
            if content.len() as u64 + read as u64 > MAX_IMPORT_BYTES {
                return Err(AppError::invalid("file", "file exceeds the 16 MiB limit"));
            }
            hasher.update(&buffer[..read]);
            content.extend_from_slice(&buffer[..read]);
        }
        let sha256 = hex::encode(hasher.finalize());
        let media_type = mime_guess::from_path(&canonical)
            .first_raw()
            .map(ToOwned::to_owned);
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, task_reference)?;
        let task_id = task.id;
        check_version(&task, expected)?;
        let session = load_session(&tx, session_id)?;
        if session.task_id != task_id {
            return Err(AppError::constraint("session_import.session_cross_task"));
        }
        let duplicate = tx
            .query_row(
                "SELECT id,session_id,source_path,media_type,sha256,length(content),imported_at
                 FROM session_imports WHERE session_id=?1 AND sha256=?2",
                params![session_id, sha256],
                import_from_row,
            )
            .optional()
            .map_err(AppError::from_sqlite)?;
        if let Some(existing) = duplicate {
            return Ok(Outcome::new(json!({"task": task, "import": existing}))
                .with_warning(warning(
                    "DUPLICATE_SESSION_IMPORT",
                    "identical content is already stored for this session",
                    json!({"sessionId": session_id, "sha256": sha256}),
                ))
                .with_warning(warning(
                    "SENSITIVE_CONTENT_CHECK_REQUIRED",
                    "verify that imported content does not contain credentials or secrets",
                    json!({"sourcePath": canonical.to_string_lossy()}),
                )));
        }
        let import_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO session_imports(id,session_id,source_path,media_type,sha256,content,imported_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                import_id,
                session_id,
                canonical.to_string_lossy(),
                media_type,
                sha256,
                content,
                timestamp
            ],
        )
        .map_err(AppError::from_sqlite)?;
        bump_task(&tx, task_id, expected, &timestamp)?;
        insert_history(
            &tx,
            task_id,
            "session.imported",
            Some(session_id),
            "session content imported",
            json!({
                "importId": import_id,
                "sessionId": session_id,
                "sha256": sha256,
                "sizeBytes": content.len(),
            }),
            &timestamp,
        )?;
        let response_task = load_task(&tx, task_id)?;
        let response_import = tx
            .query_row(
                "SELECT id,session_id,source_path,media_type,sha256,length(content),imported_at
                 FROM session_imports WHERE id=?1",
                [&import_id],
                import_from_row,
            )
            .map_err(AppError::from_sqlite)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(
            Outcome::new(json!({"task": response_task, "import": response_import})).with_warning(
                warning(
                    "SENSITIVE_CONTENT_CHECK_REQUIRED",
                    "verify that imported content does not contain credentials or secrets",
                    json!({"sourcePath": canonical.to_string_lossy()}),
                ),
            ),
        )
    }

    pub fn session_import_list(&self, session_id: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let _ = load_session(&connection, session_id)?;
        let mut statement = connection
            .prepare(
                "SELECT id,session_id,source_path,media_type,sha256,length(content),imported_at
                 FROM session_imports WHERE session_id=?1 ORDER BY imported_at ASC,id ASC",
            )
            .map_err(AppError::from_sqlite)?;
        let imports = statement
            .query_map([session_id], import_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"imports": imports})))
    }

    pub fn session_import_metadata(&self, import_id: &str) -> AppResult<SessionImportView> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id,session_id,source_path,media_type,sha256,length(content),imported_at
                 FROM session_imports WHERE id=?1",
                [import_id],
                import_from_row,
            )
            .optional()
            .map_err(AppError::from_sqlite)?
            .ok_or_else(|| AppError::not_found("SessionImport", import_id))
    }

    pub fn session_import_remove(&self, import_id: &str, expected: i64) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        connection
            .pragma_update(None, "secure_delete", "ON")
            .map_err(AppError::from_sqlite)?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let imported = tx
            .query_row(
                "SELECT id,session_id,source_path,media_type,sha256,length(content),imported_at
                 FROM session_imports WHERE id=?1",
                [import_id],
                import_from_row,
            )
            .optional()
            .map_err(AppError::from_sqlite)?
            .ok_or_else(|| AppError::not_found("SessionImport", import_id))?;
        let session = load_session(&tx, &imported.session_id)?;
        let task = load_task(&tx, session.task_id)?;
        check_version(&task, expected)?;
        tx.execute("DELETE FROM session_imports WHERE id=?1", [import_id])
            .map_err(AppError::from_sqlite)?;
        let timestamp = now();
        bump_task(&tx, session.task_id, expected, &timestamp)?;
        insert_history(
            &tx,
            session.task_id,
            "session.import_removed",
            Some(&session.id),
            "session import removed",
            json!({
                "importId": import_id,
                "sessionId": session.id,
                "sha256": imported.sha256,
                "removedAt": timestamp,
            }),
            &timestamp,
        )?;
        let response_task = load_task(&tx, session.task_id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        let checkpoint_busy: i64 = connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .unwrap_or(1);
        let mut outcome = Outcome::new(json!({
            "task": response_task,
            "import": imported,
        }));
        if checkpoint_busy != 0 {
            outcome.warnings.push(warning(
                "PHYSICAL_ERASURE_NOT_GUARANTEED",
                "logical deletion succeeded but the WAL could not be fully truncated",
                json!({"importId": import_id, "checkpointBusy": checkpoint_busy}),
            ));
        }
        Ok(outcome)
    }

    pub fn history(&self, task_reference: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task_id = resolve_task_id(&connection, task_reference)?;
        let mut statement = connection
            .prepare(
                "SELECT id,task_id,sequence,change_type,session_id,occurred_at,summary,payload_json
                 FROM history WHERE task_id=?1 ORDER BY sequence ASC",
            )
            .map_err(AppError::from_sqlite)?;
        let history = statement
            .query_map([task_id], history_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"history": history})))
    }

    pub fn doctor(&self) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let quick: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(AppError::from_sqlite)?;
        let foreign_key_errors: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(AppError::from_sqlite)?;
        let schema: i64 = connection
            .query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        let mut checks = vec![
            json!({"code":"SCHEMA_VERSION","status":"ok","details":{"version":schema}}),
            json!({"code":"SQLITE_QUICK_CHECK","status":if quick == "ok" {"ok"} else {"error"},"details":{"result":quick}}),
            json!({"code":"FOREIGN_KEY_CHECK","status":if foreign_key_errors == 0 {"ok"} else {"error"},"details":{"violations":foreign_key_errors}}),
        ];
        let mut record_statement = connection
            .prepare(
                "SELECT id,record_path FROM sessions WHERE record_path IS NOT NULL ORDER BY id",
            )
            .map_err(AppError::from_sqlite)?;
        let record_paths = record_statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        let missing_records = record_paths
            .iter()
            .filter(|(_, path)| !std::path::Path::new(path).exists())
            .map(|(session_id, path)| json!({"sessionId":session_id,"recordPath":path}))
            .collect::<Vec<_>>();
        checks.push(json!({
            "code":"RECORD_PATH_REFERENCES",
            "status":if missing_records.is_empty() {"ok"} else {"warning"},
            "details":{"registered":record_paths.len(),"missing":missing_records}
        }));

        let mut task_statement = connection
            .prepare(
                "SELECT id,task_key,title,status,version,goal,scope,acceptance_criteria,next_step,
                        block_reason,block_recovery,current_session_id,repository_path,
                        repository_common_dir,repository_branch,worktree_path,latest_checkpoint_id,
                        closure_outcome,closure_reason,closed_at,created_at,updated_at
                 FROM tasks WHERE worktree_path IS NOT NULL ORDER BY id",
            )
            .map_err(AppError::from_sqlite)?;
        let tasks = task_statement
            .query_map([], crate::db::task_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        if tasks.is_empty() {
            checks.push(json!({
                "code":"WORKTREE_REFERENCES",
                "status":"ok",
                "details":{"registered":0,"issues":[]}
            }));
        } else {
            let mut issues = Vec::new();
            for task in &tasks {
                let (repository, common, branch, path) = match (
                    task.repository_path.as_deref(),
                    task.repository_common_dir.as_deref(),
                    task.repository_branch.as_deref(),
                    task.worktree_path.as_deref(),
                ) {
                    (Some(repository), Some(common), Some(branch), Some(path)) => {
                        (repository, common, branch, path)
                    }
                    _ => {
                        let RecoveryCommand { command, args } = self.recovery_command(["doctor"]);
                        issues.push(json!({
                            "taskId":task.id,
                            "reason":"incomplete database references",
                            "recommendedCommand":command,
                            "recommendedArgs":args,
                        }));
                        continue;
                    }
                };
                match git_adapter::observe_status(repository, common, path, branch) {
                    Ok(status) if status.exists => {}
                    Ok(status) => {
                        let repository_info = match git_adapter::repository_info(Path::new(
                            repository,
                        )) {
                            Ok(info) if info.common_dir.to_string_lossy() == common => info,
                            Ok(_) => {
                                let RecoveryCommand { command, args } =
                                    self.recovery_command(["doctor"]);
                                issues.push(json!({
                                    "taskId":task.id,
                                    "reason":"repository identity no longer matches the registered common directory",
                                    "observed":status,
                                    "registeredByGit":null,
                                    "recommendedCommand":command,
                                    "recommendedArgs":args,
                                }));
                                continue;
                            }
                            Err(error) => {
                                let RecoveryCommand { command, args } =
                                    self.recovery_command(["doctor"]);
                                issues.push(json!({
                                    "taskId":task.id,
                                    "reason":"registered repository identity could not be verified",
                                    "observed":status,
                                    "registeredByGit":null,
                                    "observationError":error.to_string(),
                                    "recommendedCommand":command,
                                    "recommendedArgs":args,
                                }));
                                continue;
                            }
                        };
                        let registered_by_git = match git_adapter::find_worktree_registration(
                            &repository_info.repository_path,
                            Path::new(path),
                        ) {
                            Ok(found) => Some(found.is_some()),
                            Err(error) => {
                                let RecoveryCommand { command, args } =
                                    self.recovery_command(["doctor"]);
                                issues.push(json!({
                                        "taskId":task.id,
                                        "reason":"registered worktree registration could not be verified",
                                        "observed":status,
                                        "registeredByGit":null,
                                        "observationError":error.to_string(),
                                        "recommendedCommand":command,
                                        "recommendedArgs":args,
                                    }));
                                None
                            }
                        };
                        if registered_by_git.is_none() {
                            continue;
                        }
                        if let Err(error) =
                            git_adapter::verify_repository_identity(&repository_info)
                        {
                            let RecoveryCommand { command, args } =
                                self.recovery_command(["doctor"]);
                            issues.push(json!({
                                "taskId":task.id,
                                "reason":"registered repository identity could not be verified",
                                "observed":status,
                                "registeredByGit":null,
                                "observationError":error.to_string(),
                                "recommendedCommand":command,
                                "recommendedArgs":args,
                            }));
                            continue;
                        }
                        let (reason, recovery, message) = if registered_by_git == Some(true) {
                            (
                                "registered worktree is absent but Git still registers it",
                                self.recovery_command(["doctor"]),
                                Some(
                                    "prune the stale Git registration or restore the directory before detaching",
                                ),
                            )
                        } else {
                            (
                                "registered worktree is absent",
                                self.recovery_command(vec![
                                    "worktree".to_owned(),
                                    "detach".to_owned(),
                                    task.id.to_string(),
                                    "--expected-path".to_owned(),
                                    path.to_owned(),
                                    "--if-version".to_owned(),
                                    task.version.to_string(),
                                ]),
                                None,
                            )
                        };
                        let RecoveryCommand { command, args } = recovery;
                        issues.push(json!({
                            "taskId":task.id,
                            "reason":reason,
                            "observed":status,
                            "registeredByGit":registered_by_git,
                            "recoveryNote":message,
                            "recommendedCommand":command,
                            "recommendedArgs":args,
                        }));
                    }
                    Err(error) => {
                        let RecoveryCommand { command, args } = self.recovery_command(["doctor"]);
                        issues.push(json!({
                            "taskId":task.id,
                            "reason":error.to_string(),
                            "worktreePath":path,
                            "recommendedCommand":command,
                            "recommendedArgs":args,
                        }));
                    }
                }
            }
            checks.push(json!({
                "code":"WORKTREE_REFERENCES",
                "status":if issues.is_empty() {"ok"} else {"warning"},
                "details":{"registered":tasks.len(),"issues":issues}
            }));
        }
        Ok(Outcome::new(json!({"checks": checks})))
    }
}

fn open_import_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(path)
}

fn list_sessions_for_task(
    connection: &rusqlite::Connection,
    task_id: i64,
) -> AppResult<Vec<steward_core::SessionView>> {
    let mut statement = connection
        .prepare(
            "SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at
             FROM sessions WHERE task_id=?1 ORDER BY started_at ASC,id ASC",
        )
        .map_err(AppError::from_sqlite)?;
    statement
        .query_map([task_id], session_from_row)
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)
}

fn load_latest_checkpoint(
    connection: &rusqlite::Connection,
    task: &steward_core::TaskView,
) -> AppResult<Option<steward_core::CheckpointView>> {
    task.latest_checkpoint_id
        .as_deref()
        .map(|id| {
            connection
                .query_row(
                    "SELECT id,task_id,session_id,summary,completed_json,decisions_json,pending_json,
                            next_step,risks_json,git_head,created_at FROM checkpoints WHERE id=?1",
                    [id],
                    checkpoint_from_row,
                )
                .map_err(AppError::from_sqlite)
        })
        .transpose()
}
