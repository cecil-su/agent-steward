//! Project transport only; mutations use the same Application Service/CAS as the CLI.
use super::*;
use steward_application::SourceLocation;

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
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum SourceInput {
    Directory { path: String },
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
            Self::Directory { path } => SourceLocation::Directory(std::path::Path::new(path)),
        };
        s.project_source_add(&format!("##{project_id}"), revision, component, location)
    }
}
