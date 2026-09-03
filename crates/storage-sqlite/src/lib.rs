use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
#[cfg(test)]
use rusqlite::params;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use thiserror::Error;

pub const MAX_SCHEMA_VERSION: i64 = 5;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(
        "database is newer than this taskctl build ({database_version} > {max_supported_version})"
    )]
    UnsupportedSchema {
        database_version: i64,
        max_supported_version: i64,
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
        && !parent.exists()
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
    connection.busy_timeout(std::time::Duration::from_millis(5_000))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut connection)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    steward_core::set_private_file(&sidecar_path(path, "-wal"))?;
    steward_core::set_private_file(&sidecar_path(path, "-shm"))?;
    Ok(connection)
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let current = schema_version(connection)?;
    ensure_supported_schema(current)?;
    if current == MAX_SCHEMA_VERSION {
        return Ok(());
    }

    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current = schema_version(&tx)?;
    ensure_supported_schema(current)?;
    if current == MAX_SCHEMA_VERSION {
        tx.commit()?;
        return Ok(());
    }
    if current == 0 {
        tx.execute_batch(MIGRATION_1)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, description, applied_at) VALUES (1, ?1, ?2)",
            ("initial v0 schema", now()),
        )?;
    }
    if current < 2 {
        tx.execute_batch(MIGRATION_2)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, description, applied_at) VALUES (2, ?1, ?2)",
            ("enforce complete task state fields", now()),
        )?;
    }
    if current < 3 {
        migrate_worktree_path_keys(&tx)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, description, applied_at) VALUES (3, ?1, ?2)",
            ("add worktree path diagnostic keys", now()),
        )?;
    }
    if current < 4 {
        tx.execute(
            "INSERT INTO schema_migrations(version, description, applied_at) VALUES (4, ?1, ?2)",
            (
                "use component-aware worktree path keys for new registrations",
                now(),
            ),
        )?;
    }
    if current < 5 {
        tx.execute_batch(MIGRATION_5)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, description, applied_at) VALUES (5, ?1, ?2)",
            (
                "replace persistent path-key uniqueness with transactional live owner validation",
                now(),
            ),
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn migrate_worktree_path_keys(tx: &Transaction<'_>) -> Result<(), StorageError> {
    tx.execute_batch(
        "DROP INDEX idx_tasks_worktree_unique;
         ALTER TABLE tasks ADD COLUMN worktree_path_key TEXT NULL;",
    )?;
    populate_worktree_path_keys(tx)?;
    tx.execute_batch(MIGRATION_3)?;
    Ok(())
}

fn populate_worktree_path_keys(tx: &Transaction<'_>) -> Result<(), StorageError> {
    // Version 5 no longer treats this column as an ownership constraint. Preserve
    // legacy references without touching paths that may now be missing or unsafe
    // to compare; new registrations store a freshly observed diagnostic key.
    tx.execute(
        "UPDATE tasks SET worktree_path_key=worktree_path WHERE worktree_path IS NOT NULL",
        [],
    )?;
    Ok(())
}

fn schema_version(connection: &Connection) -> Result<i64, StorageError> {
    let migrations_table_exists: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_schema WHERE type='table' AND name='schema_migrations'
        )",
        [],
        |row| row.get(0),
    )?;
    if !migrations_table_exists {
        return Ok(0);
    }
    Ok(connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?)
}

fn ensure_supported_schema(current: i64) -> Result<(), StorageError> {
    if current > MAX_SCHEMA_VERSION {
        Err(StorageError::UnsupportedSchema {
            database_version: current,
            max_supported_version: MAX_SCHEMA_VERSION,
        })
    } else {
        Ok(())
    }
}

pub fn canonical_database_path(path: &Path) -> Result<PathBuf, StorageError> {
    if path.exists() {
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

const MIGRATION_1: &str = r#"
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL CHECK(length(trim(title)) > 0),
    status TEXT NOT NULL CHECK(status IN ('open','in_progress','blocked','closed')),
    version INTEGER NOT NULL CHECK(version >= 1),
    goal TEXT NOT NULL CHECK(length(trim(goal)) > 0),
    scope TEXT NOT NULL CHECK(length(trim(scope)) > 0),
    acceptance_criteria TEXT NOT NULL CHECK(length(trim(acceptance_criteria)) > 0),
    next_step TEXT NULL,
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
            AND (closure_outcome = 'completed' OR closure_reason IS NOT NULL))
        OR
        (status != 'closed'
            AND closure_outcome IS NULL AND closure_reason IS NULL AND closed_at IS NULL)
    ),
    CHECK (status != 'closed' OR (current_session_id IS NULL AND next_step IS NULL AND block_reason IS NULL AND block_recovery IS NULL)),
    CHECK ((repository_path IS NULL AND repository_common_dir IS NULL AND repository_branch IS NULL AND worktree_path IS NULL)
        OR (repository_path IS NOT NULL AND repository_common_dir IS NOT NULL AND repository_branch IS NOT NULL AND worktree_path IS NOT NULL))
    ,FOREIGN KEY(current_session_id, id) REFERENCES sessions(id, task_id) DEFERRABLE INITIALLY DEFERRED
    ,FOREIGN KEY(latest_checkpoint_id, id) REFERENCES checkpoints(id, task_id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    source TEXT NULL,
    external_session_id TEXT NULL,
    continued_from TEXT NULL,
    record_path TEXT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NULL,
    UNIQUE(id, task_id),
    CHECK(external_session_id IS NULL OR length(trim(source)) > 0),
    CHECK(continued_from IS NULL OR continued_from != id),
    FOREIGN KEY(continued_from, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE checkpoints (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
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
    task_id TEXT NOT NULL REFERENCES tasks(id),
    session_id TEXT NULL,
    note_type TEXT NOT NULL CHECK(note_type IN ('decision','progress','risk')),
    text TEXT NOT NULL CHECK(length(trim(text)) > 0),
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
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

CREATE INDEX idx_tasks_status_updated ON tasks(status, updated_at);
CREATE INDEX idx_tasks_repo_updated ON tasks(repository_common_dir, updated_at);
CREATE UNIQUE INDEX idx_tasks_worktree_unique ON tasks(worktree_path) WHERE worktree_path IS NOT NULL;
CREATE INDEX idx_sessions_task_started ON sessions(task_id, started_at);
CREATE UNIQUE INDEX idx_sessions_identity ON sessions(id, task_id);
CREATE UNIQUE INDEX idx_sessions_external ON sessions(source, external_session_id) WHERE external_session_id IS NOT NULL;
CREATE INDEX idx_checkpoints_task_created ON checkpoints(task_id, created_at);
CREATE UNIQUE INDEX idx_checkpoints_identity ON checkpoints(id, task_id);
CREATE INDEX idx_notes_task_created ON task_notes(task_id, created_at);
CREATE INDEX idx_history_task_sequence ON history(task_id, sequence);
CREATE INDEX idx_imports_session_time ON session_imports(session_id, imported_at);

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

const MIGRATION_2: &str = r#"
CREATE TRIGGER tasks_complete_state_fields_insert
BEFORE INSERT ON tasks
WHEN NOT (
    (
        (NEW.status = 'blocked'
            AND NEW.block_reason IS NOT NULL AND length(trim(NEW.block_reason)) > 0
            AND NEW.block_recovery IS NOT NULL AND length(trim(NEW.block_recovery)) > 0)
        OR
        (NEW.status != 'blocked' AND NEW.block_reason IS NULL AND NEW.block_recovery IS NULL)
    )
    AND
    (
        (NEW.status = 'closed'
            AND NEW.closure_outcome IS NOT NULL
            AND NEW.closure_outcome IN ('completed','partial','cancelled','superseded')
            AND NEW.closed_at IS NOT NULL AND length(trim(NEW.closed_at)) > 0
            AND (NEW.closure_reason IS NULL OR length(trim(NEW.closure_reason)) > 0)
            AND (NEW.closure_outcome = 'completed' OR NEW.closure_reason IS NOT NULL))
        OR
        (NEW.status != 'closed'
            AND NEW.closure_outcome IS NULL
            AND NEW.closure_reason IS NULL
            AND NEW.closed_at IS NULL)
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task state fields are incomplete or inconsistent');
END;

CREATE TRIGGER tasks_complete_state_fields_update
BEFORE UPDATE ON tasks
WHEN NOT (
    (
        (NEW.status = 'blocked'
            AND NEW.block_reason IS NOT NULL AND length(trim(NEW.block_reason)) > 0
            AND NEW.block_recovery IS NOT NULL AND length(trim(NEW.block_recovery)) > 0)
        OR
        (NEW.status != 'blocked' AND NEW.block_reason IS NULL AND NEW.block_recovery IS NULL)
    )
    AND
    (
        (NEW.status = 'closed'
            AND NEW.closure_outcome IS NOT NULL
            AND NEW.closure_outcome IN ('completed','partial','cancelled','superseded')
            AND NEW.closed_at IS NOT NULL AND length(trim(NEW.closed_at)) > 0
            AND (NEW.closure_reason IS NULL OR length(trim(NEW.closure_reason)) > 0)
            AND (NEW.closure_outcome = 'completed' OR NEW.closure_reason IS NOT NULL))
        OR
        (NEW.status != 'closed'
            AND NEW.closure_outcome IS NULL
            AND NEW.closure_reason IS NULL
            AND NEW.closed_at IS NULL)
    )
)
BEGIN
    SELECT RAISE(ABORT, 'task state fields are incomplete or inconsistent');
END;

-- Fire the update trigger for every legacy row so version 2 is never recorded
-- while invalid version 1 data remains readable as a normal Task.
UPDATE tasks SET status = status;
"#;

const MIGRATION_3: &str = r#"
CREATE TRIGGER tasks_worktree_path_key_insert
BEFORE INSERT ON tasks
WHEN (NEW.worktree_path IS NULL) != (NEW.worktree_path_key IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'worktree path and comparison key must be set together');
END;

CREATE TRIGGER tasks_worktree_path_key_update
BEFORE UPDATE OF worktree_path, worktree_path_key ON tasks
WHEN (NEW.worktree_path IS NULL) != (NEW.worktree_path_key IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'worktree path and comparison key must be set together');
END;
"#;

const MIGRATION_5: &str = r#"
DROP INDEX IF EXISTS idx_tasks_worktree_unique;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_first_open_serializes_all_migrations() {
        use std::sync::{Arc, Barrier};

        for round in 0..4 {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join(format!("concurrent-{round}.sqlite"));
            let barrier = Arc::new(Barrier::new(12));
            let handles = (0..12)
                .map(|_| {
                    let barrier = Arc::clone(&barrier);
                    let path = path.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        let mut connection = Connection::open(&path).unwrap();
                        connection
                            .busy_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                        migrate(&mut connection).map(|()| connection)
                    })
                })
                .collect::<Vec<_>>();

            for handle in handles {
                handle.join().unwrap().unwrap();
            }
            let connection = Connection::open(&path).unwrap();
            let version: i64 = connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(version, MAX_SCHEMA_VERSION);
        }
    }

    #[test]
    fn current_schema_check_does_not_wait_for_a_writer() {
        use std::time::{Duration, Instant};

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("current-schema.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(&format!(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    description TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations VALUES ({MAX_SCHEMA_VERSION}, 'current', 'now');"
            ))
            .unwrap();
        drop(connection);

        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut reader = Connection::open(&path).unwrap();
        reader.busy_timeout(Duration::from_millis(100)).unwrap();

        let started = Instant::now();
        migrate(&mut reader).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a current schema must be checked without acquiring the writer lock"
        );
        writer.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn initializes_schema_once() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, MAX_SCHEMA_VERSION);
    }

    #[test]
    fn task_state_fields_are_complete_and_non_blank() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        migrate(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO tasks(id,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at)
                 VALUES ('T','T','open',1,'G','S','A','now','now')",
                [],
            )
            .unwrap();

        for statement in [
            "UPDATE tasks SET block_reason='orphan' WHERE id='T'",
            "UPDATE tasks SET status='blocked',block_reason=' ',block_recovery='recover' WHERE id='T'",
            "UPDATE tasks SET closure_outcome='partial',closure_reason='residual' WHERE id='T'",
            "UPDATE tasks SET status='closed',closure_outcome='bogus',closed_at='now' WHERE id='T'",
            "UPDATE tasks SET status='closed',closure_outcome='partial',closure_reason=NULL,closed_at='now' WHERE id='T'",
            "UPDATE tasks SET status='closed',closure_outcome='completed',closure_reason=' ',closed_at='now' WHERE id='T'",
        ] {
            assert!(connection.execute(statement, []).is_err(), "{statement}");
        }

        connection
            .execute(
                "UPDATE tasks SET status='blocked',block_reason='blocked',block_recovery='recover' WHERE id='T'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='closed',block_reason=NULL,block_recovery=NULL,
                    closure_outcome='partial',closure_reason='residual',closed_at='now' WHERE id='T'",
                [],
            )
            .unwrap();
    }

    #[test]
    fn migration_two_rejects_invalid_legacy_task_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    description TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations VALUES (1,'legacy','now');
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    block_reason TEXT NULL,
                    block_recovery TEXT NULL,
                    closure_outcome TEXT NULL,
                    closure_reason TEXT NULL,
                    closed_at TEXT NULL
                );
                INSERT INTO tasks VALUES ('T','open',NULL,NULL,'partial','residual',NULL);",
            )
            .unwrap();

        assert!(migrate(&mut connection).is_err());
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn migration_two_enforces_constraints_for_valid_legacy_databases() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    description TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations VALUES (1,'legacy','now');
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    block_reason TEXT NULL,
                    block_recovery TEXT NULL,
                    closure_outcome TEXT NULL,
                    closure_reason TEXT NULL,
                    closed_at TEXT NULL,
                    worktree_path TEXT NULL
                );
                INSERT INTO tasks VALUES ('T','open',NULL,NULL,NULL,NULL,NULL,NULL);
                CREATE UNIQUE INDEX idx_tasks_worktree_unique
                    ON tasks(worktree_path) WHERE worktree_path IS NOT NULL;",
            )
            .unwrap();

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, MAX_SCHEMA_VERSION);
        assert!(
            connection
                .execute(
                    "UPDATE tasks SET closure_outcome='partial',closure_reason='residual' WHERE id='T'",
                    [],
                )
                .is_err()
        );
    }

    #[test]
    fn worktree_path_key_is_not_a_persistent_unique_constraint() {
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("Shared-Worktree");
        let second_path = temp.path().join("shared-worktree");
        let first_key = steward_core::filesystem_path_key(&first_path).unwrap();
        let second_key = steward_core::filesystem_path_key(&second_path).unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        for task_id in ["T1", "T2"] {
            connection
                .execute(
                    "INSERT INTO tasks(
                        id,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at
                     ) VALUES (?1,?1,'open',1,'G','S','A','now','now')",
                    [task_id],
                )
                .unwrap();
        }
        connection
            .execute(
                "UPDATE tasks SET repository_path='repo',repository_common_dir='common',
                    repository_branch='feature',worktree_path=?2,worktree_path_key=?3 WHERE id=?1",
                params!["T1", first_path.to_string_lossy(), first_key],
            )
            .unwrap();

        connection
            .execute(
                "UPDATE tasks SET repository_path='repo',repository_common_dir='common',
                repository_branch='other',worktree_path=?2,worktree_path_key=?3 WHERE id=?1",
                params!["T2", second_path.to_string_lossy(), second_key],
            )
            .unwrap();
    }

    #[test]
    fn migration_from_two_uses_path_snapshots_without_a_transient_unique_index() {
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("Shared-Worktree");
        let second_path = temp.path().join("shared-worktree");
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    description TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                 );",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection
            .execute_batch(
                "INSERT INTO schema_migrations VALUES (1,'legacy','now');
                 INSERT INTO schema_migrations VALUES (2,'legacy','now');",
            )
            .unwrap();
        for task_id in ["T1", "T2"] {
            connection
                .execute(
                    "INSERT INTO tasks(
                        id,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at
                     ) VALUES (?1,?1,'open',1,'G','S','A','now','now')",
                    [task_id],
                )
                .unwrap();
        }
        for (task_id, worktree) in [("T1", &first_path), ("T2", &second_path)] {
            connection
                .execute(
                    "UPDATE tasks SET repository_path='repo',
                        repository_common_dir='common',repository_branch='feature',
                        worktree_path=?2 WHERE id=?1",
                    params![task_id, worktree.to_string_lossy()],
                )
                .unwrap();
        }

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 5);
        let snapshots = connection
            .prepare("SELECT worktree_path_key FROM tasks ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            snapshots,
            vec![
                first_path.to_string_lossy().into_owned(),
                second_path.to_string_lossy().into_owned(),
            ]
        );
    }

    #[test]
    fn migration_five_preserves_stale_keys_but_removes_the_unique_index() {
        let temp = tempfile::tempdir().unwrap();
        let worktree = temp.path().join("MixedParent/Worktree");
        fs::create_dir(worktree.parent().unwrap()).unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    description TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                 );",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection
            .execute_batch(
                "DROP INDEX idx_tasks_worktree_unique;
                 ALTER TABLE tasks ADD COLUMN worktree_path_key TEXT NULL;",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection
            .execute_batch(
                "CREATE UNIQUE INDEX idx_tasks_worktree_unique
                 ON tasks(worktree_path_key) WHERE worktree_path_key IS NOT NULL;",
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO schema_migrations VALUES (1,'legacy','now');
                 INSERT INTO schema_migrations VALUES (2,'legacy','now');
                 INSERT INTO schema_migrations VALUES (3,'legacy','now');
                 INSERT INTO schema_migrations VALUES (4,'legacy','now');
                 INSERT INTO tasks(
                    id,title,status,version,goal,scope,acceptance_criteria,
                    repository_path,repository_common_dir,repository_branch,
                    worktree_path,worktree_path_key,created_at,updated_at
                 ) VALUES (
                    'T','T','open',1,'G','S','A','repo','common','feature',
                    'placeholder','legacy-key','now','now'
                 );",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET worktree_path=?1 WHERE id='T'",
                [worktree.to_str().unwrap()],
            )
            .unwrap();

        migrate(&mut connection).unwrap();

        let (version, key): (i64, String) = connection
            .query_row(
                "SELECT (SELECT MAX(version) FROM schema_migrations),worktree_path_key
                 FROM tasks WHERE id='T'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(version, 5);
        assert_eq!(key, "legacy-key");
        connection
            .execute(
                "INSERT INTO tasks(
                    id,title,status,version,goal,scope,acceptance_criteria,
                    repository_path,repository_common_dir,repository_branch,
                    worktree_path,worktree_path_key,created_at,updated_at
                 ) VALUES (
                    'T2','T2','open',1,'G','S','A','repo','common','other',
                    'other-path','legacy-key','now','now'
                 )",
                [],
            )
            .unwrap();
    }

    #[test]
    fn migration_five_does_not_recompute_unsafe_unicode_keys() {
        let temp = tempfile::tempdir().unwrap();
        let dotted_upper = temp.path().join("\u{130}");
        let dotted_lower = temp.path().join("i\u{307}");
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        connection
            .execute("DELETE FROM schema_migrations WHERE version=5", [])
            .unwrap();
        connection
            .execute_batch(
                "CREATE UNIQUE INDEX idx_tasks_worktree_unique
                 ON tasks(worktree_path_key) WHERE worktree_path_key IS NOT NULL;",
            )
            .unwrap();
        for (task_id, path, legacy_key) in [
            ("T1", &dotted_upper, "legacy-upper"),
            ("T2", &dotted_lower, "legacy-lower"),
        ] {
            connection
                .execute(
                    "INSERT INTO tasks(
                        id,title,status,version,goal,scope,acceptance_criteria,
                        repository_path,repository_common_dir,repository_branch,
                        worktree_path,worktree_path_key,created_at,updated_at
                     ) VALUES (?1,?1,'open',1,'G','S','A','repo','common','feature',
                        ?2,?3,'now','now')",
                    params![task_id, path.to_str().unwrap(), legacy_key],
                )
                .unwrap();
        }

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 5);
        connection
            .execute(
                "UPDATE tasks SET worktree_path_key='legacy-upper' WHERE id='T2'",
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
            .execute("INSERT INTO tasks(id,title,status,version,goal,scope,acceptance_criteria,created_at,updated_at) VALUES ('T','T','open',1,'G','S','A',?1,?1)", [now()])
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
