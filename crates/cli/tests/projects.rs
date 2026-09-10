use serde_json::Value;
use std::process::Command;

fn run(database: &std::path::Path, args: &[&str], success: bool) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(database)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.success(), success, "{body}");
    assert_eq!(body["schemaVersion"], 2); // Envelope version is not SQLite schema version.
    body
}

#[test]
fn full_git_project_journey_keeps_registration_separate_from_task_execution() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("journey.db");
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(repo.join("README.md"), "synthetic project navigation").unwrap();
    run(&db, &["project", "create", "--name", "Journey"], true);
    run(
        &db,
        &[
            "project",
            "source",
            "add",
            "##1",
            "--repo",
            repo.to_str().unwrap(),
            "--path",
            ".",
            "--if-revision",
            "1",
        ],
        true,
    );
    run(&db, &["task", "create"], true);
    run(
        &db,
        &[
            "task",
            "project",
            "--yes", "--reason", "fixture",
            "#1",
            "--project",
            "##1",
            "--if-version",
            "1",
        ],
        true,
    );
    let list = run(
        &db,
        &[
            "task",
            "list",
            "--project",
            "Journey",
            "--fields",
            "id,projectId",
        ],
        true,
    );
    assert_eq!(list["data"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(list["data"]["tasks"][0]["projectId"], 1);
    let before = run(&db, &["history", "#1"], true);
    let context = run(&db, &["task", "context", "#1"], true);
    assert_eq!(context["data"]["project"]["name"], "Journey");
    assert!(context["data"]["session"].is_null() && context["data"]["worktreeStatus"].is_null());
    let nav = run(
        &db,
        &[
            "project",
            "context",
            "##1",
            "--source",
            "1",
            "--worktree",
            repo.to_str().unwrap(),
        ],
        true,
    );
    assert_eq!(nav["data"]["source"]["id"], 1);
    assert_eq!(nav["data"]["reuseAllowed"], false);
    assert_eq!(run(&db, &["history", "#1"], true), before);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "synthetic project navigation"
    );
}

#[test]
fn projects_and_membership_are_accessible_through_cli() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    let p = run(&db, &["project", "create", "--name", "Mailroom"], true);
    assert_eq!(p["data"]["project"]["id"], 1);
    assert_eq!(
        run(&db, &["project", "show", "##1"], true)["data"]["project"]["name"],
        "Mailroom"
    );
    assert_eq!(
        run(&db, &["project", "show", "#1"], false)["error"]["code"],
        "INVALID_INPUT"
    );
    let created = run(&db, &["task", "create", "--project", "mailroom"], true);
    assert_eq!(created["data"]["task"]["projectId"], 1);
    assert!(created["data"]["task"]["currentSessionId"].is_null());
    let list = run(
        &db,
        &[
            "task",
            "list",
            "--project",
            "##1",
            "--fields",
            "id,projectId",
        ],
        true,
    );
    assert_eq!(list["data"]["tasks"][0]["projectId"], 1);
    run(
        &db,
        &[
            "project",
            "rename",
            "##1",
            "--name",
            "收发室",
            "--if-revision",
            "1",
        ],
        true,
    );
    run(
        &db,
        &[
            "project",
            "rename",
            "##1",
            "--name",
            "stale",
            "--if-revision",
            "1",
        ],
        false,
    );
    assert_eq!(
        run(&db, &["project", "history", "收发室"], true)["data"]["history"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        run(&db, &["project", "list"], true)["data"]["projects"][0]["id"],
        1
    );
    run(
        &db,
        &["task", "project", "#1", "--if-version", "1", "--clear", "--yes", "--reason", "fixture"],
        true,
    );
    run(
        &db,
        &[
            "task",
            "project",
            "--yes", "--reason", "fixture",
            "#1",
            "--if-version",
            "1",
            "--project",
            "##1",
        ],
        false,
    );
    run(
        &db,
        &[
            "task",
            "project",
            "--yes", "--reason", "fixture",
            "#1",
            "--if-version",
            "2",
            "--project",
            "收发室",
        ],
        true,
    );
    let text = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(&db)
        .args(["task", "context", "#1"])
        .output()
        .unwrap();
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("Task #1"));
    assert!(text.contains("##1 收发室"));
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(&db)
        .args(["project", "show", "##1"])
        .output()
        .unwrap();
    assert!(String::from_utf8(output.stdout).unwrap().contains("##1"));
}

#[test]
fn component_source_and_candidate_commands_keep_files_and_task_authority_separate() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    let directory = temp.path().join("documents");
    std::fs::create_dir(&directory).unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join("app")).unwrap();
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "-b", "main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    run(&db, &["project", "create", "--name", "Mailroom"], true);
    run(
        &db,
        &[
            "project",
            "component",
            "add",
            "##1",
            "--name",
            "backend",
            "--if-revision",
            "1",
        ],
        true,
    );
    assert_eq!(
        run(&db, &["project", "component", "list", "##1"], true)["data"]["components"][0]["name"],
        "backend"
    );
    run(
        &db,
        &[
            "project",
            "source",
            "add",
            "##1",
            "--component",
            "backend",
            "--directory",
            directory.to_str().unwrap(),
            "--if-revision",
            "2",
        ],
        true,
    );
    run(
        &db,
        &[
            "project",
            "source",
            "add",
            "##1",
            "--repo",
            repo.to_str().unwrap(),
            "--path",
            "app",
            "--if-revision",
            "3",
        ],
        true,
    );
    let here = run(
        &db,
        &[
            "project",
            "here",
            "--directory",
            directory.to_str().unwrap(),
            "--project",
            "Mailroom",
        ],
        true,
    );
    assert_eq!(here["data"]["candidates"][0]["source"]["componentId"], 1);
    assert!(here["data"]["selectedProjectId"].is_null());
    assert_eq!(
        run(&db, &["project", "source", "list", "##1"], true)["data"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    run(&db, &["project", "source", "resolve", "##1", "1"], true);
    run(&db, &["project", "source", "resolve", "##1", "2"], false);
    run(
        &db,
        &[
            "project",
            "source",
            "resolve",
            "##1",
            "2",
            "--worktree",
            repo.to_str().unwrap(),
        ],
        true,
    );
    run(&db, &["task", "create", "--project", "##1"], true);
    let scoped = run(
        &db,
        &[
            "task",
            "components",
            "--yes", "--reason", "fixture",
            "#1",
            "--component",
            "backend",
            "--if-version",
            "1",
        ],
        true,
    );
    assert_eq!(
        scoped["data"]["task"]["componentIds"],
        serde_json::json!([1])
    );
    assert!(scoped["data"]["task"]["currentSessionId"].is_null());
    run(
        &db,
        &["task", "components", "#1", "--clear", "--if-version", "2", "--yes", "--reason", "fixture"],
        true,
    );
    run(
        &db,
        &[
            "project",
            "source",
            "remove",
            "##1",
            "1",
            "--if-revision",
            "4",
        ],
        true,
    );
    assert!(directory.is_dir());
    assert!(repo.join("app").is_dir());
}

#[test]
fn project_context_cli_is_explicit_bounded_and_does_not_claim_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    let root = temp.path().join("docs");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("README.md"), "synthetic navigation").unwrap();
    std::fs::write(root.join("main.rs"), "synthetic snippet").unwrap();
    run(&db, &["project", "create", "--name", "Mailroom"], true);
    run(
        &db,
        &[
            "project",
            "source",
            "add",
            "##1",
            "--directory",
            root.to_str().unwrap(),
            "--if-revision",
            "1",
        ],
        true,
    );
    let navigation = run(&db, &["project", "context", "##1", "--source", "1"], true);
    assert!(
        navigation["data"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e.get("snippet").is_none())
    );
    let snippet = run(
        &db,
        &[
            "project",
            "context",
            "##1",
            "--source",
            "1",
            "--file",
            "main.rs",
            "--dependency",
            "README.md",
            "--budget-bytes",
            "3000",
        ],
        true,
    );
    assert!(serde_json::to_vec(&snippet["data"]).unwrap().len() <= 3000);
    assert_eq!(snippet["data"]["reuseAllowed"], false);
    assert!(
        snippet["data"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["snippet"] == "synthetic snippet")
    );
    run(
        &db,
        &[
            "project",
            "context",
            "##1",
            "--source",
            "1",
            "--file",
            "../state.db",
        ],
        false,
    );
    assert_eq!(
        run(&db, &["project", "show", "##1"], true)["data"]["project"]["revision"],
        2
    );
}

#[test]
fn previous_development_schema_is_also_rejected_without_upgrade() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("schema3.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA user_version=3; CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES ('keep');").unwrap();
    drop(conn);
    let result = run(&db, &["project", "here"], false);
    assert_eq!(result["error"]["code"], "UNSUPPORTED_SCHEMA_VERSION");
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='source_roots'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn old_schema_is_refused_without_creating_project_tables() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("old.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA user_version=2; CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES ('keep');").unwrap();
    drop(conn);
    let result = run(&db, &["project", "create", "--name", "Mailroom"], false);
    assert_eq!(result["error"]["code"], "UNSUPPORTED_SCHEMA_VERSION");
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row("SELECT value FROM marker", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='projects'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
