use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use steward_application::database_permission_warning;
use steward_application::{AppError, ErrorBody, Outcome, Service, TaskListOptions};
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
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TaskListFormat {
    Table,
    Lines,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TaskListView {
    Active,
    InProgress,
    Blocked,
    Recent,
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// Read progress, decision, and risk notes.
    Notes {
        task_id: String,
    },
    /// Find tasks associated with the current directory (does not select or claim one).
    Here,
    /// Export task context without changing Task or Session state.
    Context {
        task_id: String,
        #[arg(long, default_value = "markdown", value_parser = ["markdown"])]
        format: String,
    },
    List {
        #[arg(long, value_enum, conflicts_with = "status")]
        view: Option<TaskListView>,
        #[arg(
            long,
            help = "open, in_progress, blocked, closed, or active (all unclosed tasks)"
        )]
        status: Option<String>,
        #[arg(long = "task-key")]
        task_key: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long = "page-size")]
        page_size: Option<u32>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        fields: Option<String>,
        #[arg(long, value_enum, default_value_t = TaskListFormat::Table)]
        format: TaskListFormat,
    },
    Show {
        task_id: String,
    },
    Create {
        task_key: Option<String>,
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
    Retitle {
        task_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long)]
        title: String,
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
    Bind {
        session_id: String,
        #[arg(long = "if-version")]
        if_version: i64,
        #[arg(long)]
        source: String,
        #[arg(long = "external-session")]
        external_session: String,
    },
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
enum HookCommand {
    Ingest,
    List {
        session_id: String,
        #[arg(long, default_value_t = 0)]
        after: i64,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    Clear {
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

const JSON_SCHEMA_VERSION: u32 = 2;

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
            render_success(outcome, &cli)
        }
        Err(error) => render_error(error, cli.json, permission_warnings),
    }
}

fn dispatch(cli: &Cli, service: &Service) -> Result<Outcome, AppError> {
    match &cli.command {
        TopCommand::Hook { command } => match command {
            HookCommand::Ingest => {
                let path = cli
                    .input
                    .as_deref()
                    .ok_or_else(|| AppError::invalid("input", "--input required"))?;
                let reader: Box<dyn Read> = if path == Path::new("-") {
                    if !cli.json {
                        return Err(AppError::invalid("input", "stdin requires --json"));
                    }
                    Box::new(io::stdin())
                } else {
                    let mut options = fs::OpenOptions::new();
                    options.read(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
                    }
                    let file = options
                        .open(path)
                        .map_err(|_| AppError::invalid("input", "cannot open event file"))?;
                    if !file
                        .metadata()
                        .map_err(|_| AppError::invalid("input", "cannot inspect event file"))?
                        .is_file()
                    {
                        return Err(AppError::invalid("input", "expected a regular event file"));
                    }
                    Box::new(file)
                };
                let mut bytes = Vec::new();
                reader
                    .take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| AppError::invalid("input", "cannot read event"))?;
                if bytes.len() > 16 * 1024 {
                    return Err(AppError::invalid("input", "event exceeds 16 KiB"));
                }
                let input = std::str::from_utf8(&bytes)
                    .map_err(|_| AppError::invalid("input", "invalid UTF-8"))?;
                service.hook_ingest(input)
            }
            HookCommand::List {
                session_id,
                after,
                limit,
            } => service.hook_list(session_id, *after, *limit),
            HookCommand::Clear {
                session_id,
                if_version,
            } => {
                if !cli.yes {
                    confirm(
                        false,
                        &format!(
                            "Clear observations for Session {session_id}; deduplication markers remain"
                        ),
                    )?;
                }
                service.hook_clear(session_id, *if_version)
            }
        },
        TopCommand::Task { command } => match command {
            TaskCommand::Notes { task_id } => service.task_notes(task_id),
            TaskCommand::Here => service.task_here(
                &std::env::current_dir()
                    .map_err(|error| AppError::invalid("directory", error.to_string()))?,
            ),
            TaskCommand::Context { task_id, .. } => service.task_context(task_id),
            TaskCommand::List {
                view,
                status,
                task_key,
                query,
                page_size,
                cursor,
                fields,
                format,
            } => {
                let fields = parse_task_fields(fields.as_deref())?;
                if *format == TaskListFormat::Lines && cli.json {
                    return Err(AppError::invalid(
                        "format",
                        "lines cannot be combined with --json",
                    ));
                }
                if *format == TaskListFormat::Lines && fields.len() != 1 {
                    return Err(AppError::invalid(
                        "fields",
                        "--format lines requires exactly one field",
                    ));
                }
                service.task_list_with_options(&TaskListOptions {
                    status: match view {
                        Some(TaskListView::Active) => Some("active".into()),
                        Some(TaskListView::InProgress) => Some("in_progress".into()),
                        Some(TaskListView::Blocked) => Some("blocked".into()),
                        Some(TaskListView::Recent) => None,
                        None => status.clone(),
                    },
                    task_key: task_key.clone(),
                    query: query.clone(),
                    page_size: *page_size,
                    cursor: cursor.clone(),
                    fields,
                })
            }
            TaskCommand::Show { task_id } => service.task_show(task_id),
            TaskCommand::Create { task_key } => {
                let input = cli.input.as_deref().map(read_input).transpose()?;
                service.task_create_with_options(task_key.as_deref(), input.as_deref())
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
            } => service.task_update(task_id, *if_version, &required_input(cli.input.as_deref())?),
            TaskCommand::Retitle {
                task_id,
                if_version,
                title,
            } => service.task_retitle(task_id, *if_version, title),
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
                &required_input(cli.input.as_deref())?,
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
            SessionCommand::Bind {
                session_id,
                if_version,
                source,
                external_session,
            } => service.session_bind(session_id, *if_version, source, external_session),
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
                    let numeric_id = service.task_show(task_id)?.data["task"]["id"]
                        .as_i64()
                        .expect("serialized Task id must be an integer");
                    confirm(false, &worktree_remove_confirmation(numeric_id, path))?;
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

fn parse_task_fields(value: Option<&str>) -> Result<Vec<String>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.trim().is_empty() {
        return Err(AppError::invalid("fields", "must not be empty"));
    }
    value
        .split(',')
        .map(|field| {
            let field = field.trim();
            if field.is_empty() {
                Err(AppError::invalid("fields", "must not contain empty fields"))
            } else {
                Ok(field.to_owned())
            }
        })
        .collect()
}

fn read_input(path: &Path) -> Result<String, AppError> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| AppError::invalid("input", format!("cannot read stdin: {error}")))?;
        bytes
    } else {
        fs::read(path)
            .map_err(|error| AppError::invalid("input", format!("cannot read input: {error}")))?
    };
    let input = String::from_utf8(bytes)
        .map_err(|error| AppError::invalid("input", format!("must be valid UTF-8: {error}")))?;
    if input.trim().is_empty() {
        return Err(AppError::invalid("input", "must not be empty"));
    }
    Ok(input)
}

fn required_input(path: Option<&Path>) -> Result<String, AppError> {
    let path = path.ok_or_else(|| AppError::invalid("input", "--input <file|-> is required"))?;
    read_input(path)
}

fn session_import_remove_confirmation(imported: &steward_core::SessionImportView) -> String {
    format!(
        "delete Session Import {}\n  Session ID: {}\n  SHA-256: {}\n  Size: {} bytes",
        imported.id, imported.session_id, imported.sha256, imported.size_bytes
    )
}

fn worktree_remove_confirmation(task_id: i64, path: &str) -> String {
    format!("remove registered Worktree\n  Task ID: #{task_id}\n  Worktree path: {path}")
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

fn render_success(outcome: Outcome, cli: &Cli) -> ExitCode {
    let envelope = Envelope {
        schema_version: JSON_SCHEMA_VERSION,
        ok: true,
        data: Some(outcome.data),
        warnings: outcome.warnings,
        error: None,
    };
    if cli.json {
        println!("{}", serde_json::to_string(&envelope).unwrap());
    } else {
        let mut data = envelope.data.clone().unwrap();
        humanize_task_ids(&mut data);
        if let TopCommand::Task {
            command: TaskCommand::List { fields, format, .. },
        } = &cli.command
        {
            let fields = parse_task_fields(fields.as_deref())
                .expect("Task list fields were validated before rendering");
            render_task_list(&data, &fields, *format);
        } else if let TopCommand::Task {
            command: TaskCommand::Context { .. } | TaskCommand::Resume { .. },
        } = &cli.command
        {
            print!("{}", render_task_context(&data));
        } else if let TopCommand::Task {
            command: TaskCommand::Here,
        } = &cli.command
        {
            println!("Directory: {}", terminal_cell(&data["directory"]));
            if data["matchedBy"] == "none" {
                println!(
                    "No task is associated with this directory. Use task list --view active to find work."
                );
            } else {
                println!(
                    "Matched by {}. No task was selected or claimed.",
                    terminal_cell(&data["matchedBy"])
                );
                render_task_list(&data, &[], TaskListFormat::Table);
                println!("Read a task with: taskctl task context <id>");
            }
        } else {
            println!("{}", serde_json::to_string_pretty(&data).unwrap());
        }
        for warning in &envelope.warnings {
            eprintln!("warning[{}]: {}", warning.code, warning.message);
        }
    }
    ExitCode::SUCCESS
}

fn render_task_context(data: &Value) -> String {
    let task = &data["task"];
    let checkpoint = &data["checkpoint"];
    let text = |value: &Value| match value.as_str() {
        Some(value) => value
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect::<String>(),
        None => terminal_cell(value),
    };
    let mut output = format!(
        "# Task {} — {}\n\nStatus: {} · Version: {}\n\n",
        text(&task["id"]),
        text(&task["title"]),
        text(&task["status"]),
        text(&task["version"])
    );
    for (label, value) in [
        ("Goal", &task["goal"]),
        ("Scope", &task["scope"]),
        ("Acceptance criteria", &task["acceptanceCriteria"]),
        ("Last checkpoint", &checkpoint["summary"]),
        ("Checkpoint saved at", &checkpoint["createdAt"]),
        ("Next step", &task["nextStep"]),
        ("Blocker", &task["blockReason"]),
        ("Recovery", &task["blockRecovery"]),
    ] {
        if value.is_null() && !matches!(label, "Goal" | "Last checkpoint" | "Next step") {
            continue;
        }
        output.push_str(&format!("## {label}\n\n{}\n\n", text(value)));
    }
    for (label, field) in [
        ("Completed", "completed"),
        ("Decisions", "decisions"),
        ("Pending", "pending"),
        ("Risks", "risks"),
    ] {
        if let Some(items) = checkpoint[field].as_array()
            && !items.is_empty()
        {
            output.push_str(&format!("## {label}\n\n"));
            for item in items {
                output.push_str(&format!("- {}\n", text(item)));
            }
            output.push('\n');
        }
    }
    if task["status"] == "closed" {
        output.push_str(&format!(
            "## Closure\n\n{}: {}\n\n",
            text(&task["closureOutcome"]),
            text(&task["closureReason"])
        ));
    }
    output.push_str(&format!(
        "## Working context\n\n- Directory: {}\n- Branch: {}\n- Current session: {}\n",
        text(&task["worktreePath"]),
        text(&task["repositoryBranch"]),
        text(&task["currentSessionId"])
    ));
    let status = &data["worktreeStatus"];
    if status.is_null() {
        output.push_str("- Git state: not observed\n");
    } else {
        output.push_str(&format!(
            "- Directory exists: {}\n- HEAD: {}\n- Observed at: {}\n",
            text(&status["exists"]),
            text(&status["head"]),
            text(&status["observedAt"])
        ));
        for field in ["staged", "unstaged", "untracked", "ignored"] {
            output.push_str(&format!(
                "- {field}: {}\n",
                status[field]
                    .as_array()
                    .map(|files| files.len().to_string())
                    .unwrap_or_else(|| "unknown".into())
            ));
        }
    }
    output.push_str("\nRead the latest task version before making changes. This context does not grant permission to close the task.\n");
    output
}

fn render_task_list(data: &Value, selected_fields: &[String], format: TaskListFormat) {
    let tasks = data["tasks"]
        .as_array()
        .expect("Task list data must contain an array");
    let fields = if selected_fields.is_empty() {
        vec!["id", "title", "status", "nextStep", "updatedAt"]
    } else {
        selected_fields.iter().map(String::as_str).collect()
    };

    if format == TaskListFormat::Lines {
        let field = fields[0];
        for task in tasks {
            println!("{}", terminal_cell(&task[field]));
        }
        if data["hasMore"].as_bool() == Some(true)
            && let Some(cursor) = data["nextCursor"].as_str()
        {
            eprintln!("more results available; next cursor: {cursor}");
        }
        return;
    }

    let headers = fields
        .iter()
        .map(|field| task_field_header(field).to_owned())
        .collect::<Vec<_>>();
    let rows = tasks
        .iter()
        .map(|task| {
            fields
                .iter()
                .map(|field| truncate_table_cell(&terminal_cell(&task[*field]), 60))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let widths = (0..fields.len())
        .map(|index| {
            rows.iter()
                .map(|row| display_width(&row[index]))
                .chain(std::iter::once(display_width(&headers[index])))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    print_table_row(&headers, &widths);
    print_table_row(
        &widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>(),
        &widths,
    );
    for row in &rows {
        print_table_row(row, &widths);
    }
    if rows.is_empty() {
        println!("(no tasks)");
    }
    let suffix = if data["hasMore"].as_bool() == Some(true) {
        " · more results available"
    } else {
        ""
    };
    println!("\nShowing {} tasks{suffix}", rows.len());
    if let Some(cursor) = data["nextCursor"].as_str() {
        println!("Next cursor: {cursor}");
    }
}

fn task_field_header(field: &str) -> &str {
    match field {
        "id" => "ID",
        "taskKey" => "TASK KEY",
        "title" => "TITLE",
        "status" => "STATUS",
        "version" => "VERSION",
        "goal" => "GOAL",
        "scope" => "SCOPE",
        "acceptanceCriteria" => "ACCEPTANCE CRITERIA",
        "nextStep" => "NEXT STEP",
        "blockReason" => "BLOCK REASON",
        "blockRecovery" => "BLOCK RECOVERY",
        "currentSessionId" => "CURRENT SESSION ID",
        "repositoryPath" => "REPOSITORY PATH",
        "repositoryCommonDir" => "REPOSITORY COMMON DIR",
        "repositoryBranch" => "REPOSITORY BRANCH",
        "worktreePath" => "WORKTREE PATH",
        "latestCheckpointId" => "LATEST CHECKPOINT ID",
        "closureOutcome" => "CLOSURE OUTCOME",
        "closureReason" => "CLOSURE REASON",
        "closedAt" => "CLOSED AT",
        "createdAt" => "CREATED AT",
        "updatedAt" => "UPDATED AT",
        _ => field,
    }
}

fn terminal_cell(value: &Value) -> String {
    let value = match value {
        Value::Null => return "—".to_owned(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        value => serde_json::to_string(value).expect("JSON value must serialize"),
    };
    value
        .chars()
        .map(|character| {
            if matches!(character, '\n' | '\r' | '\t') {
                ' '
            } else if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| if character.is_ascii() { 1 } else { 2 })
        .sum()
}

fn truncate_table_cell(value: &str, maximum_width: usize) -> String {
    if display_width(value) <= maximum_width {
        return value.to_owned();
    }
    let mut truncated = String::new();
    let mut width = 0;
    for character in value.chars() {
        let character_width = if character.is_ascii() { 1 } else { 2 };
        if width + character_width + 2 > maximum_width {
            break;
        }
        truncated.push(character);
        width += character_width;
    }
    truncated.push('…');
    truncated
}

fn print_table_row(cells: &[String], widths: &[usize]) {
    let line = cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell}{}", " ".repeat(width - display_width(cell))))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{}", line.trim_end());
}

fn humanize_task_ids(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(humanize_task_ids),
        Value::Object(map) => {
            if let Some(id) = map.get("taskId").and_then(Value::as_i64) {
                map.insert("taskId".to_owned(), Value::String(format!("#{id}")));
            }
            if let Some(task) = map.get_mut("task").and_then(Value::as_object_mut)
                && let Some(id) = task.get("id").and_then(Value::as_i64)
            {
                task.insert("id".to_owned(), Value::String(format!("#{id}")));
            }
            if let Some(tasks) = map.get_mut("tasks").and_then(Value::as_array_mut) {
                for task in tasks {
                    if let Some(task) = task.as_object_mut()
                        && let Some(id) = task.get("id").and_then(Value::as_i64)
                    {
                        task.insert("id".to_owned(), Value::String(format!("#{id}")));
                    }
                }
            }
            map.values_mut().for_each(humanize_task_ids);
        }
        _ => {}
    }
}

fn render_error(error: AppError, json_output: bool, warnings: Vec<Warning>) -> ExitCode {
    let exit_code = error.exit_code;
    if json_output {
        let envelope = Envelope {
            schema_version: JSON_SCHEMA_VERSION,
            ok: false,
            data: None,
            warnings,
            error: Some(error.body),
        };
        println!("{}", serde_json::to_string(&envelope).unwrap());
    } else {
        eprintln!("error[{}]: {}", error.body.code, error.body.message);
        let mut details = error.body.details.clone();
        humanize_task_ids(&mut details);
        eprintln!("{}", serde_json::to_string_pretty(&details).unwrap());
        for warning in &warnings {
            eprintln!("warning[{}]: {}", warning.code, warning.message);
        }
    }
    ExitCode::from(exit_code as u8)
}

fn default_database_path() -> Option<PathBuf> {
    steward_core::default_data_dir().map(|directory| directory.join("steward.db"))
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
            worktree_remove_confirmation(12, r"C:\workspace with spaces\feature");
        assert!(worktree_confirmation.contains("#12"));
        assert!(worktree_confirmation.contains(r"C:\workspace with spaces\feature"));
    }
}
