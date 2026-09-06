//! Client observations never grant execution authority or mutate Task progress.
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use steward_core::TaskStatus;
use storage_sqlite::now;

use crate::db::{bump_task, check_version, insert_history, load_session, load_task};
use crate::{AppError, AppResult, Outcome, Service, warning};

const EVENT_LIMIT: i64 = 10_000;
pub const INPUT_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookEventInput {
    pub schema_version: u32,
    pub session_id: String,
    pub source: String,
    pub external_session_id: String,
    pub event_id: String,
    pub kind: String,
    pub occurred_at: String,
}

fn identifier(field: &str, value: &str, limit: usize) -> AppResult<()> {
    if value.is_empty()
        || value.len() > limit
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
        || value.contains("://")
    {
        return Err(AppError::invalid(
            field,
            "requires a bounded ASCII identifier",
        ));
    }
    Ok(())
}

impl Service {
    /// One-time explicit binding. Unlike attach, this may fill an existing Session.
    pub fn session_bind(
        &self,
        session_id: &str,
        expected: i64,
        source: &str,
        external: &str,
    ) -> AppResult<Outcome> {
        identifier("sessionId", session_id, 128)?;
        identifier("source", source, 32)?;
        identifier("externalSessionId", external, 128)?;
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let session = load_session(&tx, session_id)?;
        let task = load_task(&tx, session.task_id)?;
        check_version(&task, expected)?;
        if task.status == TaskStatus::Closed || session.ended_at.is_some() {
            return Err(AppError::constraint("session.bind.closed"));
        }
        if session.source.as_deref() == Some(source)
            && session.external_session_id.as_deref() == Some(external)
        {
            return Ok(Outcome::new(json!({"task": task, "session": session})));
        }
        if session.external_session_id.is_some()
            || session
                .source
                .as_deref()
                .is_some_and(|existing| existing != source)
        {
            return Err(AppError::session(
                task.current_session_id.as_deref(),
                session_id,
            ));
        }
        let used: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE source=?1 AND external_session_id=?2)",
                params![source, external],
                |row| row.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        if used {
            return Err(AppError::constraint("sessions.external_identity.unique"));
        }
        tx.execute(
            "UPDATE sessions SET source=?2,external_session_id=?3 WHERE id=?1",
            params![session_id, source, external],
        )
        .map_err(AppError::from_sqlite)?;
        let timestamp = now();
        bump_task(&tx, task.id, expected, &timestamp)?;
        insert_history(
            &tx,
            task.id,
            "session.bound",
            Some(session_id),
            "session source bound",
            json!({"sessionId":session_id,"source":source,"externalSessionId":external}),
            &timestamp,
        )?;
        let task = load_task(&tx, task.id)?;
        let session = load_session(&tx, session_id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": task, "session": session})))
    }

    pub fn hook_ingest(&self, input: &str) -> AppResult<Outcome> {
        if input.len() > INPUT_LIMIT {
            return Err(AppError::invalid("input", "event exceeds 16 KiB"));
        }
        // Never echo deserializer errors: they can contain arbitrary untrusted field values.
        let mut event: HookEventInput = serde_json::from_str(input)
            .map_err(|_| AppError::invalid("input", "invalid hook event JSON or unknown fields"))?;
        if event.schema_version != 1 {
            return Err(AppError::invalid("schemaVersion", "expected 1"));
        }
        identifier("sessionId", &event.session_id, 128)?;
        identifier("source", &event.source, 32)?;
        identifier("externalSessionId", &event.external_session_id, 128)?;
        identifier("eventId", &event.event_id, 128)?;
        if ![
            "started",
            "resumed",
            "idle",
            "closed",
            "user_message",
            "assistant_message",
            "tool_call",
            "tool_result",
            "error",
        ]
        .contains(&event.kind.as_str())
        {
            return Err(AppError::invalid("kind", "unsupported observation kind"));
        }
        event.occurred_at = DateTime::parse_from_rfc3339(&event.occurred_at)
            .map_err(|_| AppError::invalid("occurredAt", "expected RFC3339 with timezone"))?
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Nanos, true);
        let fingerprint = hex::encode(Sha256::digest(
            serde_json::to_vec(&event).map_err(|_| AppError::invalid("input", "invalid event"))?,
        ));
        let mut connection = self.connection()?;
        connection
            .busy_timeout(std::time::Duration::from_millis(100))
            .map_err(AppError::from_sqlite)?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let session = load_session(&tx, &event.session_id)?;
        if session.source.as_deref() != Some(&event.source)
            || session.external_session_id.as_deref() != Some(&event.external_session_id)
        {
            return Err(AppError::constraint("hook.session_binding_mismatch"));
        }
        let existing: Option<(i64, String, bool)> = tx.query_row("SELECT sequence,fingerprint,kind IS NULL FROM session_events WHERE session_id=?1 AND event_id=?2", params![event.session_id,event.event_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(AppError::from_sqlite)?;
        if let Some((sequence, hash, deleted)) = existing {
            if hash != fingerprint {
                return Err(AppError::new(
                    "HOOK_EVENT_CONFLICT",
                    "event ID already has different content",
                    false,
                    json!({"sequence":sequence}),
                    4,
                ));
            }
            return Ok(Outcome::new(
                json!({"sequence":sequence,"duplicate":true,"deleted":deleted}),
            ));
        }
        let count: i64 = tx
            .query_row(
                "SELECT count(*) FROM session_events WHERE session_id=?1",
                [&event.session_id],
                |r| r.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        if count >= EVENT_LIMIT {
            return Err(AppError::new(
                "HOOK_CAPACITY_REACHED",
                "session observation capacity reached",
                false,
                json!({"limit":EVENT_LIMIT}),
                4,
            ));
        }
        tx.execute("INSERT INTO session_events(session_id,event_id,fingerprint,kind,occurred_at,received_at) VALUES (?1,?2,?3,?4,?5,?6)",params![event.session_id,event.event_id,fingerprint,event.kind,event.occurred_at,now()]).map_err(AppError::from_sqlite)?;
        let sequence = tx.last_insert_rowid();
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"sequence":sequence,"duplicate":false,"deleted":false}),
        ))
    }

    pub fn hook_list(&self, session_id: &str, after: i64, limit: u32) -> AppResult<Outcome> {
        if after < 0 || !(1..=200).contains(&limit) {
            return Err(AppError::invalid(
                "pagination",
                "after >= 0 and limit 1..200 required",
            ));
        }
        let connection = self.connection()?;
        load_session(&connection, session_id)?;
        let mut statement = connection.prepare("SELECT sequence,event_id,kind,occurred_at,received_at FROM session_events WHERE session_id=?1 AND sequence>?2 AND kind IS NOT NULL ORDER BY sequence LIMIT ?3").map_err(AppError::from_sqlite)?;
        let mut events: Vec<serde_json::Value> = statement.query_map(params![session_id,after,limit + 1], |r| Ok(json!({"sequence":r.get::<_,i64>(0)?,"eventId":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"occurredAt":r.get::<_,String>(3)?,"receivedAt":r.get::<_,String>(4)?}))).map_err(AppError::from_sqlite)?.collect::<Result<_,_>>().map_err(AppError::from_sqlite)?;
        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        let next = events.last().and_then(|v| v["sequence"].as_i64());
        Ok(Outcome::new(
            json!({"sessionId":session_id,"events":events,"hasMore":has_more,"nextAfter":next}),
        ))
    }

    pub fn hook_clear(&self, session_id: &str, expected: i64) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        connection
            .pragma_update(None, "secure_delete", "ON")
            .map_err(AppError::from_sqlite)?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let session = load_session(&tx, session_id)?;
        let task = load_task(&tx, session.task_id)?;
        check_version(&task, expected)?;
        let removed = tx.execute("UPDATE session_events SET kind=NULL,occurred_at=NULL,received_at=NULL WHERE session_id=?1 AND kind IS NOT NULL",[session_id]).map_err(AppError::from_sqlite)?;
        if removed == 0 {
            return Ok(Outcome::new(json!({"task":task,"removed":0})));
        }
        let timestamp = now();
        bump_task(&tx, task.id, expected, &timestamp)?;
        insert_history(
            &tx,
            task.id,
            "session.observations_cleared",
            Some(session_id),
            "session observations cleared",
            json!({"sessionId":session_id,"removed":removed}),
            &timestamp,
        )?;
        let task = load_task(&tx, task.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        let checkpoint: Result<i64, _> =
            connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0));
        let mut outcome = Outcome::new(json!({"task":task,"removed":removed}));
        if !matches!(checkpoint, Ok(0)) {
            outcome.warnings.push(warning(
                "PHYSICAL_ERASURE_NOT_GUARANTEED",
                "logical deletion complete; WAL checkpoint could not be completed",
                json!({"sessionId":session_id}),
            ));
        }
        Ok(outcome)
    }
}
