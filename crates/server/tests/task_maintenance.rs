use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;

#[tokio::test]
async fn maintenance_api_enforces_authorization_cas_and_closed_invariants() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("maintenance.db"));
    s.project_create("Example").unwrap();
    s.project_component_add("1", 1, "api").unwrap();
    s.task_create_minimal().unwrap();
    s.task_claim("1", 1, "execution", false).unwrap();
    s.task_close("1", 2, "cancelled", Some("fixture")).unwrap();
    let sessions = s.session_list(Some("1")).unwrap().data;
    let app = router(
        ServerState::new(s.clone(), 43123, "fixture-admin".into())
            .with_readonly_token("fixture-reader".into()),
    );
    let valid = json!({"taskId":1,"expectedVersion":3,"confirmed":true,"reason":"user authorized fixture correction", "patch":{"goal":"corrected", "project":"Example", "components":["api"]}});
    for (token, body, expected) in [
        ("fixture-reader", valid.clone(), StatusCode::FORBIDDEN),
        (
            "fixture-admin",
            json!({"taskId":1,"expectedVersion":3,"patch":{"goal":"bad"}}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "fixture-admin",
            {
                let mut v = valid.clone();
                v["confirmed"] = json!(false);
                v
            },
            StatusCode::BAD_REQUEST,
        ),
        (
            "fixture-admin",
            {
                let mut v = valid.clone();
                v["reason"] = json!(" ");
                v
            },
            StatusCode::BAD_REQUEST,
        ),
        ("fixture-admin", valid.clone(), StatusCode::OK),
        ("fixture-admin", valid, StatusCode::CONFLICT),
    ] {
        let before = s.task_show("1").unwrap().data;
        let history = s.history("1").unwrap().data;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/commands/task-update")
                    .header("host", "127.0.0.1:43123")
                    .header("origin", "http://127.0.0.1:43123")
                    .header("x-steward-token", token)
                    .header("x-steward-csrf", "1")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let out: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(status, expected, "{out}");
        if status != StatusCode::OK {
            assert_eq!(s.task_show("1").unwrap().data, before);
            assert_eq!(s.history("1").unwrap().data, history);
        } else {
            assert_eq!(out["data"]["task"]["projectId"], 1);
            assert_eq!(out["data"]["task"]["componentIds"], json!([1]));
        }
        assert_eq!(s.session_list(Some("1")).unwrap().data, sessions);
        assert_eq!(s.task_show("1").unwrap().data["task"]["status"], "closed");
    }
}
