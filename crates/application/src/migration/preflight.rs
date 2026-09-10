//! Launcher schema preflight: never initializes or migrates the selected database.
use super::schema2::{local_file_path, require_regular_source_sidecars};
use super::{io_error, require_unused_destination};
use crate::{AppError, AppResult, Service};
use rusqlite::{Connection, OpenFlags};
use std::{fs, time::Duration};

impl Service {
    /// Return this binary's supported schema if the configured database is compatible
    /// or absent. This is a point-in-time version check, not integrity validation or
    /// a writer fence. Read-only WAL access may still involve SHM.
    pub fn check_database_schema(&self) -> AppResult<i64> {
        let path = self.database_path();
        local_file_path(path)?;
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                require_unused_destination(path)?;
                // No parent, database or sidecar is created by the preflight.
                return Ok(storage_sqlite::SCHEMA_VERSION);
            }
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                return Err(AppError::invalid(
                    "database",
                    "expected a regular database file, not a link or directory",
                ));
            }
            Err(error) => return Err(io_error(error)),
        }
        let identity = git_adapter::identify_existing(path)
            .map_err(|_| AppError::invalid("database", "cannot establish database identity"))?;
        local_file_path(&identity.canonical_path)?;
        require_regular_source_sidecars(&identity)?;
        let connection = Connection::open_with_flags(
            &identity.canonical_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(AppError::from_sqlite)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(AppError::from_sqlite)?;
        connection
            .pragma_update(None, "query_only", true)
            .map_err(AppError::from_sqlite)?;
        connection
            .pragma_update(None, "trusted_schema", false)
            .map_err(AppError::from_sqlite)?;
        // SQLite reads the committed WAL view; a main-file header alone can be stale.
        let version =
            storage_sqlite::schema_version(&connection).map_err(AppError::from_storage)?;
        if version != storage_sqlite::SCHEMA_VERSION {
            return Err(AppError::invalid(
                "database",
                format!(
                    "database schema {version} does not match binary schema {}; use a separately backed-up migration, not automatic update",
                    storage_sqlite::SCHEMA_VERSION,
                ),
            ));
        }
        git_adapter::verify_existing_identity(&identity).map_err(|_| {
            AppError::invalid("database", "database identity changed during preflight")
        })?;
        Ok(storage_sqlite::SCHEMA_VERSION)
    }
}
