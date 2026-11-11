use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;

async fn call(
    app: &axum::Router,
    path: &str,
    token: &str,
    body: Option<Value>,
    origin: &str,
    csrf: bool,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:43123")
        .header("origin", origin)
        .header("x-steward-token", token)
        .header("x-steward-ui-contract", "5");
    if body.is_some() {
        request = request
            .method("POST")
            .header("content-type", "application/json")
            .header("cookie", "synthetic=not-a-credential");
        if csrf {
            request = request.header("x-steward-csrf", "1");
        }
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
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
async fn deletion_http_permissions_identity_cas_and_readonly_audit() {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::new(dir.path().join("synthetic.db"));
    s.task_create_minimal().unwrap();
    s.task_create_minimal().unwrap();
    s.task_work_update("1",1,&json!({"reason":"fixture","requirements":[{"id":"R1","title":"first","acceptanceCriteria":"works"}]}).to_string()).unwrap();
    let app = router(
        ServerState::new(s.clone(), 43123, "synthetic-admin".into())
            .with_readonly_token("synthetic-reader".into()),
    );
    let origin = "http://127.0.0.1:43123";
    let (_, access) = call(&app, "/api/access", "synthetic-admin", None, origin, true).await;
    assert_eq!(access["data"]["taskDeletion"], true);
    assert_eq!(access["data"]["requirementDeletion"], true);
    for path in [
        "/api/tasks/1/delete-preview",
        "/api/tasks/1/requirements/R1/delete-preview",
    ] {
        assert_eq!(
            call(&app, path, "synthetic-reader", None, origin, true)
                .await
                .0,
            StatusCode::OK
        );
    }
    let requirement = json!({"taskId":1,"expectedVersion":2,"requirementId":"R1","confirmed":true,"reason":"explicit fixture removal"});
    for command in ["task-delete", "task-requirement-delete"] {
        let mut body = requirement.clone();
        if command == "task-delete" {
            body.as_object_mut().unwrap().remove("requirementId");
        }
        let path = format!("/api/commands/{command}");
        let before = s.task_audit("1").unwrap().data;
        for (token, request_origin, csrf, expected) in [
            ("synthetic-reader", origin, true, StatusCode::FORBIDDEN),
            (
                "synthetic-admin",
                "http://other.invalid",
                true,
                StatusCode::FORBIDDEN,
            ),
            ("synthetic-admin", origin, false, StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                call(&app, &path, token, Some(body.clone()), request_origin, csrf)
                    .await
                    .0,
                expected
            );
        }
        for invalid in [
            {
                let mut b = body.clone();
                b["confirmed"] = json!(false);
                b
            },
            {
                let mut b = body.clone();
                b["reason"] = json!("");
                b
            },
            {
                let mut b = body.clone();
                b["status"] = json!("cancelled");
                b
            },
        ] {
            assert_eq!(
                call(&app, &path, "synthetic-admin", Some(invalid), origin, true)
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(s.task_audit("1").unwrap().data, before);
    }
    let path = "/api/commands/task-requirement-delete";
    assert_eq!(
        call(
            &app,
            path,
            "synthetic-admin",
            Some(requirement.clone()),
            origin,
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            path,
            "synthetic-admin",
            Some(requirement),
            origin,
            true
        )
        .await
        .1["error"]["code"],
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.task_work("1").unwrap().data["work"]["requirements"],
        json!([])
    );
    assert_eq!(s.task_show("2").unwrap().data["task"]["version"], 1);
    let delete = json!({"taskId":1,"expectedVersion":3,"confirmed":true,"reason":"logical fixture deletion"});
    assert_eq!(
        call(
            &app,
            "/api/commands/task-delete",
            "synthetic-admin",
            Some(delete),
            origin,
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    for path in [
        "/api/tasks/1",
        "/api/tasks/1/context",
        "/api/tasks/1/work",
        "/api/tasks/1/history",
    ] {
        let (status, body) = call(&app, path, "synthetic-reader", None, origin, true).await;
        assert_eq!(status, StatusCode::GONE);
        assert_eq!(body["error"]["code"], "TASK_DELETED");
    }
    let (status, audit) = call(
        &app,
        "/api/tasks/1/audit",
        "synthetic-reader",
        None,
        origin,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(audit["data"]["deletion"].is_object());
    assert_eq!(audit["data"]["task"]["status"], "todo");
    let (_, list) = call(
        &app,
        "/api/tasks?pageSize=30",
        "synthetic-reader",
        None,
        origin,
        true,
    )
    .await;
    assert_eq!(list["data"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(list["data"]["tasks"][0]["id"], 2);
    s.task_claim("2", 1, "still-running", false).unwrap();
    let (status, blocked) = call(
        &app,
        "/api/commands/task-delete",
        "synthetic-admin",
        Some(json!({"taskId":2,"expectedVersion":2,"confirmed":true,"reason":"blocked"})),
        origin,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(blocked["error"]["code"], "DELETE_BLOCKED");
}
