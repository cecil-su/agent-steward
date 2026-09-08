//! Disposable live navigation, not a cache of semantic facts or execution authority.
#[cfg(all(test, target_os = "linux"))]
#[path = "project_context_pin_tests.rs"]
mod pin_tests;
use crate::{AppError, Outcome, Service};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

// Change this when discovery, fingerprint coverage or derivation semantics change.
const CONTEXT_VERSION: u32 = 3;
const FILE_LIMIT: usize = 1024 * 1024;
const TOTAL_LIMIT: usize = 8 * FILE_LIMIT;
const NAVIGATION: &[&str] = &[
    "README.md",
    "CODEMAP.md",
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "go.mod",
    "go.sum",
    "Directory.Build.props",
    "global.json",
];

pub struct ProjectContextOptions<'a> {
    pub worktree: Option<&'a Path>,
    /// Relative to the resolved SourceRoot; bodies are returned only for these files.
    pub files: &'a [String],
    /// Relative to the checkout root (or directory-only source); hashes only.
    pub dependencies: &'a [String],
    /// Compact JSON data bytes, not model tokens or transport-envelope bytes.
    pub budget_bytes: usize,
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid("context", message)
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn relative(base: &Path, name: &str) -> Result<PathBuf, AppError> {
    steward_core::validate_source_relative_path(name).map_err(invalid)?;
    if name == "." {
        return Err(invalid("a file path is required"));
    }
    Ok(base.join(name))
}

#[derive(Clone)]
struct Observed {
    manifest: Value,
    body: Option<String>,
    // Retain the live Linux pin across BOTH passes, not only its serialized dev/ino.
    _identity: Option<git_adapter::ExistingPathIdentity>,
}

fn observe(
    path: &Path,
    kind: &str,
    required: bool,
    checkout: Option<&Path>,
    total: &mut usize,
    checked_parents: &mut BTreeSet<PathBuf>,
) -> Result<Observed, AppError> {
    git_adapter::GitReadControl::check_current().map_err(|e| AppError::from_git(e, None))?;
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => {
            return Ok(Observed {
                manifest: json!({"path":path,"kind":kind,"missing":true}),
                body: None,
                _identity: None,
            });
        }
        Err(e) => return Err(invalid(format!("cannot inspect {}: {e}", path.display()))),
        Ok(_) => {}
    }
    let identity = git_adapter::identify_existing(path).map_err(|e| AppError::from_git(e, None))?;
    // Reject file and ancestor aliases, including symlinks into another project's files.
    if identity.canonical_path != path || !path.is_file() {
        return Err(invalid(format!(
            "not a direct regular file: {}",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| invalid("file has no parent"))?;
    if let Some(root) = checkout.filter(|root| path.starts_with(root))
        && checked_parents.insert(parent.to_owned())
    {
        let actual = git_adapter::checkout_info(parent).map_err(|e| AppError::from_git(e, None))?;
        if actual.repository_path != root {
            return Err(invalid("file crosses a nested repository boundary"));
        }
    }
    if checkout.is_none()
        && checked_parents.insert(parent.to_owned())
        && git_adapter::directory_has_git_marker(parent).map_err(|e| AppError::from_git(e, None))?
    {
        return Err(invalid("directory-only file crosses a Git boundary"));
    }
    let file = File::open(path).map_err(|e| invalid(e.to_string()))?;
    let mut bytes = Vec::new();
    file.take((FILE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    *total += bytes.len();
    if bytes.len() > FILE_LIMIT || *total > TOTAL_LIMIT {
        return Err(invalid(
            "context input limit exceeded (1 MiB/file, 8 MiB/observation)",
        ));
    }
    git_adapter::verify_existing_identity(&identity).map_err(|e| AppError::from_git(e, None))?;
    let body = if kind == "requested" {
        Some(
            String::from_utf8(bytes.clone())
                .map_err(|_| invalid("requested snippets must be UTF-8"))?,
        )
    } else {
        None
    };
    Ok(Observed {
        manifest: json!({"path":path,"kind":kind,"sha256":hash(&bytes),"bytes":bytes.len(),"identity":identity}),
        body,
        _identity: Some(identity),
    })
}

impl Service {
    pub fn project_context(
        &self,
        reference: &str,
        source_id: i64,
        options: ProjectContextOptions<'_>,
    ) -> Result<Outcome, AppError> {
        if !(1024..=64000).contains(&options.budget_bytes)
            || options.files.len() + options.dependencies.len() > 64
        {
            return Err(invalid(
                "budget must be 1024..64000 bytes; at most 64 explicit files/dependencies",
            ));
        }
        let first = self
            .project_source_resolve(reference, source_id, options.worktree)?
            .data;
        let root = PathBuf::from(
            first["resolvedPath"]
                .as_str()
                .ok_or_else(|| invalid("source path missing"))?,
        );
        let root_identity =
            git_adapter::identify_existing(&root).map_err(|e| AppError::from_git(e, None))?;
        let checkout = if first["source"]["repositoryId"].is_null() {
            None
        } else {
            Some(git_adapter::checkout_info(&root).map_err(|e| AppError::from_git(e, None))?)
        };
        let git_before = checkout
            .as_ref()
            .map(git_adapter::context_git_state)
            .transpose()
            .map_err(|e| AppError::from_git(e, None))?;
        let base = checkout
            .as_ref()
            .map(|info| info.repository_path.as_path())
            .unwrap_or(&root);
        let mut candidates: BTreeMap<PathBuf, (&str, bool)> = BTreeMap::new();
        // Navigation only. Known ancestor filenames are not host rule evaluation.
        for ancestor in root.ancestors() {
            for name in ["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"] {
                candidates.insert(ancestor.join(name), ("rule-navigation", false));
            }
            if ancestor.starts_with(base) {
                for name in NAVIGATION {
                    candidates.insert(ancestor.join(name), ("navigation", false));
                }
            }
        }
        for (names, boundary, kind) in [
            (options.dependencies, base, "dependency"),
            (options.files, root.as_path(), "requested"),
        ] {
            for name in names {
                let path = relative(boundary, name)?;
                for ancestor in path
                    .parent()
                    .into_iter()
                    .flat_map(Path::ancestors)
                    .take_while(|dir| dir.starts_with(boundary))
                {
                    for rule in ["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"] {
                        candidates
                            .entry(ancestor.join(rule))
                            .or_insert(("rule-navigation", false));
                    }
                }
                candidates.insert(path, (kind, true));
            }
        }
        if candidates.len() > 256 {
            return Err(invalid("navigation candidate limit exceeded (256)"));
        }
        let mut total = 0;
        // This set lives for one pass only; the second pass rechecks every parent.
        let mut checked_parents = BTreeSet::new();
        let mut observed = Vec::new();
        for (path, (kind, required)) in &candidates {
            observed.push(observe(
                path,
                kind,
                *required,
                checkout.as_ref().map(|c| c.repository_path.as_path()),
                &mut total,
                &mut checked_parents,
            )?);
        }
        // Fail closed on observed changes; this is not an atomic filesystem/SQLite snapshot.
        let second = self
            .project_source_resolve(reference, source_id, options.worktree)?
            .data;
        if first["project"] != second["project"]
            || first["source"] != second["source"]
            || first["resolvedPath"] != second["resolvedPath"]
        {
            return Err(invalid(
                "source registration changed during context observation",
            ));
        }
        let first_boundary_checks = checked_parents.len();
        checked_parents.clear();
        total = 0;
        for ((path, (kind, required)), before) in candidates.iter().zip(&observed) {
            let after = observe(
                path,
                kind,
                *required,
                checkout.as_ref().map(|c| c.repository_path.as_path()),
                &mut total,
                &mut checked_parents,
            )?;
            if before.manifest != after.manifest {
                return Err(invalid(
                    "source files changed during context observation; retry with live files",
                ));
            }
        }
        git_adapter::verify_existing_identity(&root_identity)
            .map_err(|e| AppError::from_git(e, None))?;
        if let Some(info) = &checkout {
            git_adapter::verify_repository_identity(info)
                .map_err(|e| AppError::from_git(e, None))?;
        }
        let git_after = checkout
            .as_ref()
            .map(git_adapter::context_git_state)
            .transpose()
            .map_err(|e| AppError::from_git(e, None))?;
        if git_before != git_after {
            return Err(invalid(
                "Git state changed during context observation; retry",
            ));
        }
        let git_evidence = git_after.as_ref().map(|state| json!({"head":state.head,"branch":state.branch,"dirty":state.dirty,"hasIgnored":state.has_ignored,"statusSha256":hash(&state.status)}));
        let mut blockers = vec![
            "DEPENDENCY_COVERAGE_UNKNOWN",
            "HOST_RULES_UNOBSERVED",
            "ENVIRONMENT_UNOBSERVED",
            "PERSISTED_IDENTITY_CONTINUITY_UNPROVEN",
        ];
        match &git_after {
            None => blockers.push("NON_GIT_SOURCE"),
            Some(state) => {
                if state.head.is_none() {
                    blockers.push("UNBORN_HEAD");
                }
                if state.dirty {
                    blockers.push("DIRTY_WORKTREE");
                }
                if state.has_ignored {
                    blockers.push("IGNORED_CONTENT_UNCOVERED");
                }
            }
        }
        let manifest: Vec<_> = observed.iter().map(|entry| &entry.manifest).collect();
        let fingerprint = hash(&serde_json::to_vec(&json!({"contextVersion":CONTEXT_VERSION,"project":first["project"],"source":first["source"],"rootIdentity":root_identity,"checkout":checkout.as_ref().map(|c| (&c.repository_path,c.common_identity())),"files":manifest,"git":git_evidence})).expect("manifest serializes"));
        let mut present: Vec<_> = observed
            .into_iter()
            .filter(|entry| entry.manifest["missing"] != true)
            .collect();
        present.sort_by_key(|entry| match entry.manifest["kind"].as_str() {
            Some("rule-navigation") => 0,
            Some("requested") => 1,
            Some("dependency") => 2,
            _ => 3,
        });
        let mut entries = Vec::new();
        for entry in &present {
            let mut nav = entry.manifest.clone();
            nav.as_object_mut()
                .expect("manifest object")
                .remove("identity");
            nav["evidence"] = json!(if entry.body.is_some() {
                "source-prefix-v1"
            } else {
                "file-navigation-v1"
            });
            if let Some(body) = &entry.body {
                nav["snippet"] = json!("");
                nav["snippetTruncated"] = json!(!body.is_empty());
            }
            entries.push(nav);
        }
        let mut data = json!({"contextVersion":CONTEXT_VERSION,"semanticFactsVerified":false,"project":first["project"],"source":first["source"],"resolvedPath":root,"observedAt":storage_sqlite::now(),"atomicSnapshot":false,"fingerprint":fingerprint,"entries":entries,"omittedEntries":0,"budgetBytes":options.budget_bytes,"reuseAllowed":false,"reuseBlockers":blockers,"gitStateObserved":git_evidence.is_some(),"git":git_evidence,"observation":{"candidatePaths":candidates.len(),"fileReads":present.len()*2,"boundaryChecks":first_boundary_checks+checked_parents.len()},"coverage":"Observed files and Git status only; not complete dependencies, semantic facts, or host rules"});
        while serde_json::to_vec(&data).expect("data serializes").len() > options.budget_bytes {
            if data["entries"]
                .as_array_mut()
                .expect("entries")
                .pop()
                .is_none()
            {
                return Err(invalid("budget too small for source metadata"));
            }
            data["omittedEntries"] =
                json!(present.len() - data["entries"].as_array().expect("entries").len());
        }
        // Account for actual JSON escaping and UTF-8 boundaries, rather than guessing model tokens.
        for (index, entry) in present
            .iter()
            .take(data["entries"].as_array().expect("entries").len())
            .enumerate()
        {
            let Some(body) = &entry.body else { continue };
            let boundaries: Vec<_> = body
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(body.len()))
                .collect();
            let (mut lo, mut hi) = (0, boundaries.len());
            while lo + 1 < hi {
                let mid = (lo + hi) / 2;
                let end = boundaries[mid];
                data["entries"][index]["snippet"] = json!(&body[..end]);
                data["entries"][index]["snippetTruncated"] = json!(end < body.len());
                if serde_json::to_vec(&data).expect("data serializes").len() <= options.budget_bytes
                {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let end = boundaries[lo];
            data["entries"][index]["snippet"] = json!(&body[..end]);
            data["entries"][index]["snippetTruncated"] = json!(end < body.len());
        }
        Ok(Outcome::new(data))
    }
}
