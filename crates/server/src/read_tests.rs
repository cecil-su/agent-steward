use super::*;

#[tokio::test]
async fn cancelled_read_retains_both_limits_until_worker_exits() {
    let temp = tempfile::tempdir().unwrap();
    let state = ServerState::new(
        Service::new(temp.path().join("unused.db")),
        43123,
        "synthetic".into(),
    );
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finish, finished) = std::sync::mpsc::channel();
    let request = tokio::spawn(run_read(state.clone(), move |_| {
        let _ = started.send(());
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        Err(AppError::invalid("test", "worker finished"))
    }));
    ready.await.unwrap();
    assert_eq!(state.slots.available_permits(), 7);
    assert_eq!(state.read_slots.available_permits(), 5);
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert_eq!(state.slots.available_permits(), 7);
    assert_eq!(state.read_slots.available_permits(), 5);
    finish.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.slots.available_permits() != 8 || state.read_slots.available_permits() != 6 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!temp.path().join("unused.db").exists());
}

#[tokio::test]
async fn read_capacity_is_bounded_without_touching_storage() {
    let temp = tempfile::tempdir().unwrap();
    let state = ServerState::new(
        Service::new(temp.path().join("unused.db")),
        43123,
        "synthetic".into(),
    );
    let _permits = state
        .read_slots
        .clone()
        .acquire_many_owned(6)
        .await
        .unwrap();
    let response = run_read(state, |_| panic!("busy reader must not execute")).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!temp.path().join("unused.db").exists());
}
