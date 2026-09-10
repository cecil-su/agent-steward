use serde_json::{Value, json};
use steward_application::{Service, TaskListOptions};

#[test]
fn numeric_queries_match_exact_ids_and_keep_status_project_and_text_filters() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("search.db"));
    service.project_create("One").unwrap();
    service.project_create("Two").unwrap();
    service
        .task_create("FIRST", r#"{"goal":"alpha","project":"1"}"#)
        .unwrap();
    service
        .task_create("SECOND", r#"{"goal":"1 and 100% literal","project":"2"}"#)
        .unwrap();
    let search = |query: &str, status: Option<&str>, project: Option<&str>| {
        service.task_list_with_options(&TaskListOptions {
            query: Some(query.to_owned()),
            status: status.map(str::to_owned),
            project: project.map(str::to_owned),
            ..TaskListOptions::default()
        })
    };
    let ids = |value: Value| {
        value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].clone())
            .collect::<Vec<_>>()
    };
    for query in ["1", "#1", "01", " #1 "] {
        assert_eq!(ids(search(query, None, None).unwrap().data), vec![json!(1)]);
    }
    assert!(ids(search("#99", None, None).unwrap().data).is_empty());
    assert!(ids(search("1", None, Some("2")).unwrap().data).is_empty());
    assert_eq!(
        ids(search("1", None, Some("1")).unwrap().data),
        vec![json!(1)]
    );
    service.task_claim("1", 1, "search-session", false).unwrap();
    assert!(ids(search("#1", Some("open"), None).unwrap().data).is_empty());
    assert_eq!(
        ids(search("#1", Some("in_progress"), None).unwrap().data),
        vec![json!(1)]
    );
    assert_eq!(ids(search("%", None, None).unwrap().data), vec![json!(2)]);
    assert_eq!(
        ids(search("alpha", None, None).unwrap().data),
        vec![json!(1)]
    );
    assert!(search("#999999999999999999999999", None, None).is_err());
}
