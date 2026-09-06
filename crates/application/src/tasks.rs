use rusqlite::{OptionalExtension, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use steward_core::{
    CheckpointInput, TaskCreateInput, TaskStatus, TaskView, require_non_empty,
    validate_string_array,
};
use storage_sqlite::now;
use uuid::Uuid;

use crate::db::{
    bump_task, check_version, checkpoint_from_row, insert_history, load_session, load_task,
    load_task_by_reference, note_from_row, task_from_row,
};
use crate::{AppError, AppResult, Outcome, Service, TaskListOptions};

impl Service {
    pub fn task_create(&self, task_key: &str, input_json: &str) -> AppResult<Outcome> {
        self.task_create_with_options(Some(task_key), Some(input_json))
    }

    pub fn task_create_minimal(&self) -> AppResult<Outcome> {
        self.task_create_with_options(None, None)
    }

    pub fn task_create_with_options(
        &self,
        task_key: Option<&str>,
        input_json: Option<&str>,
    ) -> AppResult<Outcome> {
        let input = input_json
            .map(|value| {
                serde_json::from_str::<TaskCreateInput>(value)
                    .map_err(|error| AppError::invalid("input", error.to_string()))
            })
            .transpose()?
            .unwrap_or_default();
        let task_key = match (task_key, input.task_key.as_deref()) {
            (Some(argument), Some(body)) if argument != body => {
                return Err(AppError::invalid(
                    "taskKey",
                    "positional task key and input taskKey must match",
                ));
            }
            (Some(argument), _) => Some(validate_task_key(argument)?),
            (None, Some(body)) => Some(validate_task_key(body)?),
            (None, None) => None,
        };
        let title = input
            .title
            .map(|value| validate_task_title(&value))
            .transpose()?;
        let goal = optional_description("goal", input.goal)?;
        let scope = optional_description("scope", input.scope)?;
        let acceptance = optional_description("acceptanceCriteria", input.acceptance_criteria)?;
        let next_step = optional_description("nextStep", input.next_step)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        tx.execute(
            "INSERT INTO tasks(
                task_key,title,status,version,goal,scope,acceptance_criteria,next_step,
                created_at,updated_at
             ) VALUES (?1,?2,'open',1,?3,?4,?5,?6,?7,?7)",
            params![
                task_key, title, goal, scope, acceptance, next_step, timestamp
            ],
        )
        .map_err(AppError::from_sqlite)?;
        let id = tx.last_insert_rowid();
        insert_history(
            &tx,
            id,
            "task.created",
            None,
            "task created",
            json!({"title": title, "taskKey": task_key}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_show(&self, reference: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task = load_task_by_reference(&connection, reference)?;
        Ok(Outcome::new(json!({"task": task})))
    }

    /// Read persisted notes without mutating the Task or its History.
    pub fn task_notes(&self, reference: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task = load_task_by_reference(&connection, reference)?;
        let mut statement = connection.prepare(
            "SELECT id,task_id,session_id,note_type,text,created_at FROM task_notes WHERE task_id=?1 ORDER BY id ASC"
        ).map_err(AppError::from_sqlite)?;
        let notes = statement
            .query_map([task.id], note_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"notes":notes})))
    }

    pub fn task_list(&self, status: Option<&str>) -> AppResult<Outcome> {
        self.task_list_with_options(&TaskListOptions {
            status: status.map(ToOwned::to_owned),
            ..TaskListOptions::default()
        })
    }

    pub fn task_list_with_options(&self, options: &TaskListOptions) -> AppResult<Outcome> {
        let status = options
            .status
            .as_deref()
            .map(|value| {
                if value == "active" {
                    return Ok(value.to_owned());
                }
                TaskStatus::try_from(value)
                    .map(|status| status.as_str().to_owned())
                    .map_err(|reason| AppError::invalid("status", reason))
            })
            .transpose()?;
        let task_key = options
            .task_key
            .as_deref()
            .map(|value| required("taskKey", value))
            .transpose()?;
        let query = options
            .query
            .as_deref()
            .map(|value| required("query", value))
            .transpose()?;
        let fields = validate_list_fields(&options.fields)?;
        let filter_digest =
            task_list_filter_digest(status.as_deref(), task_key.as_deref(), query.as_deref());
        let page_size = options.page_size.unwrap_or(DEFAULT_TASK_PAGE_SIZE);
        if !(1..=MAX_TASK_PAGE_SIZE).contains(&page_size) {
            return Err(AppError::invalid(
                "pageSize",
                format!("must be between 1 and {MAX_TASK_PAGE_SIZE}"),
            ));
        }
        let cursor = options
            .cursor
            .as_deref()
            .map(decode_task_cursor)
            .transpose()?;
        if let Some(cursor) = &cursor
            && cursor.filter_digest != filter_digest
        {
            return Err(AppError::invalid(
                "cursor",
                "does not match the current filters",
            ));
        }

        let mut sql = String::from(
            "SELECT id,task_key,title,status,version,goal,scope,acceptance_criteria,next_step,
                    block_reason,block_recovery,current_session_id,repository_path,
                    repository_common_dir,repository_branch,worktree_path,latest_checkpoint_id,
                    closure_outcome,closure_reason,closed_at,created_at,updated_at
             FROM tasks",
        );
        let mut conditions = Vec::new();
        let mut values = Vec::<SqlValue>::new();
        if let Some(status) = &status {
            if status == "active" {
                conditions.push("status != 'closed'");
            } else {
                conditions.push("status=?");
                values.push(SqlValue::Text(status.clone()));
            }
        }
        if let Some(task_key) = &task_key {
            conditions.push("task_key=?");
            values.push(SqlValue::Text(task_key.clone()));
        }
        if let Some(query) = &query {
            conditions.push(
                "(title LIKE ? ESCAPE '\\' OR goal LIKE ? ESCAPE '\\' OR scope LIKE ? ESCAPE '\\')",
            );
            let pattern = escaped_like_pattern(query);
            values.extend((0..3).map(|_| SqlValue::Text(pattern.clone())));
        }
        if let Some(cursor) = &cursor {
            conditions.push("(updated_at < ? OR (updated_at = ? AND id > ?))");
            values.push(SqlValue::Text(cursor.updated_at.clone()));
            values.push(SqlValue::Text(cursor.updated_at.clone()));
            values.push(SqlValue::Integer(cursor.id));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY updated_at DESC,id ASC LIMIT ?");
        values.push(SqlValue::Integer(i64::from(page_size) + 1));

        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql).map_err(AppError::from_sqlite)?;
        let mut tasks = statement
            .query_map(params_from_iter(values.iter()), task_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        let has_more = tasks.len() > page_size as usize;
        if has_more {
            tasks.truncate(page_size as usize);
        }
        let next_cursor = if has_more {
            tasks.last().map(|task| {
                encode_task_cursor(&TaskListCursor {
                    version: 1,
                    updated_at: task.updated_at.clone(),
                    id: task.id,
                    filter_digest: filter_digest.clone(),
                })
            })
        } else {
            None
        };
        let tasks = project_tasks(tasks, &fields);
        Ok(Outcome::new(json!({
            "tasks": tasks,
            "nextCursor": next_cursor,
            "hasMore": has_more,
            "pageSize": page_size,
        })))
    }

    pub fn task_update(
        &self,
        reference: &str,
        expected: i64,
        input_json: &str,
    ) -> AppResult<Outcome> {
        let patch: Value = serde_json::from_str(input_json)
            .map_err(|error| AppError::invalid("input", error.to_string()))?;
        let patch = patch
            .as_object()
            .ok_or_else(|| AppError::invalid("input", "must be a JSON object"))?;
        if patch.is_empty() {
            return Err(AppError::invalid("input", "patch cannot be empty"));
        }
        for key in patch.keys() {
            if !matches!(
                key.as_str(),
                "taskKey" | "title" | "goal" | "scope" | "acceptanceCriteria" | "nextStep"
            ) {
                return Err(AppError::invalid(key, "unknown patch field"));
            }
        }
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let mut task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        ensure_mutable(&task)?;
        let mut changed = Vec::new();
        if let Some(value) = patch.get("taskKey") {
            let value = value
                .as_str()
                .ok_or_else(|| AppError::invalid("taskKey", "must be a non-null string"))?;
            let value = validate_task_key(value)?;
            match task.task_key.as_deref() {
                Some(existing) if existing != value => {
                    return Err(AppError::constraint("tasks.task_key.immutable"));
                }
                Some(_) => {}
                None => {
                    task.task_key = Some(value.to_owned());
                    changed.push("taskKey".to_owned());
                }
            }
        }
        apply_title_patch(patch, &mut task.title, &mut changed)?;
        apply_description_patch(patch, "goal", &mut task.goal, &mut changed)?;
        apply_description_patch(patch, "scope", &mut task.scope, &mut changed)?;
        apply_description_patch(
            patch,
            "acceptanceCriteria",
            &mut task.acceptance_criteria,
            &mut changed,
        )?;
        if let Some(value) = patch.get("nextStep") {
            let next = if value.is_null() {
                None
            } else {
                Some(required(
                    "nextStep",
                    value
                        .as_str()
                        .ok_or_else(|| AppError::invalid("nextStep", "must be a string or null"))?,
                )?)
            };
            if task.next_step != next {
                task.next_step = next;
                changed.push("nextStep".to_owned());
            }
        }
        if changed.is_empty() {
            return Err(AppError::invalid(
                "input",
                "patch does not change any field",
            ));
        }
        changed.sort();
        let timestamp = now();
        let count = tx
            .execute(
                "UPDATE tasks SET task_key=?3,title=?4,goal=?5,scope=?6,acceptance_criteria=?7,
                    next_step=?8,version=version+1,updated_at=?9 WHERE id=?1 AND version=?2",
                params![
                    id,
                    expected,
                    task.task_key,
                    task.title,
                    task.goal,
                    task.scope,
                    task.acceptance_criteria,
                    task.next_step,
                    timestamp
                ],
            )
            .map_err(AppError::from_sqlite)?;
        if count != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "task.updated",
            task.current_session_id.as_deref(),
            "task fields updated",
            json!({"changedFields": changed}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_retitle(&self, reference: &str, expected: i64, title: &str) -> AppResult<Outcome> {
        let title = validate_task_title(title)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        if task.title.as_deref() == Some(title.as_str()) {
            return Err(AppError::invalid("title", "does not change the task title"));
        }
        let count = tx
            .execute(
                "UPDATE tasks SET title=?3,version=version+1,updated_at=?4
                 WHERE id=?1 AND version=?2",
                params![id, expected, title, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        if count != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "task.retitled",
            task.current_session_id.as_deref(),
            "task title changed",
            json!({"previousTitle": task.title, "title": title}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_note(
        &self,
        reference: &str,
        expected: i64,
        note_type: &str,
        text: &str,
    ) -> AppResult<Outcome> {
        if !matches!(note_type, "decision" | "progress" | "risk") {
            return Err(AppError::invalid(
                "type",
                "must be decision, progress, or risk",
            ));
        }
        let text = required("text", text)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        ensure_mutable(&task)?;
        tx.execute(
            "INSERT INTO task_notes(task_id,session_id,note_type,text,created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, task.current_session_id, note_type, text, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        let note_id = tx.last_insert_rowid();
        bump_task(&tx, id, expected, &timestamp)?;
        insert_history(
            &tx,
            id,
            "task.noted",
            task.current_session_id.as_deref(),
            "task note added",
            json!({"noteId": note_id, "noteType": note_type}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        let response_note = tx
            .query_row(
                "SELECT id,task_id,session_id,note_type,text,created_at FROM task_notes WHERE id=?1",
                [note_id],
                note_from_row,
            )
            .map_err(AppError::from_sqlite)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"task": response_task, "note": response_note}),
        ))
    }

    pub fn task_block(
        &self,
        reference: &str,
        expected: i64,
        reason: &str,
        recovery: &str,
    ) -> AppResult<Outcome> {
        let reason = required("reason", reason)?;
        let recovery = required("recovery", recovery)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        if task.status != TaskStatus::InProgress {
            return Err(AppError::constraint("task.block.requires_in_progress"));
        }
        let changed = tx
            .execute(
                "UPDATE tasks SET status='blocked',block_reason=?3,block_recovery=?4,
                    version=version+1,updated_at=?5 WHERE id=?1 AND version=?2",
                params![id, expected, reason, recovery, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "task.blocked",
            task.current_session_id.as_deref(),
            "task blocked",
            json!({"reason": reason, "recovery": recovery}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_unblock(
        &self,
        reference: &str,
        expected: i64,
        next_step: &str,
    ) -> AppResult<Outcome> {
        let next_step = required("nextStep", next_step)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        if task.status != TaskStatus::Blocked {
            return Err(AppError::constraint("task.unblock.requires_blocked"));
        }
        let changed = tx
            .execute(
                "UPDATE tasks SET status='in_progress',block_reason=NULL,block_recovery=NULL,next_step=?3,
                    version=version+1,updated_at=?4 WHERE id=?1 AND version=?2",
                params![id, expected, next_step, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "task.unblocked",
            task.current_session_id.as_deref(),
            "task unblocked",
            json!({"nextStep": next_step}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_close(
        &self,
        reference: &str,
        expected: i64,
        outcome: &str,
        reason: Option<&str>,
    ) -> AppResult<Outcome> {
        if !matches!(
            outcome,
            "completed" | "partial" | "cancelled" | "superseded"
        ) {
            return Err(AppError::invalid("outcome", "unsupported close outcome"));
        }
        let reason = match reason {
            Some(value) => Some(required("reason", value)?),
            None => None,
        };
        if outcome != "completed" && reason.is_none() {
            return Err(AppError::invalid(
                "reason",
                "required for this close outcome",
            ));
        }
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        if task.status == TaskStatus::Closed {
            return Err(AppError::constraint("task.close.already_closed"));
        }
        let valid = match outcome {
            "completed" => task.status == TaskStatus::InProgress,
            "partial" => matches!(task.status, TaskStatus::InProgress | TaskStatus::Blocked),
            _ => true,
        };
        if !valid {
            return Err(AppError::constraint("task.close.invalid_transition"));
        }
        if outcome == "completed"
            && (task.title.is_none()
                || task.goal.is_none()
                || task.scope.is_none()
                || task.acceptance_criteria.is_none())
        {
            return Err(AppError::constraint(
                "task.close.completed_requires_complete_descriptions",
            ));
        }
        if let Some(session_id) = task.current_session_id.as_deref() {
            tx.execute(
                "UPDATE sessions SET ended_at=?2 WHERE id=?1 AND ended_at IS NULL",
                params![session_id, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        }
        let changed = tx
            .execute(
                "UPDATE tasks SET status='closed',current_session_id=NULL,next_step=NULL,
                    block_reason=NULL,block_recovery=NULL,closure_outcome=?3,closure_reason=?4,
                    closed_at=?5,version=version+1,updated_at=?5 WHERE id=?1 AND version=?2",
                params![id, expected, outcome, reason, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "task.closed",
            task.current_session_id.as_deref(),
            "task closed",
            json!({"outcome": outcome, "reason": reason, "closedSessionId": task.current_session_id}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_claim(
        &self,
        reference: &str,
        expected: i64,
        session_id: &str,
        take_over: bool,
    ) -> AppResult<Outcome> {
        required("session", session_id)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        if task.status == TaskStatus::Closed {
            return Err(AppError::constraint("task.claim.closed"));
        }
        if matches!(task.status, TaskStatus::InProgress | TaskStatus::Blocked)
            && task.current_session_id.is_none()
        {
            return Err(AppError::session(None, session_id));
        }
        let existing = tx
            .query_row(
                "SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at
                 FROM sessions WHERE id=?1",
                [session_id],
                crate::db::session_from_row,
            )
            .optional()
            .map_err(AppError::from_sqlite)?;
        if let Some(session) = existing {
            if task.current_session_id.as_deref() == Some(session_id)
                && session.task_id == id
                && session.ended_at.is_none()
            {
                return Ok(Outcome::new(json!({"task": task})));
            }
            return Err(AppError::session(
                task.current_session_id.as_deref(),
                session_id,
            ));
        }
        let previous = task.current_session_id.clone();
        if previous.is_some() && !take_over {
            return Err(AppError::session(previous.as_deref(), session_id));
        }
        tx.execute(
            "INSERT INTO sessions(id,task_id,continued_from,started_at) VALUES (?1,?2,?3,?4)",
            params![
                session_id,
                id,
                if take_over { previous.as_deref() } else { None },
                timestamp
            ],
        )
        .map_err(AppError::from_sqlite)?;
        let next_status = if task.status == TaskStatus::Open {
            "in_progress"
        } else {
            task.status.as_str()
        };
        let changed = tx
            .execute(
                "UPDATE tasks SET status=?3,current_session_id=?4,version=version+1,updated_at=?5
                 WHERE id=?1 AND version=?2",
                params![id, expected, next_status, session_id, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "task.claimed",
            Some(session_id),
            "task claimed",
            json!({"sessionId": session_id, "previousSessionId": previous, "takeOver": take_over}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_checkpoint(
        &self,
        reference: &str,
        expected: i64,
        session_id: &str,
        input_json: &str,
    ) -> AppResult<Outcome> {
        let input: CheckpointInput = serde_json::from_str(input_json)
            .map_err(|error| AppError::invalid("input", error.to_string()))?;
        let summary = required("summary", &input.summary)?;
        let next_step = required("nextStep", &input.next_step)?;
        validate_string_array("completed", &input.completed)
            .map_err(|(field, reason)| AppError::invalid(&field, reason))?;
        validate_string_array("decisions", &input.decisions)
            .map_err(|(field, reason)| AppError::invalid(&field, reason))?;
        validate_string_array("pending", &input.pending)
            .map_err(|(field, reason)| AppError::invalid(&field, reason))?;
        validate_string_array("risks", &input.risks)
            .map_err(|(field, reason)| AppError::invalid(&field, reason))?;
        let initial = self.connection()?;
        let task = load_task_by_reference(&initial, reference)?;
        let id = task.id;
        check_version(&task, expected)?;
        let git_head = if task.worktree_path.is_some() {
            self.worktree_status(&id.to_string())?.data["worktreeStatus"]["head"]
                .as_str()
                .map(ToOwned::to_owned)
        } else {
            None
        };
        drop(initial);
        let checkpoint_id = Uuid::new_v4().to_string();
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task(&tx, id)?;
        check_version(&task, expected)?;
        if !matches!(task.status, TaskStatus::InProgress | TaskStatus::Blocked)
            || task.current_session_id.as_deref() != Some(session_id)
        {
            return Err(AppError::session(
                task.current_session_id.as_deref(),
                session_id,
            ));
        }
        let session = load_session(&tx, session_id)?;
        if session.task_id != id || session.ended_at.is_some() {
            return Err(AppError::session(
                task.current_session_id.as_deref(),
                session_id,
            ));
        }
        tx.execute(
            "INSERT INTO checkpoints(
                id,task_id,session_id,summary,completed_json,decisions_json,pending_json,
                next_step,risks_json,git_head,created_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                checkpoint_id,
                id,
                session_id,
                summary,
                serde_json::to_string(&input.completed).unwrap(),
                serde_json::to_string(&input.decisions).unwrap(),
                serde_json::to_string(&input.pending).unwrap(),
                next_step,
                serde_json::to_string(&input.risks).unwrap(),
                git_head,
                timestamp
            ],
        )
        .map_err(AppError::from_sqlite)?;
        let changed = tx
            .execute(
                "UPDATE tasks SET latest_checkpoint_id=?3,next_step=?4,version=version+1,updated_at=?5
                 WHERE id=?1 AND version=?2",
                params![id, expected, checkpoint_id, next_step, timestamp],
            )
            .map_err(AppError::from_sqlite)?;
        if changed != 1 {
            return Err(AppError::version(expected, load_task(&tx, id)?.version));
        }
        insert_history(
            &tx,
            id,
            "checkpoint.saved",
            Some(session_id),
            "checkpoint saved",
            json!({"checkpointId": checkpoint_id, "sessionId": session_id, "gitHead": git_head}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        let response_checkpoint = tx
            .query_row(
                "SELECT id,task_id,session_id,summary,completed_json,decisions_json,pending_json,
                        next_step,risks_json,git_head,created_at FROM checkpoints WHERE id=?1",
                [&checkpoint_id],
                checkpoint_from_row,
            )
            .map_err(AppError::from_sqlite)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({
            "task": response_task,
            "checkpoint": response_checkpoint,
        })))
    }
}

const DEFAULT_TASK_PAGE_SIZE: u32 = 50;
const MAX_TASK_PAGE_SIZE: u32 = 200;
const TASK_LIST_FIELDS: [&str; 22] = [
    "id",
    "taskKey",
    "title",
    "status",
    "version",
    "goal",
    "scope",
    "acceptanceCriteria",
    "nextStep",
    "blockReason",
    "blockRecovery",
    "currentSessionId",
    "repositoryPath",
    "repositoryCommonDir",
    "repositoryBranch",
    "worktreePath",
    "latestCheckpointId",
    "closureOutcome",
    "closureReason",
    "closedAt",
    "createdAt",
    "updatedAt",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskListCursor {
    version: u8,
    updated_at: String,
    id: i64,
    filter_digest: String,
}

fn validate_list_fields(fields: &[String]) -> AppResult<Vec<String>> {
    let mut normalized = Vec::with_capacity(fields.len());
    for field in fields {
        if !TASK_LIST_FIELDS.contains(&field.as_str()) {
            return Err(AppError::invalid(
                "fields",
                format!("unknown Task field: {field}"),
            ));
        }
        if normalized.contains(field) {
            return Err(AppError::invalid(
                "fields",
                format!("duplicate Task field: {field}"),
            ));
        }
        normalized.push(field.clone());
    }
    Ok(normalized)
}

fn escaped_like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for character in query.chars() {
        if matches!(character, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

fn task_list_filter_digest(
    status: Option<&str>,
    task_key: Option<&str>,
    query: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    for value in [status, task_key, query] {
        match value {
            Some(value) => {
                hasher.update([1]);
                hasher.update((value.len() as u64).to_be_bytes());
                hasher.update(value.as_bytes());
            }
            None => hasher.update([0]),
        }
    }
    hex::encode(hasher.finalize())
}

fn encode_task_cursor(cursor: &TaskListCursor) -> String {
    hex::encode(serde_json::to_vec(cursor).expect("Task list cursor must serialize"))
}

fn decode_task_cursor(value: &str) -> AppResult<TaskListCursor> {
    let invalid = || AppError::invalid("cursor", "must be a cursor returned by task list");
    if value.is_empty() || value.len() > 4096 {
        return Err(invalid());
    }
    let bytes = hex::decode(value).map_err(|_| invalid())?;
    let cursor: TaskListCursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if cursor.version != 1
        || cursor.id <= 0
        || chrono::DateTime::parse_from_rfc3339(&cursor.updated_at).is_err()
    {
        return Err(invalid());
    }
    Ok(cursor)
}

fn project_tasks(tasks: Vec<TaskView>, fields: &[String]) -> Vec<Value> {
    tasks
        .into_iter()
        .map(|task| {
            let task = serde_json::to_value(task).expect("TaskView must serialize");
            if fields.is_empty() {
                return task;
            }
            let task = task
                .as_object()
                .expect("TaskView must serialize as an object");
            let projected = fields
                .iter()
                .map(|field| {
                    (
                        field.clone(),
                        task.get(field)
                            .expect("validated Task field must be serialized")
                            .clone(),
                    )
                })
                .collect();
            Value::Object(projected)
        })
        .collect()
}

fn required(field: &str, value: &str) -> AppResult<String> {
    require_non_empty(field, value).map_err(|(field, reason)| AppError::invalid(&field, reason))
}

fn optional_description(field: &str, value: Option<String>) -> AppResult<Option<String>> {
    value.map(|value| required(field, &value)).transpose()
}

fn validate_task_key(value: &str) -> AppResult<String> {
    let value = required("taskKey", value)?;
    let numeric = value.bytes().all(|byte| byte.is_ascii_digit());
    if numeric || value.starts_with('#') {
        return Err(AppError::invalid(
            "taskKey",
            "must not be purely numeric or start with #",
        ));
    }
    Ok(value)
}

fn validate_task_title(value: &str) -> AppResult<String> {
    const TASK_TYPES: [&str; 8] = [
        "功能", "设计", "修复", "优化", "发布", "探索", "文档", "研究",
    ];

    let value = required("title", value)?;
    let parts = value.split('｜').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(AppError::invalid(
            "title",
            "must use MMDD｜类型｜主题 with exactly two full-width separators",
        ));
    }
    let date = parts[0];
    if date.len() != 4 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::invalid("title", "MMDD must contain four digits"));
    }
    let month = date[..2].parse::<u8>().expect("validated ASCII digits");
    let day = date[2..].parse::<u8>().expect("validated ASCII digits");
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29,
        _ => 0,
    };
    if day == 0 || day > maximum_day {
        return Err(AppError::invalid(
            "title",
            "MMDD must be a valid month and day",
        ));
    }
    if !TASK_TYPES.contains(&parts[1]) {
        return Err(AppError::invalid(
            "title",
            "类型 must be 功能、设计、修复、优化、发布、探索、文档 or 研究",
        ));
    }
    if parts[2].is_empty() || parts[2].trim() != parts[2] {
        return Err(AppError::invalid(
            "title",
            "主题 must be non-empty without surrounding whitespace",
        ));
    }
    Ok(value)
}

fn ensure_mutable(task: &steward_core::TaskView) -> AppResult<()> {
    if task.status == TaskStatus::Closed {
        Err(AppError::constraint("task.closed.immutable"))
    } else {
        Ok(())
    }
}

fn apply_title_patch(
    patch: &Map<String, Value>,
    destination: &mut Option<String>,
    changed: &mut Vec<String>,
) -> AppResult<()> {
    if let Some(value) = patch.get("title") {
        if value.is_null() {
            if destination.is_some() {
                return Err(AppError::invalid("title", "cannot be cleared once set"));
            }
            return Ok(());
        }
        let value = Some(validate_task_title(value.as_str().ok_or_else(|| {
            AppError::invalid("title", "must be a string or null")
        })?)?);
        if destination != &value {
            *destination = value;
            changed.push("title".to_owned());
        }
    }
    Ok(())
}

fn apply_description_patch(
    patch: &Map<String, Value>,
    field: &str,
    destination: &mut Option<String>,
    changed: &mut Vec<String>,
) -> AppResult<()> {
    if let Some(value) = patch.get(field) {
        if value.is_null() {
            if destination.is_some() {
                return Err(AppError::invalid(field, "cannot be cleared once set"));
            }
            return Ok(());
        }
        let value = Some(required(
            field,
            value
                .as_str()
                .ok_or_else(|| AppError::invalid(field, "must be a string or null"))?,
        )?);
        if destination != &value {
            *destination = value;
            changed.push(field.to_owned());
        }
    }
    Ok(())
}
