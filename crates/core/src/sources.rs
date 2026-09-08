use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentView {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRootView {
    pub id: i64,
    pub project_id: i64,
    pub component_id: Option<i64>,
    pub repository_id: Option<i64>,
    pub relative_path: Option<String>,
    pub directory_path: Option<String>,
    pub created_at: String,
}

/// A portable repository-relative directory, independent of a particular checkout.
pub fn validate_source_relative_path(value: &str) -> Result<String, String> {
    if value == "." {
        return Ok(value.into());
    }
    if value.is_empty()
        || value.len() > 4096
        || value.contains(['\\', ':'])
        || value.chars().any(char::is_control)
    {
        return Err("expected a slash-separated repository-relative directory (or .)".into());
    }
    if value.split('/').any(|part| {
        part.is_empty()
            || part == "."
            || part == ".."
            || part.eq_ignore_ascii_case(".git")
            || part.ends_with([' ', '.'])
    }) {
        return Err(
            "absolute paths, traversal, Git metadata and ambiguous path components are not allowed"
                .into(),
        );
    }
    Ok(value.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_paths_are_portable_not_normalized_into_another_location() {
        for path in [".", "apps/mailroom", "前端/src", "my project"] {
            assert!(validate_source_relative_path(path).is_ok());
        }
        for path in [
            "", "..", "../app", "/app", "a/../b", "./app", "a//b", "a/", "C:/app", "a\\b", ".git",
            "a/.GIT", "a/ ", "a.", "a\n",
        ] {
            assert!(validate_source_relative_path(path).is_err(), "{path}");
        }
    }
}
