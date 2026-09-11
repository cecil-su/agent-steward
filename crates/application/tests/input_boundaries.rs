use steward_application::{Service, TaskListOptions};

#[test]
fn invalid_page_size_is_rejected_before_database_connection_for_all_callers() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("isolated.db");
    let service = Service::new(&database);
    for size in [0, 201, u32::MAX] {
        let error = service
            .task_list_with_options(&TaskListOptions {
                page_size: Some(size),
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(error.body.code, "INVALID_INPUT");
        assert_eq!(error.body.details["field"], "pageSize");
        assert!(!database.exists());
    }
    std::fs::write(&database, b"not a sqlite database").unwrap();
    let error = service
        .task_list_with_options(&TaskListOptions {
            page_size: Some(0),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(error.body.code, "INVALID_INPUT");
    assert_eq!(std::fs::read(database).unwrap(), b"not a sqlite database");
}
