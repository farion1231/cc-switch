use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

const INDEX_FILE_NAME: &str = "index.json";
const SESSION_DIR_NAME: &str = "sessions";

#[cfg(target_os = "windows")]
const PUBLIC_CAPTURE_ROOT: &str = r"C:\Users\Public\CCSwitchMindTrace";

static INDEX_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DebugCaptureIndex {
    pub version: u32,
    pub updated_at: String,
    pub sessions: BTreeMap<String, DebugCaptureSessionEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DebugCaptureSessionEntry {
    pub session_id: String,
    pub app_type: String,
    pub model: String,
    pub file_name: String,
    pub file_path: String,
    pub first_seen: String,
    pub last_seen: String,
    pub request_count: u64,
    pub response_count: u64,
    pub total_count: u64,
}

impl Default for DebugCaptureIndex {
    fn default() -> Self {
        Self {
            version: 1,
            updated_at: Utc::now().to_rfc3339(),
            sessions: BTreeMap::new(),
        }
    }
}

pub(crate) fn capture_root_dir() -> PathBuf {
    if let Some(custom_dir) = crate::settings::get_debug_capture_output_dir() {
        return custom_dir;
    }

    #[cfg(target_os = "windows")]
    {
        PathBuf::from(PUBLIC_CAPTURE_ROOT)
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::config::get_app_config_dir().join("cc_switch_mindtrace")
    }
}

pub(crate) fn capture_index_path() -> PathBuf {
    capture_root_dir().join(INDEX_FILE_NAME)
}

pub(crate) fn capture_session_path(app_type: &str, session_id: &str) -> PathBuf {
    let app_type = sanitize_file_component(app_type);
    let session_id = sanitize_file_component(session_id);
    capture_root_dir()
        .join(SESSION_DIR_NAME)
        .join(format!("{app_type}__{session_id}.log"))
}

pub(crate) fn append_session_debug_entry(
    app_type: &str,
    session_id: &str,
    model: &str,
    direction: &str,
    entry: &str,
) -> Result<(), std::io::Error> {
    let root = capture_root_dir();
    let sessions_dir = root.join(SESSION_DIR_NAME);
    std::fs::create_dir_all(&sessions_dir)?;

    let session_path = capture_session_path(app_type, session_id);
    append_utf8_entry(&session_path, entry)?;
    update_index(app_type, session_id, model, direction, &session_path)?;
    Ok(())
}

fn append_utf8_entry(path: &PathBuf, entry: &str) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let needs_bom = file.metadata().map(|meta| meta.len() == 0).unwrap_or(false);
    if needs_bom {
        file.write_all(&[0xEF, 0xBB, 0xBF])?;
    }
    file.write_all(entry.as_bytes())?;
    Ok(())
}

fn update_index(
    app_type: &str,
    session_id: &str,
    model: &str,
    direction: &str,
    session_path: &PathBuf,
) -> Result<(), std::io::Error> {
    let _guard = INDEX_WRITE_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("debug capture index lock poisoned"))?;

    let root = capture_root_dir();
    std::fs::create_dir_all(&root)?;
    let index_path = capture_index_path();

    let mut index = if index_path.exists() {
        let raw = std::fs::read_to_string(&index_path)?;
        serde_json::from_str::<DebugCaptureIndex>(&raw).unwrap_or_default()
    } else {
        DebugCaptureIndex::default()
    };
    migrate_legacy_session_keys(&mut index);

    let now = Utc::now().to_rfc3339();
    let file_name = session_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let file_path = session_path.to_string_lossy().to_string();

    let entry = index
        .sessions
        .entry(session_index_key(app_type, session_id))
        .or_insert_with(|| DebugCaptureSessionEntry {
            session_id: session_id.to_string(),
            app_type: app_type.to_string(),
            model: model.to_string(),
            file_name: file_name.clone(),
            file_path: file_path.clone(),
            first_seen: now.clone(),
            last_seen: now.clone(),
            request_count: 0,
            response_count: 0,
            total_count: 0,
        });

    entry.app_type = app_type.to_string();
    if !model.trim().is_empty() {
        entry.model = model.to_string();
    }
    entry.file_name = file_name;
    entry.file_path = file_path;
    entry.last_seen = now.clone();

    let normalized_direction = direction.trim().to_ascii_uppercase();
    if normalized_direction == "REQUEST" {
        entry.request_count = entry.request_count.saturating_add(1);
    } else if normalized_direction == "RESPONSE" {
        entry.response_count = entry.response_count.saturating_add(1);
    }
    entry.total_count = entry.request_count.saturating_add(entry.response_count);

    index.updated_at = now;

    let serialized = serde_json::to_string_pretty(&index)
        .map_err(|err| std::io::Error::other(format!("serialize debug index failed: {err}")))?;
    std::fs::write(index_path, serialized)?;
    Ok(())
}

fn sanitize_file_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

fn session_index_key(app_type: &str, session_id: &str) -> String {
    format!("{app_type}::{session_id}")
}

fn migrate_legacy_session_keys(index: &mut DebugCaptureIndex) {
    let keys = index.sessions.keys().cloned().collect::<Vec<_>>();
    for old_key in keys {
        if old_key.contains("::") {
            continue;
        }

        let Some(entry) = index.sessions.remove(&old_key) else {
            continue;
        };
        let new_key = session_index_key(&entry.app_type, &entry.session_id);
        if let Some(existing) = index.sessions.get_mut(&new_key) {
            existing.request_count = existing.request_count.saturating_add(entry.request_count);
            existing.response_count = existing.response_count.saturating_add(entry.response_count);
            existing.total_count = existing
                .request_count
                .saturating_add(existing.response_count);
            if existing.first_seen > entry.first_seen {
                existing.first_seen = entry.first_seen;
            }
            if existing.last_seen < entry.last_seen {
                existing.last_seen = entry.last_seen;
                existing.file_name = entry.file_name;
                existing.file_path = entry.file_path;
                if !entry.model.trim().is_empty() {
                    existing.model = entry.model;
                }
            }
        } else {
            index.sessions.insert(new_key, entry);
        }
    }
}
