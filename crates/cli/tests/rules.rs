use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn run(db: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(db)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn cli_feedback_context_revision_disable_history_loop() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("db");
    let file = t.path().join("input.json");
    assert_eq!(run(&db, &["task", "create", "A"])["ok"], true);
    assert_eq!(run(&db, &["task", "create", "B"])["ok"], true);
    let before = run(&db, &["task", "show", "2"])["data"].clone();
    let mut input = json!({"scope":"global","projectId":null,"status":"active","contentVersion":1,"content":{"name":"合成偏好","body":"下次仍需读取这条规则","sources":[{"kind":"explicit","evidence":"任务A长期反馈","taskId":1,"taskVersion":1}]}});
    fs::write(&file, input.to_string()).unwrap();
    let path = file.to_str().unwrap();
    assert_eq!(
        run(
            &db,
            &["rule", "create", "--reason", "长期反馈", "--input", path]
        )["data"]["rule"]["revision"],
        1
    );
    assert_eq!(
        run(&db, &["task", "context", "B"])["data"]["sessionRules"]["rules"][0]["content"]["body"],
        input["content"]["body"]
    );
    input["content"]["body"] = json!("修正后的完整正文");
    fs::write(&file, input.to_string()).unwrap();
    let args = [
        "rule",
        "update",
        "1",
        "--if-revision",
        "1",
        "--reason",
        "修正",
        "--input",
        path,
    ];
    assert_eq!(run(&db, &args)["ok"], true);
    assert_eq!(run(&db, &args)["error"]["code"], "VERSION_CONFLICT");
    let human = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(&db)
        .args(["task", "context", "B"])
        .output()
        .unwrap();
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("修正后的完整正文"));
    assert!(text.contains("任务A长期反馈"));
    assert!(text.contains("Revision 2"));
    assert_eq!(
        run(
            &db,
            &[
                "rule",
                "disable",
                "1",
                "--if-revision",
                "2",
                "--reason",
                "撤回"
            ]
        )["ok"],
        true
    );
    assert_eq!(
        run(&db, &["task", "context", "B"])["data"]["sessionRules"]["rules"],
        json!([])
    );
    assert_eq!(
        run(&db, &["rule", "history", "1"])["data"]["history"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(run(&db, &["task", "show", "2"])["data"], before);
}
#[test]
fn cli_rule_input_rejects_invalid_types_unknown_versions_and_oversize_without_echo() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("absent.db");
    let file = t.path().join("input.json");
    for raw in [
        vec![0xff],
        b"{bad".to_vec(),
        vec![b' '; 128 * 1024 + 1],
        br#"{"scope":"SECRET_MARKER","extra":true}"#.to_vec(),
    ] {
        fs::write(&file, raw).unwrap();
        let out = run(
            &db,
            &[
                "rule",
                "create",
                "--reason",
                "invalid",
                "--input",
                file.to_str().unwrap(),
            ],
        );
        assert_eq!(out["error"]["code"], "INVALID_INPUT");
        assert!(!out.to_string().contains("SECRET_MARKER"));
        assert!(!db.exists());
    }
}
#[test]
fn schema6_cli_copy_is_explicit_and_preserves_source() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("old.db");
    let target = t.path().join("private").join("new.db");
    let c = rusqlite::Connection::open(&source).unwrap();
    c.execute_batch(include_str!("../../application/src/migration/schema6.sql"))
        .unwrap();
    c.execute_batch(
        "INSERT INTO tasks(status,version,created_at,updated_at) VALUES('open',1,'now','now');",
    )
    .unwrap();
    drop(c);
    let before = fs::read(&source).unwrap();
    assert_eq!(
        run(
            &target,
            &[
                "database",
                "import-schema6",
                "--source",
                source.to_str().unwrap()
            ]
        )["ok"],
        false
    );
    assert!(!target.exists());
    let out = run(
        &target,
        &[
            "--yes",
            "database",
            "import-schema6",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["data"]["targetSchema"], 7);
    assert_eq!(
        run(&target, &["task", "context", "1"])["data"]["sessionRules"]["rules"],
        json!([])
    );
    assert_eq!(fs::read(&source).unwrap(), before);
}
