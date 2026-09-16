use serde_json::{Value, json};
use std::{fs, path::Path};
use steward_application::{Service, SourceLocation};

#[test]
fn unavailable_cross_platform_paths_are_metadata_and_reads_do_not_mutate() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("One").unwrap();
    let absent = temp.path().join("not-created/source");
    let first = add(&s, "##1", 1, None, &absent);
    assert!(!absent.exists());
    assert_eq!(first["source"]["directoryPath"], absent.to_str().unwrap());
    for (index, path) in [
        "/not-present/code",
        r"Z:\not-present\code",
        r"\\server\share\code",
    ]
    .iter()
    .enumerate()
    {
        let data = add(&s, "##1", index as i64 + 2, None, Path::new(path));
        assert_eq!(data["source"]["directoryPath"], *path);
        for removed in ["repositoryId", "relativePath", "directoryIdentity"] {
            assert!(data["source"].get(removed).is_none());
        }
    }
    let before = s.project_history("##1", 0, 200).unwrap().data;
    let conn = storage_sqlite::open_database(s.database_path()).unwrap();
    let version = || {
        conn.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))
            .unwrap()
    };
    let original = version();
    for _ in 0..3 {
        assert_eq!(
            s.project_source("##1", 1).unwrap().data["source"],
            first["source"]
        );
        assert_eq!(
            s.project_sources("##1").unwrap().data["sources"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
    assert_eq!(version(), original);
    assert_eq!(s.project_history("##1", 0, 200).unwrap().data, before);
    assert!(!absent.exists());
    assert_eq!(s.task_list(None).unwrap().data["tasks"], json!([]));
    assert_eq!(s.session_list(None).unwrap().data["sessions"], json!([]));
}

#[test]
fn shared_paths_components_and_projects_are_explicit_and_isolated() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("One").unwrap();
    s.project_create("Two").unwrap();
    s.project_component_add("##1", 1, "frontend").unwrap();
    s.project_component_add("##1", 2, "backend").unwrap();
    s.project_component_add("##2", 1, "other").unwrap();
    let path = temp.path().join("shared");
    let root = add(&s, "##1", 3, None, &path);
    let frontend = add(&s, "##1", 4, Some("FRONTEND"), &path);
    add(&s, "##1", 5, Some("backend"), &path);
    let other = add(&s, "##2", 2, Some("other"), &path);
    assert!(root["source"]["componentId"].is_null());
    assert_eq!(frontend["source"]["componentId"], 1);
    assert_eq!(other["source"]["componentId"], 3);
    assert_eq!(
        s.project_sources("##1").unwrap().data["sources"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        s.project_sources("##2").unwrap().data["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.project_source("##2", 1).unwrap_err().body.code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_source_remove("##2", 3, 1).unwrap_err().body.code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_source_add("##1", 6, Some("other"), SourceLocation::Directory(&path))
            .unwrap_err()
            .body
            .code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_source_add("##1", 6, Some("frontend"), SourceLocation::Directory(&path))
            .unwrap_err()
            .body
            .code,
        "CONSTRAINT_VIOLATION"
    );
    let conn = storage_sqlite::open_database(s.database_path()).unwrap();
    assert!(conn.execute("INSERT INTO source_roots(project_id,component_id,directory_path,created_at) VALUES (2,1,'/invalid-scope','now')", []).is_err());
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        6
    );
}

#[test]
fn invalid_path_text_never_registers_or_advances_history() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("One").unwrap();
    let before = s.project_history("##1", 0, 200).unwrap().data;
    for path in ["", ".", "relative/source", "C:relative", "/bad\npath"] {
        assert_eq!(
            s.project_source_add("##1", 1, None, SourceLocation::Directory(Path::new(path)))
                .unwrap_err()
                .body
                .code,
            "INVALID_INPUT"
        );
    }
    let too_long = format!("/{}", "x".repeat(4096));
    assert!(
        s.project_source_add(
            "##1",
            1,
            None,
            SourceLocation::Directory(Path::new(&too_long))
        )
        .is_err()
    );
    assert_eq!(s.project_history("##1", 0, 200).unwrap().data, before);
    assert_eq!(s.project_sources("##1").unwrap().data["sources"], json!([]));
}

#[test]
fn source_records_do_not_validate_git_markers_or_delete_files() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("One").unwrap();
    let directory = temp.path().join("source");
    fs::create_dir_all(directory.join(".git")).unwrap();
    fs::write(directory.join("file"), "keep").unwrap();
    let saved = add(&s, "##1", 1, None, &directory);
    fs::rename(&directory, temp.path().join("held")).unwrap();
    assert_eq!(
        s.project_source("##1", 1).unwrap().data["source"],
        saved["source"]
    );
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("replacement"), "keep too").unwrap();
    s.project_source_remove("##1", 2, 1).unwrap();
    assert_eq!(
        fs::read_to_string(directory.join("replacement")).unwrap(),
        "keep too"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("held/file")).unwrap(),
        "keep"
    );
    assert_eq!(
        s.project_source("##1", 1).unwrap_err().body.code,
        "NOT_FOUND"
    );
}

#[test]
fn metadata_changes_are_cas_guarded_and_roll_back_with_history() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("One").unwrap();
    s.project_component_add("##1", 1, "backend").unwrap();
    assert_eq!(
        s.project_component_add("##1", 1, "other")
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.project_component_add("##1", 2, "BACKEND")
            .unwrap_err()
            .body
            .code,
        "CONSTRAINT_VIOLATION"
    );
    let path = temp.path().join("absent");
    assert_eq!(
        s.project_source_add("##1", 1, None, SourceLocation::Directory(&path))
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    let conn = storage_sqlite::open_database(s.database_path()).unwrap();
    let before = s.project_history("##1", 0, 200).unwrap().data;
    conn.execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON project_history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        s.project_source_add("##1", 2, Some("backend"), SourceLocation::Directory(&path))
            .is_err()
    );
    assert!(s.project_component_add("##1", 2, "rolled-back").is_err());
    assert_eq!(s.project_sources("##1").unwrap().data["sources"], json!([]));
    assert_eq!(
        s.project_components("##1").unwrap().data["components"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s.project_history("##1", 0, 200).unwrap().data, before);
    conn.execute_batch("DROP TRIGGER fail_history;").unwrap();
    let source = add(&s, "##1", 2, Some("backend"), &path);
    let id = source["source"]["id"].as_i64().unwrap();
    assert_eq!(
        s.project_source_remove("##1", 2, id).unwrap_err().body.code,
        "VERSION_CONFLICT"
    );
    let before = s.project_history("##1", 0, 200).unwrap().data;
    conn.execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON project_history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(s.project_source_remove("##1", 3, id).is_err());
    assert_eq!(s.project_source("##1", id).unwrap().data, source);
    assert_eq!(s.project_history("##1", 0, 200).unwrap().data, before);
    conn.execute_batch("DROP TRIGGER fail_history;").unwrap();
    s.project_source_remove("##1", 3, id).unwrap();
    let next = add(&s, "##1", 4, None, &path);
    assert!(next["source"]["id"].as_i64().unwrap() > id);
    assert!(!path.exists());
}

#[test]
fn concurrent_source_registration_never_loses_a_project_revision() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("One").unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let database = s.database_path().to_owned();
            let source = temp.path().join("absent");
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Service::new(database).project_source_add(
                    "##1",
                    1,
                    None,
                    SourceLocation::Directory(&source),
                )
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
        s.project_show("##1").unwrap().data["project"]["revision"],
        2
    );
    assert_eq!(
        s.project_sources("##1").unwrap().data["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.project_history("##1", 0, 200).unwrap().data["history"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

fn service(temp: &tempfile::TempDir) -> Service {
    Service::new(temp.path().join("state.db"))
}
fn add(s: &Service, project: &str, revision: i64, component: Option<&str>, path: &Path) -> Value {
    s.project_source_add(
        project,
        revision,
        component,
        SourceLocation::Directory(path),
    )
    .unwrap()
    .data
}

#[test]
fn task_component_scope_stays_within_its_project_and_uses_task_cas() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    s.project_create("Other").unwrap();
    s.project_component_add("##1", 1, "frontend").unwrap();
    s.project_component_add("##1", 2, "backend").unwrap();
    s.project_component_add("##2", 1, "android").unwrap();
    s.task_create_minimal().unwrap();
    assert_eq!(
        s.task_set_components("#1", 1, &["frontend".into()], true, "fixture")
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    s.task_set_project("#1", 1, Some("##1"), true, "fixture")
        .unwrap();
    assert_eq!(
        s.task_set_components("#1", 2, &["android".into()], true, "fixture")
            .unwrap_err()
            .body
            .code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.task_set_components(
            "#1",
            2,
            &["frontend".into(), "FRONTEND".into()],
            true,
            "fixture"
        )
        .unwrap_err()
        .body
        .code,
        "INVALID_INPUT"
    );
    let scoped = s
        .task_set_components(
            "#1",
            2,
            &["backend".into(), "frontend".into()],
            true,
            "fixture",
        )
        .unwrap()
        .data["task"]
        .clone();
    assert_eq!(scoped["componentIds"], serde_json::json!([1, 2]));
    assert_eq!(scoped["version"], 3);
    assert_eq!(
        s.task_set_components(
            "#1",
            3,
            &["frontend".into(), "backend".into()],
            true,
            "fixture"
        )
        .unwrap()
        .data["task"],
        scoped
    );
    assert_eq!(
        s.task_set_components("#1", 2, &[], true, "fixture")
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.task_set_project("#1", 3, Some("##1"), true, "fixture")
            .unwrap()
            .data["task"],
        scoped
    );
    let list = s
        .task_list_with_options(&steward_application::TaskListOptions {
            fields: vec!["componentIds".into()],
            ..Default::default()
        })
        .unwrap()
        .data;
    assert_eq!(list["tasks"][0]["componentIds"], scoped["componentIds"]);
    assert_eq!(
        s.task_context("#1").unwrap().data["task"]["componentIds"],
        scoped["componentIds"]
    );
    let conn = storage_sqlite::open_database(&temp.path().join("state.db")).unwrap();
    assert!(
        conn.execute(
            "INSERT INTO task_components(task_id,project_id,component_id) VALUES (1,2,3)",
            []
        )
        .is_err()
    );
    conn.execute_batch("CREATE TRIGGER fail_scope BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        s.task_set_components("#1", 3, &[], true, "fixture")
            .is_err()
    );
    assert!(
        s.task_set_project("#1", 3, Some("##2"), true, "fixture")
            .is_err()
    );
    assert_eq!(s.task_show("#1").unwrap().data["task"], scoped);
    conn.execute_batch("DROP TRIGGER fail_scope;").unwrap();
    s.task_claim("#1", 3, "s1", false).unwrap();
    assert!(
        s.task_set_project("#1", 4, Some("##2"), true, "fixture")
            .is_err()
    );
    let moved = s
        .task_update(
            "#1",
            4,
            r#"{"project":"2","components":[]}"#,
            true,
            "fixture",
        )
        .unwrap()
        .data["task"]
        .clone();
    assert_eq!(moved["componentIds"], serde_json::json!([]));
    assert_eq!(moved["currentSessionId"], "s1");
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        3
    );
    let history = s.history("#1").unwrap().data;
    assert_eq!(
        history["history"].as_array().unwrap().last().unwrap()["payload"]["previousComponentIds"],
        serde_json::json!([1, 2])
    );
    s.task_set_components("#1", 5, &["android".into()], true, "fixture")
        .unwrap();
    s.task_set_components("#1", 6, &[], true, "fixture")
        .unwrap();
    s.task_status("#1", 7, "cancelled").unwrap();
    assert_eq!(
        s.task_set_components("#1", 8, &["android".into()], true, "fixture")
            .unwrap()
            .data["task"]["status"],
        "cancelled"
    );
}
