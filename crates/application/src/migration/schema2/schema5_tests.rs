use super::*;

fn fixture(root: &Path) -> std::path::PathBuf {
    steward_core::set_private_dir(root).unwrap();
    let source = root.join("source5.db");
    let c = Connection::open(&source).unwrap();
    assert!(!SCHEMA5.contains('\r'));
    c.execute_batch(SCHEMA5).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;
        BEGIN;
        INSERT INTO projects VALUES (1,'Project','project',2,'now','now');
        INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES
            (1,1,'project.created','now','{ \"raw\": true }'),
            (1,2,'project.profile_updated','now','{}');
        INSERT INTO tasks(id,project_id,status,version,created_at,updated_at) VALUES (1,1,'open',3,'now','now');
        INSERT INTO project_profiles VALUES (1,2,'summary','architecture','development',1,3,'evidence','now');
        INSERT INTO history(task_id,sequence,change_type,occurred_at,summary,payload_json) VALUES (1,1,'task.created','now','created','{ \"unchanged\": true }');
        INSERT INTO tasks(id,status,version,current_session_id,latest_checkpoint_id,next_step,created_at,updated_at) VALUES (2,'in_progress',7,'execution','checkpoint','release','now','now');
        INSERT INTO sessions(id,task_id,started_at) VALUES ('execution',2,'now');
        INSERT INTO checkpoints(id,task_id,session_id,summary,completed_json,decisions_json,pending_json,next_step,risks_json,created_at) VALUES ('checkpoint',2,'execution','ready','[]','[]','[]','release','[]','now');
        INSERT INTO task_notes(task_id,session_id,note_type,text,created_at) VALUES (2,'execution','progress','ready','now');
        INSERT INTO tasks(id,status,version,block_reason,block_recovery,created_at,updated_at) VALUES (3,'blocked',9,'reason','recovery','now','now');
        INSERT INTO tasks(id,status,version,closure_outcome,closure_reason,closed_at,created_at,updated_at) VALUES (4,'closed',11,'cancelled','reason','now','now','now');
        COMMIT;
    ").unwrap();
    for (table, _) in SEQUENCES4 {
        c.execute("DELETE FROM sqlite_sequence WHERE name=?1", [table])
            .unwrap();
        c.execute("INSERT INTO sqlite_sequence VALUES (?1,1000)", [table])
            .unwrap();
    }
    source
}

#[test]
fn schema5_copy_preserves_all_tables_and_never_infers_pending_release() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let before = fs::read(&source).unwrap();
    assert!(storage_sqlite::open_database(&source).is_err());
    assert!(Service::new(&source).check_database_schema().is_err());
    let target = temp.path().join("new.db");
    let s = Service::new(&target);
    let result = s.import_schema5(&source, true).unwrap();
    assert_eq!(result.data["sourceSchema"], 5);
    assert_eq!(result.data["targetSchema"], 7);
    assert_eq!(result.data["tableSha256"].as_object().unwrap().len(), 14);
    assert_eq!(result.data["counts"]["project_profiles"], 1);
    let old = Connection::open(&source).unwrap();
    let new = Connection::open(&target).unwrap();
    for table in TABLES5 {
        let sql = format!("SELECT * FROM {table} ORDER BY 1");
        let read = |c: &Connection| {
            let mut statement = c.prepare(&sql).unwrap();
            let width = statement.column_count();
            statement
                .query_map([], |row| {
                    (0..width)
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
        s.task_list(Some("pending_release")).unwrap().data["tasks"],
        json!([])
    );
    assert_eq!(s.task_show("2").unwrap().data["task"]["version"], 7);
    assert_eq!(
        s.project_profile_show("1").unwrap().data["profile"]["sourceTaskVersion"],
        3
    );
    assert_eq!(s.task_create_minimal().unwrap().data["task"]["id"], 1001);
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn schema5_failures_never_publish_or_change_source() {
    for phase in ["tasks", "project_profiles", "before_publish"] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path());
        let before = fs::read(&source).unwrap();
        let target = temp.path().join("new.db");
        let result = import_version(&source, &target, true, 5, |step, _| {
            if step == phase {
                Err(refused("injected failure"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(source).unwrap(), before);
    }
}

#[test]
fn schema5_refuses_missing_confirmation_wrong_version_layout_and_collision() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let target = temp.path().join("new.db");
    let s = Service::new(&target);
    assert!(s.import_schema5(&source, false).is_err());
    assert!(s.import_schema4(&source, true).is_err());
    assert!(!target.exists());
    let result = import_version(&source, &target, true, 5, |phase, _| {
        if phase == "before_publish" {
            fs::write(&target, b"competitor").unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&target).unwrap(), b"competitor");
    Connection::open(&source)
        .unwrap()
        .execute_batch("UPDATE project_profiles SET summary=x'41'")
        .unwrap();
    let malformed = temp.path().join("malformed.db");
    assert!(
        Service::new(&malformed)
            .import_schema5(&source, true)
            .is_err()
    );
    assert!(!malformed.exists());
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE unknown(id INTEGER)")
        .unwrap();
    assert!(
        Service::new(temp.path().join("other.db"))
            .import_schema5(&source, true)
            .is_err()
    );
    assert!(!temp.path().join("other.db").exists());
}
