use sha2::{Digest, Sha256};
use std::{cell::Cell, fs};
use steward_application::assess_host_evidence;
use steward_core::{HostBinding, HostEvidence, RuleFileEvidence};
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn fixture() -> (tempfile::TempDir, HostEvidence) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let path = root.join("AGENTS.md");
    fs::write(&path, "fixture rule").unwrap();
    let identity = git_adapter::identify_existing(&path).unwrap();
    let r = HostEvidence {
        protocol_version: 1,
        binding: HostBinding {
            host: "fixture".into(),
            host_version: "1".into(),
            instance_id: "instance".into(),
            session_id: "session".into(),
            scope_sha256: "a".repeat(64),
        },
        observed_at_ms: 1000,
        expires_at_ms: 2000,
        rules: vec![
            RuleFileEvidence {
                path: path.to_str().unwrap().into(),
                sha256: Some(sha(b"fixture rule")),
                object_sha256: Some(sha(&serde_json::to_vec(&identity).unwrap())),
            },
            RuleFileEvidence {
                path: root.join("missing.md").to_str().unwrap().into(),
                sha256: None,
                object_sha256: None,
            },
        ],
        tools: vec![],
    };
    (temp, r)
}
#[test]
fn matching_files_are_not_host_authentication_or_reuse_permission() {
    let (_temp, r) = fixture();
    let out = assess_host_evidence(&r, &r.binding, || 1500).unwrap();
    assert_eq!(out.checked_rule_files, 1);
    assert_eq!(out.checked_missing_paths, 1);
    assert!(!out.host_claim_verified && !out.reuse_allowed);
    let mut reordered = r.clone();
    reordered.rules.reverse();
    assert_ne!(
        out.report_sha256,
        assess_host_evidence(&reordered, &r.binding, || 1500)
            .unwrap()
            .report_sha256
    );
    let json = serde_json::to_string(&out).unwrap();
    assert!(!json.contains("fixture rule") && !json.contains("AGENTS.md"));
    let mut binding = r.binding.clone();
    binding.session_id = "other".into();
    assert!(assess_host_evidence(&r, &binding, || 1500).is_err());
}
#[test]
fn rule_edits_replacements_and_new_missing_files_are_rejected() {
    let (_temp, r) = fixture();
    let path = std::path::Path::new(&r.rules[0].path);
    fs::write(path, "edited rule").unwrap();
    assert!(assess_host_evidence(&r, &r.binding, || 1500).is_err());
    fs::write(path, "fixture rule").unwrap();
    fs::write(&r.rules[1].path, "appeared").unwrap();
    assert!(assess_host_evidence(&r, &r.binding, || 1500).is_err());
    fs::remove_file(&r.rules[1].path).unwrap();
    fs::rename(path, path.with_extension("held")).unwrap();
    fs::write(path, "fixture rule").unwrap();
    assert!(assess_host_evidence(&r, &r.binding, || 1500).is_err());
}
#[test]
fn freshness_is_checked_again_after_io_and_input_limits_fail_closed() {
    let (_temp, r) = fixture();
    for end in [1400, 2000] {
        let calls = Cell::new(0);
        assert!(
            assess_host_evidence(&r, &r.binding, || {
                let n = calls.get();
                calls.set(n + 1);
                if n == 0 { 1500 } else { end }
            })
            .is_err()
        );
    }
    fs::write(&r.rules[0].path, vec![b'x'; 256 * 1024 + 1]).unwrap();
    assert!(assess_host_evidence(&r, &r.binding, || 1500).is_err());
    let mut duplicate = r.clone();
    duplicate.rules.push(r.rules[1].clone());
    assert!(duplicate.validate(&r.binding, 1500).is_err());
    let mut traversal = r.clone();
    traversal.rules[1].path = format!(
        "{}/missing/../absent.md",
        std::path::Path::new(&r.rules[0].path)
            .parent()
            .unwrap()
            .display()
    );
    assert!(traversal.validate(&r.binding, 1500).is_err());
    let mut unknown = serde_json::to_value(&r).unwrap();
    unknown["binding"]["trusted"] = serde_json::json!(true);
    assert!(serde_json::from_value::<HostEvidence>(unknown).is_err());
}
