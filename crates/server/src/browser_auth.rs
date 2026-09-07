//! Revocable browser grants. Only hashes of random cookie values are stored.
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const MAX_AGE: u64 = 30 * 24 * 60 * 60;
pub(crate) struct BrowserAuth(Mutex<Connection>);
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl BrowserAuth {
    pub fn memory() -> Self {
        Self::initialize(Connection::open_in_memory().expect("browser auth memory database"))
            .expect("browser auth schema")
    }
    fn initialize(connection: Connection) -> rusqlite::Result<Self> {
        connection.busy_timeout(std::time::Duration::from_millis(250))?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS browser_sessions (
            id_hash TEXT PRIMARY KEY, origin TEXT NOT NULL, role TEXT NOT NULL,
            credential_hash TEXT NOT NULL, expires INTEGER NOT NULL
        );",
        )?;
        Ok(Self(Mutex::new(connection)))
    }
    pub fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        // Parent is the already validated private identity directory.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(_) => steward_core::set_private_file(path)?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(path)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("browser session store must be a regular file".into());
                }
            }
            Err(e) => return Err(e.into()),
        }
        if steward_application::database_permission_warning(path, false).is_some() {
            return Err("browser session store must be private".into());
        }
        Ok(Self::initialize(Connection::open(path)?)?)
    }
    pub fn issue(
        &self,
        origin: &str,
        role: &str,
        credential: &str,
        previous: Option<&str>,
    ) -> rusqlite::Result<String> {
        let id = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut c = self.0.lock().expect("browser auth lock");
        let tx = c.transaction()?;
        tx.execute("DELETE FROM browser_sessions WHERE expires<=?1", [now()])?;
        if let Some(previous) = previous {
            tx.execute(
                "DELETE FROM browser_sessions WHERE id_hash=?1 AND origin=?2",
                params![hash(previous), origin],
            )?;
        }
        let count: i64 = tx.query_row("SELECT count(*) FROM browser_sessions", [], |r| r.get(0))?;
        if count >= 256 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute(
            "INSERT INTO browser_sessions VALUES (?1,?2,?3,?4,?5)",
            params![hash(&id), origin, role, hash(credential), now() + MAX_AGE],
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn role(
        &self,
        id: &str,
        origin: &str,
        admin: &str,
        reader: Option<&str>,
    ) -> Option<&'static str> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let c = self.0.lock().ok()?;
        let (role,credential):(String,String)=c.query_row("SELECT role,credential_hash FROM browser_sessions WHERE id_hash=?1 AND origin=?2 AND expires>?3",params![hash(id),origin,now()],|r|Ok((r.get(0)?,r.get(1)?))).optional().ok()??;
        match role.as_str() {
            "admin" if credential == hash(admin) => Some("admin"),
            "reader" if reader.is_some_and(|r| credential == hash(r)) => Some("reader"),
            _ => None,
        }
    }
    pub fn revoke(&self, id: &str) -> rusqlite::Result<()> {
        self.0
            .lock()
            .expect("browser auth lock")
            .execute("DELETE FROM browser_sessions WHERE id_hash=?1", [hash(id)])?;
        Ok(())
    }
    pub fn revoke_origin(&self, origin: &str) -> rusqlite::Result<()> {
        self.0
            .lock()
            .expect("browser auth lock")
            .execute("DELETE FROM browser_sessions WHERE origin=?1", [origin])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_survive_reopen_and_are_scoped_expiring_and_revocable() {
        let temp = tempfile::tempdir().unwrap();
        steward_core::set_private_dir(temp.path()).unwrap();
        let path = temp.path().join("sessions.db");
        let auth = BrowserAuth::open(&path).unwrap();
        let id = auth
            .issue("http://127.0.0.1:43123", "admin", "key", None)
            .unwrap();
        drop(auth);
        let auth = BrowserAuth::open(&path).unwrap();
        assert_eq!(
            auth.role(&id, "http://127.0.0.1:43123", "key", None),
            Some("admin")
        );
        assert_eq!(auth.role(&id, "http://127.0.0.1:43124", "key", None), None);
        assert_eq!(
            auth.role(&id, "http://127.0.0.1:43123", "rotated", None),
            None
        );
        auth.revoke(&id).unwrap();
        assert_eq!(auth.role(&id, "http://127.0.0.1:43123", "key", None), None);
        let id = auth.issue("origin", "reader", "r", None).unwrap();
        auth.0
            .lock()
            .unwrap()
            .execute("UPDATE browser_sessions SET expires=0", [])
            .unwrap();
        assert_eq!(auth.role(&id, "origin", "key", Some("r")), None);
    }
}
