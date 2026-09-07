mod db;
mod hooks;
mod migration;
pub use hooks::HookEventInput;
mod permissions;
pub use permissions::database_permission_warning;
mod sessions;
mod tasks;
mod worktrees;

use std::path::{Path, PathBuf};

use git_adapter::GitError;
use rusqlite::ErrorCode;
use serde::Serialize;
use serde_json::{Value, json};
use steward_core::Warning;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub details: Value,
}

#[derive(Debug, Clone, Default)]
pub struct TaskListOptions {
    pub status: Option<String>,
    pub task_key: Option<String>,
    pub query: Option<String>,
    pub page_size: Option<u32>,
    pub cursor: Option<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub body: ErrorBody,
    pub exit_code: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct RecoveryCommand {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PartialDatabaseState {
    Unchanged,
    Updated,
    Unknown,
}

impl PartialDatabaseState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Updated => "updated",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.body.code, self.body.message)
    }
}

impl std::error::Error for AppError {}

impl AppError {
    pub fn new(
        code: &str,
        message: impl Into<String>,
        retryable: bool,
        details: Value,
        exit_code: i32,
    ) -> Self {
        Self {
            body: ErrorBody {
                code: code.to_owned(),
                message: message.into(),
                retryable,
                details,
            },
            exit_code,
        }
    }

    pub fn invalid(field: &str, reason: impl Into<String>) -> Self {
        Self::new(
            "INVALID_INPUT",
            "input validation failed",
            false,
            json!({"field": field, "reason": reason.into()}),
            2,
        )
    }

    pub fn not_found(entity_type: &str, id: &str) -> Self {
        Self::new(
            "NOT_FOUND",
            format!("{entity_type} was not found"),
            false,
            json!({"entityType": entity_type, "id": id}),
            2,
        )
    }

    pub fn version(expected: i64, current: i64) -> Self {
        Self::new(
            "VERSION_CONFLICT",
            "task version does not match",
            true,
            json!({"expectedVersion": expected, "currentVersion": current}),
            4,
        )
    }

    pub fn session(current: Option<&str>, requested: &str) -> Self {
        Self::new(
            "SESSION_CONFLICT",
            "session cannot become the current task session",
            false,
            json!({"currentSessionId": current, "requestedSessionId": requested}),
            4,
        )
    }

    pub fn constraint(constraint: &str) -> Self {
        Self::new(
            "CONSTRAINT_VIOLATION",
            "database constraint was rejected",
            false,
            json!({"constraint": constraint}),
            4,
        )
    }

    pub fn worktree_safety(reason: impl Into<String>, path: Option<&str>) -> Self {
        Self::new(
            "WORKTREE_SAFETY_REFUSED",
            "worktree safety check refused the operation",
            false,
            json!({"reason": reason.into(), "worktreePath": path}),
            5,
        )
    }

    pub(crate) fn partial(
        repository_path: Option<&str>,
        worktree_path: Option<&str>,
        git_state: Value,
        database_state: PartialDatabaseState,
        recovery: RecoveryCommand,
    ) -> Self {
        Self::partial_with_diagnostics(
            repository_path,
            worktree_path,
            git_state,
            database_state,
            recovery,
            Value::Null,
        )
    }

    pub(crate) fn partial_with_diagnostics(
        repository_path: Option<&str>,
        worktree_path: Option<&str>,
        git_state: Value,
        database_state: PartialDatabaseState,
        recovery: RecoveryCommand,
        diagnostics: Value,
    ) -> Self {
        Self::new(
            "PARTIAL_EXTERNAL_STATE",
            "Git and database state require reconciliation",
            false,
            json!({
                "repositoryPath": repository_path,
                "worktreePath": worktree_path,
                "gitState": git_state,
                "databaseState": database_state.as_str(),
                "diagnostics": diagnostics,
                "recommendedCommand": recovery.command,
                "recommendedArgs": recovery.args,
            }),
            6,
        )
    }

    pub fn from_storage(error: storage_sqlite::StorageError) -> Self {
        match error {
            storage_sqlite::StorageError::UnsupportedSchema {
                database_version,
                supported_version,
            } => Self::new(
                "UNSUPPORTED_SCHEMA_VERSION",
                "database schema is incompatible; use a new database for this build",
                false,
                json!({
                    "databaseVersion": database_version,
                    "supportedVersion": supported_version,
                }),
                2,
            ),
            storage_sqlite::StorageError::Sqlite(error) => Self::from_sqlite(error),
            storage_sqlite::StorageError::Io(error) => Self::new(
                "DATABASE_UNAVAILABLE",
                "database path is unavailable",
                false,
                json!({"reason": error.to_string()}),
                10,
            ),
        }
    }

    pub fn from_sqlite(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::FromSqlConversionFailure(column, data_type, reason) = &error {
            return Self::new(
                "DATABASE_UNAVAILABLE",
                "stored database data is invalid",
                false,
                json!({
                    "reason": reason.to_string(),
                    "columnIndex": column,
                    "sqliteType": format!("{data_type:?}"),
                }),
                10,
            );
        }
        if let rusqlite::Error::SqliteFailure(failure, message) = &error {
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) {
                return Self::new(
                    "DATABASE_BUSY",
                    "database remained busy past the timeout",
                    true,
                    json!({"timeoutMs": 5000}),
                    4,
                );
            }
            if failure.code == ErrorCode::ConstraintViolation {
                return Self::new(
                    "CONSTRAINT_VIOLATION",
                    "database constraint was rejected",
                    false,
                    json!({"constraint": message.clone().unwrap_or_else(|| failure.to_string())}),
                    4,
                );
            }
        }
        Self::new(
            "DATABASE_UNAVAILABLE",
            "database operation failed",
            false,
            json!({"reason": error.to_string()}),
            10,
        )
    }

    pub fn from_git(error: GitError, input_path: Option<&str>) -> Self {
        match error {
            GitError::PathIdentity(reason) => Self::new(
                "PATH_IDENTITY_UNKNOWN",
                "path identity could not be established",
                false,
                json!({"inputPath": input_path, "reason": reason}),
                5,
            ),
            GitError::CommandFailed {
                operation,
                exit_status,
                summary,
            } => Self::new(
                "GIT_COMMAND_FAILED",
                "Git command failed without changing the observed worktree state",
                false,
                json!({
                    "operation": operation,
                    "exitStatus": exit_status,
                    "stderrSummary": summary,
                }),
                5,
            ),
            GitError::SafetyRefused(reason) => Self::worktree_safety(reason, input_path),
            GitError::OperationBusy => Self::new(
                "WORKTREE_OPERATION_BUSY",
                "another worktree operation is already running for this task",
                true,
                json!({"taskId": Value::Null, "operation": "worktree"}),
                4,
            ),
            GitError::Io(error) => Self::new(
                "GIT_COMMAND_FAILED",
                "Git adapter filesystem operation failed",
                false,
                json!({"operation": "filesystem", "exitStatus": Value::Null, "stderrSummary": error.to_string()}),
                5,
            ),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone)]
pub struct Outcome {
    pub data: Value,
    pub warnings: Vec<Warning>,
}

impl Outcome {
    pub fn new(data: Value) -> Self {
        Self {
            data,
            warnings: Vec::new(),
        }
    }

    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }
}

#[derive(Debug, Clone)]
pub struct Service {
    database_path: PathBuf,
    lock_root_override: Option<PathBuf>,
}

impl Service {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            lock_root_override: None,
        }
    }

    pub fn with_lock_root(mut self, lock_root: impl Into<PathBuf>) -> Self {
        self.lock_root_override = Some(lock_root.into());
        self
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub(crate) fn recovery_command<I, S>(&self, arguments: I) -> RecoveryCommand
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let database = storage_sqlite::canonical_database_path(&self.database_path)
            .unwrap_or_else(|_| self.database_path.clone());
        let mut command = vec![
            "taskctl".to_owned(),
            "--database".to_owned(),
            database.to_string_lossy().into_owned(),
        ];
        command.extend(arguments.into_iter().map(Into::into));
        RecoveryCommand {
            command: render_recovery_command(&command),
            args: command,
        }
    }

    fn connection(&self) -> AppResult<rusqlite::Connection> {
        storage_sqlite::open_database(&self.database_path).map_err(AppError::from_storage)
    }
}

#[cfg(windows)]
fn render_recovery_command(arguments: &[String]) -> String {
    let quoted = arguments
        .iter()
        .map(|argument| format!("'{}'", argument.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(" ");
    format!("& {quoted}")
}

#[cfg(not(windows))]
fn render_recovery_command(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| format!("'{}'", argument.replace('\'', "'\"'\"'")))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn warning(code: &str, message: &str, details: Value) -> Warning {
    Warning {
        code: code.to_owned(),
        message: message.to_owned(),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_error_reports_database_state_without_changing_command_type() {
        let error = AppError::partial(
            Some("repository"),
            Some("worktree"),
            json!("unknown"),
            PartialDatabaseState::Updated,
            RecoveryCommand {
                command: "taskctl doctor".to_owned(),
                args: vec!["taskctl".to_owned(), "doctor".to_owned()],
            },
        );

        assert_eq!(
            error.body.message,
            "Git and database state require reconciliation"
        );
        assert_eq!(error.body.details["databaseState"], "updated");
        assert_eq!(error.body.details["recommendedCommand"], "taskctl doctor");
        assert_eq!(
            error.body.details["recommendedArgs"],
            json!(["taskctl", "doctor"])
        );
        assert!(error.body.details["diagnostics"].is_null());
    }

    #[test]
    fn unprovable_git_state_is_unknown_with_diagnostics_in_a_separate_field() {
        let error = AppError::partial_with_diagnostics(
            Some("repository"),
            Some("worktree"),
            json!("unknown"),
            PartialDatabaseState::Updated,
            RecoveryCommand {
                command: "taskctl doctor".to_owned(),
                args: vec!["taskctl".to_owned(), "doctor".to_owned()],
            },
            json!({"phase": "afterDatabaseCommit", "observationError": "boom"}),
        );

        assert_eq!(error.body.details["gitState"], "unknown");
        assert_eq!(
            error.body.details["diagnostics"],
            json!({"phase": "afterDatabaseCommit", "observationError": "boom"})
        );
        assert!(
            error.body.details["gitState"].is_string(),
            "gitState must stay a stable string when the Git state cannot be proven"
        );
    }
}
