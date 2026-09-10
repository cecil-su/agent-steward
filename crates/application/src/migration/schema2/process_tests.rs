//! Real process termination around the private migration seam; no production fault option.
use super::*;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn migration_child() {
    let Some(source) = std::env::var_os("STEWARD_SCHEMA2_TEST_SOURCE") else {
        return;
    };
    let destination = std::env::var_os("STEWARD_SCHEMA2_TEST_DESTINATION").unwrap();
    let ready = std::env::var_os("STEWARD_SCHEMA2_TEST_READY").unwrap();
    let phase = std::env::var("STEWARD_SCHEMA2_TEST_PHASE").unwrap();
    let version = std::env::var("STEWARD_COPY_TEST_VERSION")
        .unwrap_or_else(|_| "2".into())
        .parse::<i64>()
        .unwrap();
    import_version(
        Path::new(&source),
        Path::new(&destination),
        true,
        version,
        |at, _| {
            if at == phase {
                fs::write(Path::new(&ready), b"ready").unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn killed_import_never_publishes_and_retry_does_not_adopt_abandoned_staging() {
    for (version, phase) in [
        (2, "history"),
        (2, "before_publish"),
        (4, "history"),
        (4, "before_publish"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = if version == 2 {
            super::tests::fixture(temp.path())
        } else {
            super::schema4_tests::fixture(temp.path())
        };
        let before = fs::read(&source).unwrap();
        let destination = temp.path().join("new.db");
        let ready = temp.path().join("ready");
        let mut child = Reap(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "migration::schema2::process_tests::migration_child",
                    "--nocapture",
                ])
                .env("STEWARD_COPY_TEST_VERSION", version.to_string())
                .env("STEWARD_SCHEMA2_TEST_SOURCE", &source)
                .env("STEWARD_SCHEMA2_TEST_DESTINATION", &destination)
                .env("STEWARD_SCHEMA2_TEST_READY", &ready)
                .env("STEWARD_SCHEMA2_TEST_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before the test boundary"
            );
            assert!(
                Instant::now() < deadline,
                "child did not reach the test boundary"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        assert!(!destination.exists());
        assert_eq!(fs::read(&source).unwrap(), before);
        let abandoned: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!(".import-schema{version}-"))
            })
            .collect();
        assert_eq!(abandoned.len(), 1);
        private_parent(&abandoned[0]).unwrap();
        import_version(&source, &destination, true, version, |_, _| Ok(())).unwrap();
        assert!(
            abandoned[0].exists(),
            "retry must not scan, adopt or delete old staging"
        );
        assert_eq!(
            Service::new(destination).task_show("1").unwrap().data["task"]["version"],
            3
        );
    }
}
