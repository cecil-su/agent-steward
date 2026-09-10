use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use steward_application::{Service, SourceLocation};

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn repo(path: &Path, dirs: &[&str]) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-b", "main"]);
    for dir in dirs {
        fs::create_dir_all(path.join(dir)).unwrap();
        fs::write(path.join(dir).join("fixture.txt"), "synthetic source").unwrap();
    }
    git(path, &["add", "."]);
    git(
        path,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
}
fn service(temp: &tempfile::TempDir) -> Service {
    Service::new(temp.path().join("state.db"))
}
fn add(
    s: &Service,
    project: &str,
    revision: i64,
    component: Option<&str>,
    repo: &Path,
    path: &str,
) -> Value {
    s.project_source_add(
        project,
        revision,
        component,
        SourceLocation::Git {
            worktree: repo,
            relative_path: path,
        },
    )
    .unwrap()
    .data
}
fn resolved_path(data: &Value) -> PathBuf {
    PathBuf::from(data["resolvedPath"].as_str().unwrap())
}

#[test]
fn task_component_scope_stays_within_its_project_and_uses_task_cas() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    s.project_create("Other").unwrap();
    s.project_component_add("##1", 1, "frontend").unwrap();
    s.project_component_add("##1", 2, "backend").unwrap();
    s.project_component_add("##2", 1, "android").unwrap();
    s.task_create_minimal().unwrap();
    assert_eq!(
        s.task_set_components("#1", 1, &["frontend".into()], true, "fixture")
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    s.task_set_project("#1", 1, Some("##1"), true, "fixture").unwrap();
    assert_eq!(
        s.task_set_components("#1", 2, &["android".into()], true, "fixture")
            .unwrap_err()
            .body
            .code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.task_set_components("#1", 2, &["frontend".into(), "FRONTEND".into()], true, "fixture")
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    let scoped = s
        .task_set_components("#1", 2, &["backend".into(), "frontend".into()], true, "fixture")
        .unwrap()
        .data["task"]
        .clone();
    assert_eq!(scoped["componentIds"], serde_json::json!([1, 2]));
    assert_eq!(scoped["version"], 3);
    assert_eq!(
        s.task_set_components("#1", 3, &["frontend".into(), "backend".into()], true, "fixture")
            .unwrap()
            .data["task"],
        scoped
    );
    assert_eq!(
        s.task_set_components("#1", 2, &[], true, "fixture").unwrap_err().body.code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.task_set_project("#1", 3, Some("##1"), true, "fixture").unwrap().data["task"],
        scoped
    );
    let list = s
        .task_list_with_options(&steward_application::TaskListOptions {
            fields: vec!["componentIds".into()],
            ..Default::default()
        })
        .unwrap()
        .data;
    assert_eq!(list["tasks"][0]["componentIds"], scoped["componentIds"]);
    assert_eq!(
        s.task_context("#1").unwrap().data["task"]["componentIds"],
        scoped["componentIds"]
    );
    let conn = storage_sqlite::open_database(&temp.path().join("state.db")).unwrap();
    assert!(
        conn.execute(
            "INSERT INTO task_components(task_id,project_id,component_id) VALUES (1,2,3)",
            []
        )
        .is_err()
    );
    conn.execute_batch("CREATE TRIGGER fail_scope BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(s.task_set_components("#1", 3, &[], true, "fixture").is_err());
    assert!(s.task_set_project("#1", 3, Some("##2"), true, "fixture").is_err());
    assert_eq!(s.task_show("#1").unwrap().data["task"], scoped);
    conn.execute_batch("DROP TRIGGER fail_scope;").unwrap();
    s.task_claim("#1", 3, "s1", false).unwrap();
    assert!(s.task_set_project("#1", 4, Some("##2"), true, "fixture").is_err());
    let moved = s.task_update("#1", 4, r#"{"project":"2","components":[]}"#, true, "fixture").unwrap().data["task"].clone();
    assert_eq!(moved["componentIds"], serde_json::json!([]));
    assert_eq!(moved["currentSessionId"], "s1");
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        3
    );
    let history = s.history("#1").unwrap().data;
    assert_eq!(
        history["history"].as_array().unwrap().last().unwrap()["payload"]["previousComponentIds"],
        serde_json::json!([1, 2])
    );
    s.task_set_components("#1", 5, &["android".into()], true, "fixture").unwrap();
    s.task_set_components("#1", 6, &[], true, "fixture").unwrap();
    s.task_close("#1", 7, "cancelled", Some("test complete"))
        .unwrap();
    assert_eq!(s.task_set_components("#1", 8, &["android".into()], true, "fixture").unwrap().data["task"]["status"], "closed");
}

#[test]
fn monorepo_multi_repository_and_shared_roots_are_explicit_not_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let root = temp.path().join("platform");
    let backend = temp.path().join("mail-api");
    repo(&root, &["apps/mail", "apps/order", "packages/common"]);
    repo(&backend, &["src"]);
    s.project_create("Mailroom").unwrap();
    s.project_create("Order").unwrap();
    s.project_component_add("##1", 1, "frontend").unwrap();
    s.project_component_add("##1", 2, "backend").unwrap();
    let mail = add(&s, "##1", 3, Some("FRONTEND"), &root, "apps/mail");
    let order = add(&s, "##2", 1, None, &root, "apps/order");
    assert_eq!(
        mail["source"]["repositoryId"],
        order["source"]["repositoryId"]
    );
    let api = add(&s, "##1", 4, Some("backend"), &backend, "src");
    assert_ne!(
        mail["source"]["repositoryId"],
        api["source"]["repositoryId"]
    );
    add(&s, "##1", 5, None, &root, "packages/common");
    add(&s, "##2", 2, None, &root, "packages/common");
    let before = s.project_history("##1", 0, 200).unwrap().data;
    let here = s
        .project_here(&root.join("packages/common"), None)
        .unwrap()
        .data;
    let direct: Vec<_> = here["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["matchedBy"] == "source")
        .collect();
    assert_eq!(direct.len(), 2);
    assert_ne!(direct[0]["project"]["id"], direct[1]["project"]["id"]);
    assert!(here["selectedProjectId"].is_null());
    let inside = s.project_here(&root.join("apps/mail"), None).unwrap().data;
    assert_eq!(inside["candidates"][0]["project"]["name"], "Mailroom");
    assert_eq!(inside["candidates"][0]["matchedBy"], "source");
    let filtered = s.project_here(&root, Some("##2")).unwrap().data;
    assert!(
        filtered["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["project"]["id"] == 2)
    );
    assert_eq!(
        s.project_sources("##1").unwrap().data["sources"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        s.project_components("##1").unwrap().data["components"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(s.project_history("##1", 0, 200).unwrap().data, before);
    assert!(
        s.task_list(None).unwrap().data["tasks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.session_list(None).unwrap().data["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn linked_worktrees_share_repository_identity_but_resolve_their_own_files() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let root = temp.path().join("repository");
    let worktree = temp.path().join("feature");
    repo(&root, &["app"]);
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );
    s.project_create("Mailroom").unwrap();
    let source = add(&s, "##1", 1, None, &root, "app");
    let id = source["source"]["id"].as_i64().unwrap();
    fs::write(worktree.join("app/fixture.txt"), "dirty feature contents").unwrap();
    let resolved = s
        .project_source_resolve("##1", id, Some(&worktree))
        .unwrap()
        .data;
    assert_eq!(
        resolved_path(&resolved),
        fs::canonicalize(worktree.join("app")).unwrap()
    );
    assert_eq!(
        s.project_source_resolve("##1", id, None)
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    s.project_create("Shared").unwrap();
    let other = add(&s, "##2", 1, None, &worktree, "app");
    assert_eq!(
        source["source"]["repositoryId"],
        other["source"]["repositoryId"]
    );
    let here = s.project_here(&worktree.join("app"), None).unwrap().data;
    assert_eq!(here["candidates"].as_array().unwrap().len(), 2);
    for candidate in here["candidates"].as_array().unwrap() {
        assert_eq!(
            resolved_path(candidate),
            fs::canonicalize(worktree.join("app")).unwrap()
        );
    }
    let clone = temp.path().join("clone");
    git(
        temp.path(),
        &["clone", root.to_str().unwrap(), clone.to_str().unwrap()],
    );
    assert_eq!(
        s.project_source_resolve("##1", id, Some(&clone))
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    assert!(
        s.project_here(&clone.join("app"), None).unwrap().data["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_paths_nested_repositories_and_wrong_components_never_register() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let root = temp.path().join("repo");
    repo(&root, &["app"]);
    s.project_create("Mailroom").unwrap();
    s.project_create("Other").unwrap();
    s.project_component_add("##2", 1, "only-other-project")
        .unwrap();
    assert_eq!(
        s.project_source_add(
            "##1",
            1,
            Some("only-other-project"),
            SourceLocation::Git {
                worktree: &root,
                relative_path: "app"
            }
        )
        .unwrap_err()
        .body
        .code,
        "NOT_FOUND"
    );
    for path in [
        "../repo",
        "/app",
        "a/../app",
        ".git",
        "app/fixture.txt",
        "missing",
    ] {
        assert!(
            s.project_source_add(
                "##1",
                1,
                None,
                SourceLocation::Git {
                    worktree: &root,
                    relative_path: path
                }
            )
            .is_err(),
            "{path}"
        );
    }
    repo(&root.join("nested"), &["src"]);
    assert!(
        s.project_source_add(
            "##1",
            1,
            None,
            SourceLocation::Git {
                worktree: &root,
                relative_path: "nested/src"
            }
        )
        .is_err()
    );
    add(&s, "##1", 1, None, &root, ".");
    assert!(
        s.project_here(&root.join("nested/src"), None).unwrap().data["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        s.project_source_add("##1", 2, None, SourceLocation::Directory(&root))
            .unwrap_err()
            .body
            .code,
        "INVALID_INPUT"
    );
    assert_eq!(
        s.project_sources("##1").unwrap().data["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[cfg(any(unix, windows))]
fn directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let out = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
#[cfg(any(unix, windows))]
fn source_symlink_escape_is_rejected_and_checkout_alias_deduplicates() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let root = temp.path().join("repo");
    let outside = temp.path().join("outside");
    repo(&root, &["app"]);
    fs::create_dir(&outside).unwrap();
    directory_link(&outside, &root.join("escape"));
    s.project_create("Mailroom").unwrap();
    assert!(
        s.project_source_add(
            "##1",
            1,
            None,
            SourceLocation::Git {
                worktree: &root,
                relative_path: "escape"
            }
        )
        .is_err()
    );
    let alias = temp.path().join("alias");
    directory_link(&root, &alias);
    let original = add(&s, "##1", 1, None, &root, "app");
    s.project_create("Other").unwrap();
    let linked = add(&s, "##2", 1, None, &alias, "app");
    assert_eq!(
        original["source"]["repositoryId"],
        linked["source"]["repositoryId"]
    );
}

#[test]
fn replaced_git_identity_is_never_treated_as_the_registered_repository() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let root = temp.path().join("repo");
    repo(&root, &["app"]);
    s.project_create("Mailroom").unwrap();
    let source = add(&s, "##1", 1, None, &root, "app");
    let id = source["source"]["id"].as_i64().unwrap();
    fs::rename(root.join(".git"), temp.path().join("held-old-git")).unwrap();
    git(&root, &["init", "-b", "main"]);
    assert_eq!(
        s.project_source_resolve("##1", id, Some(&root))
            .unwrap_err()
            .body
            .code,
        "PATH_IDENTITY_UNKNOWN"
    );
    let here = s.project_here(&root, None).unwrap();
    assert!(here.data["candidates"].as_array().unwrap().is_empty());
    assert_eq!(here.warnings[0].code, "SOURCE_UNAVAILABLE");
    assert!(
        s.project_source_add(
            "##1",
            2,
            None,
            SourceLocation::Git {
                worktree: &root,
                relative_path: "app"
            }
        )
        .is_err()
    );
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        2
    );
}

#[test]
fn directory_sources_are_identity_checked_and_removal_never_deletes_files() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let directory = temp.path().join("documents");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("keep.md"), "keep").unwrap();
    s.project_create("Mailroom").unwrap();
    let source = s
        .project_source_add("##1", 1, None, SourceLocation::Directory(&directory))
        .unwrap()
        .data;
    let id = source["source"]["id"].as_i64().unwrap();
    assert!(source["source"]["repositoryId"].is_null());
    assert_eq!(
        resolved_path(&s.project_source_resolve("##1", id, None).unwrap().data),
        fs::canonicalize(&directory).unwrap()
    );
    assert_eq!(
        s.project_here(&directory, None).unwrap().data["candidates"][0]["project"]["id"],
        1
    );
    git(&directory, &["init", "-b", "main"]);
    assert!(s.project_source_resolve("##1", id, None).is_err());
    fs::rename(directory.join(".git"), temp.path().join("held-new-git")).unwrap();
    fs::rename(&directory, temp.path().join("held-documents")).unwrap();
    fs::create_dir(&directory).unwrap();
    assert_eq!(
        s.project_source_resolve("##1", id, None)
            .unwrap_err()
            .body
            .code,
        "PATH_IDENTITY_UNKNOWN"
    );
    assert!(
        s.project_here(&directory, None).unwrap().data["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::remove_dir(&directory).unwrap();
    s.project_source_remove("##1", 2, id).unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("held-documents/keep.md")).unwrap(),
        "keep"
    );
    assert!(
        s.project_sources("##1").unwrap().data["sources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn metadata_changes_are_cas_guarded_and_roll_back_with_history() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    let root = temp.path().join("repo");
    repo(&root, &["app"]);
    s.project_create("Mailroom").unwrap();
    s.project_component_add("##1", 1, "backend").unwrap();
    assert_eq!(
        s.project_component_add("##1", 1, "other")
            .unwrap_err()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.project_component_add("##1", 2, "BACKEND")
            .unwrap_err()
            .body
            .code,
        "CONSTRAINT_VIOLATION"
    );
    assert_eq!(
        s.project_source_add(
            "##1",
            1,
            None,
            SourceLocation::Directory(Path::new("missing"))
        )
        .unwrap_err()
        .body
        .code,
        "VERSION_CONFLICT"
    );
    let conn = storage_sqlite::open_database(&temp.path().join("state.db")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON project_history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        s.project_source_add(
            "##1",
            2,
            Some("backend"),
            SourceLocation::Git {
                worktree: &root,
                relative_path: "app"
            }
        )
        .is_err()
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM repositories", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM source_roots", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        2
    );
    conn.execute_batch("DROP TRIGGER fail_history;").unwrap();
    let source = add(&s, "##1", 2, Some("backend"), &root, "app");
    assert_eq!(
        s.project_source_add(
            "##1",
            3,
            Some("backend"),
            SourceLocation::Git {
                worktree: &root,
                relative_path: "app"
            }
        )
        .unwrap_err()
        .body
        .code,
        "CONSTRAINT_VIOLATION"
    );
    let id = source["source"]["id"].as_i64().unwrap();
    s.project_create("Other").unwrap();
    assert_eq!(
        s.project_source_remove("##2", 1, id).unwrap_err().body.code,
        "NOT_FOUND"
    );
    assert_eq!(
        s.project_source_remove("##1", 2, id).unwrap_err().body.code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        3
    );
    assert!(conn.execute("INSERT INTO source_roots(project_id,component_id,repository_id,relative_path,created_at) VALUES (2,1,1,'.','now')", []).is_err());
}

#[test]
fn concurrent_source_registration_never_loses_a_project_revision() {
    let temp = tempfile::tempdir().unwrap();
    let s = service(&temp);
    s.project_create("Mailroom").unwrap();
    let directory = temp.path().join("docs");
    fs::create_dir(&directory).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = temp.path().join("state.db");
            let directory = directory.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Service::new(path).project_source_add(
                    "##1",
                    1,
                    None,
                    SourceLocation::Directory(&directory),
                )
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap()
            .body
            .code,
        "VERSION_CONFLICT"
    );
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        2
    );
}
