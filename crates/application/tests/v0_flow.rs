use std::fs;

use serde_json::json;
use sha2::{Digest, Sha256};
use steward_application::{Service, TaskListOptions};

fn service(temp: &tempfile::TempDir) -> Service {
    Service::new(temp.path().join("steward.db"))
}

fn version(outcome: &steward_application::Outcome) -> i64 {
    outcome.data["task"]["version"].as_i64().unwrap()
}

fn task_id(service: &Service, reference: &str) -> i64 {
    service.task_show(reference).unwrap().data["task"]["id"]
        .as_i64()
        .unwrap()
}

#[test]
fn task_session_checkpoint_import_and_history_flow() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    let created = service
        .task_create(
            "TASK-1",
            r#"{
                "title":"0904｜功能｜Implement V0",
                "goal":"Provide a local task continuity CLI",
                "scope":"V0 workspace",
                "acceptanceCriteria":"Integration flow passes",
                "nextStep":"Claim the task"
            }"#,
        )
        .unwrap();
    assert_eq!(version(&created), 1);

    let claimed = service.task_claim("TASK-1", 1, "session-a", false).unwrap();
    assert_eq!(claimed.data["task"]["status"], "todo");
    assert_eq!(version(&claimed), 2);
    let no_op = service.task_claim("TASK-1", 2, "session-a", false).unwrap();
    assert_eq!(version(&no_op), 2);
    let stale = service
        .task_update("TASK-1", 1, r#"{"nextStep":"stale"}"#, true, "fixture")
        .unwrap_err();
    assert_eq!(stale.body.code, "VERSION_CONFLICT");

    let updated = service
        .task_update(
            "TASK-1",
            2,
            r#"{"nextStep":"Write tests"}"#,
            true,
            "fixture",
        )
        .unwrap();
    assert_eq!(version(&updated), 3);
    let noted = service
        .task_note("TASK-1", 3, "decision", "Use SQLite WAL")
        .unwrap();
    assert_eq!(version(&noted), 4);
    let blocked = service.task_status("TASK-1", 4, "blocked").unwrap();
    assert_eq!(blocked.data["task"]["status"], "blocked");
    let unblocked = service.task_status("TASK-1", 5, "in_progress").unwrap();
    assert_eq!(unblocked.data["task"]["status"], "in_progress");
    let checkpoint = service
        .task_checkpoint(
            "TASK-1",
            6,
            "session-a",
            r#"{
                "summary":"Core flow works",
                "completed":["Task mutations"],
                "decisions":["SQLite WAL"],
                "pending":["Resume"],
                "nextStep":"Resume in session B",
                "risks":[]
            }"#,
        )
        .unwrap();
    assert_eq!(version(&checkpoint), 7);
    let history_before = service.history("TASK-1").unwrap().data;
    let sessions_before = service.session_list(Some("TASK-1")).unwrap().data;
    let context = service.task_context("TASK-1").unwrap();
    assert_eq!(context.data["task"], checkpoint.data["task"]);
    assert_eq!(context.data["checkpoint"]["summary"], "Core flow works");
    assert_eq!(context.data["session"]["id"], "session-a");
    assert_eq!(service.history("TASK-1").unwrap().data, history_before);
    assert_eq!(
        service.session_list(Some("TASK-1")).unwrap().data,
        sessions_before
    );
    let resumed = service
        .task_resume("TASK-1", 7, "session-b", Some("session-a"), true)
        .unwrap();
    assert_eq!(version(&resumed), 8);
    assert_eq!(
        resumed.data["task"]["latestCheckpointId"],
        resumed.data["checkpoint"]["id"]
    );
    assert_eq!(resumed.data["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(resumed.data["sessions"][1]["continuedFrom"], "session-a");

    let source = temp.path().join("session.json");
    let source_content = vec![b'x'; 128 * 1024 + 17];
    fs::write(&source, &source_content).unwrap();
    let unconfirmed = service
        .session_import_add(
            "TASK-1",
            "session-b",
            8,
            &temp.path().join("not-read-without-confirmation.json"),
            false,
        )
        .unwrap_err();
    assert_eq!(
        unconfirmed.body.details["field"],
        "confirmSensitiveContentReviewed"
    );
    let imported = service
        .session_import_add("TASK-1", "session-b", 8, &source, true)
        .unwrap();
    assert_eq!(version(&imported), 9);
    assert_eq!(
        imported.data["import"]["sha256"],
        hex::encode(Sha256::digest(&source_content))
    );
    let import_id = imported.data["import"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        imported.warnings[0].code,
        "SENSITIVE_CONTENT_CHECK_REQUIRED"
    );
    let duplicate = service
        .session_import_add("TASK-1", "session-b", 9, &source, true)
        .unwrap();
    assert_eq!(version(&duplicate), 9);
    assert_eq!(duplicate.warnings[0].code, "DUPLICATE_SESSION_IMPORT");
    assert!(
        duplicate
            .warnings
            .iter()
            .any(|warning| warning.code == "SENSITIVE_CONTENT_CHECK_REQUIRED")
    );
    assert_eq!(
        service.session_import_list("session-b").unwrap().data["imports"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let removed = service.session_import_remove(&import_id, 9).unwrap();
    assert_eq!(version(&removed), 10);
    assert!(
        service.session_import_list("session-b").unwrap().data["imports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let done = service.task_status("TASK-1", 10, "done").unwrap();
    assert_eq!(done.data["task"]["status"], "done");
    assert_eq!(done.data["task"]["currentSessionId"], "session-b");
    assert!(service.session_show("session-b").unwrap().data["session"]["endedAt"].is_null());

    let history = service.history("TASK-1").unwrap();
    let changes = history.data["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["changeType"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(changes.first(), Some(&"task.created"));
    assert!(changes.contains(&"checkpoint.saved"));
    assert!(changes.contains(&"session.resumed"));
    assert!(changes.contains(&"session.imported"));
    assert!(changes.contains(&"session.import_removed"));
    assert_eq!(changes.last(), Some(&"task.status_changed"));
}

#[test]
fn checkpoint_response_is_its_own_transaction_snapshot() {
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service
        .task_create(
            "TASK-SNAPSHOT",
            r#"{
                "title":"0904｜功能｜Return the committed mutation snapshot",
                "goal":"Keep concurrent command responses version-accurate",
                "scope":"Checkpoint response construction",
                "acceptanceCriteria":"Checkpoint returns version 3 while the database advances to 4"
            }"#,
        )
        .unwrap();
    service
        .task_claim("TASK-SNAPSHOT", 1, "session-a", false)
        .unwrap();

    let large_value = "x".repeat(8 * 1024 * 1024);
    let input = serde_json::to_string(&json!({
        "summary":"Snapshot before a concurrent update",
        "completed":[large_value],
        "decisions":[],
        "pending":[],
        "nextStep":"Continue",
        "risks":[],
    }))
    .unwrap();
    let checkpoint_service = service.clone();
    let checkpoint = std::thread::spawn(move || {
        checkpoint_service.task_checkpoint("TASK-SNAPSHOT", 2, "session-a", &input)
    });

    let numeric_id = task_id(&service, "TASK-SNAPSHOT");
    let connection = rusqlite::Connection::open(service.database_path()).unwrap();
    connection.busy_timeout(Duration::from_secs(5)).unwrap();
    let started = Instant::now();
    loop {
        let current: i64 = connection
            .query_row(
                "SELECT version FROM tasks WHERE id=?1",
                [numeric_id],
                |row| row.get(0),
            )
            .unwrap();
        if current == 3 {
            connection
                .execute(
                    "UPDATE tasks SET version=4,updated_at='concurrent' WHERE id=?1 AND version=3",
                    [numeric_id],
                )
                .unwrap();
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "checkpoint did not commit in time"
        );
        std::thread::yield_now();
    }

    let checkpoint = checkpoint.join().unwrap().unwrap();
    assert_eq!(version(&checkpoint), 3);
    assert_eq!(
        service.task_show("TASK-SNAPSHOT").unwrap().data["task"]["version"],
        4
    );
}

#[test]
fn minimal_create_merge_patch_and_terminal_maintenance_follow_cas() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    let created = service.task_create_minimal().unwrap();
    assert_eq!(created.data["task"]["id"], 1);
    assert!(created.data["task"]["taskKey"].is_null());
    assert!(created.data["task"]["title"].is_null());

    // Even an incomplete task can be marked done without claiming a Session.
    service.task_status("#1", 1, "done").unwrap();
    assert_eq!(service.task_show("1").unwrap().data["task"]["version"], 2);

    let filled = service
        .task_update(
            "1",
            2,
            r#"{
                "taskKey":"PATCHABLE",
                "title":"0904｜功能｜Initial title",
                "goal":"Initial goal",
                "scope":"Initial scope",
                "acceptanceCriteria":"Initial acceptance",
                "nextStep":"Continue"
            }"#,
            true,
            "fixture",
        )
        .unwrap();
    assert_eq!(version(&filled), 3);

    let patched = service
        .task_update(
            "PATCHABLE",
            3,
            r#"{"title":"0904｜优化｜Patched title","goal":"Patched goal","nextStep":null}"#,
            true,
            "fixture",
        )
        .unwrap();
    assert_eq!(version(&patched), 4, "one patch increments version once");
    assert_eq!(patched.data["task"]["title"], "0904｜优化｜Patched title");
    assert!(patched.data["task"]["nextStep"].is_null());

    let immutable_key = service
        .task_update("#1", 4, r#"{"taskKey":"OTHER"}"#, true, "fixture")
        .unwrap_err();
    assert_eq!(immutable_key.body.code, "CONSTRAINT_VIOLATION");
    assert_eq!(
        service.task_show("PATCHABLE").unwrap().data["task"]["version"],
        4
    );

    let clearing = service
        .task_update("PATCHABLE", 4, r#"{"title":null}"#, true, "fixture")
        .unwrap_err();
    assert_eq!(clearing.body.code, "INVALID_INPUT");
    assert_eq!(clearing.body.details["field"], "title");
    assert_eq!(
        service.task_show("PATCHABLE").unwrap().data["task"]["title"],
        "0904｜优化｜Patched title"
    );
    assert_eq!(
        service.task_show("PATCHABLE").unwrap().data["task"]["version"],
        4
    );
    let closed = service.task_status("PATCHABLE", 4, "cancelled").unwrap();
    assert_eq!(version(&closed), 5);
    let closed_at = closed.data["task"]["closedAt"].clone();

    let ordinary_update = service
        .task_update(
            "PATCHABLE",
            5,
            r#"{"nextStep":"follow-up after cancellation"}"#,
            true,
            "fixture",
        )
        .unwrap();
    assert_eq!(
        ordinary_update.data["task"]["nextStep"],
        "follow-up after cancellation"
    );
    let retitled = service
        .task_retitle("PATCHABLE", 6, "0904｜功能｜Retitled closed task")
        .unwrap();
    assert_eq!(version(&retitled), 7);
    assert_eq!(retitled.data["task"]["status"], "cancelled");
    assert_eq!(retitled.data["task"]["closedAt"], closed_at);
    assert_eq!(
        retitled.data["task"]["title"],
        "0904｜功能｜Retitled closed task"
    );
    let stale_retitle = service
        .task_retitle("PATCHABLE", 5, "0904｜功能｜Stale title")
        .unwrap_err();
    assert_eq!(stale_retitle.body.code, "VERSION_CONFLICT");

    let history = service.history("#1").unwrap();
    let entries = history.data["history"].as_array().unwrap();
    let updates = entries
        .iter()
        .filter(|entry| entry["changeType"] == "task.updated")
        .count();
    assert_eq!(updates, 3);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["changeType"] == "task.retitled")
            .count(),
        1
    );
    let retitle = entries.last().unwrap();
    assert_eq!(
        retitle["payload"]["previousTitle"],
        "0904｜优化｜Patched title"
    );
    assert_eq!(
        retitle["payload"]["title"],
        "0904｜功能｜Retitled closed task"
    );
    assert!(history.data["history"][0]["payload"]["taskKey"].is_null());
}

#[test]
fn task_titles_require_the_display_naming_rule() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);

    for title in [
        "Plain title",
        "0904|功能|ASCII separators",
        "1301｜功能｜Invalid month",
        "0230｜功能｜Invalid day",
        "0904｜测试｜Unknown type",
        "0904｜功能｜ trailing ",
    ] {
        let error = service
            .task_create("INVALID-TITLE", &format!(r#"{{"title":"{title}"}}"#))
            .unwrap_err();
        assert_eq!(
            error.body.code, "INVALID_INPUT",
            "unexpected result for {title}"
        );
        assert_eq!(error.body.details["field"], "title");
    }

    let created = service
        .task_create("VALID-TITLE", r#"{"title":"0229｜研究｜Leap-day title"}"#)
        .unwrap();
    assert_eq!(created.data["task"]["title"], "0229｜研究｜Leap-day title");
    let invalid_update = service
        .task_update(
            "VALID-TITLE",
            1,
            r#"{"title":"Still plain"}"#,
            true,
            "fixture",
        )
        .unwrap_err();
    assert_eq!(invalid_update.body.code, "INVALID_INPUT");
    assert_eq!(
        service.task_show("VALID-TITLE").unwrap().data["task"]["version"],
        1
    );
}

#[test]
fn numeric_hash_and_task_key_references_cross_task_relations() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service
        .task_create(
            "RELATIONS",
            r#"{
                "title":"0904｜功能｜Reference relations",
                "goal":"Use numeric foreign keys",
                "scope":"Session, checkpoint and history",
                "acceptanceCriteria":"Every public task reference resolves"
            }"#,
        )
        .unwrap();
    let claimed = service.task_claim("1", 1, "session-a", false).unwrap();
    assert_eq!(claimed.data["task"]["id"], 1);
    let checkpoint = service
        .task_checkpoint(
            "#1",
            2,
            "session-a",
            r#"{
                "summary":"Numeric relation checkpoint",
                "completed":[],
                "decisions":[],
                "pending":[],
                "nextStep":"Continue",
                "risks":[]
            }"#,
        )
        .unwrap();
    assert_eq!(checkpoint.data["checkpoint"]["taskId"], 1);
    assert_eq!(
        service.session_list(Some("RELATIONS")).unwrap().data["sessions"][0]["taskId"],
        1
    );
    assert_eq!(
        service.history("1").unwrap().data["history"][0]["taskId"],
        1
    );
    assert!(
        service
            .task_context("#1")
            .unwrap()
            .data
            .get("worktreeStatus")
            .is_none()
    );
}

#[test]
fn task_list_supports_filters_projection_and_stable_cursor_pagination() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    for (index, key) in ["LIST-A", "LIST-B", "LIST-C", "LIST-D", "LIST-E"]
        .into_iter()
        .enumerate()
    {
        service
            .task_create(
                key,
                &format!(
                    r#"{{
                        "title":"0904｜功能｜List item {index}",
                        "goal":"{}",
                        "scope":"Pagination test scope",
                        "acceptanceCriteria":"Every task appears once"
                    }}"#,
                    match index {
                        2 => "needle",
                        3 => "100% literal",
                        _ => "other",
                    }
                ),
            )
            .unwrap();
    }
    service.task_status("LIST-C", 1, "in_progress").unwrap();

    let exact = service
        .task_list_with_options(&TaskListOptions {
            task_key: Some("LIST-B".to_owned()),
            fields: vec!["id".to_owned(), "taskKey".to_owned()],
            ..TaskListOptions::default()
        })
        .unwrap();
    assert_eq!(exact.data["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(exact.data["tasks"][0]["taskKey"], "LIST-B");
    assert_eq!(exact.data["tasks"][0].as_object().unwrap().len(), 2);

    let searched = service
        .task_list_with_options(&TaskListOptions {
            query: Some("needle".to_owned()),
            ..TaskListOptions::default()
        })
        .unwrap();
    assert_eq!(searched.data["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(searched.data["tasks"][0]["taskKey"], "LIST-C");
    let wildcard = service
        .task_list_with_options(&TaskListOptions {
            query: Some("%".to_owned()),
            ..TaskListOptions::default()
        })
        .unwrap();
    assert_eq!(wildcard.data["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(wildcard.data["tasks"][0]["taskKey"], "LIST-D");
    let active = service
        .task_list_with_options(&TaskListOptions {
            status: Some("in_progress".to_owned()),
            ..TaskListOptions::default()
        })
        .unwrap();
    assert_eq!(active.data["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(active.data["tasks"][0]["taskKey"], "LIST-C");

    let mut cursor = None;
    let mut titles = Vec::new();
    loop {
        let page = service
            .task_list_with_options(&TaskListOptions {
                page_size: Some(2),
                cursor: cursor.clone(),
                fields: vec!["title".to_owned()],
                ..TaskListOptions::default()
            })
            .unwrap();
        assert_eq!(page.data["pageSize"], 2);
        for task in page.data["tasks"].as_array().unwrap() {
            assert_eq!(task.as_object().unwrap().len(), 1);
            titles.push(task["title"].as_str().unwrap().to_owned());
        }
        if !page.data["hasMore"].as_bool().unwrap() {
            break;
        }
        cursor = Some(page.data["nextCursor"].as_str().unwrap().to_owned());
    }
    titles.sort();
    titles.dedup();
    assert_eq!(titles.len(), 5);

    let first = service
        .task_list_with_options(&TaskListOptions {
            page_size: Some(1),
            ..TaskListOptions::default()
        })
        .unwrap();
    let mismatch = service
        .task_list_with_options(&TaskListOptions {
            status: Some("todo".to_owned()),
            cursor: Some(first.data["nextCursor"].as_str().unwrap().to_owned()),
            ..TaskListOptions::default()
        })
        .unwrap_err();
    assert_eq!(mismatch.body.code, "INVALID_INPUT");
    assert_eq!(mismatch.body.details["field"], "cursor");
    let malformed = service
        .task_list_with_options(&TaskListOptions {
            cursor: Some("not-a-cursor".to_owned()),
            ..TaskListOptions::default()
        })
        .unwrap_err();
    assert_eq!(malformed.body.details["field"], "cursor");
    for page_size in [0, 201] {
        let invalid = service
            .task_list_with_options(&TaskListOptions {
                page_size: Some(page_size),
                ..TaskListOptions::default()
            })
            .unwrap_err();
        assert_eq!(invalid.body.details["field"], "pageSize");
    }
}

#[test]
fn task_keys_are_literal_and_cannot_shadow_numeric_references() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service.task_create("key:12", "{}").unwrap();
    assert_eq!(service.task_show("key:12").unwrap().data["task"]["id"], 1);
    assert!(service.task_show("key:key:12").is_err());
    for key in ["12", "#12", "#label"] {
        assert_eq!(
            service.task_create(key, "{}").unwrap_err().body.code,
            "INVALID_INPUT"
        );
    }
}

#[test]
fn task_list_cursor_size_is_bounded_for_long_filters() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    let query = "x".repeat(2_100);
    for key in ["LONG-FILTER-A", "LONG-FILTER-B"] {
        service
            .task_create(
                key,
                &serde_json::to_string(&json!({
                    "title": format!("0904｜功能｜{key}"),
                    "goal": query,
                    "scope": "Cursor size regression",
                    "acceptanceCriteria": "Every cursor can be consumed"
                }))
                .unwrap(),
            )
            .unwrap();
    }

    let first = service
        .task_list_with_options(&TaskListOptions {
            query: Some(query.clone()),
            page_size: Some(1),
            ..TaskListOptions::default()
        })
        .unwrap();
    assert_eq!(first.data["hasMore"], true);
    let cursor = first.data["nextCursor"].as_str().unwrap();
    assert!(cursor.len() <= 4096);
    let second = service
        .task_list_with_options(&TaskListOptions {
            query: Some(query),
            page_size: Some(1),
            cursor: Some(cursor.to_owned()),
            ..TaskListOptions::default()
        })
        .unwrap();
    assert_eq!(second.data["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(second.data["hasMore"], false);
}

#[test]
fn concurrent_writers_cannot_bypass_task_cas() {
    use std::sync::{Arc, Barrier};

    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service.task_create_minimal().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = ["0904｜功能｜first", "0904｜功能｜second"].map(|title| {
        let service = service.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            service.task_update(
                "#1",
                1,
                &format!(r#"{{"title":"{title}"}}"#),
                true,
                "fixture",
            )
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .filter(|error| error.body.code == "VERSION_CONFLICT")
            .count(),
        1
    );
    assert_eq!(service.task_show("1").unwrap().data["task"]["version"], 2);
}

#[test]
fn task_without_current_session_supports_explicit_claim_or_resume() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service
        .task_create(
            "TASK-RESUME",
            r#"{
                "title":"0904｜功能｜Resume continuity",
                "goal":"Preserve the previous session relationship",
                "scope":"Task claim and resume",
                "acceptanceCriteria":"Claim and resume are explicit independent choices"
            }"#,
        )
        .unwrap();
    service
        .task_claim("TASK-RESUME", 1, "session-a", false)
        .unwrap();
    service.session_close("session-a", 2).unwrap();

    let claim = service
        .task_claim("TASK-RESUME", 3, "session-b", false)
        .unwrap();
    assert_eq!(claim.data["task"]["status"], "todo");
    assert_eq!(version(&claim), 4);
    assert!(service.session_show("session-b").unwrap().data["session"]["continuedFrom"].is_null());
    service.session_close("session-b", 4).unwrap();
    let resumed = service
        .task_resume("TASK-RESUME", 5, "session-c", Some("session-a"), false)
        .unwrap();
    assert_eq!(resumed.data["task"]["status"], "todo");
    assert_eq!(
        service.session_show("session-c").unwrap().data["session"]["continuedFrom"],
        "session-a"
    );
}

#[test]
fn invalid_persisted_json_aborts_resume_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service
        .task_create(
            "TASK-CORRUPT",
            r#"{
                "title":"0904｜修复｜Detect invalid stored JSON",
                "goal":"Never disguise database damage as empty data",
                "scope":"Checkpoint and History decoding",
                "acceptanceCriteria":"Reads fail without mutating the Task"
            }"#,
        )
        .unwrap();
    service
        .task_claim("TASK-CORRUPT", 1, "session-a", false)
        .unwrap();
    service
        .task_checkpoint(
            "TASK-CORRUPT",
            2,
            "session-a",
            r#"{
                "summary":"Valid before corruption",
                "completed":["checkpoint"],
                "decisions":[],
                "pending":["resume"],
                "nextStep":"Resume",
                "risks":[]
            }"#,
        )
        .unwrap();

    let numeric_id = task_id(&service, "TASK-CORRUPT");
    let connection = rusqlite::Connection::open(service.database_path()).unwrap();
    connection
        .execute(
            "UPDATE checkpoints SET completed_json=?1 WHERE task_id=?2",
            ("not-json", numeric_id),
        )
        .unwrap();
    drop(connection);

    let error = service
        .task_resume("TASK-CORRUPT", 3, "session-b", Some("session-a"), true)
        .unwrap_err();
    assert_eq!(error.body.code, "DATABASE_UNAVAILABLE");
    assert_eq!(error.body.message, "stored database data is invalid");
    assert!(
        error.body.details["reason"]
            .as_str()
            .unwrap()
            .contains("checkpoints.completed_json")
    );
    let task = service.task_show("TASK-CORRUPT").unwrap();
    assert_eq!(task.data["task"]["version"], 3);
    assert_eq!(task.data["task"]["currentSessionId"], "session-a");
    assert_eq!(
        service.session_list(Some("TASK-CORRUPT")).unwrap().data["sessions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let connection = rusqlite::Connection::open(service.database_path()).unwrap();
    connection
        .execute(
            "UPDATE history SET payload_json=?1 WHERE task_id=?2 AND change_type='task.created'",
            ("not-json", numeric_id),
        )
        .unwrap();
    drop(connection);
    let history_error = service.history("TASK-CORRUPT").unwrap_err();
    assert_eq!(history_error.body.code, "DATABASE_UNAVAILABLE");
    assert!(
        history_error.body.details["reason"]
            .as_str()
            .unwrap()
            .contains("history.payload_json")
    );
}

#[cfg(unix)]
#[test]
fn session_import_rejects_fifo_without_waiting_for_a_writer() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service
        .task_create(
            "TASK-FIFO",
            r#"{
                "title":"0904｜修复｜Reject FIFO import",
                "goal":"Avoid blocking on special files",
                "scope":"Session import",
                "acceptanceCriteria":"FIFO is rejected without a writer"
            }"#,
        )
        .unwrap();
    service
        .task_claim("TASK-FIFO", 1, "session-a", false)
        .unwrap();
    let fifo = temp.path().join("session.fifo");
    let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        sender
            .send(service.session_import_add("TASK-FIFO", "session-a", 2, &fifo, true))
            .unwrap();
    });
    let error = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("FIFO import should return without waiting for a writer")
        .unwrap_err();
    assert_eq!(error.body.code, "INVALID_INPUT");
    assert_eq!(error.body.details["reason"], "must be a regular file");
}
