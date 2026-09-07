use serde_json::json;
#[cfg(unix)]
use std::fs;
use std::path::{Path, PathBuf};
use steward_core::Warning;

#[cfg(unix)]
pub fn database_permission_warning(path: &Path, _custom_database: bool) -> Option<Warning> {
    use std::os::unix::fs::PermissionsExt;

    let parent = database_parent(path).map_err(|reason| permission_check_warning(path, reason));
    let parent = match parent {
        Ok(parent) => parent,
        Err(warning) => return Some(warning),
    };
    match fs::metadata(&parent) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Some(Warning {
                    code: "INSECURE_DATABASE_PERMISSIONS".into(),
                    message: "the database parent directory is accessible to other users".into(),
                    details: json!({"path": parent, "mode": format!("{mode:04o}")}),
                });
            }
        }
        Err(error) => return Some(permission_check_warning(path, error.to_string())),
    }
    insecure_database_file_warning(path)
}

#[cfg(unix)]
fn insecure_database_file_warning(path: &Path) -> Option<Warning> {
    use std::os::unix::fs::PermissionsExt;

    for file in database_storage_files(path) {
        match file.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
        match fs::metadata(&file) {
            Ok(metadata) => {
                let mode = metadata.permissions().mode() & 0o777;
                if mode & 0o077 == 0 {
                    continue;
                }
                return Some(Warning {
                    code: "INSECURE_DATABASE_PERMISSIONS".into(),
                    message: "the database file or sidecar is accessible to other users".into(),
                    details: json!({"path": file, "mode": format!("{mode:04o}")}),
                });
            }
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
    }
    None
}

#[cfg(windows)]
pub fn database_permission_warning(path: &Path, custom_database: bool) -> Option<Warning> {
    if custom_database {
        return Some(Warning {
            code: "INSECURE_DATABASE_PERMISSIONS".into(),
            message: "the custom database parent directory is not managed by taskctl".into(),
            details: json!({
                "path": database_parent(path).ok(),
                "reason": "custom parent directory ACL may allow database replacement"
            }),
        });
    }
    let parent = match database_parent(path) {
        Ok(parent) => parent,
        Err(reason) => return Some(permission_check_warning(path, reason)),
    };
    match steward_core::private_acl_is_protected(&parent) {
        Ok(true) => {}
        Ok(false) => {
            return Some(permission_check_warning(
                &parent,
                "database directory DACL grants access to unapproved principals or is not protected",
            ));
        }
        Err(error) => return Some(permission_check_warning(&parent, error.to_string())),
    }
    insecure_database_file_warning(path)
}

#[cfg(windows)]
fn insecure_database_file_warning(path: &Path) -> Option<Warning> {
    for file in database_storage_files(path) {
        match file.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
        match steward_core::private_acl_is_protected(&file) {
            Ok(true) => {}
            Ok(false) => {
                return Some(permission_check_warning(
                    &file,
                    "database file or sidecar ACL grants access to unapproved principals or is not protected",
                ));
            }
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
    }
    None
}

#[cfg(not(any(unix, windows)))]
pub fn database_permission_warning(path: &Path, _custom_database: bool) -> Option<Warning> {
    Some(permission_check_warning(
        path,
        "permission verification unavailable",
    ))
}

fn database_storage_files(path: &Path) -> [PathBuf; 3] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shm = path.as_os_str().to_os_string();
    shm.push("-shm");
    [path.to_path_buf(), PathBuf::from(wal), PathBuf::from(shm)]
}

fn database_parent(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve current directory: {error}"))?
            .join(path)
    };
    absolute
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| "database parent directory is unavailable".to_owned())
}

fn permission_check_warning(path: &Path, reason: impl Into<String>) -> Warning {
    Warning {
        code: "INSECURE_DATABASE_PERMISSIONS".into(),
        message: "taskctl could not verify the database permissions".into(),
        details: json!({"path": path, "reason": reason.into()}),
    }
}
