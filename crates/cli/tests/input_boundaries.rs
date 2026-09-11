use serde_json::Value;
use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}
fn error(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn json_flag_respects_positional_terminator_and_option_values() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    for args in [
        vec!["task", "show", "--", "--json", "extra"],
        vec!["task", "show", "--", "--json"],
        vec!["--input=--json", "unknown"],
    ] {
        let result = run(&db, &args);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty(), "{result:?}");
        assert!(!result.stderr.is_empty());
    }
    for args in [
        vec!["--json", "task", "show", "--", "--json", "extra"],
        vec!["task", "show", "--json"],
        vec!["--json", "unknown"],
        vec!["--input=--json", "--json", "unknown"],
    ] {
        error(&run(&db, &args));
    }
    for args in [
        vec!["--help", "--json"],
        vec!["task", "--help", "--json"],
        vec!["--version", "--json"],
    ] {
        let output = run(&db, &args);
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["ok"],
            true
        );
    }
}

#[test]
fn invalid_page_size_does_not_create_or_inspect_database() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    for size in ["0", "201", "4294967295"] {
        error(&run(&db, &["--json", "task", "list", "--page-size", size]));
        assert!(!db.exists());
    }
    fs::write(&db, b"not sqlite").unwrap();
    error(&run(&db, &["--json", "task", "list", "--page-size", "0"]));
    assert_eq!(fs::read(db).unwrap(), b"not sqlite");
}

#[test]
fn generic_input_has_inclusive_16_mib_file_and_stdin_limit() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    let input = temp.path().join("input.json");
    let mut bytes = vec![b' '; 16 * 1024 * 1024];
    bytes[..2].copy_from_slice(b"{}");
    fs::write(&input, &bytes).unwrap();
    assert!(
        run(
            &db,
            &[
                "--json",
                "--input",
                input.to_str().unwrap(),
                "task",
                "create"
            ]
        )
        .status
        .success()
    );
    bytes.push(b' ');
    fs::write(&input, &bytes).unwrap();
    let rejected = error(&run(
        &db,
        &[
            "--json",
            "--input",
            input.to_str().unwrap(),
            "task",
            "create",
        ],
    ));
    assert!(rejected["error"]["details"].to_string().contains("16 MiB"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(&db)
        .args(["--json", "--input", "-", "task", "create"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Keep stdin open: rejection must happen at limit+1, not wait for EOF.
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&bytes).unwrap();
    error(&child.wait_with_output().unwrap());
    drop(stdin);
}

#[test]
fn generic_input_rejects_non_files_and_json_confirmation_never_prompts() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    error(&run(
        &db,
        &[
            "--json",
            "--input",
            temp.path().to_str().unwrap(),
            "task",
            "create",
        ],
    ));
    #[cfg(windows)]
    for path in ["NUL", r"\\.\pipe\steward-test-must-not-open", r"\\.\NUL"] {
        error(&run(&db, &["--json", "--input", path, "task", "create"]));
    }
    #[cfg(unix)]
    {
        let fifo = temp.path().join("fifo");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        error(&run(
            &db,
            &[
                "--json",
                "--input",
                fifo.to_str().unwrap(),
                "task",
                "create",
            ],
        ));
    }
    let output = run(
        &db,
        &["--json", "hook", "clear", "missing", "--if-version", "1"],
    );
    assert_eq!(error(&output)["error"]["details"]["field"], "yes");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("[y/N]"));
    let output = run(&db, &["--verbose", "--json", "task", "list"]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["ok"],
        true
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("command parsed"));
}
