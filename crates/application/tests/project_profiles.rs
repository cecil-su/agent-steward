use serde_json::{Value, json};
use steward_application::{ProjectProfileInput, ProjectProfileView, Service};

fn fixture() -> (tempfile::TempDir, Service) {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::new(temp.path().join("profiles.db"));
    service.project_create("Example").unwrap();
    service
        .task_create_in_project(None, None, Some("##1"))
        .unwrap();
    (temp, service)
}

fn input() -> ProjectProfileInput {
    ProjectProfileInput {
        summary: "  项目概要  ".into(),
        architecture: "\n架构说明\n".into(),
        development: " 开发说明 ".into(),
        source_task_id: 1,
        source_task_version: 1,
        evidence: " 来源任务中的公开结论 ".into(),
    }
}

fn snapshot(s: &Service) -> Value {
    json!({
        "profile": s.project_profile_show("##1").unwrap().data,
        "history": s.project_history("##1", 0, 200).unwrap().data,
        "task": s.task_show("#1").unwrap().data,
        "taskHistory": s.history("#1").unwrap().data,
        "sessions": s.session_list(None).unwrap().data,
    })
}

#[test]
fn profiles_create_read_replace_and_preserve_source_task() {
    let (_temp, s) = fixture();
    let initial = snapshot(&s);
    assert!(initial["profile"]["profile"].is_null());
    let first = s.project_profile_set("Example", 1, input()).unwrap().data;
    assert_eq!(first["project"]["revision"], 2);
    assert_eq!(first["profile"]["revision"], 2);
    assert_eq!(first["profile"]["projectId"], 1);
    assert_eq!(first["profile"]["summary"], "项目概要");
    assert_eq!(first["profile"]["architecture"], "架构说明");
    assert_eq!(first["profile"]["development"], "开发说明");
    assert_eq!(first["profile"]["evidence"], "来源任务中的公开结论");
    assert!(!first["profile"]["updatedAt"].as_str().unwrap().is_empty());
    let _: ProjectProfileView = serde_json::from_value(first["profile"].clone()).unwrap();
    for reference in ["Example", "##1", "1"] {
        assert_eq!(s.project_profile_show(reference).unwrap().data, first);
    }
    // Profile revision follows project CAS, not an independent profile counter.
    s.project_rename("##1", 2, "Renamed").unwrap();
    let mut replacement = input();
    replacement.summary = "新概要".into();
    replacement.architecture = "新架构".into();
    replacement.development = "新开发说明".into();
    replacement.evidence = "新证据".into();
    let second = s
        .project_profile_set("Renamed", 3, replacement)
        .unwrap()
        .data;
    assert_eq!(second["project"]["revision"], 4);
    assert_eq!(second["profile"]["revision"], 4);
    assert_eq!(second["profile"]["summary"], "新概要");
    assert_eq!(second["profile"]["architecture"], "新架构");
    assert_eq!(second["profile"]["development"], "新开发说明");
    assert_eq!(s.project_profile_show("##1").unwrap().data, second);
    let history = s.project_history("##1", 0, 200).unwrap().data;
    assert_eq!(
        history["history"][1]["changeType"],
        "project.profile_updated"
    );
    assert_eq!(
        history["history"][1]["payload"],
        json!({
            "before": null, "after": first["profile"], "sourceTaskId": 1,
            "sourceTaskVersion": 1, "evidence": "来源任务中的公开结论",
        })
    );
    assert_eq!(
        history["history"][3]["payload"],
        json!({
            "before": first["profile"], "after": second["profile"], "sourceTaskId": 1,
            "sourceTaskVersion": 1, "evidence": "新证据",
        })
    );
    let after = snapshot(&s);
    for key in ["task", "taskHistory", "sessions"] {
        assert_eq!(initial[key], after[key]);
    }
}

#[test]
fn cas_and_source_validation_fail_without_side_effects() {
    let (_temp, s) = fixture();
    s.project_create("Other").unwrap();
    s.task_create_in_project(None, None, Some("##2")).unwrap();
    s.task_create_minimal().unwrap();
    s.project_profile_set("##1", 1, input()).unwrap();
    let before = snapshot(&s);
    assert_eq!(
        s.project_profile_set("##1", 1, input())
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    for (id, code) in [
        (999, "NOT_FOUND"),
        (2, "INVALID_INPUT"),
        (3, "INVALID_INPUT"),
    ] {
        let mut value = input();
        value.source_task_id = id;
        assert_eq!(
            s.project_profile_set("##1", 2, value)
                .unwrap_err()
                .body
                .code,
            code
        );
        assert_eq!(snapshot(&s), before);
    }
    // Advance the source through an ordinary test-fixture API, not profile mutation.
    s.task_note("#1", 1, "progress", "new evidence").unwrap();
    let before = snapshot(&s);
    let error = s.project_profile_set("##1", 2, input()).unwrap_err();
    assert_eq!(error.body.code, "VERSION_CONFLICT");
    assert_eq!(error.body.details["currentVersion"], 2);
    assert_eq!(snapshot(&s), before);
    let mut value = input();
    value.source_task_version = 2;
    assert_eq!(
        s.project_profile_set("##1", 2, value).unwrap().data["profile"]["sourceTaskVersion"],
        2
    );
    s.task_set_project("#1", 2, Some("##2"), true, "fixture")
        .unwrap();
    let before = snapshot(&s);
    let mut moved = input();
    moved.source_task_version = 3;
    assert_eq!(
        s.project_profile_set("##1", 3, moved)
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    assert_eq!(snapshot(&s), before);
}

#[test]
fn input_requires_all_fields_and_rejects_unknown_or_invalid_values() {
    let (_temp, s) = fixture();
    let valid = serde_json::to_value(input()).unwrap();
    for field in [
        "summary",
        "architecture",
        "development",
        "evidence",
        "sourceTaskId",
        "sourceTaskVersion",
    ] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ProjectProfileInput>(missing).is_err());
        let mut null = valid.clone();
        null[field] = Value::Null;
        assert!(serde_json::from_value::<ProjectProfileInput>(null).is_err());
    }
    let mut unknown = valid.clone();
    unknown["patch"] = json!(true);
    assert!(serde_json::from_value::<ProjectProfileInput>(unknown).is_err());
    let before = snapshot(&s);
    for (field, limit) in [
        ("summary", 4000),
        ("evidence", 4000),
        ("architecture", 8000),
        ("development", 8000),
    ] {
        for text in [
            String::new(),
            " \n\t\u{3000}".into(),
            "\0abc".into(),
            "abc\0def".into(),
            "界".repeat(limit + 1),
        ] {
            let mut value = valid.clone();
            value[field] = json!(text);
            let error = s
                .project_profile_set("##1", 1, serde_json::from_value(value).unwrap())
                .unwrap_err();
            assert_eq!(error.body.code, "INVALID_INPUT");
            assert_eq!(error.body.details["field"], field);
            assert_eq!(snapshot(&s), before);
        }
    }
    for field in ["sourceTaskId", "sourceTaskVersion"] {
        for number in [0, -1] {
            let mut value = valid.clone();
            value[field] = json!(number);
            assert_eq!(
                s.project_profile_set("##1", 1, serde_json::from_value(value).unwrap())
                    .unwrap_err()
                    .body
                    .code,
                "INVALID_INPUT"
            );
            assert_eq!(snapshot(&s), before);
        }
    }
    assert_eq!(
        s.project_profile_show("##999").unwrap_err().body.code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_profile_set("##999", 1, input())
            .unwrap_err()
            .body
            .code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_profile_show("#1").unwrap_err().body.code,
        "INVALID_INPUT"
    );
    assert_eq!(snapshot(&s), before);
    let mut value = input();
    value.summary = format!(" {} ", "界".repeat(4000));
    value.evidence = "证".repeat(4000);
    value.architecture = "构".repeat(8000);
    value.development = "🦀".repeat(8000);
    let profile = s.project_profile_set("##1", 1, value).unwrap().data;
    assert_eq!(
        profile["profile"]["summary"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        4000
    );
    assert_eq!(
        profile["profile"]["development"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        8000
    );
}

#[test]
fn history_failure_rolls_back_both_profile_creation_and_replacement() {
    let (temp, s) = fixture();
    let connection = storage_sqlite::open_database(&temp.path().join("profiles.db")).unwrap();
    for revision in [1, 2] {
        let before = snapshot(&s);
        connection.execute_batch("CREATE TRIGGER fail_profile_history BEFORE INSERT ON project_history WHEN NEW.change_type='project.profile_updated' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        let mut value = input();
        value.summary = "must not persist".into();
        assert!(s.project_profile_set("##1", revision, value).is_err());
        assert_eq!(snapshot(&s), before);
        connection
            .execute_batch("DROP TRIGGER fail_profile_history;")
            .unwrap();
        if revision == 1 {
            s.project_profile_set("##1", 1, input()).unwrap();
        }
    }
}
