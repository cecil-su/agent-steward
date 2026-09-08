//! Explicit, offline v7 archive import. Never opens the source through the current initializer.
use std::{fs, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter, types::Value};
use serde_json::json;

use crate::{AppError, AppResult, Outcome, Service};

const TABLES: [&str; 6] = [
    "tasks",
    "sessions",
    "checkpoints",
    "task_notes",
    "history",
    "session_imports",
];

fn io_error(error: std::io::Error) -> AppError {
    AppError::from_storage(storage_sqlite::StorageError::Io(error))
}
fn refused(reason: &str) -> AppError {
    AppError::invalid("source", reason)
}

fn require_unused_destination(destination: &Path) -> AppResult<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut path = destination.as_os_str().to_os_string();
        path.push(suffix);
        // Also reject dangling symlinks; an occupied pathname is never ours to replace.
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
            Ok(_) => {
                return Err(AppError::invalid(
                    "database",
                    "destination and SQLite sidecars must not exist; merge/overwrite is forbidden",
                ));
            }
        }
    }
    Ok(())
}
fn columns(connection: &Connection, table: &str) -> AppResult<Vec<String>> {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(AppError::from_sqlite)?
        .query_map([], |row| row.get(1))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)
}
fn integrity(connection: &Connection) -> AppResult<()> {
    let mut statement = connection
        .prepare("PRAGMA quick_check")
        .map_err(AppError::from_sqlite)?;
    let checks = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)?;
    if checks != ["ok"] {
        return Err(refused("database integrity check failed"));
    }
    if connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(AppError::from_sqlite)?
        .query([])
        .map_err(AppError::from_sqlite)?
        .next()
        .map_err(AppError::from_sqlite)?
        .is_some()
    {
        return Err(refused("database contains foreign key violations"));
    }
    Ok(())
}

impl Service {
    /// Import a closed-task v7 archive into an absent destination. Existing Task versions and
    /// History remain unchanged: this copies an archive, it is not a business Task mutation.
    pub fn import_legacy_v7(&self, source: &Path, confirmed: bool) -> AppResult<Outcome> {
        if !confirmed {
            return Err(AppError::invalid(
                "yes",
                "explicit --yes confirmation is required",
            ));
        }
        let destination = self.database_path();
        if !source.is_absolute() || !destination.is_absolute() {
            return Err(AppError::invalid(
                "database",
                "source and destination must be absolute paths",
            ));
        }
        require_unused_destination(destination)?;
        if !source.is_file() {
            return Err(refused("expected an existing regular database file"));
        }
        let mut old = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::from_sqlite)?;
        old.busy_timeout(Duration::from_secs(5))
            .map_err(AppError::from_sqlite)?;
        old.pragma_update(None, "query_only", true)
            .map_err(AppError::from_sqlite)?;
        let snapshot = old.transaction().map_err(AppError::from_sqlite)?;
        let user_version: i64 = snapshot
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(AppError::from_sqlite)?;
        let mut statement = snapshot.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(AppError::from_sqlite)?;
        let tables = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        let mut expected = TABLES.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        expected.push("schema_migrations".into());
        expected.sort();
        if user_version != 0 || tables != expected {
            return Err(refused(
                "only the legacy schema_migrations v7 archive is supported",
            ));
        }
        drop(statement);
        let versions = snapshot
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .map_err(AppError::from_sqlite)?
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        if versions != (1..=7).collect::<Vec<_>>() {
            return Err(refused(
                "expected the complete legacy v1 through v7 migration chain",
            ));
        }
        integrity(&snapshot)?;
        let unsupported: bool = snapshot.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE status!='closed' OR current_session_id IS NOT NULL
             OR repository_path IS NOT NULL OR repository_common_dir IS NOT NULL OR repository_branch IS NOT NULL
             OR worktree_path IS NOT NULL OR worktree_path_key IS NOT NULL)", [], |row| row.get(0))
            .map_err(AppError::from_sqlite)?;
        if unsupported {
            return Err(refused(
                "this archive importer requires closed tasks without Worktree bindings",
            ));
        }
        // Avoid silently changing reference semantics when the new resolver has no key: escape.
        let keys = snapshot
            .prepare("SELECT task_key FROM tasks WHERE task_key IS NOT NULL")
            .map_err(AppError::from_sqlite)?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        for key in keys {
            if key.trim() != key
                || key.is_empty()
                || key.starts_with('#')
                || key.starts_with("key:")
                || key.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(refused(
                    "legacy taskKey has ambiguous reference syntax; explicit mapping is required",
                ));
            }
        }
        let parent = destination.parent().ok_or_else(|| {
            AppError::invalid("database", "destination requires a parent directory")
        })?;
        // The operator supplies a dedicated parent. Never chmod an existing shared directory.
        if !parent.try_exists().map_err(io_error)? {
            fs::create_dir_all(parent).map_err(io_error)?;
            steward_core::set_private_dir(parent).map_err(io_error)?;
        }
        let staging = tempfile::Builder::new()
            .prefix(".import-v7-")
            .tempdir_in(parent)
            .map_err(io_error)?;
        steward_core::set_private_dir(staging.path()).map_err(io_error)?;
        let staged_path = staging.path().join("steward.db");
        let mut current =
            storage_sqlite::open_database(&staged_path).map_err(AppError::from_storage)?;
        let tx = storage_sqlite::write_transaction(&mut current).map_err(AppError::from_storage)?;
        tx.pragma_update(None, "defer_foreign_keys", true)
            .map_err(AppError::from_sqlite)?;
        let mut counts = serde_json::Map::new();
        for table in TABLES {
            let mut fields = columns(&tx, table)?;
            let mut old_fields = columns(&snapshot, table)?;
            if table == "tasks" {
                // Legacy archives have no project membership. Keep the new FK NULL;
                // never infer it from old task descriptions or repository paths.
                fields.retain(|c| c != "project_id");
                old_fields.retain(|c| c != "worktree_path_key");
            }
            if old_fields != fields {
                return Err(refused(
                    "legacy table columns do not match the supported v7 layout",
                ));
            }
            let select = format!("SELECT {} FROM {table} ORDER BY id", fields.join(","));
            let insert = format!(
                "INSERT INTO {table} ({}) VALUES ({})",
                fields.join(","),
                vec!["?"; fields.len()].join(",")
            );
            let mut read = snapshot.prepare(&select).map_err(AppError::from_sqlite)?;
            let mut rows = read.query([]).map_err(AppError::from_sqlite)?;
            let mut write = tx.prepare(&insert).map_err(AppError::from_sqlite)?;
            let mut count = 0_u64;
            while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
                let values = (0..fields.len())
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(AppError::from_sqlite)?;
                write
                    .execute(params_from_iter(values))
                    .map_err(AppError::from_sqlite)?;
                count += 1;
            }
            drop(rows);
            // Compare every field and BLOB, not merely row counts, within the same snapshots.
            let mut check = tx.prepare(&select).map_err(AppError::from_sqlite)?;
            let mut copied = check.query([]).map_err(AppError::from_sqlite)?;
            let mut original = read.query([]).map_err(AppError::from_sqlite)?;
            loop {
                match (
                    original.next().map_err(AppError::from_sqlite)?,
                    copied.next().map_err(AppError::from_sqlite)?,
                ) {
                    (None, None) => break,
                    (Some(a), Some(b)) => {
                        for i in 0..fields.len() {
                            if a.get::<_, Value>(i).map_err(AppError::from_sqlite)?
                                != b.get::<_, Value>(i).map_err(AppError::from_sqlite)?
                            {
                                return Err(refused("post-copy field comparison failed"));
                            }
                        }
                    }
                    _ => return Err(refused("post-copy row count comparison failed")),
                }
            }
            counts.insert(table.into(), json!(count));
        }
        for table in ["tasks", "task_notes", "history"] {
            let sequence: Option<i64> = snapshot
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name=?1",
                    [table],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::from_sqlite)?;
            if let Some(sequence) = sequence {
                let maximum: i64 = tx
                    .query_row(
                        &format!("SELECT coalesce(max(id),0) FROM {table}"),
                        [],
                        |row| row.get(0),
                    )
                    .map_err(AppError::from_sqlite)?;
                if sequence < maximum {
                    return Err(refused("invalid legacy autoincrement high-water mark"));
                }
                tx.execute("DELETE FROM sqlite_sequence WHERE name=?1", [table])
                    .map_err(AppError::from_sqlite)?;
                tx.execute(
                    "INSERT INTO sqlite_sequence(name,seq) VALUES (?1,?2)",
                    params![table, sequence],
                )
                .map_err(AppError::from_sqlite)?;
            }
        }
        integrity(&tx)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        // Exercise public readers before publishing; corrupt persisted JSON must not be hidden.
        let staged_service = Service::new(&staged_path);
        let ids = current
            .prepare("SELECT id FROM tasks ORDER BY id")
            .map_err(AppError::from_sqlite)?
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        for id in &ids {
            staged_service.task_context(&id.to_string())?;
            staged_service.history(&id.to_string())?;
        }
        // Publish one self-contained database, without WAL dependencies. hard_link is atomic
        // and fails if destination appeared concurrently. Never overwrite or truncate it.
        let mode: String = current
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(AppError::from_sqlite)?;
        if mode != "delete" {
            return Err(refused("could not finalize staged database journal"));
        }
        current.close().map_err(|(_, e)| AppError::from_sqlite(e))?;
        fs::OpenOptions::new()
            .write(true)
            .open(&staged_path)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)?;
        // A sidecar may have appeared during the copy. Never publish over it:
        // SQLite could replay unrelated WAL/journal pages into the imported archive.
        require_unused_destination(destination)?;
        fs::hard_link(&staged_path, destination).map_err(io_error)?;
        Ok(Outcome::new(json!({
            "source":source,"destination":destination,"sourceSchema":7,"targetSchema":storage_sqlite::SCHEMA_VERSION,
            "counts":counts,"taskIds":ids,"verified":true,"sourceUnchanged":true,
            "omittedLegacyFields":["schema_migrations", "tasks.worktree_path_key"],
            "note":"No Task versions or History events were changed; no session_events existed in this archive."
        })))
    }
}
