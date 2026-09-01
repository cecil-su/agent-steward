use std::fs;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(arguments)
        .output()
        .expect("taskctl should start")
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should be one JSON object: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn json_cli_supports_create_claim_checkpoint_resume_and_conflict() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("steward.db");
    let task_input = temp.path().join("task.json");
    fs::write(
        &task_input,
        serde_json::to_vec(&json!({
            "title":"CLI contract",
            "goal":"Exercise the public process boundary",
            "scope":"Synthetic temporary database",
            "acceptanceCriteria":"Stable JSON envelopes",
            "nextStep":"Claim"
        }))
        .unwrap(),
    )
    .unwrap();

    let created = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "--input",
        task_input.to_str().unwrap(),
        "task",
        "create",
        "TASK-CLI",
    ]);
    assert!(created.status.success());
    let created = json_output(&created);
    assert_eq!(created["schemaVersion"], 1);
    assert_eq!(created["ok"], true);
    assert_eq!(created["data"]["task"]["version"], 1);
    assert!(created["error"].is_null());

    let claimed = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "task",
        "claim",
        "TASK-CLI",
        "--session",
        "session-a",
        "--if-version",
        "1",
    ]);
    assert!(claimed.status.success());
    assert_eq!(json_output(&claimed)["data"]["task"]["version"], 2);

    let checkpoint_input = temp.path().join("checkpoint.json");
    fs::write(
        &checkpoint_input,
        serde_json::to_vec(&json!({
            "summary":"CLI flow checkpoint",
            "completed":["create","claim"],
            "decisions":["use JSON envelope"],
            "pending":["resume"],
            "nextStep":"resume in session-b",
            "risks":[]
        }))
        .unwrap(),
    )
    .unwrap();
    let checkpoint = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "--input",
        checkpoint_input.to_str().unwrap(),
        "task",
        "checkpoint",
        "TASK-CLI",
        "--session",
        "session-a",
        "--if-version",
        "2",
    ]);
    assert!(checkpoint.status.success());
    assert_eq!(json_output(&checkpoint)["data"]["task"]["version"], 3);

    let resumed = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "task",
        "resume",
        "TASK-CLI",
        "--session",
        "session-b",
        "--from-session",
        "session-a",
        "--take-over",
        "--if-version",
        "3",
    ]);
    assert!(resumed.status.success());
    let resumed = json_output(&resumed);
    assert_eq!(resumed["data"]["task"]["version"], 4);
    assert_eq!(resumed["data"]["sessions"][1]["continuedFrom"], "session-a");

    let stale = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "--input",
        task_input.to_str().unwrap(),
        "task",
        "update",
        "TASK-CLI",
        "--if-version",
        "1",
    ]);
    assert_eq!(stale.status.code(), Some(4));
    let stale = json_output(&stale);
    assert_eq!(stale["ok"], false);
    assert!(stale["data"].is_null());
    assert_eq!(stale["error"]["code"], "VERSION_CONFLICT");
    assert_eq!(stale["error"]["details"]["expectedVersion"], 1);
    assert_eq!(stale["error"]["details"]["currentVersion"], 4);
}

#[test]
fn json_parse_errors_and_doctor_keep_the_envelope_contract() {
    let invalid = run(&["--json", "not-a-command"]);
    assert_eq!(invalid.status.code(), Some(2));
    let invalid = json_output(&invalid);
    assert_eq!(invalid["schemaVersion"], 1);
    assert_eq!(invalid["ok"], false);
    assert!(invalid["data"].is_null());
    assert!(invalid["warnings"].as_array().unwrap().is_empty());
    assert_eq!(invalid["error"]["code"], "INVALID_INPUT");

    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("steward.db");
    let doctor = run(&["--database", database.to_str().unwrap(), "--json", "doctor"]);
    assert!(doctor.status.success());
    let doctor = json_output(&doctor);
    let codes = doctor["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| check["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"SCHEMA_VERSION"));
    assert!(codes.contains(&"SQLITE_QUICK_CHECK"));
    assert!(codes.contains(&"FOREIGN_KEY_CHECK"));
    assert!(codes.contains(&"RECORD_PATH_REFERENCES"));
    assert!(codes.contains(&"WORKTREE_REFERENCES"));
}

#[cfg(unix)]
#[test]
fn custom_database_in_shared_directory_returns_permission_warning() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let database = temp.path().join("steward.db");
    let output = run(&["--database", database.to_str().unwrap(), "--json", "doctor"]);
    assert!(output.status.success());
    let output = json_output(&output);
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning["code"] == "INSECURE_DATABASE_PERMISSIONS"
                    && warning["details"]["mode"] == "0755"
            })
    );
    let resulting_mode = fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        resulting_mode, 0o755,
        "taskctl must not chmod an existing parent"
    );
}
