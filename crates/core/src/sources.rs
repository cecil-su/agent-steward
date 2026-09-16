use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentView {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub created_at: String,
}

/// A recorded source directory, not an assertion about the local filesystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRootView {
    pub id: i64,
    pub project_id: i64,
    pub component_id: Option<i64>,
    pub directory_path: String,
    pub created_at: String,
}

/// Validate portable path *text* without accessing or canonicalizing the directory.
/// A record can refer to a different machine or an unavailable drive.
pub fn validate_source_directory(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let absolute = value.starts_with('/')
        || value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'));
    if !absolute || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(
            "expected an absolute directory path without control characters (metadata only)".into(),
        );
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_paths_are_portable_metadata_not_local_observations() {
        for path in [
            "/missing/project",
            "E:/ai/项目",
            "C:\\not-present\\code",
            "\\\\server\\share\\code",
        ] {
            assert_eq!(validate_source_directory(path).unwrap(), path);
        }
        for path in ["", ".", "relative/project", "C:relative", "/code\n"] {
            assert!(validate_source_directory(path).is_err(), "{path}");
        }
    }
}
