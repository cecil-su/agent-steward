//! Browser approval controls; authentication data stays outside the task database.
use crate::{Access, ServerState, auth_job, browser_auth::AccessError, browser_result, failure};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub(crate) struct Peer(pub Option<std::net::IpAddr>);
fn success(data: Value) -> Response {
    Json(json!({"schemaVersion":3,"ok":true,"data":data,"warnings":[],"error":null}))
        .into_response()
}
fn failed(error: AccessError) -> Response {
    match error {
        AccessError::Limit => failure(
            StatusCode::TOO_MANY_REQUESTS,
            "ACCESS_LIMIT",
            "too many access requests or browser grants; wait before trying again",
        ),
        AccessError::Conflict => failure(
            StatusCode::CONFLICT,
            "ACCESS_CHANGED",
            "request expired, was handled, code differs, or targets the current browser; refresh before deciding",
        ),
        AccessError::Store => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "AUTH_STORE_UNAVAILABLE",
            "access authorization storage unavailable",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Apply {
    label: String,
}
pub(crate) async fn request(
    State(state): State<ServerState>,
    Extension(access): Extension<Access>,
    Extension(peer): Extension<Peer>,
    headers: HeaderMap,
    Json(body): Json<Apply>,
) -> Response {
    if access.role.is_some() {
        return success(json!({"state":"authorized"}));
    }
    let label = body.label.trim().to_owned();
    if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
        return failure(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "browser label must be 1-80 printable characters",
        );
    }
    if state.readonly_token.is_none() {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "ACCESS_DISABLED",
            "reader approval is not configured",
        );
    }
    let Some(peer) = peer.0 else {
        return failure(
            StatusCode::BAD_REQUEST,
            "PEER_UNAVAILABLE",
            "direct peer metadata required",
        );
    };
    let agent: String = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    let worker = state.clone();
    auth_job(&state, move || {
        match worker.browser_auth.request_access(
            &worker.origin,
            &label,
            &peer.to_string(),
            &agent,
            worker.cookie(&headers),
        ) {
            Ok((cookie, data)) => {
                let mut response = success(data);
                if let Some(cookie) = cookie {
                    let issued = browser_result(&worker, None, Some(&cookie));
                    response
                        .headers_mut()
                        .insert("set-cookie", issued.headers()["set-cookie"].clone());
                }
                response
            }
            Err(error) => failed(error),
        }
    })
    .await
    .unwrap_or_else(|r| *r)
}
pub(crate) async fn status(
    State(state): State<ServerState>,
    Extension(access): Extension<Access>,
    headers: HeaderMap,
) -> Response {
    if access.role.is_some() {
        return success(json!({"state":"authorized"}));
    }
    let worker = state.clone();
    auth_job(&state, move || {
        match worker
            .browser_auth
            .request_status(&worker.origin, worker.cookie(&headers))
        {
            Ok(data) => success(data),
            Err(_) => failed(AccessError::Store),
        }
    })
    .await
    .unwrap_or_else(|r| *r)
}
pub(crate) async fn list(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    let worker = state.clone();
    auth_job(&state, move || {
        match worker.browser_auth.access_list(
            &worker.origin,
            &worker.token,
            worker.readonly_token.as_deref(),
            worker.cookie(&headers),
        ) {
            Ok(data) => success(data),
            Err(_) => failed(AccessError::Store),
        }
    })
    .await
    .unwrap_or_else(|r| *r)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Approve {
    verification_code: String,
}
pub(crate) async fn approve(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(body): Json<Approve>,
) -> Response {
    let Some(credential) = state.readonly_token.clone() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "ACCESS_DISABLED",
            "reader approval is not configured",
        );
    };
    let worker = state.clone();
    auth_job(&state, move || {
        match worker.browser_auth.approve_access(
            &worker.origin,
            &id,
            &body.verification_code,
            &credential,
        ) {
            Ok(()) => success(json!({"state":"approved"})),
            Err(error) => failed(error),
        }
    })
    .await
    .unwrap_or_else(|r| *r)
}
pub(crate) async fn revoke(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let worker = state.clone();
    auth_job(&state, move || {
        match worker
            .browser_auth
            .revoke_access(&worker.origin, &id, worker.cookie(&headers))
        {
            Ok(()) => success(json!({"state":"revoked"})),
            Err(error) => failed(error),
        }
    })
    .await
    .unwrap_or_else(|r| *r)
}
