use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;

fn request(path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:43123")
        .header("x-steward-token", token)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn authenticated_sse_observes_external_writes_and_stops_on_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("state.db"));
    service.task_create_minimal().unwrap();
    let state = ServerState::new(service.clone(), 43123, "admin".into())
        .with_readonly_token("reader".into());
    let app = router(state.clone());
    let denied = app
        .clone()
        .oneshot(request("/api/events", "wrong"))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let response = app.oneshot(request("/api/events", "reader")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let mut stream = response.into_body().into_data_stream();
    let initial = stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&initial).contains("event: changed"));
    // Service writes through another SQLite connection, just like the CLI or Hook process.
    service.task_create_minimal().unwrap();
    let update = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&update).contains("event: changed"));
    assert!(!String::from_utf8_lossy(&update).contains("admin"));
    state.stop_event_streams();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn subscriber_limit_does_not_exhaust_regular_api_slots() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("state.db"));
    service.task_create_minimal().unwrap();
    let app = router(ServerState::new(service, 43123, "admin".into()));
    let mut subscribers = Vec::new();
    for _ in 0..16 {
        let response = app
            .clone()
            .oneshot(request("/api/events", "admin"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        subscribers.push(response);
    }
    assert_eq!(
        app.clone()
            .oneshot(request("/api/events", "admin"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        app.clone()
            .oneshot(request("/api/tasks", "admin"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    drop(subscribers);
    assert_eq!(
        app.oneshot(request("/api/events", "admin"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}
