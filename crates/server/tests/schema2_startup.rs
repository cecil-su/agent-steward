//! Disposable migrated-database startup/restart checks; no browser or installed service.
use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use steward_application::Service;

struct Daemon {
    child: Child,
    output: Option<thread::JoinHandle<()>>,
    stop: PathBuf,
    address: String,
    credential: PathBuf,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
    }
}
impl Daemon {
    fn start(database: &Path, runtime: &Path, stop: PathBuf) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_taskd"))
            .args([
                "--database",
                database.to_str().unwrap(),
                "--runtime-dir",
                runtime.to_str().unwrap(),
                "--port",
                "0",
                "--bind",
                "127.0.0.1",
                "--require-local-auth",
                "--no-open",
                "--shutdown-file",
                stop.to_str().unwrap(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let output = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // Keep draining after startup so closing the receiver cannot break daemon stdout.
                let _ = send.send(line);
            }
        });
        let mut daemon = Self {
            child,
            output: Some(output),
            stop,
            address: String::new(),
            credential: PathBuf::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while daemon.address.is_empty() || daemon.credential.as_os_str().is_empty() {
            let line = receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("isolated daemon startup");
            if let Some(address) = line.strip_prefix("Agent Steward: http://") {
                daemon.address = address.into();
            }
            if let Some(credential) = line.strip_prefix("Credential file: ") {
                daemon.credential = credential.into();
            }
        }
        daemon
    }
    fn context(&self) -> Value {
        let token = fs::read_to_string(&self.credential).unwrap();
        let mut socket = TcpStream::connect(&self.address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(socket,"GET /api/tasks/1/context HTTP/1.1\r\nHost: {}\r\nx-steward-token: {}\r\nConnection: close\r\n\r\n",self.address,token.trim()).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "isolated context request must succeed"
        );
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }
    fn stop(&mut self) {
        fs::write(&self.stop, b"stop owned test daemon").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "isolated daemon graceful shutdown"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn migrated_database_starts_and_restarts_but_old_schemas_are_refused_before_listening() {
    for version in [2,4] {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.db");
    let c = Connection::open(&source).unwrap();
    c.execute_batch(if version == 2 { include_str!("../../application/src/migration/schema2.sql") } else { include_str!("../../application/src/migration/schema4.sql") }).unwrap();
    c.execute_batch(&format!("PRAGMA user_version={version}; INSERT INTO tasks(id,status,version,created_at,updated_at) VALUES (1,'open',1,'2026-09-08T00:00:00Z','2026-09-08T00:00:00Z');")).unwrap();
    drop(c);
    let before = fs::read(&source).unwrap();
    let refused_runtime = temp.path().join("refused-runtime");
    let refused = Command::new(env!("CARGO_BIN_EXE_taskd"))
        .args([
            "--database",
            source.to_str().unwrap(),
            "--runtime-dir",
            refused_runtime.to_str().unwrap(),
            "--port",
            "0",
            "--no-open",
            "--require-local-auth",
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!refused_runtime.exists());
    assert!(!String::from_utf8_lossy(&refused.stdout).contains("Agent Steward:"));
    let target = temp.path().join("private-target").join("target.db");
    if version == 2 { Service::new(&target).import_schema2(&source, true).unwrap(); }
    else { Service::new(&target).import_schema4(&source, true).unwrap(); }
    let runtime = temp.path().join("runtime");
    let s = Service::new(&target);
    let task = s.task_show("1").unwrap().data;
    let history = s.history("1").unwrap().data;
    for attempt in 0..2 {
        let mut daemon = Daemon::start(
            &target,
            &runtime,
            temp.path().join(format!("stop-{attempt}")),
        );
        let context = daemon.context();
        assert_eq!(context["data"]["task"], task["task"]);
        assert!(context["data"]["project"].is_null());
        daemon.stop();
    }
    assert_eq!(s.task_show("1").unwrap().data, task);
    assert_eq!(s.history("1").unwrap().data, history);
    assert_eq!(fs::read(source).unwrap(), before);
    }
}
