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
        );
        CREATE TABLE IF NOT EXISTS browser_access_requests (
            id_hash TEXT PRIMARY KEY, origin TEXT NOT NULL, label TEXT NOT NULL,
            peer TEXT NOT NULL, agent TEXT NOT NULL, code TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('pending','approved','revoked')),
            created INTEGER NOT NULL, expires INTEGER NOT NULL
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
    // A pending cookie is not a grant. Approval inserts its hash into browser_sessions;
    // neither the administrator nor the database ever receives the cookie plaintext.
    pub fn request_access(
        &self,
        origin: &str,
        label: &str,
        peer: &str,
        agent: &str,
        previous: Option<&str>,
    ) -> Result<(Option<String>, serde_json::Value), AccessError> {
        let mut c = self.0.lock().expect("browser auth lock");
        let tx = c.transaction()?;
        let time = now();
        tx.execute("DELETE FROM browser_access_requests WHERE expires<=?1 OR (state='approved' AND created<?2 AND NOT EXISTS(SELECT 1 FROM browser_sessions s WHERE s.id_hash=browser_access_requests.id_hash))", params![time,time.saturating_sub(60)])?;
        if let Some(previous) = previous {
            let existing = request_view(&tx, origin, &hash(previous))?;
            if existing["state"] == "pending" {
                return Ok((None, existing));
            }
        }
        let recent: i64 = tx.query_row(
            "SELECT count(*) FROM browser_access_requests WHERE origin=?1 AND peer=?2 AND created>?3",
            params![origin, peer, time.saturating_sub(60)], |r| r.get(0),
        )?;
        let count: i64 = tx.query_row("SELECT count(*) FROM browser_access_requests", [], |r| {
            r.get(0)
        })?;
        let pending: i64 = tx.query_row(
            "SELECT count(*) FROM browser_access_requests WHERE state='pending'",
            [],
            |r| r.get(0),
        )?;
        if recent >= 4 || pending >= 64 || count >= 512 {
            return Err(AccessError::Limit);
        }
        let cookie = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let code = uuid::Uuid::new_v4().simple().to_string()[..8].to_uppercase();
        let id = hash(&cookie);
        tx.execute(
            "INSERT INTO browser_access_requests VALUES (?1,?2,?3,?4,?5,?6,'pending',?7,?8)",
            params![id, origin, label, peer, agent, code, time, time + 600],
        )?;
        let view = request_view(&tx, origin, &id)?;
        tx.commit()?;
        Ok((Some(cookie), view))
    }

    pub fn request_status(
        &self,
        origin: &str,
        cookie: Option<&str>,
    ) -> rusqlite::Result<serde_json::Value> {
        let c = self.0.lock().expect("browser auth lock");
        request_view(&c, origin, &cookie.map(hash).unwrap_or_default())
    }

    pub fn access_list(
        &self,
        origin: &str,
        admin: &str,
        reader: Option<&str>,
        current: Option<&str>,
    ) -> rusqlite::Result<serde_json::Value> {
        use serde_json::json;
        let c = self.0.lock().expect("browser auth lock");
        let mut query = c.prepare("SELECT id_hash,label,peer,agent,code,created,expires FROM browser_access_requests WHERE origin=?1 AND state='pending' AND expires>?2 ORDER BY created,id_hash")?;
        let pending = query.query_map(params![origin,now()], |r| Ok(json!({
            "id":r.get::<_,String>(0)?, "label":r.get::<_,String>(1)?, "peer":r.get::<_,String>(2)?,
            "agent":r.get::<_,String>(3)?, "verificationCode":r.get::<_,String>(4)?,
            "createdAt":r.get::<_,u64>(5)?, "expiresAt":r.get::<_,u64>(6)?
        })))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut query = c.prepare("SELECT s.id_hash,s.role,s.expires,r.label,r.peer,r.agent,r.created FROM browser_sessions s LEFT JOIN browser_access_requests r ON r.id_hash=s.id_hash AND r.origin=s.origin WHERE s.origin=?1 AND s.expires>?2 AND ((s.role='admin' AND s.credential_hash=?3) OR (s.role='reader' AND s.credential_hash=?4)) ORDER BY s.expires DESC,s.id_hash")?;
        let current = current.map(hash);
        let grants = query.query_map(params![origin,now(),hash(admin),reader.map(hash).unwrap_or_default()], |r| {
            let id: String = r.get(0)?;
            Ok(json!({"isCurrent":current.as_ref()==Some(&id), "id":id, "role":r.get::<_,String>(1)?,
                "expiresAt":r.get::<_,u64>(2)?, "authorizedAt":r.get::<_,u64>(2)?.saturating_sub(MAX_AGE), "label":r.get::<_,Option<String>>(3)?.unwrap_or_else(||"历史浏览器授权".into()),
                "peer":r.get::<_,Option<String>>(4)?, "agent":r.get::<_,Option<String>>(5)?, "createdAt":r.get::<_,Option<u64>>(6)?}))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"pending":pending,"grants":grants}))
    }

    pub fn approve_access(
        &self,
        origin: &str,
        id: &str,
        code: &str,
        credential: &str,
    ) -> Result<(), AccessError> {
        let mut c = self.0.lock().expect("browser auth lock");
        let tx = c.transaction()?;
        tx.execute("DELETE FROM browser_sessions WHERE expires<=?1", [now()])?;
        let count: i64 = tx.query_row("SELECT count(*) FROM browser_sessions", [], |r| r.get(0))?;
        if count >= 256 {
            return Err(AccessError::Limit);
        }
        let changed = tx.execute("UPDATE browser_access_requests SET state='approved',expires=?4 WHERE id_hash=?1 AND origin=?2 AND code=?3 AND state='pending' AND expires>?5",
            params![id,origin,code,now()+MAX_AGE,now()])?;
        if changed != 1 {
            return Err(AccessError::Conflict);
        }
        tx.execute(
            "INSERT INTO browser_sessions VALUES (?1,?2,'reader',?3,?4)",
            params![id, origin, hash(credential), now() + MAX_AGE],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn revoke_access(
        &self,
        origin: &str,
        id: &str,
        current: Option<&str>,
    ) -> Result<(), AccessError> {
        if current.is_some_and(|value| hash(value) == id) {
            return Err(AccessError::Conflict);
        }
        let mut c = self.0.lock().expect("browser auth lock");
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM browser_sessions WHERE id_hash=?1 AND origin=?2",
            params![id, origin],
        )?;
        tx.execute("UPDATE browser_access_requests SET state='revoked',expires=MIN(expires,?3) WHERE id_hash=?1 AND origin=?2", params![id,origin,now()+60])?;
        tx.commit()?;
        Ok(())
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

#[derive(Debug)]
pub(crate) enum AccessError {
    Store,
    Limit,
    Conflict,
}
impl From<rusqlite::Error> for AccessError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Store
    }
}
fn request_view(c: &Connection, origin: &str, id: &str) -> rusqlite::Result<serde_json::Value> {
    let data = c.query_row("SELECT state,code,expires FROM browser_access_requests WHERE id_hash=?1 AND origin=?2", params![id,origin], |r| {
        let state: String = r.get(0)?;
        let expires: u64 = r.get(2)?;
        // An approved request without a valid grant (expired, rotated or revoked) is never reissued.
        let state = if expires<=now() {"expired"} else if state=="approved" {"revoked"} else {state.as_str()};
        Ok(serde_json::json!({"state":state,"verificationCode":r.get::<_,String>(1)?,"expiresAt":expires}))
    }).optional()?;
    Ok(data.unwrap_or_else(|| serde_json::json!({"state":"none"})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_is_browser_scoped_readonly_and_cannot_be_replayed_after_revoke() {
        let auth = BrowserAuth::memory();
        let (cookie, view) = auth
            .request_access("origin", "browser", "192.0.2.1", "agent", None)
            .unwrap();
        let cookie = cookie.unwrap();
        assert_eq!(view["state"], "pending");
        assert_eq!(auth.role(&cookie, "origin", "a", Some("r")), None);
        assert_eq!(
            auth.request_status("other", Some(&cookie)).unwrap()["state"],
            "none"
        );
        let list = auth.access_list("origin", "a", Some("r"), None).unwrap();
        assert!(!list.to_string().contains(&cookie));
        let item = &list["pending"][0];
        let id = item["id"].as_str().unwrap();
        let code = item["verificationCode"].as_str().unwrap();
        assert!(matches!(
            auth.approve_access("other", id, code, "r"),
            Err(AccessError::Conflict)
        ));
        assert!(matches!(
            auth.approve_access("origin", id, "wrong", "r"),
            Err(AccessError::Conflict)
        ));
        auth.approve_access("origin", id, code, "r").unwrap();
        assert_eq!(auth.role(&cookie, "origin", "a", Some("r")), Some("reader"));
        assert_eq!(auth.role(&cookie, "origin", "a", Some("rotated")), None);
        assert!(matches!(
            auth.approve_access("origin", id, code, "r"),
            Err(AccessError::Conflict)
        ));
        auth.revoke_access("origin", id, None).unwrap();
        assert_eq!(auth.role(&cookie, "origin", "a", Some("r")), None);
        assert_eq!(
            auth.request_status("origin", Some(&cookie)).unwrap()["state"],
            "revoked"
        );
        assert!(matches!(
            auth.approve_access("origin", id, code, "r"),
            Err(AccessError::Conflict)
        ));
        let (new_cookie, _) = auth
            .request_access("origin", "browser", "192.0.2.1", "agent", Some(&cookie))
            .unwrap();
        assert_ne!(new_cookie.unwrap(), cookie);
    }
    #[test]
    fn requests_are_idempotent_bounded_expiring_and_grants_survive_reopen() {
        let temp = tempfile::tempdir().unwrap();
        steward_core::set_private_dir(temp.path()).unwrap();
        let path = temp.path().join("auth.db");
        let auth = BrowserAuth::open(&path).unwrap();
        let (cookie, view) = auth
            .request_access("origin", "browser", "192.0.2.1", "agent", None)
            .unwrap();
        let cookie = cookie.unwrap();
        assert_eq!(
            auth.request_access("origin", "browser", "192.0.2.1", "agent", Some(&cookie))
                .unwrap(),
            (None, view)
        );
        for _ in 0..3 {
            auth.request_access("origin", "browser", "192.0.2.1", "agent", None)
                .unwrap();
        }
        assert!(matches!(
            auth.request_access("origin", "browser", "192.0.2.1", "agent", None),
            Err(AccessError::Limit)
        ));
        let list = auth.access_list("origin", "a", Some("r"), None).unwrap();
        let row = list["pending"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == hash(&cookie))
            .unwrap();
        auth.approve_access(
            "origin",
            &hash(&cookie),
            row["verificationCode"].as_str().unwrap(),
            "r",
        )
        .unwrap();
        drop(auth);
        let auth = BrowserAuth::open(&path).unwrap();
        assert_eq!(auth.role(&cookie, "origin", "a", Some("r")), Some("reader"));
        auth.0
            .lock()
            .unwrap()
            .execute(
                "UPDATE browser_access_requests SET expires=0 WHERE state='pending'",
                [],
            )
            .unwrap();
        let row = list["pending"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] != hash(&cookie))
            .unwrap();
        assert!(matches!(
            auth.approve_access(
                "origin",
                row["id"].as_str().unwrap(),
                row["verificationCode"].as_str().unwrap(),
                "r"
            ),
            Err(AccessError::Conflict)
        ));
    }
    #[test]
    fn existing_grants_survive_auth_table_extension_and_self_revoke_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        steward_core::set_private_dir(temp.path()).unwrap();
        let path = temp.path().join("legacy-auth.db");
        let auth = BrowserAuth::open(&path).unwrap();
        let cookie = auth.issue("origin", "admin", "a", None).unwrap();
        auth.0
            .lock()
            .unwrap()
            .execute("DROP TABLE browser_access_requests", [])
            .unwrap();
        drop(auth);
        let auth = BrowserAuth::open(&path).unwrap();
        assert_eq!(auth.role(&cookie, "origin", "a", None), Some("admin"));
        let list = auth
            .access_list("origin", "a", None, Some(&cookie))
            .unwrap();
        assert_eq!(list["grants"][0]["isCurrent"], true);
        assert_eq!(
            list["grants"][0]["authorizedAt"].as_u64().unwrap() + MAX_AGE,
            list["grants"][0]["expiresAt"].as_u64().unwrap()
        );
        assert!(matches!(
            auth.revoke_access("origin", &hash(&cookie), Some(&cookie)),
            Err(AccessError::Conflict)
        ));
        assert_eq!(auth.role(&cookie, "origin", "a", None), Some("admin"));
    }
    #[test]
    fn pending_capacity_is_global_and_does_not_grow_without_bound() {
        let auth = BrowserAuth::memory();
        for n in 0..64 {
            auth.request_access("origin", "browser", &format!("192.0.2.{n}"), "agent", None)
                .unwrap();
        }
        assert!(matches!(
            auth.request_access("origin", "browser", "192.0.2.200", "agent", None),
            Err(AccessError::Limit)
        ));
    }
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
