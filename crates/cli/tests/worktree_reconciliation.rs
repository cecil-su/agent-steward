use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
fn cli(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_taskctl"))
        .arg("--database")
        .arg(db)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}
fn value(output: Output, code: i32) -> Value {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let repo = root.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("file"), b"fixture").unwrap();
    git(&repo, &["add", "file"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    );
    git(&repo, &["branch", "feature"]);
    let db = root.join("isolated.db");
    value(cli(&db, &["task", "create"]), 0);
    (repo, db)
}
fn quoted(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''")
    )
}

#[test]
fn post_git_database_failures_never_recommend_a_stale_version() {
    for corrupt_schema in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (repo, db) = fixture(temp.path());
        let worktree = temp.path().join("feature");
        let ready = temp.path().join("ready");
        let proceed = temp.path().join("proceed");
        let hook = repo.join(".git/hooks/post-checkout");
        fs::write(&hook, format!("#!/bin/sh\n{} --database {} --json task retitle 1 --if-version 1 --title '0911｜修复｜Synthetic' >/dev/null || exit 1\n: > {}\ni=0\nwhile [ ! -f {} ]; do i=$((i+1)); [ \"$i\" -lt 400 ] || exit 1; sleep .05; done\n", quoted(Path::new(env!("CARGO_BIN_EXE_taskctl"))), quoted(&db), quoted(&ready), quoted(&proceed))).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_taskctl"))
            .arg("--database")
            .arg(&db)
            .args(["--json", "worktree", "create", "1", "--repo"])
            .arg(&repo)
            .arg("--path")
            .arg(&worktree)
            .args(["--branch", "feature", "--if-version", "1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if !ready.exists() {
            let _ = child.kill();
            panic!("hook did not reach fixture barrier");
        }
        if corrupt_schema {
            rusqlite::Connection::open(&db)
                .unwrap()
                .execute_batch("PRAGMA user_version=99")
                .unwrap();
        }
        fs::write(&proceed, b"continue").unwrap();
        let output = value(child.wait_with_output().unwrap(), 6);
        let details = &output["error"]["details"];
        assert_eq!(output["error"]["code"], "PARTIAL_EXTERNAL_STATE");
        assert!(details["gitState"].is_null());
        if corrupt_schema {
            assert_eq!(details["diagnostics"]["phase"], "databaseReconnect");
            assert_eq!(
                details["recommendedArgs"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap(),
                "doctor"
            );
            assert!(
                !details["recommendedCommand"]
                    .as_str()
                    .unwrap()
                    .contains("--if-version")
            );
            assert!(
                details["diagnostics"]["recoveryInstruction"]
                    .as_str()
                    .unwrap()
                    .contains("reread")
            );
        } else {
            assert_eq!(details["recommendedArgs"][4], "adopt");
            assert_eq!(
                details["recommendedArgs"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap(),
                "2"
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn broken_unrelated_git_registration_does_not_poison_healthy_status_but_owner_checks_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, db) = fixture(temp.path());
    let other = temp.path().join("feature");
    git(
        &repo,
        &["worktree", "add", other.to_str().unwrap(), "feature"],
    );
    value(
        cli(
            &db,
            &[
                "worktree",
                "adopt",
                "1",
                "--repo",
                repo.to_str().unwrap(),
                "--path",
                repo.to_str().unwrap(),
                "--if-version",
                "1",
            ],
        ),
        0,
    );
    fs::write(
        repo.join(".git/worktrees/feature/gitdir"),
        b"?:/unavailable/.git\n",
    )
    .unwrap();
    value(cli(&db, &["worktree", "status", "1"]), 0);
    // Unobservable database ownership remains a write barrier, never a free path.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE tasks SET worktree_path='?:/unavailable' WHERE id=1",
            [],
        )
        .unwrap();
    value(cli(&db, &["task", "create"]), 0);
    let result = value(
        cli(
            &db,
            &[
                "worktree",
                "adopt",
                "2",
                "--repo",
                repo.to_str().unwrap(),
                "--path",
                repo.to_str().unwrap(),
                "--if-version",
                "1",
            ],
        ),
        5,
    );
    assert_eq!(result["error"]["code"], "PATH_IDENTITY_UNKNOWN");
    assert!(value(cli(&db, &["task", "show", "2"]), 0)["data"]["task"]["worktreePath"].is_null());
}

#[cfg(windows)]
#[test]
fn missing_case_alias_registration_refuses_detach_without_clearing_database() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, db) = fixture(temp.path());
    let worktree = temp.path().join("feature");
    git(
        &repo,
        &["worktree", "add", worktree.to_str().unwrap(), "feature"],
    );
    fs::write(
        repo.join(".git/worktrees/feature/gitdir"),
        format!(
            "{}/.git\n",
            temp.path()
                .join("FEATURE")
                .to_string_lossy()
                .replace('\\', "/")
        ),
    )
    .unwrap();
    value(
        cli(
            &db,
            &[
                "worktree",
                "adopt",
                "1",
                "--repo",
                repo.to_str().unwrap(),
                "--path",
                worktree.to_str().unwrap(),
                "--if-version",
                "1",
            ],
        ),
        0,
    );
    // Remove only this test's synthetic directory, preserving Git's registration.
    fs::remove_dir_all(&worktree).unwrap();
    let result = value(
        cli(
            &db,
            &[
                "worktree",
                "detach",
                "1",
                "--expected-path",
                worktree.to_str().unwrap(),
                "--if-version",
                "2",
            ],
        ),
        5,
    );
    assert_eq!(result["error"]["code"], "PATH_IDENTITY_UNKNOWN");
    let task = value(cli(&db, &["task", "show", "1"]), 0);
    assert!(!task["data"]["task"]["worktreePath"].is_null());
    assert_eq!(task["data"]["task"]["version"], 2);
    assert!(repo.join(".git/worktrees/feature/gitdir").exists());
}
