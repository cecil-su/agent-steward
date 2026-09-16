use super::*;

// Independent expected projection: all retained fields (including raw JSON/BLOBs)
// compare, with only the explicitly contracted state conversion.
pub(super) fn assert_converted_table(old: &Connection, new: &Connection, table: &str) {
    if table == "repositories" {
        assert!(
            !new.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='repositories')",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
        );
        return;
    }
    let fields = columns(new, table).unwrap();
    let old_fields = columns(old, table).unwrap();
    let projection = fields.iter().map(|field| {
        if table == "tasks" && field == "status" {
            "CASE WHEN status='open' THEN 'todo' WHEN status='pending_release' THEN 'in_review' WHEN status='closed' AND closure_outcome='completed' THEN 'done' WHEN status='closed' THEN 'cancelled' ELSE status END".to_string()
        } else if old_fields.contains(field) { field.clone() } else { "NULL".into() }
    }).collect::<Vec<_>>().join(",");
    let filter = if table == "sqlite_sequence" {
        "WHERE name!='repositories'"
    } else {
        ""
    };
    let read = |c: &Connection, projection: &str| {
        let mut stmt = c
            .prepare(&format!(
                "SELECT {projection} FROM {table} {filter} ORDER BY 1"
            ))
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
    assert_eq!(
        read(old, &projection),
        read(new, &fields.join(",")),
        "{table}"
    );
}

const RULE_CREATED: &str = r#" { "id": 1, "scope": "global", "projectId": null,
  "status": "active", "revision": 1, "contentVersion": 1,
  "content": { "name": "Keep", "body": "  body\n preserved  ", "sources": [] },
  "createdAt": "now", "updatedAt": "now" } "#;

fn fixture(root: &Path) -> std::path::PathBuf {
    steward_core::set_private_dir(root).unwrap();
    let path = root.join("schema7.db");
    let c = Connection::open(&path).unwrap();
    assert!(!SCHEMA7.contains('\r'));
    c.execute_batch(SCHEMA7).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; BEGIN;
        INSERT INTO projects VALUES(1,'One','one',1,'now','now');
        INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES(1,1,'project.created','now','{ \"raw\": true }');
        INSERT INTO tasks(id,status,version,created_at,updated_at) VALUES(1,'open',9,'now','now'),(2,'in_progress',3,'now','now'),(3,'pending_release',4,'now','now');
        INSERT INTO tasks(id,status,version,block_reason,block_recovery,created_at,updated_at) VALUES(4,'blocked',7,'old reason','old recovery','now','now');
        INSERT INTO tasks(id,status,version,title,goal,scope,acceptance_criteria,closure_outcome,closure_reason,closed_at,created_at,updated_at) VALUES
          (5,'closed',8,'title','goal','scope','accept','completed',NULL,'old closed','now','now'),
          (6,'closed',8,NULL,NULL,NULL,NULL,'cancelled','old reason','old closed','now','now'),
          (7,'closed',8,NULL,NULL,NULL,NULL,'superseded','old reason','old closed','now','now'),
          (8,'closed',8,NULL,NULL,NULL,NULL,'partial','old reason','old closed','now','now');
        UPDATE tasks SET repository_path='DO_NOT_READ',repository_common_dir='DO_NOT_READ',repository_branch='old',worktree_path='DO_NOT_READ' WHERE id=2;
        INSERT INTO history(task_id,sequence,change_type,occurred_at,summary,payload_json) VALUES(5,1,'task.closed','now','old','{ \"status\": \"closed\", \"repositoryPath\": \"DO_NOT_READ\" }');
        INSERT INTO sessions(id,task_id,started_at,ended_at,record_path) VALUES('session',5,'now','now','DO_NOT_READ');
        INSERT INTO checkpoints(id,task_id,session_id,summary,completed_json,decisions_json,pending_json,risks_json,next_step,git_head,created_at) VALUES('checkpoint',5,'session','old','[ \"raw\" ]','[]','[]','[]','old next','historical-head','now');
        INSERT INTO rules VALUES(1,'global',NULL,'active',1,1,'{ \"name\": \"Keep\", \"body\": \"  body\\n preserved  \", \"sources\": [] }','now','now');
        COMMIT;").unwrap();
    c.execute(
        "INSERT INTO rule_history VALUES(1,1,1,'rule.created','reason',NULL,?1,'now')",
        [RULE_CREATED],
    )
    .unwrap();
    // History JSON retains its original whitespace and historical state spellings.
    let bytes = [0u8, 255, 42];
    c.execute(
        "INSERT INTO session_imports VALUES('import','session','DO_NOT_READ',NULL,?1,?2,'now')",
        params![hex::encode(Sha256::digest(bytes)), bytes.as_slice()],
    )
    .unwrap();
    for (table, _) in SEQUENCES7 {
        c.execute("DELETE FROM sqlite_sequence WHERE name=?1", [table])
            .unwrap();
        c.execute("INSERT INTO sqlite_sequence VALUES(?1,1000)", [table])
            .unwrap();
    }
    path
}

#[test]
fn schema7_maps_all_states_preserves_history_rules_blobs_and_retired_metadata() {
    let t = tempfile::tempdir().unwrap();
    let source = fixture(t.path());
    let before = fs::read(&source).unwrap();
    let target = t.path().join("new.db");
    let s = Service::new(&target);
    let result = s.import_schema7(&source, true, "{}").unwrap();
    assert_eq!(result.data["sourceSchema"], 7);
    assert_eq!(result.data["targetSchema"], 8);
    assert_eq!(result.data["externalPathsObserved"], false);
    let old = Connection::open(&source).unwrap();
    let new = Connection::open(&target).unwrap();
    for table in TABLES7.into_iter().chain(["sqlite_sequence"]) {
        assert_converted_table(&old, &new, table);
    }
    for (id, status) in [
        (1, "todo"),
        (2, "in_progress"),
        (3, "in_review"),
        (4, "blocked"),
        (5, "done"),
        (6, "cancelled"),
        (7, "cancelled"),
        (8, "cancelled"),
    ] {
        assert_eq!(
            s.task_show(&id.to_string()).unwrap().data["task"]["status"],
            status
        );
    }
    let history = s.rule_history(1).unwrap().data;
    assert_eq!(
        history["history"][0]["after"]["content"]["body"],
        "  body\n preserved  "
    );
    assert_eq!(
        new.query_row("SELECT after_json FROM rule_history WHERE id=1", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        RULE_CREATED
    );
    assert_eq!(s.task_create_minimal().unwrap().data["task"]["id"], 1001);
    assert_eq!(fs::read(source).unwrap(), before);
}

fn add_sources(source: &Path, root: &Path) {
    let historical = root.join("historical");
    fs::create_dir(&historical).unwrap();
    let identity = crate::path_safety::identify_existing(&historical).unwrap();
    let c = Connection::open(source).unwrap();
    let text = serde_json::to_string(&identity).unwrap();
    let path = identity.canonical_path.to_str().unwrap();
    c.execute(
        "INSERT INTO repositories VALUES(1,?1,?2,'now')",
        params![path, text],
    )
    .unwrap();
    c.execute(
        "INSERT INTO source_roots VALUES(1,1,NULL,1,'.',NULL,NULL,'now')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO source_roots VALUES(2,1,NULL,NULL,NULL,?1,?2,'now')",
        params![path, text],
    )
    .unwrap();
    fs::remove_dir(historical).unwrap();
}

#[test]
fn schema7_git_mapping_is_explicit_and_directory_is_unchanged_without_path_io() {
    let t = tempfile::tempdir().unwrap();
    let source = fixture(t.path());
    add_sources(&source, t.path());
    let before = fs::read(&source).unwrap();
    let target = t.path().join("new.db");
    let s = Service::new(&target);
    for text in [
        "{}",
        "[]",
        "null",
        "{\"1\":null}",
        "{\"1\":\"relative\"}",
        "{\"1\":\"E:/one\",\"1\":\"E:/two\"}",
        "{\"01\":\"E:/one\"}",
        "{\"2\":\"E:/one\"}",
        "{\"9\":\"E:/one\"}",
        "{\"1\":\"E:relative\"}",
    ] {
        assert!(s.import_schema7(&source, true, text).is_err(), "{text}");
        assert!(!target.exists());
    }
    s.import_schema7(&source, true, r#"{"1":"E:/does-not-exist/资料"}"#)
        .unwrap();
    let c = Connection::open(&target).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT directory_path FROM source_roots WHERE id=1",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "E:/does-not-exist/资料"
    );
    let old = Connection::open(&source).unwrap();
    assert_eq!(
        old.query_row(
            "SELECT directory_path FROM source_roots WHERE id=2",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        c.query_row(
            "SELECT directory_path FROM source_roots WHERE id=2",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap()
    );
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn schema7_safety_failures_never_publish_or_change_source() {
    let t = tempfile::tempdir().unwrap();
    let source = fixture(t.path());
    let before = fs::read(&source).unwrap();
    let target = t.path().join("new.db");
    assert!(
        Service::new(&target)
            .import_schema7(&source, false, "{}")
            .is_err()
    );
    for phase in ["tasks", "rules", "before_publish"] {
        assert!(
            import_version_paths(
                &source,
                &target,
                true,
                7,
                "{}",
                |step, _| if phase == step {
                    Err(refused("injected failure"))
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(!target.exists());
    }
    assert!(
        import_version_paths(&source, &target, true, 7, "{}", |step, _| {
            if step == "before_publish" {
                fs::write(&target, b"competitor").unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), b"competitor");
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn schema7_rejects_drift_and_invalid_records_without_publishing() {
    for sql in [
        "CREATE TABLE unexpected(id INTEGER)",
        "UPDATE rules SET content_json='{}'",
        "UPDATE history SET payload_json='not-json'",
        "UPDATE session_imports SET sha256='wrong'",
        "UPDATE sqlite_sequence SET seq=0 WHERE name='rules'",
        "INSERT INTO sqlite_sequence VALUES('rules',1000)",
        "DELETE FROM sqlite_sequence WHERE name='rule_history'",
    ] {
        let t = tempfile::tempdir().unwrap();
        let source = fixture(t.path());
        Connection::open(&source)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let before = fs::read(&source).unwrap();
        let target = t.path().join("new.db");
        assert!(
            Service::new(&target)
                .import_schema7(&source, true, "{}")
                .is_err(),
            "{sql}"
        );
        assert!(!target.exists());
        assert_eq!(fs::read(source).unwrap(), before);
    }
}

#[test]
fn schema7_rejects_invalid_rule_history_without_publishing_or_changing_source() {
    for sql in [
        "UPDATE rule_history SET after_json='{\"raw\":true}' WHERE revision=2",
        "PRAGMA ignore_check_constraints=ON; UPDATE rule_history SET after_json='not-json' WHERE revision=2",
        "PRAGMA ignore_check_constraints=ON; UPDATE rule_history SET before_json='not-json' WHERE revision=2",
        "UPDATE rule_history SET before_json='{\"raw\":true}' WHERE revision=2",
        "UPDATE rule_history SET after_json=json_set(after_json,'$.id',2) WHERE revision=2",
        "UPDATE rule_history SET before_json=json_set(before_json,'$.id',2) WHERE revision=2",
        "UPDATE rule_history SET after_json=json_set(after_json,'$.revision',1) WHERE revision=2",
        "UPDATE rule_history SET before_json=json_set(before_json,'$.revision',2) WHERE revision=2",
        "UPDATE rule_history SET after_json=json_set(after_json,'$.contentVersion',99) WHERE revision=2",
        "UPDATE rule_history SET before_json=json_set(before_json,'$.contentVersion',99) WHERE revision=2",
        "UPDATE rule_history SET after_json='{\"raw\":true}' WHERE revision=1",
        "PRAGMA foreign_keys=OFF; UPDATE rule_history SET rule_id=999 WHERE revision=2",
    ] {
        let t = tempfile::tempdir().unwrap();
        let source = fixture(t.path());
        let c = Connection::open(&source).unwrap();
        c.execute_batch("UPDATE rules SET revision=2;
            INSERT INTO rule_history(rule_id,revision,operation,reason,before_json,after_json,occurred_at)
            SELECT rule_id,2,'rule.updated','reason',after_json,json_set(after_json,'$.revision',2),'later'
            FROM rule_history WHERE revision=1;").unwrap();
        c.execute_batch(sql).unwrap();
        drop(c);
        let before = fs::read(&source).unwrap();
        let target = t.path().join("new.db");
        assert!(
            Service::new(&target)
                .import_schema7(&source, true, "{}")
                .is_err(),
            "{sql}"
        );
        for suffix in ["", "-wal", "-shm", "-journal"] {
            assert!(!t.path().join(format!("new.db{suffix}")).exists(), "{sql}");
        }
        assert_eq!(fs::read(&source).unwrap(), before, "{sql}");
    }
}

#[test]
fn schema7_preserves_readable_before_and_after_rule_history_verbatim() {
    let t = tempfile::tempdir().unwrap();
    let source = fixture(t.path());
    let c = Connection::open(&source).unwrap();
    c.execute_batch("UPDATE rules SET revision=2;
        INSERT INTO rule_history(rule_id,revision,operation,reason,before_json,after_json,occurred_at)
        SELECT rule_id,2,'rule.updated','reason',after_json,json_set(after_json,'$.revision',2),'later'
        FROM rule_history WHERE revision=1;").unwrap();
    drop(c);
    let before = fs::read(&source).unwrap();
    let target = t.path().join("new.db");
    let s = Service::new(&target);
    s.import_schema7(&source, true, "{}").unwrap();
    let history = s.rule_history(1).unwrap().data;
    assert_eq!(history["history"].as_array().unwrap().len(), 2);
    assert_eq!(history["history"][1]["before"]["revision"], 1);
    assert_eq!(history["history"][1]["after"]["revision"], 2);
    assert_eq!(
        history["history"][1]["before"]["content"]["body"],
        "  body\n preserved  "
    );
    assert_eq!(
        history["history"][1]["after"]["content"]["body"],
        "  body\n preserved  "
    );
    assert_converted_table(
        &Connection::open(&source).unwrap(),
        &Connection::open(&target).unwrap(),
        "rule_history",
    );
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn schema4_5_6_git_sources_have_version_specific_mapped_imports() {
    for (version, definition) in [(4, SCHEMA4), (5, SCHEMA5), (6, SCHEMA6)] {
        let t = tempfile::tempdir().unwrap();
        steward_core::set_private_dir(t.path()).unwrap();
        let source = t.path().join("old.db");
        let c = Connection::open(&source).unwrap();
        c.execute_batch(definition).unwrap();
        c.execute_batch("INSERT INTO projects VALUES(1,'One','one',1,'now','now');
            INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES(1,1,'project.created','now','{ \"raw\": true }');
            INSERT INTO tasks(id,project_id,status,version,created_at,updated_at) VALUES(1,1,'open',3,'now','now');").unwrap();
        drop(c);
        add_sources(&source, t.path());
        let before = fs::read(&source).unwrap();
        let target = t.path().join("new.db");
        let s = Service::new(&target);
        let unmapped = match version {
            4 => s.import_schema4(&source, true),
            5 => s.import_schema5(&source, true),
            _ => s.import_schema6(&source, true),
        }
        .unwrap_err();
        assert_eq!(unmapped.body.details["field"], "sourcePaths");
        assert!(
            unmapped.body.details["reason"]
                .as_str()
                .unwrap()
                .contains(&format!("import-schema{version} --source-paths"))
        );
        assert!(!target.exists());
        assert_eq!(fs::read(&source).unwrap(), before);
        let mapped = |confirmed| match version {
            4 => s.import_schema4_with_paths(
                &source,
                confirmed,
                r#"{"1":"E:/explicit/not-observed"}"#,
            ),
            5 => s.import_schema5_with_paths(
                &source,
                confirmed,
                r#"{"1":"E:/explicit/not-observed"}"#,
            ),
            _ => s.import_schema6_with_paths(
                &source,
                confirmed,
                r#"{"1":"E:/explicit/not-observed"}"#,
            ),
        };
        assert!(mapped(false).is_err());
        assert!(!target.exists());
        assert!(
            s.import_schema7(&source, true, r#"{"1":"E:/explicit/not-observed"}"#)
                .is_err()
        );
        let wrong_version = if version == 4 {
            s.import_schema5_with_paths(&source, true, r#"{"1":"E:/explicit/not-observed"}"#)
        } else {
            s.import_schema4_with_paths(&source, true, r#"{"1":"E:/explicit/not-observed"}"#)
        };
        assert!(wrong_version.is_err());
        assert!(!target.exists());
        let result = mapped(true).unwrap();
        assert_eq!(result.data["sourceSchema"], version);
        assert_eq!(result.data["targetSchema"], 8);
        assert_eq!(result.data["externalPathsObserved"], false);
        let new = Connection::open(&target).unwrap();
        let old = Connection::open(&source).unwrap();
        assert_eq!(
            new.query_row(
                "SELECT directory_path FROM source_roots WHERE id=1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "E:/explicit/not-observed"
        );
        assert_eq!(
            new.query_row(
                "SELECT directory_path FROM source_roots WHERE id=2",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            old.query_row(
                "SELECT directory_path FROM source_roots WHERE id=2",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap()
        );
        assert_converted_table(&old, &new, "tasks");
        assert_converted_table(&old, &new, "project_history");
        assert_eq!(fs::read(&source).unwrap(), before);
    }
}

#[test]
fn old_entry_points_refuse_git_sources_without_guessing_paths() {
    let t = tempfile::tempdir().unwrap();
    let source = schema4_tests::fixture(t.path());
    let c = Connection::open(&source).unwrap();
    c.execute("UPDATE source_roots SET repository_id=1,relative_path='.',directory_path=NULL,directory_identity_json=NULL WHERE id=2", []).unwrap();
    let target = t.path().join("new.db");
    let error = Service::new(&target)
        .import_schema4(&source, true)
        .unwrap_err();
    assert_eq!(error.body.details["field"], "sourcePaths");
    assert!(!target.exists());
}
