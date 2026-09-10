use serde_json::{Value, json};
use steward_application::Service;

fn fixture() -> (tempfile::TempDir, Service) {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("release.db"));
    s.task_create("FIXTURE", &json!({"title":"0910｜功能｜Fixture","goal":"goal","scope":"scope","acceptanceCriteria":"criteria"}).to_string()).unwrap();
    (temp, s)
}
fn snapshot(s: &Service) -> Value {
    json!([
        s.task_show("1").unwrap().data,
        s.session_list(Some("1")).unwrap().data,
        s.history("1").unwrap().data
    ])
}

#[test]
fn transitions_preserve_session_and_only_close_on_explicit_command() {
    let (_temp, s) = fixture();
    let before = snapshot(&s);
    assert!(s.task_pending_release("1", 1).is_err());
    assert!(s.task_continue("1", 1).is_err());
    assert_eq!(snapshot(&s), before);
    s.task_claim("1", 1, "execution", false).unwrap();
    let sessions = s.session_list(Some("1")).unwrap().data;
    let out = s.task_pending_release("1", 2).unwrap().data;
    assert_eq!(out["task"]["status"], "pending_release");
    assert_eq!(out["task"]["version"], 3);
    assert!(out["task"]["closureOutcome"].is_null());
    assert_eq!(s.session_list(Some("1")).unwrap().data, sessions);
    let before = snapshot(&s);
    assert!(s.task_continue("1", 2).is_err());
    assert!(s.task_pending_release("1", 3).is_err());
    assert!(s.task_block("1", 3, "reason", "recovery").is_err());
    assert!(
        s.task_update("1", 3, r#"{"status":"in_progress"}"#, true, "fixture")
            .is_err()
    );
    assert_eq!(snapshot(&s), before);
    assert_eq!(s.task_claim("1", 3, "execution", false).unwrap().data, out);
    assert!(s.task_claim("1", 3, "other", false).is_err());
    s.task_continue("1", 3).unwrap();
    assert_eq!(s.session_list(Some("1")).unwrap().data, sessions);
    s.task_pending_release("1", 4).unwrap();
    s.task_close("1", 5, "completed", None).unwrap();
    let closed = s.task_show("1").unwrap().data;
    assert_eq!(closed["task"]["status"], "closed");
    assert!(closed["task"]["currentSessionId"].is_null());
    assert!(s.session_show("execution").unwrap().data["session"]["endedAt"].is_string());
    assert!(s.task_continue("1", 6).is_err());
    assert!(s.task_pending_release("1", 6).is_err());
    let events = s.history("1").unwrap().data;
    assert_eq!(events["history"][2]["changeType"], "task.pending_release");
    assert_eq!(
        events["history"][2]["payload"],
        json!({"previousStatus":"in_progress","status":"pending_release"})
    );
}

#[test]
fn pending_release_supports_checkpoint_resume_notes_maintenance_and_active_filter() {
    let (_temp, s) = fixture();
    s.task_claim("1", 1, "execution", false).unwrap();
    s.task_pending_release("1", 2).unwrap();
    for (filter, count) in [
        ("pending_release", 1),
        ("active", 1),
        ("in_progress", 0),
        ("closed", 0),
    ] {
        assert_eq!(
            s.task_list(Some(filter)).unwrap().data["tasks"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
    }
    assert_eq!(
        s.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    s.task_checkpoint("1", 3, "execution", &json!({"summary":"ready","completed":[],"decisions":[],"pending":[],"risks":[],"nextStep":"await release"}).to_string()).unwrap();
    s.task_note("1", 4, "progress", "await release").unwrap();
    s.task_update(
        "1",
        5,
        r#"{"goal":"corrected"}"#,
        true,
        "authorized fixture",
    )
    .unwrap();
    s.session_close("execution", 6).unwrap();
    assert!(s.task_claim("1", 7, "new", false).is_err());
    s.task_resume("1", 7, "new", Some("execution"), false)
        .unwrap();
    assert_eq!(
        s.task_show("1").unwrap().data["task"]["status"],
        "pending_release"
    );
    s.task_claim("1", 8, "takeover", true).unwrap();
    assert_eq!(
        s.task_show("1").unwrap().data["task"]["status"],
        "pending_release"
    );
}

#[test]
fn history_failure_rolls_back_both_transitions_and_close() {
    for operation in ["enter", "continue", "close"] {
        let (temp, s) = fixture();
        s.task_claim("1", 1, "execution", false).unwrap();
        let version = if operation == "enter" {
            2
        } else {
            s.task_pending_release("1", 2).unwrap();
            3
        };
        let before = snapshot(&s);
        let c = rusqlite::Connection::open(temp.path().join("release.db")).unwrap();
        c.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        let result = match operation {
            "enter" => s.task_pending_release("1", version),
            "continue" => s.task_continue("1", version),
            _ => s.task_close("1", version, "completed", None),
        };
        assert!(result.is_err());
        assert_eq!(snapshot(&s), before);
    }
}

#[test]
fn blocked_and_incomplete_tasks_keep_existing_close_boundaries() {
    let (_temp, s) = fixture();
    s.task_claim("1", 1, "execution", false).unwrap();
    s.task_block("1", 2, "reason", "recovery").unwrap();
    let before = snapshot(&s);
    assert!(s.task_pending_release("1", 3).is_err());
    assert!(s.task_continue("1", 3).is_err());
    assert_eq!(snapshot(&s), before);
    s.task_create_minimal().unwrap();
    s.task_claim("2", 1, "incomplete", false).unwrap();
    s.task_pending_release("2", 2).unwrap();
    assert!(s.task_close("2", 3, "completed", None).is_err());
    s.task_close("2", 3, "cancelled", Some("explicit fixture choice"))
        .unwrap();
}
