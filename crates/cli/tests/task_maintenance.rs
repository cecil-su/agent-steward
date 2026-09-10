use serde_json::{Value, json};
use std::process::Command;

#[test]
fn update_requires_explicit_authorization_and_atomically_maintains_closed_task() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("maintenance.db");
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
            .args(["--database", db.to_str().unwrap(), "--json"])
            .args(args)
            .output()
            .unwrap();
        (
            out.status.success(),
            serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        )
    };
    assert!(run(&["project", "create", "--name", "Example"]).0);
    assert!(
        run(&[
            "project",
            "component",
            "add",
            "1",
            "--name",
            "api",
            "--if-revision",
            "1"
        ])
        .0
    );
    assert!(run(&["task", "create"]).0);
    assert!(
        run(&[
            "task",
            "close",
            "1",
            "--if-version",
            "1",
            "--outcome",
            "cancelled",
            "--reason",
            "fixture",
            "--yes"
        ])
        .0
    );
    let patch = temp.path().join("patch.json");
    std::fs::write(
        &patch,
        json!({"goal":"corrected", "project":"Example", "components":["api"]}).to_string(),
    )
    .unwrap();
    let before = run(&["task", "show", "1"]).1["data"].clone();
    let args = [
        "task",
        "update",
        "1",
        "--if-version",
        "2",
        "--input",
        patch.to_str().unwrap(),
        "--reason",
        "user authorized fixture correction",
    ];
    assert!(!run(&args).0);
    assert_eq!(run(&["task", "show", "1"]).1["data"], before);
    let mut confirmed = args.to_vec();
    confirmed.push("--yes");
    let (ok, out) = run(&confirmed);
    assert!(ok, "{out}");
    assert_eq!(out["data"]["task"]["status"], "closed");
    assert_eq!(out["data"]["task"]["version"], 3);
    assert_eq!(out["data"]["task"]["projectId"], 1);
    assert_eq!(out["data"]["task"]["componentIds"], json!([1]));
    assert!(!run(&confirmed).0);
    assert_eq!(
        run(&["session", "list", "--task", "1"]).1["data"]["sessions"],
        json!([])
    );
}
