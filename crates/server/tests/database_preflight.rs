//! Real taskd preflight: explicit paths, no initializer, runtime, browser or listener.
use rusqlite::Connection;
use std::{fs, path::Path, process::Command};
use steward_application::Service;

fn check(database: &Path, runtime: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_taskd"))
        .arg("--check-database-schema")
        .arg("--database")
        .arg(database)
        .arg("--runtime-dir")
        .arg(runtime)
        .arg("--port")
        .arg("0")
        .output()
        .unwrap()
}

#[test]
fn preflight_requires_explicit_database_and_does_not_initialize_missing_paths() {
    let out = Command::new(env!("CARGO_BIN_EXE_taskd"))
        .arg("--check-database-schema")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("missing-parent").join("new.db");
    let runtime = temp.path().join("runtime");
    let out = check(&database, &runtime);
    assert!(out.status.success(), "{:?}", out.stderr);
    assert_eq!(out.stdout, b"databaseSchema=5\n");
    assert!(!database.parent().unwrap().exists());
    assert!(!runtime.exists());
    for suffix in ["-wal", "-shm", "-journal"] {
        let database = temp.path().join("occupied.db");
        let sidecar = temp.path().join(format!("occupied.db{suffix}"));
        fs::write(&sidecar, b"keep").unwrap();
        assert!(!check(&database, &runtime).status.success());
        assert_eq!(fs::read(&sidecar).unwrap(), b"keep");
        assert!(!database.exists());
        fs::remove_file(sidecar).unwrap();
    }
}

#[test]
fn preflight_refuses_old_invalid_and_ambiguous_paths_without_touching_them() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = temp.path().join("runtime");
    for version in [0, 1, 2, 3, 4, 6] {
        let database = temp.path().join(format!("schema-{version}.db"));
        let c = Connection::open(&database).unwrap();
        c.execute_batch(include_str!("../../application/src/migration/schema2.sql"))
            .unwrap();
        c.pragma_update(None, "user_version", version).unwrap();
        drop(c);
        let before = fs::read(&database).unwrap();
        assert!(!check(&database, &runtime).status.success());
        assert_eq!(fs::read(database).unwrap(), before);
        assert!(!runtime.exists());
    }
    let database = temp.path().join("invalid.db");
    fs::write(&database, b"not a database").unwrap();
    assert!(!check(&database, &runtime).status.success());
    assert_eq!(fs::read(&database).unwrap(), b"not a database");
    assert!(!check(Path::new("relative.db"), &runtime).status.success());
    #[cfg(windows)]
    for tail in [".", " "] {
        let alias = temp.path().join(format!("invalid.db{tail}"));
        assert!(!check(&alias, &runtime).status.success());
    }
    assert!(!runtime.exists());
}

#[test]
fn preflight_reads_committed_wal_schema_and_preserves_business_records() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("current.db");
    let runtime = temp.path().join("runtime");
    let service = Service::new(&database);
    let task = service.task_create_minimal().unwrap().data;
    let history = service.history("1").unwrap().data;
    let c = Connection::open(&database).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let before = fs::read(&database).unwrap();
    assert!(check(&database, &runtime).status.success());
    c.pragma_update(None, "user_version", 2).unwrap();
    assert_eq!(fs::read(&database).unwrap(), before);
    let wal = temp.path().join("current.db-wal");
    let wal_before = fs::read(&wal).unwrap();
    assert!(
        !check(&database, &runtime).status.success(),
        "must read the WAL version, not the main header"
    );
    assert_eq!(fs::read(&database).unwrap(), before);
    assert_eq!(fs::read(&wal).unwrap(), wal_before);
    c.pragma_update(None, "user_version", 5).unwrap();
    assert_eq!(check(&database, &runtime).stdout, b"databaseSchema=5\n");
    drop(c);
    assert_eq!(service.task_show("1").unwrap().data, task);
    assert_eq!(service.history("1").unwrap().data, history);
    assert!(!runtime.exists());
}
