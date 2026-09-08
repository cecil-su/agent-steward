//! Host reports are claims, not authentication, Task ownership or reuse permission.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostBinding {
    pub host: String,
    pub host_version: String,
    pub instance_id: String,
    pub session_id: String,
    /// Computed independently by the caller from the current request's project/source/worktree scope.
    pub scope_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleFileEvidence {
    /// Canonical absolute local path. No URI, inline text or credentials.
    pub path: String,
    /// Both hashes absent means an explicitly observed missing file, not a loaded rule.
    pub sha256: Option<String>,
    /// SHA-256 of serialized git-adapter ExistingPathIdentity on this machine.
    pub object_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAvailability {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolEvidence {
    pub name: String,
    pub availability: ToolAvailability,
    pub version: Option<String>,
    pub configuration_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostEvidence {
    pub protocol_version: u32,
    pub binding: HostBinding,
    pub observed_at_ms: u64,
    pub expires_at_ms: u64,
    /// Reported order is significant. This list does not prove scope/conditions or completeness.
    pub rules: Vec<RuleFileEvidence>,
    pub tools: Vec<ToolEvidence>,
}

fn text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl HostBinding {
    pub fn validate(&self) -> Result<(), &'static str> {
        if ![
            &self.host,
            &self.host_version,
            &self.instance_id,
            &self.session_id,
        ]
        .iter()
        .all(|s| text(s, 160))
            || !digest(&self.scope_sha256)
        {
            return Err("invalid host binding");
        }
        Ok(())
    }
}

impl HostEvidence {
    /// Expected binding and current time must come from the current caller, never from this report.
    /// Success means well-formed and bound, not that the host loaded rules or tools were probed.
    pub fn validate(&self, expected: &HostBinding, now_ms: u64) -> Result<(), &'static str> {
        if self.protocol_version != 1 {
            return Err("unsupported host evidence protocol");
        }
        let b = &self.binding;
        b.validate()?;
        if b != expected {
            return Err("host evidence binding mismatch");
        }
        if self.observed_at_ms > now_ms
            || self.expires_at_ms <= now_ms
            || self.expires_at_ms <= self.observed_at_ms
            || self.expires_at_ms - self.observed_at_ms > 300_000
        {
            return Err("host evidence is future-dated, expired or exceeds five minutes");
        }
        if self.rules.len() > 64 || self.tools.len() > 64 {
            return Err("too many host evidence entries");
        }
        let mut paths = BTreeSet::new();
        for rule in &self.rules {
            if !text(&rule.path, 4096)
                || !std::path::Path::new(&rule.path).is_absolute()
                || rule
                    .path
                    .split(['/', '\\'])
                    .any(|part| matches!(part, "." | ".."))
                || !paths.insert(&rule.path)
            {
                return Err("invalid or duplicate rule path");
            }
            match (&rule.sha256, &rule.object_sha256) {
                (None, None) => {}
                (Some(hash), Some(object)) if digest(hash) && digest(object) => {}
                _ => return Err("rule hashes must be a valid pair"),
            }
        }
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            if !text(&tool.name, 160)
                || !names.insert(&tool.name)
                || tool.version.as_deref().is_some_and(|v| !text(v, 160))
                || tool
                    .configuration_sha256
                    .as_deref()
                    .is_some_and(|v| !digest(v))
            {
                return Err("invalid or duplicate tool evidence");
            }
            if tool.availability == ToolAvailability::Available
                && (tool.version.is_none() || tool.configuration_sha256.is_none())
            {
                return Err("available tool requires version and configuration fingerprint");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> HostEvidence {
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
            rules: vec![],
            tools: vec![],
        }
    }
    #[test]
    fn versions_binding_and_freshness_fail_closed() {
        let r = report();
        let expected = r.binding.clone();
        assert!(r.validate(&expected, 1500).is_ok());
        for now in [999, 2000, u64::MAX] {
            assert!(r.validate(&expected, now).is_err());
        }
        for field in 0..5 {
            let mut other = expected.clone();
            match field {
                0 => other.host = "other".into(),
                1 => other.host_version = "2".into(),
                2 => other.instance_id = "other".into(),
                3 => other.session_id = "other".into(),
                _ => other.scope_sha256 = "b".repeat(64),
            }
            assert!(r.validate(&other, 1500).is_err());
        }
        let mut r = r;
        r.protocol_version = 2;
        assert!(r.validate(&expected, 1500).is_err());
        r.protocol_version = 1;
        r.expires_at_ms = 301001;
        assert!(r.validate(&expected, 1500).is_err());
    }
    #[test]
    fn unknown_fields_and_incomplete_tool_claims_are_rejected() {
        let mut r = report();
        let expected = r.binding.clone();
        r.tools.push(ToolEvidence {
            name: "compiler".into(),
            availability: ToolAvailability::Available,
            version: None,
            configuration_sha256: None,
        });
        assert!(r.validate(&expected, 1500).is_err());
        r.tools[0].availability = ToolAvailability::Unknown;
        assert!(r.validate(&expected, 1500).is_ok());
        let mut json = serde_json::to_value(r).unwrap();
        json["complete"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HostEvidence>(json).is_err());
    }
}
