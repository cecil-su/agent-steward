use rusqlite::{Connection, params};
use std::{fs, path::Path};
use steward_application::Service;

fn legacy(path: &Path) {
    let service = Service::new(path);
    service.task_create("LEGACY", r#"{"title":"0904｜功能｜Archive","goal":"Goal","scope":"Scope","acceptanceCriteria":"Accept"}"#).unwrap();
    service.task_claim("1", 1, "s1", false).unwrap();
    service
        .task_note("1", 2, "progress", "Original note")
        .unwrap();
    service.task_checkpoint("1", 3, "s1", r#"{"summary":"Original checkpoint","completed":[],"decisions":[],"pending":[],"nextStep":"Close","risks":[]}"#).unwrap();
    service.task_close("1", 4, "completed", None).unwrap();
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA journal_mode=DELETE; DROP TABLE session_events;
        DROP TABLE rule_history;
        DROP TABLE rules;
        DROP TABLE project_profiles;
        DROP TABLE project_history;
        DROP TABLE source_roots;
        DROP TABLE task_components;
        DROP INDEX idx_tasks_project_identity;
        DROP TABLE components;
        DROP TABLE repositories;
        DROP INDEX idx_tasks_project_updated;
        ALTER TABLE tasks DROP COLUMN project_id;
        DROP TABLE projects;
        UPDATE history SET payload_json=json_remove(payload_json,'$.projectId') WHERE change_type='task.created';
        ALTER TABLE tasks ADD COLUMN worktree_path_key TEXT;
        CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,description TEXT NOT NULL,applied_at TEXT NOT NULL);
        PRAGMA user_version=0;
        UPDATE sqlite_sequence SET seq=50 WHERE name='tasks';").unwrap();
    for version in 1..=7 {
        c.execute(
            "INSERT INTO schema_migrations VALUES (?1,'legacy','now')",
            [version],
        )
        .unwrap();
    }
    c.execute("INSERT INTO session_imports VALUES ('import-1','s1','archive.txt',NULL,'synthetic',?1,'now')", params![vec![0u8,1,255]]).unwrap();
}

#[test]
fn v7_archive_preserves_rows_relations_history_and_high_water_mark() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("old.db");
    let target = temp.path().join("new.db");
    legacy(&source);
    let before = fs::read(&source).unwrap();
    let old =
        Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let result = Service::new(&target)
        .import_legacy_v7(&source, true)
        .unwrap();
    assert_eq!(result.data["counts"]["history"], 5);
    assert_eq!(result.data["counts"]["session_imports"], 1);
    let c = Connection::open(&target).unwrap();
    for table in [
        "sessions",
        "checkpoints",
        "task_notes",
        "history",
        "session_imports",
    ] {
        fn rows(c: &Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
            let mut st = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY id"))
                .unwrap();
            let n = st.column_count();
            st.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        }
        assert_eq!(rows(&old, table), rows(&c, table), "{table}");
    }
    let task = Service::new(&target).task_show("1").unwrap();
    assert_eq!(task.data["task"]["version"], 5);
    assert_eq!(task.data["task"]["status"], "closed");
    assert!(task.data["task"]["projectId"].is_null());
    assert_eq!(
        c.query_row("SELECT count(*) FROM projects", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        Service::new(&target).task_create_minimal().unwrap().data["task"]["id"],
        51
    );
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn import_refuses_missing_confirmation_existing_target_and_unsupported_sources() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("old.db");
    let target = temp.path().join("new.db");
    legacy(&source);
    assert!(
        Service::new(&target)
            .import_legacy_v7(&source, false)
            .is_err()
    );
    assert!(!target.exists());
    fs::write(&target, b"keep this file").unwrap();
    assert!(
        Service::new(&target)
            .import_legacy_v7(&source, true)
            .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), b"keep this file");
    assert!(
        Service::new(&source)
            .import_legacy_v7(&source, true)
            .is_err()
    );
    let fresh = temp.path().join("fresh.db");
    Service::new(&fresh).task_create_minimal().unwrap();
    assert!(
        Service::new(temp.path().join("other.db"))
            .import_legacy_v7(&fresh, true)
            .is_err()
    );
}

#[test]
fn import_refuses_existing_sidecars_without_modifying_them() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("old.db");
    legacy(&source);
    let source_before = fs::read(&source).unwrap();
    for suffix in ["-wal", "-shm", "-journal"] {
        let target = temp.path().join(format!("target{suffix}.db"));
        let sidecar = temp.path().join(format!("target{suffix}.db{suffix}"));
        fs::write(&sidecar, b"existing data must not be touched").unwrap();
        let error = Service::new(&target)
            .import_legacy_v7(&source, true)
            .unwrap_err();
        assert_eq!(error.body.code, "INVALID_INPUT");
        assert_eq!(error.body.details["field"], "database");
        assert!(!target.exists());
        assert_eq!(
            fs::read(&sidecar).unwrap(),
            b"existing data must not be touched"
        );
    }
    assert_eq!(fs::read(&source).unwrap(), source_before);
}

#[test]
fn import_refuses_a_replayable_wal_from_another_database() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("old.db");
    let target = temp.path().join("new.db");
    legacy(&source);
    let other = Connection::open(temp.path().join("other.db")).unwrap();
    other
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=0;
             CREATE TABLE unrelated(value TEXT);
             INSERT INTO unrelated VALUES ('other database');",
        )
        .unwrap();
    let wal = fs::read(temp.path().join("other.db-wal")).unwrap();
    assert!(!wal.is_empty());
    let sidecar = temp.path().join("new.db-wal");
    fs::write(&sidecar, &wal).unwrap();
    let error = Service::new(&target)
        .import_legacy_v7(&source, true)
        .unwrap_err();
    assert_eq!(error.body.code, "INVALID_INPUT");
    assert!(!target.exists());
    assert_eq!(fs::read(sidecar).unwrap(), wal);
}

#[test]
fn invalid_records_never_publish_a_partial_database() {
    for sql in [
        "UPDATE checkpoints SET completed_json='not json'",
        "UPDATE tasks SET status='open',closure_outcome=NULL,closed_at=NULL",
        "UPDATE tasks SET worktree_path_key='nonempty'",
        "UPDATE sqlite_sequence SET seq=0 WHERE name='tasks'",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("old.db");
        let target = temp.path().join("new.db");
        legacy(&source);
        Connection::open(&source)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        assert!(
            Service::new(&target)
                .import_legacy_v7(&source, true)
                .is_err(),
            "{sql}"
        );
        assert!(!target.exists(), "must not publish: {sql}");
    }
}
