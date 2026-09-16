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
const CHECKPOINT: &str = r#"{"summary":"snapshot","completed":[],"decisions":[],"pending":[],"risks":[],"nextStep":"continue"}"#;

fn version(s: &Service) -> i64 {
    s.task_show("1").unwrap().data["task"]["version"]
        .as_i64()
        .unwrap()
}
fn snapshot(s: &Service) -> Value {
    json!({"task": s.task_show("1").unwrap().data,
        "sessions": s.session_list(Some("1")).unwrap().data,
        "history": s.history("1").unwrap().data})
}
fn unchanged_status(s: &Service, status: &str) {
    assert_eq!(s.task_show("1").unwrap().data["task"]["status"], status);
}

#[test]
fn explicit_session_operations_work_in_every_business_status() {
    for status in STATUSES {
        let temp = tempfile::tempdir().unwrap();
        let s = Service::new(temp.path().join("sessions.db"));
        s.task_create_minimal().unwrap();
        s.task_status("1", 1, status).unwrap();
        s.task_claim("1", version(&s), "first", false).unwrap();
        unchanged_status(&s, status);
        let claimed = snapshot(&s);
        s.task_claim("1", version(&s), "first", false).unwrap();
        assert_eq!(snapshot(&s), claimed);
        s.session_bind("first", version(&s), "pi", "external-first")
            .unwrap();
        unchanged_status(&s, status);
        let cp = s
            .task_checkpoint("1", version(&s), "first", CHECKPOINT)
            .unwrap();
        assert!(cp.data["checkpoint"]["gitHead"].is_null());
        unchanged_status(&s, status);
        let record = temp.path().join("synthetic-session.txt");
        std::fs::write(&record, "synthetic reviewed session record").unwrap();
        let imported = s
            .session_import_add("1", "first", version(&s), &record, true)
            .unwrap();
        unchanged_status(&s, status);
        s.session_import_remove(imported.data["import"]["id"].as_str().unwrap(), version(&s))
            .unwrap();
        unchanged_status(&s, status);
        s.session_attach(
            "1",
            version(&s),
            "attached",
            Some("pi"),
            Some("external-attached"),
            None,
        )
        .unwrap();
        unchanged_status(&s, status);
        assert_eq!(
            s.task_show("1").unwrap().data["task"]["currentSessionId"],
            "first"
        );
        s.session_close("attached", version(&s)).unwrap();
        unchanged_status(&s, status);
        assert_eq!(
            s.task_show("1").unwrap().data["task"]["currentSessionId"],
            "first"
        );
        s.session_close("first", version(&s)).unwrap();
        unchanged_status(&s, status);
        assert!(s.task_show("1").unwrap().data["task"]["currentSessionId"].is_null());
        let ended = s.session_show("first").unwrap().data;
        let resumed = s
            .task_resume("1", version(&s), "resumed", Some("first"), false)
            .unwrap();
        assert!(resumed.data.get("worktreeStatus").is_none());
        assert_eq!(
            s.session_show("resumed").unwrap().data["session"]["continuedFrom"],
            "first"
        );
        assert_eq!(s.session_show("first").unwrap().data, ended);
        unchanged_status(&s, status);
        s.task_claim("1", version(&s), "takeover", true).unwrap();
        unchanged_status(&s, status);
        assert!(s.session_show("resumed").unwrap().data["session"]["endedAt"].is_null());
        s.task_resume("1", version(&s), "next", Some("takeover"), true)
            .unwrap();
        unchanged_status(&s, status);
        assert!(s.session_show("takeover").unwrap().data["session"]["endedAt"].is_null());
        let before_read = snapshot(&s);
        assert!(
            s.task_context("1")
                .unwrap()
                .data
                .get("worktreeStatus")
                .is_none()
        );
        assert_eq!(snapshot(&s), before_read);
    }
}

#[test]
fn session_identity_ownership_takeover_and_cas_guards_remain() {
    for status in STATUSES {
        let temp = tempfile::tempdir().unwrap();
        let s = Service::new(temp.path().join("guards.db"));
        s.task_create_minimal().unwrap();
        s.task_create_minimal().unwrap();
        s.task_claim("2", 1, "foreign", false).unwrap();
        s.task_status("1", 1, status).unwrap();
        let before = snapshot(&s);
        let v = version(&s);
        assert!(s.task_resume("1", v, "new", None, false).is_err());
        assert!(
            s.task_resume("1", v, "new", Some("foreign"), false)
                .is_err()
        );
        assert!(s.task_claim("1", v, "foreign", true).is_err());
        assert!(s.task_checkpoint("1", v, "foreign", CHECKPOINT).is_err());
        assert_eq!(snapshot(&s), before);
        s.task_claim("1", v, "current", false).unwrap();
        let v = version(&s);
        let before = snapshot(&s);
        assert!(s.task_claim("1", v, "other", false).is_err());
        assert!(
            s.task_resume("1", v, "other", Some("current"), false)
                .is_err()
        );
        assert!(
            s.task_resume("1", v, "current", Some("current"), true)
                .is_err()
        );
        assert!(
            s.task_resume("1", v, "foreign", Some("current"), true)
                .is_err()
        );
        assert!(
            s.task_resume("1", v, "other", Some("foreign"), true)
                .is_err()
        );
        assert!(s.task_checkpoint("1", v, "foreign", CHECKPOINT).is_err());
        assert!(
            s.session_attach("1", v, "foreign", None, None, None)
                .is_err()
        );
        assert_eq!(
            s.task_claim("1", v - 1, "current", false)
                .unwrap_err()
                .body
                .code,
            "VERSION_CONFLICT"
        );
        assert_eq!(
            s.task_checkpoint("1", v - 1, "current", CHECKPOINT)
                .unwrap_err()
                .body
                .code,
            "VERSION_CONFLICT"
        );
        assert_eq!(
            s.task_resume("1", v - 1, "other", Some("current"), true)
                .unwrap_err()
                .body
                .code,
            "VERSION_CONFLICT"
        );
        assert_eq!(
            s.session_close("current", v - 1).unwrap_err().body.code,
            "VERSION_CONFLICT"
        );
        assert_eq!(snapshot(&s), before);
        s.session_close("current", v).unwrap();
        let v = version(&s);
        let ended = snapshot(&s);
        assert!(s.task_claim("1", v, "current", true).is_err());
        assert!(s.task_checkpoint("1", v, "current", CHECKPOINT).is_err());
        assert!(s.session_bind("current", v, "pi", "external").is_err());
        // Reattaching identical metadata is a no-op, not a reactivation.
        s.session_attach("1", v, "current", None, None, None)
            .unwrap();
        s.session_close("current", v).unwrap();
        assert_eq!(snapshot(&s), ended);
        // Starting a fresh independent Session does not require resuming history.
        s.task_claim("1", v, "fresh", false).unwrap();
        assert!(s.session_show("fresh").unwrap().data["session"]["continuedFrom"].is_null());
        unchanged_status(&s, status);
    }
}

#[test]
fn history_failure_rolls_back_session_and_checkpoint_writes() {
    for operation in ["claim", "resume", "checkpoint", "attach", "bind", "close"] {
        let temp = tempfile::tempdir().unwrap();
        let s = Service::new(temp.path().join("rollback.db"));
        s.task_create_minimal().unwrap();
        s.task_status("1", 1, "done").unwrap();
        s.task_claim("1", 2, "current", false).unwrap();
        let before = snapshot(&s);
        let c = rusqlite::Connection::open(s.database_path()).unwrap();
        c.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        let result = match operation {
            "claim" => s.task_claim("1", 3, "new", true),
            "resume" => s.task_resume("1", 3, "new", Some("current"), true),
            "checkpoint" => s.task_checkpoint("1", 3, "current", CHECKPOINT),
            "attach" => s.session_attach("1", 3, "attached", None, None, None),
            "bind" => s.session_bind("current", 3, "pi", "external"),
            _ => s.session_close("current", 3),
        };
        assert!(result.is_err(), "{operation}");
        assert_eq!(snapshot(&s), before, "{operation}");
        let checkpoints: i64 = c
            .query_row("SELECT COUNT(*) FROM checkpoints", [], |r| r.get(0))
            .unwrap();
        assert_eq!(checkpoints, 0);
    }
}

#[test]
fn doctor_retains_database_and_record_diagnostics_without_git_checks() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("doctor.db"));
    s.task_create_minimal().unwrap();
    let missing = temp.path().join("missing-session-record.json");
    s.session_attach("1", 1, "record", None, None, Some(&missing))
        .unwrap();
    let before = snapshot(&s);
    let checks = s.doctor().unwrap().data["checks"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        checks
            .iter()
            .map(|c| c["code"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "SCHEMA_VERSION",
            "SQLITE_QUICK_CHECK",
            "FOREIGN_KEY_CHECK",
            "RECORD_PATH_REFERENCES"
        ]
    );
    assert_eq!(checks[1]["status"], "ok");
    assert_eq!(checks[2]["status"], "ok");
    assert_eq!(checks[3]["status"], "warning");
    assert_eq!(checks[3]["details"]["missing"][0]["sessionId"], "record");
    assert_eq!(snapshot(&s), before);
}
