use regex::Regex;
use std::sync::LazyLock;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::config::write_text_file;
use crate::openclaw_config::get_openclaw_workspace_dir;

/// Allowed workspace filenames (whitelist for security)
const ALLOWED_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "USER.md",
    "IDENTITY.md",
    "TOOLS.md",
    "MEMORY.md",
    "HEARTBEAT.md",
    "BOOTSTRAP.md",
    "BOOT.md",
];

fn validate_filename(filename: &str) -> Result<(), String> {
    if !ALLOWED_FILES.contains(&filename) {
        return Err(format!(
            "Invalid workspace filename: {filename}. Allowed: {}",
            ALLOWED_FILES.join(", ")
        ));
    }
    Ok(())
}

// --- Daily memory files (memory/YYYY-MM-DD.md) ---

static DAILY_MEMORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}\.md$").unwrap());

fn validate_daily_memory_filename(filename: &str) -> Result<(), String> {
    if !DAILY_MEMORY_RE.is_match(filename) {
        return Err(format!(
            "Invalid daily memory filename: {filename}. Expected: YYYY-MM-DD.md"
        ));
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyMemoryFileInfo {
    pub filename: String,
    pub date: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub preview: String,
}

// --- Daily memory commands ---

/// List all daily memory files under `workspace/memory/`.
#[tauri::command]
pub async fn list_daily_memory_files() -> Result<Vec<DailyMemoryFileInfo>, String> {
    let memory_dir = get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .join("memory");

    if !memory_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<DailyMemoryFileInfo> = Vec::new();

    let entries = std::fs::read_dir(&memory_dir)
        .map_err(|e| format!("Failed to read memory directory: {e}"))?;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".md") {
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }

        let date = name.trim_end_matches(".md").to_string();

        let size_bytes = meta.len();
        let modified_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let preview = std::fs::read_to_string(entry.path())
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect::<String>();

        files.push(DailyMemoryFileInfo {
            filename: name,
            date,
            size_bytes,
            modified_at,
            preview,
        });
    }

    // Sort by filename descending (newest date first, YYYY-MM-DD.md)
    files.sort_by(|a, b| b.filename.cmp(&a.filename));

    Ok(files)
}

/// Read a daily memory file.
#[tauri::command]
pub async fn read_daily_memory_file(filename: String) -> Result<Option<String>, String> {
    validate_daily_memory_filename(&filename)?;

    let path = get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .join("memory")
        .join(&filename);

    if !path.exists() {
        return Ok(None);
    }

    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("Failed to read daily memory file {filename}: {e}"))
}

/// Write a daily memory file (atomic write).
#[tauri::command]
pub async fn write_daily_memory_file(filename: String, content: String) -> Result<(), String> {
    validate_daily_memory_filename(&filename)?;

    let memory_dir = get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .join("memory");

    std::fs::create_dir_all(&memory_dir)
        .map_err(|e| format!("Failed to create memory directory: {e}"))?;

    let path = memory_dir.join(&filename);

    write_text_file(&path, &content)
        .map_err(|e| format!("Failed to write daily memory file {filename}: {e}"))
}

/// Find the largest index `<= i` that is a valid UTF-8 char boundary.
/// Equivalent to the unstable `str::floor_char_boundary` (stabilized in 1.91).
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Find the smallest index `>= i` that is a valid UTF-8 char boundary.
/// Equivalent to the unstable `str::ceil_char_boundary` (stabilized in 1.91).
fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Search result for daily memory full-text search.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyMemorySearchResult {
    pub filename: String,
    pub date: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub snippet: String,
    pub match_count: usize,
}

/// Full-text search across all daily memory files.
///
/// Performs case-insensitive search on both the date field and file content.
/// Returns results sorted by filename descending (newest first), each with a
/// snippet showing ~120 characters of context around the first match.
#[tauri::command]
pub async fn search_daily_memory_files(
    query: String,
) -> Result<Vec<DailyMemorySearchResult>, String> {
    let memory_dir = get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .join("memory");

    if !memory_dir.exists() || query.is_empty() {
        return Ok(Vec::new());
    }

    let query_lower = query.to_lowercase();
    let mut results: Vec<DailyMemorySearchResult> = Vec::new();

    let entries = std::fs::read_dir(&memory_dir)
        .map_err(|e| format!("Failed to read memory directory: {e}"))?;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".md") {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };

        let date = name.trim_end_matches(".md").to_string();
        let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
        let content_lower = content.to_lowercase();

        // Count matches in content
        let content_matches: Vec<usize> = content_lower
            .match_indices(&query_lower)
            .map(|(i, _)| i)
            .collect();

        // Also check date field
        let date_matches = date.to_lowercase().contains(&query_lower);

        if content_matches.is_empty() && !date_matches {
            continue;
        }

        // Build snippet around first content match (~120 chars of context)
        let snippet = if let Some(&first_pos) = content_matches.first() {
            let start = if first_pos > 50 {
                floor_char_boundary(&content, first_pos - 50)
            } else {
                0
            };
            let end = ceil_char_boundary(&content, (first_pos + 70).min(content.len()));
            let mut s = String::new();
            if start > 0 {
                s.push_str("...");
            }
            s.push_str(&content[start..end]);
            if end < content.len() {
                s.push_str("...");
            }
            s
        } else {
            // Date-only match — use beginning of file as preview
            let end = ceil_char_boundary(&content, 120.min(content.len()));
            let mut s = content[..end].to_string();
            if end < content.len() {
                s.push_str("...");
            }
            s
        };

        let size_bytes = meta.len();
        let modified_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        results.push(DailyMemorySearchResult {
            filename: name,
            date,
            size_bytes,
            modified_at,
            snippet,
            match_count: content_matches.len(),
        });
    }

    // Sort by filename descending (newest date first)
    results.sort_by(|a, b| b.filename.cmp(&a.filename));

    Ok(results)
}

/// Delete a daily memory file (idempotent).
#[tauri::command]
pub async fn delete_daily_memory_file(filename: String) -> Result<(), String> {
    validate_daily_memory_filename(&filename)?;

    let path = get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .join("memory")
        .join(&filename);

    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete daily memory file {filename}: {e}"))?;
    }

    Ok(())
}

// --- Workspace file commands ---

/// Read an OpenClaw workspace file content.
/// Returns None if the file does not exist.
#[tauri::command]
pub async fn read_workspace_file(filename: String) -> Result<Option<String>, String> {
    validate_filename(&filename)?;

    let path = get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .join(&filename);

    if !path.exists() {
        return Ok(None);
    }

    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("Failed to read workspace file {filename}: {e}"))
}

/// Write content to an OpenClaw workspace file (atomic write).
/// Creates the workspace directory if it does not exist.
#[tauri::command]
pub async fn write_workspace_file(filename: String, content: String) -> Result<(), String> {
    validate_filename(&filename)?;

    let workspace_dir = get_openclaw_workspace_dir().map_err(|e| e.to_string())?;

    // Ensure workspace directory exists
    std::fs::create_dir_all(&workspace_dir)
        .map_err(|e| format!("Failed to create workspace directory: {e}"))?;

    let path = workspace_dir.join(&filename);

    write_text_file(&path, &content)
        .map_err(|e| format!("Failed to write workspace file {filename}: {e}"))
}

/// Return the same workspace root used by all file operations.
#[tauri::command]
pub async fn get_workspace_root_directory() -> Result<String, String> {
    Ok(get_openclaw_workspace_dir()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string())
}

/// Open the resolved workspace root or its memory subdirectory.
#[tauri::command]
pub async fn open_workspace_directory(handle: AppHandle, subdir: String) -> Result<bool, String> {
    let dir = match subdir.as_str() {
        "memory" => get_openclaw_workspace_dir()
            .map_err(|e| e.to_string())?
            .join("memory"),
        _ => get_openclaw_workspace_dir().map_err(|e| e.to_string())?,
    };

    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    handle
        .opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("Failed to open directory: {e}"))?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    use crate::openclaw_config::test_environment::TestEnvironment;

    #[tokio::test]
    #[serial]
    async fn workspace_commands_use_configured_root_for_files_and_memory() {
        let temp = tempfile::tempdir().unwrap();
        let _environment = TestEnvironment::isolated(temp.path());
        let root = temp.path().join("custom-workspace");
        let config_dir = crate::openclaw_config::get_openclaw_dir();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("openclaw.json"),
            serde_json::json!({
                "agents": {"defaults": {"workspace": root}}
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            get_workspace_root_directory().await.unwrap(),
            root.to_string_lossy()
        );
        write_workspace_file("MEMORY.md".into(), "shared memory".into())
            .await
            .unwrap();
        assert_eq!(
            read_workspace_file("MEMORY.md".into())
                .await
                .unwrap()
                .as_deref(),
            Some("shared memory")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("MEMORY.md")).unwrap(),
            "shared memory"
        );

        let filename = "2026-10-09.md".to_string();
        write_daily_memory_file(filename.clone(), "daily needle".into())
            .await
            .unwrap();
        assert_eq!(
            read_daily_memory_file(filename.clone())
                .await
                .unwrap()
                .as_deref(),
            Some("daily needle")
        );
        let files = list_daily_memory_files().await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, filename);
        let results = search_daily_memory_files("needle".into()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, filename);
        delete_daily_memory_file(filename.clone()).await.unwrap();
        assert!(!root.join("memory").join(filename).exists());
        assert!(!config_dir.join("workspace").exists());
    }

    #[tokio::test]
    #[serial]
    async fn workspace_commands_reject_invalid_config_and_unsafe_filenames() {
        let temp = tempfile::tempdir().unwrap();
        let _environment = TestEnvironment::isolated(temp.path());
        let config_dir = crate::openclaw_config::get_openclaw_dir();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("openclaw.json"), "{ broken").unwrap();
        assert!(get_workspace_root_directory().await.is_err());
        assert!(write_workspace_file("MEMORY.md".into(), "text".into())
            .await
            .is_err());
        assert!(
            write_daily_memory_file("2026-10-09.md".into(), "text".into())
                .await
                .is_err()
        );
        assert!(!config_dir.join("workspace").exists());
        std::fs::write(config_dir.join("openclaw.json"), "{}").unwrap();
        assert!(write_workspace_file("../outside.md".into(), "text".into())
            .await
            .is_err());
        assert!(delete_daily_memory_file("../2026-10-09.md".into())
            .await
            .is_err());
    }

    #[tokio::test]
    #[serial]
    async fn workspace_tests_do_not_touch_an_inherited_state_directory() {
        let original_state = std::env::var_os("OPENCLAW_STATE_DIR");
        let inherited = tempfile::tempdir().unwrap();
        std::fs::write(
            inherited.path().join("openclaw.json"),
            r#"{"reviewSentinel":"must-survive"}"#,
        )
        .unwrap();
        let original_environment = TestEnvironment::capture();
        std::env::set_var("OPENCLAW_STATE_DIR", inherited.path());

        let temp = tempfile::tempdir().unwrap();
        let environment = TestEnvironment::isolated(temp.path());
        let config_dir = crate::openclaw_config::get_openclaw_dir();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("openclaw.json"), "{}").unwrap();
        write_workspace_file("MEMORY.md".into(), "isolated".into())
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(inherited.path().join("openclaw.json")).unwrap(),
            r#"{"reviewSentinel":"must-survive"}"#
        );
        drop(environment);
        assert_eq!(
            std::env::var_os("OPENCLAW_STATE_DIR").as_deref(),
            Some(inherited.path().as_os_str())
        );
        drop(original_environment);
        assert_eq!(std::env::var_os("OPENCLAW_STATE_DIR"), original_state);
    }

    #[test]
    #[serial]
    fn workspace_test_environment_restores_unset_and_set_values_after_unwind() {
        let _original_environment = TestEnvironment::capture();
        let temp = tempfile::tempdir().unwrap();
        for original in [None, Some(std::ffi::OsString::from("external-state"))] {
            match &original {
                Some(value) => std::env::set_var("OPENCLAW_STATE_DIR", value),
                None => std::env::remove_var("OPENCLAW_STATE_DIR"),
            }
            for panic in [false, true] {
                let result = std::panic::catch_unwind(|| {
                    let _environment = TestEnvironment::isolated(temp.path());
                    assert!(std::env::var_os("OPENCLAW_STATE_DIR").is_none());
                    assert_eq!(
                        crate::openclaw_config::get_openclaw_dir(),
                        temp.path().join(".openclaw")
                    );
                    if panic {
                        panic!("exercise environment cleanup");
                    }
                });
                assert_eq!(result.is_err(), panic);
                assert_eq!(std::env::var_os("OPENCLAW_STATE_DIR"), original);
            }
        }
    }
}
