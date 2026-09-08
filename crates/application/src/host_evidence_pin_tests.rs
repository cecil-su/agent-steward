use super::*;
use std::fs;
use steward_core::RuleFileEvidence;

fn report(path: &Path, present: bool) -> HostEvidence {
    HostEvidence {
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
        rules: vec![RuleFileEvidence {
            path: path.to_str().unwrap().into(),
            sha256: present.then(|| sha(b"fixture")),
            object_sha256: present.then(|| {
                sha(&serde_json::to_vec(&git_adapter::identify_existing(path).unwrap()).unwrap())
            }),
        }],
        tools: vec![],
    }
}

#[test]
fn replacements_between_rule_passes_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let path = root.join("rule.md");
    fs::write(&path, "fixture").unwrap();
    let r = report(&path, true);
    assert!(
        assess_host_evidence_between_passes(
            &r,
            &r.binding,
            || 1500,
            || {
                fs::rename(&path, root.join("held.md")).unwrap();
                fs::write(&path, "fixture").unwrap();
            }
        )
        .is_err()
    );
}

#[test]
fn missing_rule_ancestor_is_compared_across_passes() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let parent = root.join("scope");
    fs::create_dir(&parent).unwrap();
    let r = report(&parent.join("absent.md"), false);
    assert!(assess_host_evidence(&r, &r.binding, || 1500).is_ok());
    assert!(
        assess_host_evidence_between_passes(
            &r,
            &r.binding,
            || 1500,
            || {
                fs::rename(&parent, root.join("held-scope")).unwrap();
                fs::create_dir(&parent).unwrap();
            }
        )
        .is_err()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn rules_remain_pinned_through_the_gap_between_passes() {
    let temp = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temp.path()).unwrap().join("rule.md");
    fs::write(&path, "fixture").unwrap();
    let r = report(&path, true); // Report creation retains no live identity.
    let original = git_adapter::identify_existing(&path).unwrap().record();
    assert!(
        assess_host_evidence_between_passes(
            &r,
            &r.binding,
            || 1500,
            || {
                for _ in 0..128 {
                    fs::remove_file(&path).unwrap();
                    fs::write(&path, "fixture").unwrap();
                    assert_ne!(
                        original,
                        git_adapter::identify_existing(&path).unwrap().record()
                    );
                }
            }
        )
        .is_err()
    );
}
