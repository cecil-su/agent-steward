use super::*;
fn fixture(root: &Path) -> std::path::PathBuf {
    steward_core::set_private_dir(root).unwrap();
    let path = root.join("schema6.db");
    let c = Connection::open(&path).unwrap();
    assert!(
        !SCHEMA6.contains('\r'),
        "frozen SQL must retain Rust literal LF line endings"
    );
    c.execute_batch(SCHEMA6).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; BEGIN;
        INSERT INTO projects VALUES(1,'One','one',1,'now','now');
        INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES(1,1,'project.created','now','{}');
        INSERT INTO tasks(id,project_id,status,version,current_session_id,created_at,updated_at) VALUES(1,1,'pending_release',5,'session','now','now');
        INSERT INTO sessions(id,task_id,started_at) VALUES('session',1,'now');
        INSERT INTO history(task_id,sequence,change_type,occurred_at,summary,payload_json) VALUES(1,1,'task.created','now','synthetic','{ \"raw\": true }');
        COMMIT;").unwrap();
    path
}
#[test]
fn schema6_copy_preserves_records_and_starts_rules_empty() {
    let t = tempfile::tempdir().unwrap();
    let source = fixture(t.path());
    let before = fs::read(&source).unwrap();
    let target = t.path().join("new.db");
    let s = Service::new(&target);
    let out = s.import_schema6(&source, true).unwrap();
    assert_eq!(out.data["sourceSchema"], 6);
    assert_eq!(out.data["targetSchema"], 7);
    let old = Connection::open(&source).unwrap();
    let new = Connection::open(&target).unwrap();
    for table in TABLES5.into_iter().chain(["sqlite_sequence"]) {
        let read = |c: &Connection| {
            let mut stmt = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                .unwrap();
            let n = stmt.column_count();
            stmt.query_map([], |row| {
                (0..n)
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        assert_eq!(read(&old), read(&new), "{table}");
    }
    assert_eq!(
        s.task_context("1").unwrap().data["sessionRules"],
        json!({"formatVersion":1,"rules":[]})
    );
    assert_eq!(
        s.task_show("1").unwrap().data["task"]["status"],
        "pending_release"
    );
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(s.import_schema6(&source, true).is_err());
    assert!(storage_sqlite::open_database(&source).is_err());
}
#[test]
fn schema6_failures_leave_source_unchanged_and_never_publish() {
    for phase in ["tasks", "history", "before_publish"] {
        let t = tempfile::tempdir().unwrap();
        let source = fixture(t.path());
        let before = fs::read(&source).unwrap();
        let target = t.path().join("new.db");
        assert!(
            import_version(&source, &target, true, 6, |step, _| if step == phase {
                Err(refused("fault"))
            } else {
                Ok(())
            })
            .is_err()
        );
        assert!(!target.exists());
        assert_eq!(fs::read(&source).unwrap(), before);
    }
    let t = tempfile::tempdir().unwrap();
    let source = fixture(t.path());
    let target = t.path().join("new.db");
    let s = Service::new(&target);
    assert!(s.import_schema6(&source, false).is_err());
    assert!(s.import_schema5(&source, true).is_err());
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE unexpected(id INTEGER)")
        .unwrap();
    assert!(s.import_schema6(&source, true).is_err());
    assert!(!target.exists());
}
