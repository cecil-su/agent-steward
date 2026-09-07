//! Same-origin HTTP transport. Authentication is checked before extracting bodies.
mod browser_auth;
mod events;
use axum::{
    Extension, Json, Router,
    body::{Bytes, to_bytes},
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::Path as FsPath,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use steward_application::{
    AppError, AppResult, Outcome, Service, TaskListOptions, database_permission_warning,
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct ServerState {
    pub service: Service,
    host: String,
    local_address: std::net::IpAddr,
    trust_local: bool,
    origin: String,
    token: Arc<str>,
    browser_auth: Arc<browser_auth::BrowserAuth>,
    cookie_name: String,
    readonly_token: Option<Arc<str>>,
    event_slots: Arc<Semaphore>,
    event_shutdown: Arc<std::sync::atomic::AtomicBool>,
    slots: Arc<Semaphore>,
    connection_code: Arc<Mutex<Option<(String, Instant)>>>,
}
impl ServerState {
    pub fn new(service: Service, port: u16, token: String) -> Self {
        Self::with_address(service, ([127, 0, 0, 1], port).into(), token)
    }

    pub fn with_address(service: Service, address: std::net::SocketAddr, token: String) -> Self {
        Self {
            service,
            host: address.to_string(),
            local_address: address.ip(),
            trust_local: false,
            origin: format!("http://{address}"),
            token: token.into(),
            browser_auth: Arc::new(browser_auth::BrowserAuth::memory()),
            cookie_name: format!("steward_session_{}", address.port()),
            readonly_token: None,
            event_slots: Arc::new(Semaphore::new(16)),
            event_shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            slots: Arc::new(Semaphore::new(8)),
            connection_code: Arc::new(Mutex::new(None)),
        }
    }

    /// Trust direct local TCP callers. Do not expose this mode through a proxy.
    pub fn with_local_access(mut self) -> Self {
        self.trust_local = true;
        self
    }

    fn local_request(&self, request: &Request) -> bool {
        self.trust_local
            && !request.headers().contains_key("x-steward-token")
            && !request
                .headers()
                .keys()
                .any(|k| k.as_str() == "forwarded" || k.as_str().starts_with("x-forwarded-"))
            && request
                .extensions()
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .is_some_and(|peer| peer.0.ip().is_loopback() || peer.0.ip() == self.local_address)
    }

    pub fn stop_event_streams(&self) {
        self.event_shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn with_readonly_token(mut self, token: String) -> Self {
        assert!(!token.is_empty() && token != self.token.as_ref());
        self.readonly_token = Some(token.into());
        self
    }

    pub fn with_browser_store(
        mut self,
        path: &std::path::Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        self.browser_auth = Arc::new(browser_auth::BrowserAuth::open(path)?);
        Ok(self)
    }

    fn cookie<'a>(&self, headers: &'a HeaderMap) -> Option<&'a str> {
        let mut values = headers
            .get_all("cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|v| v.trim().split_once('='))
            .filter(|(name, _)| *name == self.cookie_name)
            .map(|(_, value)| value);
        let value = values.next()?;
        if values.next().is_some() {
            None
        } else {
            Some(value)
        }
    }

    fn role(&self, headers: &HeaderMap) -> Option<&'static str> {
        if headers.contains_key("x-steward-token") {
            return self.header_role(headers);
        }
        self.browser_auth.role(
            self.cookie(headers)?,
            &self.origin,
            &self.token,
            self.readonly_token.as_deref(),
        )
    }

    fn header_role(&self, headers: &HeaderMap) -> Option<&'static str> {
        if headers.get_all("x-steward-token").iter().count() != 1 {
            return None;
        }
        let supplied = headers.get("x-steward-token")?.as_bytes();
        if constant_time_equal(supplied, self.token.as_bytes()) {
            Some("admin")
        } else if self
            .readonly_token
            .as_ref()
            .is_some_and(|token| constant_time_equal(supplied, token.as_bytes()))
        {
            Some("reader")
        } else {
            None
        }
    }

    /// Hand this URL only to the local browser launcher; never log it.
    pub fn browser_connection_url(&self) -> String {
        let code = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        *self.connection_code.lock().expect("connection code lock") =
            Some((code.clone(), Instant::now() + Duration::from_secs(120)));
        format!("{}/#connect={code}", self.origin)
    }
}

pub fn router(state: ServerState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route("/style.css", get(style))
        .route("/api/connect", post(connect))
        .route("/api/access", get(access))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/browser-sessions/revoke", post(revoke_browsers))
        .route("/api/events", get(events::subscribe))
        .route("/api/tasks", get(tasks))
        .route("/api/tasks/{id}", get(task))
        .route("/api/tasks/{id}/{resource}", get(task_resource))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{id}", get(session))
        .route("/api/sessions/{id}/{resource}", get(session_resource))
        .route("/api/doctor", get(doctor))
        .route("/api/commands/{command}", post(command))
        .route("/api/hook", post(hook))
        .fallback(|| async { failure(StatusCode::NOT_FOUND, "NOT_FOUND", "endpoint not found") })
        .method_not_allowed_fallback(|| async {
            failure(
                StatusCode::METHOD_NOT_ALLOWED,
                "METHOD_NOT_ALLOWED",
                "method not allowed",
            )
        })
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}

fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"schemaVersion":2,"ok":false,"data":null,"warnings":[],"error":{"code":code,"message":message,"retryable":false,"details":{}}}))).into_response()
}
fn constant_time_equal(a: &[u8], b: &[u8]) -> bool {
    let mut difference = a.len() ^ b.len();
    for (index, byte) in b.iter().enumerate() {
        difference |= usize::from(a.get(index).copied().unwrap_or(0) ^ byte);
    }
    difference == 0
}
#[derive(Clone, Copy)]
struct Access {
    role: Option<&'static str>,
    local: bool,
}

async fn boundary(State(state): State<ServerState>, mut request: Request, next: Next) -> Response {
    let local = state.local_request(&request);
    let headers = request.headers();
    let host_ok = headers.get_all("host").iter().count() == 1
        && headers.get("host").and_then(|v| v.to_str().ok()) == Some(state.host.as_str());
    let origin_ok = headers.get_all("origin").iter().count() <= 1
        && headers
            .get("origin")
            .is_none_or(|v| v.to_str().ok() == Some(state.origin.as_str()));
    let fetch_ok = headers
        .get("sec-fetch-site")
        .is_none_or(|v| v == "same-origin" || v == "none");
    let api = request.uri().path().starts_with("/api/");
    // Use one authorization snapshot; concurrent revocation must not turn a reader
    // into an unclassified request between authentication and the write gate.
    let role = if api && host_ok && origin_ok && fetch_ok {
        if local {
            Some("admin")
        } else {
            state.role(headers)
        }
    } else {
        None
    };
    let mut response = if !host_ok || !origin_ok || !fetch_ok {
        failure(
            StatusCode::FORBIDDEN,
            "REQUEST_ORIGIN_REJECTED",
            "request host or origin is not permitted",
        )
    } else if api
        && (local || headers.contains_key("cookie"))
        && !matches!(*request.method(), Method::GET | Method::HEAD)
        && (headers.get_all("x-steward-csrf").iter().count() != 1
            || headers.get("x-steward-csrf").is_none_or(|v| v != "1")
            || headers.get("origin").and_then(|v| v.to_str().ok()) != Some(state.origin.as_str()))
    {
        failure(
            StatusCode::FORBIDDEN,
            "CSRF_REJECTED",
            "same-origin browser write confirmation required",
        )
    } else if api
        // The exchange endpoint authenticates with a short-lived one-use credential
        // in its handler. It never grants access based on the caller's IP.
        && !(matches!(request.uri().path(), "/api/connect" | "/api/logout") && request.method() == Method::POST)
        && role.is_none()
    {
        failure(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "connect using this Daemon's credential",
        )
    } else if api
        && role == Some("reader")
        && !matches!(*request.method(), Method::GET | Method::HEAD)
        && !matches!(
            request.uri().path(),
            "/api/login" | "/api/logout" | "/api/connect"
        )
    {
        failure(
            StatusCode::FORBIDDEN,
            "READ_ONLY",
            "this credential only permits reading",
        )
    } else if api
        && request.method() == Method::POST
        && headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| v.split(';').next().map(str::trim) != Some("application/json"))
    {
        failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "INVALID_INPUT",
            "application/json is required",
        )
    } else {
        let limit = if request.uri().path() == "/api/hook" {
            16 * 1024
        } else {
            1024 * 1024
        };
        // Bound both body size and slow body uploads before handlers touch storage.
        request.extensions_mut().insert(Access { role, local });
        let (parts, body) = request.into_parts();
        match tokio::time::timeout(std::time::Duration::from_secs(10), to_bytes(body, limit)).await
        {
            Ok(Ok(bytes)) => next.run(Request::from_parts(parts, bytes.into())).await,
            Ok(Err(_)) => failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "INVALID_INPUT",
                "request body exceeds limit",
            ),
            Err(_) => failure(
                StatusCode::REQUEST_TIMEOUT,
                "REQUEST_TIMEOUT",
                "request body timed out",
            ),
        }
    };
    // Axum extractor failures (for example invalid percent-encoded paths) must
    // obey the same JSON error contract as application failures.
    if api
        && response.status().is_client_error()
        && response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| !v.starts_with("application/json"))
    {
        response = failure(response.status(), "INVALID_INPUT", "invalid HTTP request");
    }
    for (name, value) in [
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cross-origin-resource-policy", "same-origin"),
        ("x-frame-options", "DENY"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

async fn run(
    state: ServerState,
    operation: impl FnOnce(&Service) -> AppResult<Outcome> + Send + 'static,
) -> Response {
    let permit = match state.slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "SERVER_BUSY",
                "local service is busy; read current state before retrying writes",
            );
        }
    };
    match tokio::task::spawn_blocking(move || {
        let _permit=permit;
        let result=operation(&state.service);
        let warnings=database_permission_warning(state.service.database_path(),state.service.database_path().parent()!=steward_core::default_data_dir().as_deref()).into_iter().collect::<Vec<_>>();
        match result {
            Ok(mut outcome)=> { outcome.warnings.extend(warnings); (StatusCode::OK,json!({"schemaVersion":2,"ok":true,"data":outcome.data,"warnings":outcome.warnings,"error":null})) },
            Err(error)=> {
                let status=match error.body.code.as_str() {
                    "INVALID_INPUT"|"UNSUPPORTED_SCHEMA_VERSION"=>StatusCode::BAD_REQUEST,
                    "NOT_FOUND"=>StatusCode::NOT_FOUND,
                    "VERSION_CONFLICT"|"SESSION_CONFLICT"|"CONSTRAINT_VIOLATION"|"HOOK_EVENT_CONFLICT"|"HOOK_CAPACITY_REACHED"|"WORKTREE_SAFETY_REFUSED"=>StatusCode::CONFLICT,
                    "DATABASE_BUSY"|"WORKTREE_OPERATION_BUSY"=>StatusCode::SERVICE_UNAVAILABLE,
                    _=>StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status,json!({"schemaVersion":2,"ok":false,"data":null,"warnings":warnings,"error":error.body}))
            }
        }
    }).await {
        Ok((status,body))=>(status,Json(body)).into_response(),
        Err(_)=>failure(StatusCode::INTERNAL_SERVER_ERROR,"INTERNAL_ERROR","operation result is unconfirmed; refresh before further writes"),
    }
}

fn browser_result(state: &ServerState, role: Option<&str>, id: Option<&str>) -> Response {
    let cookie = format!(
        "{}={}; Path=/api; HttpOnly; SameSite=Strict; Max-Age={}",
        state.cookie_name,
        id.unwrap_or(""),
        if id.is_some() {
            browser_auth::MAX_AGE
        } else {
            0
        }
    );
    // taskd currently serves HTTP only. A future HTTPS listener must add Secure.
    let mut response =
        Json(json!({"schemaVersion":2,"ok":true,"data":{"role":role},"warnings":[],"error":null}))
            .into_response();
    response.headers_mut().insert(
        "set-cookie",
        HeaderValue::from_str(&cookie).expect("generated cookie"),
    );
    response
}
fn grant_browser(state: &ServerState, headers: &HeaderMap, role: &str) -> Response {
    let credential = if role == "admin" {
        state.token.as_ref()
    } else {
        state.readonly_token.as_deref().unwrap_or("")
    };
    match state
        .browser_auth
        .issue(&state.origin, role, credential, state.cookie(headers))
    {
        Ok(id) => browser_result(state, Some(role), Some(&id)),
        Err(_) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "AUTH_STORE_UNAVAILABLE",
            "cannot create browser authorization; storage unavailable or grant limit reached",
        ),
    }
}
async fn login(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    match state.header_role(&headers) {
        Some(role) => grant_browser(&state, &headers, role),
        None => failure(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "a valid credential is required for first browser authorization",
        ),
    }
}
async fn logout(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if let Some(id) = state.cookie(&headers)
        && state.browser_auth.revoke(id).is_err()
    {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "AUTH_STORE_UNAVAILABLE",
            "could not revoke browser authorization; retry logout",
        );
    }
    browser_result(&state, None, None)
}
async fn revoke_browsers(State(state): State<ServerState>) -> Response {
    if state.browser_auth.revoke_origin(&state.origin).is_err() {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "AUTH_STORE_UNAVAILABLE",
            "could not revoke browser authorizations",
        );
    }
    browser_result(&state, None, None)
}

async fn access(Extension(access): Extension<Access>) -> Response {
    Json(json!({"schemaVersion":2,"ok":true,"data":{"role":access.role,"local":access.local},"warnings":[],"error":null})).into_response()
}

async fn connect(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    let supplied = headers.get("x-steward-connect");
    let mut pending = state.connection_code.lock().expect("connection code lock");
    let valid = headers.get_all("x-steward-connect").iter().count() == 1
        && pending.as_ref().is_some_and(|(code, expires)| {
            Instant::now() < *expires
                && supplied
                    .is_some_and(|value| constant_time_equal(value.as_bytes(), code.as_bytes()))
        });
    if !valid {
        return failure(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "connection link is invalid or expired; use the credential file or restart taskd",
        );
    }
    pending.take();
    grant_browser(&state, &headers, "admin")
}

async fn index() -> impl IntoResponse {
    (
        [("content-type", "text/html; charset=utf-8")],
        include_str!("../web/index.html"),
    )
}
async fn script() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
}
async fn style() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../web/style.css"),
    )
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuery {
    status: Option<String>,
    view: Option<String>,
    task_key: Option<String>,
    query: Option<String>,
    page_size: Option<u32>,
    cursor: Option<String>,
    fields: Option<String>,
}
async fn tasks(
    State(state): State<ServerState>,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid task filters",
        );
    };
    run(state, move |s| {
        if query.status.is_some() && query.view.is_some() {
            return Err(AppError::invalid("view", "cannot combine view and status"));
        }
        let status = match query.view.as_deref() {
            None => query.status,
            Some("active") => Some("active".into()),
            Some("in-progress") => Some("in_progress".into()),
            Some("blocked") => Some("blocked".into()),
            Some("recent") => None,
            Some(_) => return Err(AppError::invalid("view", "unknown view")),
        };
        s.task_list_with_options(&TaskListOptions {
            status,
            task_key: query.task_key,
            query: query.query,
            page_size: query.page_size,
            cursor: query.cursor,
            fields: query
                .fields
                .map(|v| v.split(',').map(str::to_owned).collect())
                .unwrap_or_default(),
        })
    })
    .await
}
async fn task(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    run(state, move |s| s.task_show(&id)).await
}
async fn task_resource(
    State(state): State<ServerState>,
    Path((id, resource)): Path<(String, String)>,
) -> Response {
    run(state, move |s| match resource.as_str() {
        "context" => s.task_context(&id),
        "history" => s.history(&id),
        "notes" => s.task_notes(&id),
        "worktree-status" => s.worktree_status(&id),
        _ => Err(AppError::not_found("Endpoint", "resource")),
    })
    .await
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionQuery {
    task_id: Option<String>,
}
async fn sessions(
    State(state): State<ServerState>,
    query: Result<Query<SessionQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid session filters",
        );
    };
    run(state, move |s| s.session_list(query.task_id.as_deref())).await
}
async fn session(State(state): State<ServerState>, Path(id): Path<String>) -> Response {
    run(state, move |s| s.session_show(&id)).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: u32,
}
fn default_limit() -> u32 {
    100
}
async fn session_resource(
    State(state): State<ServerState>,
    Path((id, resource)): Path<(String, String)>,
    query: Result<Query<EventQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid event pagination",
        );
    };
    run(state, move |s| match resource.as_str() {
        "events" => s.hook_list(&id, query.after, query.limit),
        "imports" => s.session_import_list(&id),
        _ => Err(AppError::not_found("Endpoint", "resource")),
    })
    .await
}
async fn doctor(State(state): State<ServerState>) -> Response {
    run(state, Service::doctor).await
}
async fn hook(State(state): State<ServerState>, body: Bytes) -> Response {
    let Ok(input) = String::from_utf8(body.to_vec()) else {
        return failure(StatusCode::BAD_REQUEST, "INVALID_INPUT", "invalid UTF-8");
    };
    run(state, move |s| s.hook_ingest(&input)).await
}

#[derive(Deserialize)]
#[serde(
    tag = "command",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Command {
    TaskCreate {
        input: steward_core::TaskCreateInput,
    },
    TaskUpdate {
        task_id: i64,
        expected_version: i64,
        patch: Value,
    },
    TaskRetitle {
        task_id: i64,
        expected_version: i64,
        title: String,
    },
    TaskNote {
        task_id: i64,
        expected_version: i64,
        note_type: String,
        text: String,
    },
    TaskBlock {
        task_id: i64,
        expected_version: i64,
        reason: String,
        recovery: String,
    },
    TaskUnblock {
        task_id: i64,
        expected_version: i64,
        next_step: String,
    },
    TaskClose {
        task_id: i64,
        expected_version: i64,
        outcome: String,
        reason: Option<String>,
        confirmed: bool,
    },
    TaskClaim {
        task_id: i64,
        expected_version: i64,
        session_id: String,
        #[serde(default)]
        take_over: bool,
    },
    TaskResume {
        task_id: i64,
        expected_version: i64,
        session_id: String,
        from_session: Option<String>,
        #[serde(default)]
        take_over: bool,
    },
    TaskCheckpoint {
        task_id: i64,
        expected_version: i64,
        session_id: String,
        input: steward_core::CheckpointInput,
    },
    SessionBind {
        session_id: String,
        expected_version: i64,
        source: String,
        external_session_id: String,
    },
    SessionAttach {
        task_id: i64,
        expected_version: i64,
        session_id: String,
        source: Option<String>,
        external_session_id: Option<String>,
        record_path: Option<String>,
    },
    SessionClose {
        session_id: String,
        expected_version: i64,
        confirmed: bool,
    },
    SessionImportAdd {
        task_id: i64,
        expected_version: i64,
        session_id: String,
        path: String,
        confirm_sensitive_content_reviewed: bool,
    },
    SessionImportRemove {
        import_id: String,
        expected_version: i64,
        confirmed: bool,
    },
    HookClear {
        session_id: String,
        expected_version: i64,
        confirmed: bool,
    },
    WorktreeCreate {
        task_id: i64,
        expected_version: i64,
        repo: String,
        branch: String,
        path: String,
        confirmed: bool,
    },
    WorktreeAdopt {
        task_id: i64,
        expected_version: i64,
        repo: String,
        path: String,
        confirmed: bool,
    },
    WorktreeRemove {
        task_id: i64,
        expected_version: i64,
        confirmed: bool,
    },
    WorktreeDetach {
        task_id: i64,
        expected_version: i64,
        expected_path: String,
        confirmed: bool,
    },
}
fn confirm(value: bool) -> AppResult<()> {
    if value {
        Ok(())
    } else {
        Err(AppError::invalid(
            "confirmed",
            "explicit confirmation required",
        ))
    }
}
fn absolute(value: &str) -> AppResult<&FsPath> {
    let p = FsPath::new(value);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(AppError::invalid("path", "HTTP paths must be absolute"))
    }
}
fn execute(s: &Service, command: Command) -> AppResult<Outcome> {
    match command {
        Command::TaskCreate { input } => {
            s.task_create_with_options(None, Some(&serde_json::to_string(&input).unwrap()))
        }
        Command::TaskUpdate {
            task_id,
            expected_version,
            patch,
        } => s.task_update(&task_id.to_string(), expected_version, &patch.to_string()),
        Command::TaskRetitle {
            task_id,
            expected_version,
            title,
        } => s.task_retitle(&task_id.to_string(), expected_version, &title),
        Command::TaskNote {
            task_id,
            expected_version,
            note_type,
            text,
        } => s.task_note(&task_id.to_string(), expected_version, &note_type, &text),
        Command::TaskBlock {
            task_id,
            expected_version,
            reason,
            recovery,
        } => s.task_block(&task_id.to_string(), expected_version, &reason, &recovery),
        Command::TaskUnblock {
            task_id,
            expected_version,
            next_step,
        } => s.task_unblock(&task_id.to_string(), expected_version, &next_step),
        Command::TaskClose {
            task_id,
            expected_version,
            outcome,
            reason,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.task_close(
                &task_id.to_string(),
                expected_version,
                &outcome,
                reason.as_deref(),
            )
        }
        Command::TaskClaim {
            task_id,
            expected_version,
            session_id,
            take_over,
        } => s.task_claim(
            &task_id.to_string(),
            expected_version,
            &session_id,
            take_over,
        ),
        Command::TaskResume {
            task_id,
            expected_version,
            session_id,
            from_session,
            take_over,
        } => s.task_resume(
            &task_id.to_string(),
            expected_version,
            &session_id,
            from_session.as_deref(),
            take_over,
        ),
        Command::TaskCheckpoint {
            task_id,
            expected_version,
            session_id,
            input,
        } => s.task_checkpoint(
            &task_id.to_string(),
            expected_version,
            &session_id,
            &serde_json::to_string(&input).unwrap(),
        ),
        Command::SessionBind {
            session_id,
            expected_version,
            source,
            external_session_id,
        } => s.session_bind(&session_id, expected_version, &source, &external_session_id),
        Command::SessionAttach {
            task_id,
            expected_version,
            session_id,
            source,
            external_session_id,
            record_path,
        } => s.session_attach(
            &task_id.to_string(),
            expected_version,
            &session_id,
            source.as_deref(),
            external_session_id.as_deref(),
            record_path.as_deref().map(absolute).transpose()?,
        ),
        Command::SessionClose {
            session_id,
            expected_version,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.session_close(&session_id, expected_version)
        }
        Command::SessionImportAdd {
            task_id,
            expected_version,
            session_id,
            path,
            confirm_sensitive_content_reviewed,
        } => s.session_import_add(
            &task_id.to_string(),
            &session_id,
            expected_version,
            absolute(&path)?,
            confirm_sensitive_content_reviewed,
        ),
        Command::SessionImportRemove {
            import_id,
            expected_version,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.session_import_remove(&import_id, expected_version)
        }
        Command::HookClear {
            session_id,
            expected_version,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.hook_clear(&session_id, expected_version)
        }
        Command::WorktreeCreate {
            task_id,
            expected_version,
            repo,
            branch,
            path,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.worktree_create(
                &task_id.to_string(),
                expected_version,
                absolute(&repo)?,
                &branch,
                absolute(&path)?,
            )
        }
        Command::WorktreeAdopt {
            task_id,
            expected_version,
            repo,
            path,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.worktree_adopt(
                &task_id.to_string(),
                expected_version,
                absolute(&repo)?,
                absolute(&path)?,
            )
        }
        Command::WorktreeRemove {
            task_id,
            expected_version,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.worktree_remove(&task_id.to_string(), expected_version)
        }
        Command::WorktreeDetach {
            task_id,
            expected_version,
            expected_path,
            confirmed,
        } => {
            confirm(confirmed)?;
            s.worktree_detach(
                &task_id.to_string(),
                expected_version,
                absolute(&expected_path)?,
            )
        }
    }
}
async fn command(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let Ok(Value::Object(mut object)) = serde_json::from_slice(&body) else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "expected a command JSON object",
        );
    };
    if object.contains_key("command") {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "command comes from endpoint",
        );
    }
    object.insert("command".into(), json!(name));
    let Ok(command) = serde_json::from_value(Value::Object(object)) else {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "unknown command, missing or invalid fields",
        );
    };
    run(state, move |s| execute(s, command)).await
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[tokio::test]
    async fn expired_and_disabled_connection_codes_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let state = ServerState::new(
            Service::new(temp.path().join("unused.db")),
            1234,
            "synthetic".into(),
        );
        let mut headers = HeaderMap::new();
        headers.insert("x-steward-connect", HeaderValue::from_static("expired"));
        assert_eq!(
            connect(State(state.clone()), headers.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        *state.connection_code.lock().unwrap() =
            Some(("expired".into(), Instant::now() - Duration::from_secs(1)));
        assert_eq!(
            connect(State(state), headers).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(!temp.path().join("unused.db").exists());
    }
}
