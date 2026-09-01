use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use steward_application::Service;

fn service(temp: &tempfile::TempDir) -> Service {
    Service::new(temp.path().join("steward.db")).with_lock_root(temp.path().join("locks"))
}

fn version(outcome: &steward_application::Outcome) -> i64 {
    outcome.data["task"]["version"].as_i64().unwrap()
}

#[test]
fn task_session_checkpoint_import_and_history_flow() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    let created = service
        .task_create(
            "TASK-1",
            r#"{
                "title":"Implement V0",
                "goal":"Provide a local task continuity CLI",
                "scope":"V0 workspace",
                "acceptanceCriteria":"Integration flow passes",
                "nextStep":"Claim the task"
            }"#,
        )
        .unwrap();
    assert_eq!(version(&created), 1);

    let claimed = service.task_claim("TASK-1", 1, "session-a", false).unwrap();
    assert_eq!(claimed.data["task"]["status"], "in_progress");
    assert_eq!(version(&claimed), 2);
    let no_op = service.task_claim("TASK-1", 2, "session-a", false).unwrap();
    assert_eq!(version(&no_op), 2);
    let stale = service
        .task_update("TASK-1", 1, r#"{"nextStep":"stale"}"#)
        .unwrap_err();
    assert_eq!(stale.body.code, "VERSION_CONFLICT");

    let updated = service
        .task_update("TASK-1", 2, r#"{"nextStep":"Write tests"}"#)
        .unwrap();
    assert_eq!(version(&updated), 3);
    let noted = service
        .task_note("TASK-1", 3, "decision", "Use SQLite WAL")
        .unwrap();
    assert_eq!(version(&noted), 4);
    let blocked = service
        .task_block("TASK-1", 4, "Need fixture", "Create a synthetic fixture")
        .unwrap();
    assert_eq!(blocked.data["task"]["status"], "blocked");
    let unblocked = service
        .task_unblock("TASK-1", 5, "Save checkpoint")
        .unwrap();
    assert_eq!(unblocked.data["task"]["status"], "in_progress");
    let checkpoint = service
        .task_checkpoint(
            "TASK-1",
            6,
            "session-a",
            r#"{
                "summary":"Core flow works",
                "completed":["Task mutations"],
                "decisions":["SQLite WAL"],
                "pending":["Resume"],
                "nextStep":"Resume in session B",
                "risks":[]
            }"#,
        )
        .unwrap();
    assert_eq!(version(&checkpoint), 7);
    let resumed = service
        .task_resume("TASK-1", 7, "session-b", Some("session-a"), true)
        .unwrap();
    assert_eq!(version(&resumed), 8);
    assert_eq!(resumed.data["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(resumed.data["sessions"][1]["continuedFrom"], "session-a");

    let source = temp.path().join("session.json");
    fs::write(&source, br#"{"messages":["synthetic"]}"#).unwrap();
    let imported = service
        .session_import_add("TASK-1", "session-b", 8, &source)
        .unwrap();
    assert_eq!(version(&imported), 9);
    let import_id = imported.data["import"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        imported.warnings[0].code,
        "SENSITIVE_CONTENT_CHECK_REQUIRED"
    );
    let duplicate = service
        .session_import_add("TASK-1", "session-b", 9, &source)
        .unwrap();
    assert_eq!(version(&duplicate), 9);
    assert_eq!(duplicate.warnings[0].code, "DUPLICATE_SESSION_IMPORT");
    assert_eq!(
        service.session_import_list("session-b").unwrap().data["imports"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let removed = service.session_import_remove(&import_id, 9).unwrap();
    assert_eq!(version(&removed), 10);
    assert!(
        service.session_import_list("session-b").unwrap().data["imports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let closed = service.task_close("TASK-1", 10, "completed", None).unwrap();
    assert_eq!(closed.data["task"]["status"], "closed");
    assert!(closed.data["task"]["currentSessionId"].is_null());

    let history = service.history("TASK-1").unwrap();
    let changes = history.data["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["changeType"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(changes.first(), Some(&"task.created"));
    assert!(changes.contains(&"checkpoint.saved"));
    assert!(changes.contains(&"session.resumed"));
    assert!(changes.contains(&"session.imported"));
    assert!(changes.contains(&"session.import_removed"));
    assert_eq!(changes.last(), Some(&"task.closed"));
}

#[test]
fn worktree_create_status_dirty_refusal_and_remove_flow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "feature"]);

    let service = service(&temp);
    service
        .task_create(
            "TASK-WT",
            r#"{
                "title":"Worktree flow",
                "goal":"Exercise safe Git operations",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Create and remove worktree"
            }"#,
        )
        .unwrap();
    let worktree = temp.path().join("feature-worktree");
    let created = service
        .worktree_create("TASK-WT", 1, &repo, "feature", &worktree)
        .unwrap();
    assert_eq!(version(&created), 2);
    assert_eq!(created.data["worktreeStatus"]["exists"], true);
    let duplicate = service
        .worktree_create("TASK-WT", 2, &repo, "feature", &temp.path().join("other"))
        .unwrap_err();
    assert_eq!(duplicate.body.code, "WORKTREE_SAFETY_REFUSED");

    fs::write(worktree.join("untracked.txt"), "do not delete\n").unwrap();
    let dirty = service.worktree_remove("TASK-WT", 2).unwrap_err();
    assert_eq!(dirty.body.code, "WORKTREE_SAFETY_REFUSED");
    fs::remove_file(worktree.join("untracked.txt")).unwrap();
    let removed = service.worktree_remove("TASK-WT", 2).unwrap();
    assert_eq!(version(&removed), 3);
    assert_eq!(removed.data["worktreeStatus"]["registered"], false);
    assert!(!worktree.exists());
}

#[test]
fn worktree_adopt_doctor_and_detach_recover_external_state() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "recovery"]);

    let worktree = temp.path().join("recovery-worktree");
    git(
        &repo,
        ["worktree", "add", worktree.to_str().unwrap(), "recovery"],
    );
    let service = service(&temp);
    service
        .task_create(
            "TASK-RECOVER",
            r#"{
                "title":"Recovery flow",
                "goal":"Reconcile proven external Git state",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Adopt and detach succeed"
            }"#,
        )
        .unwrap();
    let adopted = service
        .worktree_adopt("TASK-RECOVER", 1, &repo, &worktree)
        .unwrap();
    assert_eq!(version(&adopted), 2);
    assert_eq!(adopted.data["worktreeStatus"]["exists"], true);

    git(&repo, ["worktree", "remove", worktree.to_str().unwrap()]);
    let stale_status = service.worktree_status("TASK-RECOVER").unwrap();
    assert_eq!(stale_status.data["worktreeStatus"]["registered"], true);
    assert_eq!(stale_status.data["worktreeStatus"]["exists"], false);
    let doctor = service.doctor().unwrap();
    let worktree_check = doctor.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["code"] == "WORKTREE_REFERENCES")
        .unwrap();
    assert_eq!(worktree_check["status"], "warning");
    assert!(
        worktree_check["details"]["issues"][0]["recommendedCommand"]
            .as_str()
            .unwrap()
            .contains("worktree detach TASK-RECOVER")
    );

    let detached = service
        .worktree_detach("TASK-RECOVER", 2, &worktree)
        .unwrap();
    assert_eq!(version(&detached), 3);
    assert_eq!(detached.data["worktreeStatus"]["registered"], false);
    let history = service.history("TASK-RECOVER").unwrap();
    let changes = history.data["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["changeType"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        changes,
        vec!["task.created", "worktree.adopted", "worktree.detached"]
    );
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(dead_code)]
fn _assert_json(value: &Value) {
    assert!(value.is_object());
}
