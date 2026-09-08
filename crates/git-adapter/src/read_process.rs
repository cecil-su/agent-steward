//! Bounded read-only Git processes. Never apply this runner to Git mutations.
use crate::GitError;
use std::{
    cell::RefCell,
    io::{self, Read},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const STDOUT_LIMIT: usize = 8 * 1024 * 1024;
const STDERR_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
pub struct GitReadControl(Arc<Control>);
struct Control {
    deadline: Instant,
    cancelled: AtomicBool,
}
thread_local! { static CONTROL: RefCell<Option<GitReadControl>> = const { RefCell::new(None) }; }
impl Default for GitReadControl {
    fn default() -> Self {
        Self::new(TIMEOUT)
    }
}
impl GitReadControl {
    pub fn new(timeout: Duration) -> Self {
        Self(Arc::new(Control {
            deadline: Instant::now() + timeout,
            cancelled: AtomicBool::new(false),
        }))
    }
    pub fn check_current() -> Result<(), GitError> {
        CONTROL.with(|c| c.borrow().as_ref().map_or(Ok(()), Self::check))
    }
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn check(&self) -> Result<(), GitError> {
        if self.0.cancelled.load(Ordering::Relaxed) {
            return Err(GitError::ReadLimit("Git read cancelled"));
        }
        if Instant::now() >= self.0.deadline {
            return Err(GitError::ReadLimit("Git read deadline exceeded"));
        }
        Ok(())
    }
    /// One request deadline shared by all synchronous Git reads on this worker.
    pub fn within<T>(&self, action: impl FnOnce() -> T) -> T {
        struct Restore(Option<GitReadControl>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CONTROL.with(|c| c.replace(self.0.take()));
            }
        }
        let _restore = Restore(CONTROL.with(|c| c.replace(Some(self.clone()))));
        action()
    }
}

// Existing mutation pre/postcondition paths keep their original semantics. Only
// a read request installs the shared cancellation scope for generic Git queries.
pub(crate) fn output_if_scoped(command: &mut Command) -> Result<Output, GitError> {
    match CONTROL.with(|c| c.borrow().clone()) {
        Some(control) => bounded_output(command, &control, STDOUT_LIMIT, STDERR_LIMIT),
        None => Ok(command.output()?),
    }
}

pub(crate) fn output(command: &mut Command) -> Result<Output, GitError> {
    let control = CONTROL.with(|c| c.borrow().clone()).unwrap_or_default();
    bounded_output(command, &control, STDOUT_LIMIT, STDERR_LIMIT)
}

fn read_stream(mut stream: impl Read, limit: usize) -> Result<Vec<u8>, GitError> {
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = match stream.read(&mut buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(data);
        }
        if count > limit.saturating_sub(data.len()) {
            return Err(GitError::ReadLimit("Git read output limit exceeded"));
        }
        data.extend_from_slice(&buffer[..count]);
    }
}

fn bounded_output(
    command: &mut Command,
    control: &GitReadControl,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Output, GitError> {
    control.check()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = ProcessTree::spawn(command)?;
    let stdout = process.child.stdout.take().expect("piped stdout");
    let stderr = process.child.stderr.take().expect("piped stderr");
    let (sender, receiver) = mpsc::channel();
    let out_sender = sender.clone();
    let out_thread = std::thread::spawn(move || {
        let _ = out_sender.send((true, read_stream(stdout, stdout_limit)));
    });
    let err_thread = std::thread::spawn(move || {
        let _ = sender.send((false, read_stream(stderr, stderr_limit)));
    });
    let mut out = None;
    let mut err = None;
    let result: Result<(), GitError> = (|| {
        loop {
            control.check()?;
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok((is_out, result)) => {
                    if is_out {
                        out = Some(result?);
                    } else {
                        err = Some(result?);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            if out.is_some() && err.is_some() && process.exited()? {
                return Ok(());
            }
        }
    })();
    // Kill descendants too, including inherited-pipe holders; then reap before releasing a caller's slot.
    process.terminate();
    let status = process.child.wait();
    let _ = out_thread.join();
    let _ = err_thread.join();
    result?;
    Ok(Output {
        status: status?,
        stdout: out.expect("read complete"),
        stderr: err.expect("read complete"),
    })
}

struct ProcessTree {
    child: Child,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    terminated: bool,
}
impl ProcessTree {
    #[cfg(unix)]
    fn spawn(command: &mut Command) -> io::Result<Self> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        Ok(Self {
            child: command.spawn()?,
            terminated: false,
        })
    }
    #[cfg(unix)]
    fn exited(&mut self) -> io::Result<bool> {
        // Observe without reaping: the leader's PID pins the group ID until termination,
        // so cleanup cannot accidentally kill a new process group after PID reuse.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.si_signo != 0)
    }
    #[cfg(windows)]
    fn spawn(command: &mut Command) -> io::Result<Self> {
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{JobObjects::*, Threading::*},
        };
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                let error = io::Error::last_os_error();
                CloseHandle(job);
                return Err(error);
            }
            // Assign the job before any Git code can spawn descendants.
            command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
            let child = match command.spawn() {
                Ok(child) => child,
                Err(e) => {
                    CloseHandle(job);
                    return Err(e);
                }
            };
            let process = Self {
                child,
                job,
                terminated: false,
            };
            if AssignProcessToJobObject(job, process.child.as_raw_handle() as _) == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_initial_thread(process.child.id())?;
            Ok(process)
        }
    }
    #[cfg(windows)]
    fn exited(&mut self) -> io::Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }
    fn terminate(&mut self) {
        if self.terminated {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
        }
        // Also covers a Windows failure before the suspended process was assigned to its job.
        let _ = self.child.kill();
        self.terminated = true;
    }
}
impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.child.wait();
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

#[cfg(windows)]
unsafe fn resume_initial_thread(pid: u32) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    // std::process::Child does not retain the initial thread handle. The suspended
    // process has exactly one thread; enumerate by PID, never by executable name.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut found = Thread32First(snapshot, &mut entry);
            while found != 0 {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let resumed = ResumeThread(thread);
                    let result = if resumed == u32::MAX {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    };
                    CloseHandle(thread);
                    return result;
                }
                found = Thread32Next(snapshot, &mut entry);
            }
            Err(io::Error::other("cannot find suspended Git thread"))
        })();
        CloseHandle(snapshot);
        result
    }
}

#[cfg(test)]
mod tests;
