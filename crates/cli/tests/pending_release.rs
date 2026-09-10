use serde_json::{Value, json};
use std::process::Command;

#[test]
fn cli_release_roundtrip_filters_and_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("release.db");
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
    let input = temp.path().join("task.json");
    std::fs::write(&input, json!({"title":"0910｜功能｜Fixture","goal":"goal","scope":"scope","acceptanceCriteria":"criteria"}).to_string()).unwrap();
    assert!(run(&["task", "create", "--input", input.to_str().unwrap()]).0);
    assert!(
        run(&[
            "task",
            "claim",
            "1",
            "--session",
            "fixture",
            "--if-version",
            "1"
        ])
        .0
    );
    let out = run(&["task", "pending-release", "1", "--if-version", "2"]);
    assert!(out.0, "{}", out.1);
    assert_eq!(out.1["data"]["task"]["status"], "pending_release");
    assert!(!run(&["task", "continue", "1", "--if-version", "2"]).0);
    for (option, filter, count) in [
        ("--view", "pending-release", 1),
        ("--status", "pending_release", 1),
        ("--view", "active", 1),
        ("--view", "in-progress", 0),
    ] {
        let out = run(&["task", "list", option, filter]);
        assert!(out.0, "{}", out.1);
        assert_eq!(out.1["data"]["tasks"].as_array().unwrap().len(), count);
    }
    assert!(run(&["task", "continue", "1", "--if-version", "3"]).0);
    assert!(run(&["task", "pending-release", "1", "--if-version", "4"]).0);
    // CLI retains its existing explicit close-command contract (no new confirmation flag).
    assert!(
        !run(&[
            "task",
            "close",
            "1",
            "--if-version",
            "4",
            "--outcome",
            "completed"
        ])
        .0
    );
    assert!(
        run(&[
            "task",
            "close",
            "1",
            "--if-version",
            "5",
            "--outcome",
            "completed",
            "--yes"
        ])
        .0
    );
}

#[test]
fn cli_explicit_schema5_copy_keeps_source_and_requires_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    steward_core::set_private_dir(temp.path()).unwrap();
    let source = temp.path().join("schema5.db");
    let c = rusqlite::Connection::open(&source).unwrap();
    c.execute_batch(include_str!("../../application/src/migration/schema5.sql"))
        .unwrap();
    drop(c);
    let before = std::fs::read(&source).unwrap();
    let target = temp.path().join("new.db");
    for confirmed in [false, true] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_taskctl"));
        cmd.args([
            "--database",
            target.to_str().unwrap(),
            "--json",
            "database",
            "import-schema5",
            "--source",
            source.to_str().unwrap(),
        ]);
        if confirmed {
            cmd.arg("--yes");
        }
        let out = cmd.output().unwrap();
        assert_eq!(out.status.success(), confirmed, "{:?}", out);
        assert_eq!(target.exists(), confirmed);
    }
    assert_eq!(std::fs::read(source).unwrap(), before);
}
