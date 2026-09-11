//! Local, immutable UI packages. No installation or filesystem paths are exposed over HTTP.
use crate::{ServerState, failure};
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex},
};

// Contract 4 requires effective personal/project rules in task/project context.
// Older UIs must not silently omit these rules when handing off task context.
pub const API_CONTRACT: u32 = 4;
const FILES: [&str; 3] = ["index.html", "app.js", "style.css"];
const MAX_FILE: u64 = 4 * 1024 * 1024;
// Checked-in three-file React snapshot; update via web's explicit sync:embedded step.
const HTML: &str = include_str!("../web-readonly/index.html");
const JS: &str = include_str!("../web-readonly/app.js");
const CSS: &str = include_str!("../web-readonly/style.css");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    package_format: u32,
    ui_version: String,
    required_api_contract: u32,
    entry: String,
    files: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    release: String,
}
struct Release {
    id: String,
    version: String,
    files: BTreeMap<String, Vec<u8>>,
}
struct Current {
    release: Arc<Release>,
    error: Option<&'static str>,
}
pub struct UiStore {
    root: Option<PathBuf>,
    current: Mutex<Current>,
    embedded: Arc<Release>,
    slots: Arc<tokio::sync::Semaphore>,
    asset_slots: Arc<tokio::sync::Semaphore>,
    historical: Mutex<Vec<Arc<Release>>>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn plain(path: &FsPath, directory: bool) -> Result<(), &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "UI_PACKAGE_UNAVAILABLE")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("UI_PACKAGE_INVALID");
        }
    }
    if meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err("UI_PACKAGE_INVALID");
    }
    Ok(())
}
fn read(path: &FsPath, limit: u64) -> Result<Vec<u8>, &'static str> {
    plain(path, false)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "UI_PACKAGE_UNAVAILABLE")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "UI_PACKAGE_UNAVAILABLE")?;
    if bytes.len() as u64 > limit {
        return Err("UI_PACKAGE_TOO_LARGE");
    }
    Ok(bytes)
}
impl UiStore {
    pub fn new(root: Option<PathBuf>) -> Self {
        let files: BTreeMap<String, Vec<u8>> = FILES
            .into_iter()
            .zip([HTML, JS, CSS])
            .map(|(name, text)| (name.into(), text.as_bytes().to_vec()))
            .collect();
        let id = format!(
            "embedded-{}",
            hash(
                format!(
                    "{}{}{}",
                    hash(HTML.as_bytes()),
                    hash(JS.as_bytes()),
                    hash(CSS.as_bytes())
                )
                .as_bytes()
            )
        );
        let embedded = Arc::new(Release {
            id,
            version: "embedded".into(),
            files,
        });
        Self {
            root,
            current: Mutex::new(Current {
                release: embedded.clone(),
                error: None,
            }),
            embedded,
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
            asset_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            historical: Mutex::new(Vec::new()),
        }
    }
    fn load(&self, id: &str) -> Result<Arc<Release>, &'static str> {
        if id == self.embedded.id {
            return Ok(self.embedded.clone());
        }
        if !valid_id(id) {
            return Err("UI_PACKAGE_INVALID");
        }
        let root = self.root.as_ref().ok_or("UI_PACKAGE_UNAVAILABLE")?;
        plain(root, true)?;
        plain(&root.join("releases"), true)?;
        let directory = root.join("releases").join(id);
        plain(&directory, true)?;
        let manifest = read(&directory.join("manifest.json"), 16 * 1024)?;
        if hash(&manifest) != id {
            return Err("UI_PACKAGE_HASH_MISMATCH");
        }
        let manifest: Manifest =
            serde_json::from_slice(&manifest).map_err(|_| "UI_PACKAGE_INVALID")?;
        if manifest.package_format != 1 || manifest.required_api_contract != API_CONTRACT {
            return Err("UI_API_INCOMPATIBLE");
        }
        if manifest.entry != "index.html"
            || manifest.ui_version.is_empty()
            || manifest.ui_version.len() > 100
            || !manifest
                .ui_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            || manifest.files.len() != FILES.len()
        {
            return Err("UI_PACKAGE_INVALID");
        }
        let mut files = BTreeMap::new();
        for name in FILES {
            let bytes = read(&directory.join(name), MAX_FILE)?;
            if manifest.files.get(name) != Some(&hash(&bytes)) {
                return Err("UI_PACKAGE_HASH_MISMATCH");
            }
            std::str::from_utf8(&bytes).map_err(|_| "UI_PACKAGE_INVALID")?;
            files.insert(name.into(), bytes);
        }
        let html = std::str::from_utf8(&files["index.html"]).unwrap();
        if !html.contains("src=\"/app.js\"")
            || !html.contains("href=\"/style.css\"")
            || !html.contains("<head>")
        {
            return Err("UI_PACKAGE_INVALID");
        }
        Ok(Arc::new(Release {
            id: id.into(),
            version: manifest.ui_version,
            files,
        }))
    }
    fn cached_release(&self, id: &str) -> Result<Arc<Release>, &'static str> {
        // Immutable verified bytes, at most two historical packages (24 MiB).
        // Serialize misses to avoid hashing the same release concurrently.
        let mut cache = self.historical.lock().expect("historical UI lock");
        if let Some(release) = cache.iter().find(|release| release.id == id) {
            return Ok(release.clone());
        }
        let release = self.load(id)?;
        if cache.len() == 2 {
            cache.remove(0);
        }
        cache.push(release.clone());
        Ok(release)
    }

    fn refresh(&self) -> Arc<Release> {
        let mut current = self.current.lock().expect("UI lock");
        if let Some(root) = &self.root {
            let result = (|| {
                plain(root, true)?;
                let bytes = read(&root.join("current.json"), 1024)?;
                let pointer: Pointer =
                    serde_json::from_slice(&bytes).map_err(|_| "UI_POINTER_INVALID")?;
                if pointer.release == "embedded" {
                    return Ok(self.embedded.clone());
                }
                if pointer.release == current.release.id {
                    return Ok(current.release.clone());
                }
                self.load(&pointer.release)
            })();
            match result {
                Ok(release) => {
                    current.release = release;
                    current.error = None;
                }
                Err(error) => current.error = Some(error),
            }
        }
        current.release.clone()
    }
}
fn content(release: &Release, name: &str) -> Response {
    let Some(bytes) = release.files.get(name) else {
        return failure(StatusCode::NOT_FOUND, "NOT_FOUND", "UI resource not found");
    };
    let (mime, body, cache) = match name {
        "index.html" => {
            let html = String::from_utf8(bytes.clone())
                .expect("validated UTF-8")
                .replace(
                    "<head>",
                    &format!(
                        "<head><meta name=\"steward-ui-release\" content=\"{}\">",
                        release.id
                    ),
                )
                .replace(
                    "href=\"/style.css\"",
                    &format!("href=\"/ui/releases/{}/style.css\"", release.id),
                )
                .replace(
                    "src=\"/app.js\"",
                    &format!("src=\"/ui/releases/{}/app.js\"", release.id),
                );
            ("text/html; charset=utf-8", html.into_bytes(), "no-store")
        }
        "app.js" => (
            "text/javascript; charset=utf-8",
            bytes.clone(),
            "public, max-age=31536000, immutable",
        ),
        "style.css" => (
            "text/css; charset=utf-8",
            bytes.clone(),
            "public, max-age=31536000, immutable",
        ),
        _ => return failure(StatusCode::NOT_FOUND, "NOT_FOUND", "UI resource not found"),
    };
    ([("content-type", mime), ("cache-control", cache)], body).into_response()
}
async fn blocking(
    slots: Arc<tokio::sync::Semaphore>,
    work: impl FnOnce() -> Response + Send + 'static,
) -> Response {
    let Ok(permit) = slots.try_acquire_owned() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "UI_BUSY",
            "UI temporarily busy",
        );
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .unwrap_or_else(|_| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "UI_UNAVAILABLE",
            "UI temporarily unavailable",
        )
    })
}
pub async fn index(State(state): State<ServerState>) -> Response {
    blocking(state.ui.slots.clone(), move || {
        content(&state.ui.refresh(), "index.html")
    })
    .await
}
pub async fn status(State(state): State<ServerState>) -> Response {
    blocking(state.ui.slots.clone(), move || {
        state.ui.refresh();
        let current = state.ui.current.lock().expect("UI lock");
        Json(json!({"packageFormat":1,"apiContract":API_CONTRACT,"release":current.release.id,"uiVersion":current.release.version,"externalEnabled":state.ui.root.is_some(),"error":current.error})).into_response()
    }).await
}
pub async fn asset(
    State(state): State<ServerState>,
    Path((id, name)): Path<(String, String)>,
) -> Response {
    let Ok(asset_permit) = state.ui.asset_slots.clone().try_acquire_owned() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "UI_BUSY",
            "UI assets temporarily busy",
        );
    };
    blocking(state.ui.slots.clone(), move || {
        let _asset_permit = asset_permit;
        if !matches!(name.as_str(), "app.js" | "style.css") {
            return failure(StatusCode::NOT_FOUND, "NOT_FOUND", "UI resource not found");
        }
        let current = state.ui.current.lock().expect("UI lock").release.clone();
        let release = if id == current.id {
            Ok(current)
        } else {
            state.ui.cached_release(&id)
        };
        match release {
            Ok(release) => content(&release, &name),
            Err(_) => failure(StatusCode::NOT_FOUND, "NOT_FOUND", "UI resource not found"),
        }
    })
    .await
}

#[cfg(test)]
mod capacity_tests {
    use super::*;

    #[tokio::test]
    async fn asset_saturation_reserves_index_and_status_capacity() {
        let temp = tempfile::tempdir().unwrap();
        let state = ServerState::new(
            steward_application::Service::new(temp.path().join("isolated.db")),
            43123,
            "synthetic".into(),
        );
        let permits = state
            .ui
            .asset_slots
            .clone()
            .acquire_many_owned(2)
            .await
            .unwrap();
        let id = state.ui.embedded.id.clone();
        assert_eq!(
            asset(State(state.clone()), Path((id.clone(), "app.js".into())))
                .await
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(index(State(state.clone())).await.status(), StatusCode::OK);
        assert_eq!(status(State(state.clone())).await.status(), StatusCode::OK);
        drop(permits);
        assert_eq!(
            asset(State(state), Path((id, "app.js".into())))
                .await
                .status(),
            StatusCode::OK
        );
    }
}
