mod db;
mod host_context;
pub use host_context::{HostContextRequest, HostInstance};
mod host_evidence;
pub use host_evidence::{HostEvidenceAssessment, assess_host_evidence};
mod hooks;
mod migration;
pub use hooks::HookEventInput;
mod permissions;
pub use permissions::database_permission_warning;
pub mod path_safety;
mod rules;
pub use rules::{MAX_RULE_BYTES, RuleContent, RuleInput, RuleSource, RuleView};
mod project_profiles;
pub use project_profiles::{ProjectProfileInput, ProjectProfileView};
mod projects;
mod sources;
pub use sources::SourceLocation;
mod sessions;
mod tasks;

use path_safety::PathError;
use rusqlite::ErrorCode;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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
    pub project: Option<String>,
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
            json!({"field":field,"reason":reason.into()}),
            2,
        )
    }
    pub fn not_found(entity_type: &str, id: &str) -> Self {
        Self::new(
            "NOT_FOUND",
            format!("{entity_type} was not found"),
            false,
            json!({"entityType":entity_type,"id":id}),
            2,
        )
    }
    pub fn version(expected: i64, current: i64) -> Self {
        Self::new(
            "VERSION_CONFLICT",
            "task version does not match",
            true,
            json!({"expectedVersion":expected,"currentVersion":current}),
            4,
        )
    }
    pub fn session(current: Option<&str>, requested: &str) -> Self {
        Self::new(
            "SESSION_CONFLICT",
            "session cannot become the current task session",
            false,
            json!({"currentSessionId":current,"requestedSessionId":requested}),
            4,
        )
    }
    pub fn constraint(constraint: &str) -> Self {
        Self::new(
            "CONSTRAINT_VIOLATION",
            "database constraint was rejected",
            false,
            json!({"constraint":constraint}),
            4,
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
                json!({"databaseVersion":database_version,"supportedVersion":supported_version}),
                2,
            ),
            storage_sqlite::StorageError::Sqlite(error) => Self::from_sqlite(error),
            storage_sqlite::StorageError::Io(error) => Self::new(
                "DATABASE_UNAVAILABLE",
                "database path is unavailable",
                false,
                json!({"reason":error.to_string()}),
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
                json!({"reason":reason.to_string(),"columnIndex":column,"sqliteType":format!("{data_type:?}")}),
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
                    json!({"timeoutMs":5000}),
                    4,
                );
            }
            if failure.code == ErrorCode::ConstraintViolation {
                return Self::new(
                    "CONSTRAINT_VIOLATION",
                    "database constraint was rejected",
                    false,
                    json!({"constraint":message.clone().unwrap_or_else(||failure.to_string())}),
                    4,
                );
            }
        }
        Self::new(
            "DATABASE_UNAVAILABLE",
            "database operation failed",
            false,
            json!({"reason":error.to_string()}),
            10,
        )
    }
    pub fn from_path(error: PathError, input_path: Option<&str>) -> Self {
        match error {
            PathError::PathIdentity(reason) => Self::new(
                "PATH_IDENTITY_UNKNOWN",
                "path identity could not be established",
                false,
                json!({"inputPath":input_path,"reason":reason}),
                5,
            ),
            PathError::Io(error) => Self::new(
                "FILESYSTEM_UNAVAILABLE",
                "filesystem operation failed",
                false,
                json!({"inputPath":input_path,"reason":error.to_string()}),
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
}
impl Service {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
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
