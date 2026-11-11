use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};
fn call(db: &std::path::Path, args: &[&str], input: Option<Value>) -> Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_taskctl"));
    cmd.args(["--database", db.to_str().unwrap(), "--json"]);
    if input.is_some() {
        cmd.args(["--input", "-"]);
    }
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
    }
    let result = child.wait_with_output().unwrap();
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        result.status.success(),
        value["ok"] == true,
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    value
}
#[test]
fn real_cli_persists_requirement_and_reply_and_context_without_schema_change() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("work.db");
    assert_eq!(call(&db, &["task", "create"], None)["ok"], true);
    let input = json!({"reason":"explicit fixture request", "requirements":[{"id":"R1","title":"多需求管理","acceptanceCriteria":"重启能恢复"}],"confirmations":[{"id":"Q1","requirementIds":["R1"],"question":"选择A还是B","recommendation":"A","impact":"决定下一步"}]});
    let created = call(
        &db,
        &["task", "work-update", "1", "--if-version", "1"],
        Some(input),
    );
    assert_eq!(created["data"]["task"]["status"], "todo");
    let resolved = call(
        &db,
        &["task", "work-update", "1", "--if-version", "2"],
        Some(
            json!({"reason":"user answered A","confirmations":[{"id":"Q1","status":"resolved","answer":"A","evidence":["fixture user reply"]}]}),
        ),
    );
    assert_eq!(
        resolved["data"]["work"]["confirmations"][0]["status"],
        "resolved"
    );
    assert_eq!(
        call(&db, &["task", "context", "1", "--require-read-only"], None)["data"]["work"],
        resolved["data"]["work"]
    );
    assert_eq!(
        call(&db, &["task", "work", "1"], None)["data"]["work"],
        resolved["data"]["work"]
    );
    assert_eq!(
        call(
            &db,
            &["task", "work-update", "1", "--if-version", "1"],
            Some(json!({"reason":"stale","requirements":[{"id":"R1","status":"in_progress"}]}))
        )["error"]["code"],
        "VERSION_CONFLICT"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args([
            "--database",
            db.to_str().unwrap(),
            "task",
            "context",
            "1",
            "--require-read-only",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Current requirements")
            && text.contains("多需求管理")
            && text.contains("resolved")
    );
}
