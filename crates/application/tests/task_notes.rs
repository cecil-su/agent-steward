use serde_json::{Value, json};
use steward_application::Service;

fn snapshot(s: &Service) -> Value {
    json!({"task":s.task_show("1").unwrap().data,"notes":s.task_notes("1").unwrap().data,
        "history":s.history("1").unwrap().data,"sessions":s.session_list(Some("1")).unwrap().data})
}

#[test]
fn notes_on_terminal_tasks_without_a_current_session_preserve_legacy_outcomes() {
    for (outcome, status) in [
        ("completed", "done"),
        ("partial", "cancelled"),
        ("cancelled", "cancelled"),
        ("superseded", "cancelled"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let s = Service::new(temp.path().join("legacy-notes.db"));
        s.task_create_minimal().unwrap();
        s.task_claim("1", 1, "historical", false).unwrap();
        s.session_close("historical", 2).unwrap();
        s.task_status("1", 3, status).unwrap();
        let c = rusqlite::Connection::open(s.database_path()).unwrap();
        c.execute("UPDATE tasks SET closure_outcome=?1,closure_reason='historical decision',closed_at='historical time' WHERE id=1", [outcome]).unwrap();
        let before = snapshot(&s);
        let result = s
            .task_note("1", 4, "progress", "**Follow-up**")
            .unwrap()
            .data;
        let mut expected = before["task"]["task"].clone();
        expected["version"] = json!(5);
        expected["updatedAt"] = result["task"]["updatedAt"].clone();
        assert_eq!(result["task"], expected);
        assert!(result["note"]["sessionId"].is_null());
        assert_eq!(snapshot(&s)["sessions"], before["sessions"]);
        let history = s.history("1").unwrap().data;
        let old_history = before["history"]["history"].as_array().unwrap();
        assert_eq!(
            &history["history"].as_array().unwrap()[..old_history.len()],
            old_history
        );
        assert_eq!(
            s.task_notes("1").unwrap().data["notes"][0]["text"],
            "**Follow-up**"
        );
    }
}

#[test]
fn notes_preserve_lifecycle_in_every_status() {
    for state in [
        "backlog",
        "todo",
        "in_progress",
        "in_review",
        "blocked",
        "done",
        "cancelled",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let s = Service::new(temp.path().join("notes.db"));
        s.task_create("NOTE", r#"{"title":"0916｜功能｜Note fixture","goal":"Goal","scope":"Scope","acceptanceCriteria":"Accept"}"#).unwrap();
        s.task_claim("1", 1, "fixture-session", false).unwrap();
        s.task_checkpoint("1", 2, "fixture-session", r#"{"summary":"Snapshot","completed":[],"decisions":[],"pending":[],"risks":[],"nextStep":"Check"}"#).unwrap();
        let changed = s.task_status("1", 3, state).unwrap();
        let mut version = changed.data["task"]["version"].as_i64().unwrap();
        for kind in ["progress", "decision", "risk"] {
            let before = snapshot(&s);
            let text = "## 阅读\n\n- **中文**\n\n```rust\nlet n = 1;\n```";
            let result = s.task_note("1", version, kind, text).unwrap().data;
            assert_eq!(result["note"]["text"], text);
            assert_eq!(result["note"]["noteType"], kind);
            assert_eq!(
                result["note"]["sessionId"],
                before["task"]["task"]["currentSessionId"]
            );
            let after = snapshot(&s);
            assert_eq!(after["sessions"], before["sessions"]);
            let mut expected = before["task"]["task"].clone();
            expected["version"] = json!(version + 1);
            expected["updatedAt"] = result["task"]["updatedAt"].clone();
            assert_eq!(result["task"], expected, "state {state}");
            let old_history = before["history"]["history"].as_array().unwrap();
            let history = after["history"]["history"].as_array().unwrap();
            assert_eq!(&history[..old_history.len()], old_history);
            assert_eq!(history.len(), old_history.len() + 1);
            assert_eq!(history.last().unwrap()["changeType"], "task.noted");
            assert_eq!(
                history.last().unwrap()["sessionId"],
                result["note"]["sessionId"]
            );
            assert_eq!(
                after["notes"]["notes"].as_array().unwrap().last().unwrap(),
                &result["note"]
            );
            assert_eq!(
                s.task_context("1").unwrap().data["notesSinceCheckpoint"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap(),
                &result["note"]
            );
            version += 1;
        }
        let before = snapshot(&s);
        for (v, kind, text, code) in [
            (version - 1, "progress", "stale", "VERSION_CONFLICT"),
            (version, "unknown", "body", "INVALID_INPUT"),
            (version, "progress", " \n ", "INVALID_INPUT"),
        ] {
            assert_eq!(s.task_note("1", v, kind, text).unwrap_err().body.code, code);
            assert_eq!(snapshot(&s), before);
        }
        assert_eq!(
            s.task_note("9999", 1, "progress", "missing")
                .unwrap_err()
                .body
                .code,
            "NOT_FOUND"
        );
        let connection = rusqlite::Connection::open(s.database_path()).unwrap();
        connection.execute_batch("CREATE TRIGGER reject_note_history BEFORE INSERT ON history WHEN NEW.change_type='task.noted' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        assert!(s.task_note("1", version, "progress", "rollback").is_err());
        assert_eq!(snapshot(&s), before);
    }
}
