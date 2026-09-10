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
    /// Check the explicit database schema read-only and exit; no initialization or listener.
    #[arg(long, requires = "database")]
    check_database_schema: bool,
    /// Local UI package store. Pure UI activation needs no daemon restart.
    #[arg(long)]
    ui_root: Option<PathBuf>,
    /// Stable browser address; use --port 0 only for an ephemeral test listener.
    #[arg(long, default_value_t = 43123)]
    port: u16,
    /// Explicit local IPv4 address. Non-loopback addresses expose HTTP to the network.
    #[arg(long, default_value = "127.0.0.1")]
    bind: std::net::Ipv4Addr,
    /// Do not open the local browser (headless or automated use).
    #[arg(long)]
    no_open: bool,
    /// Require credentials even from this machine (mandatory behind a proxy).
    #[arg(long)]
    require_local_auth: bool,
    /// Launcher-owned stop marker. Exits gracefully; never kills in-flight work.
    #[arg(long)]
    shutdown_file: Option<PathBuf>,
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
    if args.check_database_schema {
        let database = args
            .database
            .ok_or("--database is required for preflight")?;
        let schema = Service::new(database).check_database_schema()?;
        println!("databaseSchema={schema}");
        return Ok(());
    }
    if args
        .ui_root
        .as_ref()
        .is_some_and(|root| !root.is_absolute())
    {
        return Err("--ui-root must be an absolute path".into());
    }
    if args
        .shutdown_file
        .as_ref()
        .is_some_and(|path| path.exists())
    {
        return Err("shutdown marker already exists; use a fresh launcher marker".into());
    }
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
    // Keep the privately initialized WAL/SHM alive for the daemon lifetime, without
    // holding a transaction. Otherwise a later READ_ONLY context/SSE connection can
    // recreate sidecars with inherited (unprotected) Windows ACLs after the last
    // short-lived writer closes. Reads must not chmod files or weaken ACL checks.
    let _database_lifetime = storage_sqlite::open_database(service.database_path())?;
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
    let mut state = ServerState::with_address(service, address, credentials.admin)
        .with_readonly_token(credentials.reader)
        .with_browser_store(&credentials.directory.join("browser-sessions.db"))?;
    if let Some(root) = args.ui_root {
        state = state.with_ui_root(root);
    }
    if !args.require_local_auth {
        state = state.with_local_access();
        eprintln!(
            "Local direct requests are trusted as administrator. Do not forward/proxy this listener; use --require-local-auth for that deployment."
        );
    }
    if !args.no_open {
        let url = if args.require_local_auth {
            state.browser_connection_url()
        } else {
            format!("http://{address}")
        };
        println!("Opening the local browser.");
        std::thread::spawn(move || {
            if open_browser(&url).is_err() {
                eprintln!(
                    "Could not open the browser. Open the displayed address and use the credential file; use --no-open on headless systems."
                );
            }
        });
    }
    let shutdown = state.clone();
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async {
                if let Some(path) = args.shutdown_file {
                    loop {
                        if path.exists() { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                } else { std::future::pending::<()>().await; }
            } => {},
        }
        shutdown.stop_event_streams();
    })
    .await?;
    Ok(())
}
