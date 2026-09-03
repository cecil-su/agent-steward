use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Map, Value, json};
use steward_core::{
    CheckpointInput, TaskCreateInput, TaskStatus, require_non_empty, validate_string_array,
};
use storage_sqlite::now;
use uuid::Uuid;

use crate::db::{
    bump_task, check_version, checkpoint_from_row, insert_history, load_session, load_task,
    note_from_row, task_from_row,
};
use crate::{AppError, AppResult, Outcome, Service};

impl Service {
    pub fn task_create(&self, id: &str, input_json: &str) -> AppResult<Outcome> {
        require_non_empty("id", id).map_err(|(field, reason)| AppError::invalid(&field, reason))?;
        let input: TaskCreateInput = serde_json::from_str(input_json)
            .map_err(|error| AppError::invalid("input", error.to_string()))?;
        let title = required("title", &input.title)?;
        let goal = required("goal", &input.goal)?;
        let scope = required("scope", &input.scope)?;
        let acceptance = required("acceptanceCriteria", &input.acceptance_criteria)?;
        let next_step = match input.next_step {
            Some(value) => Some(required("nextStep", &value)?),
            None => None,
        };
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        tx.execute(
            "INSERT INTO tasks(
                id,title,status,version,goal,scope,acceptance_criteria,next_step,
                created_at,updated_at
             ) VALUES (?1,?2,'open',1,?3,?4,?5,?6,?7,?7)",
            params![id, title, goal, scope, acceptance, next_step, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        insert_history(
            &tx,
            id,
            "task.created",
            None,
            "task created",
            json!({"title": title}),
            &timestamp,
        )?;
        let response_task = load_task(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response_task})))
    }

    pub fn task_show(&self, id: &str) -> AppResult<Outcome> {
        let connection = self.connection()?;
        let task = load_task(&connection, id)?;
        Ok(Outcome::new(json!({"task": task})))
    }

    pub fn task_list(&self, status: Option<&str>) -> AppResult<Outcome> {
        if let Some(status) = status {
            TaskStatus::try_from(status).map_err(|reason| AppError::invalid("status", reason))?;
        }
        let connection = self.connection()?;
        let mut statement = if status.is_some() {
            connection
                .prepare(
                    "SELECT id,title,status,version,goal,scope,acceptance_criteria,next_step,
                            block_reason,block_recovery,current_session_id,repository_path,
                            repository_common_dir,repository_branch,worktree_path,latest_checkpoint_id,
                            closure_outcome,closure_reason,closed_at,created_at,updated_at
                     FROM tasks WHERE status=?1 ORDER BY updated_at DESC,id ASC",
                )
                .map_err(AppError::from_sqlite)?
        } else {
            connection
                .prepare(
                    "SELECT id,title,status,version,goal,scope,acceptance_criteria,next_step,
                            block_reason,block_recovery,current_session_id,repository_path,
                            repository_common_dir,repository_branch,worktree_path,latest_checkpoint_id,
                            closure_outcome,closure_reason,closed_at,created_at,updated_at
                     FROM tasks ORDER BY updated_at DESC,id ASC",
                )
                .map_err(AppError::from_sqlite)?
        };
        let tasks = if let Some(status) = status {
            statement
                .query_map([status], task_from_row)
                .map_err(AppError::from_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from_sqlite)?
        } else {
            statement
                .query_map([], task_from_row)
                .map_err(AppError::from_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from_sqlite)?
        };
        Ok(Outcome::new(json!({"tasks": tasks})))
    }

    pub fn task_update(&self, id: &str, expected: i64, input_json: &str) -> AppResult<Outcome> {
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
                "title" | "goal" | "scope" | "acceptanceCriteria" | "nextStep"
            ) {
                return Err(AppError::invalid(key, "unknown patch field"));
            }
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let mut task = load_task(&tx, id)?;
        check_version(&task, expected)?;
        ensure_mutable(&task)?;
        let mut changed = Vec::new();
        apply_required_patch(patch, "title", &mut task.title, &mut changed)?;
        apply_required_patch(patch, "goal", &mut task.goal, &mut changed)?;
        apply_required_patch(patch, "scope", &mut task.scope, &mut changed)?;
        apply_required_patch(
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
                "UPDATE tasks SET title=?3,goal=?4,scope=?5,acceptance_criteria=?6,next_step=?7,
                    version=version+1,updated_at=?8 WHERE id=?1 AND version=?2",
                params![
                    id,
                    expected,
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

    pub fn task_note(
        &self,
        id: &str,
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
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let task = load_task(&tx, id)?;
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
        id: &str,
        expected: i64,
        reason: &str,
        recovery: &str,
    ) -> AppResult<Outcome> {
        let reason = required("reason", reason)?;
        let recovery = required("recovery", recovery)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let task = load_task(&tx, id)?;
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

    pub fn task_unblock(&self, id: &str, expected: i64, next_step: &str) -> AppResult<Outcome> {
        let next_step = required("nextStep", next_step)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let task = load_task(&tx, id)?;
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
        id: &str,
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
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let task = load_task(&tx, id)?;
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
        id: &str,
        expected: i64,
        session_id: &str,
        take_over: bool,
    ) -> AppResult<Outcome> {
        required("session", session_id)?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
        let task = load_task(&tx, id)?;
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
        id: &str,
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
        let task = load_task(&initial, id)?;
        check_version(&task, expected)?;
        let git_head = if task.worktree_path.is_some() {
            self.worktree_status(id)?.data["worktreeStatus"]["head"]
                .as_str()
                .map(ToOwned::to_owned)
        } else {
            None
        };
        drop(initial);
        let checkpoint_id = Uuid::new_v4().to_string();
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(AppError::from_sqlite)?;
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

fn required(field: &str, value: &str) -> AppResult<String> {
    require_non_empty(field, value).map_err(|(field, reason)| AppError::invalid(&field, reason))
}

fn ensure_mutable(task: &steward_core::TaskView) -> AppResult<()> {
    if task.status == TaskStatus::Closed {
        Err(AppError::constraint("task.closed.immutable"))
    } else {
        Ok(())
    }
}

fn apply_required_patch(
    patch: &Map<String, Value>,
    field: &str,
    destination: &mut String,
    changed: &mut Vec<String>,
) -> AppResult<()> {
    if let Some(value) = patch.get(field) {
        let value = value
            .as_str()
            .ok_or_else(|| AppError::invalid(field, "must be a non-null string"))?;
        let value = required(field, value)?;
        if *destination != value {
            *destination = value;
            changed.push(field.to_owned());
        }
    }
    Ok(())
}
