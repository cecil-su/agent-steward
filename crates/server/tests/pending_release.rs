use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    endpoint: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(endpoint)
                .header("host", "172.19.10.185:43123")
                .header("origin", "http://172.19.10.185:43123")
                .header("x-steward-token", token)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
        .unwrap();
    (status, body)
}

#[tokio::test]
async fn seven_statuses_preserve_sessions_enforce_cas_and_reader_access() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("status.db"));
    assert_eq!(
        s.task_create_minimal().unwrap().data["task"]["status"],
        "todo"
    );
    s.task_claim("1", 1, "execution", false).unwrap();
    assert_eq!(s.task_show("1").unwrap().data["task"]["status"], "todo");
    let app = router(
        ServerState::with_address(
            s.clone(),
            "172.19.10.185:43123".parse().unwrap(),
            "admin".into(),
        )
        .with_readonly_token("reader".into()),
    );
    let sessions = s.session_list(Some("1")).unwrap().data;
    for status in [
        "backlog",
        "todo",
        "in_progress",
        "in_review",
        "blocked",
        "done",
        "cancelled",
        "in_progress",
    ] {
        let before = s.task_show("1").unwrap().data;
        let version = before["task"]["version"].as_i64().unwrap();
        let body = json!({"taskId":1,"expectedVersion":version,"status":status});
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/commands/task-status",
                "reader",
                body.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(s.task_show("1").unwrap().data, before);
        let (code, out) = request(&app, "POST", "/api/commands/task-status", "admin", body).await;
        assert_eq!(code, StatusCode::OK, "{out}");
        assert_eq!(out["schemaVersion"], 3);
        assert_eq!(out["data"]["task"]["status"], status);
        assert_eq!(out["data"]["task"]["currentSessionId"], "execution");
        let history = s.history("1").unwrap().data;
        let task = s.task_show("1").unwrap().data;
        let (code, _) = request(
            &app,
            "POST",
            "/api/commands/task-status",
            "admin",
            json!({"taskId":1,"expectedVersion":version+1,"status":status}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(s.task_show("1").unwrap().data, task);
        assert_eq!(s.history("1").unwrap().data, history);
        let (code, out) = request(
            &app,
            "POST",
            "/api/commands/task-status",
            "admin",
            json!({"taskId":1,"expectedVersion":version,"status":status}),
        )
        .await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(out["error"]["code"], "VERSION_CONFLICT");
        for (filter, count) in [
            (format!("status={status}"), 1),
            (
                "view=active".into(),
                usize::from(!["done", "cancelled"].contains(&status)),
            ),
        ] {
            let (code, out) = request(
                &app,
                "GET",
                &format!("/api/tasks?{filter}"),
                "reader",
                Value::Null,
            )
            .await;
            assert_eq!(code, StatusCode::OK);
            assert_eq!(out["data"]["tasks"].as_array().unwrap().len(), count);
        }
        assert_eq!(s.session_list(Some("1")).unwrap().data, sessions);
    }
    let before = s.task_show("1").unwrap().data;
    for command in [
        "task-block",
        "task-unblock",
        "task-pending-release",
        "task-continue",
        "task-close",
        "worktree-create",
        "worktree-adopt",
        "worktree-remove",
        "worktree-detach",
    ] {
        assert_eq!(
            request(
                &app,
                "POST",
                &format!("/api/commands/{command}"),
                "admin",
                json!({"taskId":1,"expectedVersion":before["task"]["version"]})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    for path in [
        "/api/tasks/1/worktree-status",
        "/api/projects/1/context",
        "/api/projects/1/sources/1",
    ] {
        assert_eq!(
            request(&app, "GET", path, "reader", Value::Null).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (code, context) = request(&app, "GET", "/api/tasks/1/context", "reader", Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert!(context["data"].get("worktreeStatus").is_none());
    for field in [
        "worktreePath",
        "repositoryPath",
        "repositoryBranch",
        "repositoryCommonDir",
    ] {
        assert!(context["data"]["task"].get(field).is_none());
    }
    assert_eq!(s.task_show("1").unwrap().data, before);
}
