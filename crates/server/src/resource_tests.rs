use super::*;

#[tokio::test]
async fn exhausted_read_capacity_preserves_write_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let state = ServerState::new(
        Service::new(temp.path().join("state.db")),
        43123,
        "synthetic".into(),
    );
    let reads = state
        .read_slots
        .clone()
        .acquire_many_owned(6)
        .await
        .unwrap();
    assert_eq!(
        run_read(state.clone(), |_| Ok(Outcome::new(json!({}))))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(state.slots.available_permits(), 8);
    assert_eq!(
        run(state.clone(), |_| Ok(Outcome::new(json!({}))))
            .await
            .status(),
        StatusCode::OK
    );
    drop(reads);
    assert_eq!(
        run_read(state.clone(), |_| Ok(Outcome::new(json!({}))))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(state.read_slots.available_permits(), 6);
}

#[tokio::test(flavor = "current_thread")]
async fn auth_work_does_not_block_runtime_and_retains_capacity_after_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    let state = ServerState::new(
        Service::new(temp.path().join("state.db")),
        43124,
        "synthetic".into(),
    );
    let started = Arc::new(tokio::sync::Notify::new());
    let (release, wait) = std::sync::mpsc::channel();
    let worker_state = state.clone();
    let signal = started.clone();
    let task = tokio::spawn(async move {
        auth_job(&worker_state, move || {
            signal.notify_one();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(1), started.notified())
        .await
        .unwrap();
    // This timer cannot execute if the blocking job runs on this sole async thread.
    tokio::time::timeout(
        Duration::from_millis(100),
        tokio::time::sleep(Duration::from_millis(1)),
    )
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(state.auth_slots.available_permits(), 3);
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while state.auth_slots.available_permits() != 4 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let permits = state
        .auth_slots
        .clone()
        .acquire_many_owned(4)
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "cookie",
        HeaderValue::from_str(&format!("{}={}", state.cookie_name, "a".repeat(64))).unwrap(),
    );
    assert_eq!(
        state.role_async(headers).await.unwrap_err().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Token authentication is pure computation, independent of SQLite capacity.
    let mut headers = HeaderMap::new();
    headers.insert("x-steward-token", HeaderValue::from_static("synthetic"));
    assert_eq!(state.role_async(headers).await.unwrap(), Some("admin"));
    drop(permits);
}

#[test]
fn http_device_guards_do_not_change_unc_filesystem_policy() {
    assert!(absolute("/path\0invalid").is_err());
    #[cfg(windows)]
    {
        for path in [
            r"\\.\NUL",
            r"\\.\pipe\synthetic",
            r"\\localhost\pipe\synthetic",
            r"\\?\pipe\synthetic",
            r"\\?\GLOBALROOT\Device\NamedPipe\synthetic",
        ] {
            assert!(absolute(path).is_err());
        }
        // Lexical check only: no network lookup or filesystem call.
        assert!(absolute(r"\\example.invalid\share\repository").is_ok());
        assert!(absolute(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\repository").is_ok());
    }
}
