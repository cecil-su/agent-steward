use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use thiserror::Error;

pub const MAX_SCHEMA_VERSION: i64 = 1;

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
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
        set_private_dir(parent)?;
    }
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    set_private_file(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.busy_timeout(std::time::Duration::from_millis(5_000))?;
    migrate(&mut connection)?;
    set_private_file(&sidecar_path(path, "-wal"))?;
    set_private_file(&sidecar_path(path, "-shm"))?;
    Ok(connection)
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current: i64 = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if current > MAX_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            database_version: current,
            max_supported_version: MAX_SCHEMA_VERSION,
        });
    }
    if current == 0 {
        let tx = connection.transaction()?;
        tx.execute_batch(MIGRATION_1)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, description, applied_at) VALUES (1, ?1, ?2)",
            ("initial v0 schema", now()),
        )?;
        tx.commit()?;
    }
    Ok(())
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

#[cfg(unix)]
fn set_private_dir(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_dir(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
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
    CHECK ((status = 'blocked') = (block_reason IS NOT NULL AND block_recovery IS NOT NULL)),
    CHECK ((status = 'closed') = (closure_outcome IS NOT NULL AND closed_at IS NOT NULL)),
    CHECK (status != 'closed' OR (current_session_id IS NULL AND next_step IS NULL AND block_reason IS NULL AND block_recovery IS NULL)),
    CHECK (closure_outcome NOT IN ('partial','cancelled','superseded') OR length(trim(closure_reason)) > 0),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(version, 1);
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
