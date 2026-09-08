//! Project transport only; mutations use the same Application Service/CAS as the CLI.
use super::*;
use steward_application::{ProjectContextOptions, SourceLocation};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "page_limit")]
    limit: u32,
}
fn page_limit() -> u32 {
    50
}

pub(super) async fn list(
    State(state): State<ServerState>,
    query: Result<Query<PageQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid project pagination",
        );
    };
    run(state, move |s| s.project_list(q.after, q.limit)).await
}
pub(super) async fn show(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    run(state, move |s| s.project_show(&id)).await
}
pub(super) async fn history(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    query: Result<Query<PageQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid project pagination",
        );
    };
    run(state, move |s| s.project_history(&id, q.after, q.limit)).await
}
pub(super) async fn components(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    run(state, move |s| s.project_components(&id)).await
}
pub(super) async fn sources(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    run(state, move |s| s.project_sources(&id)).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResolveQuery {
    worktree: Option<String>,
}
pub(super) async fn resolve(
    State(state): State<ServerState>,
    Path((id, source)): Path<(String, String)>,
    query: Result<Query<ResolveQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid source query",
        );
    };
    run_read(state, move |s| {
        let source = source
            .parse::<i64>()
            .map_err(|_| AppError::invalid("sourceId", "expected a numeric source ID"))?;
        let worktree = q
            .worktree
            .as_deref()
            .map(|p| s.project_source_http_worktree(&id, source, absolute(p)?))
            .transpose()?;
        s.project_source_resolve(&id, source, worktree.as_deref())
    })
    .await
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ContextQuery {
    source_id: i64,
    worktree: Option<String>,
    #[serde(default = "context_budget")]
    budget_bytes: usize,
}
fn context_budget() -> usize {
    8000
}
pub(super) async fn context(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    query: Result<Query<ContextQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid project context query",
        );
    };
    // HTTP exposes bounded navigation only, not arbitrary file bodies or inferred worktrees.
    run_read(state, move |s| {
        let worktree = q
            .worktree
            .as_deref()
            .map(|p| s.project_source_http_worktree(&id, q.source_id, absolute(p)?))
            .transpose()?;
        s.project_context(
            &id,
            q.source_id,
            ProjectContextOptions {
                worktree: worktree.as_deref(),
                files: &[],
                dependencies: &[],
                budget_bytes: q.budget_bytes,
            },
        )
    })
    .await
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum SourceInput {
    Git {
        worktree: String,
        relative_path: String,
    },
    Directory {
        path: String,
    },
}
impl SourceInput {
    pub(super) fn add(
        self,
        s: &Service,
        project_id: i64,
        revision: i64,
        component: Option<&str>,
    ) -> AppResult<Outcome> {
        let location = match &self {
            Self::Git {
                worktree,
                relative_path,
            } => SourceLocation::Git {
                worktree: absolute(worktree)?,
                relative_path,
            },
            Self::Directory { path } => SourceLocation::Directory(absolute(path)?),
        };
        s.project_source_add(&format!("##{project_id}"), revision, component, location)
    }
}
