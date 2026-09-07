mod credentials;

use clap::Parser;
use std::path::PathBuf;
use steward_application::Service;
use steward_server::{ServerState, router};

#[derive(Parser)]
#[command(
    name = "taskd",
    version,
    about = "Authenticated local V0 task interface"
)]
struct Args {
    #[arg(long)]
    database: Option<PathBuf>,
    /// Stable browser address; use --port 0 only for an ephemeral test listener.
    #[arg(long, default_value_t = 43123)]
    port: u16,
    /// Explicit local IPv4 address. Non-loopback addresses expose HTTP to the network.
    #[arg(long, default_value = "127.0.0.1")]
    bind: std::net::Ipv4Addr,
    /// Do not open the local browser (headless or automated use).
    #[arg(long)]
    no_open: bool,
    /// Private directory containing persistent local administrator and reader credentials.
    #[arg(long)]
    runtime_dir: Option<PathBuf>,
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
    service.task_list(Some("active"))?;
    let listener = tokio::net::TcpListener::bind((args.bind, args.port)).await?;
    let address = listener.local_addr()?;
    let root = args
        .runtime_dir
        .or_else(|| steward_core::default_data_dir().map(|p| p.join("runtime")))
        .ok_or("cannot resolve runtime directory")?;
    let credentials = credentials::load(&root)?;
    println!("Agent Steward: http://{address}");
    println!(
        "Credential file: {}",
        credentials.directory.join("credential").display()
    );
    println!(
        "Read-only credential file: {}",
        credentials.directory.join("readonly-credential").display()
    );
    println!(
        "Local browser connects as administrator. Share only the read-only credential with other devices. Credentials persist across restarts."
    );
    let state = ServerState::with_address(service, address, credentials.admin)
        .with_readonly_token(credentials.reader)
        .with_browser_store(&credentials.directory.join("browser-sessions.db"))?;
    if !args.no_open {
        let url = state.browser_connection_url();
        println!(
            "Opening the local browser with a one-time connection link (valid for 2 minutes)."
        );
        std::thread::spawn(move || {
            if open_browser(&url).is_err() {
                eprintln!(
                    "Could not open the browser. Open the displayed address and use the credential file; use --no-open on headless systems."
                );
            }
        });
    }
    let shutdown = state.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.stop_event_streams();
        })
        .await?;
    Ok(())
}
