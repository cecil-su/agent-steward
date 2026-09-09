use serde_json::{Value, json};
use steward_application::Service;

fn snapshot(service: &Service) -> Value {
    json!({
        "task": service.task_show("1").unwrap().data,
        "history": service.history("1").unwrap().data,
        "sessions": service.session_list(Some("1")).unwrap().data,
    })
}

#[test]
fn close_outcomes_keep_transition_gates_and_atomicity() {
    for status in ["open", "in_progress", "blocked"] {
        for outcome in ["completed", "partial", "cancelled", "superseded"] {
            let temp = tempfile::tempdir().unwrap();
            let service = Service::new(temp.path().join("test.db"));
            service.task_create("CLOSE", r#"{"title":"0909｜修复｜Close regression","goal":"Goal","scope":"Scope","acceptanceCriteria":"Accept"}"#).unwrap();
            let mut version = 1;
            if status != "open" {
                service.task_claim("1", version, "session", false).unwrap();
                version += 1;
            }
            if status == "blocked" {
                service
                    .task_block("1", version, "pending acceptance", "user confirmation")
                    .unwrap();
                version += 1;
            }
            let before = snapshot(&service);
            let result = service.task_close("1", version, outcome, Some("user decision"));
            let valid = match outcome {
                "completed" => status == "in_progress",
                "partial" => status != "open",
                _ => true,
            };
            if valid {
                let task = result.unwrap().data["task"].clone();
                assert_eq!(task["status"], "closed");
                assert_eq!(task["version"], version + 1);
                assert_eq!(task["closureOutcome"], outcome);
                for field in [
                    "currentSessionId",
                    "nextStep",
                    "blockReason",
                    "blockRecovery",
                ] {
                    assert!(task[field].is_null(), "{field}");
                }
                if status != "open" {
                    assert!(
                        !service.session_list(Some("1")).unwrap().data["sessions"][0]["endedAt"]
                            .is_null()
                    );
                }
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.body.code, "CONSTRAINT_VIOLATION");
                assert_eq!(
                    error.body.details["constraint"],
                    "task.close.invalid_transition"
                );
                assert_eq!(error.body.details["currentStatus"], status);
                assert_eq!(error.body.details["outcome"], outcome);
                assert!(!error.body.retryable);
                assert_eq!(error.exit_code, 4);
                if status == "blocked" {
                    assert!(error.body.message.contains("explicitly unblock"));
                    assert!(!error.body.message.contains("database constraint"));
                }
                assert_eq!(snapshot(&service), before);
            }
        }
    }
}

#[test]
fn close_history_failure_rolls_back_task_and_session_updates() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("test.db"));
    service.task_create("ROLLBACK", r#"{"title":"0909｜修复｜Close rollback","goal":"Goal","scope":"Scope","acceptanceCriteria":"Accept"}"#).unwrap();
    service.task_claim("1", 1, "session", false).unwrap();
    let before = snapshot(&service);
    // Fault injection in an isolated test database; no business rows are edited.
    let connection = rusqlite::Connection::open(service.database_path()).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_close_history BEFORE INSERT ON history WHEN NEW.change_type = 'task.closed' BEGIN SELECT RAISE(ABORT, 'close history fixture'); END;").unwrap();
    let error = service.task_close("1", 2, "completed", None).unwrap_err();
    assert_eq!(error.body.code, "CONSTRAINT_VIOLATION");
    assert_eq!(snapshot(&service), before);
}

#[test]
fn migrated_shape_without_current_session_keeps_checkpoint_and_closes_after_explicit_unblock() {
    // Reproduce the migrated tasks' shape through public APIs, not SQL mutations.
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("test.db"));
    service.task_create("IMPORTED-SHAPE", r#"{"title":"0909｜修复｜Imported regression","goal":"Goal","scope":"Scope","acceptanceCriteria":"Accept"}"#).unwrap();
    service
        .task_claim("1", 1, "historical-session", false)
        .unwrap();
    service.task_checkpoint("1", 2, "historical-session", r#"{"summary":"Historical checkpoint","completed":[],"decisions":[],"pending":[],"nextStep":"Await acceptance","risks":[]}"#).unwrap();
    service
        .task_block("1", 3, "historical pending acceptance", "user confirmation")
        .unwrap();
    service.session_close("historical-session", 4).unwrap();
    let before = snapshot(&service);
    assert!(before["task"]["task"]["currentSessionId"].is_null());
    let error = service.task_close("1", 5, "completed", None).unwrap_err();
    assert_eq!(
        error.body.details["constraint"],
        "task.close.invalid_transition"
    );
    assert_eq!(snapshot(&service), before);
    service
        .task_unblock(
            "1",
            5,
            "User confirmed acceptance; record completed closure",
        )
        .unwrap();
    let unblocked = snapshot(&service);
    let stale = service.task_close("1", 5, "completed", None).unwrap_err();
    assert_eq!(stale.body.code, "VERSION_CONFLICT");
    assert_eq!(snapshot(&service), unblocked);
    let result = service.task_close("1", 6, "completed", None).unwrap();
    assert_eq!(result.data["task"]["version"], 7);
    assert_eq!(
        result.data["task"]["latestCheckpointId"],
        before["task"]["task"]["latestCheckpointId"]
    );
    assert_eq!(
        service.session_list(Some("1")).unwrap().data,
        before["sessions"]
    );
}
