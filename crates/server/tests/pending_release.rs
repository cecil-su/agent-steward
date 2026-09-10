use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;

#[tokio::test]
async fn release_commands_obey_auth_cas_confirmation_and_read_filters() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("release.db"));
    s.task_create("FIXTURE", &json!({"title":"0910｜功能｜Fixture","goal":"goal","scope":"scope","acceptanceCriteria":"criteria"}).to_string()).unwrap();
    s.task_claim("1", 1, "execution", false).unwrap();
    let app = router(
        ServerState::new(s.clone(), 43123, "fixture-admin".into())
            .with_readonly_token("fixture-reader".into()),
    );
    for (command, token, version, confirmed, expected) in [
        (
            "task-pending-release",
            "fixture-reader",
            2,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "task-pending-release",
            "fixture-admin",
            1,
            None,
            StatusCode::CONFLICT,
        ),
        (
            "task-pending-release",
            "fixture-admin",
            2,
            None,
            StatusCode::OK,
        ),
        (
            "task-continue",
            "fixture-reader",
            3,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "task-continue",
            "fixture-admin",
            2,
            None,
            StatusCode::CONFLICT,
        ),
        ("task-continue", "fixture-admin", 3, None, StatusCode::OK),
        (
            "task-pending-release",
            "fixture-admin",
            4,
            None,
            StatusCode::OK,
        ),
        (
            "task-close",
            "fixture-admin",
            5,
            Some(false),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let before = s.task_show("1").unwrap().data;
        let history = s.history("1").unwrap().data;
        let sessions = s.session_list(Some("1")).unwrap().data;
        let mut body = json!({"taskId":1,"expectedVersion":version});
        if let Some(confirmed) = confirmed {
            body["confirmed"] = json!(confirmed);
            body["outcome"] = json!("completed");
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/commands/{command}"))
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
        assert_eq!(response.status(), expected);
        if expected != StatusCode::OK {
            assert_eq!(s.task_show("1").unwrap().data, before);
            assert_eq!(s.history("1").unwrap().data, history);
        }
        assert_eq!(s.session_list(Some("1")).unwrap().data, sessions);
    }
    for (query, count) in [
        ("view=pending-release", 1),
        ("status=pending_release", 1),
        ("view=active", 1),
        ("view=recent", 1),
        ("view=in-progress", 0),
        ("status=closed", 0),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks?{query}"))
                    .header("host", "127.0.0.1:43123")
                    .header("x-steward-token", "fixture-reader")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(body["data"]["tasks"].as_array().unwrap().len(), count);
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/commands/task-close")
                .header("host", "127.0.0.1:43123")
                .header("origin", "http://127.0.0.1:43123")
                .header("x-steward-token", "fixture-admin")
                .header("x-steward-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"taskId":1,"expectedVersion":5,"outcome":"completed","confirmed":true})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(s.task_show("1").unwrap().data["task"]["status"], "closed");
}
