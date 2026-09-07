use clap::Parser;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
use steward_application::Service;
use steward_server::{ServerState, router};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "taskd",
    version,
    about = "Authenticated local V0 task interface"
)]
struct Args {
    #[arg(long)]
    database: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Explicit local IPv4 address. Non-loopback addresses expose HTTP to the network.
    #[arg(long, default_value = "127.0.0.1")]
    bind: std::net::Ipv4Addr,
    /// Do not open the local browser (headless or automated use).
    #[arg(long)]
    no_open: bool,
    /// Private runtime directory. A fresh credential subdirectory is created per launch.
    #[arg(long)]
    runtime_dir: Option<PathBuf>,
}
struct Credential {
    directory: PathBuf,
}
impl Drop for Credential {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.directory.join("credential"));
        let _ = fs::remove_dir(&self.directory);
    }
}
fn credential(root: PathBuf, token: &str) -> std::io::Result<Credential> {
    if !root.try_exists()? {
        fs::create_dir_all(&root)?;
        steward_core::set_private_dir(&root)?;
    }
    if steward_application::database_permission_warning(&root.join("credential-probe"), false)
        .is_some()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runtime directory must be private to the current user",
        ));
    }
    let directory = root.join(format!("taskd-{}", Uuid::new_v4()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(&directory)?;
    let guard = Credential { directory };
    steward_core::set_private_dir(&guard.directory)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let path = guard.directory.join("credential");
    let mut file = options.open(&path)?;
    steward_core::set_private_file(&path)?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    Ok(guard)
}
fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = "xdg-open";
    let status = std::process::Command::new(program)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("browser launcher failed"))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.bind.is_unspecified() || args.bind.is_multicast() || args.bind.is_broadcast() {
        return Err("--bind must be a specific local unicast IPv4 address".into());
    }
    if !args.bind.is_loopback() {
        eprintln!(
            "Warning: non-loopback HTTP listener; credentials and task data are not encrypted in transit. Restrict network access using your firewall."
        );
    }
    let database = args
        .database
        .or_else(|| steward_core::default_data_dir().map(|p| p.join("steward.db")))
        .ok_or("cannot resolve database path")?;
    let service = Service::new(database);
    // Fail before advertising a service or credential for an incompatible database.
    service.task_list(Some("active"))?;
    let listener = tokio::net::TcpListener::bind((args.bind, args.port)).await?;
    let address = listener.local_addr()?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let root = args
        .runtime_dir
        .or_else(|| steward_core::default_data_dir().map(|p| p.join("runtime")))
        .ok_or("cannot resolve runtime directory")?;
    let credential = credential(root, &token)?;
    println!("Agent Steward: http://{address}");
    println!(
        "Credential file: {}",
        credential.directory.join("credential").display()
    );
    println!(
        "Reload keeps this tab connected. Other devices or manual connection: use the credential file above."
    );
    let state = ServerState::with_address(service, address, token);
    if !args.no_open {
        let url = state.browser_connection_url();
        println!(
            "Opening the local browser with a one-time connection link (valid for 2 minutes)."
        );
        // No shell expansion, and never include the secret URL in diagnostics.
        std::thread::spawn(move || {
            if open_browser(&url).is_err() {
                eprintln!(
                    "Could not open the browser. Open the displayed address and use the credential file; use --no-open on headless systems."
                );
            }
        });
    }
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    drop(credential);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn credentials_are_private_removed_on_exit_and_do_not_chmod_shared_runtime_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("private-runtime");
        let guard = credential(root.clone(), "synthetic-test-only").unwrap();
        let file = guard.directory.join("credential");
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "synthetic-test-only");
        drop(guard);
        assert!(!file.exists());
        let shared = temp.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(credential(shared.clone(), "synthetic").is_err());
        assert_eq!(
            fs::metadata(shared).unwrap().permissions().mode() & 0o777,
            0o777
        );
    }
}
