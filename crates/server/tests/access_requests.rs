use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;
const ADMIN: &str = "synthetic-admin-access-test";
const READER: &str = "synthetic-reader-access-test";
fn fixture() -> (tempfile::TempDir, Router) {
    let temp = tempfile::tempdir().unwrap();
    let state = ServerState::new(
        Service::new(temp.path().join("tasks.db")),
        43123,
        ADMIN.into(),
    )
    .with_readonly_token(READER.into());
    (temp, router(state))
}
async fn send(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    token: Option<&str>,
    body: Option<Value>,
    csrf: bool,
) -> (StatusCode, Value, Option<String>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:43123")
        .extension(ConnectInfo(
            "192.0.2.8:50000".parse::<std::net::SocketAddr>().unwrap(),
        ));
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(token) = token {
        builder = builder.header("x-steward-token", token);
    }
    if csrf {
        builder = builder
            .header("origin", "http://127.0.0.1:43123")
            .header("x-steward-csrf", "1");
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
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
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().to_owned());
    let value = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
        .unwrap();
    (status, value, cookie)
}
#[tokio::test]
async fn approval_grants_only_reader_and_revocation_blocks_the_same_browser() {
    let (temp, app) = fixture();
    let (status, pending, cookie) = send(
        &app,
        "POST",
        "/api/access-request",
        None,
        None,
        Some(json!({"label":"合成浏览器"})),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending["data"]["state"], "pending");
    let set_cookie = cookie.unwrap();
    assert!(set_cookie.contains("HttpOnly; SameSite=Strict"));
    let cookie = set_cookie.split(';').next().unwrap();
    assert_eq!(
        send(&app, "GET", "/api/tasks", Some(cookie), None, None, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/access-requests",
            Some(cookie),
            None,
            None,
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, list, _) = send(
        &app,
        "GET",
        "/api/access-requests",
        None,
        Some(ADMIN),
        None,
        false,
    )
    .await;
    let row = &list["data"]["pending"][0];
    let id = row["id"].as_str().unwrap();
    assert_eq!(row["peer"], "192.0.2.8");
    assert_eq!(row["verificationCode"], pending["data"]["verificationCode"]);
    assert!(!list.to_string().contains(cookie.split('=').nth(1).unwrap()));
    let approve = format!("/api/access-requests/{id}/approve");
    let body = Some(json!({"verificationCode":row["verificationCode"]}));
    assert_eq!(
        send(
            &app,
            "POST",
            &approve,
            None,
            Some(READER),
            body.clone(),
            true
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &approve,
            None,
            Some(ADMIN),
            body.clone(),
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &approve,
            None,
            Some(ADMIN),
            body.clone(),
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/access-request",
            Some(cookie),
            None,
            None,
            false
        )
        .await
        .1["data"]["state"],
        "authorized"
    );
    assert_eq!(
        send(&app, "GET", "/api/access", Some(cookie), None, None, false)
            .await
            .1["data"]["role"],
        "reader"
    );
    assert_eq!(
        send(&app, "GET", "/api/access", None, None, None, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED,
        "another browser on the same peer IP must remain unauthorized"
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/commands/task-create",
            Some(cookie),
            None,
            Some(json!({})),
            true
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/access-requests",
            Some(cookie),
            None,
            None,
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let revoke = format!("/api/access-requests/{id}/revoke");
    assert_eq!(
        send(
            &app,
            "POST",
            &revoke,
            None,
            Some(ADMIN),
            Some(json!({})),
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "GET", "/api/access", Some(cookie), None, None, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/access-request",
            Some(cookie),
            None,
            None,
            false
        )
        .await
        .1["data"]["state"],
        "revoked"
    );
    assert_eq!(
        send(&app, "POST", &approve, None, Some(ADMIN), body, true)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert!(
        !temp.path().join("tasks.db").exists(),
        "approval must not initialize or mutate task storage"
    );
}
#[tokio::test]
async fn public_requests_require_csrf_and_are_bounded_and_private() {
    let (temp, app) = fixture();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/access-request")
                .header("host", "127.0.0.1:43123")
                .header("origin", "http://untrusted.invalid")
                .header("x-steward-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"label":"browser"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/access-request",
            None,
            None,
            Some(json!({"label":"browser"})),
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/access-request",
            None,
            None,
            Some(json!({"label":"x".repeat(5000)})),
            true
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/access-request",
            None,
            None,
            Some(json!({"label":"","role":"admin"})),
            true
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for _ in 0..4 {
        assert_eq!(
            send(
                &app,
                "POST",
                "/api/access-request",
                None,
                None,
                Some(json!({"label":"browser"})),
                true
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/access-request",
            None,
            None,
            Some(json!({"label":"browser"})),
            true
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        send(&app, "GET", "/api/access-request", None, None, None, false)
            .await
            .1["data"]["state"],
        "none"
    );
    assert!(!temp.path().join("tasks.db").exists());
}
