use rusqlite::Connection;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn run(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .args(args)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.success(), value["ok"].as_bool().unwrap());
    value
}
fn source(path: &Path) {
    let c = Connection::open(path).unwrap();
    c.execute_batch(include_str!("../../application/src/migration/schema2.sql"))
        .unwrap();
    c.execute_batch("PRAGMA user_version=2;
        INSERT INTO tasks(id,status,version,created_at,updated_at) VALUES (1,'open',1,'2026-09-08T00:00:00Z','2026-09-08T00:00:00Z');
        UPDATE sqlite_sequence SET seq=50 WHERE name='tasks';").unwrap();
}

#[test]
fn schema2_cli_requires_explicit_destination_and_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.db");
    let target = temp.path().join("target.db");
    fs::write(&src, b"not opened without consent").unwrap();
    let args = [
        "--json",
        "--yes",
        "database",
        "import-schema2",
        "--source",
        src.to_str().unwrap(),
    ];
    let out = run(&args);
    assert_eq!(out["error"]["details"]["field"], "database");
    assert_eq!(out["warnings"], serde_json::json!([]));
    let out = run(&[
        "--json",
        "--database",
        target.to_str().unwrap(),
        "database",
        "import-schema2",
        "--source",
        src.to_str().unwrap(),
    ]);
    assert_eq!(out["error"]["details"]["field"], "yes");
    assert!(!target.exists());
    assert_eq!(fs::read(src).unwrap(), b"not opened without consent");
}

#[test]
fn schema2_cli_publishes_one_verified_copy_and_never_overwrites_it() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.db");
    source(&src);
    let target = temp.path().join("private").join("target.db");
    let before = fs::read(&src).unwrap();
    let args = [
        "--json",
        "--yes",
        "--database",
        target.to_str().unwrap(),
        "database",
        "import-schema2",
        "--source",
        src.to_str().unwrap(),
    ];
    let out = run(&args);
    assert_eq!(out["ok"], true);
    assert_eq!(out["data"]["sourceSchema"], 2);
    assert_eq!(out["data"]["targetSchema"], 5);
    assert_eq!(out["data"]["counts"]["tasks"], 1);
    assert_eq!(out["data"]["highWaterMarks"]["tasks"], 50);
    assert_eq!(
        out["data"]["tableSha256"]["tasks"].as_str().unwrap().len(),
        64
    );
    let target_before = fs::read(&target).unwrap();
    assert_eq!(run(&args)["ok"], false);
    assert_eq!(fs::read(&target).unwrap(), target_before);
    assert_eq!(fs::read(src).unwrap(), before);
    assert!(!Path::new(&format!("{}-wal", target.display())).exists());
    let read = run(&[
        "--json",
        "--database",
        target.to_str().unwrap(),
        "task",
        "show",
        "1",
    ]);
    assert_eq!(read["data"]["task"]["version"], 1);
    assert!(read["data"]["task"]["projectId"].is_null());
    let returned_path_read = run(&[
        "--json",
        "--database",
        out["data"]["destination"].as_str().unwrap(),
        "task",
        "show",
        "1",
    ]);
    assert_eq!(returned_path_read["data"]["task"], read["data"]["task"]);
}

#[cfg(windows)]
#[test]
fn schema2_cli_rejects_ambiguous_windows_paths_without_creating_a_target() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.db");
    source(&src);
    let before = fs::read(&src).unwrap();
    // Leave an occupied canonical sidecar in place: alias spelling must not bypass it.
    let sidecar = temp.path().join("source.db-shm");
    fs::create_dir(&sidecar).unwrap();
    for root in [
        temp.path().to_path_buf(),
        fs::canonicalize(temp.path()).unwrap(),
    ] {
        for tail in [".", " ", ". ", ".."] {
            for (source, destination) in [
                (
                    root.join("source.db"),
                    root.join("private").join(format!("target.db{tail}")),
                ),
                (
                    root.join("source.db"),
                    root.join(format!("private{tail}")).join("target.db"),
                ),
                (
                    root.join(format!("source.db{tail}")),
                    root.join("private").join("target.db"),
                ),
            ] {
                let out = run(&[
                    "--json",
                    "--yes",
                    "--database",
                    destination.to_str().unwrap(),
                    "database",
                    "import-schema2",
                    "--source",
                    source.to_str().unwrap(),
                ]);
                assert_eq!(out["ok"], false);
                assert_eq!(out["error"]["code"], "INVALID_INPUT");
                assert_eq!(out["error"]["details"]["field"], "database");
                assert!(
                    out["error"]["details"]["reason"]
                        .as_str()
                        .unwrap()
                        .contains("ending in dots or spaces")
                );
                // Catch literal extended-name files as well as normalized ones.
                assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
                assert_eq!(fs::read(&src).unwrap(), before);
                assert!(sidecar.is_dir());
            }
        }
    }
}

#[test]
fn schema2_cli_requires_a_literal_final_filename() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source.db");
    source(&src);
    let target = temp.path().join("private").join("target.db");
    for suffix in ["/", "/."] {
        let target = format!("{}{suffix}", target.display());
        let out = run(&[
            "--json",
            "--yes",
            "--database",
            &target,
            "database",
            "import-schema2",
            "--source",
            src.to_str().unwrap(),
        ]);
        assert_eq!(out["ok"], false);
        assert_eq!(out["error"]["details"]["field"], "database");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}

#[cfg(windows)]
#[test]
fn schema2_cli_normal_and_extended_paths_reopen_the_same_nonempty_database() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source with spaces.db");
    source(&src);
    for (i, root) in [
        temp.path().to_path_buf(),
        fs::canonicalize(temp.path()).unwrap(),
    ]
    .into_iter()
    .enumerate()
    {
        let target = root
            .join(format!("私有目录 {i}"))
            .join("target with spaces.db");
        let source = root.join("source with spaces.db");
        let out = run(&[
            "--json",
            "--yes",
            "--database",
            target.to_str().unwrap(),
            "database",
            "import-schema2",
            "--source",
            source.to_str().unwrap(),
        ]);
        assert_eq!(out["ok"], true);
        assert_eq!(out["data"]["counts"]["tasks"], 1);
        for path in [
            target.to_str().unwrap(),
            out["data"]["destination"].as_str().unwrap(),
        ] {
            let read = run(&["--json", "--database", path, "task", "show", "1"]);
            assert_eq!(read["ok"], true);
            assert_eq!(read["data"]["task"]["id"], 1);
            assert_eq!(read["data"]["task"]["version"], 1);
        }
    }
}
