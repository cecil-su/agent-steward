//! Opt-in generic host adapter: project away all content before persistence.
use clap::{Parser, ValueEnum};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{self, Read},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};
use steward_application::Service;

#[derive(Parser)]
#[command(
    name = "task-hook",
    version,
    about = "Opt-in metadata-only JSON client hook; never changes Task execution state"
)]
struct Args {
    #[arg(long)]
    database: PathBuf,
    #[arg(long)]
    session: String,
    #[arg(long)]
    source: String,
    #[arg(long, value_enum, default_value = "generic")]
    client: Client,
    #[arg(long = "external-session")]
    external_session: String,
}
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Client {
    Generic,
    Codex,
}

#[derive(Deserialize)]
struct CodexEvent {
    session_id: String,
    hook_event_name: String,
    source: Option<String>,
}
fn codex_event(bytes: &[u8], expected: &str) -> Result<Option<HostEvent>, String> {
    let event: CodexEvent = serde_json::from_slice(bytes)
        .map_err(|_| "HOOK_INPUT_REJECTED: invalid Codex hook metadata")?;
    if event.session_id != expected {
        return Ok(None);
    }
    let kind = match event.hook_event_name.as_str() {
        "SessionStart" => {
            if event.source.as_deref() == Some("startup") {
                "started"
            } else {
                "resumed"
            }
        }
        "SessionEnd" => "closed",
        "UserPromptSubmit" => "user_message",
        "PreToolUse" => "tool_call",
        "PostToolUse" => "tool_result",
        "Stop" | "Interrupt" => "idle",
        _ => return Ok(None),
    };
    // Codex has no common delivery ID or occurrence timestamp. These describe
    // this adapter observation; the exact DTO is reused for internal retries.
    Ok(Some(HostEvent {
        event_id: format!("observation-{}", uuid::Uuid::new_v4()),
        kind: kind.into(),
        occurred_at: chrono::Utc::now().to_rfc3339(),
    }))
}
// Host payloads can contain arbitrary extra fields. Only these three are retained.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostEvent {
    event_id: String,
    kind: String,
    occurred_at: String,
}
fn execute(args: Args) -> Result<(), String> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "HOOK_INPUT_REJECTED: cannot read host event")?;
    if bytes.len() > 256 * 1024 {
        return Err("HOOK_INPUT_REJECTED: host event exceeds 256 KiB".into());
    }
    let event = match args.client {
        Client::Generic => serde_json::from_slice(&bytes)
            .map_err(|_| "HOOK_INPUT_REJECTED: expected eventId, kind, occurredAt")?,
        Client::Codex => {
            if args.source != "codex" {
                return Err("HOOK_INPUT_REJECTED: Codex requires source codex".into());
            }
            let Some(event) = codex_event(&bytes, &args.external_session)? else {
                println!("{{}}");
                return Ok(());
            };
            event
        }
    };
    let projected=json!({"schemaVersion":1,"sessionId":args.session,"source":args.source,"externalSessionId":args.external_session,"eventId":event.event_id,"kind":event.kind,"occurredAt":event.occurred_at}).to_string();
    drop(bytes);
    let service = Service::new(args.database);
    for attempt in 0..3 {
        let result = service.hook_ingest(&projected);
        if steward_application::database_permission_warning(service.database_path(), true).is_some()
        {
            eprintln!("INSECURE_DATABASE_PERMISSIONS: database permissions could not be verified");
        }
        match result {
            Ok(result) => {
                if args.client == Client::Codex {
                    // Never inject acknowledgement text into the model context.
                    println!("{{}}");
                } else {
                    println!("{}", json!({"ok":true,"data":result.data}));
                }
                return Ok(());
            }
            Err(error) if error.body.code == "DATABASE_BUSY" && attempt < 2 => {
                std::thread::sleep(Duration::from_millis(100 * (attempt + 1)))
            }
            Err(error) => {
                return Err(format!(
                    "{}: observation was not recorded; explicit taskctl commands remain available",
                    error.body.code
                ));
            }
        }
    }
    unreachable!()
}
fn main() -> ExitCode {
    match execute(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
