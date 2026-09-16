use serde_json::Value;
use std::process::Command;

#[test]
fn cli_seven_statuses_cas_noop_filters_and_retired_commands() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("status.db");
    let service = steward_application::Service::new(&db);
    service.task_create_minimal().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_taskctl"))
            .args(["--database", db.to_str().unwrap(), "--json"])
            .args(args)
            .output()
            .unwrap();
        (
            out.status.success(),
            serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        )
    };
    assert_eq!(
        run(&["task", "show", "1"]).1["data"]["task"]["status"],
        "todo"
    );
    assert!(
        run(&[
            "task",
            "claim",
            "1",
            "--session",
            "fixture",
            "--if-version",
            "1"
        ])
        .0
    );
    assert_eq!(
        service.task_show("1").unwrap().data["task"]["status"],
        "todo"
    );
    let sessions = service.session_list(Some("1")).unwrap().data;
    for status in [
        "backlog",
        "todo",
        "in_progress",
        "in_review",
        "blocked",
        "done",
        "cancelled",
        "todo",
    ] {
        let version = service.task_show("1").unwrap().data["task"]["version"]
            .as_i64()
            .unwrap();
        let args = ["task", "status", "1", status, "--if-version"];
        let mut change = args.to_vec();
        let v = version.to_string();
        change.push(&v);
        let (ok, out) = run(&change);
        assert!(ok, "{out}");
        assert_eq!(out["schemaVersion"], 3);
        assert_eq!(out["data"]["task"]["status"], status);
        assert_eq!(out["data"]["task"]["currentSessionId"], "fixture");
        let before = service.task_show("1").unwrap().data;
        let history = service.history("1").unwrap().data;
        assert_eq!(run(&change).1["error"]["code"], "VERSION_CONFLICT");
        let next = (version + 1).to_string();
        *change.last_mut().unwrap() = &next;
        assert!(run(&change).0);
        assert_eq!(service.task_show("1").unwrap().data, before);
        assert_eq!(service.history("1").unwrap().data, history);
        assert_eq!(service.session_list(Some("1")).unwrap().data, sessions);
        for (filter, count) in [
            (status, 1),
            (
                "active",
                usize::from(!["done", "cancelled"].contains(&status)),
            ),
        ] {
            let (ok, out) = run(&["task", "list", "--status", filter]);
            assert!(ok, "{out}");
            assert_eq!(out["data"]["tasks"].as_array().unwrap().len(), count);
        }
    }
    let before = service.task_show("1").unwrap().data;
    for command in [
        "close",
        "block",
        "unblock",
        "pending-release",
        "continue",
        "here",
    ] {
        assert!(!run(&["task", command, "1"]).0);
    }
    for args in [
        vec!["worktree", "status", "1"],
        vec!["project", "here"],
        vec!["project", "context", "1", "--source", "1"],
        vec!["project", "source", "resolve", "1", "1"],
    ] {
        assert!(!run(&args).0);
    }
    let (ok, context) = run(&["task", "context", "1", "--require-read-only"]);
    assert!(ok, "{context}");
    assert!(context["data"].get("worktreeStatus").is_none());
    for field in [
        "worktreePath",
        "repositoryPath",
        "repositoryCommonDir",
        "repositoryBranch",
    ] {
        assert!(context["data"]["task"].get(field).is_none());
    }
    assert_eq!(service.task_show("1").unwrap().data, before);
}

#[test]
fn cli_explicit_schema5_copy_keeps_source_and_requires_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    steward_core::set_private_dir(temp.path()).unwrap();
    let source = temp.path().join("schema5.db");
    let c = rusqlite::Connection::open(&source).unwrap();
    c.execute_batch(include_str!("../../application/src/migration/schema5.sql"))
        .unwrap();
    drop(c);
    let before = std::fs::read(&source).unwrap();
    let target = temp.path().join("new.db");
    for confirmed in [false, true] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_taskctl"));
        cmd.args([
            "--database",
            target.to_str().unwrap(),
            "--json",
            "database",
            "import-schema5",
            "--source",
            source.to_str().unwrap(),
        ]);
        if confirmed {
            cmd.arg("--yes");
        }
        let out = cmd.output().unwrap();
        assert_eq!(out.status.success(), confirmed, "{out:?}");
        assert_eq!(target.exists(), confirmed);
    }
    assert_eq!(std::fs::read(source).unwrap(), before);
}
