use serde_json::{Value, json};
use steward_application::Service;

fn task(s: &Service) -> Value {
    s.task_show("1").unwrap().data["task"].clone()
}
fn update(s: &Service, patch: Value) -> steward_application::Outcome {
    s.task_update(
        "1",
        task(s)["version"].as_i64().unwrap(),
        &patch.to_string(),
        true,
        "user authorized fixture correction",
    )
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, Service) {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("maintenance.db"));
    s.project_create("First").unwrap();
    s.project_create("Second").unwrap();
    s.project_component_add("1", 1, "api").unwrap();
    s.project_component_add("2", 1, "web").unwrap();
    s.task_create("FIXTURE", &json!({"title":"0909｜功能｜Fixture", "goal":"goal", "scope":"scope", "acceptanceCriteria":"criteria", "project":"First"}).to_string()).unwrap();
    (temp, s)
}

#[test]
fn every_status_and_closure_outcome_preserves_execution_and_old_history() {
    for state in [
        "open",
        "in_progress",
        "blocked",
        "completed",
        "partial",
        "cancelled",
        "superseded",
    ] {
        let (_temp, s) = fixture();
        if state != "open" {
            s.task_claim("1", 1, "execution", false).unwrap();
        }
        if state == "blocked" {
            s.task_block("1", 2, "dependency", "wait").unwrap();
        } else if !["open", "in_progress"].contains(&state) {
            s.task_close("1", 2, state, Some("original closure"))
                .unwrap();
        }
        let before = task(&s);
        let sessions = s.session_list(Some("1")).unwrap().data;
        let history = s.history("1").unwrap().data["history"]
            .as_array()
            .unwrap()
            .clone();
        let after = update(
            &s,
            json!({"goal":"corrected", "project":"Second", "components":["web"]}),
        )
        .data["task"]
            .clone();
        assert_eq!(after["version"], before["version"].as_i64().unwrap() + 1);
        assert_eq!(after["projectId"], 2);
        assert_eq!(after["componentIds"], json!([2]));
        for key in [
            "status",
            "currentSessionId",
            "closedAt",
            "closureOutcome",
            "closureReason",
            "blockReason",
            "blockRecovery",
            "latestCheckpointId",
            "createdAt",
            "repositoryPath",
            "worktreePath",
        ] {
            assert_eq!(after[key], before[key], "{state}: {key}");
        }
        assert_eq!(s.session_list(Some("1")).unwrap().data, sessions);
        let new_history = s.history("1").unwrap().data["history"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(&new_history[..history.len()], &history);
        assert_eq!(new_history.len(), history.len() + 1);
        let event = new_history.last().unwrap();
        assert!(event["sessionId"].is_null());
        assert_eq!(event["payload"]["before"], before);
        assert_eq!(event["payload"]["after"], after);
        assert_eq!(event["payload"]["confirmed"], true);
        assert_eq!(
            event["payload"]["reason"],
            "user authorized fixture correction"
        );
        assert_eq!(s.project_show("2").unwrap().data["project"]["revision"], 2);
    }
}

#[test]
fn authorization_validation_cas_and_history_failure_are_atomic() {
    let (temp, s) = fixture();
    update(&s, json!({"components":["api"]}));
    let before = task(&s);
    let history = s.history("1").unwrap().data;
    for (version, confirmed, reason, patch) in [
        (2, false, "authorized?", json!({"goal":"bad"})),
        (2, true, "  ", json!({"goal":"bad"})),
        (1, true, "stale", json!({"goal":"bad"})),
        (2, true, "wrong project", json!({"project":"missing"})),
        (2, true, "implicit discard", json!({"project":"Second"})),
        (2, true, "implicit clear", json!({"project":null})),
        (
            2,
            true,
            "wrong component",
            json!({"goal":"bad", "project":"Second", "components":["api"]}),
        ),
        (2, true, "duplicate", json!({"components":["api","API"]})),
        (2, true, "wrong type", json!({"components":null})),
        (2, true, "status forbidden", json!({"status":"closed"})),
        (
            2,
            true,
            "session forbidden",
            json!({"currentSessionId":null}),
        ),
        (
            2,
            true,
            "closure forbidden",
            json!({"closureOutcome":"completed"}),
        ),
        (2, true, "description gate", json!({"goal":null})),
    ] {
        assert!(
            s.task_update("1", version, &patch.to_string(), confirmed, reason)
                .is_err(),
            "{reason}"
        );
        assert_eq!(task(&s), before);
        assert_eq!(s.history("1").unwrap().data, history);
    }
    let conn = storage_sqlite::open_database(&temp.path().join("maintenance.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_audit BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    assert!(
        s.task_update(
            "1",
            2,
            &json!({"goal":"bad", "project":"Second", "components":["web"]}).to_string(),
            true,
            "rollback"
        )
        .is_err()
    );
    assert_eq!(task(&s), before);
    assert_eq!(s.history("1").unwrap().data, history);
    conn.execute_batch("DROP TRIGGER fail_audit;").unwrap();
    let cleared = update(&s, json!({"project":null, "components":[]}));
    assert!(cleared.data["task"]["projectId"].is_null());
    assert_eq!(cleared.data["task"]["componentIds"], json!([]));
}

#[test]
fn wrappers_share_closed_authorization_and_cas_contract() {
    let (_temp, s) = fixture();
    s.task_close("1", 1, "cancelled", Some("fixture")).unwrap();
    assert!(
        s.task_set_project("1", 2, Some("Second"), false, "fixture")
            .is_err()
    );
    assert!(
        s.task_set_components("1", 2, &["api".into()], false, "fixture")
            .is_err()
    );
    s.task_set_project("1", 2, Some("Second"), true, "fixture")
        .unwrap();
    s.task_set_components("1", 3, &["web".into()], true, "fixture")
        .unwrap();
    assert!(
        s.task_set_project("1", 4, Some("First"), true, "fixture")
            .is_err()
    );
    assert!(
        s.task_set_components("1", 3, &["web".into()], true, "stale noop")
            .is_err()
    );
    assert!(
        s.task_update(
            "1",
            4,
            &json!({"nextStep":"not allowed"}).to_string(),
            true,
            "fixture"
        )
        .is_err()
    );
    assert_eq!(task(&s)["status"], "closed");
    assert_eq!(
        s.session_list(Some("1")).unwrap().data["sessions"],
        json!([])
    );
}
