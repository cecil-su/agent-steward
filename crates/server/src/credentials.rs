//! Persistent local credentials, created once in a private directory. No silent rotation.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub(crate) struct Credentials {
    pub directory: PathBuf,
    pub admin: String,
    pub reader: String,
}

fn private_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(io::Error::other(
                "credential directory must be a real directory",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            steward_core::set_private_dir(path)?;
        }
        Err(error) => return Err(error),
    }
    if steward_application::database_permission_warning(&path.join("permission-probe"), false)
        .is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "credential directory must be private to the current user",
        ));
    }
    Ok(())
}

fn load_token(path: &Path) -> io::Result<String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            steward_core::set_private_file(path)?;
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
            Ok(token)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || steward_application::database_permission_warning(path, false).is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "existing credential must be a private regular file",
                ));
            }
            let mut value = String::new();
            fs::File::open(path)?.take(65).read_to_string(&mut value)?;
            if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid credential file; stop taskd and explicitly reset credentials",
                ));
            }
            Ok(value)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn load(root: &Path) -> io::Result<Credentials> {
    private_directory(root)?;
    let directory = root.join("identity");
    private_directory(&directory)?;
    let admin = load_token(&directory.join("credential"))?;
    let reader = load_token(&directory.join("readonly-credential"))?;
    if admin == reader {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "administrator and reader credentials must differ",
        ));
    }
    Ok(Credentials {
        directory,
        admin,
        reader,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_survive_restart_and_corruption_is_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("runtime");
        let first = load(&root).unwrap();
        let second = load(&root).unwrap();
        assert_eq!(first.admin, second.admin);
        assert_eq!(first.reader, second.reader);
        assert_ne!(first.admin, first.reader);
        fs::write(first.directory.join("credential"), "invalid").unwrap();
        assert!(load(&root).is_err());
        assert_eq!(
            fs::read_to_string(first.directory.join("credential")).unwrap(),
            "invalid"
        );
    }
    #[cfg(unix)]
    #[test]
    fn shared_directories_are_not_chmodded_or_accepted() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("shared");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(load(&root).is_err());
        assert_eq!(
            fs::metadata(root).unwrap().permissions().mode() & 0o777,
            0o777
        );
    }
}
