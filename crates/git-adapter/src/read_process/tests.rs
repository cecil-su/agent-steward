use super::*;

#[test]
fn child() {
    let Ok(mode) = std::env::var("STEWARD_GIT_READ_FIXTURE") else {
        return;
    };
    match mode.as_str() {
        "ok" => println!("synthetic output"),
        "stdout" | "stderr" => {
            use std::io::Write;
            let bytes = vec![b'x'; 64 * 1024];
            if mode == "stdout" {
                std::io::stdout().write_all(&bytes).unwrap();
            } else {
                std::io::stderr().write_all(&bytes).unwrap();
            }
        }
        "large-stdout" => {
            use std::io::Write;
            std::io::stdout()
                .write_all(&vec![b'x'; STDOUT_LIMIT + 1])
                .unwrap();
        }
        "sleep" => std::thread::sleep(Duration::from_secs(30)),
        "descendant" => {
            let mut cmd = fixture("sleep");
            let child = cmd.spawn().unwrap();
            std::fs::write(
                std::env::var_os("STEWARD_CHILD_PID").unwrap(),
                child.id().to_string(),
            )
            .unwrap();
            // Inherited pipes stay open after this parent exits unless tree cleanup works.
            std::mem::forget(child);
        }
        _ => panic!("unexpected fixture mode"),
    }
}
fn fixture(mode: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "read_process::tests::child", "--nocapture"])
        .env("STEWARD_GIT_READ_FIXTURE", mode);
    cmd
}
#[test]
fn bounded_output_success_and_both_stream_limits() {
    let output = bounded_output(
        &mut fixture("ok"),
        &GitReadControl::new(Duration::from_secs(5)),
        1024,
        1024,
    )
    .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("synthetic output")
    );
    for mode in ["stdout", "stderr"] {
        let result = bounded_output(
            &mut fixture(mode),
            &GitReadControl::new(Duration::from_secs(5)),
            1024,
            1024,
        );
        assert!(matches!(
            result,
            Err(GitError::ReadLimit("Git read output limit exceeded"))
        ));
    }
}
#[test]
fn timeout_and_cancellation_reap_processes() {
    let start = Instant::now();
    let result = bounded_output(
        &mut fixture("sleep"),
        &GitReadControl::new(Duration::from_millis(200)),
        1024,
        1024,
    );
    assert!(matches!(
        result,
        Err(GitError::ReadLimit("Git read deadline exceeded"))
    ));
    assert!(start.elapsed() < Duration::from_secs(5));
    let control = GitReadControl::new(Duration::from_secs(10));
    let other = control.clone();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        other.cancel();
    });
    let result = bounded_output(&mut fixture("sleep"), &control, 1024, 1024);
    cancel.join().unwrap();
    assert!(matches!(
        result,
        Err(GitError::ReadLimit("Git read cancelled"))
    ));
}
#[test]
fn descendant_pipe_holder_is_terminated_even_after_parent_exits() {
    let temp = tempfile::tempdir().unwrap();
    let pid = temp.path().join("child.pid");
    let mut cmd = fixture("descendant");
    cmd.env("STEWARD_CHILD_PID", &pid);
    let start = Instant::now();
    let result = bounded_output(
        &mut cmd,
        &GitReadControl::new(Duration::from_secs(2)),
        4096,
        4096,
    );
    assert!(matches!(result, Err(GitError::ReadLimit(_))));
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(pid.exists(), "fixture must have spawned a descendant");
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::*};
        let id: u32 = std::fs::read_to_string(pid).unwrap().parse().unwrap();
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, id);
        if !process.is_null() {
            // Job termination is asynchronous; require acknowledgement well before the 30s fixture sleep.
            assert_eq!(WaitForSingleObject(process, 1000), 0);
            CloseHandle(process);
        }
    }
}
#[test]
fn generic_git_queries_use_the_installed_scope_limits() {
    let control = GitReadControl::new(Duration::from_secs(5));
    let result = control.within(|| output_if_scoped(&mut fixture("large-stdout")));
    assert!(matches!(
        result,
        Err(GitError::ReadLimit("Git read output limit exceeded"))
    ));
    let control = GitReadControl::new(Duration::from_millis(200));
    let result = control.within(|| output_if_scoped(&mut fixture("sleep")));
    assert!(matches!(
        result,
        Err(GitError::ReadLimit("Git read deadline exceeded"))
    ));
}

#[test]
fn scoped_budget_is_shared_and_restored() {
    let control = GitReadControl::new(Duration::from_secs(1));
    control.cancel();
    control.within(|| {
        assert!(matches!(
            output(&mut fixture("ok")),
            Err(GitError::ReadLimit(_))
        ))
    });
    assert!(output(&mut fixture("ok")).unwrap().status.success());
    let shared = GitReadControl::new(Duration::from_secs(1));
    shared.within(|| {
        assert!(output(&mut fixture("ok")).unwrap().status.success());
        std::thread::sleep(Duration::from_secs(1));
        assert!(matches!(
            output(&mut fixture("ok")),
            Err(GitError::ReadLimit("Git read deadline exceeded"))
        ));
    });
}
