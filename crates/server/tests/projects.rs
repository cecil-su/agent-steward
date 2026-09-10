use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::path::Path;
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

// HTTP selects the spelling advertised by Git, not an arbitrary filesystem alias.
// Only this test's owned fixture paths may be canonicalized to find the intended
// checkout; production must still reject unadvertised paths before request-path IO.
fn advertised_checkout(repo: &Path, checkout: &Path) -> String {
    let expected = std::fs::canonicalize(checkout).unwrap();
    let output = std::process::Command::new("git")
        .current_dir(std::fs::canonicalize(repo).unwrap())
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let matches: Vec<_> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .map(|bytes| std::str::from_utf8(bytes).unwrap())
        .filter(|path| std::fs::canonicalize(path).unwrap() == expected)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "select the intended fixture, not a default"
    );
    matches[0].to_owned()
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
    assert!(context["data"]["session"].is_null() && context["data"]["worktreeStatus"].is_null());
    let (status, nav) = request(&app, "/api/projects/1/context?sourceId=1", None, true).await;
    assert_eq!(status, StatusCode::OK, "{nav}");
    assert!(
        nav["data"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"].as_str().unwrap().ends_with("README.md"))
    );
    assert!(!nav.to_string().contains("synthetic source bytes"));
    assert_eq!(nav["data"]["reuseAllowed"], false);
    assert_eq!(
        request(&app, "/api/projects/2/context?sourceId=1", None, true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(s.history("1").unwrap().data, before);
    assert_eq!(s.project_history("1", 0, 200).unwrap().data, project_before);
    let (_, resolved) = request(&app, "/api/projects/1/sources/1", None, true).await;
    assert!(resolved["data"]["resolvedPath"].is_string());
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
    for path in [
        "/api/projects?limit=201",
        "/api/projects?unknown=1",
        "/api/projects/1/context",
        "/api/projects/1/context?sourceId=1&files=README.md",
        "/api/projects/1/sources/1?worktree=relative",
    ] {
        assert_eq!(
            request(&app, path, None, false).await.0,
            StatusCode::BAD_REQUEST,
            "{path}"
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
async fn git_registration_http_context_requires_an_explicit_matching_worktree() {
    let (temp, s, app) = fixture();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(repo.join("README.md"), "fixture source").unwrap();
    command(&app, "project-create", json!({"name":"Git project"})).await;
    command(&app, "project-source-add", json!({"projectId":1,"expectedRevision":1,"location":{"kind":"git","worktree":repo,"relativePath":"."}})).await;
    assert_eq!(
        request(&app, "/api/projects/1/context?sourceId=1", None, true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let history = s.project_history("1", 0, 200).unwrap().data;
    let selected = advertised_checkout(&repo, &repo);
    let encoded: String = selected.bytes().map(|b| format!("%{b:02X}")).collect();
    let (status, data) = request(
        &app,
        &format!("/api/projects/1/context?sourceId=1&worktree={encoded}"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{data}");
    assert_eq!(data["data"]["gitStateObserved"], true);
    assert_eq!(data["data"]["source"]["id"], 1);
    // Rejected selections must fail as candidates, not probe an arbitrary path.
    let unrelated = temp.path().join("unregistered");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.join(".git"), "UNTRUSTED_GIT_SENTINEL").unwrap();
    let mut rejected = vec![
        unrelated.to_string_lossy().into_owned(),
        temp.path().join("missing").to_string_lossy().into_owned(),
        repo.join("..").join("repo").to_string_lossy().into_owned(),
    ];
    #[cfg(windows)]
    {
        // On a case-insensitive volume this resolves to the same object, but the
        // altered component spelling is not advertised. On a case-sensitive
        // volume it is simply an absent path; it must be refused there as well.
        let alias = Path::new(&selected).with_file_name("REPO");
        if alias.try_exists().unwrap() {
            assert_eq!(
                std::fs::canonicalize(&alias).unwrap(),
                std::fs::canonicalize(&repo).unwrap()
            );
        }
        rejected.push(alias.to_str().unwrap().to_owned());
    }
    rejected.extend([
        r"\\untrusted.invalid\share\checkout".into(),
        r"\\?\UNC\untrusted.invalid\share\checkout".into(),
        r"\\.\PIPE\untrusted".into(),
    ]);
    for candidate in rejected {
        let encoded: String = candidate.bytes().map(|b| format!("%{b:02X}")).collect();
        for endpoint in [
            format!("/api/projects/1/context?sourceId=1&worktree={encoded}"),
            format!("/api/projects/1/sources/1?worktree={encoded}"),
        ] {
            let (status, data) = request(&app, &endpoint, None, true).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{data}");
            assert_eq!(data["error"]["code"], "INVALID_INPUT");
            assert!(!data.to_string().contains("UNTRUSTED_GIT_SENTINEL"));
        }
    }
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "fixture source"
    );
    assert_eq!(s.project_history("1", 0, 200).unwrap().data, history);
}

#[tokio::test]
async fn registered_linked_checkout_is_allowed_without_a_default_worktree() {
    let (temp, _s, app) = fixture();
    let repo = temp.path().join("repo");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    };
    git(&["init", "-b", "main"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "--allow-empty",
        "-m",
        "fixture",
    ]);
    git(&["worktree", "add", "--detach", linked.to_str().unwrap()]);
    command(&app, "project-create", json!({"name":"linked"})).await;
    command(&app, "project-source-add", json!({"projectId":1,"expectedRevision":1,"location":{"kind":"git","worktree":repo,"relativePath":"."}})).await;
    let encoded: String = advertised_checkout(&repo, &linked)
        .bytes()
        .map(|b| format!("%{b:02X}"))
        .collect();
    let (status, data) = request(
        &app,
        &format!("/api/projects/1/context?sourceId=1&worktree={encoded}"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{data}");
    assert!(
        data["data"]["resolvedPath"]
            .as_str()
            .unwrap()
            .ends_with("linked")
    );
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
