use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn migration_cli_requires_explicit_destination_confirmation_and_never_overwrites() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.db");
    let target = temp.path().join("target.db");
    fs::write(&source, b"unchanged source").unwrap();
    for (args, field) in [
        (
            vec![
                "--json",
                "--yes",
                "database",
                "import-v7",
                "--source",
                source.to_str().unwrap(),
            ],
            "database",
        ),
        (
            vec![
                "--json",
                "--database",
                target.to_str().unwrap(),
                "database",
                "import-v7",
                "--source",
                source.to_str().unwrap(),
            ],
            "yes",
        ),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_taskctl"))
            .args(args)
            .output()
            .unwrap();
        assert!(!result.status.success());
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["error"]["details"]["field"], field);
        assert!(!target.exists());
    }
    fs::write(&target, b"unchanged target").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args([
            "--json",
            "--yes",
            "--database",
            target.to_str().unwrap(),
            "database",
            "import-v7",
            "--source",
            source.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(&target).unwrap(), b"unchanged target");
    assert_eq!(fs::read(&source).unwrap(), b"unchanged source");
}
