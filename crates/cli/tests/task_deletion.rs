use serde_json::{Value, json};
use std::process::Command;
fn call(db: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(["--database", db.to_str().unwrap(), "--json"])
        .args(args)
        .output()
        .unwrap();
    let out: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        output.status.success(),
        out["ok"] == true,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}
#[test]
fn real_cli_previews_requires_confirmation_and_preserves_audit_after_single_deletions() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("synthetic-cli.db");
    call(&db, &["task", "create"]);
    call(&db, &["task", "create"]);
    let s = steward_application::Service::new(&db);
    s.task_work_update("1",1,&json!({"reason":"fixture","requirements":[{"id":"R1","title":"one","acceptanceCriteria":"works"}]}).to_string()).unwrap();
    assert_eq!(
        call(&db, &["task", "requirement-delete-preview", "1", "R1"])["data"]["requirement"]["id"],
        "R1"
    );
    let args = [
        "task",
        "requirement-delete",
        "1",
        "R1",
        "--if-version",
        "2",
        "--reason",
        "synthetic removal",
    ];
    assert_eq!(call(&db, &args)["error"]["code"], "INVALID_INPUT");
    let mut yes = vec!["--yes"];
    yes.extend(args);
    assert_eq!(call(&db, &yes)["data"]["work"]["requirements"], json!([]));
    assert_eq!(
        call(&db, &["task", "delete-preview", "1"])["data"]["mode"],
        "logical"
    );
    let args = [
        "task",
        "delete",
        "#1",
        "--if-version",
        "3",
        "--reason",
        "synthetic logical deletion",
    ];
    assert_eq!(call(&db, &args)["error"]["code"], "INVALID_INPUT");
    let mut yes = vec!["--yes"];
    yes.extend(args);
    assert_eq!(call(&db, &yes)["data"]["task"]["status"], "todo");
    assert_eq!(
        call(&db, &["task", "show", "1"])["error"]["code"],
        "TASK_DELETED"
    );
    assert_eq!(
        call(&db, &["task", "context", "1", "--require-read-only"])["error"]["code"],
        "TASK_DELETED"
    );
    let audit = call(&db, &["task", "audit", "1"]);
    assert_eq!(audit["data"]["work"]["requirements"], json!([]));
    assert!(audit["data"]["deletion"].is_object());
    assert_eq!(audit["data"]["history"].as_array().unwrap().len(), 4);
    assert_eq!(
        call(&db, &["task", "list", "--view", "recent"])["data"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(&db, &["task", "show", "2"])["data"]["task"]["version"],
        1
    );
    let missing = dir.path().join("missing.db");
    assert_eq!(call(&missing, &["task", "audit", "1"])["ok"], false);
    assert!(!missing.exists());
}
