use serde_json::{Value, json};
use steward_application::Service;

const STATUSES: [&str; 7] = [
    "backlog",
    "todo",
    "in_progress",
    "in_review",
    "blocked",
    "done",
    "cancelled",
];

fn snapshot(service: &Service) -> Value {
    json!({
        "task": service.task_show("1").unwrap().data["task"],
        "history": service.history("1").unwrap().data["history"],
        "sessions": service.session_list(Some("1")).unwrap().data,
        "notes": service.task_notes("1").unwrap().data,
    })
}

#[test]
fn all_49_status_pairs_work_without_descriptions_or_sessions() {
    for from in STATUSES {
        for to in STATUSES {
            let temp = tempfile::tempdir().unwrap();
            let service = Service::new(temp.path().join("status.db"));
            let created = service.task_create_minimal().unwrap();
            assert_eq!(created.data["task"]["status"], "todo");
            assert!(created.data["task"]["title"].is_null());
            service.task_status("1", 1, from).unwrap();
            let before = snapshot(&service);
            let version = before["task"]["version"].as_i64().unwrap();
            let response = service.task_status("#1", version, to).unwrap();
            let after = snapshot(&service);
            assert_eq!(after["task"], response.data["task"]);
            assert_eq!(after["sessions"], before["sessions"]);
            assert_eq!(after["notes"], before["notes"]);
            if from == to {
                assert_eq!(after, before, "same status {from} must be no-op");
            } else {
                let mut expected = before["task"].clone();
                expected["status"] = json!(to);
                expected["version"] = json!(version + 1);
                expected["updatedAt"] = after["task"]["updatedAt"].clone();
                assert_eq!(after["task"], expected, "{from} -> {to}");
                let old = before["history"].as_array().unwrap();
                let history = after["history"].as_array().unwrap();
                assert_eq!(&history[..old.len()], old);
                assert_eq!(history.len(), old.len() + 1);
                let event = history.last().unwrap();
                assert_eq!(event["changeType"], "task.status_changed");
                assert_eq!(event["payload"], json!({"previousStatus":from,"status":to}));
                assert!(event["sessionId"].is_null());
            }
            assert_eq!(
                service
                    .task_status("1", version - 1, to)
                    .unwrap_err()
                    .body
                    .code,
                "VERSION_CONFLICT"
            );
            assert_eq!(snapshot(&service), after);
        }
    }
}

#[test]
fn status_preserves_sessions_checkpoints_notes_and_legacy_facts() {
    for outcome in ["completed", "partial", "cancelled", "superseded"] {
        let temp = tempfile::tempdir().unwrap();
        let service = Service::new(temp.path().join("history.db"));
        service.task_create_minimal().unwrap();
        service.task_claim("1", 1, "session", false).unwrap();
        service.task_checkpoint("1", 2, "session", r#"{"summary":"checkpoint","completed":[],"decisions":[],"pending":[],"nextStep":"retain next step","risks":[]}"#).unwrap();
        service
            .task_note("1", 3, "decision", "retained note")
            .unwrap();
        // Synthetic legacy facts only; migration itself is tested by the schema owner.
        let c = rusqlite::Connection::open(service.database_path()).unwrap();
        c.execute("UPDATE tasks SET closure_outcome=?1,closure_reason='legacy reason',closed_at='legacy time',block_reason='legacy blocker',block_recovery='legacy recovery' WHERE id=1", [outcome]).unwrap();
        c.execute(
            "UPDATE checkpoints SET git_head='historical-head' WHERE task_id=1",
            [],
        )
        .unwrap();
        let checkpoint = service.task_context("1").unwrap().data["checkpoint"].clone();
        for to in STATUSES {
            let before = snapshot(&service);
            let version = before["task"]["version"].as_i64().unwrap();
            let response = service.task_status("1", version, to).unwrap();
            let after = snapshot(&service);
            let mut expected = before["task"].clone();
            expected["status"] = json!(to);
            expected["version"] = response.data["task"]["version"].clone();
            expected["updatedAt"] = response.data["task"]["updatedAt"].clone();
            assert_eq!(after["task"], expected);
            assert_eq!(after["sessions"], before["sessions"]);
            assert_eq!(after["notes"], before["notes"]);
            assert_eq!(
                service.task_context("1").unwrap().data["checkpoint"],
                checkpoint
            );
            assert_eq!(after["task"]["closureOutcome"], outcome);
            assert_eq!(after["task"]["currentSessionId"], "session");
            assert_eq!(
                after["history"].as_array().unwrap().last().unwrap()["sessionId"],
                "session"
            );
        }
    }
}

#[test]
fn invalid_inputs_and_audit_failure_leave_no_partial_status_write() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("rollback.db"));
    service.task_create_minimal().unwrap();
    service.task_claim("1", 1, "session", false).unwrap();
    let before = snapshot(&service);
    for status in ["open", "closed", "pending_release", "unknown", "", " done "] {
        assert_eq!(
            service.task_status("1", 2, status).unwrap_err().body.code,
            "INVALID_INPUT"
        );
        assert_eq!(snapshot(&service), before);
    }
    assert_eq!(
        service.task_status("999", 1, "done").unwrap_err().body.code,
        "NOT_FOUND"
    );
    let c = rusqlite::Connection::open(service.database_path()).unwrap();
    c.execute_batch("CREATE TRIGGER reject_status_history BEFORE INSERT ON history WHEN NEW.change_type='task.status_changed' BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
    for status in ["blocked", "done", "cancelled"] {
        assert!(service.task_status("1", 2, status).is_err());
        assert_eq!(snapshot(&service), before);
    }
    // No-op does not write History, even when a history write would fail.
    service.task_status("1", 2, "todo").unwrap();
    assert_eq!(snapshot(&service), before);
}

#[test]
fn status_filters_include_all_nonterminal_states_in_active() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("list.db"));
    for status in STATUSES {
        let task = service.task_create_minimal().unwrap().data["task"].clone();
        service
            .task_status(&task["id"].to_string(), 1, status)
            .unwrap();
    }
    for status in STATUSES {
        let tasks = service.task_list(Some(status)).unwrap().data["tasks"].clone();
        assert_eq!(tasks.as_array().unwrap().len(), 1);
        assert_eq!(tasks[0]["status"], status);
    }
    let active = service.task_list(Some("active")).unwrap().data["tasks"].clone();
    assert_eq!(active.as_array().unwrap().len(), 5);
    assert!(
        active
            .as_array()
            .unwrap()
            .iter()
            .all(|task| task["status"] != "done" && task["status"] != "cancelled")
    );
    assert_eq!(
        service.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    for old in ["open", "closed", "pending_release"] {
        assert!(service.task_list(Some(old)).is_err());
    }
}

#[test]
fn concurrent_status_writers_keep_cas_and_single_history_event() {
    use std::sync::{Arc, Barrier};
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("concurrent.db"));
    service.task_create_minimal().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["done", "cancelled"].map(|status| {
        let s = service.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            s.task_status("1", 1, status)
        })
    });
    let results = handles.map(|h| h.join().unwrap());
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .filter(|e| e.body.code == "VERSION_CONFLICT")
            .count(),
        1
    );
    assert_eq!(service.task_show("1").unwrap().data["task"]["version"], 2);
    assert_eq!(
        service.history("1").unwrap().data["history"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
