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
    assert_eq!(body["schemaVersion"], 3); // Envelope version is not SQLite schema version.
    body
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
        &[
            "task",
            "project",
            "#1",
            "--if-version",
            "1",
            "--clear",
            "--yes",
            "--reason",
            "fixture",
        ],
        true,
    );
    run(
        &db,
        &[
            "task",
            "project",
            "--yes",
            "--reason",
            "fixture",
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
            "--yes",
            "--reason",
            "fixture",
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
fn previous_development_schema_is_also_rejected_without_upgrade() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("schema3.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA user_version=3; CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES ('keep');").unwrap();
    drop(conn);
    let result = run(&db, &["project", "list"], false);
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
