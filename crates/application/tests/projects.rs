use serde_json::json;
use steward_application::{Service, TaskListOptions};

fn service(temp: &tempfile::TempDir) -> Service {
    Service::new(temp.path().join("projects.db"))
}

#[test]
fn projects_are_unique_stable_and_separate_from_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    assert_eq!(
        s.project_create(" Mailroom ").unwrap().data["project"]["id"],
        1
    );
    assert_eq!(
        s.project_create("Agent Steward").unwrap().data["project"]["id"],
        2
    );
    assert_eq!(
        s.project_create("MAILROOM").unwrap_err().body.code,
        "CONSTRAINT_VIOLATION"
    );
    s.project_create("Café").unwrap();
    assert_eq!(
        s.project_create("Cafe\u{301}").unwrap_err().body.code,
        "CONSTRAINT_VIOLATION"
    );
    for reference in ["1", "##1", "MAILROOM"] {
        assert_eq!(
            s.project_show(reference).unwrap().data["project"]["name"],
            "Mailroom"
        );
    }
    assert_eq!(s.project_show("#1").unwrap_err().body.code, "INVALID_INPUT");
    assert_eq!(s.project_show("Mail").unwrap_err().body.code, "NOT_FOUND");
    assert!(
        s.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.session_list(None).unwrap().data["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let first = s.project_list(0, 1).unwrap().data;
    assert_eq!(first["hasMore"], true);
    assert_eq!(first["nextAfter"], 1);
    assert_eq!(s.project_list(1, 1).unwrap().data["projects"][0]["id"], 2);
    for (after, limit) in [(-1, 10), (0, 0), (0, 201)] {
        assert_eq!(
            s.project_list(after, limit).unwrap_err().body.code,
            "INVALID_INPUT"
        );
    }
}

#[test]
fn project_storage_preserves_ids_and_referential_integrity() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Disposable").unwrap();
    let conn = storage_sqlite::open_database(&temp.path().join("projects.db")).unwrap();
    assert!(
        conn.execute("UPDATE projects SET id=9 WHERE id=1", [])
            .is_err()
    );
    conn.execute("DELETE FROM project_history WHERE project_id=1", [])
        .unwrap();
    conn.execute("DELETE FROM projects WHERE id=1", []).unwrap();
    assert_eq!(
        s.project_create("Mailroom").unwrap().data["project"]["id"],
        2
    );
    s.task_create_in_project(None, None, Some("##2")).unwrap();
    assert!(
        conn.execute("UPDATE tasks SET project_id=99 WHERE id=1", [])
            .is_err()
    );
    assert!(conn.execute("DELETE FROM projects WHERE id=2", []).is_err());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        storage_sqlite::SCHEMA_VERSION
    );
}

#[test]
fn task_membership_uses_cas_without_claiming_or_adopting() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    s.project_create("Agent Steward").unwrap();
    let task = s
        .task_create_in_project(None, None, Some("Mailroom"))
        .unwrap()
        .data["task"]
        .clone();
    assert_eq!(task["projectId"], 1);
    assert_eq!(task["version"], 1);
    assert_eq!(task["status"], "open");
    for key in [
        "currentSessionId",
        "worktreePath",
        "repositoryPath",
        "repositoryCommonDir",
        "repositoryBranch",
    ] {
        assert!(task[key].is_null());
    }
    let before = s.history("#1").unwrap().data;
    assert_eq!(
        s.task_set_project("#1", 1, Some("##1"), true, "fixture").unwrap().data["task"],
        task
    );
    assert_eq!(s.history("#1").unwrap().data, before);
    assert_eq!(
        s.task_set_project("#1", 0, Some("##1"), true, "fixture")
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    s.task_claim("#1", 1, "s1", false).unwrap();
    let changed = s.task_set_project("#1", 2, Some("##2"), true, "fixture").unwrap().data["task"].clone();
    assert_eq!(changed["projectId"], 2);
    assert_eq!(changed["version"], 3);
    assert_eq!(changed["currentSessionId"], "s1");
    assert_eq!(changed["status"], "in_progress");
    let history = s.history("#1").unwrap().data;
    assert_eq!(history["history"][2]["changeType"], "task.project_changed");
    assert_eq!(
        history["history"][2]["payload"]["before"]["projectId"],
        json!(1)
    );
    s.project_rename("##2", 1, "Steward").unwrap();
    assert_eq!(s.task_show("#1").unwrap().data["task"], changed);
    assert_eq!(
        s.project_show("Agent Steward").unwrap_err().body.code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_show("##2").unwrap().data["project"]["revision"],
        2
    );
    assert_eq!(
        s.project_rename("##2", 1, "Wrong").unwrap_err().body.code,
        "VERSION_CONFLICT"
    );
    let cleared = s.task_set_project("#1", 3, None, true, "fixture").unwrap().data["task"].clone();
    assert!(cleared["projectId"].is_null());
    assert_eq!(cleared["version"], 4);
    s.task_close("#1", 4, "cancelled", Some("fixture finished"))
        .unwrap();
    assert_eq!(
        s.task_set_project("#1", 5, Some("##1"), true, "fixture")
            .unwrap().data["task"]["status"],
        "closed"
    );
    assert_eq!(s.task_show("##1").unwrap_err().body.code, "INVALID_INPUT");
}

#[test]
fn project_resolution_is_atomic_with_creation_and_filters_bind_cursor_to_id() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    s.project_create("Steward").unwrap();
    assert_eq!(
        s.task_create_in_project(None, None, Some("missing"))
            .unwrap_err()
            .body
            .code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.task_create_in_project(None, Some(r#"{"project":"Mailroom"}"#), Some("##2"))
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    for _ in 0..3 {
        s.task_create_in_project(None, Some(r#"{"project":"Mailroom"}"#), Some("##1"))
            .unwrap();
    }
    s.task_create_with_options(None, Some(r###"{"project":"##2"}"###))
        .unwrap();
    assert!(s.task_create_minimal().unwrap().data["task"]["projectId"].is_null());
    assert_eq!(
        s.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let mut options = TaskListOptions {
        project: Some("Mailroom".into()),
        page_size: Some(1),
        fields: vec!["id".into(), "projectId".into()],
        ..Default::default()
    };
    let page = s.task_list_with_options(&options).unwrap().data;
    assert_eq!(page["tasks"][0]["projectId"], 1);
    options.cursor = Some(page["nextCursor"].as_str().unwrap().to_owned());
    s.project_rename("##1", 1, "Mailroom New").unwrap();
    options.project = Some("##1".into());
    assert!(s.task_list_with_options(&options).is_ok());
    options.project = Some("##2".into());
    assert_eq!(
        s.task_list_with_options(&options).unwrap_err().body.code,
        "INVALID_INPUT"
    );
    options.project = Some("Mailroom".into());
    assert_eq!(
        s.task_list_with_options(&options).unwrap_err().body.code,
        "NOT_FOUND"
    );
}

#[test]
fn project_history_and_mutations_commit_or_roll_back_together() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    s.project_create("Steward").unwrap();
    assert!(s.project_rename("##1", 1, "STEWARD").is_err());
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        1
    );
    let before = s.project_history("##1", 0, 50).unwrap().data;
    s.project_rename("##1", 1, "Mailroom").unwrap();
    assert_eq!(s.project_history("##1", 0, 50).unwrap().data, before);
    s.project_rename("##1", 1, "MAILROOM").unwrap();
    let page = s.project_history("##1", 0, 1).unwrap().data;
    assert_eq!(page["hasMore"], true);
    assert_eq!(page["nextAfter"], 1);
    let renamed = s.project_history("##1", 1, 50).unwrap().data;
    assert_eq!(
        renamed["history"][0]["payload"],
        json!({"previousName":"Mailroom","name":"MAILROOM"})
    );
    let conn = storage_sqlite::open_database(&temp.path().join("projects.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_project_history BEFORE INSERT ON project_history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(s.project_rename("##1", 2, "Lost").is_err());
    assert!(s.project_create("Not committed").is_err());
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["name"],
        "MAILROOM"
    );
    assert_eq!(
        s.project_show("Not committed").unwrap_err().body.code,
        "NOT_FOUND"
    );
    s.task_create_minimal().unwrap();
    conn.execute_batch("CREATE TRIGGER fail_task_history BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(s.task_set_project("#1", 1, Some("##1"), true, "fixture").is_err());
    let task = s.task_show("#1").unwrap().data["task"].clone();
    assert_eq!(task["version"], 1);
    assert!(task["projectId"].is_null());
}

#[test]
fn concurrent_project_writers_do_not_overwrite_or_duplicate() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["A", "B"]
        .into_iter()
        .map(|name| {
            let path = temp.path().join("projects.db");
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Service::new(path).project_rename("##1", 1, name)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.project_history("##1", 0, 50).unwrap().data["history"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn context_reads_project_and_new_notes_without_task_or_session_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    s.task_create_in_project(None, None, Some("##1")).unwrap();
    s.task_claim("#1", 1, "s1", false).unwrap();
    s.task_note("#1", 2, "progress", "before checkpoint")
        .unwrap();
    s.task_checkpoint("#1", 3, "s1", r#"{"summary":"baseline","completed":[],"decisions":[],"pending":[],"nextStep":"review","risks":[]}"#).unwrap();
    s.task_note("#1", 4, "decision", "newer decision").unwrap();
    let conn = storage_sqlite::open_database(&temp.path().join("projects.db")).unwrap();
    // Ordering is by History sequence, not timestamp equality or wall-clock changes.
    conn.execute("UPDATE task_notes SET created_at='1900-01-01'", [])
        .unwrap();
    let before = s.history("#1").unwrap().data;
    let sessions = s.session_list(None).unwrap().data;
    let context = s.task_context("#1").unwrap().data;
    assert_eq!(context["project"]["id"], 1);
    assert_eq!(context["notesSinceCheckpoint"].as_array().unwrap().len(), 1);
    assert_eq!(context["notesSinceCheckpoint"][0]["text"], "newer decision");
    assert_eq!(context["notesTruncated"], false);
    assert!(context["worktreeStatus"].is_null());
    assert_eq!(s.history("#1").unwrap().data, before);
    assert_eq!(s.session_list(None).unwrap().data, sessions);
    for version in 5..56 {
        s.task_note("#1", version, "progress", &format!("note {version}"))
            .unwrap();
    }
    let context = s.task_context("#1").unwrap();
    assert_eq!(context.data["notesTruncated"], true);
    assert_eq!(
        context.data["notesSinceCheckpoint"]
            .as_array()
            .unwrap()
            .len(),
        50
    );
    assert_eq!(context.warnings[0].code, "CONTEXT_NOTES_TRUNCATED");
    assert_eq!(
        s.task_notes("#1").unwrap().data["notes"]
            .as_array()
            .unwrap()
            .len(),
        53
    );
}
