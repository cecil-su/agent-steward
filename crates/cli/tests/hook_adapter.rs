use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn cli(db: &std::path::Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(["--database", db.to_str().unwrap(), "--json"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn generic_adapter_drops_host_content_and_reports_failures_without_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    cli(&db, &["task", "create"]);
    cli(
        &db,
        &[
            "task",
            "claim",
            "1",
            "--session",
            "local",
            "--if-version",
            "1",
        ],
    );
    cli(
        &db,
        &[
            "session",
            "bind",
            "local",
            "--if-version",
            "2",
            "--source",
            "generic",
            "--external-session",
            "external",
        ],
    );
    let payload = json!({"eventId":"host-1","kind":"tool_result","occurredAt":"2026-09-07T00:00:00Z","toolResult":{"authorization":"synthetic-secret-never-store"},"message":"private body never stored"});
    let run = |input: &str, external: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_task-hook"))
            .args([
                "--database",
                db.to_str().unwrap(),
                "--session",
                "local",
                "--source",
                "generic",
                "--external-session",
                external,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    for _ in 0..2 {
        let out = run(&payload.to_string(), "external");
        assert!(out.status.success());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-secret"));
    }
    let events = cli(&db, &["hook", "list", "local"]);
    assert_eq!(events["data"]["events"].as_array().unwrap().len(), 1);
    assert!(!events.to_string().contains("private body"));
    assert_eq!(
        cli(&db, &["task", "show", "1"])["data"]["task"]["version"],
        3
    );
    let out = run(&payload.to_string(), "unbound");
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("synthetic-secret"));
    let database_bytes = std::fs::read(db).unwrap();
    assert!(!String::from_utf8_lossy(&database_bytes).contains("synthetic-secret"));
}
