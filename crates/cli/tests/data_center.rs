use serde_json::{Value, json};
use std::{path::Path, process::Command};

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
fn directory_sources_are_metadata_only_and_retired_reads_cannot_expose_files() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("db");
    let root = t.path().join("documents");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("private.txt"), "PRIVATE_FILE_SENTINEL").unwrap();
    assert_eq!(
        run(&db, &["project", "create", "--name", "Docs"])["ok"],
        true
    );
    assert_eq!(
        run(
            &db,
            &[
                "project",
                "component",
                "add",
                "1",
                "--name",
                "docs",
                "--if-revision",
                "1"
            ]
        )["ok"],
        true
    );
    let args = [
        "project",
        "source",
        "add",
        "1",
        "--component",
        "docs",
        "--directory",
        root.to_str().unwrap(),
        "--if-revision",
        "2",
    ];
    let out = run(&db, &args);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(run(&db, &args)["error"]["code"], "VERSION_CONFLICT");
    let sources = run(&db, &["project", "source", "list", "1"]);
    assert!(sources["data"].get("repositories").is_none());
    let source = sources["data"]["sources"][0].as_object().unwrap();
    assert_eq!(source.len(), 5);
    for field in [
        "id",
        "projectId",
        "componentId",
        "directoryPath",
        "createdAt",
    ] {
        assert!(source.contains_key(field));
    }
    assert_eq!(source["componentId"], 1);
    assert!(!sources.to_string().contains("PRIVATE_FILE_SENTINEL"));
    assert_eq!(run(&db, &["task", "create", "--project", "1"])["ok"], true);
    let before = run(&db, &["task", "show", "1"])["data"].clone();
    for args in [
        vec!["project", "here"],
        vec!["project", "source", "resolve", "1", "1"],
        vec![
            "project",
            "context",
            "1",
            "--source",
            "1",
            "--file",
            "private.txt",
        ],
        vec![
            "project",
            "source",
            "add",
            "1",
            "--repo",
            root.to_str().unwrap(),
            "--path",
            ".",
            "--if-revision",
            "3",
        ],
    ] {
        let out = run(&db, &args);
        assert_eq!(out["ok"], false);
        assert!(!out.to_string().contains("PRIVATE_FILE_SENTINEL"));
    }
    assert_eq!(run(&db, &["task", "show", "1"])["data"], before);
    assert_eq!(
        run(
            &db,
            &[
                "project",
                "source",
                "remove",
                "1",
                "1",
                "--if-revision",
                "3"
            ]
        )["ok"],
        true
    );
    assert_eq!(
        std::fs::read_to_string(root.join("private.txt")).unwrap(),
        "PRIVATE_FILE_SENTINEL"
    );
}

#[test]
fn schema7_import_is_explicit_preserves_source_and_reads_path_mapping_file() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("schema7.db");
    let c = rusqlite::Connection::open(&source).unwrap();
    c.execute_batch(include_str!("../../application/src/migration/schema7.sql"))
        .unwrap();
    c.execute_batch(
        "INSERT INTO tasks(status,version,created_at,updated_at) VALUES('open',1,'now','now');",
    )
    .unwrap();
    drop(c);
    let before = std::fs::read(&source).unwrap();
    let target = t.path().join("private/new.db");
    let mapping = t.path().join("paths.json");
    std::fs::write(&mapping, json!({}).to_string()).unwrap();
    let args = [
        "database",
        "import-schema7",
        "--source",
        source.to_str().unwrap(),
        "--source-paths",
        mapping.to_str().unwrap(),
    ];
    assert_eq!(run(&target, &args)["ok"], false);
    assert!(!target.exists());
    let mut confirmed = args.to_vec();
    confirmed.push("--yes");
    let out = run(&target, &confirmed);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["schemaVersion"], 3);
    assert_eq!(out["data"]["sourceSchema"], 7);
    assert_eq!(out["data"]["targetSchema"], 8);
    assert_eq!(
        run(&target, &["task", "show", "1"])["data"]["task"]["status"],
        "todo"
    );
    assert_eq!(run(&target, &confirmed)["ok"], false);
    assert_eq!(std::fs::read(&source).unwrap(), before);
    let default_target = t.path().join("private/default.db");
    assert_eq!(
        run(
            &default_target,
            &[
                "database",
                "import-schema7",
                "--source",
                source.to_str().unwrap(),
                "--yes"
            ]
        )["ok"],
        true
    );
}
