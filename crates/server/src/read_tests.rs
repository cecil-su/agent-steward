use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn cancelled_read_worker_releases_its_slot() {
    let temp = tempfile::tempdir().unwrap();
    let state = ServerState::new(
        Service::new(temp.path().join("test.db")),
        43123,
        "synthetic".into(),
    );
    let (started, ready) = tokio::sync::oneshot::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = cancelled.clone();
    let request = tokio::spawn(run_read(state.clone(), move |_| {
        let _ = started.send(());
        let fallback = Instant::now() + Duration::from_secs(2);
        while Instant::now() < fallback {
            if let Err(error) = steward_application::GitReadControl::check_current() {
                observed.store(true, Ordering::Relaxed);
                return Err(AppError::from_git(error, None));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err(AppError::invalid("test", "cancellation was not observed"))
    }));
    ready.await.unwrap();
    assert_eq!(state.slots.available_permits(), 7);
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    wait_for_slots(&state).await;
    assert!(cancelled.load(Ordering::Relaxed));
}

async fn wait_for_slots(state: &ServerState) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.slots.available_permits() != 8 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("cancelled Git reader must release its slot");
}

// Invoked only by the synthetic repository's clean filter, not a server test hook.
#[test]
fn filter_child() {
    let Some(path) = std::env::var_os("STEWARD_READ_TEST_READY") else {
        return;
    };
    std::fs::write(path, std::process::id().to_string()).unwrap();
    std::thread::sleep(Duration::from_secs(30));
}

#[tokio::test]
async fn task_read_routes_cancel_real_git_and_release_shared_slots() {
    use axum::body::Body;
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "synthetic Git setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    std::fs::write(repo.join("sample"), "before\n").unwrap();
    git(&["add", "sample"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=test@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "fixture",
    ]);
    let service = Service::new(temp.path().join("test.db"));
    service.task_create_minimal().unwrap();
    service.worktree_adopt("1", 1, &repo, &repo).unwrap();
    let before = service.task_show("1").unwrap().data;
    let history = service.history("1").unwrap().data;
    let ready = temp.path().join("filter-ready");
    let shell_path = |p: &std::path::Path| {
        let text = p.to_str().unwrap();
        #[cfg(windows)]
        let text = text.replace('\\', "/");
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    };
    let command = format!(
        "STEWARD_READ_TEST_READY={} {} --exact read_tests::filter_child --nocapture",
        shell_path(&ready),
        shell_path(&std::env::current_exe().unwrap())
    );
    // Only this temporary repository is configured. Do not mutate process-wide PATH/env.
    git(&["config", "--local", "filter.read-test.clean", &command]);
    std::fs::write(repo.join(".gitattributes"), "sample filter=read-test\n").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(repo.join("sample"), "edited\n").unwrap(); // same size, forces content comparison
    let state = ServerState::new(service.clone(), 43123, "synthetic-admin".into())
        .with_readonly_token("synthetic-reader".into());
    let app = router(state.clone());
    for endpoint in [
        "/api/tasks/1/context",
        "/api/tasks/1/worktree-status",
        "/api/doctor",
    ] {
        if ready.exists() {
            std::fs::remove_file(&ready).unwrap();
        }
        let req = axum::http::Request::builder()
            .uri(endpoint)
            .header("host", "127.0.0.1:43123")
            .header("x-steward-token", "synthetic-reader")
            .body(Body::empty())
            .unwrap();
        let request = tokio::spawn(app.clone().oneshot(req));
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready.exists() {
                assert!(
                    !request.is_finished(),
                    "{endpoint} must reach the synthetic slow Git filter"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Git filter must actually start before cancellation");
        assert_eq!(state.slots.available_permits(), 7);
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        wait_for_slots(&state).await;
        assert_eq!(service.task_show("1").unwrap().data, before);
        assert_eq!(service.history("1").unwrap().data, history);
        assert_eq!(
            std::fs::read_to_string(repo.join("sample")).unwrap(),
            "edited\n"
        );
    }
}
