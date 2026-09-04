use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};
use steward_application::{AppError, ErrorBody, Outcome, Service};
use steward_core::Warning;

#[derive(Debug, Parser)]
#[command(
    name = "taskctl",
    version,
    about = "Local-first V0 task continuity CLI"
)]
struct Cli {
    #[arg(long, global = true)]
    database: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    input: Option<PathBuf>,
    #[arg(long, global = true)]
    yes: bool,
    #[arg(long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: TopCommand,
}

#[derive(Debug, Subcommand)]
enum TopCommand {
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
    },
    History {
        task_id: String,
    },
    Doctor,
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Show {
        task_id: String,
    },
    Create {
        task_id: String,
    },
    Claim {
        task_id: String,
        #[arg(long)]
        session: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long = "take-over")]
        take_over: bool,
    },
    Update {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
    },
    Note {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long = "type")]
        note_type: String,
        #[arg(long)]
        text: String,
    },
    Block {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        recovery: String,
    },
    Unblock {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long = "next-step")]
        next_step: String,
    },
    Checkpoint {
        task_id: String,
        #[arg(long)]
        session: String,
        #[arg(long = "if-version")]
        if_version: i64,
    },
    Resume {
        task_id: String,
        #[arg(long)]
        session: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long = "from-session")]
        from_session: Option<String>,
        #[arg(long = "take-over")]
        take_over: bool,
    },
    Close {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long)]
        outcome: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    List {
        #[arg(long)]
        task: Option<String>,
    },
    Show {
        session_id: String,
    },
    Attach {
        task_id: String,
        #[arg(long)]
        session: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long)]
        source: Option<String>,
        #[arg(long = "external-session")]
        external_session: Option<String>,
        #[arg(long = "record-path")]
        record_path: Option<PathBuf>,
    },
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
    Close {
        session_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
    },
}

#[derive(Debug, Subcommand)]
enum ImportCommand {
    Add {
        task_id: String,
        #[arg(long)]
        session: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long)]
        file: PathBuf,
        #[arg(long = "confirm-sensitive-content-reviewed")]
        confirm_sensitive_content_reviewed: bool,
    },
    List {
        session_id: String,
    },
    Remove {
        import_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
    },
}

#[derive(Debug, Subcommand)]
enum WorktreeCommand {
    Create {
        task_id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        path: PathBuf,
        #[arg(long = "if-version")]
        if_version: i64,
    },
    Status {
        task_id: String,
    },
    Remove {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
    },
    Adopt {
        task_id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        path: PathBuf,
        #[arg(long = "if-version")]
        if_version: i64,
    },
    Detach {
        task_id: String,
        #[arg(long = "expected-path")]
        expected_path: PathBuf,
        #[arg(long = "if-version")]
        if_version: i64,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    schema_version: u32,
    ok: bool,
    data: Option<Value>,
    warnings: Vec<Warning>,
    error: Option<ErrorBody>,
}

fn main() -> ExitCode {
    let json_requested = std::env::args_os().any(|argument| argument == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            return render_error(
                AppError::new(
                    "INVALID_INPUT",
                    "command-line arguments are invalid",
                    false,
                    json!({"field": "arguments", "reason": error.to_string()}),
                    2,
                ),
                json_requested,
                Vec::new(),
            );
        }
    };
    let database = match cli.database.clone().or_else(default_database_path) {
        Some(path) => path,
        None => {
            return render_error(
                AppError::new(
                    "DATABASE_UNAVAILABLE",
                    "cannot resolve the user application data directory",
                    false,
                    json!({"reason":"platform data directory unavailable"}),
                    10,
                ),
                cli.json,
                Vec::new(),
            );
        }
    };
    let custom_database = cli.database.is_some();
    let service = Service::new(database);
    let result = dispatch(&cli, &service);
    let permission_warnings = database_permission_warning(service.database_path(), custom_database)
        .into_iter()
        .collect::<Vec<_>>();
    match result {
        Ok(mut outcome) => {
            outcome.warnings.extend(permission_warnings);
            render_success(outcome, cli.json)
        }
        Err(error) => render_error(error, cli.json, permission_warnings),
    }
}

fn dispatch(cli: &Cli, service: &Service) -> Result<Outcome, AppError> {
    match &cli.command {
        TopCommand::Task { command } => match command {
            TaskCommand::List { status } => service.task_list(status.as_deref()),
            TaskCommand::Show { task_id } => service.task_show(task_id),
            TaskCommand::Create { task_id } => {
                service.task_create(task_id, &read_input(cli.input.as_deref())?)
            }
            TaskCommand::Claim {
                task_id,
                session,
                if_version,
                take_over,
            } => service.task_claim(task_id, *if_version, session, *take_over),
            TaskCommand::Update {
                task_id,
                if_version,
            } => service.task_update(task_id, *if_version, &read_input(cli.input.as_deref())?),
            TaskCommand::Note {
                task_id,
                if_version,
                note_type,
                text,
            } => service.task_note(task_id, *if_version, note_type, text),
            TaskCommand::Block {
                task_id,
                if_version,
                reason,
                recovery,
            } => service.task_block(task_id, *if_version, reason, recovery),
            TaskCommand::Unblock {
                task_id,
                if_version,
                next_step,
            } => service.task_unblock(task_id, *if_version, next_step),
            TaskCommand::Checkpoint {
                task_id,
                session,
                if_version,
            } => service.task_checkpoint(
                task_id,
                *if_version,
                session,
                &read_input(cli.input.as_deref())?,
            ),
            TaskCommand::Resume {
                task_id,
                session,
                if_version,
                from_session,
                take_over,
            } => service.task_resume(
                task_id,
                *if_version,
                session,
                from_session.as_deref(),
                *take_over,
            ),
            TaskCommand::Close {
                task_id,
                if_version,
                outcome,
                reason,
            } => service.task_close(task_id, *if_version, outcome, reason.as_deref()),
        },
        TopCommand::Session { command } => match command {
            SessionCommand::List { task } => service.session_list(task.as_deref()),
            SessionCommand::Show { session_id } => service.session_show(session_id),
            SessionCommand::Attach {
                task_id,
                session,
                if_version,
                source,
                external_session,
                record_path,
            } => service.session_attach(
                task_id,
                *if_version,
                session,
                source.as_deref(),
                external_session.as_deref(),
                record_path.as_deref(),
            ),
            SessionCommand::Import { command } => match command {
                ImportCommand::Add {
                    task_id,
                    session,
                    if_version,
                    file,
                    confirm_sensitive_content_reviewed,
                } => {
                    if !confirm_sensitive_content_reviewed {
                        return Err(AppError::invalid(
                            "confirmSensitiveContentReviewed",
                            "--confirm-sensitive-content-reviewed is required before importing content into the unencrypted database",
                        ));
                    }
                    if !cli.json {
                        eprintln!(
                            "warning[SENSITIVE_CONTENT_CHECK_REQUIRED]: check the file for credentials before importing"
                        );
                    }
                    service.session_import_add(
                        task_id,
                        session,
                        *if_version,
                        file,
                        *confirm_sensitive_content_reviewed,
                    )
                }
                ImportCommand::List { session_id } => service.session_import_list(session_id),
                ImportCommand::Remove {
                    import_id,
                    if_version,
                } => {
                    if !cli.yes {
                        let imported = service.session_import_metadata(import_id)?;
                        confirm(false, &session_import_remove_confirmation(&imported))?;
                    }
                    service.session_import_remove(import_id, *if_version)
                }
            },
            SessionCommand::Close {
                session_id,
                if_version,
            } => service.session_close(session_id, *if_version),
        },
        TopCommand::Worktree { command } => match command {
            WorktreeCommand::Create {
                task_id,
                repo,
                branch,
                path,
                if_version,
            } => service.worktree_create(task_id, *if_version, repo, branch, path),
            WorktreeCommand::Status { task_id } => service.worktree_status(task_id),
            WorktreeCommand::Remove {
                task_id,
                if_version,
            } => {
                if !cli.yes {
                    let status = service.worktree_status(task_id)?;
                    let path = status.data["worktreeStatus"]["path"]
                        .as_str()
                        .ok_or_else(|| {
                            AppError::worktree_safety("task has no registered worktree", None)
                        })?;
                    confirm(false, &worktree_remove_confirmation(task_id, path))?;
                }
                service.worktree_remove(task_id, *if_version)
            }
            WorktreeCommand::Adopt {
                task_id,
                repo,
                path,
                if_version,
            } => service.worktree_adopt(task_id, *if_version, repo, path),
            WorktreeCommand::Detach {
                task_id,
                expected_path,
                if_version,
            } => service.worktree_detach(task_id, *if_version, expected_path),
        },
        TopCommand::History { task_id } => service.history(task_id),
        TopCommand::Doctor => service.doctor(),
    }
}

fn read_input(path: Option<&Path>) -> Result<String, AppError> {
    let path = path.ok_or_else(|| AppError::invalid("input", "--input <file> is required"))?;
    fs::read_to_string(path)
        .map_err(|error| AppError::invalid("input", format!("cannot read UTF-8 input: {error}")))
}

fn session_import_remove_confirmation(imported: &steward_core::SessionImportView) -> String {
    format!(
        "delete Session Import {}\n  Session ID: {}\n  SHA-256: {}\n  Size: {} bytes",
        imported.id, imported.session_id, imported.sha256, imported.size_bytes
    )
}

fn worktree_remove_confirmation(task_id: &str, path: &str) -> String {
    format!("remove registered Worktree\n  Task ID: {task_id}\n  Worktree path: {path}")
}

fn confirm(yes: bool, operation: &str) -> Result<(), AppError> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        return Err(AppError::invalid(
            "yes",
            "--yes is required in a non-interactive environment",
        ));
    }
    eprint!("Confirm {operation}? [y/N] ");
    io::stderr().flush().ok();
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| AppError::invalid("confirmation", error.to_string()))?;
    if answer.trim().eq_ignore_ascii_case("y") || answer.trim().eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(AppError::invalid(
            "confirmation",
            "operation was not confirmed",
        ))
    }
}

fn render_success(outcome: Outcome, json_output: bool) -> ExitCode {
    let envelope = Envelope {
        schema_version: 1,
        ok: true,
        data: Some(outcome.data),
        warnings: outcome.warnings,
        error: None,
    };
    if json_output {
        println!("{}", serde_json::to_string(&envelope).unwrap());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(envelope.data.as_ref().unwrap()).unwrap()
        );
        for warning in &envelope.warnings {
            eprintln!("warning[{}]: {}", warning.code, warning.message);
        }
    }
    ExitCode::SUCCESS
}

fn render_error(error: AppError, json_output: bool, warnings: Vec<Warning>) -> ExitCode {
    let exit_code = error.exit_code;
    if json_output {
        let envelope = Envelope {
            schema_version: 1,
            ok: false,
            data: None,
            warnings,
            error: Some(error.body),
        };
        println!("{}", serde_json::to_string(&envelope).unwrap());
    } else {
        eprintln!("error[{}]: {}", error.body.code, error.body.message);
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&error.body.details).unwrap()
        );
        for warning in &warnings {
            eprintln!("warning[{}]: {}", warning.code, warning.message);
        }
    }
    ExitCode::from(exit_code as u8)
}

fn default_database_path() -> Option<PathBuf> {
    steward_core::default_data_dir().map(|directory| directory.join("steward.db"))
}

#[cfg(unix)]
fn database_permission_warning(path: &Path, _custom_database: bool) -> Option<Warning> {
    use std::os::unix::fs::PermissionsExt;

    let parent = database_parent(path).map_err(|reason| permission_check_warning(path, reason));
    let parent = match parent {
        Ok(parent) => parent,
        Err(warning) => return Some(warning),
    };
    match fs::metadata(&parent) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Some(Warning {
                    code: "INSECURE_DATABASE_PERMISSIONS".into(),
                    message: "the database parent directory is accessible to other users".into(),
                    details: json!({"path": parent, "mode": format!("{mode:04o}")}),
                });
            }
        }
        Err(error) => return Some(permission_check_warning(path, error.to_string())),
    }
    insecure_database_file_warning(path)
}

#[cfg(unix)]
fn insecure_database_file_warning(path: &Path) -> Option<Warning> {
    use std::os::unix::fs::PermissionsExt;

    for file in database_storage_files(path) {
        match file.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
        match fs::metadata(&file) {
            Ok(metadata) => {
                let mode = metadata.permissions().mode() & 0o777;
                if mode & 0o077 == 0 {
                    continue;
                }
                return Some(Warning {
                    code: "INSECURE_DATABASE_PERMISSIONS".into(),
                    message: "the database file or sidecar is accessible to other users".into(),
                    details: json!({"path": file, "mode": format!("{mode:04o}")}),
                });
            }
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
    }
    None
}

#[cfg(windows)]
fn database_permission_warning(path: &Path, custom_database: bool) -> Option<Warning> {
    if custom_database {
        return Some(Warning {
            code: "INSECURE_DATABASE_PERMISSIONS".into(),
            message: "the custom database parent directory is not managed by taskctl".into(),
            details: json!({
                "path": database_parent(path).ok(),
                "reason": "custom parent directory ACL may allow database replacement"
            }),
        });
    }
    let parent = match database_parent(path) {
        Ok(parent) => parent,
        Err(reason) => return Some(permission_check_warning(path, reason)),
    };
    match steward_core::private_acl_is_protected(&parent) {
        Ok(true) => {}
        Ok(false) => {
            return Some(permission_check_warning(
                &parent,
                "database directory DACL grants access to unapproved principals or is not protected",
            ));
        }
        Err(error) => return Some(permission_check_warning(&parent, error.to_string())),
    }
    insecure_database_file_warning(path)
}

#[cfg(windows)]
fn insecure_database_file_warning(path: &Path) -> Option<Warning> {
    for file in database_storage_files(path) {
        match file.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
        match steward_core::private_acl_is_protected(&file) {
            Ok(true) => {}
            Ok(false) => {
                return Some(permission_check_warning(
                    &file,
                    "database file or sidecar ACL grants access to unapproved principals or is not protected",
                ));
            }
            Err(error) => return Some(permission_check_warning(&file, error.to_string())),
        }
    }
    None
}

#[cfg(not(any(unix, windows)))]
fn database_permission_warning(path: &Path, _custom_database: bool) -> Option<Warning> {
    Some(permission_check_warning(
        path,
        "permission verification unavailable",
    ))
}

fn database_storage_files(path: &Path) -> [PathBuf; 3] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shm = path.as_os_str().to_os_string();
    shm.push("-shm");
    [path.to_path_buf(), PathBuf::from(wal), PathBuf::from(shm)]
}

fn database_parent(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve current directory: {error}"))?
            .join(path)
    };
    absolute
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| "database parent directory is unavailable".to_owned())
}

fn permission_check_warning(path: &Path, reason: impl Into<String>) -> Warning {
    Warning {
        code: "INSECURE_DATABASE_PERMISSIONS".into(),
        message: "taskctl could not verify the database permissions".into(),
        details: json!({"path": path, "reason": reason.into()}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_confirmation_describes_the_exact_targets() {
        let imported = steward_core::SessionImportView {
            id: "import-1".into(),
            session_id: "session-1".into(),
            source_path: "session.json".into(),
            media_type: Some("application/json".into()),
            sha256: "abc123".into(),
            size_bytes: 42,
            imported_at: "2026-09-02T00:00:00.000Z".into(),
        };
        let import_confirmation = session_import_remove_confirmation(&imported);
        assert!(import_confirmation.contains("import-1"));
        assert!(import_confirmation.contains("session-1"));
        assert!(import_confirmation.contains("abc123"));
        assert!(import_confirmation.contains("42 bytes"));

        let worktree_confirmation =
            worktree_remove_confirmation("TASK 1", r"C:\workspace with spaces\feature");
        assert!(worktree_confirmation.contains("TASK 1"));
        assert!(worktree_confirmation.contains(r"C:\workspace with spaces\feature"));
    }
}
