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
async fn project_task_filter_and_context_keep_project_isolation() {
    let (_temp, service, app) = fixture();
    service.project_create("Mailroom").unwrap();
    service.project_create("Steward").unwrap();
    service.task_set_project("1", 1, Some("##1"), true, "fixture").unwrap();
    service
        .task_create_in_project(None, None, Some("##2"))
        .unwrap();
    let before = service.history("1").unwrap().data;
    for path in ["/api/tasks?project=%23%231", "/api/tasks?project=MAILROOM"] {
        let (status, body) = request(&app, "GET", path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(body["data"]["tasks"][0]["projectId"], 1);
    }
    let (status, body) = request(&app, "GET", "/api/tasks?project=Missing", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    let (status, body) = request(&app, "GET", "/api/tasks/1/context", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["project"]["name"], "Mailroom");
    assert!(body["data"]["session"].is_null());
    assert!(body["data"]["worktreeStatus"].is_null());
    assert_eq!(service.history("1").unwrap().data, before);
}

#[tokio::test]
async fn reader_credentials_can_query_but_cannot_mutate_any_resource() {
    let (_temp, service, _) = fixture();
    let app = router(
        ServerState::new(service.clone(), 43123, TOKEN.into())
            .with_readonly_token("reader-only".into()),
    );
    for endpoint in ["/api/tasks", "/api/tasks/1/context", "/api/access"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(endpoint)
                    .header("host", "127.0.0.1:43123")
                    .header("x-steward-token", "reader-only")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        if endpoint == "/api/access" {
            let value: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            assert_eq!(value["data"]["role"], "reader");
        }
    }
    for endpoint in [
        "/api/commands/task-create",
        "/api/commands/task-update",
        "/api/commands/task-close",
        "/api/commands/task-claim",
        "/api/commands/task-note",
        "/api/commands/task-retitle",
        "/api/commands/session-bind",
        "/api/commands/session-import-add",
        "/api/commands/session-import-remove",
        "/api/commands/hook-clear",
        "/api/commands/worktree-create",
        "/api/commands/worktree-remove",
        "/api/hook",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(endpoint)
                    .header("host", "127.0.0.1:43123")
                    .header("x-steward-token", "reader-only")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{endpoint}");
        let error: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(error["error"]["code"], "READ_ONLY");
    }
    assert_eq!(service.task_show("1").unwrap().data["task"]["version"], 1);
    assert_eq!(
        service.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn browser_connection_code_is_one_use_and_does_not_authorize_other_endpoints() {
    let (_temp, service, _) = fixture();
    let state = ServerState::new(service, 43123, TOKEN.into());
    let url = state.browser_connection_url();
    assert!(!url.contains(TOKEN));
    let code = url.split("#connect=").nth(1).unwrap();
    let app = router(state);
    let make = |path: &str, code: &str, origin: &str| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:43123")
            .header("origin", origin)
            .header("content-type", "application/json")
            .header("x-steward-connect", code)
            .body(Body::from("{}"))
            .unwrap()
    };
    for (path, value, origin, expected) in [
        (
            "/api/connect",
            "wrong",
            "http://127.0.0.1:43123",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "/api/connect",
            code,
            "http://evil.invalid",
            StatusCode::FORBIDDEN,
        ),
        (
            "/api/commands/task-create",
            code,
            "http://127.0.0.1:43123",
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(make(path, value, origin))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let (a, b) = tokio::join!(
        app.clone()
            .oneshot(make("/api/connect", code, "http://127.0.0.1:43123")),
        app.clone()
            .oneshot(make("/api/connect", code, "http://127.0.0.1:43123"))
    );
    let mut responses = [a.unwrap(), b.unwrap()];
    responses.sort_by_key(|response| response.status().as_u16());
    let [success, refused] = responses;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(success.headers()["cache-control"], "no-store");
    let data: Value =
        serde_json::from_slice(&to_bytes(success.into_body(), 4096).await.unwrap()).unwrap();
    assert!(data["data"]["token"].is_null());
    assert_eq!(data["data"]["role"], "admin");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tasks")
                .header("host", "127.0.0.1:43123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn explicit_address_keeps_exact_host_origin_and_authentication_checks() {
    let (_temp, service, _) = fixture();
    let app = router(ServerState::with_address(
        service,
        "172.19.10.185:43123".parse().unwrap(),
        TOKEN.into(),
    ));
    for (host, origin, token, expected) in [
        (
            "172.19.10.185:43123",
            "http://172.19.10.185:43123",
            TOKEN,
            StatusCode::OK,
        ),
        (
            "127.0.0.1:43123",
            "http://172.19.10.185:43123",
            TOKEN,
            StatusCode::FORBIDDEN,
        ),
        (
            "172.19.10.185:43123",
            "http://localhost:43123",
            TOKEN,
            StatusCode::FORBIDDEN,
        ),
        ("172.19.10.185:43123", "null", TOKEN, StatusCode::FORBIDDEN),
        (
            "172.19.10.185:43123",
            "http://172.19.10.185:43123",
            "invalid",
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tasks")
                    .header("host", host)
                    .header("origin", origin)
                    .header("x-steward-token", token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn default_http_port_accepts_browser_authority_without_relaxing_origin_or_csrf() {
    let (_temp, service, _) = fixture();
    let state = ServerState::with_address(
        service.clone(),
        "172.19.10.185:80".parse().unwrap(),
        TOKEN.into(),
    );
    assert!(
        state
            .browser_connection_url()
            .starts_with("http://172.19.10.185/#connect=")
    );
    let app = router(state);
    for (host, origin, expected) in [
        ("172.19.10.185", "http://172.19.10.185", StatusCode::OK),
        ("172.19.10.185:80", "http://172.19.10.185", StatusCode::OK),
        (
            "172.19.10.185:8080",
            "http://172.19.10.185",
            StatusCode::FORBIDDEN,
        ),
        ("127.0.0.1", "http://172.19.10.185", StatusCode::FORBIDDEN),
        ("172.19.10.185", "http://evil.test", StatusCode::FORBIDDEN),
        (
            "172.19.10.185",
            "http://172.19.10.185:8080",
            StatusCode::FORBIDDEN,
        ),
        ("172.19.10.185", "null", StatusCode::FORBIDDEN),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/access")
                    .header("host", host)
                    .header("origin", origin)
                    .header("x-steward-token", TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let duplicated = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/access")
                .header("host", "172.19.10.185")
                .header("host", "172.19.10.185:80")
                .header("x-steward-token", TOKEN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicated.status(), StatusCode::FORBIDDEN);
    let anonymous = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/access")
                .header("host", "172.19.10.185")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("host", "172.19.10.185")
                .header("origin", "http://172.19.10.185")
                .header("x-steward-token", TOKEN)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    for (origin, csrf, expected) in [
        ("http://172.19.10.185", false, StatusCode::FORBIDDEN),
        ("http://evil.test", true, StatusCode::FORBIDDEN),
        ("http://172.19.10.185", true, StatusCode::OK),
    ] {
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/commands/task-create")
            .header("host", "172.19.10.185")
            .header("origin", origin)
            .header("cookie", &cookie)
            .header("content-type", "application/json");
        if csrf {
            req = req.header("x-steward-csrf", "1");
        }
        let response = app
            .clone()
            .oneshot(req.body(Body::from(r#"{"input":{}}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(
        service.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
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
        Some(json!({"taskId":1,"expectedVersion":1,"patch":{"goal":"from browser"},"confirmed":true,"reason":"fixture"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["task"]["version"], 2);
    s.task_update("1", 2, r#"{"goal":"from CLI"}"#, true, "fixture").unwrap();
    let (status, body) = request(
        &app,
        "POST",
        "/api/commands/task-update",
        Some(json!({"taskId":1,"expectedVersion":2,"patch":{"goal":"stale browser"},"confirmed":true,"reason":"fixture"})),
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
async fn static_ui_is_public_but_cannot_be_embedded_and_rotated_credentials_invalidate_auth() {
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
