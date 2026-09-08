use rusqlite::{Connection, OptionalExtension, Row, Transaction};
use serde::de::DeserializeOwned;
use serde_json::Value;
use steward_core::{
    CheckpointView, HistoryEntry, SessionImportView, SessionView, TaskNoteView, TaskStatus,
    TaskView,
};

use crate::{AppError, AppResult};

pub(crate) fn resolve_task_id(connection: &Connection, reference: &str) -> AppResult<i64> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(AppError::invalid(
            "taskReference",
            "must be a numeric id, #id, or non-empty taskKey",
        ));
    }
    let numeric = if let Some(value) = reference.strip_prefix('#') {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AppError::invalid(
                "taskReference",
                "# must be followed by a positive numeric task id",
            ));
        }
        Some(value)
    } else if reference.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(reference)
    } else {
        None
    };
    if let Some(numeric) = numeric {
        let id = numeric
            .parse::<i64>()
            .map_err(|_| AppError::invalid("taskReference", "numeric task id is out of range"))?;
        if id <= 0 {
            return Err(AppError::invalid(
                "taskReference",
                "numeric task id must be positive",
            ));
        }
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        return exists
            .then_some(id)
            .ok_or_else(|| AppError::not_found("Task", reference));
    }
    connection
        .query_row(
            "SELECT id FROM tasks WHERE task_key=?1",
            [reference],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("Task", reference))
}

pub(crate) fn load_task_by_reference(
    connection: &Connection,
    reference: &str,
) -> AppResult<TaskView> {
    let id = resolve_task_id(connection, reference)?;
    load_task(connection, id)
}

pub(crate) fn load_task(connection: &Connection, id: i64) -> AppResult<TaskView> {
    connection
        .query_row(
            "SELECT id,task_key,title,status,version,goal,scope,acceptance_criteria,next_step,
                    block_reason,block_recovery,current_session_id,repository_path,
                    repository_common_dir,repository_branch,worktree_path,latest_checkpoint_id,
                    closure_outcome,closure_reason,closed_at,created_at,updated_at,project_id,
                    (SELECT json_group_array(component_id) FROM (SELECT component_id FROM task_components WHERE task_id=tasks.id ORDER BY component_id))
             FROM tasks WHERE id=?1",
            [id],
            task_from_row,
        )
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("Task", &id.to_string()))
}

pub(crate) fn task_from_row(row: &Row<'_>) -> rusqlite::Result<TaskView> {
    let status: String = row.get(3)?;
    let status = TaskStatus::try_from(status.as_str()).map_err(|message| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )),
        )
    })?;
    Ok(TaskView {
        id: row.get(0)?,
        task_key: row.get(1)?,
        title: row.get(2)?,
        status,
        version: row.get(4)?,
        goal: row.get(5)?,
        scope: row.get(6)?,
        acceptance_criteria: row.get(7)?,
        next_step: row.get(8)?,
        block_reason: row.get(9)?,
        block_recovery: row.get(10)?,
        current_session_id: row.get(11)?,
        repository_path: row.get(12)?,
        repository_common_dir: row.get(13)?,
        repository_branch: row.get(14)?,
        worktree_path: row.get(15)?,
        latest_checkpoint_id: row.get(16)?,
        closure_outcome: row.get(17)?,
        closure_reason: row.get(18)?,
        closed_at: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
        project_id: row.get(22)?,
        component_ids: json_from_column(23, "task_components", &row.get::<_, String>(23)?)?,
    })
}

pub(crate) fn load_session(connection: &Connection, id: &str) -> AppResult<SessionView> {
    connection
        .query_row(
            "SELECT id,task_id,source,external_session_id,continued_from,record_path,started_at,ended_at
             FROM sessions WHERE id=?1",
            [id],
            session_from_row,
        )
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("Session", id))
}

pub(crate) fn session_from_row(row: &Row<'_>) -> rusqlite::Result<SessionView> {
    Ok(SessionView {
        id: row.get(0)?,
        task_id: row.get(1)?,
        source: row.get(2)?,
        external_session_id: row.get(3)?,
        continued_from: row.get(4)?,
        record_path: row.get(5)?,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
    })
}

pub(crate) fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<CheckpointView> {
    let completed: String = row.get(4)?;
    let decisions: String = row.get(5)?;
    let pending: String = row.get(6)?;
    let risks: String = row.get(8)?;
    Ok(CheckpointView {
        id: row.get(0)?,
        task_id: row.get(1)?,
        session_id: row.get(2)?,
        summary: row.get(3)?,
        completed: json_from_column(4, "checkpoints.completed_json", &completed)?,
        decisions: json_from_column(5, "checkpoints.decisions_json", &decisions)?,
        pending: json_from_column(6, "checkpoints.pending_json", &pending)?,
        next_step: row.get(7)?,
        risks: json_from_column(8, "checkpoints.risks_json", &risks)?,
        git_head: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub(crate) fn note_from_row(row: &Row<'_>) -> rusqlite::Result<TaskNoteView> {
    Ok(TaskNoteView {
        id: row.get(0)?,
        task_id: row.get(1)?,
        session_id: row.get(2)?,
        note_type: row.get(3)?,
        text: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub(crate) fn import_from_row(row: &Row<'_>) -> rusqlite::Result<SessionImportView> {
    Ok(SessionImportView {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source_path: row.get(2)?,
        media_type: row.get(3)?,
        sha256: row.get(4)?,
        size_bytes: row.get(5)?,
        imported_at: row.get(6)?,
    })
}

pub(crate) fn history_from_row(row: &Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let payload: Option<String> = row.get(7)?;
    Ok(HistoryEntry {
        id: row.get(0)?,
        task_id: row.get(1)?,
        sequence: row.get(2)?,
        change_type: row.get(3)?,
        session_id: row.get(4)?,
        occurred_at: row.get(5)?,
        summary: row.get(6)?,
        payload: payload
            .map(|value| json_from_column(7, "history.payload_json", &value))
            .transpose()?,
    })
}

fn json_from_column<T: DeserializeOwned>(
    column: usize,
    name: &str,
    value: &str,
) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid JSON in {name}: {error}"),
            )),
        )
    })
}

pub(crate) fn check_version(task: &TaskView, expected: i64) -> AppResult<()> {
    if task.version == expected {
        Ok(())
    } else {
        Err(AppError::version(expected, task.version))
    }
}

pub(crate) fn bump_task(
    tx: &Transaction<'_>,
    task_id: i64,
    expected: i64,
    now: &str,
) -> AppResult<()> {
    let changed = tx
        .execute(
            "UPDATE tasks SET version=version+1, updated_at=?3 WHERE id=?1 AND version=?2",
            (task_id, expected, now),
        )
        .map_err(AppError::from_sqlite)?;
    if changed == 1 {
        Ok(())
    } else {
        let current = load_task(tx, task_id)?;
        Err(AppError::version(expected, current.version))
    }
}

pub(crate) fn insert_history(
    tx: &Transaction<'_>,
    task_id: i64,
    change_type: &str,
    session_id: Option<&str>,
    summary: &str,
    payload: Value,
    occurred_at: &str,
) -> AppResult<()> {
    let sequence: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM history WHERE task_id=?1",
            [task_id],
            |row| row.get(0),
        )
        .map_err(AppError::from_sqlite)?;
    tx.execute(
        "INSERT INTO history(task_id,sequence,change_type,session_id,occurred_at,summary,payload_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        (
            task_id,
            sequence,
            change_type,
            session_id,
            occurred_at,
            summary,
            serde_json::to_string(&payload).unwrap(),
        ),
    )
    .map_err(AppError::from_sqlite)?;
    Ok(())
}
