//! Explicit Schema 2 copy. No source initializer, external record-path IO, or service switch.
use super::{columns, io_error, refused, require_unused_destination};
use crate::{AppError, AppResult, HookEventInput, Outcome, Service, db};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags, params, params_from_iter, types::Value};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Duration};

const SCHEMA2: &str = include_str!("schema2.sql");
const TABLES: [&str; 7] = [
    "tasks",
    "sessions",
    "checkpoints",
    "task_notes",
    "history",
    "session_imports",
    "session_events",
];
const SEQUENCES: [(&str, &str); 4] = [
    ("tasks", "id"),
    ("task_notes", "id"),
    ("history", "id"),
    ("session_events", "sequence"),
];
const PROJECT_TABLES: [&str; 6] = [
    "projects",
    "project_history",
    "components",
    "repositories",
    "source_roots",
    "task_components",
];

type SchemaEntry = (String, String, String, Option<String>);
fn layout(c: &Connection) -> AppResult<Vec<SchemaEntry>> {
    c.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY name,type")
        .map_err(AppError::from_sqlite)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<_, _>>()
        .map_err(AppError::from_sqlite)
}

fn integrity(c: &Connection) -> AppResult<()> {
    // Unlike quick_check, integrity_check verifies index/table agreement. Missing
    // index entries must not make ordered source queries silently omit records.
    let checks = c
        .prepare("PRAGMA integrity_check")
        .map_err(AppError::from_sqlite)?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)?;
    if checks != ["ok"] {
        return Err(refused("database integrity check failed"));
    }
    super::integrity(c)
}

fn data_version(c: &Connection) -> AppResult<i64> {
    c.pragma_query_value(None, "data_version", |r| r.get(0))
        .map_err(AppError::from_sqlite)
}

fn local_path(path: &Path) -> AppResult<()> {
    git_adapter::local_worktree_path(path).map_err(|_| AppError::invalid(
        "database", "source and destination require local absolute paths without traversal or device/network prefixes",
    ))?;
    // Win32 and SQLite normalize these names, whereas publication through a
    // canonical extended-length parent can create the literal, different file.
    // Keep this migration-only: do not change the HTTP checkout allowlist.
    #[cfg(windows)]
    for component in path.components() {
        if let std::path::Component::Normal(name) = component
            && name.to_string_lossy().ends_with(['.', ' '])
        {
            return Err(AppError::invalid(
                "database",
                "Windows database paths must not contain components ending in dots or spaces",
            ));
        }
    }
    Ok(())
}

fn require_regular_source_sidecars(source: &git_adapter::ExistingPathIdentity) -> AppResult<()> {
    // Always derive these from the same canonical filename passed to SQLite,
    // never the input spelling (for example, a Windows 8.3 basename alias).
    // This is not a race fence: the source directory must remain trusted/quiescent.
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = source.canonical_path.as_os_str().to_os_string();
        sidecar.push(suffix);
        match fs::symlink_metadata(&sidecar) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(refused(
                    "source sidecars must be regular files, not links or directories",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(())
}

fn private_parent(path: &Path) -> AppResult<()> {
    if !path.is_dir() {
        return Err(AppError::invalid(
            "database",
            "destination requires a dedicated private parent directory",
        ));
    }
    #[cfg(windows)]
    if !steward_core::private_acl_is_protected(path).map_err(io_error)? {
        return Err(AppError::invalid(
            "database",
            "destination parent requires a protected private ACL",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::metadata(path).map_err(io_error)?;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(AppError::invalid(
                "database",
                "destination parent must be private and belong to the current user",
            ));
        }
    }
    #[cfg(not(any(unix, windows)))]
    return Err(AppError::invalid(
        "database",
        "private directory verification unavailable",
    ));
    Ok(())
}

impl Service {
    /// Copy a supported, quiescent Schema 2 snapshot to an absent Schema 4 destination.
    /// The operator must stop writers; identity/data_version rechecks are not a write fence.
    pub fn import_schema2(&self, source: &Path, confirmed: bool) -> AppResult<Outcome> {
        import(source, self.database_path(), confirmed, |_, _| Ok(()))
    }
}

// A private observation seam for deterministic interruption/collision tests, not a CLI fault flag.
fn import(
    source: &Path,
    destination: &Path,
    confirmed: bool,
    mut observe: impl FnMut(&str, &Path) -> AppResult<()>,
) -> AppResult<Outcome> {
    if !confirmed {
        return Err(AppError::invalid(
            "yes",
            "explicit --yes confirmation is required",
        ));
    }
    for path in [source, destination] {
        local_path(path)?;
        // Path::file_name normalizes terminal separators and `/.`. Do not publish
        // a file that cannot be reopened using the operator's original argument.
        let text = path.to_string_lossy();
        if path.file_name().is_none()
            || text.ends_with('/')
            || text.ends_with("/.")
            || (cfg!(windows) && (text.ends_with('\\') || text.ends_with("\\.")))
        {
            return Err(AppError::invalid(
                "database",
                "database paths must end with an explicit filename, not a separator or dot component",
            ));
        }
    }
    require_unused_destination(destination)?;
    if !fs::symlink_metadata(source)
        .map_err(io_error)?
        .file_type()
        .is_file()
    {
        return Err(refused(
            "expected an existing regular database file, not a link",
        ));
    }
    let source_identity = git_adapter::identify_existing(source)
        .map_err(|_| refused("cannot establish source file identity"))?;
    local_path(&source_identity.canonical_path)?;
    // SQLite may access sidecars even for a read-only WAL snapshot.
    require_regular_source_sidecars(&source_identity)?;
    let mut old = Connection::open_with_flags(
        &source_identity.canonical_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(AppError::from_sqlite)?;
    old.busy_timeout(Duration::from_secs(5))
        .map_err(AppError::from_sqlite)?;
    old.pragma_update(None, "query_only", true)
        .map_err(AppError::from_sqlite)?;
    old.pragma_update(None, "trusted_schema", false)
        .map_err(AppError::from_sqlite)?;
    let before = data_version(&old)?;
    let snapshot = old.transaction().map_err(AppError::from_sqlite)?;
    if storage_sqlite::schema_version(&snapshot).map_err(AppError::from_storage)? != 2 {
        return Err(refused("only Schema 2 snapshots are supported"));
    }
    let expected = Connection::open_in_memory().map_err(AppError::from_sqlite)?;
    expected
        .execute_batch(SCHEMA2)
        .map_err(AppError::from_sqlite)?;
    if layout(&snapshot)? != layout(&expected)? {
        return Err(refused(
            "Schema 2 layout differs from the supported definition",
        ));
    }
    integrity(&snapshot)?;
    // Reject oversized imports before materializing their BLOBs (same bound as session import).
    if snapshot
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM session_imports WHERE length(content)>16777216)",
            [],
            |r| r.get::<_, bool>(0),
        )
        .map_err(AppError::from_sqlite)?
    {
        return Err(refused("session import exceeds the supported size"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::invalid("database", "destination requires a parent"))?;
    // Create only the final directory; never chmod a directory another caller created.
    if !parent.try_exists().map_err(io_error)? {
        fs::create_dir(parent).map_err(io_error)?;
        steward_core::set_private_dir(parent).map_err(io_error)?;
    }
    private_parent(parent)?;
    let parent_identity = git_adapter::identify_existing(parent).map_err(|_| {
        AppError::invalid("database", "cannot establish destination parent identity")
    })?;
    local_path(&parent_identity.canonical_path)?;
    let destination = parent_identity.canonical_path.join(
        destination
            .file_name()
            .ok_or_else(|| AppError::invalid("database", "destination requires a filename"))?,
    );
    require_unused_destination(&destination)?;
    let staging = tempfile::Builder::new()
        .prefix(".import-schema2-")
        .tempdir_in(&parent_identity.canonical_path)
        .map_err(io_error)?;
    steward_core::set_private_dir(staging.path()).map_err(io_error)?;
    let staged_path = staging.path().join("steward.db");
    let mut current =
        storage_sqlite::open_database(&staged_path).map_err(AppError::from_storage)?;
    let tx = storage_sqlite::write_transaction(&mut current).map_err(AppError::from_storage)?;
    tx.pragma_update(None, "defer_foreign_keys", true)
        .map_err(AppError::from_sqlite)?;
    let mut counts = serde_json::Map::new();
    let mut hashes = serde_json::Map::new();
    for table in TABLES {
        let fields = columns(&snapshot, table)?;
        let mut target_fields = columns(&tx, table)?;
        if table == "tasks" {
            target_fields.retain(|f| f != "project_id");
        }
        if fields != target_fields {
            return Err(refused("source and target columns are incompatible"));
        }
        let select = format!("SELECT {} FROM {table} ORDER BY 1", fields.join(","));
        let insert = format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            fields.join(","),
            vec!["?"; fields.len()].join(",")
        );
        let mut read = snapshot.prepare(&select).map_err(AppError::from_sqlite)?;
        let mut rows = read.query([]).map_err(AppError::from_sqlite)?;
        let mut write = tx.prepare(&insert).map_err(AppError::from_sqlite)?;
        let mut count = 0u64;
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
        let mut check = tx.prepare(&select).map_err(AppError::from_sqlite)?;
        let mut copied = check.query([]).map_err(AppError::from_sqlite)?;
        let mut original = read.query([]).map_err(AppError::from_sqlite)?;
        let mut hash = Sha256::new();
        // Typed length-delimited values, in primary-key order; no JSON reserialization or bodies in output.
        loop {
            match (
                original.next().map_err(AppError::from_sqlite)?,
                copied.next().map_err(AppError::from_sqlite)?,
            ) {
                (None, None) => break,
                (Some(a), Some(b)) => {
                    hash.update(*b"R");
                    for i in 0..fields.len() {
                        let value: Value = a.get(i).map_err(AppError::from_sqlite)?;
                        if value != b.get::<_, Value>(i).map_err(AppError::from_sqlite)? {
                            return Err(refused("post-copy field comparison failed"));
                        }
                        hash_value(&mut hash, &value);
                    }
                }
                _ => return Err(refused("post-copy row count comparison failed")),
            }
        }
        counts.insert(table.into(), json!(count));
        hashes.insert(table.into(), json!(hex::encode(hash.finalize())));
        observe(table, &staged_path)?;
    }
    let mut highwater = serde_json::Map::new();
    let mut sequence_rows = snapshot
        .prepare("SELECT name,seq FROM sqlite_sequence ORDER BY name")
        .map_err(AppError::from_sqlite)?;
    let mut sequences = sequence_rows.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = sequences.next().map_err(AppError::from_sqlite)? {
        let name: String = row.get(0).map_err(AppError::from_sqlite)?;
        let seq: i64 = row.get(1).map_err(AppError::from_sqlite)?;
        let Some((_, field)) = SEQUENCES.iter().find(|(table, _)| *table == name) else {
            return Err(refused("unknown autoincrement high-water mark"));
        };
        if highwater.contains_key(&name) {
            return Err(refused("duplicate autoincrement high-water mark"));
        }
        let maximum: i64 = tx
            .query_row(
                &format!("SELECT coalesce(max({field}),0) FROM {name}"),
                [],
                |r| r.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        if seq < maximum || seq < 0 {
            return Err(refused("invalid autoincrement high-water mark"));
        }
        tx.execute("DELETE FROM sqlite_sequence WHERE name=?1", [&name])
            .map_err(AppError::from_sqlite)?;
        tx.execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES (?1,?2)",
            params![name, seq],
        )
        .map_err(AppError::from_sqlite)?;
        highwater.insert(name, json!(seq));
    }
    for (table, _) in SEQUENCES {
        if counts[table].as_u64().unwrap_or(0) > 0 && !highwater.contains_key(table) {
            return Err(refused("missing autoincrement high-water mark"));
        }
    }
    drop(sequences);
    drop(sequence_rows);
    for table in PROJECT_TABLES {
        if tx
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(AppError::from_sqlite)?
            != 0
        {
            return Err(refused("project metadata must remain empty"));
        }
    }
    validate_records(&tx)
        .map_err(|_| refused("source records are incompatible or contain invalid metadata"))?;
    integrity(&tx)?;
    tx.commit().map_err(AppError::from_sqlite)?;
    // Reads above only decode DB records. Do not call task_context here: persisted Worktree
    // paths are metadata, not authorization to run Git or probe another machine during migration.
    let mode: String = current
        .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
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
    observe("before_publish", &staged_path)?;
    snapshot.commit().map_err(AppError::from_sqlite)?;
    if data_version(&old)? != before {
        return Err(refused(
            "source changed during copy; stop writers and take a new snapshot",
        ));
    }
    git_adapter::verify_existing_identity(&source_identity)
        .map_err(|_| refused("source file identity changed"))?;
    git_adapter::verify_existing_identity(&parent_identity)
        .map_err(|_| AppError::invalid("database", "destination parent identity changed"))?;
    private_parent(&parent_identity.canonical_path)?;
    require_unused_destination(&destination)?;
    fs::hard_link(&staged_path, &destination).map_err(io_error)?;
    Ok(Outcome::new(json!({
        "source":source_identity.canonical_path,"destination":destination,
        "sourceSchema":2,"targetSchema":storage_sqlite::SCHEMA_VERSION,
        "counts":counts,"tableSha256":hashes,"digestEncoding":"sqlite-typed-rows-v1",
        "highWaterMarks":highwater,"verified":true,"sourceOpenedReadOnly":true,
        "sourceQuiescenceVerified":false,"externalPathsObserved":false,
        "note":"No Task versions, History, sessions or execution bindings changed. No installation or default database switch. Stop all writers before cutover."
    })))
}

fn hash_value(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Null => hash.update(*b"N"),
        Value::Integer(v) => {
            hash.update(*b"I");
            hash.update(v.to_le_bytes());
        }
        Value::Real(v) => {
            hash.update(*b"F");
            hash.update(v.to_bits().to_le_bytes());
        }
        Value::Text(v) => {
            hash.update(*b"T");
            hash.update((v.len() as u64).to_le_bytes());
            hash.update(v.as_bytes());
        }
        Value::Blob(v) => {
            hash.update(*b"B");
            hash.update((v.len() as u64).to_le_bytes());
            hash.update(v);
        }
    }
}

fn decode<T>(
    c: &Connection,
    sql: &str,
    decoder: fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> AppResult<()> {
    let mut s = c.prepare(sql).map_err(AppError::from_sqlite)?;
    let mut rows = s.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        decoder(row).map_err(AppError::from_sqlite)?;
    }
    Ok(())
}

fn validate_records(c: &Connection) -> AppResult<()> {
    let mut tasks = c
        .prepare("SELECT id FROM tasks ORDER BY id")
        .map_err(AppError::from_sqlite)?;
    let mut rows = tasks.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        let id: i64 = row.get(0).map_err(AppError::from_sqlite)?;
        let task = db::load_task(c, id)?;
        if id <= 0 || task.project_id.is_some() || !task.component_ids.is_empty() {
            return Err(refused("invalid task identity or project membership"));
        }
        if let Some(key) = task.task_key
            && (key.trim() != key
                || key.starts_with('#')
                || key.bytes().all(|b| b.is_ascii_digit())
                || db::resolve_task_id(c, &key)? != id)
        {
            return Err(refused("ambiguous task key"));
        }
        if let Some(session) = task.current_session_id
            && db::load_session(c, &session)?.ended_at.is_some()
        {
            return Err(refused("current session has ended"));
        }
    }
    decode(c, "SELECT * FROM sessions", db::session_from_row)?;
    decode(c, "SELECT * FROM checkpoints", db::checkpoint_from_row)?;
    decode(c, "SELECT * FROM task_notes", db::note_from_row)?;
    decode(c, "SELECT * FROM history", db::history_from_row)?;
    let mut imports = c
        .prepare("SELECT sha256,content FROM session_imports")
        .map_err(AppError::from_sqlite)?;
    let mut rows = imports.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        let hash: String = row.get(0).map_err(AppError::from_sqlite)?;
        let bytes: Vec<u8> = row.get(1).map_err(AppError::from_sqlite)?;
        if hex::encode(Sha256::digest(&bytes)) != hash {
            return Err(refused("invalid import digest"));
        }
    }
    decode(
        c,
        "SELECT id,session_id,source_path,media_type,sha256,length(content),imported_at FROM session_imports",
        db::import_from_row,
    )?;
    let mut events = c.prepare("SELECT e.session_id,s.source,s.external_session_id,e.event_id,e.kind,e.occurred_at,e.fingerprint,e.received_at FROM session_events e JOIN sessions s ON s.id=e.session_id").map_err(AppError::from_sqlite)?;
    let mut rows = events.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        let session: String = row.get(0).map_err(AppError::from_sqlite)?;
        let source: String = row.get(1).map_err(AppError::from_sqlite)?;
        let external: String = row.get(2).map_err(AppError::from_sqlite)?;
        let event_id: String = row.get(3).map_err(AppError::from_sqlite)?;
        for (value, max) in [
            (&session, 128),
            (&source, 32),
            (&external, 128),
            (&event_id, 128),
        ] {
            if value.is_empty()
                || value.len() > max
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
            {
                return Err(refused("invalid Hook identifier"));
            }
        }
        let hash: String = row.get(6).map_err(AppError::from_sqlite)?;
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(refused("invalid Hook fingerprint"));
        }
        if let Some(kind) = row
            .get::<_, Option<String>>(4)
            .map_err(AppError::from_sqlite)?
        {
            let occurred: String = row.get(5).map_err(AppError::from_sqlite)?;
            let received: String = row.get(7).map_err(AppError::from_sqlite)?;
            DateTime::parse_from_rfc3339(&received).map_err(|_| refused("invalid Hook time"))?;
            let occurred_at = DateTime::parse_from_rfc3339(&occurred)
                .map_err(|_| refused("invalid Hook time"))?
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Nanos, true);
            let event = HookEventInput {
                schema_version: 1,
                session_id: session,
                source,
                external_session_id: external,
                event_id,
                kind,
                occurred_at,
            };
            let bytes = serde_json::to_vec(&event).map_err(|_| refused("invalid Hook metadata"))?;
            if hex::encode(Sha256::digest(bytes)) != hash {
                return Err(refused("Hook fingerprint mismatch"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod process_tests;
#[cfg(test)]
mod tests;
