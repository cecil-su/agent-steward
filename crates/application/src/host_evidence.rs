//! Explicit host-neutral assessment. No CLI/Pi ingestion or persistence, no authority upgrade.
#[cfg(test)]
#[path = "host_evidence_pin_tests.rs"]
mod pin_tests;

use crate::AppError;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
use steward_core::{HostBinding, HostEvidence};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostEvidenceAssessment {
    pub report_sha256: String,
    pub checked_rule_files: usize,
    pub checked_missing_paths: usize,
    pub reported_tools: usize,
    pub host_claim_verified: bool,
    pub reuse_allowed: bool,
}
fn invalid(message: &str) -> AppError {
    AppError::invalid("hostEvidence", message)
}
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Caller supplies an independently obtained expected binding and live clock.
/// A matching local file does NOT prove it was loaded by a host, nor prove rule completeness.
/// Callers must bound the transport before deserialization (suggested maximum: 128 KiB).
pub fn assess_host_evidence(
    report: &HostEvidence,
    expected: &HostBinding,
    clock: impl Fn() -> u64,
) -> Result<HostEvidenceAssessment, AppError> {
    assess_host_evidence_between_passes(report, expected, clock, || {})
}

fn assess_host_evidence_between_passes(
    report: &HostEvidence,
    expected: &HostBinding,
    clock: impl Fn() -> u64,
    mut between_passes: impl FnMut(),
) -> Result<HostEvidenceAssessment, AppError> {
    let started = clock();
    report.validate(expected, started).map_err(invalid)?;
    let mut present = 0;
    let mut missing = 0;
    // A serialized object hash cannot pin an inode. Keep first-pass observations alive
    // until the second pass has compared every rule (including missing-rule ancestors).
    let mut first_pass_pins = Vec::with_capacity(report.rules.len());
    for pass in 0..2 {
        let mut total = 0;
        for (index, rule) in report.rules.iter().enumerate() {
            let path = Path::new(&rule.path);
            match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && rule.sha256.is_none() => {
                    // The nearest existing ancestor must itself be canonical, not an alias.
                    let mut ancestor =
                        path.parent().ok_or_else(|| invalid("rule has no parent"))?;
                    loop {
                        match std::fs::symlink_metadata(ancestor) {
                            Ok(meta) if meta.file_type().is_dir() => break,
                            Ok(_) => {
                                return Err(invalid(
                                    "missing rule ancestor is not a direct directory",
                                ));
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                ancestor = ancestor.parent().ok_or_else(|| {
                                    invalid("missing rule has no existing ancestor")
                                })?
                            }
                            Err(_) => return Err(invalid("cannot inspect missing rule ancestor")),
                        }
                    }
                    let identity = git_adapter::identify_existing(ancestor)
                        .map_err(|e| AppError::from_git(e, None))?;
                    if identity.canonical_path != ancestor {
                        return Err(invalid("missing rule path uses an alias"));
                    }
                    if pass == 0 {
                        missing += 1;
                        first_pass_pins.push(identity);
                    } else if first_pass_pins[index] != identity {
                        return Err(invalid("missing rule ancestor changed between passes"));
                    }
                    continue;
                }
                Err(_) => return Err(invalid("rule file cannot be inspected or disappeared")),
                Ok(meta) => {
                    if !meta.file_type().is_file() || rule.sha256.is_none() {
                        return Err(invalid("rule appeared or is not a direct regular file"));
                    }
                }
            }
            let identity =
                git_adapter::identify_existing(path).map_err(|e| AppError::from_git(e, None))?;
            if identity.canonical_path != path
                || Some(sha(
                    &serde_json::to_vec(&identity).expect("identity serializes")
                )) != rule.object_sha256
            {
                return Err(invalid("rule file object or canonical path changed"));
            }
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(|_| invalid("cannot read rule file"))?
                .take(256 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| invalid("cannot read rule file"))?;
            total += bytes.len();
            if bytes.len() > 256 * 1024 || total > 4 * 1024 * 1024 {
                return Err(invalid("rule input exceeds 256 KiB/file or 4 MiB/pass"));
            }
            if Some(sha(&bytes)) != rule.sha256 {
                return Err(invalid("loaded rule bytes differ from current file"));
            }
            git_adapter::verify_existing_identity(&identity)
                .map_err(|e| AppError::from_git(e, None))?;
            if pass == 0 {
                present += 1;
                first_pass_pins.push(identity);
            } else if first_pass_pins[index] != identity {
                return Err(invalid("rule object changed between passes"));
            }
        }
        if pass == 0 {
            between_passes();
        }
    }
    let ended = clock();
    if ended < started {
        return Err(invalid(
            "clock moved backwards during host evidence assessment",
        ));
    }
    report.validate(expected, ended).map_err(invalid)?;
    Ok(HostEvidenceAssessment {
        report_sha256: sha(&serde_json::to_vec(report).expect("report serializes")),
        checked_rule_files: present,
        checked_missing_paths: missing,
        reported_tools: report.tools.len(),
        host_claim_verified: false,
        reuse_allowed: false,
    })
}
