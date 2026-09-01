use rusqlite::{Connection, OptionalExtension, Row, Transaction};
use serde_json::Value;
use steward_core::{
    CheckpointView, HistoryEntry, SessionImportView, SessionView, TaskNoteView, TaskStatus,
    TaskView,
};

use crate::{AppError, AppResult};

pub(crate) fn load_task(connection: &Connection, id: &str) -> AppResult<TaskView> {
    connection
        .query_row(
            "SELECT id,title,status,version,goal,scope,acceptance_criteria,next_step,
                    block_reason,block_recovery,current_session_id,repository_path,
                    repository_common_dir,repository_branch,worktree_path,latest_checkpoint_id,
                    closure_outcome,closure_reason,closed_at,created_at,updated_at
             FROM tasks WHERE id=?1",
            [id],
            task_from_row,
        )
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("Task", id))
}

pub(crate) fn task_from_row(row: &Row<'_>) -> rusqlite::Result<TaskView> {
    let status: String = row.get(2)?;
    let status = TaskStatus::try_from(status.as_str()).map_err(|message| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )),
        )
    })?;
    Ok(TaskView {
        id: row.get(0)?,
        title: row.get(1)?,
        status,
        version: row.get(3)?,
        goal: row.get(4)?,
        scope: row.get(5)?,
        acceptance_criteria: row.get(6)?,
        next_step: row.get(7)?,
        block_reason: row.get(8)?,
        block_recovery: row.get(9)?,
        current_session_id: row.get(10)?,
        repository_path: row.get(11)?,
        repository_common_dir: row.get(12)?,
        repository_branch: row.get(13)?,
        worktree_path: row.get(14)?,
        latest_checkpoint_id: row.get(15)?,
        closure_outcome: row.get(16)?,
        closure_reason: row.get(17)?,
        closed_at: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
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
        completed: serde_json::from_str(&completed).unwrap_or_default(),
        decisions: serde_json::from_str(&decisions).unwrap_or_default(),
        pending: serde_json::from_str(&pending).unwrap_or_default(),
        next_step: row.get(7)?,
        risks: serde_json::from_str(&risks).unwrap_or_default(),
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
        payload: payload.and_then(|value| serde_json::from_str(&value).ok()),
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
    task_id: &str,
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
    task_id: &str,
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
