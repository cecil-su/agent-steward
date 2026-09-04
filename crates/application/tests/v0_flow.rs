use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use steward_application::{Service, TaskListOptions};

fn service(temp: &tempfile::TempDir) -> Service {
    Service::new(temp.path().join("steward.db")).with_lock_root(temp.path().join("locks"))
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
    assert_eq!(claimed.data["task"]["status"], "in_progress");
    assert_eq!(version(&claimed), 2);
    let no_op = service.task_claim("TASK-1", 2, "session-a", false).unwrap();
    assert_eq!(version(&no_op), 2);
    let stale = service
        .task_update("TASK-1", 1, r#"{"nextStep":"stale"}"#)
        .unwrap_err();
    assert_eq!(stale.body.code, "VERSION_CONFLICT");

    let updated = service
        .task_update("TASK-1", 2, r#"{"nextStep":"Write tests"}"#)
        .unwrap();
    assert_eq!(version(&updated), 3);
    let noted = service
        .task_note("TASK-1", 3, "decision", "Use SQLite WAL")
        .unwrap();
    assert_eq!(version(&noted), 4);
    let blocked = service
        .task_block("TASK-1", 4, "Need fixture", "Create a synthetic fixture")
        .unwrap();
    assert_eq!(blocked.data["task"]["status"], "blocked");
    let unblocked = service
        .task_unblock("TASK-1", 5, "Save checkpoint")
        .unwrap();
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
    let closed = service.task_close("TASK-1", 10, "completed", None).unwrap();
    assert_eq!(closed.data["task"]["status"], "closed");
    assert!(closed.data["task"]["currentSessionId"].is_null());

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
    assert_eq!(changes.last(), Some(&"task.closed"));
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
fn minimal_create_merge_patch_and_completed_gate_follow_cas() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    let created = service.task_create_minimal().unwrap();
    assert_eq!(created.data["task"]["id"], 1);
    assert!(created.data["task"]["taskKey"].is_null());
    assert!(created.data["task"]["title"].is_null());

    service.task_claim("#1", 1, "session-a", false).unwrap();
    let completed = service.task_close("#1", 2, "completed", None).unwrap_err();
    assert_eq!(completed.body.code, "CONSTRAINT_VIOLATION");
    assert_eq!(
        completed.body.details["constraint"],
        "task.close.completed_requires_complete_descriptions"
    );
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
        )
        .unwrap();
    assert_eq!(version(&filled), 3);

    let patched = service
        .task_update(
            "PATCHABLE",
            3,
            r#"{"title":"0904｜优化｜Patched title","goal":"Patched goal","nextStep":null}"#,
        )
        .unwrap();
    assert_eq!(version(&patched), 4, "one patch increments version once");
    assert_eq!(patched.data["task"]["title"], "0904｜优化｜Patched title");
    assert!(patched.data["task"]["nextStep"].is_null());

    let immutable_key = service
        .task_update("#1", 4, r#"{"taskKey":"OTHER"}"#)
        .unwrap_err();
    assert_eq!(immutable_key.body.code, "CONSTRAINT_VIOLATION");
    assert_eq!(
        service.task_show("PATCHABLE").unwrap().data["task"]["version"],
        4
    );

    let clearing = service
        .task_update("PATCHABLE", 4, r#"{"title":null}"#)
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
    let closed = service
        .task_close("PATCHABLE", 4, "completed", None)
        .unwrap();
    assert_eq!(version(&closed), 5);
    let closed_at = closed.data["task"]["closedAt"].clone();

    let ordinary_update = service
        .task_update("PATCHABLE", 5, r#"{"title":"0904｜文档｜Closed title"}"#)
        .unwrap_err();
    assert_eq!(ordinary_update.body.code, "CONSTRAINT_VIOLATION");
    let retitled = service
        .task_retitle("PATCHABLE", 5, "0904｜功能｜Retitled closed task")
        .unwrap();
    assert_eq!(version(&retitled), 6);
    assert_eq!(retitled.data["task"]["status"], "closed");
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
    assert_eq!(updates, 2);
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
        .task_update("VALID-TITLE", 1, r#"{"title":"Still plain"}"#)
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
                "scope":"Session, checkpoint, history and worktree status",
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
    assert_eq!(
        service.worktree_status("#1").unwrap().data["worktreeStatus"]["registered"],
        false
    );
}

#[test]
fn explicit_key_prefix_resolves_legacy_numeric_and_reserved_task_keys() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    for _ in 0..12 {
        service.task_create_minimal().unwrap();
    }
    storage_sqlite::open_database(service.database_path())
        .unwrap()
        .execute_batch(
            "UPDATE tasks SET task_key='12' WHERE id=1;
             UPDATE tasks SET task_key='#12' WHERE id=2;
             UPDATE tasks SET task_key='key:12' WHERE id=3;",
        )
        .unwrap();

    assert_eq!(service.task_show("12").unwrap().data["task"]["id"], 12);
    assert_eq!(service.task_show("#12").unwrap().data["task"]["id"], 12);
    assert_eq!(service.task_show("key:12").unwrap().data["task"]["id"], 1);
    assert_eq!(service.task_show("key:#12").unwrap().data["task"]["id"], 2);
    assert_eq!(
        service.task_show("key:key:12").unwrap().data["task"]["id"],
        3
    );
    let empty = service.task_show("key:").unwrap_err();
    assert_eq!(empty.body.code, "INVALID_INPUT");
    assert_eq!(empty.body.details["field"], "taskReference");
    let reserved = service
        .task_update("#4", 1, r#"{"taskKey":"key:future"}"#)
        .unwrap_err();
    assert_eq!(reserved.body.code, "INVALID_INPUT");
    assert_eq!(reserved.body.details["field"], "taskKey");
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
    service.task_claim("LIST-C", 1, "session-c", false).unwrap();

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
            status: Some("open".to_owned()),
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
            service.task_update("#1", 1, &format!(r#"{{"title":"{title}"}}"#))
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
fn active_task_without_current_session_requires_resume() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp);
    service
        .task_create(
            "TASK-RESUME",
            r#"{
                "title":"0904｜功能｜Resume continuity",
                "goal":"Preserve the previous session relationship",
                "scope":"Task claim and resume",
                "acceptanceCriteria":"Claim cannot bypass resume"
            }"#,
        )
        .unwrap();
    service
        .task_claim("TASK-RESUME", 1, "session-a", false)
        .unwrap();
    service.session_close("session-a", 2).unwrap();

    let claim = service
        .task_claim("TASK-RESUME", 3, "session-b", false)
        .unwrap_err();
    assert_eq!(claim.body.code, "SESSION_CONFLICT");
    assert_eq!(
        service.task_show("TASK-RESUME").unwrap().data["task"]["version"],
        3
    );

    let resumed = service
        .task_resume("TASK-RESUME", 3, "session-b", Some("session-a"), false)
        .unwrap();
    assert_eq!(resumed.data["sessions"][1]["continuedFrom"], "session-a");
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

#[test]
fn worktree_create_status_dirty_refusal_and_remove_flow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    fs::write(repo.join("-name.txt"), "rename fixture\n").unwrap();
    fs::write(repo.join(".gitignore"), "*.secret\n").unwrap();
    git(&repo, ["add", "-A"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "feature"]);

    let service = service(&temp);
    service
        .task_create(
            "TASK-WT",
            r#"{
                "title":"0904｜功能｜Worktree flow",
                "goal":"Exercise safe Git operations",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Create and remove worktree"
            }"#,
        )
        .unwrap();
    let worktree = temp.path().join("feature-worktree");
    let created = service
        .worktree_create("TASK-WT", 1, &repo, "feature", &worktree)
        .unwrap();
    assert_eq!(version(&created), 2);
    assert_eq!(created.data["worktreeStatus"]["exists"], true);

    fs::rename(worktree.join("-name.txt"), worktree.join("new-name.txt")).unwrap();
    git(&worktree, ["add", "-A"]);
    let renamed = service.worktree_status("TASK-WT").unwrap();
    assert_eq!(
        renamed.data["worktreeStatus"]["staged"],
        json!(["new-name.txt"])
    );
    assert_eq!(renamed.data["worktreeStatus"]["unstaged"], json!([]));
    fs::rename(worktree.join("new-name.txt"), worktree.join("-name.txt")).unwrap();
    git(&worktree, ["add", "-A"]);

    let duplicate = service
        .worktree_create("TASK-WT", 2, &repo, "feature", &temp.path().join("other"))
        .unwrap_err();
    assert_eq!(duplicate.body.code, "WORKTREE_SAFETY_REFUSED");

    fs::write(worktree.join("untracked.txt"), "do not delete\n").unwrap();
    let dirty = service.worktree_remove("TASK-WT", 2).unwrap_err();
    assert_eq!(dirty.body.code, "WORKTREE_SAFETY_REFUSED");
    fs::remove_file(worktree.join("untracked.txt")).unwrap();

    let ignored_path = worktree.join("only-copy.secret");
    fs::write(&ignored_path, "only copy\n").unwrap();
    let ignored_status = service.worktree_status("TASK-WT").unwrap();
    assert_eq!(
        ignored_status.data["worktreeStatus"]["ignored"],
        json!(["only-copy.secret"])
    );
    let ignored = service.worktree_remove("TASK-WT", 2).unwrap_err();
    assert_eq!(ignored.body.code, "WORKTREE_SAFETY_REFUSED");
    assert!(
        ignored.body.details["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("ignored"))
    );
    assert_eq!(fs::read_to_string(&ignored_path).unwrap(), "only copy\n");
    fs::remove_file(ignored_path).unwrap();

    let removed = service.worktree_remove("TASK-WT", 2).unwrap();
    assert_eq!(version(&removed), 3);
    assert_eq!(removed.data["worktreeStatus"]["registered"], false);
    assert!(!worktree.exists());
}

#[test]
fn worktree_create_uses_detected_filesystem_path_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "feature"]);

    let service = service(&temp);
    for task_id in ["TASK-WT-OWNER", "TASK-WT-CREATOR"] {
        service
            .task_create(
                task_id,
                &format!(
                    r#"{{
                        "title":"0904｜功能｜{task_id}",
                        "goal":"Protect registered Worktree ownership",
                        "scope":"Synthetic repository",
                        "acceptanceCriteria":"Only one Task owns the path"
                    }}"#
                ),
            )
            .unwrap();
    }
    let worktree = temp.path().join("MixedParent/Worktree");
    fs::create_dir(worktree.parent().unwrap()).unwrap();
    service
        .worktree_create("TASK-WT-OWNER", 1, &repo, "feature", &worktree)
        .unwrap();
    git(&repo, ["worktree", "remove", worktree.to_str().unwrap()]);
    assert!(!worktree.exists());
    fs::remove_dir(worktree.parent().unwrap()).unwrap();
    assert!(
        git_adapter::find_worktree(&repo, &worktree)
            .unwrap()
            .is_none()
    );

    let attempted_worktree = temp.path().join("mixedparent/worktree");
    let equivalent = git_adapter::worktree_path_key(&temp.path().join("Alias-Aa")).unwrap()
        == git_adapter::worktree_path_key(&temp.path().join("alias-aA")).unwrap();
    if equivalent {
        let error = service
            .worktree_create(
                "TASK-WT-CREATOR",
                1,
                &temp.path().join("repository-that-must-not-be-read"),
                "feature",
                &attempted_worktree,
            )
            .unwrap_err();
        assert_eq!(error.body.code, "WORKTREE_SAFETY_REFUSED");
        assert!(error.body.details["reason"].as_str().is_some_and(|reason| {
            reason.contains(&format!("#{}", task_id(&service, "TASK-WT-OWNER")))
        }));
        assert!(!attempted_worktree.exists());
    } else {
        fs::create_dir(attempted_worktree.parent().unwrap()).unwrap();
        let created = service
            .worktree_create("TASK-WT-CREATOR", 1, &repo, "feature", &attempted_worktree)
            .unwrap();
        assert_eq!(created.data["task"]["version"], 2);
        assert!(attempted_worktree.exists());
    }
    let creator = service.task_show("TASK-WT-CREATOR").unwrap();
    assert_eq!(
        creator.data["task"]["version"],
        if equivalent { 1 } else { 2 }
    );
    assert_eq!(creator.data["task"]["worktreePath"].is_null(), equivalent);
}

#[cfg(unix)]
#[test]
fn worktree_create_rejects_a_non_utf8_target_before_git_mutation() {
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "feature"]);

    let service = service(&temp);
    service
        .task_create(
            "TASK-WT-NON-UTF8",
            r#"{
                "title":"0904｜修复｜Reject non-UTF-8 Worktree",
                "goal":"Keep path persistence lossless",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Git is not mutated"
            }"#,
        )
        .unwrap();
    let worktree = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"worktree-\xff".to_vec()));
    let error = service
        .worktree_create("TASK-WT-NON-UTF8", 1, &repo, "feature", &worktree)
        .unwrap_err();

    assert_eq!(error.body.code, "PATH_IDENTITY_UNKNOWN");
    assert!(!worktree.exists());
    let task = service.task_show("TASK-WT-NON-UTF8").unwrap();
    assert_eq!(task.data["task"]["version"], 1);
    assert!(task.data["task"]["worktreePath"].is_null());
}

#[test]
fn worktree_create_does_not_commit_a_missing_post_checkout_directory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "feature"]);

    let hook = repo.join(".git/hooks/post-checkout");
    fs::write(
        &hook,
        "#!/bin/sh\ntarget=$(pwd) || exit 1\ncd .. || exit 1\nrm -rf -- \"$target\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let service = service(&temp);
    service
        .task_create(
            "TASK-WT-MISSING",
            r#"{
                "title":"0904｜修复｜Missing worktree directory",
                "goal":"Do not commit an invalid Worktree reference",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Database stays unchanged"
            }"#,
        )
        .unwrap();
    let worktree = temp.path().join("missing-after-checkout");

    let error = service
        .worktree_create("TASK-WT-MISSING", 1, &repo, "feature", &worktree)
        .unwrap_err();

    assert_eq!(error.body.code, "PARTIAL_EXTERNAL_STATE");
    assert_eq!(error.body.details["databaseState"], "unchanged");
    assert_eq!(error.body.details["gitState"]["pathExists"], false);
    assert_eq!(error.body.details["gitState"]["registeredByGit"], true);
    assert_eq!(
        error.body.details["recommendedArgs"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "doctor"
    );
    assert!(!worktree.exists());

    let task = service.task_show("TASK-WT-MISSING").unwrap();
    assert_eq!(task.data["task"]["version"], 1);
    assert!(task.data["task"]["repositoryPath"].is_null());
    assert!(task.data["task"]["worktreePath"].is_null());
    let history = service.history("TASK-WT-MISSING").unwrap();
    let changes = history.data["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["changeType"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(changes, vec!["task.created"]);
}

#[test]
fn unicode_worktree_rechecks_live_owner_and_can_be_detached_after_external_removal() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "unicode"]);
    let worktree = temp.path().join("工作树");
    git(
        &repo,
        ["worktree", "add", worktree.to_str().unwrap(), "unicode"],
    );

    let service = service(&temp);
    for task_id in ["TASK-WT-UNICODE-OWNER", "TASK-WT-UNICODE-ADOPTER"] {
        service
            .task_create(
                task_id,
                &format!(
                    r#"{{
                        "title":"0904｜功能｜{task_id}",
                        "goal":"Keep one live Worktree owner",
                        "scope":"Synthetic repository",
                        "acceptanceCriteria":"Stale comparison keys cannot hide ownership"
                    }}"#
                ),
            )
            .unwrap();
    }
    service
        .worktree_adopt("TASK-WT-UNICODE-OWNER", 1, &repo, &worktree)
        .unwrap();
    storage_sqlite::open_database(service.database_path())
        .unwrap()
        .execute(
            "UPDATE tasks SET worktree_path_key='stale-key' WHERE id=?1",
            [task_id(&service, "TASK-WT-UNICODE-OWNER")],
        )
        .unwrap();

    let error = service
        .worktree_adopt("TASK-WT-UNICODE-ADOPTER", 1, &repo, &worktree)
        .unwrap_err();

    assert_eq!(error.body.code, "WORKTREE_SAFETY_REFUSED");
    assert!(error.body.details["reason"].as_str().is_some_and(|reason| {
        reason.contains(&format!("#{}", task_id(&service, "TASK-WT-UNICODE-OWNER")))
    }));
    let adopter = service.task_show("TASK-WT-UNICODE-ADOPTER").unwrap();
    assert_eq!(adopter.data["task"]["version"], 1);
    assert!(adopter.data["task"]["worktreePath"].is_null());

    let registered_path =
        service.task_show("TASK-WT-UNICODE-OWNER").unwrap().data["task"]["worktreePath"]
            .as_str()
            .unwrap()
            .to_owned();
    git(&repo, ["worktree", "remove", worktree.to_str().unwrap()]);

    let doctor = service.doctor().unwrap();
    let issue = doctor.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["code"] == "WORKTREE_REFERENCES")
        .unwrap()["details"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["taskId"] == task_id(&service, "TASK-WT-UNICODE-OWNER"))
        .unwrap();
    assert_eq!(issue["registeredByGit"], false);
    assert_eq!(
        issue["recommendedArgs"].as_array().unwrap().last().unwrap(),
        "2"
    );
    assert!(
        issue["recommendedArgs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == &registered_path)
    );

    let detached = service
        .worktree_detach("TASK-WT-UNICODE-OWNER", 2, Path::new(&registered_path))
        .unwrap();
    assert_eq!(version(&detached), 3);
    assert!(detached.data["task"]["worktreePath"].is_null());
}

#[test]
fn worktree_adopt_uses_live_owner_result_when_persisted_keys_collide() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "owner"]);
    git(&repo, ["branch", "adopter"]);
    let owner_worktree = temp.path().join("owner-worktree");
    let adopter_worktree = temp.path().join("adopter-worktree");
    git(
        &repo,
        ["worktree", "add", owner_worktree.to_str().unwrap(), "owner"],
    );
    git(
        &repo,
        [
            "worktree",
            "add",
            adopter_worktree.to_str().unwrap(),
            "adopter",
        ],
    );

    let service = service(&temp);
    for task_id in ["TASK-WT-KEY-OWNER", "TASK-WT-KEY-ADOPTER"] {
        service
            .task_create(
                task_id,
                &format!(
                    r#"{{
                        "title":"0904｜功能｜{task_id}",
                        "goal":"Use live Worktree ownership",
                        "scope":"Synthetic repository",
                        "acceptanceCriteria":"A stale diagnostic key cannot reject a distinct Worktree"
                    }}"#
                ),
            )
            .unwrap();
    }
    service
        .worktree_adopt("TASK-WT-KEY-OWNER", 1, &repo, &owner_worktree)
        .unwrap();
    let adopter_key = git_adapter::worktree_path_key(&adopter_worktree).unwrap();
    storage_sqlite::open_database(service.database_path())
        .unwrap()
        .execute(
            "UPDATE tasks SET worktree_path_key=?2 WHERE id=?1",
            rusqlite::params![task_id(&service, "TASK-WT-KEY-OWNER"), adopter_key],
        )
        .unwrap();

    let adopted = service
        .worktree_adopt("TASK-WT-KEY-ADOPTER", 1, &repo, &adopter_worktree)
        .unwrap();

    assert_eq!(version(&adopted), 2);
    assert_eq!(
        adopted.data["task"]["worktreePath"],
        fs::canonicalize(&adopter_worktree)
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn worktree_adopt_doctor_and_detach_recover_external_state() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "recovery"]);

    let worktree = temp.path().join("recovery worktree;$");
    git(
        &repo,
        ["worktree", "add", worktree.to_str().unwrap(), "recovery"],
    );
    let database = temp.path().join("custom data").join("steward db.sqlite");
    let service =
        Service::new(&database).with_lock_root(temp.path().join("recovery operation locks"));
    let task_id = "TASK RECOVER;$";
    service
        .task_create(
            task_id,
            r#"{
                "title":"0904｜修复｜Recovery flow",
                "goal":"Reconcile proven external Git state",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Adopt and detach succeed"
            }"#,
        )
        .unwrap();
    let numeric_id = service.task_show(task_id).unwrap().data["task"]["id"]
        .as_i64()
        .unwrap();
    let adopted = service
        .worktree_adopt(task_id, 1, &repo, &worktree)
        .unwrap();
    assert_eq!(version(&adopted), 2);
    assert_eq!(adopted.data["worktreeStatus"]["exists"], true);

    git(&repo, ["worktree", "remove", worktree.to_str().unwrap()]);
    let registered_path = adopted.data["worktreeStatus"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let remove_error = service.worktree_remove(task_id, 2).unwrap_err();
    assert_eq!(remove_error.body.code, "WORKTREE_SAFETY_REFUSED");
    assert!(
        remove_error.body.details["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("use detach"))
    );
    let stale_status = service.worktree_status(task_id).unwrap();
    assert_eq!(stale_status.data["worktreeStatus"]["registered"], true);
    assert_eq!(stale_status.data["worktreeStatus"]["exists"], false);
    let doctor = service.doctor().unwrap();
    let worktree_check = doctor.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["code"] == "WORKTREE_REFERENCES")
        .unwrap();
    assert_eq!(worktree_check["status"], "warning");
    let issue = &worktree_check["details"]["issues"][0];
    assert!(
        issue["recommendedCommand"]
            .as_str()
            .is_some_and(|command| command.contains("taskctl"))
    );
    assert_eq!(issue["registeredByGit"], false);
    assert!(issue["recoveryNote"].is_null());
    assert_eq!(
        issue["recommendedArgs"],
        json!([
            "taskctl",
            "--database",
            fs::canonicalize(&database).unwrap().to_string_lossy(),
            "worktree",
            "detach",
            numeric_id.to_string(),
            "--expected-path",
            registered_path,
            "--if-version",
            "2",
        ])
    );

    let expected_path = worktree.with_file_name("RECOVERY WORKTREE;$");
    let equivalent = git_adapter::worktree_path_key(&worktree).unwrap()
        == git_adapter::worktree_path_key(&expected_path).unwrap();
    let detached = if equivalent {
        service.worktree_detach(task_id, 2, &expected_path).unwrap()
    } else {
        let refused = service
            .worktree_detach(task_id, 2, &expected_path)
            .unwrap_err();
        assert_eq!(refused.body.code, "WORKTREE_SAFETY_REFUSED");
        service.worktree_detach(task_id, 2, &worktree).unwrap()
    };
    assert_eq!(version(&detached), 3);
    assert_eq!(detached.data["worktreeStatus"]["registered"], false);
    let history = service.history(task_id).unwrap();
    let changes = history.data["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["changeType"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        changes,
        vec!["task.created", "worktree.adopted", "worktree.detached"]
    );
}

#[test]
fn doctor_does_not_recommend_detach_while_git_still_registers_the_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, ["init", "-b", "main"]);
    git(&repo, ["config", "user.email", "tests@example.invalid"]);
    git(&repo, ["config", "user.name", "Agent Steward Tests"]);
    fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-m", "fixture"]);
    git(&repo, ["branch", "recovery"]);

    let worktree = temp.path().join("manually deleted worktree");
    git(
        &repo,
        ["worktree", "add", worktree.to_str().unwrap(), "recovery"],
    );
    let service = service(&temp);
    let task_id = "TASK-GIT-STALE";
    service
        .task_create(
            task_id,
            r#"{
                "title":"0904｜修复｜Stale registration recovery",
                "goal":"Never recommend a detach that Git would refuse",
                "scope":"Synthetic repository",
                "acceptanceCriteria":"Doctor keeps reporting until Git registration is gone"
            }"#,
        )
        .unwrap();
    let adopted = service
        .worktree_adopt(task_id, 1, &repo, &worktree)
        .unwrap();
    assert_eq!(version(&adopted), 2);

    // Delete the directory manually: Git still registers the worktree.
    fs::remove_dir_all(&worktree).unwrap();
    let remove_error = service.worktree_remove(task_id, 2).unwrap_err();
    assert_eq!(remove_error.body.code, "WORKTREE_SAFETY_REFUSED");
    let reason = remove_error.body.details["reason"].as_str().unwrap();
    assert!(reason.contains("Git still registers"));
    assert!(reason.contains("taskctl doctor"));
    assert!(!reason.contains("use detach"));
    let doctor = service.doctor().unwrap();
    let worktree_check = doctor.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["code"] == "WORKTREE_REFERENCES")
        .unwrap();
    assert_eq!(worktree_check["status"], "warning");
    let issue = &worktree_check["details"]["issues"][0];
    assert_eq!(issue["registeredByGit"], true);
    assert_eq!(
        issue["reason"],
        "registered worktree is absent but Git still registers it"
    );
    assert!(
        issue["recoveryNote"]
            .as_str()
            .is_some_and(|note| note.contains("prune"))
    );
    assert_eq!(
        issue["recommendedArgs"],
        json!([
            "taskctl",
            "--database",
            fs::canonicalize(service.database_path())
                .unwrap()
                .to_string_lossy(),
            "doctor",
        ])
    );

    let aliased_path = worktree.with_file_name("MANUALLY DELETED WORKTREE");
    if git_adapter::paths_equivalent(&worktree, &aliased_path).unwrap() {
        let refused = service
            .worktree_detach(task_id, 2, &aliased_path)
            .unwrap_err();
        assert_eq!(refused.body.code, "WORKTREE_SAFETY_REFUSED");
        assert!(
            service.task_show(task_id).unwrap().data["task"]["worktreePath"]
                .as_str()
                .is_some()
        );
        assert!(
            git_adapter::find_worktree_registration(&repo, &worktree)
                .unwrap()
                .is_some()
        );
    }

    // detach itself must still refuse while Git registers the worktree.
    let refused = service.worktree_detach(task_id, 2, &worktree).unwrap_err();
    assert_eq!(refused.body.code, "WORKTREE_SAFETY_REFUSED");
}

#[test]
fn doctor_does_not_recommend_detach_for_a_different_repository_common_dir() {
    let temp = tempfile::tempdir().unwrap();
    let registered_repo = temp.path().join("registered-repo");
    let current_repo = temp.path().join("current-repo");
    fs::create_dir(&registered_repo).unwrap();
    fs::create_dir(&current_repo).unwrap();
    git(&registered_repo, ["init", "-b", "main"]);
    git(&current_repo, ["init", "-b", "main"]);
    let registered_info = git_adapter::repository_info(&registered_repo).unwrap();
    let current_info = git_adapter::repository_info(&current_repo).unwrap();
    let missing_worktree = temp.path().join("missing-worktree");

    let service = service(&temp);
    service
        .task_create(
            "TASK-DOCTOR-COMMON-DIR",
            r#"{
                "title":"0904｜修复｜Repository identity mismatch",
                "goal":"Do not recommend an impossible detach",
                "scope":"Synthetic repository references",
                "acceptanceCriteria":"Doctor recommends continued diagnosis"
            }"#,
        )
        .unwrap();
    let worktree_key = git_adapter::worktree_path_key(&missing_worktree).unwrap();
    storage_sqlite::open_database(service.database_path())
        .unwrap()
        .execute(
            "UPDATE tasks SET repository_path=?2,repository_common_dir=?3,
                repository_branch='main',worktree_path=?4,worktree_path_key=?5,
                version=2 WHERE id=?1",
            rusqlite::params![
                task_id(&service, "TASK-DOCTOR-COMMON-DIR"),
                current_info.repository_path.to_string_lossy(),
                registered_info.common_dir.to_string_lossy(),
                missing_worktree.to_string_lossy(),
                worktree_key,
            ],
        )
        .unwrap();

    let doctor = service.doctor().unwrap();
    let issue = doctor.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["code"] == "WORKTREE_REFERENCES")
        .unwrap()["details"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["taskId"] == task_id(&service, "TASK-DOCTOR-COMMON-DIR"))
        .unwrap();
    assert_eq!(
        issue["reason"],
        "repository identity no longer matches the registered common directory"
    );
    assert!(issue["registeredByGit"].is_null());
    assert_eq!(
        issue["recommendedArgs"].as_array().unwrap().last().unwrap(),
        "doctor"
    );
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(dead_code)]
fn _assert_json(value: &Value) {
    assert!(value.is_object());
}
