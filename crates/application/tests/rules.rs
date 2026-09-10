use serde_json::{Value, json};
use steward_application::{RuleInput, Service};
fn input(scope: &str, project: Option<i64>, status: &str) -> RuleInput {
    serde_json::from_value(json!({"scope":scope,"projectId":project,"status":status,"contentVersion":1,"content":{"name":"输出偏好","body":"请用自由 Markdown\n- 完整保留内容","sources":[]}})).unwrap()
}
fn rules(s: &Service, task: &str) -> Value {
    s.task_context(task).unwrap().data["sessionRules"]["rules"].clone()
}
fn snapshot(s: &Service) -> Value {
    json!({"task":s.task_show("1").unwrap().data,"project":s.project_list(0,200).unwrap().data,"history":s.history("1").unwrap().data,"sessions":s.session_list(None).unwrap().data})
}
#[test]
fn feedback_a_to_context_b_isolated_scopes_revisions_and_no_execution_side_effects() {
    let t = tempfile::tempdir().unwrap();
    let s = Service::new(t.path().join("db"));
    s.project_create("One").unwrap();
    s.project_create("Two").unwrap();
    s.task_create_in_project(None, None, Some("1")).unwrap();
    s.task_create_in_project(None, None, Some("2")).unwrap();
    s.task_create_minimal().unwrap();
    s.task_note("1", 1, "progress", "长期反馈").unwrap();
    let before = snapshot(&s);
    s.rule_create(
        input("global", None, "active"),
        "明确长期偏好，无需任务来源",
    )
    .unwrap();
    let mut inferred = input("project", Some(1), "candidate");
    inferred.content.sources = serde_json::from_value(json!([
        {"kind":"inferred","evidence":"任务A的历史反馈","taskId":1,"taskVersion":1},
        {"kind":"explicit","evidence":"另一项目任务的反馈","taskId":2,"taskVersion":1}
    ]))
    .unwrap();
    s.rule_create(inferred.clone(), "跨任务归纳，歧义先候选")
        .unwrap();
    s.rule_create(input("project", Some(2), "active"), "仅项目二")
        .unwrap();
    assert_eq!(rules(&s, "1").as_array().unwrap().len(), 1);
    assert_eq!(rules(&s, "2").as_array().unwrap().len(), 2);
    assert_eq!(rules(&s, "3").as_array().unwrap().len(), 1);
    inferred.status = "active".into();
    s.rule_update(2, 1, inferred.clone(), "已消除歧义").unwrap();
    assert_eq!(rules(&s, "1")[1]["revision"], 2);
    inferred.content.body = "修正后的规则".into();
    s.rule_update(2, 2, inferred, "修正规则").unwrap();
    assert_eq!(
        rules(&Service::new(s.database_path()), "1")[1]["content"]["body"],
        "修正后的规则"
    );
    assert_eq!(
        s.project_show("1").unwrap().data["sessionRules"]["rules"],
        rules(&s, "1")
    );
    assert_eq!(snapshot(&s), before);
    s.task_set_project("1", 2, Some("2"), true, "合成任务换项目")
        .unwrap();
    assert_eq!(rules(&s, "1")[1]["id"], 3);
    s.rule_disable(1, 1, "撤回通用规则").unwrap();
    assert_eq!(rules(&s, "3"), json!([]));
    let h = s.rule_history(2).unwrap().data;
    assert_eq!(h["history"].as_array().unwrap().len(), 3);
    assert!(h["history"][0]["before"].is_null());
    assert_eq!(h["history"][2]["before"]["revision"], 2);
    assert_eq!(
        h["history"][2]["after"]["content"]["sources"][0]["taskVersion"],
        1
    );
}
#[test]
fn cas_competition_and_history_failure_are_atomic() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let s = Service::new(&path);
    s.rule_create(input("global", None, "active"), "create")
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|i| {
            let barrier = barrier.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                let mut value = input("global", None, "active");
                value.content.body = format!("writer {i}");
                barrier.wait();
                Service::new(path).rule_update(1, 1, value, "compete")
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    let before = s.rule_history(1).unwrap().data;
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TRIGGER fail_rule_history BEFORE INSERT ON rule_history BEGIN SELECT RAISE(ABORT,'injected history failure'); END;").unwrap();
    assert!(s.rule_disable(1, 2, "disable").is_err());
    assert!(
        s.rule_create(input("global", None, "active"), "create")
            .is_err()
    );
    assert_eq!(s.rule_history(1).unwrap().data, before);
    assert_eq!(
        s.rule_list(None, None, None).unwrap().data["rules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn invalid_input_and_corrupt_context_are_not_empty_successes() {
    let t = tempfile::tempdir().unwrap();
    let s = Service::new(t.path().join("db"));
    s.task_create_minimal().unwrap();
    let good = input("global", None, "active");
    let mut invalid = vec![];
    let mut v = good.clone();
    v.content_version = 2;
    invalid.push(v);
    let mut v = good.clone();
    v.project_id = Some(1);
    invalid.push(v);
    let mut v = good.clone();
    v.scope = "project".into();
    invalid.push(v);
    let mut v = good.clone();
    v.status = "unknown".into();
    invalid.push(v);
    let mut v = good.clone();
    v.content.body = "x".repeat(48001);
    invalid.push(v);
    let mut v = good.clone();
    v.content.name = "\0".into();
    invalid.push(v);
    let mut v = good.clone();
    v.content.sources = serde_json::from_value(
        json!([{"kind":"explicit","evidence":"e","taskId":1,"taskVersion":2}]),
    )
    .unwrap();
    invalid.push(v);
    let mut v = good.clone();
    v.content.sources = serde_json::from_value(
        json!([{"kind":"unknown","evidence":"e","taskId":null,"taskVersion":null}]),
    )
    .unwrap();
    invalid.push(v);
    let mut v = good.clone();
    v.content.sources = serde_json::from_value(
        json!([{"kind":"explicit","evidence":"x".repeat(3999),"taskId":null,"taskVersion":null}]),
    )
    .unwrap();
    v.content.sources = vec![v.content.sources[0].clone(); 33];
    invalid.push(v);
    for value in invalid {
        assert!(s.rule_create(value, "invalid").is_err());
    }
    let mut raw = json!(good);
    raw["unknown"] = json!(true);
    assert!(serde_json::from_value::<RuleInput>(raw).is_err());
    assert_eq!(
        s.rule_list(None, None, None).unwrap().data["rules"],
        json!([])
    );
    let mut oversize = good.clone();
    oversize.content.body = "x".repeat(48000);
    oversize.content.sources = (0..5)
        .map(|i| steward_application::RuleSource {
            kind: "explicit".into(),
            evidence: format!("{i}{}", "x".repeat(3998)),
            task_id: None,
            task_version: None,
        })
        .collect();
    assert!(s.rule_create(oversize, "total JSON byte limit").is_err());
    let mut full = good.clone();
    full.content.body = format!("{}END", "x".repeat(47997));
    s.rule_create(full.clone(), "valid").unwrap();
    let before = snapshot(&s);
    let c = rusqlite::Connection::open(s.database_path()).unwrap();
    let data_version = || {
        c.pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))
            .unwrap()
    };
    let initial_data_version = data_version();
    for _ in 0..3 {
        assert_eq!(rules(&s, "1")[0]["content"]["body"], full.content.body);
    }
    assert_eq!(data_version(), initial_data_version);
    assert_eq!(snapshot(&s), before);
    // Storage permits future formats without a schema migration; this build still refuses them.
    for invalid in ["0", "-1", "1.5", "'future'"] {
        assert!(
            c.execute_batch(&format!("UPDATE rules SET content_version={invalid}"))
                .is_err()
        );
    }
    c.execute_batch("UPDATE rules SET content_version=2;")
        .unwrap();
    let before_unknown = snapshot(&s);
    let rules_before: String = c
        .query_row("SELECT content_json FROM rules WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        s.task_context("1").unwrap_err().body.code,
        "DATABASE_UNAVAILABLE"
    );
    let mut future = full.clone();
    future.content_version = 2;
    assert!(s.rule_create(future.clone(), "unsupported").is_err());
    assert!(s.rule_update(1, 1, future, "unsupported").is_err());
    assert_eq!(snapshot(&s), before_unknown);
    assert_eq!(
        c.query_row("SELECT content_json FROM rules WHERE id=1", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        rules_before
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM rule_history", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    c.execute_batch("UPDATE rules SET content_version=1,content_json='{}';")
        .unwrap();
    assert_eq!(
        s.task_context("1").unwrap_err().body.code,
        "DATABASE_UNAVAILABLE"
    );
    s.task_claim("1", 1, "old-session", false).unwrap();
    let before_resume = snapshot(&s);
    assert_eq!(
        s.task_resume("1", 2, "new-session", None, true)
            .unwrap_err()
            .body
            .code,
        "DATABASE_UNAVAILABLE"
    );
    assert_eq!(snapshot(&s), before_resume);
}
