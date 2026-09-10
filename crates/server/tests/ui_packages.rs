use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use steward_application::Service;
use steward_server::{ServerState, router};
use tower::ServiceExt;
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn package(root: &Path, version: &str, contract: u32) -> String {
    let html = format!(
        "<html><head><link href=\"/style.css\"></head><body>{version}<script src=\"/app.js\"></script></body></html>"
    );
    let js = format!("// {version}");
    let css = format!("/* {version} */");
    let files = [
        ("index.html", html.as_str()),
        ("app.js", js.as_str()),
        ("style.css", css.as_str()),
    ];
    let manifest = json!({"packageFormat":1,"uiVersion":version,"requiredApiContract":contract,"entry":"index.html","files":files.iter().map(|(name,text)|(name.to_string(),Value::String(hash(text.as_bytes())))).collect::<serde_json::Map<_,_>>()}).to_string();
    let id = hash(manifest.as_bytes());
    let dir = root.join("releases").join(&id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.json"), manifest).unwrap();
    for (name, text) in files {
        fs::write(dir.join(name), text).unwrap();
    }
    id
}
fn activate(root: &Path, id: &str) {
    fs::write(root.join("current.json"), json!({"release":id}).to_string()).unwrap();
}
fn fixture(root: &Path) -> Router {
    router(
        ServerState::new(
            Service::new(root.join("isolated.db")),
            43123,
            "synthetic".into(),
        )
        .with_ui_root(root.into()),
    )
}
async fn get(app: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("host", "127.0.0.1:43123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let text = String::from_utf8(
        to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, headers, text)
}
async fn status(app: &Router) -> Value {
    serde_json::from_str(&get(app, "/ui/status").await.2).unwrap()
}
#[tokio::test]
async fn switches_whole_releases_and_rolls_back_without_rebuilding_router() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let app = fixture(root);
    assert_eq!(status(&app).await["apiContract"], 3);
    let embedded = status(&app).await["release"].as_str().unwrap().to_owned();
    let a = package(root, "a", 3);
    let b = package(root, "b", 3);
    activate(root, &a);
    let (_, headers, html) = get(&app, "/").await;
    assert_eq!(headers["cache-control"], "no-store");
    assert!(html.contains(&format!("/ui/releases/{a}/app.js")));
    activate(root, &b);
    assert_eq!(status(&app).await["release"], b);
    for id in [&a, &b, &embedded] {
        let (code, headers, _) = get(&app, &format!("/ui/releases/{id}/app.js")).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(
            headers["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(headers["x-content-type-options"], "nosniff");
    }
    assert_eq!(
        get(&app, &format!("/ui/releases/{a}/app.js")).await.2,
        "// a"
    );
    assert_eq!(
        get(&app, &format!("/ui/releases/{b}/app.js")).await.2,
        "// b"
    );
    activate(root, &a);
    assert_eq!(status(&app).await["release"], a);
    activate(root, "embedded");
    assert_eq!(status(&app).await["release"], embedded);
    assert_eq!(get(&app, "/app.js").await.1["cache-control"], "no-store");
}
#[tokio::test]
async fn refuses_bad_packages_and_keeps_last_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let app = fixture(root);
    let a = package(root, "good", 3);
    activate(root, &a);
    assert_eq!(status(&app).await["release"], a);
    let incompatible = package(root, "old-contract", 2);
    activate(root, &incompatible);
    let s = status(&app).await;
    assert_eq!(s["release"], a);
    assert_eq!(s["error"], "UI_API_INCOMPATIBLE");
    let restarted = fixture(root);
    assert_eq!(status(&restarted).await["uiVersion"], "embedded");
    assert_eq!(status(&restarted).await["error"], "UI_API_INCOMPATIBLE");
    let corrupt = package(root, "corrupt", 3);
    fs::write(
        root.join("releases").join(&corrupt).join("app.js"),
        "modified",
    )
    .unwrap();
    activate(root, &corrupt);
    assert_eq!(status(&app).await["error"], "UI_PACKAGE_HASH_MISMATCH");
    assert_eq!(
        get(&app, &format!("/ui/releases/{corrupt}/app.js")).await.0,
        StatusCode::NOT_FOUND
    );
    fs::write(root.join("current.json"), "{").unwrap();
    assert_eq!(status(&app).await["release"], a);
    // A deleted active directory cannot tear a loaded in-memory release.
    fs::remove_dir_all(root.join("releases").join(&a)).unwrap();
    assert_eq!(
        get(&app, &format!("/ui/releases/{a}/app.js")).await.2,
        "// good"
    );
    let restarted = fixture(root);
    assert_eq!(status(&restarted).await["uiVersion"], "embedded");
}
#[tokio::test]
async fn checks_paths_manifest_identity_size_and_security_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let app = fixture(root);
    for id in ["../secret", "C:\\secret", "", "A".repeat(64).as_str()] {
        activate(root, id);
        assert!(status(&app).await["error"].is_string());
    }
    let a = package(root, "a", 3);
    fs::write(root.join("releases").join(&a).join("manifest.json"), "{}").unwrap();
    activate(root, &a);
    assert_eq!(status(&app).await["error"], "UI_PACKAGE_HASH_MISMATCH");
    let big = package(root, "big", 3);
    fs::write(
        root.join("releases").join(&big).join("app.js"),
        vec![b'x'; 4 * 1024 * 1024 + 1],
    )
    .unwrap();
    activate(root, &big);
    assert_eq!(status(&app).await["error"], "UI_PACKAGE_TOO_LARGE");
    for path in ["/ui/status", "/", "/ui/releases/bad/app.js"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "evil.test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    assert_eq!(
        get(&app, "/ui/releases/bad/manifest.json").await.0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn concurrent_switches_never_mix_an_entry_and_its_resources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let app = fixture(&root);
    let a = package(&root, "a", 3);
    let b = package(&root, "b", 3);
    activate(&root, &a);
    assert_eq!(status(&app).await["release"], a);
    let writer_app = app.clone();
    let (writer_a, writer_b) = (a.clone(), b.clone());
    let writer = tokio::spawn(async move {
        for i in 0..30 {
            activate(&root, if i % 2 == 0 { &writer_b } else { &writer_a });
            let _ = status(&writer_app).await;
        }
    });
    for _ in 0..30 {
        let (code, _, html) = get(&app, "/").await;
        assert_eq!(code, StatusCode::OK);
        let (id, label) = if html.contains("<body>a") {
            (&a, "a")
        } else {
            (&b, "b")
        };
        assert!(html.contains(&format!("content=\"{id}\"")));
        assert!(html.contains(&format!("/ui/releases/{id}/style.css")));
        assert!(html.contains(&format!("/ui/releases/{id}/app.js")));
        assert_eq!(
            get(&app, &format!("/ui/releases/{id}/app.js")).await.2,
            format!("// {label}")
        );
    }
    writer.await.unwrap();
}

#[tokio::test]
async fn stale_ui_contract_cannot_execute_a_write() {
    let temp = tempfile::tempdir().unwrap();
    let app = fixture(temp.path());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/commands/task-create")
                .header("host", "127.0.0.1:43123")
                .header("x-steward-token", "synthetic")
                .header("x-steward-ui-contract", "2")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"input":{}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("UI_API_INCOMPATIBLE"));
    assert!(!temp.path().join("isolated.db").exists());
}
