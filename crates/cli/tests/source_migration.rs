use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn run(target: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(target)
        .args(["--json", "--yes"])
        .args(args)
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn schema4_5_6_git_sources_require_explicit_mapping_and_keep_the_real_source_version() {
    for (schema, sql) in [
        (
            4,
            include_str!("../../application/src/migration/schema4.sql"),
        ),
        (
            5,
            include_str!("../../application/src/migration/schema5.sql"),
        ),
        (
            6,
            include_str!("../../application/src/migration/schema6.sql"),
        ),
    ] {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("old.db");
        let target = t.path().join("private/new.db");
        let c = rusqlite::Connection::open(&source).unwrap();
        c.execute_batch(sql).unwrap();
        c.execute_batch("INSERT INTO projects(id,name,name_key,revision,created_at,updated_at) VALUES(1,'Fixture','fixture',1,'now','now'); INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES(1,1,'project.created','now','{}');").unwrap();
        let legacy_path = t.path().join("unobserved-git-path");
        let identity =
            json!({"canonical_path":legacy_path,"object":{"first":1,"second":2}}).to_string();
        c.execute("INSERT INTO repositories(id,common_dir,common_identity_json,created_at) VALUES(1,?1,?2,'now')", rusqlite::params![legacy_path.to_string_lossy(), identity]).unwrap();
        c.execute_batch("INSERT INTO source_roots(id,project_id,repository_id,relative_path,created_at) VALUES(1,1,1,'.','now');").unwrap();
        drop(c);
        let before = std::fs::read(&source).unwrap();
        let command = format!("import-schema{schema}");
        let args = [
            "database",
            command.as_str(),
            "--source",
            source.to_str().unwrap(),
        ];
        let rejected = run(&target, &args);
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert_eq!(rejected["error"]["code"], "INVALID_INPUT");
        assert_eq!(rejected["error"]["details"]["field"], "sourcePaths");
        let reason = rejected["error"]["details"]["reason"].as_str().unwrap();
        assert!(
            reason.contains(&command) && reason.contains("--source-paths"),
            "{rejected}"
        );
        assert!(!target.exists());
        let mapping = t.path().join("paths.json");
        let directory = t.path().join("unobserved-directory");
        std::fs::write(&mapping, json!({"1":directory}).to_string()).unwrap();
        let mut mapped = args.to_vec();
        mapped.extend(["--source-paths", mapping.to_str().unwrap()]);
        let result = run(&target, &mapped);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["data"]["sourceSchema"], schema);
        assert_eq!(result["data"]["targetSchema"], 8);
        let listed = run(&target, &["project", "source", "list", "1"]);
        assert_eq!(
            listed["data"]["sources"][0]["directoryPath"],
            directory.to_string_lossy().as_ref()
        );
        assert!(listed["data"].get("repositories").is_none());
        assert!(!directory.exists());
        assert_eq!(std::fs::read(&source).unwrap(), before);
    }
}
