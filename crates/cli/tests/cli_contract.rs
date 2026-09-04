use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(arguments)
        .output()
        .expect("taskctl should start")
}

fn run_with_stdin(arguments: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("taskctl should start");
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
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
            "title":"0904｜功能｜CLI contract",
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
    assert_eq!(created["schemaVersion"], 2);
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
fn json_cli_supports_minimal_create_stdin_and_all_task_references() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("references.db");
    let database_arg = database.to_str().unwrap();

    let minimal = run(&["--database", database_arg, "--json", "task", "create"]);
    assert!(minimal.status.success());
    let minimal = json_output(&minimal);
    assert_eq!(minimal["data"]["task"]["id"], 1);
    assert!(minimal["data"]["task"]["taskKey"].is_null());
    assert!(minimal["data"]["task"]["title"].is_null());

    let full_input = serde_json::to_vec(&json!({
        "taskKey":"STDIN-KEY",
        "title":"0904｜功能｜Created from stdin",
        "goal":"Read a complete JSON document",
        "scope":"CLI stdin",
        "acceptanceCriteria":"All references resolve",
        "nextStep":"Patch by numeric id"
    }))
    .unwrap();
    let created = run_with_stdin(
        &[
            "--database",
            database_arg,
            "--json",
            "--input",
            "-",
            "task",
            "create",
        ],
        &full_input,
    );
    assert!(created.status.success());
    let created = json_output(&created);
    assert_eq!(created["schemaVersion"], 2);
    assert_eq!(created["data"]["task"]["id"], 2);
    assert_eq!(created["data"]["task"]["taskKey"], "STDIN-KEY");

    let patched = run_with_stdin(
        &[
            "--database",
            database_arg,
            "--json",
            "--input",
            "-",
            "task",
            "update",
            "#2",
            "--if-version",
            "1",
        ],
        r#"{"title":"0904｜优化｜Patched from stdin","nextStep":null}"#.as_bytes(),
    );
    assert!(patched.status.success());
    assert_eq!(json_output(&patched)["data"]["task"]["version"], 2);

    for reference in ["2", "#2", "STDIN-KEY", "key:STDIN-KEY"] {
        let shown = run(&[
            "--database",
            database_arg,
            "--json",
            "task",
            "show",
            reference,
        ]);
        assert!(
            shown.status.success(),
            "reference {reference} should resolve"
        );
        assert_eq!(json_output(&shown)["data"]["task"]["id"], 2);
    }

    let human = run(&["--database", database_arg, "task", "show", "STDIN-KEY"]);
    assert!(human.status.success());
    assert_eq!(json_output(&human)["task"]["id"], "#2");
}

#[test]
fn cli_retitle_updates_a_closed_task_without_reopening_it() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("retitle.db");
    let database_arg = database.to_str().unwrap();
    let input = serde_json::to_vec(&json!({
        "taskKey":"RETITLE",
        "title":"0904｜功能｜Before retitle",
        "goal":"Correct display metadata",
        "scope":"Task title only",
        "acceptanceCriteria":"Closed task remains closed"
    }))
    .unwrap();
    assert!(
        run_with_stdin(
            &[
                "--database",
                database_arg,
                "--json",
                "--input",
                "-",
                "task",
                "create",
            ],
            &input,
        )
        .status
        .success()
    );
    assert!(
        run(&[
            "--database",
            database_arg,
            "--json",
            "task",
            "claim",
            "#1",
            "--session",
            "session-a",
            "--if-version",
            "1",
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "--database",
            database_arg,
            "--json",
            "task",
            "close",
            "#1",
            "--if-version",
            "2",
            "--outcome",
            "completed",
        ])
        .status
        .success()
    );

    let retitled = run(&[
        "--database",
        database_arg,
        "--json",
        "task",
        "retitle",
        "#1",
        "--if-version",
        "3",
        "--title",
        "0904｜文档｜After retitle",
    ]);
    assert!(retitled.status.success());
    let retitled = json_output(&retitled);
    assert_eq!(
        retitled["data"]["task"]["title"],
        "0904｜文档｜After retitle"
    );
    assert_eq!(retitled["data"]["task"]["status"], "closed");
    assert_eq!(retitled["data"]["task"]["version"], 4);

    let history = run(&["--database", database_arg, "--json", "history", "#1"]);
    assert!(history.status.success());
    let history = json_output(&history);
    let last = history["data"]["history"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(last["changeType"], "task.retitled");
    assert_eq!(
        last["payload"]["previousTitle"],
        "0904｜功能｜Before retitle"
    );
    assert_eq!(last["payload"]["title"], "0904｜文档｜After retitle");
}

#[test]
fn stdin_input_errors_are_stable_and_do_not_panic() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("stdin-errors.db");
    let database_arg = database.to_str().unwrap();
    let arguments = [
        "--database",
        database_arg,
        "--json",
        "--input",
        "-",
        "task",
        "create",
    ];

    for (input, reason_fragment) in [
        (Vec::new(), "must not be empty"),
        (b"{".to_vec(), "EOF while parsing"),
        (br#"{"unknown":true}"#.to_vec(), "unknown field"),
        (vec![0xff, 0xfe], "valid UTF-8"),
    ] {
        let output = run_with_stdin(&arguments, &input);
        assert_eq!(output.status.code(), Some(2));
        let output = json_output(&output);
        assert_eq!(output["schemaVersion"], 2);
        assert_eq!(output["error"]["code"], "INVALID_INPUT");
        assert_eq!(output["error"]["details"]["field"], "input");
        assert!(
            output["error"]["details"]["reason"]
                .as_str()
                .unwrap()
                .contains(reason_fragment),
            "unexpected error: {output}"
        );
    }
}

#[test]
fn task_list_cli_supports_projection_cursor_and_terminal_formats() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("task-list.db");
    let database_arg = database.to_str().unwrap();
    for (key, title) in [
        ("LIST-A", "0904｜功能｜Terminal A"),
        ("LIST-B", "0904｜功能｜Terminal B"),
        ("LIST-C", "0904｜功能｜Terminal C"),
    ] {
        let input = serde_json::to_vec(&json!({
            "title":title,
            "goal":"Filter and paginate tasks",
            "scope":"Terminal list output",
            "acceptanceCriteria":"Output remains readable"
        }))
        .unwrap();
        let created = run_with_stdin(
            &[
                "--database",
                database_arg,
                "--json",
                "--input",
                "-",
                "task",
                "create",
                key,
            ],
            &input,
        );
        assert!(created.status.success());
    }

    let first = run(&[
        "--database",
        database_arg,
        "--json",
        "task",
        "list",
        "--query",
        "Terminal",
        "--fields",
        "title",
        "--page-size",
        "2",
    ]);
    assert!(first.status.success());
    let first = json_output(&first);
    assert_eq!(first["data"]["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(first["data"]["tasks"][0].as_object().unwrap().len(), 1);
    assert_eq!(first["data"]["hasMore"], true);
    assert_eq!(first["data"]["pageSize"], 2);
    let cursor = first["data"]["nextCursor"].as_str().unwrap();
    let second = run(&[
        "--database",
        database_arg,
        "--json",
        "task",
        "list",
        "--query",
        "Terminal",
        "--fields",
        "title",
        "--page-size",
        "2",
        "--cursor",
        cursor,
    ]);
    assert!(second.status.success());
    let second = json_output(&second);
    assert_eq!(second["data"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(second["data"]["hasMore"], false);
    assert!(second["data"]["nextCursor"].is_null());

    let table = run(&[
        "--database",
        database_arg,
        "task",
        "list",
        "--fields",
        "id,title",
    ]);
    assert!(table.status.success());
    let table = String::from_utf8(table.stdout).unwrap();
    assert!(table.contains("ID"));
    assert!(table.contains("TITLE"));
    assert!(table.contains("#1"));
    assert!(table.contains("Showing 3 tasks"));
    assert!(!table.starts_with('{'));
    assert!(!table.contains('\u{1b}'));

    let lines = run(&[
        "--database",
        database_arg,
        "task",
        "list",
        "--fields",
        "title",
        "--format",
        "lines",
    ]);
    assert!(lines.status.success());
    let lines = String::from_utf8(lines.stdout).unwrap();
    assert_eq!(lines.lines().count(), 3);
    assert!(lines.lines().all(|line| line.contains("Terminal")));
    let null_lines = run(&[
        "--database",
        database_arg,
        "task",
        "list",
        "--fields",
        "nextStep",
        "--format",
        "lines",
    ]);
    assert!(null_lines.status.success());
    let null_lines = String::from_utf8(null_lines.stdout).unwrap();
    assert_eq!(null_lines.lines().collect::<Vec<_>>(), vec!["—"; 3]);

    let unknown = run(&[
        "--database",
        database_arg,
        "--json",
        "task",
        "list",
        "--fields",
        "unknown",
    ]);
    assert_eq!(unknown.status.code(), Some(2));
    let unknown = json_output(&unknown);
    assert_eq!(unknown["error"]["code"], "INVALID_INPUT");
    assert_eq!(unknown["error"]["details"]["field"], "fields");
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
            "title":"0904｜功能｜Import confirmation",
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
    assert_eq!(invalid["schemaVersion"], 2);
    assert_eq!(invalid["ok"], false);
    assert!(invalid["data"].is_null());
    assert!(invalid["warnings"].as_array().unwrap().is_empty());
    assert_eq!(invalid["error"]["code"], "INVALID_INPUT");
    assert_eq!(invalid["error"]["details"]["field"], "arguments");
    assert!(invalid["error"]["details"]["reason"].is_string());

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
