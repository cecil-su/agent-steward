use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use thiserror::Error;

pub const SCHEMA_VERSION: i64 = 2;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(
        "unsupported database schema ({database_version}); use a new database for this build (schema {supported_version})"
    )]
    UnsupportedSchema {
        database_version: i64,
        supported_version: i64,
    },
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn open_database(path: &Path) -> Result<Connection, StorageError> {
    let mut parent_created = false;
    let parent = path.parent().map(|parent| {
        if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        }
    });
    if let Some(parent) = parent
        && !parent.try_exists()?
    {
        fs::create_dir_all(parent)?;
        parent_created = true;
    }
    if let Some(parent) = parent
        && (parent_created || steward_core::default_data_dir().as_deref() == Some(parent))
    {
        steward_core::set_private_dir(parent)?;
    }
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    steward_core::set_private_file(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    initialize(&mut connection)?;
    enable_wal(&connection)?;
    steward_core::set_private_file(&sidecar_path(path, "-wal"))?;
    steward_core::set_private_file(&sidecar_path(path, "-shm"))?;
    Ok(connection)
}

fn enable_wal(connection: &Connection) -> Result<(), StorageError> {
    // SQLite may skip its busy handler when journal-mode lock promotion would
    // deadlock. Retry only this startup configuration, with no live transaction
    // or statement and one deadline; business writes are never replayed here.
    let deadline = Instant::now() + BUSY_TIMEOUT;
    connection.busy_timeout(Duration::ZERO)?;
    loop {
        let result = (|| -> rusqlite::Result<()> {
            let journal_mode: String =
                connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
            if journal_mode != "wal" {
                connection.pragma_update(None, "journal_mode", "WAL")?;
            }
            Ok(())
        })();
        match result {
            Err(ref error)
                if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy)
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            result => {
                connection.busy_timeout(BUSY_TIMEOUT)?;
                return result.map_err(StorageError::from);
            }
        }
    }
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub fn write_transaction(connection: &mut Connection) -> Result<Transaction<'_>, StorageError> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_current_schema(schema_version(&tx)?)?;
    Ok(tx)
}

// Current databases need only a read. Serialize only the first initialization,
// then recheck after acquiring the writer lock to handle concurrent first opens.
fn initialize(connection: &mut Connection) -> Result<(), StorageError> {
    let version = schema_version(connection)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version != 0 {
        return require_current_schema(version);
    }
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = schema_version(&tx)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let populated: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%')",
        [],
        |row| row.get(0),
    )?;
    if version != 0 || populated {
        return Err(StorageError::UnsupportedSchema {
            database_version: version,
            supported_version: SCHEMA_VERSION,
        });
    }
    tx.execute_batch(SCHEMA)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

pub fn schema_version(connection: &Connection) -> Result<i64, StorageError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn require_current_schema(version: i64) -> Result<(), StorageError> {
    if version != SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            database_version: version,
            supported_version: SCHEMA_VERSION,
        });
    }
    Ok(())
}

pub fn canonical_database_path(path: &Path) -> Result<PathBuf, StorageError> {
    if path.try_exists()? {
        Ok(fs::canonicalize(path)?)
    } else {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        Ok(absolute)
    }
}

const SCHEMA: &str = r#"
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_key TEXT NULL UNIQUE CHECK(task_key IS NULL OR length(trim(task_key)) > 0),
    title TEXT NULL CHECK(title IS NULL OR length(trim(title)) > 0),
    status TEXT NOT NULL CHECK(status IN ('open','in_progress','blocked','closed')),
    version INTEGER NOT NULL CHECK(version >= 1),
    goal TEXT NULL CHECK(goal IS NULL OR length(trim(goal)) > 0),
    scope TEXT NULL CHECK(scope IS NULL OR length(trim(scope)) > 0),
    acceptance_criteria TEXT NULL CHECK(acceptance_criteria IS NULL OR length(trim(acceptance_criteria)) > 0),
    next_step TEXT NULL CHECK(next_step IS NULL OR length(trim(next_step)) > 0),
    block_reason TEXT NULL,
    block_recovery TEXT NULL,
    current_session_id TEXT NULL,
    repository_path TEXT NULL,
    repository_common_dir TEXT NULL,
    repository_branch TEXT NULL,
    worktree_path TEXT NULL,
    latest_checkpoint_id TEXT NULL,
    closure_outcome TEXT NULL CHECK(closure_outcome IS NULL OR closure_outcome IN ('completed','partial','cancelled','superseded')),
    closure_reason TEXT NULL,
    closed_at TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(id, current_session_id),
    CHECK (
        (status = 'blocked'
            AND block_reason IS NOT NULL AND length(trim(block_reason)) > 0
            AND block_recovery IS NOT NULL AND length(trim(block_recovery)) > 0)
        OR
        (status != 'blocked' AND block_reason IS NULL AND block_recovery IS NULL)
    ),
    CHECK (
        (status = 'closed'
            AND closure_outcome IS NOT NULL
            AND closed_at IS NOT NULL AND length(trim(closed_at)) > 0
            AND (closure_reason IS NULL OR length(trim(closure_reason)) > 0)
            AND (closure_outcome = 'completed' OR closure_reason IS NOT NULL)
            AND (closure_outcome != 'completed' OR (
                title IS NOT NULL AND goal IS NOT NULL AND scope IS NOT NULL
                AND acceptance_criteria IS NOT NULL)))
        OR
        (status != 'closed'
            AND closure_outcome IS NULL AND closure_reason IS NULL AND closed_at IS NULL)
    ),
    CHECK (status != 'closed' OR (current_session_id IS NULL AND next_step IS NULL AND block_reason IS NULL AND block_recovery IS NULL)),
    CHECK ((repository_path IS NULL AND repository_common_dir IS NULL AND repository_branch IS NULL AND worktree_path IS NULL)
        OR (repository_path IS NOT NULL AND repository_common_dir IS NOT NULL AND repository_branch IS NOT NULL AND worktree_path IS NOT NULL)),
    FOREIGN KEY(current_session_id, id) REFERENCES sessions(id, task_id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(latest_checkpoint_id, id) REFERENCES checkpoints(id, task_id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    source TEXT NULL,
    external_session_id TEXT NULL,
    continued_from TEXT NULL,
    record_path TEXT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NULL,
    UNIQUE(id, task_id),
    CHECK(external_session_id IS NULL OR (source IS NOT NULL AND length(trim(source)) > 0)),
    CHECK(continued_from IS NULL OR continued_from != id),
    FOREIGN KEY(continued_from, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE checkpoints (
    id TEXT PRIMARY KEY,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    session_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    completed_json TEXT NOT NULL,
    decisions_json TEXT NOT NULL,
    pending_json TEXT NOT NULL,
    next_step TEXT NOT NULL,
    risks_json TEXT NOT NULL,
    git_head TEXT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(id, task_id),
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE task_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    session_id TEXT NULL,
    note_type TEXT NOT NULL CHECK(note_type IN ('decision','progress','risk')),
    text TEXT NOT NULL CHECK(length(trim(text)) > 0),
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    sequence INTEGER NOT NULL,
    change_type TEXT NOT NULL,
    session_id TEXT NULL,
    occurred_at TEXT NOT NULL,
    summary TEXT NOT NULL,
    payload_json TEXT NULL,
    UNIQUE(task_id, sequence),
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE session_imports (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    source_path TEXT NOT NULL,
    media_type TEXT NULL,
    sha256 TEXT NOT NULL,
    content BLOB NOT NULL,
    imported_at TEXT NOT NULL,
    UNIQUE(session_id, sha256)
);

CREATE TABLE session_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    event_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    kind TEXT NULL CHECK(kind IS NULL OR kind IN ('started','resumed','idle','closed','user_message','assistant_message','tool_call','tool_result','error')),
    occurred_at TEXT NULL,
    received_at TEXT NULL,
    UNIQUE(session_id, event_id),
    CHECK ((kind IS NULL AND occurred_at IS NULL AND received_at IS NULL)
        OR (kind IS NOT NULL AND occurred_at IS NOT NULL AND received_at IS NOT NULL))
);
CREATE INDEX idx_session_events_sequence ON session_events(session_id, sequence);

CREATE INDEX idx_tasks_status_updated ON tasks(status, updated_at);
CREATE INDEX idx_tasks_repo_updated ON tasks(repository_common_dir, updated_at);
CREATE INDEX idx_sessions_task_started ON sessions(task_id, started_at);
CREATE UNIQUE INDEX idx_sessions_identity ON sessions(id, task_id);
CREATE UNIQUE INDEX idx_sessions_external ON sessions(source, external_session_id) WHERE external_session_id IS NOT NULL;
CREATE INDEX idx_checkpoints_task_created ON checkpoints(task_id, created_at);
CREATE UNIQUE INDEX idx_checkpoints_identity ON checkpoints(id, task_id);
CREATE INDEX idx_notes_task_created ON task_notes(task_id, created_at);
CREATE INDEX idx_history_task_sequence ON history(task_id, sequence);
CREATE INDEX idx_imports_session_time ON session_imports(session_id, imported_at);

CREATE TRIGGER tasks_id_immutable
BEFORE UPDATE OF id ON tasks
WHEN NEW.id != OLD.id
BEGIN
    SELECT RAISE(ABORT, 'task id is immutable');
END;

CREATE TRIGGER tasks_key_immutable
BEFORE UPDATE OF task_key ON tasks
WHEN OLD.task_key IS NOT NULL AND NEW.task_key IS NOT OLD.task_key
BEGIN
    SELECT RAISE(ABORT, 'task key can only be set once');
END;

CREATE TRIGGER tasks_descriptions_not_cleared
BEFORE UPDATE OF title, goal, scope, acceptance_criteria ON tasks
WHEN (OLD.title IS NOT NULL AND NEW.title IS NULL)
    OR (OLD.goal IS NOT NULL AND NEW.goal IS NULL)
    OR (OLD.scope IS NOT NULL AND NEW.scope IS NULL)
    OR (OLD.acceptance_criteria IS NOT NULL AND NEW.acceptance_criteria IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'task descriptions cannot be cleared once set');
END;

CREATE TRIGGER tasks_current_session_same_task
BEFORE UPDATE OF current_session_id ON tasks
WHEN NEW.current_session_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM sessions s
        WHERE s.id = NEW.current_session_id AND s.task_id = NEW.id AND s.ended_at IS NULL
    ) THEN RAISE(ABORT, 'current session must be active and belong to task') END;
END;

CREATE TRIGGER tasks_latest_checkpoint_same_task
BEFORE UPDATE OF latest_checkpoint_id ON tasks
WHEN NEW.latest_checkpoint_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM checkpoints c WHERE c.id = NEW.latest_checkpoint_id AND c.task_id = NEW.id
    ) THEN RAISE(ABORT, 'latest checkpoint must belong to task') END;
END;

"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_switch_waits_for_an_existing_writer() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("wal.db");
        let mut writer = Connection::open(&path).unwrap();
        initialize(&mut writer).unwrap();
        let tx = writer
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(schema_version(&tx).unwrap(), SCHEMA_VERSION);
        tx.execute(
            "INSERT INTO tasks(status,version,created_at,updated_at) VALUES ('open',1,'now','now')",
            [],
        )
        .unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let result = open_database(&path);
                send.send(result).unwrap();
            });
            assert!(matches!(
                receive.recv_timeout(std::time::Duration::from_millis(100)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            tx.commit().unwrap();
            let connection = receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            let mode: String = connection
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
            assert_eq!(
                connection
                    .query_row("SELECT count(*) FROM tasks", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            handle.join().unwrap();
        });
    }

    #[test]
    fn concurrent_first_open_uses_one_schema_and_wal() {
        let temp = tempfile::tempdir().unwrap();
        for attempt in 0..8 {
            let path = temp.path().join(format!("concurrent-{attempt}.db"));
            let start = std::sync::Barrier::new(12);
            std::thread::scope(|scope| {
                let handles = (0..12)
                    .map(|_| {
                        scope.spawn(|| {
                            start.wait();
                            let connection = open_database(&path).unwrap();
                            assert_eq!(schema_version(&connection).unwrap(), SCHEMA_VERSION);
                            let mode: String = connection
                                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                                .unwrap();
                            assert_eq!(mode, "wal");
                        })
                    })
                    .collect::<Vec<_>>();
                for handle in handles {
                    handle.join().unwrap();
                }
            });
        }
    }

    #[test]
    fn opening_current_database_does_not_wait_for_a_writer() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut writer = open_database(&path).unwrap();
        writer.execute("INSERT INTO tasks(status,version,created_at,updated_at) VALUES ('open',1,'now','now')", []).unwrap();
        let tx = write_transaction(&mut writer).unwrap();
        tx.execute("UPDATE tasks SET version=2", []).unwrap();
        // A second connection must see the last committed snapshot while tx lives.
        let reader = open_database(&path).unwrap();
        let version: i64 = reader
            .query_row("SELECT version FROM tasks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        tx.commit().unwrap();
        assert_eq!(
            reader
                .query_row("SELECT version FROM tasks", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn initialization_rejects_existing_data_without_rebuilding_it() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE tasks(id TEXT); INSERT INTO tasks VALUES ('keep-me');")
            .unwrap();
        assert!(matches!(
            initialize(&mut connection),
            Err(StorageError::UnsupportedSchema { .. })
        ));
        assert_eq!(
            connection
                .query_row("SELECT id FROM tasks", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "keep-me"
        );
        assert_eq!(schema_version(&connection).unwrap(), 0);
    }

    #[test]
    fn unknown_schema_is_refused_on_open_and_before_writing() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(
            initialize(&mut connection),
            Err(StorageError::UnsupportedSchema {
                database_version: 99,
                ..
            })
        ));
        assert!(matches!(
            write_transaction(&mut connection),
            Err(StorageError::UnsupportedSchema {
                database_version: 99,
                ..
            })
        ));
    }

    #[test]
    fn task_ids_are_not_reused_and_keys_are_set_once() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection.execute("INSERT INTO tasks(status,version,created_at,updated_at) VALUES ('open',1,'now','now')", []).unwrap();
        let first = connection.last_insert_rowid();
        connection.execute("DELETE FROM tasks", []).unwrap();
        connection.execute("INSERT INTO tasks(status,version,created_at,updated_at) VALUES ('open',1,'now','now')", []).unwrap();
        assert!(connection.last_insert_rowid() > first);
        connection
            .execute("UPDATE tasks SET task_key='KEY'", [])
            .unwrap();
        assert!(
            connection
                .execute("UPDATE tasks SET task_key='OTHER'", [])
                .is_err()
        );
        assert!(connection.execute("UPDATE tasks SET id=100", []).is_err());
    }
    #[test]
    fn task_state_fields_are_complete_and_non_blank() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO tasks(task_key,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at)
                 VALUES ('T','T','open',1,'G','S','A','now','now')",
                [],
            )
            .unwrap();

        for statement in [
            "UPDATE tasks SET block_reason='orphan' WHERE task_key='T'",
            "UPDATE tasks SET status='blocked',block_reason=' ',block_recovery='recover' WHERE task_key='T'",
            "UPDATE tasks SET closure_outcome='partial',closure_reason='residual' WHERE task_key='T'",
            "UPDATE tasks SET status='closed',closure_outcome='bogus',closed_at='now' WHERE task_key='T'",
            "UPDATE tasks SET status='closed',closure_outcome='partial',closure_reason=NULL,closed_at='now' WHERE task_key='T'",
            "UPDATE tasks SET status='closed',closure_outcome='completed',closure_reason=' ',closed_at='now' WHERE task_key='T'",
        ] {
            assert!(connection.execute(statement, []).is_err(), "{statement}");
        }

        connection
            .execute(
                "UPDATE tasks SET status='blocked',block_reason='blocked',block_recovery='recover' WHERE task_key='T'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='closed',block_reason=NULL,block_recovery=NULL,
                    closure_outcome='partial',closure_reason='residual',closed_at='now' WHERE task_key='T'",
                [],
            )
            .unwrap();
    }
    #[test]
    fn sessions_require_a_source_for_external_identifiers() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO tasks(task_key,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at)
                 VALUES ('T','T','open',1,'G','S','A','now','now')",
                [],
            )
            .unwrap();

        for statement in [
            "INSERT INTO sessions(id,task_id,source,external_session_id,started_at)
             VALUES ('S-NULL',(SELECT id FROM tasks WHERE task_key='T'),NULL,'external-1','now')",
            "INSERT INTO sessions(id,task_id,source,external_session_id,started_at)
             VALUES ('S-BLANK',(SELECT id FROM tasks WHERE task_key='T'),' ','external-2','now')",
        ] {
            assert!(connection.execute(statement, []).is_err(), "{statement}");
        }
        connection
            .execute(
                "INSERT INTO sessions(id,task_id,source,external_session_id,started_at)
                 VALUES ('S-VALID',(SELECT id FROM tasks WHERE task_key='T'),'pi','external-3','now')",
                [],
            )
            .unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn database_and_sidecars_are_private_without_chmodding_existing_parent() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let path = temp.path().join("custom.sqlite");
        let connection = open_database(&path).unwrap();
        connection
            .execute("INSERT INTO tasks(task_key,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at) VALUES ('T','T','open',1,'G','S','A',?1,?1)", [now()])
            .unwrap();

        assert_eq!(
            fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777,
            0o755
        );
        for file in [
            &path,
            &sidecar_path(&path, "-wal"),
            &sidecar_path(&path, "-shm"),
        ] {
            assert!(file.exists(), "{} should exist", file.display());
            assert_eq!(
                fs::metadata(file).unwrap().permissions().mode() & 0o077,
                0,
                "{} must not be group/world accessible",
                file.display()
            );
        }
    }
}
