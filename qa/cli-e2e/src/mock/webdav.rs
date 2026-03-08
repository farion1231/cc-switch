use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedRequest {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body_size: usize,
}

#[derive(Debug, Clone)]
struct StoredFile {
    body: Vec<u8>,
    etag: String,
}

#[derive(Clone)]
struct WebDavState {
    files: Arc<RwLock<HashMap<String, StoredFile>>>,
    directories: Arc<RwLock<HashSet<String>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

pub struct WebDavServer {
    address: SocketAddr,
    state: WebDavState,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl WebDavServer {
    pub async fn start() -> Result<Self> {
        let state = WebDavState {
            files: Arc::new(RwLock::new(HashMap::new())),
            directories: Arc::new(RwLock::new(HashSet::from([normalize_path("/dav")]))),
            requests: Arc::new(Mutex::new(Vec::new())),
        };

        let app = Router::new()
            .route("/dav", any(handle_request))
            .route("/dav/*path", any(handle_request))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind webdav mock server")?;
        let address = listener
            .local_addr()
            .context("failed to inspect webdav mock server address")?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Ok(Self {
            address,
            state,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}/dav", self.address)
    }

    pub async fn requests_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&*self.state.requests.lock().await).map_err(Into::into)
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle_request(
    State(state): State<WebDavState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = normalize_path(uri.path());
    let query = uri.query().unwrap_or_default().to_string();
    let recorded_headers = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|text| (name.as_str().to_string(), text.to_string()))
        })
        .collect::<HashMap<_, _>>();

    state.requests.lock().await.push(RecordedRequest {
        method: method.to_string(),
        path: path.clone(),
        query,
        headers: recorded_headers,
        body_size: body.len(),
    });

    match method.as_str() {
        "PROPFIND" => (
            StatusCode::MULTI_STATUS,
            [("content-type", "application/xml")],
            "<?xml version=\"1.0\"?><multistatus xmlns=\"DAV:\"/>",
        )
            .into_response(),
        "MKCOL" => {
            state.directories.write().await.insert(path);
            StatusCode::CREATED.into_response()
        }
        "PUT" => {
            let etag = format!("\"{}-{}\"", uri.path().len(), body.len());
            state.files.write().await.insert(
                path,
                StoredFile {
                    body: body.to_vec(),
                    etag: etag.clone(),
                },
            );
            (StatusCode::CREATED, [("etag", etag)]).into_response()
        }
        "HEAD" => head_response(&state, &path).await,
        "GET" => get_response(&state, &path).await,
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn head_response(state: &WebDavState, path: &str) -> Response {
    let Some(file) = state.files.read().await.get(path).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        StatusCode::OK,
        [
            ("etag", file.etag),
            ("content-length", file.body.len().to_string()),
        ],
    )
        .into_response()
}

async fn get_response(state: &WebDavState, path: &str) -> Response {
    let Some(file) = state.files.read().await.get(path).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        StatusCode::OK,
        [
            ("etag", file.etag),
            ("content-type", content_type_for_path(path).to_string()),
        ],
        file.body,
    )
        .into_response()
}

fn normalize_path(raw: &str) -> String {
    if raw.len() > 1 {
        raw.trim_end_matches('/').to_string()
    } else {
        raw.to_string()
    }
}

fn content_type_for_path(path: &str) -> &'static str {
    if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    }
}
