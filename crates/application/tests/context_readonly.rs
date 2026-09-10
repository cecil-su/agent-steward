use rusqlite::Connection;
use std::fs;
use steward_application::Service;

#[test]
fn context_never_initializes_empty_invalid_or_missing_databases() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing-parent/db");
    assert!(Service::new(&missing).task_context("1").is_err());
    assert!(!missing.parent().unwrap().exists());
    for kind in ["zero-byte", "sqlite-empty", "schema0-with-data", "invalid"] {
        let path = root.path().join(kind);
        match kind {
            "zero-byte" => fs::write(&path, []).unwrap(),
            "invalid" => fs::write(&path, b"not sqlite").unwrap(),
            _ => {
                let c = Connection::open(&path).unwrap();
                c.execute_batch("PRAGMA user_version=0; VACUUM;").unwrap();
                if kind == "schema0-with-data" {
                    c.execute_batch(
                        "CREATE TABLE untouched(value TEXT); INSERT INTO untouched VALUES('keep');",
                    )
                    .unwrap();
                }
            }
        }
        let bytes = fs::read(&path).unwrap();
        let entries = fs::read_dir(root.path()).unwrap().count();
        assert!(Service::new(&path).task_context("1").is_err(), "{kind}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "{kind}");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), entries);
        if kind.starts_with("schema0") {
            let c = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
            assert_eq!(
                c.query_row("SELECT value FROM untouched", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "keep"
            );
            assert_eq!(
                c.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type='table'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
    }
}

#[test]
fn valid_context_reads_committed_wal_without_business_or_journal_writes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    let s = Service::new(&path);
    s.task_create_minimal().unwrap();
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    s.task_note("1", 1, "progress", "committed WAL evidence")
        .unwrap();
    let task = s.task_show("1").unwrap().data;
    let history = s.history("1").unwrap().data;
    let bytes = fs::read(&path).unwrap();
    let wal_path = path.with_file_name("db-wal");
    let wal_bytes = fs::read(&wal_path).unwrap();
    let data_version = || {
        writer
            .pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))
            .unwrap()
    };
    let version = data_version();
    let context = s.task_context("1").unwrap().data;
    assert_eq!(context["task"], task["task"]);
    assert_eq!(
        context["notesSinceCheckpoint"][0]["text"],
        "committed WAL evidence"
    );
    assert!(s.task_context("999").is_err());
    assert_eq!(data_version(), version);
    assert_eq!(s.history("1").unwrap().data, history);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(wal_path).unwrap(), wal_bytes);
    let readonly = storage_sqlite::open_database_readonly(&path).unwrap();
    readonly.execute_batch("PRAGMA query_only=OFF;").unwrap();
    assert!(
        readonly
            .execute_batch("CREATE TABLE forbidden(id INTEGER)")
            .is_err(),
        "OS/SQLite READ_ONLY must hold even without query_only"
    );
}
