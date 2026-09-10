use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;
#[tokio::test]
async fn reader_context_exposes_rules_and_no_rule_write_route_exists() {
    let t = tempfile::tempdir().unwrap();
    let s = Service::new(t.path().join("db"));
    s.project_create("One").unwrap();
    s.task_create_in_project(None, None, Some("1")).unwrap();
    s.rule_create(serde_json::from_value(json!({"scope":"global","projectId":null,"status":"active","contentVersion":1,"content":{"name":"Test","body":"Full body","sources":[]}})).unwrap(),"fixture").unwrap();
    let app = router(
        ServerState::new(s.clone(), 43123, "fixture-admin".into())
            .with_readonly_token("fixture-reader".into()),
    );
    let before = s.task_show("1").unwrap().data;
    for path in ["/api/tasks/1/context", "/api/projects/1"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "127.0.0.1:43123")
                    .header("x-steward-token", "fixture-reader")
                    .header("x-steward-ui-contract", "4")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(
            body["data"]["sessionRules"]["rules"][0]["content"]["body"],
            "Full body"
        );
    }
    for contract in ["1", "2", "3"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tasks/1/context")
                    .header("host", "127.0.0.1:43123")
                    .header("x-steward-token", "fixture-reader")
                    .header("x-steward-ui-contract", contract)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.status().is_success());
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/commands/rule-create")
                .header("host", "127.0.0.1:43123")
                .header("origin", "http://127.0.0.1:43123")
                .header("x-steward-token", "fixture-admin")
                .header("x-steward-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!response.status().is_success());
    assert_eq!(s.task_show("1").unwrap().data, before);
    assert_eq!(
        s.rule_list(None, None, None).unwrap().data["rules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
