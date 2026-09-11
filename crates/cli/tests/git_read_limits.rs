use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use steward_application::Service;

#[test]
fn filter_child() {
    let Ok(mode) = std::env::var("TASKCTL_READ_FIXTURE") else {
        return;
    };
    if mode == "stderr" {
        use std::io::Write;
        std::io::stderr()
            .write_all(&vec![b'x'; 128 * 1024])
            .unwrap();
    } else {
        std::thread::sleep(Duration::from_secs(30));
    }
}

fn shell_path(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_str()
            .unwrap()
            .replace('\\', "/")
            .replace('\'', "'\"'\"'")
    )
}

#[test]
fn cli_git_reads_are_bounded_without_changing_task_or_history() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    fs::write(repo.join("sample"), "before\n").unwrap();
    git(&["add", "sample"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=test@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "fixture",
    ]);
    let database = temp.path().join("isolated.db");
    let service = Service::new(&database).with_lock_root(temp.path().join("locks"));
    service.task_create_minimal().unwrap();
    service.worktree_adopt("1", 1, &repo, &repo).unwrap();
    let before = service.task_show("1").unwrap().data;
    let history = service.history("1").unwrap().data;
    fs::write(repo.join(".gitattributes"), "sample filter=bounded-test\n").unwrap();
    fs::write(repo.join("sample"), "edited\n").unwrap();
    for mode in ["stderr", "sleep"] {
        let filter = format!(
            "TASKCTL_READ_FIXTURE={mode} {} --exact filter_child --nocapture",
            shell_path(&std::env::current_exe().unwrap())
        );
        git(&["config", "--local", "filter.bounded-test.clean", &filter]);
        let start = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_taskctl"))
            .args([
                "--database",
                database.to_str().unwrap(),
                "--json",
                "worktree",
                "status",
                "1",
            ])
            .output()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(20));
        assert_eq!(output.status.code(), Some(5));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["schemaVersion"], 2);
        assert_eq!(value["error"]["code"], "GIT_READ_LIMIT");
        assert_eq!(service.task_show("1").unwrap().data, before);
        assert_eq!(service.history("1").unwrap().data, history);
    }
}
