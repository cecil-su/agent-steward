use std::fs;
use std::process::{Command, Output, Stdio};

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
fn concurrent_first_startup_serializes_migrations() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("concurrent.db");
    let children = (0..12)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_taskctl"))
                .args(["--database", database.to_str().unwrap(), "--json", "doctor"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("taskctl should start")
        })
        .collect::<Vec<_>>();

    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn json_import_requires_sensitive_content_confirmation_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("steward.db");
    let task_input = temp.path().join("task.json");
    let import_file = temp.path().join("session.json");
    fs::write(
        &task_input,
        serde_json::to_vec(&json!({
            "title":"Import confirmation",
            "goal":"Require review before persistence",
            "scope":"Session import",
            "acceptanceCriteria":"Unconfirmed content is not stored"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&import_file, br#"{"messages":["reviewed"]}"#).unwrap();

    assert!(
        run(&[
            "--database",
            database.to_str().unwrap(),
            "--json",
            "--input",
            task_input.to_str().unwrap(),
            "task",
            "create",
            "TASK-IMPORT",
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "--database",
            database.to_str().unwrap(),
            "--json",
            "task",
            "claim",
            "TASK-IMPORT",
            "--session",
            "session-a",
            "--if-version",
            "1",
        ])
        .status
        .success()
    );

    let unconfirmed = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "session",
        "import",
        "add",
        "TASK-IMPORT",
        "--session",
        "session-a",
        "--if-version",
        "2",
        "--file",
        import_file.to_str().unwrap(),
    ]);
    assert_eq!(unconfirmed.status.code(), Some(2));
    let unconfirmed = json_output(&unconfirmed);
    assert_eq!(
        unconfirmed["error"]["details"]["field"],
        "confirmSensitiveContentReviewed"
    );
    let imports = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "session",
        "import",
        "list",
        "session-a",
    ]);
    assert!(imports.status.success());
    assert!(
        json_output(&imports)["data"]["imports"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let confirmed = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "session",
        "import",
        "add",
        "TASK-IMPORT",
        "--session",
        "session-a",
        "--if-version",
        "2",
        "--file",
        import_file.to_str().unwrap(),
        "--confirm-sensitive-content-reviewed",
    ]);
    assert!(confirmed.status.success());
    assert_eq!(json_output(&confirmed)["data"]["task"]["version"], 3);
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

    let missing = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "task",
        "show",
        "MISSING",
    ]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        json_output(&missing)["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "INSECURE_DATABASE_PERMISSIONS")
    );
}

#[cfg(unix)]
#[test]
fn relative_database_path_checks_the_actual_parent_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .current_dir(temp.path())
        .args(["--database", "state.db", "--json", "doctor"])
        .output()
        .expect("taskctl should start");
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
}

#[cfg(unix)]
#[test]
fn loose_database_file_warns_when_a_command_fails_before_opening_it() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let database = temp.path().join("steward.db");
    fs::write(&database, []).unwrap();
    fs::set_permissions(&database, fs::Permissions::from_mode(0o644)).unwrap();

    let output = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "session",
        "import",
        "add",
        "TASK-1",
        "--session",
        "session-a",
        "--if-version",
        "1",
        "--file",
        "session.json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let output = json_output(&output);
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning["code"] == "INSECURE_DATABASE_PERMISSIONS"
                    && warning["details"]["path"] == database.to_str().unwrap()
                    && warning["details"]["mode"] == "0644"
            }),
        "a loose database file must warn even when the command fails before opening it: {output}"
    );
}

#[cfg(windows)]
#[test]
fn custom_windows_database_warning_is_included_on_failure() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("steward.db");
    let output = run(&[
        "--database",
        database.to_str().unwrap(),
        "--json",
        "task",
        "show",
        "MISSING",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let output = json_output(&output);
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning["code"] == "INSECURE_DATABASE_PERMISSIONS"
                    && warning["details"]["reason"]
                        == "custom parent directory ACL may allow database replacement"
            })
    );
}

#[cfg(windows)]
#[test]
fn default_windows_database_directory_has_a_protected_dacl() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .env("LOCALAPPDATA", temp.path())
        .args(["--json", "doctor"])
        .output()
        .expect("taskctl should start");
    assert!(output.status.success());
    let output = json_output(&output);
    assert!(
        !output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "INSECURE_DATABASE_PERMISSIONS")
    );
    assert!(steward_core::private_acl_is_protected(&temp.path().join("agent-steward")).unwrap());
}

#[cfg(windows)]
#[test]
fn loose_database_file_warns_when_a_command_fails_before_opening_it() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("agent-steward");
    fs::create_dir(&directory).unwrap();
    steward_core::set_private_dir(&directory).unwrap();
    let database = directory.join("steward.db");
    fs::write(&database, []).unwrap();
    let loose = Command::new("icacls")
        .args([
            database.to_str().unwrap(),
            "/inheritance:r",
            "/grant:r",
            "*S-1-1-0:F",
        ])
        .output()
        .expect("icacls should start");
    assert!(
        loose.status.success(),
        "icacls failed: {}",
        String::from_utf8_lossy(&loose.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .env("LOCALAPPDATA", temp.path())
        .args([
            "--json",
            "session",
            "import",
            "add",
            "TASK-1",
            "--session",
            "session-a",
            "--if-version",
            "1",
            "--file",
            "session.json",
        ])
        .output()
        .expect("taskctl should start");
    assert_eq!(output.status.code(), Some(2));
    let output = json_output(&output);
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning["code"] == "INSECURE_DATABASE_PERMISSIONS"
                    && warning["details"]["path"] == database.to_str().unwrap()
            }),
        "a loose database file must warn even when the command fails before opening it: {output}"
    );
}
