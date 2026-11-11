use serde_json::{Value, json};
use steward_application::Service;

fn start() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::new(dir.path().join("work.db"));
    s.task_create_minimal().unwrap();
    (dir, s)
}
fn apply(s: &Service, version: i64, input: Value) -> Value {
    s.task_work_update("1", version, &input.to_string())
        .unwrap()
        .data
}
fn seed(s: &Service) -> Value {
    apply(
        s,
        1,
        json!({"reason":"Explicit synthetic multi-requirement request", "requirements":[
        {"id":"R1","title":"数量同步","acceptanceCriteria":"删除后数量正确","status":"done","result":"删除后统计同步","evidence":["fixture check passed"]},
        {"id":"R2","title":"图片预览","acceptanceCriteria":"预览清楚","status":"in_progress"}
    ], "confirmations":[{"id":"Q1","requirementIds":["R2"],"question":"选300px还是原图？","recommendation":"300px","impact":"原图可能增加流量"}]}),
    )
}
#[test]
fn persists_partial_updates_and_decisions_across_sessions_without_changing_task_status() {
    let (_dir, s) = start();
    assert!(s.task_work("1").unwrap().data["work"].is_null());
    let before = seed(&s);
    let old_r1 = before["work"]["requirements"][0].clone();
    let answer = apply(
        &s,
        2,
        json!({"reason":"User selected 300px", "requirements":[{"id":"R2","nextStep":"实现300px"}],"confirmations":[{"id":"Q1","status":"resolved","answer":"用户选择300px","evidence":["synthetic user reply"]}]}),
    );
    assert_eq!(answer["work"]["requirements"][0], old_r1);
    assert_eq!(answer["work"]["requirements"][1]["status"], "in_progress");
    assert_eq!(answer["task"]["status"], "todo");
    assert!(answer["task"]["currentSessionId"].is_null());
    let fresh = Service::new(s.database_path());
    let context = fresh.task_context("1").unwrap().data;
    assert_eq!(context["work"], answer["work"]);
    assert_eq!(context["work"]["confirmations"][0]["status"], "resolved");
    assert_eq!(fresh.task_notes("1").unwrap().data["notes"], json!([]));
    let done = apply(
        &fresh,
        3,
        json!({"reason":"Completed scoped change", "requirements":[{"id":"R2","status":"done","result":"300px已实现","nextStep":"","evidence":["fixture request verified"]}]}),
    );
    assert_eq!(done["task"]["status"], "todo");
    assert_eq!(done["work"]["requirements"][0], old_r1);
    let history = fresh.history("1").unwrap().data;
    assert_eq!(
        history["history"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|h| h["changeType"] == "task.work_updated")
            .count(),
        3
    );
    assert_eq!(history["history"][1]["payload"]["work"], before["work"]);
}
#[test]
fn cas_invalid_links_unknown_fields_and_failed_history_are_atomic() {
    let (_dir, s) = start();
    seed(&s);
    let baseline = s.task_work("1").unwrap().data;
    let history = s.history("1").unwrap().data;
    let cases = [
        (
            1,
            json!({"reason":"stale","requirements":[{"id":"R2","title":"stale overwrite"}]}),
            "VERSION_CONFLICT",
        ),
        (
            2,
            json!({"reason":"invalid reference","requirements":[{"id":"R2","result":"must roll back"}],"confirmations":[{"id":"Q1","requirementIds":["R999"]}]}),
            "INVALID_INPUT",
        ),
        (
            2,
            json!({"reason":"fake done","requirements":[{"id":"R2","status":"done"}]}),
            "INVALID_INPUT",
        ),
        (
            2,
            json!({"reason":"unknown","requirements":[{"id":"R2","taskStatus":"done"}]}),
            "INVALID_INPUT",
        ),
        (
            2,
            json!({"reason":"invalid decision","confirmations":[{"id":"Q1","status":"resolved"}]}),
            "INVALID_INPUT",
        ),
        (
            2,
            json!({"reason":"duplicate","requirements":[{"id":"R2"},{"id":"R2"}]}),
            "INVALID_INPUT",
        ),
        (
            2,
            json!({"reason":"invalid stage","requirements":[{"id":"R2","status":"in_review"}]}),
            "INVALID_INPUT",
        ),
    ];
    for (version, input, code) in cases {
        assert_eq!(
            s.task_work_update("1", version, &input.to_string())
                .unwrap_err()
                .body
                .code,
            code
        );
        assert_eq!(s.task_work("1").unwrap().data, baseline);
        assert_eq!(s.history("1").unwrap().data, history);
    }
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    c.execute_batch("CREATE TRIGGER reject_work BEFORE INSERT ON history WHEN NEW.change_type='task.work_updated' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(
        s.task_work_update(
            "1",
            2,
            &json!({"reason":"rollback","requirements":[{"id":"R2","title":"must not persist"}]})
                .to_string()
        )
        .is_err()
    );
    assert_eq!(s.task_work("1").unwrap().data, baseline);
    assert_eq!(s.history("1").unwrap().data, history);
}
#[test]
fn noop_checks_version_but_does_not_bump_and_corruption_is_not_empty_work() {
    let (_dir, s) = start();
    let original = seed(&s);
    assert_eq!(
        apply(
            &s,
            2,
            json!({"reason":"same fact","requirements":[{"id":"R2","status":"in_progress"}]})
        ),
        original
    );
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    c.execute("UPDATE history SET payload_json='{\"work\":{\"formatVersion\":99}}' WHERE change_type='task.work_updated'", []).unwrap();
    assert_eq!(
        s.task_work("1").unwrap_err().body.code,
        "DATABASE_UNAVAILABLE"
    );
    assert_eq!(
        s.task_context("1").unwrap_err().body.code,
        "DATABASE_UNAVAILABLE"
    );
}
#[test]
fn invalid_input_does_not_echo_untrusted_values() {
    let (_dir, s) = start();
    let marker = "private-value-must-not-be-echoed";
    let input =
        json!({"reason":"fixture", "requirements":[{"id":"R1","status":marker}]}).to_string();
    let error = s.task_work_update("1", 1, &input).unwrap_err();
    assert_eq!(error.body.code, "INVALID_INPUT");
    assert!(!serde_json::to_string(&error.body).unwrap().contains(marker));
    assert!(s.task_work("1").unwrap().data["work"].is_null());
}

#[test]
fn work_is_task_local_readonly_and_independent_of_note_truncation() {
    let (dir, s) = start();
    let original = seed(&s);
    s.task_create_minimal().unwrap();
    assert!(s.task_work("2").unwrap().data["work"].is_null());
    for version in 2..57 {
        s.task_note("1", version, "progress", "unrelated later evidence")
            .unwrap();
    }
    assert_eq!(s.task_context("1").unwrap().data["work"], original["work"]);
    let before = s.task_show("1").unwrap().data;
    s.task_work("1").unwrap();
    s.task_context("1").unwrap();
    assert_eq!(s.task_show("1").unwrap().data, before);
    let missing = dir.path().join("absent.db");
    assert!(Service::new(&missing).task_work("1").is_err());
    assert!(!missing.exists());
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
}
