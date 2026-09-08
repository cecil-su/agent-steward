use super::*;

pub(super) fn fixture(root: &Path) -> std::path::PathBuf {
    steward_core::set_private_dir(root).unwrap();
    let source = root.join("source.db");
    let c = Connection::open(&source).unwrap();
    c.execute_batch(SCHEMA2).unwrap();
    c.execute_batch("PRAGMA user_version=2; PRAGMA foreign_keys=ON;
        BEGIN; PRAGMA defer_foreign_keys=ON;
        INSERT INTO tasks(id,task_key,status,version,current_session_id,latest_checkpoint_id,created_at,updated_at)
          VALUES (1,'key:literal','in_progress',3,'s1','c1','2026-09-08T00:00:00Z','2026-09-08T00:00:00Z');
        INSERT INTO sessions VALUES ('s1',1,'generic','external',NULL,'DO_NOT_READ_RECORD_PATH','2026-09-08T00:00:00Z',NULL);
        INSERT INTO checkpoints VALUES ('c1',1,'s1','checkpoint','[\"raw text\"]','[]','[]','next','[]',NULL,'2026-09-08T00:00:00Z');
        INSERT INTO task_notes(task_id,session_id,note_type,text,created_at) VALUES (1,'s1','progress','note','2026-09-08T00:00:00Z');
        INSERT INTO history(task_id,sequence,change_type,session_id,occurred_at,summary,payload_json)
          VALUES (1,1,'task.created',NULL,'2026-09-08T00:00:00Z','created','{ \"raw\": true }');
        COMMIT;").unwrap();
    let bytes = [0_u8, 255, 1];
    c.execute("INSERT INTO session_imports VALUES ('i1','s1','DO_NOT_READ_IMPORT_PATH',NULL,?1,?2,'2026-09-08T00:00:00Z')", params![hex::encode(Sha256::digest(bytes)),bytes.as_slice()]).unwrap();
    for id in ["visible", "deleted"] {
        let event = HookEventInput {
            schema_version: 1,
            session_id: "s1".into(),
            source: "generic".into(),
            external_session_id: "external".into(),
            event_id: id.into(),
            kind: "idle".into(),
            occurred_at: "2026-09-08T00:00:00.000000000Z".into(),
        };
        let hash = hex::encode(Sha256::digest(serde_json::to_vec(&event).unwrap()));
        if id == "visible" {
            c.execute("INSERT INTO session_events(session_id,event_id,fingerprint,kind,occurred_at,received_at) VALUES ('s1',?1,?2,'idle',?3,?3)",params![id,hash,event.occurred_at]).unwrap();
        } else {
            c.execute(
                "INSERT INTO session_events(session_id,event_id,fingerprint) VALUES ('s1',?1,?2)",
                params![id, hash],
            )
            .unwrap();
        }
    }
    c.execute("UPDATE sqlite_sequence SET seq=1000", [])
        .unwrap();
    source
}

#[test]
fn schema2_copy_preserves_active_records_tombstones_and_all_high_water_marks() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let target = temp.path().join("new.db");
    let before = fs::read(&source).unwrap();
    let out = Service::new(&target).import_schema2(&source, true).unwrap();
    assert_eq!(out.data["counts"]["session_events"], 2);
    assert_eq!(out.data["externalPathsObserved"], false);
    let s = Service::new(&target);
    let task = s.task_show("key:literal").unwrap().data["task"].clone();
    assert_eq!(task["version"], 3);
    assert_eq!(task["currentSessionId"], "s1");
    assert!(task["projectId"].is_null());
    let deleted=s.hook_ingest(r#"{"schemaVersion":1,"sessionId":"s1","source":"generic","externalSessionId":"external","eventId":"deleted","kind":"idle","occurredAt":"2026-09-08T00:00:00Z"}"#).unwrap();
    assert_eq!(deleted.data["deleted"], true);
    assert_eq!(s.task_create_minimal().unwrap().data["task"]["id"], 1001);
    let c = Connection::open(&target).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='session_events'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1000
    );
    assert_eq!(fs::read(source).unwrap(), before);
    integrity(&c).unwrap();
}

#[test]
fn schema2_reads_committed_wal_and_refuses_commits_during_copy() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let keeper = Connection::open(&source).unwrap();
    keeper
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let before = fs::read(&source).unwrap();
    keeper
        .execute("UPDATE tasks SET next_step='WAL-only'", [])
        .unwrap();
    assert_eq!(fs::read(&source).unwrap(), before);
    let target = temp.path().join("good.db");
    Service::new(&target).import_schema2(&source, true).unwrap();
    assert_eq!(
        Service::new(target).task_show("1").unwrap().data["task"]["nextStep"],
        "WAL-only"
    );
    let target = temp.path().join("changed.db");
    let err = import(&source, &target, true, |phase, _| {
        if phase == "history" {
            keeper
                .execute("UPDATE tasks SET next_step='late commit'", [])
                .unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.body.details["reason"]
            .as_str()
            .unwrap()
            .contains("source changed")
    );
    assert!(!target.exists());
}

#[test]
fn schema2_interruptions_and_late_destination_collisions_never_publish_partial_data() {
    for phase in ["tasks", "history", "session_events", "before_publish"] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path());
        let target = temp.path().join("new.db");
        let before = fs::read(&source).unwrap();
        assert!(
            import(&source, &target, true, |at, _| if at == phase {
                Err(refused("injected interruption"))
            } else {
                Ok(())
            })
            .is_err()
        );
        assert!(!target.exists());
        assert_eq!(fs::read(source).unwrap(), before);
        assert!(!fs::read_dir(temp.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".import-schema2-")
        }));
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path());
        let target = temp.path().join("new.db");
        let occupied = std::path::PathBuf::from(format!("{}{suffix}", target.display()));
        assert!(
            import(&source, &target, true, |phase, _| {
                if phase == "before_publish" {
                    fs::write(&occupied, b"untouched collision").unwrap();
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&occupied).unwrap(), b"untouched collision");
        if !suffix.is_empty() {
            assert!(!target.exists());
        }
    }
}

#[test]
fn schema2_rejects_schema_drift_corruption_and_invalid_metadata() {
    for sql in [
        "PRAGMA user_version=3",
        "PRAGMA user_version=4",
        "CREATE TABLE unknown(value TEXT)",
        "DROP INDEX idx_notes_task_created",
        "UPDATE checkpoints SET completed_json='PRIVATE_INVALID_JSON'",
        "UPDATE history SET payload_json='PRIVATE_INVALID_JSON'",
        "UPDATE session_imports SET sha256='wrong'",
        "UPDATE session_events SET fingerprint=printf('%064d',0)",
        "UPDATE sessions SET ended_at='2026-09-08T00:00:00Z'",
        "UPDATE sqlite_sequence SET seq=0 WHERE name='tasks'",
        "DELETE FROM sqlite_sequence WHERE name='tasks'",
        "INSERT INTO sqlite_sequence VALUES ('tasks',1000)",
        "INSERT INTO sqlite_sequence VALUES ('unknown',1000)",
        "UPDATE session_imports SET content=zeroblob(16777217)",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = fixture(temp.path());
        let target = temp.path().join("new.db");
        Connection::open(&source)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let before = fs::read(&source).unwrap();
        let err = Service::new(&target)
            .import_schema2(&source, true)
            .unwrap_err();
        assert!(
            !serde_json::to_string(&err.body)
                .unwrap()
                .contains("PRIVATE_INVALID_JSON")
        );
        assert!(!target.exists(), "{sql}");
        assert_eq!(fs::read(source).unwrap(), before);
    }
}

#[test]
fn schema2_requires_confirmation_local_paths_and_an_absent_target() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let target = temp.path().join("new.db");
    assert!(
        Service::new(&target)
            .import_schema2(&source, false)
            .is_err()
    );
    assert!(!target.exists());
    assert!(Service::new(&source).import_schema2(&source, true).is_err());
    for path in ["relative", r"\\untrusted.invalid\share\db", r"\\.\PIPE\db"] {
        assert!(
            Service::new(&target)
                .import_schema2(Path::new(path), true)
                .is_err()
        );
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let target = temp.path().join(format!("target-{suffix}.db"));
        let occupied = std::path::PathBuf::from(format!("{}{suffix}", target.display()));
        fs::write(&occupied, b"keep").unwrap();
        assert!(Service::new(&target).import_schema2(&source, true).is_err());
        assert_eq!(fs::read(occupied).unwrap(), b"keep");
    }
}

#[test]
fn schema2_refuses_nonprivate_parent_and_nonfile_source_sidecars() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let shared = temp.path().join("shared");
    fs::create_dir(&shared).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(private_parent(&shared).is_err());
    assert!(
        Service::new(shared.join("new.db"))
            .import_schema2(&source, true)
            .is_err()
    );
    assert!(
        private_parent(&shared).is_err(),
        "must not chmod an existing directory"
    );
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = std::path::PathBuf::from(format!("{}{suffix}", source.display()));
        fs::create_dir(&sidecar).unwrap();
        let target = temp.path().join(format!("rejected{suffix}.db"));
        assert!(Service::new(&target).import_schema2(&source, true).is_err());
        assert!(!target.exists());
        fs::remove_dir(sidecar).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn schema2_rejects_windows_trailing_dot_or_space_in_any_path_component() {
    for root in [r"C:\sandbox", r"\\?\C:\sandbox"] {
        for tail in [".", " ", ". ", ".."] {
            for path in [
                format!(r"{root}\source.db{tail}"),
                format!(r"{root}\parent{tail}\source.db"),
            ] {
                assert!(local_path(Path::new(&path)).is_err(), "{path}");
            }
        }
        assert!(local_path(Path::new(&format!(r"{root}\目录 with spaces\source.db"))).is_ok());
    }
}

#[cfg(windows)]
#[test]
fn schema2_sidecar_validation_uses_the_canonical_identity_not_alias_spelling() {
    let temp = tempfile::tempdir().unwrap();
    // These aliases need ordinary Win32 spelling even if TEMP uses an extended prefix.
    let source = git_adapter::local_worktree_path(&fixture(temp.path())).unwrap();
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = source.with_file_name(format!("source.db{suffix}"));
        fs::create_dir(&sidecar).unwrap();
        for tail in [".", " "] {
            let alias = source.with_file_name(format!("source.db{tail}"));
            // Exercise canonical sidecar selection independently of the lexical
            // gate, which now rejects these aliases before filesystem access.
            let identity = git_adapter::identify_existing(&alias).unwrap();
            assert_ne!(identity.canonical_path.file_name(), alias.file_name());
            let error = require_regular_source_sidecars(&identity).unwrap_err();
            assert_eq!(
                error.body.details["reason"],
                "source sidecars must be regular files, not links or directories"
            );
            assert!(
                !alias
                    .with_file_name(format!("source.db{tail}{suffix}"))
                    .exists()
            );
        }
        fs::remove_dir(sidecar).unwrap();
    }
}

#[test]
fn schema2_empty_database_and_new_private_parent_are_supported() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("empty.db");
    let c = Connection::open(&source).unwrap();
    c.execute_batch(SCHEMA2).unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
    drop(c);
    let target = temp.path().join("private-new").join("new.db");
    let out = Service::new(&target).import_schema2(&source, true).unwrap();
    assert_eq!(out.data["counts"]["tasks"], 0);
    assert!(crate::database_permission_warning(&target, false).is_none());
    assert_eq!(
        Service::new(target).task_create_minimal().unwrap().data["task"]["id"],
        1
    );
}
