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
    s.task_set_project("#1", 1, Some("##1"), true, "fixture")
        .unwrap();
    s.task_claim("#1", 2, "s1", false).unwrap();
    (temp, s)
}
fn request<'a>() -> HostContextRequest<'a> {
    HostContextRequest {
        task: "#1",
        task_version: 3,
        session_id: "s1",
        source_id: 1,
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
fn task_versions_sessions_and_projects_cannot_replay_reports() {
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
    s.task_set_project("#2", 2, Some("##1"), true, "fixture")
        .unwrap();
    req.task_version = 3;
    assert!(s.assess_task_host_evidence(&req, &host(), &r).is_err());
    s.session_close("s2", 3).unwrap();
    req.task_version = 4;
    assert!(s.host_context_binding(&req, &host()).is_err());
}

#[test]
fn selected_components_and_source_registration_are_bound() {
    let (temp, s) = fixture();
    let mut req = request();
    let before = report(&s, &req);
    s.project_component_add("##1", 2, "frontend").unwrap();
    s.project_component_add("##1", 3, "backend").unwrap();
    s.project_source_add(
        "##1",
        4,
        Some("backend"),
        SourceLocation::Directory(&temp.path().join("source")),
    )
    .unwrap();
    assert!(s.assess_task_host_evidence(&req, &host(), &before).is_err());
    let project_source = s.host_context_binding(&req, &host()).unwrap();
    req.source_id = 2;
    assert_ne!(
        project_source.scope_sha256,
        s.host_context_binding(&req, &host()).unwrap().scope_sha256
    );
    s.task_set_components("#1", 3, &["frontend".into()], true, "fixture")
        .unwrap();
    req.task_version = 4;
    assert!(s.host_context_binding(&req, &host()).is_err());
    req.source_id = 1;
    assert!(s.host_context_binding(&req, &host()).is_ok());
}

#[test]
fn business_status_does_not_gate_a_current_session_binding() {
    let (_temp, s) = fixture();
    let mut req = request();
    for status in [
        "backlog",
        "todo",
        "in_progress",
        "in_review",
        "blocked",
        "done",
        "cancelled",
    ] {
        let changed = s.task_status("#1", req.task_version, status).unwrap();
        req.task_version = changed.data["task"]["version"].as_i64().unwrap();
        assert!(s.host_context_binding(&req, &host()).is_ok(), "{status}");
    }
}

#[test]
fn binding_does_not_read_source_files_or_require_a_live_source_directory() {
    let (temp, s) = fixture();
    let req = request();
    let r = report(&s, &req);
    fs::remove_dir_all(temp.path().join("source")).unwrap();
    assert_eq!(
        r.binding.scope_sha256,
        s.host_context_binding(&req, &host()).unwrap().scope_sha256
    );
    assert!(s.assess_task_host_evidence(&req, &host(), &r).is_ok());
}

#[test]
fn task_and_registration_changes_during_assessment_are_rejected() {
    for task_change in [false, true] {
        let (_temp, s) = fixture();
        let req = request();
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
                    s.project_component_add("##1", 2, "new component").unwrap();
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
