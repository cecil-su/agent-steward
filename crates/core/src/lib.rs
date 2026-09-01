use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Blocked,
    Closed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
        }
    }
}

impl TryFrom<&str> for TaskStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "open" => Ok(Self::Open),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("unknown task status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub version: i64,
    pub goal: String,
    pub scope: String,
    pub acceptance_criteria: String,
    pub next_step: Option<String>,
    pub block_reason: Option<String>,
    pub block_recovery: Option<String>,
    pub current_session_id: Option<String>,
    pub repository_path: Option<String>,
    pub repository_common_dir: Option<String>,
    pub repository_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub latest_checkpoint_id: Option<String>,
    pub closure_outcome: Option<String>,
    pub closure_reason: Option<String>,
    pub closed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub task_id: String,
    pub source: Option<String>,
    pub external_session_id: Option<String>,
    pub continued_from: Option<String>,
    pub record_path: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointView {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub summary: String,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub pending: Vec<String>,
    pub next_step: String,
    pub risks: Vec<String>,
    pub git_head: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskNoteView {
    pub id: i64,
    pub task_id: String,
    pub session_id: Option<String>,
    pub note_type: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionImportView {
    pub id: String,
    pub session_id: String,
    pub source_path: String,
    pub media_type: Option<String>,
    pub sha256: String,
    pub size_bytes: i64,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub registered: bool,
    pub repository_path: Option<String>,
    pub repository_common_dir: Option<String>,
    pub path: Option<String>,
    pub exists: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub staged: Option<Vec<String>>,
    pub unstaged: Option<Vec<String>>,
    pub untracked: Option<Vec<String>>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub task_id: String,
    pub sequence: i64,
    pub change_type: String,
    pub session_id: Option<String>,
    pub occurred_at: String,
    pub summary: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCreateInput {
    pub title: String,
    pub goal: String,
    pub scope: String,
    pub acceptance_criteria: String,
    #[serde(default)]
    pub next_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointInput {
    pub summary: String,
    pub completed: Vec<String>,
    pub decisions: Vec<String>,
    pub pending: Vec<String>,
    pub next_step: String,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub code: String,
    pub message: String,
    pub details: Value,
}

pub fn require_non_empty(field: &str, value: &str) -> Result<String, (String, String)> {
    let value = value.trim();
    if value.is_empty() {
        Err((field.to_owned(), "must be a non-empty string".to_owned()))
    } else {
        Ok(value.to_owned())
    }
}

pub fn validate_string_array(field: &str, values: &[String]) -> Result<(), (String, String)> {
    if values.iter().any(|value| value.trim().is_empty()) {
        Err((
            field.to_owned(),
            "items must be non-empty strings".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/agent-steward"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|root| root.join("agent-steward"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(root).join("agent-steward"));
        }
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|home| home.join(".local/share/agent-steward"))
    }
}
