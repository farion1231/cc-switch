//! Transport-level tests against local fake WebDAV / S3 servers.
//!
//! The fakes implement just enough of each protocol (Basic auth, MKCOL
//! parent checks, PROPFIND Depth 0/1 multistatus, S3 path-style objects and
//! ListObjectsV2 with delimiter + forced small pages) to drive the real
//! reqwest-based transport code end to end over loopback HTTP.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;

use super::transport::UsageTransport;
use super::{sync_with, SyncOptions};
use crate::database::Database;
use crate::services::usage_stats::UsageDeviceScope;
use crate::settings::{S3SyncSettings, UsageSyncDetailLevel, WebDavSyncSettings};

const NOW: i64 = 1_760_000_000;

fn decode(path: &str) -> String {
    percent_encoding::percent_decode_str(path)
        .decode_utf8_lossy()
        .to_string()
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            percent_encoding::utf8_percent_encode(seg, percent_encoding::NON_ALPHANUMERIC)
                .to_string()
                .replace("%2D", "-")
                .replace("%2E", ".")
                .replace("%5F", "_")
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn init_http() {
    let _ = crate::proxy::http_client::init(None);
}

// ─── Fake WebDAV ─────────────────────────────────────────────

#[derive(Default)]
struct DavState {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
    auth: String,
    requests: Vec<String>,
}

type Dav = Arc<Mutex<DavState>>;

fn parent_of(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent.to_string(),
        _ => "/".to_string(),
    }
}

async fn dav_handler(
    State(state): State<Dav>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut st = state.lock().unwrap();
    let path = decode(uri.path()).trim_end_matches('/').to_string();
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path
    };
    st.requests.push(format!("{method} {path}"));
    let authorized = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == st.auth);
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match method.as_str() {
        "MKCOL" => {
            if st.dirs.contains(&path) {
                return StatusCode::METHOD_NOT_ALLOWED.into_response();
            }
            if !st.dirs.contains(&parent_of(&path)) {
                return StatusCode::CONFLICT.into_response();
            }
            st.dirs.insert(path);
            StatusCode::CREATED.into_response()
        }
        "PUT" => {
            if !st.dirs.contains(&parent_of(&path)) {
                return StatusCode::CONFLICT.into_response();
            }
            st.files.insert(path, body.to_vec());
            StatusCode::CREATED.into_response()
        }
        "GET" => match st.files.get(&path) {
            Some(bytes) => (StatusCode::OK, bytes.clone()).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        "HEAD" => match st.files.get(&path) {
            Some(_) => StatusCode::OK.into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        "DELETE" => match st.files.remove(&path) {
            Some(_) => StatusCode::NO_CONTENT.into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        "PROPFIND" => {
            if !st.dirs.contains(&path) && !st.files.contains_key(&path) {
                return StatusCode::NOT_FOUND.into_response();
            }
            let depth = headers
                .get("depth")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("1");
            let mut entries = vec![(path.clone(), st.dirs.contains(&path))];
            if depth == "1" {
                let prefix = if path == "/" {
                    "/".to_string()
                } else {
                    format!("{path}/")
                };
                for dir in &st.dirs {
                    if let Some(rest) = dir.strip_prefix(&prefix) {
                        if !rest.is_empty() && !rest.contains('/') {
                            entries.push((dir.clone(), true));
                        }
                    }
                }
                for file in st.files.keys() {
                    if let Some(rest) = file.strip_prefix(&prefix) {
                        if !rest.contains('/') {
                            entries.push((file.clone(), false));
                        }
                    }
                }
            }
            let mut xml = String::from(
                r#"<?xml version="1.0" encoding="utf-8"?><d:multistatus xmlns:d="DAV:">"#,
            );
            for (entry, is_dir) in entries {
                let href = if is_dir {
                    format!("{}/", encode_path(&entry))
                } else {
                    encode_path(&entry)
                };
                let kind = if is_dir { "<d:collection/>" } else { "" };
                xml.push_str(&format!(
                    "<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:resourcetype>{kind}</d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
                ));
            }
            xml.push_str("</d:multistatus>");
            (StatusCode::MULTI_STATUS, xml).into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn start_dav(user: &str, password: &str) -> (String, Dav) {
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    let state: Dav = Arc::new(Mutex::new(DavState {
        auth: format!("Basic {token}"),
        ..Default::default()
    }));
    state.lock().unwrap().dirs.insert("/".into());
    state.lock().unwrap().dirs.insert("/dav".into());
    let router = Router::new()
        .fallback(dav_handler)
        .with_state(state.clone());
    let base = serve(router).await;
    (format!("{base}/dav/"), state)
}

fn webdav_settings(base_url: &str, password: &str) -> WebDavSyncSettings {
    WebDavSyncSettings {
        base_url: base_url.to_string(),
        username: "alice".into(),
        password: password.into(),
        remote_root: "cc-switch-sync".into(),
        profile: "default".into(),
        ..WebDavSyncSettings::default()
    }
}

// ─── Fake S3 ─────────────────────────────────────────────────

#[derive(Default)]
struct S3State {
    objects: BTreeMap<String, Vec<u8>>,
    page_size: usize,
    list_calls: usize,
}

type S3 = Arc<Mutex<S3State>>;

fn query_map(uri: &Uri) -> BTreeMap<String, String> {
    uri.query()
        .unwrap_or_default()
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

async fn s3_handler(
    State(state): State<S3>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let auth_ok = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("AWS4-HMAC-SHA256 Credential=AKIDTEST/"));
    if !auth_ok {
        return StatusCode::FORBIDDEN.into_response();
    }
    let declared = headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if declared != crate::services::sync_protocol::sha256_hex(&body) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let path = decode(uri.path());
    let Some(rest) = path.strip_prefix("/bucket") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let key = rest.trim_start_matches('/').to_string();
    let mut st = state.lock().unwrap();
    let query = query_map(&uri);

    if key.is_empty()
        && method == Method::GET
        && query.get("list-type").map(String::as_str) == Some("2")
    {
        st.list_calls += 1;
        let prefix = query.get("prefix").cloned().unwrap_or_default();
        let delimiter = query.get("delimiter").cloned();
        let token = query.get("continuation-token").cloned();
        let mut items: BTreeSet<(String, bool)> = BTreeSet::new();
        for object in st.objects.keys() {
            let Some(rest) = object.strip_prefix(&prefix) else {
                continue;
            };
            match delimiter
                .as_deref()
                .and_then(|d| rest.find(d).map(|i| (d, i)))
            {
                Some((d, idx)) => {
                    items.insert((format!("{prefix}{}{d}", &rest[..idx]), true));
                }
                None => {
                    items.insert((object.clone(), false));
                }
            }
        }
        let items: Vec<_> = items
            .into_iter()
            .filter(|(name, _)| token.as_ref().is_none_or(|t| name > t))
            .collect();
        let page: Vec<_> = items.iter().take(st.page_size).cloned().collect();
        let truncated = items.len() > page.len();
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#,
        );
        xml.push_str(&format!("<Prefix>{}</Prefix>", xml_escape(&prefix)));
        xml.push_str(&format!("<IsTruncated>{truncated}</IsTruncated>"));
        if truncated {
            let next = page.last().map(|(n, _)| n.clone()).unwrap_or_default();
            xml.push_str(&format!(
                "<NextContinuationToken>{}</NextContinuationToken>",
                xml_escape(&next)
            ));
        }
        for (name, is_prefix) in page {
            if is_prefix {
                xml.push_str(&format!(
                    "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
                    xml_escape(&name)
                ));
            } else {
                xml.push_str(&format!(
                    "<Contents><Key>{}</Key></Contents>",
                    xml_escape(&name)
                ));
            }
        }
        xml.push_str("</ListBucketResult>");
        return (StatusCode::OK, xml).into_response();
    }

    match method.as_str() {
        "HEAD" if key.is_empty() => StatusCode::OK.into_response(),
        "PUT" => {
            st.objects.insert(key, body.to_vec());
            StatusCode::OK.into_response()
        }
        "GET" | "HEAD" => match st.objects.get(&key) {
            Some(bytes) => (StatusCode::OK, bytes.clone()).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        "DELETE" => {
            st.objects.remove(&key);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn start_s3(page_size: usize) -> (String, S3) {
    let state: S3 = Arc::new(Mutex::new(S3State {
        page_size,
        ..Default::default()
    }));
    let router = Router::new().fallback(s3_handler).with_state(state.clone());
    (serve(router).await, state)
}

fn s3_settings(endpoint: &str) -> S3SyncSettings {
    S3SyncSettings {
        region: "us-east-1".into(),
        bucket: "bucket".into(),
        access_key_id: "AKIDTEST".into(),
        secret_access_key: "s3-secret-value".into(),
        endpoint: endpoint.to_string(),
        remote_root: "cc-switch-sync".into(),
        profile: "default".into(),
        ..S3SyncSettings::default()
    }
}

// ─── Shared helpers ──────────────────────────────────────────

fn device_with_logs(prefix: &str, count: usize) -> Arc<Database> {
    let db = Arc::new(Database::memory().unwrap());
    {
        let conn = db.conn.lock().unwrap();
        for i in 0..count {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens, output_tokens,
                    input_token_semantics, total_cost_usd, latency_ms, status_code, created_at
                 ) VALUES (?1, 'p', 'codex', 'gpt-5', 10, 5, 2, '0.001', 50, 200, ?2)",
                rusqlite::params![format!("{prefix}-{i}"), NOW - (i as i64) * 600],
            )
            .unwrap();
        }
    }
    db
}

fn options(name: &str) -> SyncOptions {
    SyncOptions {
        detail_level: UsageSyncDetailLevel::Records,
        system_device_name: name.to_string(),
        platform: "test".into(),
        app_version: "0.0.0-test".into(),
        now: NOW,
    }
}

fn all_requests(db: &Database) -> u64 {
    db.get_usage_summary_scoped(None, None, None, None, None, &UsageDeviceScope::All)
        .unwrap()
        .total_requests
}

// ─── WebDAV ──────────────────────────────────────────────────

#[tokio::test]
async fn webdav_two_devices_roundtrip_and_isolation() {
    init_http();
    let (base_url, dav) = start_dav("alice", "dav-secret").await;
    // An existing configuration snapshot must not be touched.
    {
        let mut st = dav.lock().unwrap();
        for dir in [
            "/dav/cc-switch-sync",
            "/dav/cc-switch-sync/v2",
            "/dav/cc-switch-sync/v2/db-v6",
            "/dav/cc-switch-sync/v2/db-v6/default",
        ] {
            st.dirs.insert(dir.to_string());
        }
        st.files.insert(
            "/dav/cc-switch-sync/v2/db-v6/default/manifest.json".into(),
            b"{\"config\":true}".to_vec(),
        );
    }
    let transport = UsageTransport::from_webdav(&webdav_settings(&base_url, "dav-secret")).unwrap();
    let a = device_with_logs("a", 3);
    let b = device_with_logs("b", 4);

    let ra = sync_with(&a, &transport, &options("A")).await.unwrap();
    assert!(ra.upload_error.is_none(), "{ra:?}");
    let rb = sync_with(&b, &transport, &options("B")).await.unwrap();
    assert!(
        rb.upload_error.is_none() && rb.download_error.is_none(),
        "{rb:?}"
    );
    let ra = sync_with(&a, &transport, &options("A")).await.unwrap();
    assert_eq!(ra.devices_seen, 1);
    assert_eq!(all_requests(&a), 7);
    assert_eq!(all_requests(&b), 7);

    let st = dav.lock().unwrap();
    assert_eq!(
        st.files
            .get("/dav/cc-switch-sync/v2/db-v6/default/manifest.json"),
        Some(&b"{\"config\":true}".to_vec())
    );
    assert!(st
        .files
        .keys()
        .filter(|k| !k.contains("/v2/"))
        .all(|k| k.starts_with("/dav/cc-switch-sync/usage-v1/default/devices/")));
    assert!(st
        .requests
        .iter()
        .any(|r| r.starts_with("PROPFIND /dav/cc-switch-sync/usage-v1/default/devices")));
}

#[tokio::test]
async fn webdav_auth_failure_is_reported_without_credentials() {
    init_http();
    let (base_url, _dav) = start_dav("alice", "right-password").await;
    let transport =
        UsageTransport::from_webdav(&webdav_settings(&base_url, "wrong-password-xyz")).unwrap();
    let a = device_with_logs("a", 1);
    let report = sync_with(&a, &transport, &options("A")).await.unwrap();
    let upload_error = report.upload_error.expect("upload must fail");
    assert!(upload_error.contains("401"), "{upload_error}");
    assert!(!upload_error.contains("wrong-password-xyz"));
    assert!(report.download_error.is_some());
}

#[tokio::test]
async fn webdav_listing_of_missing_namespace_is_empty_not_an_error() {
    init_http();
    let (base_url, _dav) = start_dav("alice", "pw").await;
    let transport = UsageTransport::from_webdav(&webdav_settings(&base_url, "pw")).unwrap();
    assert!(transport.list_device_ids().await.unwrap().is_empty());
}

// ─── S3 ──────────────────────────────────────────────────────

#[tokio::test]
async fn s3_multi_device_discovery_paginates() {
    init_http();
    let (endpoint, s3) = start_s3(2).await;
    let transport = UsageTransport::from_s3(&s3_settings(&endpoint)).unwrap();
    let devices: Vec<_> = (0..5)
        .map(|i| device_with_logs(&format!("d{i}"), 2))
        .collect();
    for (i, db) in devices.iter().enumerate() {
        let report = sync_with(db, &transport, &options(&format!("D{i}")))
            .await
            .unwrap();
        assert!(report.upload_error.is_none(), "{report:?}");
    }
    s3.lock().unwrap().list_calls = 0;
    let report = sync_with(&devices[0], &transport, &options("D0"))
        .await
        .unwrap();
    assert_eq!(report.devices_seen, 4);
    assert!(
        s3.lock().unwrap().list_calls >= 3,
        "5 prefixes at 2 per page → 3 pages"
    );
    assert_eq!(all_requests(&devices[0]), 10);
    assert!(s3
        .lock()
        .unwrap()
        .objects
        .keys()
        .all(|k| k.starts_with("cc-switch-sync/usage-v1/default/devices/")));
}

#[tokio::test]
async fn s3_garbage_collection_deletes_retired_objects() {
    init_http();
    let (endpoint, s3) = start_s3(1000).await;
    let transport = UsageTransport::from_s3(&s3_settings(&endpoint)).unwrap();
    let db = device_with_logs("a", 1);
    sync_with(&db, &transport, &options("A")).await.unwrap();
    let first: BTreeSet<String> = s3.lock().unwrap().objects.keys().cloned().collect();
    {
        let conn = db.conn.lock().unwrap();
        conn.execute("UPDATE proxy_request_logs SET total_cost_usd = '0.5'", [])
            .unwrap();
    }
    sync_with(&db, &transport, &options("A")).await.unwrap();
    let mut later = options("A");
    later.now += super::protocol::RETIRED_GRACE_SECS + 1;
    sync_with(&db, &transport, &later).await.unwrap();
    let now_keys: BTreeSet<String> = s3.lock().unwrap().objects.keys().cloned().collect();
    let removed: Vec<_> = first.difference(&now_keys).collect();
    assert_eq!(removed.len(), 1, "the superseded part object is gone");
    assert!(removed[0].ends_with(".json.gz"));
}

#[tokio::test]
async fn s3_rejected_credentials_do_not_leak_secret() {
    init_http();
    let (endpoint, _s3) = start_s3(1000).await;
    let mut settings = s3_settings(&endpoint);
    settings.access_key_id = "WRONGKEY".into();
    let transport = UsageTransport::from_s3(&settings).unwrap();
    let db = device_with_logs("a", 1);
    let report = sync_with(&db, &transport, &options("A")).await.unwrap();
    let error = report.upload_error.expect("must fail");
    assert!(error.contains("403"), "{error}");
    assert!(!error.contains("s3-secret-value"));
}
