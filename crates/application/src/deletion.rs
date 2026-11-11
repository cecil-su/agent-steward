//! Logical deletion is independent of Task status. History remains authoritative;
//! no schema migration, cascade, file deletion, Session close, or restore operation.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use storage_sqlite::now;

use crate::db::{
    bump_task, check_version, insert_history, load_task, load_task_for_audit,
    resolve_task_id_for_audit,
};
use crate::{AppError, AppResult, Outcome, Service};

pub(crate) const DELETED_EVENT: &str = "task.deleted";
// Use the same predicate for all normal task and unfiltered Session listings.
pub(crate) const LIVE_TASK: &str = "NOT EXISTS (SELECT 1 FROM history deletion WHERE deletion.task_id=tasks.id AND deletion.change_type='task.deleted')";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskDeletion {
    deleted_at: String,
    reason: String,
    previous_version: i64,
}

pub(crate) fn deletion(c: &Connection, id: i64) -> AppResult<Option<TaskDeletion>> {
    let raw: Option<String> = c.query_row(
        "SELECT payload_json FROM history WHERE task_id=?1 AND change_type=?2 ORDER BY sequence DESC LIMIT 1",
        params![id, DELETED_EVENT], |row| row.get(0),
    ).optional().map_err(AppError::from_sqlite)?;
    raw.map(|raw| {
        serde_json::from_str(&raw).map_err(|_| {
            AppError::new(
                "DATABASE_UNAVAILABLE",
                "stored task deletion is invalid",
                false,
                json!({"taskId": id}),
                10,
            )
        })
    })
    .transpose()
}

pub(crate) fn ensure_live(c: &Connection, id: i64) -> AppResult<()> {
    let deleted: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history WHERE task_id=?1 AND change_type=?2)",
            params![id, DELETED_EVENT],
            |row| row.get(0),
        )
        .map_err(AppError::from_sqlite)?;
    if deleted {
        return Err(AppError::new(
            "TASK_DELETED",
            "task was logically deleted; use the read-only audit entry",
            false,
            json!({"taskId": id, "auditPath": format!("/api/tasks/{id}/audit")}),
            4,
        ));
    }
    Ok(())
}

pub(crate) fn delete_confirmation(confirmed: bool, reason: &str) -> AppResult<()> {
    if !confirmed {
        return Err(AppError::invalid(
            "confirmed",
            "explicit deletion confirmation is required",
        ));
    }
    if reason.trim().is_empty() || reason.chars().count() > 4000 || reason.contains('\0') {
        return Err(AppError::invalid(
            "reason",
            "nonblank reason without NUL, at most 4000 characters required",
        ));
    }
    Ok(())
}

fn preview(c: &Connection, id: i64) -> AppResult<Value> {
    let task = load_task(c, id)?;
    let work = crate::work::load_work(c, id)?;
    let mut statement = c
        .prepare("SELECT id FROM sessions WHERE task_id=?1 AND ended_at IS NULL ORDER BY id")
        .map_err(AppError::from_sqlite)?;
    let open_sessions = statement
        .query_map([id], |r| r.get::<_, String>(0))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)?;
    let count = |sql: &str| -> AppResult<i64> {
        c.query_row(sql, [id], |r| r.get(0))
            .map_err(AppError::from_sqlite)
    };
    let mut statement = c
        .prepare(
            "SELECT project_id FROM project_profiles WHERE source_task_id=?1 ORDER BY project_id",
        )
        .map_err(AppError::from_sqlite)?;
    let profiles = statement
        .query_map([id], |r| r.get::<_, i64>(0))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)?;
    let mut statement = c.prepare("SELECT id FROM rules WHERE EXISTS (SELECT 1 FROM json_each(content_json,'$.sources') source WHERE json_extract(source.value,'$.taskId')=?1) ORDER BY id")
        .map_err(AppError::from_sqlite)?;
    let rules = statement
        .query_map([id], |r| r.get::<_, i64>(0))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)?;
    Ok(json!({
        "task": task, "mode": "logical", "preserveHistory": true,
        "blocked": !open_sessions.is_empty(), "openSessionIds": open_sessions,
        "sourceReferences": {"projectIds": profiles, "ruleIds": rules},
        "retainedCounts": {
            "requirements": work.as_ref().map_or(0, |w| w.requirements.len()),
            "confirmations": work.as_ref().map_or(0, |w| w.confirmations.len()),
            "notes": count("SELECT count(*) FROM task_notes WHERE task_id=?1")?,
            "checkpoints": count("SELECT count(*) FROM checkpoints WHERE task_id=?1")?,
            "history": count("SELECT count(*) FROM history WHERE task_id=?1")?,
            "sessions": count("SELECT count(*) FROM sessions WHERE task_id=?1")?,
            "imports": count("SELECT count(*) FROM session_imports WHERE session_id IN (SELECT id FROM sessions WHERE task_id=?1)")?,
            "events": count("SELECT count(*) FROM session_events WHERE session_id IN (SELECT id FROM sessions WHERE task_id=?1)")?
        }
    }))
}

impl Service {
    pub fn task_delete_preview(&self, reference: &str) -> AppResult<Outcome> {
        let mut c = self.context_connection()?;
        let tx = c.transaction().map_err(AppError::from_sqlite)?;
        let id = crate::db::resolve_task_id(&tx, reference)?;
        let data = preview(&tx, id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(data))
    }

    pub fn task_delete(
        &self,
        reference: &str,
        expected: i64,
        confirmed: bool,
        reason: &str,
    ) -> AppResult<Outcome> {
        delete_confirmation(confirmed, reason)?;
        let mut c = self.connection()?;
        let tx = storage_sqlite::write_transaction(&mut c).map_err(AppError::from_storage)?;
        let task = crate::db::load_task_by_reference(&tx, reference)?;
        check_version(&task, expected)?;
        // Re-evaluate the complete impact under the write lock, not a stale UI preview.
        let impact = preview(&tx, task.id)?;
        if impact["blocked"] == true {
            return Err(AppError::new(
                "DELETE_BLOCKED",
                "unfinished Sessions prevent task deletion; no Session was changed",
                false,
                json!({"taskId": task.id, "openSessionIds": impact["openSessionIds"]}),
                4,
            ));
        }
        let timestamp = now();
        bump_task(&tx, task.id, expected, &timestamp)?;
        let deleted = TaskDeletion {
            deleted_at: timestamp.clone(),
            reason: reason.to_owned(),
            previous_version: expected,
        };
        insert_history(
            &tx,
            task.id,
            DELETED_EVENT,
            None,
            "task logically deleted; history retained",
            serde_json::to_value(&deleted)
                .map_err(|_| AppError::invalid("deletion", "cannot encode"))?,
            &timestamp,
        )?;
        let task = load_task_for_audit(&tx, task.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"task": task, "deletion": deleted, "auditPath": format!("/api/tasks/{}/audit", task.id)}),
        ))
    }

    /// Explicit read-only audit access, including deleted tasks. Never revives a task.
    /// Full retained snapshots/notes/checkpoints are returned; Import BLOBs are not.
    pub fn task_audit(&self, reference: &str) -> AppResult<Outcome> {
        let mut c = self.context_connection()?;
        let tx = c.transaction().map_err(AppError::from_sqlite)?;
        let id = resolve_task_id_for_audit(&tx, reference)?;
        let task = load_task_for_audit(&tx, id)?;
        let deleted = deletion(&tx, id)?;
        let work = crate::work::load_work(&tx, id)?;
        let mut statement = tx.prepare("SELECT id,task_id,sequence,change_type,session_id,occurred_at,summary,payload_json FROM history WHERE task_id=?1 ORDER BY sequence")
            .map_err(AppError::from_sqlite)?;
        let history = statement
            .query_map([id], crate::db::history_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        drop(statement);
        let mut statement = tx.prepare("SELECT id,task_id,session_id,note_type,text,created_at FROM task_notes WHERE task_id=?1 ORDER BY id")
            .map_err(AppError::from_sqlite)?;
        let notes = statement
            .query_map([id], crate::db::note_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        drop(statement);
        let mut statement = tx.prepare("SELECT id,task_id,session_id,summary,completed_json,decisions_json,pending_json,next_step,risks_json,git_head,created_at FROM checkpoints WHERE task_id=?1 ORDER BY created_at,id")
            .map_err(AppError::from_sqlite)?;
        let checkpoints = statement
            .query_map([id], crate::db::checkpoint_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        drop(statement);
        let mut statement = tx.prepare("SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at FROM sessions WHERE task_id=?1 ORDER BY started_at,id")
            .map_err(AppError::from_sqlite)?;
        let sessions = statement
            .query_map([id], crate::db::session_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        drop(statement);
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"task": task, "deletion": deleted, "work": work, "history": history, "notes": notes, "checkpoints": checkpoints, "sessions": sessions}),
        ))
    }
}
