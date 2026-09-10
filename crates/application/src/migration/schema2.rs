//! Explicit Schema 2/4/5 copies. No source initializer, external record-path IO, or service switch.
use super::{columns, io_error, refused, require_unused_destination};
use crate::{AppError, AppResult, HookEventInput, Outcome, Service, db};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags, params, params_from_iter, types::Value};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Duration};

const SCHEMA2: &str = include_str!("schema2.sql");
const SCHEMA4: &str = include_str!("schema4.sql");
const SCHEMA5: &str = include_str!("schema5.sql");
const SCHEMA6: &str = include_str!("schema6.sql");
const TABLES5: [&str; 14] = [
    "projects",
    "project_history",
    "components",
    "repositories",
    "source_roots",
    "tasks",
    "sessions",
    "checkpoints",
    "task_notes",
    "history",
    "session_imports",
    "session_events",
    "task_components",
    "project_profiles",
];
const TABLES4: [&str; 13] = [
    "projects",
    "project_history",
    "components",
    "repositories",
    "source_roots",
    "tasks",
    "sessions",
    "checkpoints",
    "task_notes",
    "history",
    "session_imports",
    "session_events",
    "task_components",
];
const SEQUENCES4: [(&str, &str); 9] = [
    ("tasks", "id"),
    ("task_notes", "id"),
    ("history", "id"),
    ("session_events", "sequence"),
    ("projects", "id"),
    ("project_history", "id"),
    ("components", "id"),
    ("repositories", "id"),
    ("source_roots", "id"),
];
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
const PROJECT_TABLES: [&str; 7] = [
    "project_profiles",
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
    // Keep this in offline database checks: do not change the HTTP checkout allowlist.
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

pub(super) fn local_file_path(path: &Path) -> AppResult<()> {
    local_path(path)?;
    // Path::file_name normalizes terminal separators and `/.`.
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
    Ok(())
}

pub(super) fn require_regular_source_sidecars(
    source: &git_adapter::ExistingPathIdentity,
) -> AppResult<()> {
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
    /// Copy a supported, quiescent Schema 2 snapshot to an absent current-schema destination.
    /// The operator must stop writers; identity/data_version rechecks are not a write fence.
    pub fn import_schema2(&self, source: &Path, confirmed: bool) -> AppResult<Outcome> {
        import(source, self.database_path(), confirmed, |_, _| Ok(()))
    }

    /// Preserve Schema 4 task/project records in a separate, explicitly confirmed copy.
    pub fn import_schema4(&self, source: &Path, confirmed: bool) -> AppResult<Outcome> {
        import_version(source, self.database_path(), confirmed, 4, |_, _| Ok(()))
    }

    /// Copy Schema 5 unchanged; never infer pending-release status for existing tasks.
    pub fn import_schema6(&self, source: &Path, confirmed: bool) -> AppResult<Outcome> {
        import_version(source, self.database_path(), confirmed, 6, |_, _| Ok(()))
    }

    pub fn import_schema5(&self, source: &Path, confirmed: bool) -> AppResult<Outcome> {
        import_version(source, self.database_path(), confirmed, 5, |_, _| Ok(()))
    }
}

// A private observation seam for deterministic interruption/collision tests, not a CLI fault flag.
fn import(
    source: &Path,
    destination: &Path,
    confirmed: bool,
    observe: impl FnMut(&str, &Path) -> AppResult<()>,
) -> AppResult<Outcome> {
    import_version(source, destination, confirmed, 2, observe)
}

fn import_version(
    source: &Path,
    destination: &Path,
    confirmed: bool,
    source_version: i64,
    mut observe: impl FnMut(&str, &Path) -> AppResult<()>,
) -> AppResult<Outcome> {
    let (definition, tables, sequence_fields, empty_tables): (
        &str,
        &[&str],
        &[(&str, &str)],
        &[&str],
    ) = match source_version {
        2 => (SCHEMA2, &TABLES, &SEQUENCES, &PROJECT_TABLES),
        4 => (SCHEMA4, &TABLES4, &SEQUENCES4, &["project_profiles"]),
        5 => (SCHEMA5, &TABLES5, &SEQUENCES4, &[]),
        6 => (SCHEMA6, &TABLES5, &SEQUENCES4, &[]),
        _ => return Err(refused("unsupported source schema")),
    };
    if !confirmed {
        return Err(AppError::invalid(
            "yes",
            "explicit --yes confirmation is required",
        ));
    }
    for path in [source, destination] {
        local_file_path(path)?;
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
    if storage_sqlite::schema_version(&snapshot).map_err(AppError::from_storage)? != source_version
    {
        return Err(refused(&format!(
            "only Schema {source_version} snapshots are supported"
        )));
    }
    let expected = Connection::open_in_memory().map_err(AppError::from_sqlite)?;
    expected
        .execute_batch(definition)
        .map_err(AppError::from_sqlite)?;
    if layout(&snapshot)? != layout(&expected)? {
        return Err(refused(&format!(
            "Schema {source_version} layout differs from the supported definition",
        )));
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
        .prefix(&format!(".import-schema{source_version}-"))
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
    for &table in tables {
        let fields = columns(&snapshot, table)?;
        let mut target_fields = columns(&tx, table)?;
        if source_version == 2 && table == "tasks" {
            target_fields.retain(|f| f != "project_id");
        }
        if fields != target_fields {
            return Err(refused("source and target columns are incompatible"));
        }
        let order = if table == "task_components" {
            "1,3"
        } else {
            "1"
        };
        let select = format!("SELECT {} FROM {table} ORDER BY {order}", fields.join(","));
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
        let Some((_, field)) = sequence_fields.iter().find(|(table, _)| *table == name) else {
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
    for &(table, _) in sequence_fields {
        if counts[table].as_u64().unwrap_or(0) > 0 && !highwater.contains_key(table) {
            return Err(refused("missing autoincrement high-water mark"));
        }
    }
    drop(sequences);
    drop(sequence_rows);
    for &table in empty_tables {
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
    for table in ["rules", "rule_history"] {
        let count: i64 = tx
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .map_err(AppError::from_sqlite)?;
        if count != 0 {
            return Err(refused("new rule tables must be empty"));
        }
    }
    validate_records(&tx, source_version >= 4)
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
        "sourceSchema":source_version,"targetSchema":storage_sqlite::SCHEMA_VERSION,
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

fn validate_records(c: &Connection, allow_projects: bool) -> AppResult<()> {
    if allow_projects {
        validate_project_records(c)?;
    }
    let mut tasks = c
        .prepare("SELECT id FROM tasks ORDER BY id")
        .map_err(AppError::from_sqlite)?;
    let mut rows = tasks.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        let id: i64 = row.get(0).map_err(AppError::from_sqlite)?;
        let task = db::load_task(c, id)?;
        if id <= 0
            || (!allow_projects && (task.project_id.is_some() || !task.component_ids.is_empty()))
        {
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

// Decode only persisted metadata. Historical source paths are never filesystem authority.
fn validate_project_records(c: &Connection) -> AppResult<()> {
    for table in ["projects", "components"] {
        let mut statement = c
            .prepare(&format!("SELECT id,name,name_key FROM {table}"))
            .map_err(AppError::from_sqlite)?;
        let mut rows = statement.query([]).map_err(AppError::from_sqlite)?;
        while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
            let id: i64 = row.get(0).map_err(AppError::from_sqlite)?;
            let name: String = row.get(1).map_err(AppError::from_sqlite)?;
            let key: String = row.get(2).map_err(AppError::from_sqlite)?;
            if id <= 0 || steward_core::normalize_project_name(&name).ok() != Some((name, key)) {
                return Err(refused("invalid project/component name or identity"));
            }
            if table == "projects" {
                // Schema 5 profiles must remain readable by the current DTO, not just pass SQLite affinity checks.
                crate::project_profiles::load_profile(c, id)?;
            }
        }
    }
    if c.query_row("SELECT EXISTS(SELECT 1 FROM projects p WHERE p.revision != (SELECT count(*) FROM project_history h WHERE h.project_id=p.id) OR p.revision != (SELECT coalesce(max(revision),0) FROM project_history h WHERE h.project_id=p.id))", [], |r| r.get::<_, bool>(0)).map_err(AppError::from_sqlite)? {
        return Err(refused("project history does not match revision"));
    }
    let mut history = c
        .prepare("SELECT payload_json FROM project_history")
        .map_err(AppError::from_sqlite)?;
    let mut rows = history.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        let text: String = row.get(0).map_err(AppError::from_sqlite)?;
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|_| refused("invalid project history JSON"))?;
    }
    for sql in [
        "SELECT common_dir,common_identity_json FROM repositories",
        "SELECT directory_path,directory_identity_json FROM source_roots WHERE directory_path IS NOT NULL",
    ] {
        let mut statement = c.prepare(sql).map_err(AppError::from_sqlite)?;
        let mut rows = statement.query([]).map_err(AppError::from_sqlite)?;
        while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
            let path: String = row.get(0).map_err(AppError::from_sqlite)?;
            let text: String = row.get(1).map_err(AppError::from_sqlite)?;
            let identity: git_adapter::ExistingPathIdentityRecord = serde_json::from_str(&text)
                .map_err(|_| refused("invalid persisted source identity"))?;
            if identity.canonical_path.to_str() != Some(path.as_str()) {
                return Err(refused("source path differs from persisted identity"));
            }
        }
    }
    let mut statement = c
        .prepare("SELECT relative_path FROM source_roots WHERE relative_path IS NOT NULL")
        .map_err(AppError::from_sqlite)?;
    let mut rows = statement.query([]).map_err(AppError::from_sqlite)?;
    while let Some(row) = rows.next().map_err(AppError::from_sqlite)? {
        let path: String = row.get(0).map_err(AppError::from_sqlite)?;
        steward_core::validate_source_relative_path(&path)
            .map_err(|_| refused("invalid relative source path"))?;
    }
    Ok(())
}

#[cfg(test)]
mod process_tests;
#[cfg(test)]
mod schema4_tests;
#[cfg(test)]
mod schema5_tests;
#[cfg(test)]
mod schema6_tests;
#[cfg(test)]
mod tests;
