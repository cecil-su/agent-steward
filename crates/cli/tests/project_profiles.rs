use std::{fs, path::Path, process::Command};
use serde_json::{Value, json};

fn run(db: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--json").arg("--database").arg(db).args(args).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("CLI output is not JSON: {:?}", output.stderr))
}

#[test]
fn cli_profile_requires_provenance_and_cas_and_injects_read_only_context() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("new.db");
    let file = temp.path().join("profile.json");
    assert_eq!(run(&db, &["project","create","--name","Profile"])["ok"], true);
    assert_eq!(run(&db, &["task","create","SOURCE","--project","##1"])["ok"], true);
    let before = run(&db, &["task","show","1"])["data"].clone();
    let task_history = run(&db, &["history","1"])["data"].clone();
    assert!(run(&db, &["project","profile","show","##1"])["data"]["profile"].is_null());
    let payload = json!({"summary":"简介","architecture":"入口与结构","development":"隔离验证", "evidence":"synthetic source only", "sourceTaskId":1,"sourceTaskVersion":1});
    fs::write(&file, payload.to_string()).unwrap();
    let args = ["project","profile","set","##1","--if-revision","1","--input",file.to_str().unwrap()];
    let result = run(&db, &args);
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["project"]["revision"], 2);
    assert_eq!(run(&db, &args)["error"]["code"], "VERSION_CONFLICT");
    let context = run(&db, &["task","context","1"]);
    assert_eq!(context["data"]["projectProfile"]["summary"], "简介");
    assert_eq!(context["data"]["projectProfile"]["sourceTaskVersion"], 1);
    assert_eq!(run(&db, &["task","show","1"])["data"], before);
    assert_eq!(run(&db, &["history","1"])["data"], task_history);
    fs::write(&file, "{}").unwrap();
    assert_eq!(run(&db, &["project","profile","set","##1","--if-revision","2","--input",file.to_str().unwrap()])["ok"], false);
    assert_eq!(run(&db, &["project","show","##1"])["data"]["project"]["revision"], 2);
}

#[test]
fn profile_input_is_bounded_and_parse_errors_do_not_echo_values() {
    use std::io::Write;
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("must-not-exist.db");
    let file = temp.path().join("invalid.json");
    fs::write(&file, r#"{"summary":"safe","architecture":"safe","development":"safe","evidence":"safe","sourceTaskId":1,"sourceTaskVersion":"PRIVATE_MARKER"}"#).unwrap();
    let out = run(&db, &["project","profile","set","##1","--if-revision","1","--input",file.to_str().unwrap()]);
    assert_eq!(out["ok"], false);
    assert!(!out.to_string().contains("PRIVATE_MARKER"));
    assert!(!db.exists());
    let mut child = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--json").arg("--database").arg(&db)
        .args(["project","profile","set","##1","--if-revision","1","--input","-"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&vec![b' '; 512 * 1024 + 1]).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("512 KiB"));
    assert!(!db.exists());
}

#[test]
fn schema4_cli_copy_requires_explicit_confirmation_and_keeps_source_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("old.db");
    let target = temp.path().join("private").join("new.db");
    let c = rusqlite::Connection::open(&source).unwrap();
    c.execute_batch(include_str!("../../application/src/migration/schema4.sql")).unwrap();
    drop(c);
    let before = fs::read(&source).unwrap();
    let args = ["database","import-schema4","--source",source.to_str().unwrap()];
    assert_eq!(run(&target, &args)["ok"], false);
    assert!(!target.exists());
    let out = run(&target, &["--yes","database","import-schema4","--source",source.to_str().unwrap()]);
    assert_eq!(out["ok"], true);
    assert_eq!(out["data"]["sourceSchema"], 4);
    assert_eq!(out["data"]["targetSchema"], 5);
    assert_eq!(fs::read(source).unwrap(), before);
}
