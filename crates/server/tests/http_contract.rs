use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;
const TOKEN: &str = "synthetic-test-credential-not-a-real-secret";
fn fixture() -> (tempfile::TempDir, Service, Router) {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("state.db"));
    s.task_create_minimal().unwrap();
    let app = router(ServerState::new(s.clone(), 43123, TOKEN.into()));
    (temp, s, app)
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:43123")
        .header("x-steward-token", TOKEN);
    if body.is_some() {
        builder = builder
            .header("content-type", "application/json")
            .header("origin", "http://127.0.0.1:43123");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(
                    body.map(|v| Body::from(v.to_string()))
                        .unwrap_or(Body::empty()),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn authentication_origin_and_csrf_fail_before_storage_changes() {
    let (_temp, s, app) = fixture();
    for (host, origin, token, content, expected) in [
        (
            "127.0.0.1:43123",
            None,
            None,
            "application/json",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "evil.test:43123",
            None,
            Some(TOKEN),
            "application/json",
            StatusCode::FORBIDDEN,
        ),
        (
            "127.0.0.1:43123",
            Some("null"),
            Some(TOKEN),
            "application/json",
            StatusCode::FORBIDDEN,
        ),
        (
            "127.0.0.1:43123",
            Some("http://evil.test"),
            Some(TOKEN),
            "application/json",
            StatusCode::FORBIDDEN,
        ),
        (
            "127.0.0.1:43123",
            Some("http://127.0.0.1:43123"),
            Some("wrong"),
            "application/json",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "127.0.0.1:43123",
            None,
            Some(TOKEN),
            "text/plain",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/commands/task-create")
            .header("host", host)
            .header("content-type", content);
        if let Some(v) = origin {
            req = req.header("origin", v);
        }
        if let Some(v) = token {
            req = req.header("x-steward-token", v);
        }
        let response = app
            .clone()
            .oneshot(req.body(Body::from(r#"{"input":{}}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 10000).await.unwrap()).unwrap();
        assert_eq!(body["schemaVersion"], 2);
        assert_eq!(body["ok"], false);
    }
    assert_eq!(
        s.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[tokio::test]
async fn commands_and_cli_share_cas_and_conflict_details() {
    let (_temp, s, app) = fixture();
    let (status, body) = request(
        &app,
        "POST",
        "/api/commands/task-update",
        Some(json!({"taskId":1,"expectedVersion":1,"patch":{"goal":"from browser"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["task"]["version"], 2);
    s.task_update("1", 2, r#"{"goal":"from CLI"}"#).unwrap();
    let (status, body) = request(
        &app,
        "POST",
        "/api/commands/task-update",
        Some(json!({"taskId":1,"expectedVersion":2,"patch":{"goal":"stale browser"}})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
    assert!(body["data"].is_null());
    assert_eq!(s.task_show("1").unwrap().data["task"]["goal"], "from CLI");
    let (_, body) = request(&app, "GET", "/api/tasks/1/context", None).await;
    assert_eq!(body["data"]["task"]["version"], 3);
    let (_, body) = request(&app, "GET", "/api/tasks?view=active&pageSize=1", None).await;
    assert_eq!(body["data"]["tasks"].as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn strict_dto_body_limits_and_confirmation_are_enforced() {
    let (_temp, _s, app) = fixture();
    for (path, body) in [
        (
            "/api/commands/task-create",
            json!({"input":{},"unexpected":"secret-do-not-echo"}),
        ),
        (
            "/api/commands/task-close",
            json!({"taskId":1,"expectedVersion":1,"outcome":"cancelled","reason":"test","confirmed":false}),
        ),
        (
            "/api/commands/worktree-create",
            json!({"taskId":1,"expectedVersion":1,"repo":"relative","path":"relative","branch":"main","confirmed":true}),
        ),
        (
            "/api/commands/session-import-add",
            json!({"taskId":1,"expectedVersion":1,"sessionId":"none","path":"/definitely/not/read","confirmSensitiveContentReviewed":false}),
        ),
    ] {
        let (status, result) = request(&app, "POST", path, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!result.to_string().contains("secret-do-not-echo"));
    }
    let (status, _) = request(
        &app,
        "POST",
        "/api/hook",
        Some(json!({"body":"x".repeat(17000)})),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _) = request(&app, "GET", "/api/tasks?pageSize=no", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, result) = request(&app, "GET", "/api/no-such-route", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(result["ok"], false);
}
#[tokio::test]
async fn http_observations_do_not_change_task_and_can_be_deleted_explicitly() {
    let (_temp, s, app) = fixture();
    s.task_claim("1", 1, "session-a", false).unwrap();
    let (status,_)=request(&app,"POST","/api/commands/session-bind",Some(json!({"sessionId":"session-a","expectedVersion":2,"source":"generic","externalSessionId":"external-a"}))).await;
    assert_eq!(status, StatusCode::OK);
    let event = json!({"schemaVersion":1,"sessionId":"session-a","source":"generic","externalSessionId":"external-a","eventId":"event-a","kind":"idle","occurredAt":"2026-09-07T00:00:00Z"});
    assert_eq!(
        request(&app, "POST", "/api/hook", Some(event.clone()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "POST", "/api/hook", Some(event)).await.1["data"]["duplicate"],
        true
    );
    assert_eq!(s.task_show("1").unwrap().data["task"]["version"], 3);
    let (_, result) = request(&app, "GET", "/api/sessions/session-a/events", None).await;
    assert_eq!(result["data"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/commands/hook-clear",
            Some(json!({"sessionId":"session-a","expectedVersion":3,"confirmed":true}))
        )
        .await
        .0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn static_ui_is_public_but_cannot_be_embedded_and_restart_invalidates_auth() {
    let (_temp, s, app) = fixture();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "127.0.0.1:43123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    let fresh = router(ServerState::new(
        s,
        43123,
        "another-synthetic-credential".into(),
    ));
    assert_eq!(
        request(&fresh, "GET", "/api/tasks", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn framework_errors_fetch_metadata_and_duplicate_headers_keep_json_envelope() {
    let (_temp, _s, app) = fixture();
    let (status, body) = request(&app, "GET", "/api/sessions/%FF", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_INPUT");
    for (key, value) in [
        ("sec-fetch-site", "cross-site"),
        ("origin", "http://evil.test"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tasks")
                    .header("host", "127.0.0.1:43123")
                    .header("x-steward-token", TOKEN)
                    .header(key, value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tasks")
                .header("host", "127.0.0.1:43123")
                .header("host", "evil.test")
                .header("x-steward-token", TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn notes_remain_readable_after_write_and_read_has_no_side_effects() {
    let (_temp, s, app) = fixture();
    s.task_note("1", 1, "decision", "A synthetic decision to retain")
        .unwrap();
    let before = s.task_show("1").unwrap().data;
    let (status, body) = request(&app, "GET", "/api/tasks/1/notes", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["data"]["notes"][0]["text"],
        "A synthetic decision to retain"
    );
    assert_eq!(s.task_show("1").unwrap().data, before);
}
