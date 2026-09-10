use super::*;

#[test]
fn frozen_schema4_preserves_released_rust_literal_line_endings() {
    assert!(
        !SCHEMA4.contains('\r'),
        "CRLF changes sqlite_schema SQL; frozen DDL must stay LF"
    );
}

pub(super) fn fixture(root: &Path) -> std::path::PathBuf {
    steward_core::set_private_dir(root).unwrap();
    let source = root.join("source4.db");
    let c = Connection::open(&source).unwrap();
    c.execute_batch(SCHEMA4).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;
        INSERT INTO projects VALUES (1,'Project','project',5,'now','now');
        INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES
            (1,1,'project.created','now','{ \"raw\": true }'),
            (1,2,'component.created','now','{}'),(1,3,'component.created','now','{}'),
            (1,4,'source.added','now','{}'),(1,5,'source.added','now','{}');
        INSERT INTO components VALUES (1,1,'A','a','now'),(2,1,'B','b','now');
        INSERT INTO tasks(id,project_id,status,version,created_at,updated_at) VALUES (1,1,'open',3,'now','now');
        INSERT INTO task_components VALUES (1,1,2),(1,1,1);
        INSERT INTO history(task_id,sequence,change_type,occurred_at,summary,payload_json) VALUES (1,1,'task.created','now','created','{ \"unchanged\": true }');
    ").unwrap();
    // Capture a real identity, then remove the path. Import must not probe its existence.
    let historical = root.join("historical-source");
    fs::create_dir(&historical).unwrap();
    let identity = git_adapter::identify_existing(&historical).unwrap();
    let text = serde_json::to_string(&identity).unwrap();
    let canonical = identity.canonical_path.to_str().unwrap();
    c.execute(
        "INSERT INTO repositories VALUES (1,?1,?2,'now')",
        params![canonical, text],
    )
    .unwrap();
    c.execute(
        "INSERT INTO source_roots VALUES (1,1,1,NULL,NULL,?1,?2,'now')",
        params![canonical, text],
    )
    .unwrap();
    c.execute(
        "INSERT INTO source_roots VALUES (2,1,2,1,'.',NULL,NULL,'now')",
        [],
    )
    .unwrap();
    fs::remove_dir(historical).unwrap();
    // Include empty autoincrement tables, plus deleted/retired ID ranges.
    for (table, _) in SEQUENCES4 {
        c.execute("DELETE FROM sqlite_sequence WHERE name=?1", [table])
            .unwrap();
        c.execute("INSERT INTO sqlite_sequence VALUES (?1,1000)", [table])
            .unwrap();
    }
    source
}

#[test]
fn schema4_copy_preserves_metadata_membership_history_and_highwater_without_path_io() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let before = fs::read(&source).unwrap();
    let target = temp.path().join("new.db");
    let service = Service::new(&target);
    let out = service.import_schema4(&source, true).unwrap();
    assert_eq!(out.data["sourceSchema"], 4);
    assert_eq!(out.data["targetSchema"], storage_sqlite::SCHEMA_VERSION);
    assert_eq!(out.data["externalPathsObserved"], false);
    assert_eq!(out.data["sourceQuiescenceVerified"], false);
    assert_eq!(out.data["counts"]["task_components"], 2);
    assert_eq!(out.data["highWaterMarks"].as_object().unwrap().len(), 9);
    for (table, _) in SEQUENCES4 {
        assert_eq!(out.data["highWaterMarks"][table], 1000);
    }
    let task = service.task_show("1").unwrap().data["task"].clone();
    assert_eq!(task["projectId"], 1);
    assert_eq!(task["componentIds"], json!([1, 2]));
    assert_eq!(task["version"], 3);
    let c = Connection::open(&target).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT payload_json FROM project_history WHERE id=1",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "{ \"raw\": true }"
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM project_profiles", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        service.project_show("##1").unwrap().data["project"]["revision"],
        5
    );
    assert_eq!(
        service.project_create("Next").unwrap().data["project"]["id"],
        1001
    );
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn schema4_copy_refuses_confirmation_collisions_and_wrong_source_version() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let target = temp.path().join("new.db");
    let s = Service::new(&target);
    assert!(s.import_schema4(&source, false).is_err());
    assert!(!target.exists());
    assert!(s.import_schema2(&source, true).is_err());
    assert!(!target.exists());
    // An independently created destination wins; never overwrite it.
    let result = import_version(&source, &target, true, 4, |phase, _| {
        if phase == "before_publish" {
            fs::write(&target, b"competitor").unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(target).unwrap(), b"competitor");
}

#[test]
fn schema4_copy_rejects_unknown_layout_or_invalid_persisted_metadata() {
    for sql in [
        "CREATE TABLE extra(id INTEGER)",
        "UPDATE project_history SET payload_json='invalid' WHERE id=1",
        "UPDATE projects SET name_key='wrong'",
        "UPDATE projects SET revision=6",
        "UPDATE repositories SET common_identity_json='{}'",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path());
        Connection::open(&source)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let before = fs::read(&source).unwrap();
        let target = temp.path().join("new.db");
        assert!(
            Service::new(&target).import_schema4(&source, true).is_err(),
            "{sql}"
        );
        assert!(!target.exists());
        assert_eq!(fs::read(source).unwrap(), before);
    }
}

#[test]
fn schema4_open_never_implicitly_upgrades_or_changes_source() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let before = fs::read(&source).unwrap();
    assert!(storage_sqlite::open_database(&source).is_err());
    assert!(Service::new(&source).check_database_schema().is_err());
    assert_eq!(fs::read(source).unwrap(), before);
}
