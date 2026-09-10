use super::*;
use crate::SourceLocation;
use std::{cell::Cell, fs};

fn fixture() -> (tempfile::TempDir, Service) {
    let temp = tempfile::tempdir().unwrap();
    let s = Service::new(temp.path().join("state.db"));
    let root = temp.path().join("source");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("sample.rs"), "original source").unwrap();
    s.project_create("Steward").unwrap();
    s.project_source_add("##1", 1, None, SourceLocation::Directory(&root))
        .unwrap();
    s.task_create_minimal().unwrap();
    s.task_set_project("#1", 1, Some("##1"), true, "fixture").unwrap();
    s.task_claim("#1", 2, "s1", false).unwrap();
    (temp, s)
}
fn request<'a>() -> HostContextRequest<'a> {
    HostContextRequest {
        task: "#1",
        task_version: 3,
        session_id: "s1",
        source_id: 1,
        context: ProjectContextOptions {
            worktree: None,
            files: &[],
            dependencies: &[],
            budget_bytes: 8000,
        },
    }
}
fn host() -> HostInstance<'static> {
    HostInstance {
        name: "fixture",
        version: "1",
        instance_id: "instance-1",
    }
}
fn report(s: &Service, request: &HostContextRequest<'_>) -> HostEvidence {
    let binding = s.host_context_binding(request, &host()).unwrap();
    let now = current_time();
    HostEvidence {
        protocol_version: 1,
        binding,
        observed_at_ms: now,
        expires_at_ms: now + 60_000,
        rules: vec![],
        tools: vec![],
    }
}

#[test]
fn reports_do_not_follow_a_request_to_a_different_linked_worktree() {
    let (temp, s) = fixture();
    let repo = temp.path().join("repo");
    let linked = temp.path().join("linked");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("README.md"), "fixture").unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "fixture",
    ]);
    git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    s.project_source_add(
        "##1",
        2,
        None,
        SourceLocation::Git {
            worktree: &repo,
            relative_path: ".",
        },
    )
    .unwrap();
    let mut req = request();
    req.source_id = 2;
    req.context.worktree = Some(&repo);
    let r = report(&s, &req);
    req.context.worktree = Some(&linked);
    assert_ne!(
        r.binding.scope_sha256,
        s.host_context_binding(&req, &host()).unwrap().scope_sha256
    );
    assert!(s.assess_task_host_evidence(&req, &host(), &r).is_err());
}

#[test]
fn binding_is_read_from_database_and_request_not_from_report() {
    let (temp, s) = fixture();
    let req = request();
    let r = report(&s, &req);
    let before = s.task_show("#1").unwrap().data;
    let result = s.assess_task_host_evidence(&req, &host(), &r).unwrap();
    assert!(!result.host_claim_verified && !result.reuse_allowed);
    assert_eq!(before, s.task_show("#1").unwrap().data);
    let mut forged = r.clone();
    forged.binding.scope_sha256 = "f".repeat(64);
    assert!(s.assess_task_host_evidence(&req, &host(), &forged).is_err());
    let another = HostInstance {
        instance_id: "instance-2",
        ..host()
    };
    assert!(s.assess_task_host_evidence(&req, &another, &r).is_err());
    // Same records and source files in a copied database are not the same authority store.
    let copy = temp.path().join("copy.db");
    fs::copy(s.database_path(), &copy).unwrap();
    let other = Service::new(copy);
    assert_ne!(
        r.binding.scope_sha256,
        other
            .host_context_binding(&req, &host())
            .unwrap()
            .scope_sha256
    );
    assert!(other.assess_task_host_evidence(&req, &host(), &r).is_err());
}

#[test]
fn task_versions_sessions_projects_and_closed_tasks_cannot_replay_reports() {
    let (temp, s) = fixture();
    let mut req = request();
    let r = report(&s, &req);
    req.session_id = "other";
    assert!(s.host_context_binding(&req, &host()).is_err());
    req.session_id = "s1";
    req.task_version = 2;
    assert_eq!(
        s.host_context_binding(&req, &host()).unwrap_err().body.code,
        "VERSION_CONFLICT"
    );
    req.task_version = 3;
    s.project_create("Mailroom").unwrap();
    s.project_source_add(
        "##2",
        1,
        None,
        SourceLocation::Directory(&temp.path().join("source")),
    )
    .unwrap();
    req.source_id = 2;
    assert_eq!(
        s.host_context_binding(&req, &host()).unwrap_err().body.code,
        "NOT_FOUND"
    );
    req.source_id = 1;
    s.task_create_minimal().unwrap();
    s.task_claim("#2", 1, "s2", false).unwrap();
    req.task = "#2";
    req.task_version = 2;
    req.session_id = "s2";
    assert!(s.host_context_binding(&req, &host()).is_err());
    s.task_set_project("#2", 2, Some("##1"), true, "fixture").unwrap();
    req.task_version = 3;
    assert!(s.assess_task_host_evidence(&req, &host(), &r).is_err());
    s.session_close("s2", 3).unwrap();
    req.task_version = 4;
    assert!(s.host_context_binding(&req, &host()).is_err());
    s.task_close("#1", 3, "cancelled", Some("fixture")).unwrap();
    req = request();
    req.task_version = 4;
    assert!(s.host_context_binding(&req, &host()).is_err());
}

#[test]
fn selected_components_and_rendering_parameters_are_bound() {
    let (temp, s) = fixture();
    let mut req = request();
    let before = report(&s, &req);
    req.context.budget_bytes = 4000;
    assert_ne!(
        before.binding.scope_sha256,
        s.host_context_binding(&req, &host()).unwrap().scope_sha256
    );
    assert!(s.assess_task_host_evidence(&req, &host(), &before).is_err());
    let files = ["sample.rs".into()];
    req.context.files = &files;
    assert_ne!(
        before.binding.scope_sha256,
        s.host_context_binding(&req, &host()).unwrap().scope_sha256
    );
    s.project_component_add("##1", 2, "frontend").unwrap();
    s.project_component_add("##1", 3, "backend").unwrap();
    s.project_source_add(
        "##1",
        4,
        Some("backend"),
        SourceLocation::Directory(&temp.path().join("source")),
    )
    .unwrap();
    s.task_set_components("#1", 3, &["frontend".into()], true, "fixture")
        .unwrap();
    req.task_version = 4;
    req.source_id = 2;
    assert!(s.host_context_binding(&req, &host()).is_err());
    req.source_id = 1; // Project-wide, component-free sources remain explicit shared inputs.
    assert!(s.host_context_binding(&req, &host()).is_ok());
}

#[test]
fn changes_during_assessment_are_rejected_after_file_io() {
    for task_change in [false, true] {
        let (temp, s) = fixture();
        let mut req = request();
        let files = ["sample.rs".into()];
        req.context.files = &files;
        let r = report(&s, &req);
        let calls = Cell::new(0);
        let result = s.assess_task_host_evidence_with_clock(&req, &host(), &r, || {
            let n = calls.get();
            calls.set(n + 1);
            if n == 2 {
                if task_change {
                    s.task_note("#1", 3, "progress", "fixture mutation")
                        .unwrap();
                } else {
                    fs::write(temp.path().join("source/sample.rs"), "changed source").unwrap();
                }
            }
            r.observed_at_ms + 1
        });
        assert!(result.is_err());
        if task_change {
            assert_eq!(result.unwrap_err().body.code, "VERSION_CONFLICT");
        }
    }
}

#[test]
fn expiry_and_clock_reversal_during_final_context_recheck_fail_closed() {
    let (_temp, s) = fixture();
    let req = request();
    let r = report(&s, &req);
    for backwards in [false, true] {
        let calls = Cell::new(0);
        assert!(
            s.assess_task_host_evidence_with_clock(&req, &host(), &r, || {
                let n = calls.get();
                calls.set(n + 1);
                if n == 3 {
                    if backwards {
                        r.observed_at_ms
                    } else {
                        r.expires_at_ms
                    }
                } else {
                    r.observed_at_ms + 1
                }
            })
            .is_err()
        );
    }
}
