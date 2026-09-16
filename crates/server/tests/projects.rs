use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;

#[tokio::test]
async fn reader_can_read_profile_and_provenance_without_a_new_http_write_endpoint() {
    let (_temp, s, app) = fixture();
    s.project_create("Profile").unwrap();
    s.task_create_in_project(None, None, Some("##1")).unwrap();
    s.project_profile_set(
        "##1",
        1,
        steward_application::ProjectProfileInput {
            summary: "简介".into(),
            architecture: "架构入口".into(),
            development: "隔离验证".into(),
            source_task_id: 1,
            source_task_version: 1,
            evidence: "synthetic only".into(),
        },
    )
    .unwrap();
    let before = s.project_profile_show("##1").unwrap().data;
    let task = s.task_show("1").unwrap().data;
    let history = s.project_history("##1", 0, 50).unwrap().data;
    let (status, project) = request(&app, "/api/projects/1", None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(project["data"]["project"], before["project"]);
    assert_eq!(project["data"]["profile"], before["profile"]);
    assert_eq!(
        project["data"]["sessionRules"],
        json!({"formatVersion":1,"rules":[]})
    );
    let (status, context) = request(&app, "/api/tasks/1/context", None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(context["data"]["projectProfile"], before["profile"]);
    let (status, _) = request(
        &app,
        "/api/commands/project-profile-set",
        Some(json!({})),
        false,
    )
    .await;
    assert!(!status.is_success());
    assert_eq!(s.project_profile_show("##1").unwrap().data, before);
    assert_eq!(s.task_show("1").unwrap().data, task);
    assert_eq!(s.project_history("##1", 0, 50).unwrap().data, history);
}

fn fixture() -> (tempfile::TempDir, Service, Router) {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("state.db"));
    let app = router(
        ServerState::new(service.clone(), 43123, "fixture-admin".into())
            .with_readonly_token("fixture-reader".into()),
    );
    (temp, service, app)
}
async fn request(
    app: &Router,
    path: &str,
    input: Option<Value>,
    reader: bool,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(if input.is_some() { "POST" } else { "GET" })
                .uri(path)
                .header("host", "127.0.0.1:43123")
                .header("origin", "http://127.0.0.1:43123")
                .header(
                    "x-steward-token",
                    if reader {
                        "fixture-reader"
                    } else {
                        "fixture-admin"
                    },
                )
                .header("x-steward-csrf", "1")
                .header("content-type", "application/json")
                .body(
                    input
                        .map(|v| Body::from(v.to_string()))
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
async fn command(app: &Router, name: &str, input: Value) -> Value {
    let (status, out) = request(app, &format!("/api/commands/{name}"), Some(input), false).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    out["data"].clone()
}

#[tokio::test]
async fn project_source_task_query_context_journey_is_explicit_and_read_only_queries_do_not_mutate()
{
    let (temp, s, app) = fixture();
    let root = temp.path().join("documents");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("README.md"), "synthetic source bytes").unwrap();
    let p = command(&app, "project-create", json!({"name":"Mailroom"})).await;
    assert_eq!(p["project"]["revision"], 1);
    command(&app, "project-create", json!({"name":"Other"})).await;
    command(
        &app,
        "project-component-add",
        json!({"projectId":1,"expectedRevision":1,"name":"docs"}),
    )
    .await;
    let source=command(&app,"project-source-add",json!({"projectId":1,"expectedRevision":2,"component":"docs","location":{"kind":"directory","path":root}})).await;
    assert_eq!(source["project"]["revision"], 3);
    command(&app, "task-create", json!({"input":{}})).await;
    let task = command(
        &app,
        "task-project",
        json!({"taskId":1,"expectedVersion":1,"project":"##1","confirmed":true,"reason":"fixture"}),
    )
    .await;
    assert_eq!(task["task"]["projectId"], 1);
    command(
        &app,
        "task-components",
        json!({"taskId":1,"expectedVersion":2,"components":["docs"],"confirmed":true,"reason":"fixture"}),
    )
    .await;
    let before = s.history("1").unwrap().data;
    let project_before = s.project_history("1", 0, 200).unwrap().data;
    for (path, field) in [
        ("/api/projects?limit=1", "projects"),
        ("/api/projects/1/components", "components"),
        ("/api/projects/1/sources", "sources"),
        ("/api/tasks?project=%23%231", "tasks"),
    ] {
        let (status, data) = request(&app, path, None, true).await;
        assert_eq!(status, StatusCode::OK, "{data}");
        assert_eq!(data["data"][field].as_array().unwrap().len(), 1);
    }
    let (_, other) = request(&app, "/api/tasks?project=%23%232", None, true).await;
    assert_eq!(other["data"]["tasks"].as_array().unwrap().len(), 0);
    let (_, context) = request(&app, "/api/tasks/1/context", None, true).await;
    assert_eq!(context["data"]["project"]["name"], "Mailroom");
    assert_eq!(context["data"]["task"]["componentIds"], json!([1]));
    assert!(context["data"]["session"].is_null());
    assert!(context["data"].get("worktreeStatus").is_none());
    let (status, nav) = request(&app, "/api/projects/1/context?sourceId=1", None, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{nav}");
    assert!(!nav.to_string().contains("synthetic source bytes"));
    assert_eq!(
        request(&app, "/api/projects/2/context?sourceId=1", None, true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(s.history("1").unwrap().data, before);
    assert_eq!(s.project_history("1", 0, 200).unwrap().data, project_before);
    let (status, resolved) = request(&app, "/api/projects/1/sources/1", None, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!resolved.to_string().contains("synthetic source bytes"));
    let (_, sources) = request(&app, "/api/projects/1/sources", None, true).await;
    assert!(sources["data"].get("repositories").is_none());
    let source = sources["data"]["sources"][0].as_object().unwrap();
    assert_eq!(source.len(), 5);
    for field in [
        "id",
        "projectId",
        "componentId",
        "directoryPath",
        "createdAt",
    ] {
        assert!(source.contains_key(field));
    }
    command(
        &app,
        "project-source-remove",
        json!({"projectId":1,"expectedRevision":3,"sourceId":1,"confirmed":true}),
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "synthetic source bytes"
    );
    assert_eq!(s.task_show("1").unwrap().data["task"]["version"], 3);
}

#[tokio::test]
async fn source_directory_metadata_is_portable_verbatim_and_keeps_cas_history_guards() {
    let (_temp, s, app) = fixture();
    s.project_create("Portable").unwrap();
    let paths = [
        "/srv/nonexistent/项目资料",
        "E:/not-present/Project docs",
        r"C:\not-present\项目资料",
        r"\\unavailable.invalid\share\Project docs",
    ];
    for (index, path) in paths.iter().enumerate() {
        let revision = index as i64 + 1;
        let before = s.project_history("1", 0, 50).unwrap().data;
        let added = command(
            &app,
            "project-source-add",
            json!({"projectId":1,"expectedRevision":revision,"location":{"kind":"directory","path":path}}),
        ).await;
        assert_eq!(added["source"]["directoryPath"], *path);
        assert_eq!(added["project"]["revision"], revision + 1);
        let after = s.project_history("1", 0, 50).unwrap().data;
        let history = after["history"].as_array().unwrap();
        assert_eq!(
            &history[..history.len() - 1],
            before["history"].as_array().unwrap()
        );
        assert_eq!(history.len(), revision as usize + 1);
        assert_eq!(history.last().unwrap()["changeType"], "source.added");
        assert_eq!(history.last().unwrap()["payload"]["directoryPath"], *path);
    }
    let project = s.project_show("1").unwrap().data;
    let history = s.project_history("1", 0, 50).unwrap().data;
    let sources = s.project_sources("1").unwrap().data;
    let (status, listed) = request(&app, "/api/projects/1/sources", None, true).await;
    assert_eq!(status, StatusCode::OK);
    for (source, path) in listed["data"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .zip(paths)
    {
        assert_eq!(source["directoryPath"], path);
    }
    // Valid metadata with stale CAS must not append a source or ProjectHistory.
    let (status, error) = request(
        &app, "/api/commands/project-source-add",
        Some(json!({"projectId":1,"expectedRevision":1,"location":{"kind":"directory","path":"/stale/new-source"}})), false,
    ).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["details"]["entityType"], "Project");
    assert_eq!(s.project_show("1").unwrap().data, project);
    assert_eq!(s.project_history("1", 0, 50).unwrap().data, history);
    assert_eq!(s.project_sources("1").unwrap().data, sources);
    for path in [
        "relative/project",
        "C:relative",
        "",
        "/srv/line\nbreak",
        "E:/tab\tname",
        "/srv/null\0byte",
        "/srv/delete\u{7f}",
    ] {
        let (status, error) = request(
            &app, "/api/commands/project-source-add",
            Some(json!({"projectId":1,"expectedRevision":5,"location":{"kind":"directory","path":path}})), false,
        ).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path:?}: {error}");
        assert_eq!(error["error"]["code"], "INVALID_INPUT");
        assert_eq!(s.project_show("1").unwrap().data, project);
        assert_eq!(s.project_history("1", 0, 50).unwrap().data, history);
        assert_eq!(s.project_sources("1").unwrap().data, sources);
    }
}

#[tokio::test]
async fn project_revision_and_task_version_are_separate_and_inputs_fail_closed() {
    let (_temp, s, app) = fixture();
    command(&app, "project-create", json!({"name":"First"})).await;
    command(
        &app,
        "project-rename",
        json!({"projectId":1,"expectedRevision":1,"name":"Renamed"}),
    )
    .await;
    let (status, error) = request(
        &app,
        "/api/commands/project-component-add",
        Some(json!({"projectId":1,"expectedRevision":1,"name":"stale"})),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["details"]["entityType"], "Project");
    assert_eq!(error["error"]["details"]["currentRevision"], 2);
    command(&app, "task-create", json!({"input":{"project":"##1"}})).await;
    for body in [
        json!({"taskId":1,"expectedVersion":1,"confirmed":true,"reason":"fixture"}),
        json!({"taskId":1,"expectedVersion":1,"project":"##1","clear":true,"confirmed":true,"reason":"fixture"}),
        json!({"taskId":1,"expectedVersion":1,"clear":true,"confirmed":false,"reason":"fixture"}),
    ] {
        assert_eq!(
            request(&app, "/api/commands/task-project", Some(body), false)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    // Numeric command IDs must not be reinterpreted as names/keys such as "-1".
    command(&app, "project-create", json!({"name":"-1"})).await;
    assert_eq!(
        request(
            &app,
            "/api/commands/project-rename",
            Some(json!({"projectId":-1,"expectedRevision":1,"name":"wrong target"})),
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(s.project_show("##2").unwrap().data["project"]["name"], "-1");
    assert_eq!(
        request(
            &app,
            "/api/commands/task-project",
            Some(json!({"taskId":-1,"expectedVersion":1,"project":"##1","confirmed":true,"reason":"fixture"})),
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for location in [
        json!({"kind":"directory","path":"relative"}),
        json!({"kind":"directory","path":"/fixture","trusted":true}),
        json!({"kind":"git","worktree":"relative","relativePath":"."}),
    ] {
        assert_eq!(
            request(
                &app,
                "/api/commands/project-source-add",
                Some(json!({"projectId":1,"expectedRevision":2,"location":location})),
                false
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    for path in ["/api/projects?limit=201", "/api/projects?unknown=1"] {
        assert_eq!(
            request(&app, path, None, false).await.0,
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    for path in [
        "/api/projects/1/context",
        "/api/projects/1/context?sourceId=1&files=README.md",
        "/api/projects/1/sources/1?worktree=relative",
    ] {
        assert_eq!(
            request(&app, path, None, false).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(s.project_show("1").unwrap().data["project"]["revision"], 2);
    command(
        &app,
        "task-project",
        json!({"taskId":1,"expectedVersion":1,"clear":true,"confirmed":true,"reason":"fixture"}),
    )
    .await;
    assert!(s.task_show("1").unwrap().data["task"]["projectId"].is_null());
}

#[tokio::test]
async fn project_mutations_are_denied_to_readers_before_parsing_or_side_effects() {
    let (_temp, s, app) = fixture();
    for name in [
        "project-create",
        "project-rename",
        "project-component-add",
        "project-source-add",
        "project-source-remove",
        "task-project",
        "task-components",
    ] {
        let (status, out) = request(
            &app,
            &format!("/api/commands/{name}"),
            Some(json!({})),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(out["error"]["code"], "READ_ONLY");
    }
    assert_eq!(s.project_list(0, 50).unwrap().data["projects"], json!([]));
    assert_eq!(
        request(&app, "/api/access", None, true).await.1["data"]["projectManagement"],
        true
    );
}
