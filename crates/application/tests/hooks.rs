use serde_json::{Value, json};
use steward_application::Service;

fn fixture() -> (tempfile::TempDir, Service) {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("test.db"));
    service.task_create_minimal().unwrap();
    service.task_claim("1", 1, "local-a", false).unwrap();
    service
        .session_bind("local-a", 2, "generic", "external-a")
        .unwrap();
    (temp, service)
}
fn event(id: &str) -> Value {
    json!({"schemaVersion":1,"sessionId":"local-a","source":"generic","externalSessionId":"external-a","eventId":id,"kind":"closed","occurredAt":"2026-09-07T00:00:00Z"})
}

#[test]
fn binding_is_explicit_once_and_preserves_execution_authority() {
    let (_temp, service) = fixture();
    let before = service.task_show("1").unwrap().data;
    assert_eq!(
        service
            .session_bind("local-a", 3, "generic", "external-a")
            .unwrap()
            .data["task"]["version"],
        3
    );
    assert_eq!(
        service
            .session_bind("local-a", 2, "generic", "external-a")
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert!(
        service
            .session_bind("local-a", 3, "generic", "different")
            .is_err()
    );
    service.task_create_minimal().unwrap();
    service.task_claim("2", 1, "local-b", false).unwrap();
    assert!(
        service
            .session_bind("local-b", 2, "generic", "external-a")
            .is_err()
    );
    assert_eq!(service.task_show("1").unwrap().data, before);
}

#[test]
fn duplicate_late_and_out_of_order_observations_never_close_or_change_task() {
    let (_temp, service) = fixture();
    let before = service.task_show("1").unwrap().data;
    let history = service.history("1").unwrap().data;
    let first = service.hook_ingest(&event("one").to_string()).unwrap();
    assert_eq!(first.data["duplicate"], false);
    assert_eq!(
        service.hook_ingest(&event("one").to_string()).unwrap().data["duplicate"],
        true
    );
    let mut conflicting = event("one");
    conflicting["kind"] = json!("idle");
    assert_eq!(
        service
            .hook_ingest(&conflicting.to_string())
            .unwrap_err()
            .body
            .code,
        "HOOK_EVENT_CONFLICT"
    );
    let mut old = event("old");
    old["occurredAt"] = json!("2020-01-01T00:00:00+00:00");
    service.hook_ingest(&old.to_string()).unwrap();
    assert_eq!(service.task_show("1").unwrap().data, before);
    assert_eq!(service.history("1").unwrap().data, history);
    assert!(service.session_show("local-a").unwrap().data["session"]["endedAt"].is_null());
    service.session_close("local-a", 3).unwrap();
    service.hook_ingest(&event("late").to_string()).unwrap();
    assert_eq!(service.task_show("1").unwrap().data["task"]["version"], 4);
    assert!(
        service
            .session_bind("local-a", 4, "generic", "external-a")
            .is_err()
    );
}

#[test]
fn pagination_clear_and_tombstones_prevent_resurrection() {
    let (_temp, service) = fixture();
    for id in ["one", "two", "three"] {
        service.hook_ingest(&event(id).to_string()).unwrap();
    }
    let page = service.hook_list("local-a", 0, 2).unwrap().data;
    assert_eq!(page["events"].as_array().unwrap().len(), 2);
    assert_eq!(page["hasMore"], true);
    let second = service
        .hook_list("local-a", page["nextAfter"].as_i64().unwrap(), 2)
        .unwrap()
        .data;
    assert_eq!(second["events"][0]["eventId"], "three");
    assert_eq!(service.hook_clear("local-a", 3).unwrap().data["removed"], 3);
    assert!(
        service.hook_list("local-a", 0, 200).unwrap().data["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        service.hook_ingest(&event("one").to_string()).unwrap().data["deleted"],
        true
    );
    assert_eq!(
        service.hook_clear("local-a", 4).unwrap().data["task"]["version"],
        4
    );
}

#[test]
fn invalid_sensitive_and_unbound_events_are_rejected_without_echoing_input() {
    let (_temp, service) = fixture();
    let mut secret = event("secret");
    secret["authorization"] = json!("sensitive-value");
    let error = service.hook_ingest(&secret.to_string()).unwrap_err();
    assert!(
        !serde_json::to_string(&error.body)
            .unwrap()
            .contains("sensitive-value")
    );
    assert!(service.hook_ingest(&"x".repeat(16 * 1024 + 1)).is_err());
    for (field, value) in [
        ("source", "other"),
        ("kind", "unknown"),
        ("occurredAt", "yesterday"),
        ("eventId", "../../file"),
    ] {
        let mut e = event("invalid");
        e[field] = json!(value);
        assert!(service.hook_ingest(&e.to_string()).is_err());
    }
    assert!(service.hook_list("local-a", 0, 201).is_err());
    assert!(service.hook_list("local-a", -1, 2).is_err());
    assert_eq!(
        service.hook_list("local-a", 0, 20).unwrap().data["events"],
        json!([])
    );
}

#[test]
fn concurrent_duplicate_delivery_is_atomic() {
    let (_temp, service) = fixture();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let s = service.clone();
            std::thread::spawn(move || s.hook_ingest(&event("same").to_string()).unwrap().data)
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r["duplicate"] == false).count(),
        1
    );
    assert_eq!(
        service.hook_list("local-a", 0, 20).unwrap().data["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(service.task_show("1").unwrap().data["task"]["version"], 3);
}

#[test]
fn capacity_counts_tombstones_but_still_accepts_duplicate_retries() {
    let (_temp, service) = fixture();
    service.hook_ingest(&event("existing").to_string()).unwrap();
    let connection = storage_sqlite::open_database(service.database_path()).unwrap();
    connection.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<9999) INSERT INTO session_events(session_id,event_id,fingerprint) SELECT 'local-a','reserved-'||x,'synthetic-fingerprint' FROM n;").unwrap();
    assert_eq!(
        service
            .hook_ingest(&event("existing").to_string())
            .unwrap()
            .data["duplicate"],
        true
    );
    assert_eq!(
        service
            .hook_ingest(&event("new").to_string())
            .unwrap_err()
            .body
            .code,
        "HOOK_CAPACITY_REACHED"
    );
    assert_eq!(
        service.hook_list("local-a", 0, 100).unwrap().data["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
