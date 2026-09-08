use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: i64,
    pub name: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Display names use NFC; only ASCII case is folded for identity.
/// This is not filesystem normalization or fuzzy/locale-dependent matching.
pub fn normalize_project_name(value: &str) -> Result<(String, String), String> {
    let name: String = value.trim().nfc().collect();
    if name.is_empty() || name.chars().count() > 120 {
        return Err("name must contain 1 to 120 characters after trimming".into());
    }
    if name.chars().any(char::is_control) {
        return Err("name must not contain control characters".into());
    }
    if name.starts_with('#') || name.bytes().all(|c| c.is_ascii_digit()) {
        return Err(
            "names starting with # or consisting of ASCII digits are reserved for references"
                .into(),
        );
    }
    let key = name.to_ascii_lowercase();
    Ok((name, key))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectReference {
    Id(i64),
    NameKey(String),
}

impl ProjectReference {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        let digits = if let Some(digits) = value.strip_prefix("##") {
            Some(digits)
        } else if value.starts_with('#') {
            return Err("project references use ##id; #id refers to a task".into());
        } else if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) {
            Some(value)
        } else {
            None
        };
        if let Some(digits) = digits {
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return Err("## must be followed by a positive numeric project id".into());
            }
            let id = digits
                .parse::<i64>()
                .map_err(|_| "project id is out of range")?;
            if id <= 0 {
                return Err("project id must be positive".into());
            }
            Ok(Self::Id(id))
        } else {
            normalize_project_name(value).map(|(_, key)| Self::NameKey(key))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_normalized_without_fuzzy_matching() {
        assert_eq!(
            normalize_project_name("  Mailroom  ").unwrap(),
            ("Mailroom".into(), "mailroom".into())
        );
        assert_eq!(normalize_project_name("Cafe\u{301}").unwrap().0, "Café");
        assert_eq!(
            ProjectReference::parse("MAILROOM").unwrap(),
            ProjectReference::NameKey("mailroom".into())
        );
        assert_ne!(
            ProjectReference::parse("Mail room").unwrap(),
            ProjectReference::parse("Mailroom").unwrap()
        );
        assert!(normalize_project_name("订单系统").is_ok());
        for value in ["", " ", "123", "#34", "##3", "bad\nname"] {
            assert!(normalize_project_name(value).is_err(), "{value:?}");
        }
        assert!(normalize_project_name(&"中".repeat(121)).is_err());
    }

    #[test]
    fn project_and_task_references_are_distinct() {
        for value in ["3", "##3", " ##003 "] {
            assert_eq!(
                ProjectReference::parse(value).unwrap(),
                ProjectReference::Id(3)
            );
        }
        for value in [
            "#3",
            "##",
            "###3",
            "##0",
            "0",
            "##-1",
            "##3x",
            "##9223372036854775808",
        ] {
            assert!(ProjectReference::parse(value).is_err(), "{value}");
        }
    }
}
