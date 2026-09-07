use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;
fn req(
    method: &str,
    path: &str,
    cookie: Option<&str>,
    token: Option<&str>,
    csrf: bool,
    origin: &str,
) -> Request<Body> {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:43123")
        .header("origin", origin)
        .header("content-type", "application/json");
    if let Some(v) = cookie {
        r = r.header("cookie", v);
    }
    if let Some(v) = token {
        r = r.header("x-steward-token", v);
    }
    if csrf {
        r = r.header("x-steward-csrf", "1");
    }
    r.body(Body::from("{}")).unwrap()
}
async fn login(app: &Router, token: &str) -> String {
    let response = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/login",
            None,
            Some(token),
            false,
            "http://127.0.0.1:43123",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let set = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        set.contains("HttpOnly")
            && set.contains("SameSite=Strict")
            && set.contains("Max-Age=2592000")
            && set.contains("Path=/api")
    );
    assert!(!set.contains(token));
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(token));
    set.split(';').next().unwrap().to_owned()
}
#[tokio::test]
async fn local_one_use_link_can_replace_a_remembered_reader_grant() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("tasks.db"));
    service.task_create_minimal().unwrap();
    let state = ServerState::new(service, 43123, "admin-key".into())
        .with_readonly_token("reader-key".into());
    let url = state.browser_connection_url();
    let code = url.split("#connect=").nth(1).unwrap();
    let app = router(state);
    let reader = login(&app, "reader-key").await;
    let mut exchange = req(
        "POST",
        "/api/connect",
        Some(&reader),
        None,
        true,
        "http://127.0.0.1:43123",
    );
    exchange
        .headers_mut()
        .insert("x-steward-connect", code.parse().unwrap());
    let response = app.clone().oneshot(exchange).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let data: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(data["data"]["role"], "admin");
    assert_eq!(
        app.oneshot(req(
            "GET",
            "/api/access",
            Some(&reader),
            None,
            false,
            "http://127.0.0.1:43123"
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn browser_cookies_require_csrf_preserve_roles_and_support_logout_and_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("tasks.db"));
    service.task_create_minimal().unwrap();
    let app = router(
        ServerState::new(service.clone(), 43123, "admin-key".into())
            .with_readonly_token("reader-key".into()),
    );
    let admin = login(&app, "admin-key").await;
    let reader = login(&app, "reader-key").await;
    let origin = "http://127.0.0.1:43123";
    assert_eq!(
        app.clone()
            .oneshot(req("GET", "/api/tasks", Some(&admin), None, false, origin))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for (cookie, csrf, origin, expected) in [
        (&admin, false, origin, StatusCode::FORBIDDEN),
        (&admin, true, "http://evil.invalid", StatusCode::FORBIDDEN),
        (&reader, true, origin, StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(req(
                    "POST",
                    "/api/commands/task-create",
                    Some(cookie),
                    None,
                    csrf,
                    origin
                ))
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert_eq!(
        service.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let write = req(
        "POST",
        "/api/commands/task-create",
        Some(&admin),
        None,
        true,
        origin,
    )
    .map(|_| Body::from(r#"{"input":{}}"#));
    assert_eq!(
        app.clone().oneshot(write).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(req(
                "POST",
                "/api/logout",
                Some(&reader),
                None,
                true,
                origin
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(req("GET", "/api/tasks", Some(&reader), None, false, origin))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let other = login(&app, "reader-key").await;
    assert_eq!(
        app.clone()
            .oneshot(req(
                "POST",
                "/api/browser-sessions/revoke",
                Some(&other),
                None,
                true,
                origin
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(req(
                "POST",
                "/api/browser-sessions/revoke",
                Some(&admin),
                None,
                true,
                origin
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for cookie in [admin, other] {
        assert_eq!(
            app.clone()
                .oneshot(req("GET", "/api/tasks", Some(&cookie), None, false, origin))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn browser_grant_is_restored_after_server_state_recreation() {
    let temp = tempfile::tempdir().unwrap();
    steward_core::set_private_dir(temp.path()).unwrap();
    let path = temp.path().join("browser.db");
    let service = Service::new(temp.path().join("tasks.db"));
    service.task_create_minimal().unwrap();
    let state = || {
        ServerState::new(service.clone(), 43123, "admin-key".into())
            .with_browser_store(&path)
            .unwrap()
    };
    let app = router(state());
    let cookie = login(&app, "admin-key").await;
    drop(app);
    let app = router(state());
    assert_eq!(
        app.oneshot(req(
            "GET",
            "/api/access",
            Some(&cookie),
            None,
            false,
            "http://127.0.0.1:43123"
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::OK
    );
}
