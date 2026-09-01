mod db;
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

#[derive(Debug, Clone)]
pub struct AppError {
    pub body: ErrorBody,
    pub exit_code: i32,
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

    pub fn partial(
        repository_path: Option<&str>,
        worktree_path: Option<&str>,
        git_state: Value,
        recommended_command: &str,
    ) -> Self {
        Self::new(
            "PARTIAL_EXTERNAL_STATE",
            "Git may have changed but the database was not updated",
            false,
            json!({
                "repositoryPath": repository_path,
                "worktreePath": worktree_path,
                "gitState": git_state,
                "recommendedCommand": recommended_command,
            }),
            6,
        )
    }

    pub fn from_storage(error: storage_sqlite::StorageError) -> Self {
        match error {
            storage_sqlite::StorageError::UnsupportedSchema {
                database_version,
                max_supported_version,
            } => Self::new(
                "UNSUPPORTED_SCHEMA_VERSION",
                "database schema is newer than this taskctl build",
                false,
                json!({
                    "databaseVersion": database_version,
                    "maxSupportedVersion": max_supported_version,
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

    fn connection(&self) -> AppResult<rusqlite::Connection> {
        storage_sqlite::open_database(&self.database_path).map_err(AppError::from_storage)
    }
}

pub fn warning(code: &str, message: &str, details: Value) -> Warning {
    Warning {
        code: code.to_owned(),
        message: message.to_owned(),
        details,
    }
}
