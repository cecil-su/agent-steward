use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;
#[tokio::test]
async fn only_direct_local_peers_are_trusted_and_browser_writes_require_csrf() {
    let tmp = tempfile::tempdir().unwrap();
    let service = Service::new(tmp.path().join("tasks.db"));
    service.task_create_minimal().unwrap();
    let state = ServerState::with_address(
        service.clone(),
        "172.19.10.185:43123".parse().unwrap(),
        "admin".into(),
    )
    .with_readonly_token("reader".into());
    for (peer, extra, write, expected) in [
        (Some("172.19.10.185:1000"), None, false, 200),
        (Some("127.0.0.1:1000"), None, false, 200),
        (Some("172.19.10.186:1000"), None, false, 401),
        (None, None, false, 401),
        (
            Some("172.19.10.186:1000"),
            Some(("x-forwarded-for", "172.19.10.185")),
            false,
            401,
        ),
        (
            Some("172.19.10.185:1000"),
            Some(("forwarded", "for=172.19.10.186")),
            false,
            401,
        ),
        (
            Some("172.19.10.185:1000"),
            Some(("x-steward-token", "wrong")),
            false,
            401,
        ),
        (Some("172.19.10.185:1000"), None, true, 403),
        (
            Some("172.19.10.185:1000"),
            Some(("x-steward-csrf", "1")),
            true,
            200,
        ),
        (
            Some("172.19.10.185:1000"),
            Some(("origin", "http://evil.invalid")),
            false,
            403,
        ),
        (
            Some("172.19.10.186:1000"),
            Some(("x-steward-token", "reader")),
            true,
            403,
        ),
    ] {
        let mut r = Request::builder()
            .method(if write { "POST" } else { "GET" })
            .uri(if write {
                "/api/commands/task-create"
            } else {
                "/api/access"
            })
            .header("host", "172.19.10.185:43123")
            .header("content-type", "application/json");
        if write {
            r = r.header("origin", "http://172.19.10.185:43123");
        }
        if let Some((k, v)) = extra {
            r = r.header(k, v);
        }
        let mut r = r.body(Body::from(r#"{"input":{}}"#)).unwrap();
        if let Some(peer) = peer {
            r.extensions_mut()
                .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
        }
        let response = router(state.clone().with_local_access())
            .oneshot(r)
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            expected,
            "{peer:?} {extra:?} write={write}"
        );
        if expected == 200 && !write {
            let data: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            assert_eq!(data["data"]["local"], true);
        }
    }
    let mut r = Request::builder()
        .uri("/api/access")
        .header("host", "172.19.10.185:43123")
        .body(Body::empty())
        .unwrap();
    r.extensions_mut().insert(ConnectInfo(
        "172.19.10.185:1000"
            .parse::<std::net::SocketAddr>()
            .unwrap(),
    ));
    assert_eq!(
        router(state).oneshot(r).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
