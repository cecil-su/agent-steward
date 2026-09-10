//! Windows default-database ACL regression using only an owned temporary daemon/database.
#![cfg(windows)]
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use steward_application::Service;

struct Daemon {
    child: Child,
    temp: Option<tempfile::TempDir>,
    stop: PathBuf,
    address: String,
    database: PathBuf,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::write(&self.stop, b"stop");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        if let Some(temp) = self.temp.take() {
            eprintln!(
                "Graceful stop failed; isolated evidence retained: {:?}",
                temp.keep()
            );
        }
    }
}
impl Daemon {
    fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("agent-steward/steward.db");
        Service::new(&database)
            .task_create("PERMISSION", "{}")
            .unwrap();
        let stop = temp.path().join("stop");
        let mut child = Command::new(env!("CARGO_BIN_EXE_taskd"))
            .env("LOCALAPPDATA", temp.path())
            .args(["--no-open", "--port", "0", "--database"])
            .arg(&database)
            .arg("--runtime-dir")
            .arg(temp.path().join("runtime"))
            .arg("--shutdown-file")
            .arg(&stop)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(address) = line.strip_prefix("Agent Steward: http://") {
                    let _ = send.send(address.to_owned());
                }
            }
        });
        let mut daemon = Self {
            child,
            temp: Some(temp),
            stop,
            address: String::new(),
            database,
        };
        daemon.address = receive
            .recv_timeout(Duration::from_secs(10))
            .expect("isolated startup");
        daemon
    }
    fn get(&self, path: &str) -> Value {
        let mut stream = TcpStream::connect(&self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {}\r\nX-Steward-UI-Contract: 4\r\nConnection: close\r\n\r\n", self.address).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }
}

#[test]
fn fresh_context_reads_keep_private_sidecars_and_real_acl_failures_still_warn() {
    let daemon = Daemon::start();
    for _ in 0..3 {
        for path in [
            "/api/access",
            "/api/tasks",
            "/api/tasks/1/context",
            "/api/tasks/1/notes",
            "/api/tasks/1/context",
        ] {
            let result = daemon.get(path);
            assert_eq!(result["ok"], true);
            assert_eq!(result["warnings"], serde_json::json!([]), "{path}");
        }
    }
    let wal = PathBuf::from(format!("{}-wal", daemon.database.display()));
    assert!(steward_core::private_acl_is_protected(&wal).unwrap());
    let grant = Command::new("icacls")
        .arg(&wal)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .unwrap();
    assert!(grant.status.success(), "synthetic ACL setup");
    assert!(!steward_core::private_acl_is_protected(&wal).unwrap());
    let result = daemon.get("/api/tasks/1/context");
    assert_eq!(result["ok"], true);
    assert!(
        result["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "INSECURE_DATABASE_PERMISSIONS")
    );
    steward_core::set_private_file(&wal).unwrap();
    assert_eq!(
        daemon.get("/api/tasks/1/context")["warnings"],
        serde_json::json!([])
    );
}
