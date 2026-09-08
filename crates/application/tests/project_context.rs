use serde_json::Value;
use std::{fs, path::Path, process::Command};
use steward_application::{ProjectContextOptions, Service, SourceLocation};

fn context(
    s: &Service,
    project: &str,
    source: i64,
    worktree: Option<&Path>,
    files: &[String],
    deps: &[String],
    budget: usize,
) -> Value {
    s.project_context(
        project,
        source,
        ProjectContextOptions {
            worktree,
            files,
            dependencies: deps,
            budget_bytes: budget,
        },
    )
    .unwrap()
    .data
}
fn git(path: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn setup(temp: &tempfile::TempDir) -> (Service, std::path::PathBuf) {
    let s = Service::new(temp.path().join("state.db"));
    let root = temp.path().join("docs");
    fs::create_dir(&root).unwrap();
    s.project_create("Mailroom").unwrap();
    s.project_source_add("##1", 1, None, SourceLocation::Directory(&root))
        .unwrap();
    (s, root)
}

#[test]
fn git_evidence_handles_unborn_detached_commits_and_incomplete_ignored_content() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("state.db"));
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    s.project_create("Mailroom").unwrap();
    s.project_source_add(
        "##1",
        1,
        None,
        SourceLocation::Git {
            worktree: &repo,
            relative_path: ".",
        },
    )
    .unwrap();
    let unborn = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    assert_eq!(unborn["gitStateObserved"], true);
    assert!(unborn["git"]["head"].is_null());
    assert_eq!(unborn["git"]["branch"], "main");
    assert!(
        unborn["reuseBlockers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("UNBORN_HEAD"))
    );
    git(
        &repo,
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
            "first",
        ],
    );
    let first = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    assert_eq!(first["git"]["dirty"], false);
    git(
        &repo,
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
            "second",
        ],
    );
    let second = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    assert_ne!(first["git"]["head"], second["git"]["head"]);
    assert_ne!(first["fingerprint"], second["fingerprint"]);
    git(&repo, &["checkout", "--detach", "HEAD"]);
    let detached = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    assert!(detached["git"]["branch"].is_null());
    assert_eq!(detached["git"]["head"], second["git"]["head"]);
    assert_ne!(detached["fingerprint"], second["fingerprint"]);
    fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(repo.join("ignored.txt"), "first contents").unwrap();
    let ignored = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    assert_eq!(ignored["git"]["hasIgnored"], true);
    assert_eq!(ignored["git"]["dirty"], true);
    fs::write(repo.join("ignored.txt"), "different unobserved contents").unwrap();
    let changed = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    // Same status is NOT complete content evidence. Never approve reuse on that basis.
    assert_eq!(ignored["fingerprint"], changed["fingerprint"]);
    assert_eq!(changed["reuseAllowed"], false);
    assert!(
        changed["reuseBlockers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("DEPENDENCY_COVERAGE_UNKNOWN"))
    );
    git(&repo, &["add", ".gitignore"]);
    let staged = context(&s, "##1", 1, Some(&repo), &[], &[], 8000);
    assert_ne!(
        changed["git"]["statusSha256"],
        staged["git"]["statusSha256"]
    );
}

#[test]
fn boundary_checks_are_shared_only_within_each_observation_pass() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("state.db"));
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    s.project_create("Mailroom").unwrap();
    s.project_source_add(
        "##1",
        1,
        None,
        SourceLocation::Git {
            worktree: &repo,
            relative_path: ".",
        },
    )
    .unwrap();
    let files: Vec<_> = (0..8).map(|n| format!("sample{n}.rs")).collect();
    for name in &files {
        fs::write(repo.join(name), "fixture").unwrap();
    }
    let started = std::time::Instant::now();
    let result = context(&s, "##1", 1, Some(&repo), &files, &[], 8000);
    assert_eq!(result["observation"]["boundaryChecks"], 2);
    assert!(result["observation"]["fileReads"].as_u64().unwrap() >= 16);
    eprintln!(
        "context fixture: 8 requested files, 2 passes, 2 directory checks; elapsed_ms={}, data_bytes={}",
        started.elapsed().as_millis(),
        serde_json::to_vec(&result).unwrap().len()
    );
    // A new boundary on the next call is not hidden by any cross-call cache.
    fs::create_dir(repo.join("nested")).unwrap();
    fs::write(repo.join("nested/sample.rs"), "fixture").unwrap();
    context(
        &s,
        "##1",
        1,
        Some(&repo),
        &["nested/sample.rs".into()],
        &[],
        8000,
    );
    git(&repo.join("nested"), &["init", "-b", "main"]);
    assert!(
        s.project_context(
            "##1",
            1,
            ProjectContextOptions {
                worktree: Some(&repo),
                files: &["nested/sample.rs".into()],
                dependencies: &[],
                budget_bytes: 8000
            }
        )
        .is_err()
    );
}

#[test]
fn navigation_has_no_bodies_and_observed_hashes_are_not_reuse_authority() {
    let temp = tempfile::tempdir().unwrap();
    let (s, root) = setup(&temp);
    fs::write(root.join("README.md"), "source navigation").unwrap();
    let before = context(&s, "##1", 1, None, &[], &[], 8000);
    assert_eq!(before["reuseAllowed"], false);
    assert_eq!(before["contextVersion"], 3);
    assert!(
        before["reuseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "PERSISTED_IDENTITY_CONTINUITY_UNPROVEN")
    );
    assert_eq!(before["semanticFactsVerified"], false);
    assert!(
        before["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["evidence"] == "file-navigation-v1")
    );
    assert_eq!(before["gitStateObserved"], false);
    assert!(
        before["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry.get("snippet").is_none())
    );
    assert_eq!(
        before["fingerprint"],
        context(&s, "##1", 1, None, &[], &[], 8000)["fingerprint"]
    );
    // A newly appearing rule file changes the manifest, not only edits of existing files.
    fs::write(root.join("AGENTS.md"), "new rule").unwrap();
    let added = context(&s, "##1", 1, None, &[], &[], 8000);
    assert_ne!(before["fingerprint"], added["fingerprint"]);
    fs::write(root.join("AGENTS.override.md"), "override rule").unwrap();
    let overridden = context(&s, "##1", 1, None, &[], &[], 8000);
    assert_ne!(added["fingerprint"], overridden["fingerprint"]);
    assert!(
        overridden["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"]
                .as_str()
                .unwrap()
                .ends_with("AGENTS.override.md"))
    );
    fs::write(root.join("README.md"), "current navigation").unwrap();
    let edited = context(&s, "##1", 1, None, &[], &[], 8000);
    assert_ne!(added["fingerprint"], edited["fingerprint"]);
    // Replacement at identical path with identical content is still a different file object.
    fs::rename(root.join("README.md"), root.join("held.md")).unwrap();
    fs::write(root.join("README.md"), "current navigation").unwrap();
    assert_ne!(
        edited["fingerprint"],
        context(&s, "##1", 1, None, &[], &[], 8000)["fingerprint"]
    );
    assert_eq!(
        s.project_show("##1").unwrap().data["project"]["revision"],
        2
    );
    let conn = storage_sqlite::open_database(&temp.path().join("state.db")).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn utf8_and_json_escaping_fit_the_actual_data_budget_and_omissions_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let (s, root) = setup(&temp);
    let body = "中文\"\\\n\t😀".repeat(5000);
    fs::write(root.join("sample.rs"), &body).unwrap();
    let result = context(&s, "##1", 1, None, &["sample.rs".into()], &[], 2400);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 2400);
    let snippet = result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "requested")
        .unwrap();
    assert!(body.starts_with(snippet["snippet"].as_str().unwrap()));
    assert!(!snippet["snippet"].as_str().unwrap().is_empty());
    assert_eq!(snippet["snippetTruncated"], true);
    assert_eq!(snippet["evidence"], "source-prefix-v1");
    assert_eq!(result["semanticFactsVerified"], false);
    let larger = context(&s, "##1", 1, None, &["sample.rs".into()], &[], 8000);
    assert_eq!(result["fingerprint"], larger["fingerprint"]);
    let larger_snippet = larger["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "requested")
        .unwrap()["snippet"]
        .as_str()
        .unwrap();
    assert!(larger_snippet.len() > snippet["snippet"].as_str().unwrap().len());
    // An observation fingerprint is not a cache key for a budget-dependent rendered response.
    assert_eq!(larger["reuseAllowed"], false);
    for name in [
        "README.md",
        "CODEMAP.md",
        "Cargo.toml",
        "Cargo.lock",
        "package.json",
        "go.mod",
    ] {
        fs::write(root.join(name), "nav").unwrap();
    }
    let small = context(&s, "##1", 1, None, &[], &[], 1400);
    assert!(serde_json::to_vec(&small).unwrap().len() <= 1400);
    assert!(small["omittedEntries"].as_u64().unwrap() > 0);
}

#[test]
fn explicit_files_limits_and_cross_repository_boundaries_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let (s, root) = setup(&temp);
    for file in ["../state.db", ".git/config", "missing.rs"] {
        assert!(
            s.project_context(
                "##1",
                1,
                ProjectContextOptions {
                    worktree: None,
                    files: &[file.into()],
                    dependencies: &[],
                    budget_bytes: 8000
                }
            )
            .is_err()
        );
    }
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("other.rs"), "not this source").unwrap();
    let alias = root.join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    #[cfg(windows)]
    assert!(
        Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success()
    );
    #[cfg(any(unix, windows))]
    assert!(
        s.project_context(
            "##1",
            1,
            ProjectContextOptions {
                worktree: None,
                files: &["alias/other.rs".into()],
                dependencies: &[],
                budget_bytes: 8000
            }
        )
        .is_err()
    );
    fs::write(root.join("big.md"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
    assert!(
        s.project_context(
            "##1",
            1,
            ProjectContextOptions {
                worktree: None,
                files: &["big.md".into()],
                dependencies: &[],
                budget_bytes: 8000
            }
        )
        .is_err()
    );
    fs::write(root.join("binary.rs"), [255, 254]).unwrap();
    assert!(
        s.project_context(
            "##1",
            1,
            ProjectContextOptions {
                worktree: None,
                files: &["binary.rs".into()],
                dependencies: &[],
                budget_bytes: 8000
            }
        )
        .is_err()
    );
    fs::create_dir(root.join("nested")).unwrap();
    git(&root.join("nested"), &["init", "-b", "main"]);
    fs::write(root.join("nested/other.rs"), "other project").unwrap();
    assert!(
        s.project_context(
            "##1",
            1,
            ProjectContextOptions {
                worktree: None,
                files: &["nested/other.rs".into()],
                dependencies: &[],
                budget_bytes: 8000
            }
        )
        .is_err()
    );
}

#[test]
fn worktrees_projects_and_root_dependencies_have_separate_live_fingerprints() {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("state.db"));
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("app")).unwrap();
    fs::create_dir(repo.join("shared")).unwrap();
    fs::write(repo.join("app/main.rs"), "main source").unwrap();
    fs::write(repo.join("shared/lib.rs"), "shared dependency").unwrap();
    fs::write(repo.join("Cargo.lock"), "root lock").unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    );
    let feature = temp.path().join("feature");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            feature.to_str().unwrap(),
        ],
    );
    for name in ["Mailroom", "Steward"] {
        s.project_create(name).unwrap();
    }
    s.project_source_add(
        "##1",
        1,
        None,
        SourceLocation::Git {
            worktree: &repo,
            relative_path: "app",
        },
    )
    .unwrap();
    s.project_source_add(
        "##2",
        1,
        None,
        SourceLocation::Git {
            worktree: &repo,
            relative_path: "app",
        },
    )
    .unwrap();
    let files = ["main.rs".into()];
    let deps = ["shared/lib.rs".into()];
    let first = context(&s, "##1", 1, Some(&repo), &files, &deps, 8000);
    let other = context(&s, "##2", 2, Some(&repo), &files, &deps, 8000);
    assert_ne!(first["fingerprint"], other["fingerprint"]);
    let linked = context(&s, "##1", 1, Some(&feature), &files, &deps, 8000);
    assert_ne!(first["fingerprint"], linked["fingerprint"]);
    // Same HEAD, dirty contents: return the current file, never the old snippet.
    fs::write(feature.join("app/main.rs"), "dirty feature source").unwrap();
    let dirty = context(&s, "##1", 1, Some(&feature), &files, &deps, 8000);
    assert_ne!(linked["fingerprint"], dirty["fingerprint"]);
    assert_eq!(dirty["git"]["head"], linked["git"]["head"]);
    assert_eq!(dirty["git"]["dirty"], true);
    assert_eq!(dirty["reuseAllowed"], false);
    assert_eq!(
        dirty["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "requested")
            .unwrap()["snippet"],
        "dirty feature source"
    );
    assert_eq!(
        first["fingerprint"],
        context(&s, "##1", 1, Some(&repo), &files, &deps, 8000)["fingerprint"]
    );
    fs::write(repo.join("shared/AGENTS.md"), "dependency-local rules").unwrap();
    let rules = context(&s, "##1", 1, Some(&repo), &files, &deps, 8000);
    assert_ne!(first["fingerprint"], rules["fingerprint"]);
    assert!(
        rules["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "rule-navigation"
                && e["path"].as_str().unwrap().ends_with("AGENTS.md"))
    );
    fs::write(repo.join("Cargo.lock"), "changed root lock").unwrap();
    let lock = context(&s, "##1", 1, Some(&repo), &files, &deps, 8000);
    assert_ne!(first["fingerprint"], lock["fingerprint"]);
    fs::write(repo.join("shared/lib.rs"), "changed shared dependency").unwrap();
    assert_ne!(
        lock["fingerprint"],
        context(&s, "##1", 1, Some(&repo), &files, &deps, 8000)["fingerprint"]
    );
    assert!(
        s.project_context(
            "##1",
            2,
            ProjectContextOptions {
                worktree: Some(&repo),
                files: &[],
                dependencies: &[],
                budget_bytes: 8000
            }
        )
        .is_err()
    );
}
