use serde_json::{Value, json};
use steward_application::{RuleInput, Service};

fn fixture() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::new(dir.path().join("synthetic-deletion.db"));
    s.task_create_minimal().unwrap();
    (dir, s)
}
fn version(s: &Service) -> i64 {
    s.task_show("1").unwrap().data["task"]["version"]
        .as_i64()
        .unwrap()
}
fn seed_work(s: &Service) -> Value {
    s.task_work_update("1", version(s), &json!({"reason":"synthetic request", "requirements":[
        {"id":"R1","title":"first","acceptanceCriteria":"first works"},
        {"id":"R2","title":"second","acceptanceCriteria":"second works"}
    ], "confirmations":[
        {"id":"Q1","requirementIds":["R1"],"question":"first only","recommendation":"A","impact":"first"},
        {"id":"Q2","requirementIds":["R1","R2"],"question":"shared","recommendation":"B","impact":"both","status":"resolved","answer":"B","evidence":["fixture reply"]},
        {"id":"Q3","requirementIds":["R2"],"question":"second only","recommendation":"C","impact":"second"}
    ]}).to_string()).unwrap().data
}
fn table_counts(s: &Service) -> Vec<i64> {
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    [
        "tasks",
        "task_notes",
        "checkpoints",
        "sessions",
        "session_imports",
        "session_events",
        "task_components",
    ]
    .iter()
    .map(|table| {
        c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}

#[test]
fn logical_deletion_preserves_all_records_and_only_audit_remains_task_readable() {
    let (dir, s) = fixture();
    s.task_create_minimal().unwrap();
    seed_work(&s);
    s.task_claim("1", version(&s), "execution", false).unwrap();
    s.session_bind("execution", version(&s), "fixture", "external")
        .unwrap();
    let event = json!({"schemaVersion":1,"sessionId":"execution","source":"fixture","externalSessionId":"external","eventId":"one","kind":"idle","occurredAt":"2026-11-11T00:00:00Z"}).to_string();
    s.hook_ingest(&event).unwrap();
    s.task_note("1", version(&s), "progress", "synthetic historical note")
        .unwrap();
    s.task_checkpoint("1", version(&s), "execution", &json!({"summary":"synthetic checkpoint","completed":[],"decisions":[],"pending":[],"nextStep":"fixture next","risks":[]}).to_string()).unwrap();
    let path = dir.path().join("synthetic.txt");
    std::fs::write(&path, "synthetic import only").unwrap();
    let imported = s
        .session_import_add("1", "execution", version(&s), &path, true)
        .unwrap()
        .data["import"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    s.session_close("execution", version(&s)).unwrap();
    let before = s.task_audit("1").unwrap().data;
    let counts = table_counts(&s);
    let deleted = s
        .task_delete("#1", version(&s), true, "explicit synthetic deletion")
        .unwrap()
        .data;
    assert_eq!(deleted["task"]["status"], before["task"]["status"]);
    assert_eq!(
        deleted["task"]["latestCheckpointId"],
        before["task"]["latestCheckpointId"]
    );
    assert_eq!(table_counts(&s), counts);
    assert!(path.exists());
    let fresh = Service::new(s.database_path());
    let audit = fresh.task_audit("1").unwrap().data;
    for key in ["work", "notes", "sessions", "checkpoints"] {
        assert_eq!(audit[key], before[key], "{key}");
    }
    let history = audit["history"].as_array().unwrap();
    assert_eq!(
        &history[..history.len() - 1],
        before["history"].as_array().unwrap()
    );
    assert_eq!(history.last().unwrap()["changeType"], "task.deleted");
    assert_eq!(fresh.task_list(None).unwrap().data["tasks"][0]["id"], 2);
    for status in [None, Some("active"), Some("todo")] {
        assert!(
            fresh.task_list(status).unwrap().data["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["id"] != 1)
        );
    }
    for result in [
        fresh.task_show("1"),
        fresh.task_context("1"),
        fresh.task_work("1"),
        fresh.task_notes("1"),
        fresh.history("1"),
        fresh.session_list(Some("1")),
        fresh.task_delete_preview("1"),
    ] {
        assert_eq!(result.unwrap_err().body.code, "TASK_DELETED");
    }
    let v = deleted["task"]["version"].as_i64().unwrap();
    for result in [
        fresh.task_status("1", v, "done"),
        fresh.task_note("1", v, "progress", "must not write"),
        fresh.task_claim("1", v, "new", false),
        fresh.task_delete("1", v, true, "duplicate"),
        fresh.hook_ingest(&event),
        fresh.session_import_remove(&imported, v),
        fresh.hook_clear("execution", v),
        fresh.task_work_update(
            "1",
            v,
            &json!({"reason":"no revive","requirements":[{"id":"R1","result":"no"}]}).to_string(),
        ),
    ] {
        assert_eq!(result.unwrap_err().body.code, "TASK_DELETED");
    }
    assert_eq!(
        fresh.session_list(None).unwrap().data["sessions"],
        json!([])
    );
    assert!(fresh.session_show("execution").unwrap().data["deletion"].is_object());
    assert_eq!(
        fresh.hook_list("execution", 0, 20).unwrap().data["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fresh.session_import_list("execution").unwrap().data["imports"][0]["id"],
        imported
    );
    assert_eq!(fresh.task_audit("1").unwrap().data, audit);
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert!(
        c.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
}

#[test]
fn any_unfinished_session_blocks_even_when_not_current_and_status_is_terminal() {
    let (_dir, s) = fixture();
    s.task_claim("1", 1, "old", false).unwrap();
    s.task_claim("1", 2, "new", true).unwrap();
    s.session_close("new", 3).unwrap();
    s.task_status("1", 4, "done").unwrap();
    let before = s.task_audit("1").unwrap().data;
    let preview = s.task_delete_preview("1").unwrap().data;
    assert_eq!(preview["openSessionIds"], json!(["old"]));
    assert_eq!(preview["blocked"], true);
    assert_eq!(
        s.task_delete("1", 5, true, "blocked fixture")
            .unwrap_err()
            .body
            .code,
        "DELETE_BLOCKED"
    );
    assert_eq!(s.task_audit("1").unwrap().data, before);
    // A separate explicit Session close is possible, never done by deletion.
    s.session_close("old", 5).unwrap();
    s.task_delete("1", 6, true, "after explicit fixture close")
        .unwrap();
}

#[test]
fn soft_deletion_retains_shared_profile_rule_provenance_and_does_not_bump_them() {
    let (_dir, s) = fixture();
    s.project_create("Synthetic").unwrap();
    s.task_set_project("1", 1, Some("1"), true, "fixture project")
        .unwrap();
    s.project_profile_set("1", 1, serde_json::from_value(json!({"summary":"summary","architecture":"architecture","development":"development","sourceTaskId":1,"sourceTaskVersion":2,"evidence":"fixture"})).unwrap()).unwrap();
    let rule: RuleInput = serde_json::from_value(json!({"scope":"global","projectId":null,"status":"active","contentVersion":1,"content":{"name":"rule","body":"body","sources":[{"kind":"explicit","evidence":"fixture","taskId":1,"taskVersion":2}]}})).unwrap();
    s.rule_create(rule.clone(), "fixture").unwrap();
    let project_history = s.project_history("1", 0, 100).unwrap().data;
    let rule_history = s.rule_history(1).unwrap().data;
    let preview = s.task_delete_preview("1").unwrap().data;
    assert_eq!(
        preview["sourceReferences"],
        json!({"projectIds":[1],"ruleIds":[1]})
    );
    s.task_delete("1", 2, true, "keep provenance").unwrap();
    assert_eq!(
        s.project_history("1", 0, 100).unwrap().data,
        project_history
    );
    let after_rule = s.rule_history(1).unwrap().data;
    assert_eq!(after_rule["history"], rule_history["history"]);
    assert_eq!(
        after_rule["rule"]["revision"],
        rule_history["rule"]["revision"]
    );
    assert_eq!(
        after_rule["rule"]["content"],
        rule_history["rule"]["content"]
    );
    let profile = s.project_profile_show("1").unwrap().data["profile"].clone();
    assert_eq!(profile["sourceTaskId"], 1);
    assert_eq!(profile["sourceTaskVersion"], 2);
    assert!(profile["sourceTaskDeletion"].is_object());
    assert_eq!(
        s.rule_show(1).unwrap().data["rule"]["deletedSourceTaskIds"],
        json!([1])
    );
    s.rule_disable(
        1,
        1,
        "explicit fixture disable still works with retained source",
    )
    .unwrap();
    s.rule_update(1, 2, rule, "explicit fixture reenable with retained source")
        .unwrap();
}

#[test]
fn requirements_remove_only_target_and_fix_shared_and_exclusive_confirmations() {
    let (_dir, s) = fixture();
    let original = seed_work(&s);
    s.task_claim("1", 2, "active", false).unwrap();
    let history = s.history("1").unwrap().data;
    let preview = s.task_requirement_delete_preview("1", "R1").unwrap().data;
    assert_eq!(preview["removedConfirmations"][0]["id"], "Q1");
    assert_eq!(
        preview["updatedConfirmations"][0]["requirementIds"],
        json!(["R2"])
    );
    let after = s
        .task_requirement_delete("1", 3, "R1", true, "synthetic scoped removal")
        .unwrap()
        .data;
    assert_eq!(
        after["work"]["requirements"],
        json!([original["work"]["requirements"][1]])
    );
    assert_eq!(
        after["work"]["confirmations"][1],
        original["work"]["confirmations"][2]
    );
    assert_eq!(after["work"]["confirmations"][0]["id"], "Q2");
    assert_eq!(
        after["work"]["confirmations"][0]["requirementIds"],
        json!(["R2"])
    );
    assert_eq!(after["work"]["confirmations"][0]["answer"], "B");
    assert_eq!(after["task"]["status"], "todo");
    assert_eq!(after["task"]["currentSessionId"], "active");
    let all = s.history("1").unwrap().data;
    assert_eq!(
        &all["history"].as_array().unwrap()[..history["history"].as_array().unwrap().len()],
        history["history"].as_array().unwrap()
    );
    for input in [
        json!({"reason":"old id","requirements":[{"id":"R1","title":"new meaning","acceptanceCriteria":"no"}]}),
        json!({"reason":"old confirmation","confirmations":[{"id":"Q1","question":"new","requirementIds":["R2"],"recommendation":"no","impact":"no"}]}),
    ] {
        assert_eq!(
            s.task_work_update("1", 4, &input.to_string())
                .unwrap_err()
                .body
                .code,
            "WORK_ITEM_DELETED"
        );
    }
    assert_eq!(s.task_work("1").unwrap().data["work"], after["work"]);
    assert_eq!(
        s.task_requirement_delete("1", 4, "R1", true, "repeat")
            .unwrap_err()
            .body
            .code,
        "NOT_FOUND"
    );
    let empty = s
        .task_requirement_delete("1", 4, "R2", true, "last requirement")
        .unwrap()
        .data;
    assert_eq!(
        empty["work"],
        json!({"formatVersion":1,"requirements":[],"confirmations":[]})
    );
    assert_eq!(s.task_context("1").unwrap().data["work"], empty["work"]);
}

#[test]
fn validation_cas_missing_target_and_history_failure_have_no_partial_effects() {
    let (_dir, s) = fixture();
    seed_work(&s);
    let baseline = s.task_audit("1").unwrap().data;
    for result in [
        s.task_delete("1", 2, false, "no confirmation"),
        s.task_delete("1", 2, true, " "),
        s.task_delete("1", 1, true, "stale"),
        s.task_requirement_delete("1", 1, "R1", true, "stale"),
        s.task_requirement_delete("1", 2, "R9", true, "wrong target"),
        s.task_requirement_delete("1", 2, "R1", false, "no confirmation"),
    ] {
        assert!(result.is_err());
    }
    assert_eq!(s.task_audit("1").unwrap().data, baseline);
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    c.execute_batch("CREATE TRIGGER fail_deletion BEFORE INSERT ON history WHEN NEW.change_type IN ('task.deleted','task.work_updated') BEGIN SELECT RAISE(ABORT,'synthetic history failure'); END;").unwrap();
    assert!(s.task_delete("1", 2, true, "rollback").is_err());
    assert!(
        s.task_requirement_delete("1", 2, "R1", true, "rollback")
            .is_err()
    );
    assert_eq!(s.task_audit("1").unwrap().data, baseline);
}

#[test]
fn every_business_state_can_be_deleted_without_changing_status_and_ids_are_not_reused() {
    for status in [
        "backlog",
        "todo",
        "in_progress",
        "in_review",
        "blocked",
        "done",
        "cancelled",
    ] {
        let (_dir, s) = fixture();
        s.task_status("1", 1, status).unwrap();
        s.task_delete("1", version(&s), true, "explicit fixture deletion")
            .unwrap();
        assert_eq!(s.task_audit("1").unwrap().data["task"]["status"], status);
        assert_eq!(s.task_create_minimal().unwrap().data["task"]["id"], 2);
    }
}

#[test]
fn concurrent_deletions_have_one_winner_and_one_history_entry() {
    let (_dir, s) = fixture();
    seed_work(&s);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let s = s.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                s.task_requirement_delete("1", 2, "R1", true, "concurrent fixture")
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.history("1").unwrap().data["history"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn preview_and_audit_never_create_a_missing_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.db");
    let s = Service::new(&path);
    assert!(s.task_delete_preview("1").is_err());
    assert!(s.task_audit("1").is_err());
    assert!(s.task_requirement_delete_preview("1", "R1").is_err());
    assert!(!path.exists());
}
