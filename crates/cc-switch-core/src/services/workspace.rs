use once_cell::sync::Lazy;
use regex::Regex;

use crate::config::write_text_file;
use crate::error::AppError;
use crate::openclaw_config::get_openclaw_dir;

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

static DAILY_MEMORY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}\.md$").expect("valid daily memory regex"));

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyMemoryFileInfo {
    pub filename: String,
    pub date: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub preview: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyMemorySearchResult {
    pub filename: String,
    pub date: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub snippet: String,
    pub match_count: usize,
}

pub struct WorkspaceService;

impl WorkspaceService {
    pub fn list_daily_memory_files() -> Result<Vec<DailyMemoryFileInfo>, AppError> {
        let memory_dir = memory_dir();
        if !memory_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        let entries = std::fs::read_dir(&memory_dir).map_err(|e| AppError::io(&memory_dir, e))?;

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".md") {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(metadata) if metadata.is_file() => metadata,
                _ => continue,
            };

            let preview = std::fs::read_to_string(entry.path())
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>();

            files.push(DailyMemoryFileInfo {
                filename: name.clone(),
                date: name.trim_end_matches(".md").to_string(),
                size_bytes: metadata.len(),
                modified_at: modified_unix_secs(&metadata),
                preview,
            });
        }

        files.sort_by(|left, right| right.filename.cmp(&left.filename));
        Ok(files)
    }

    pub fn read_daily_memory_file(filename: &str) -> Result<Option<String>, AppError> {
        validate_daily_memory_filename(filename)?;
        let path = memory_dir().join(filename);
        if !path.exists() {
            return Ok(None);
        }

        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| AppError::io(&path, e))
    }

    pub fn write_daily_memory_file(filename: &str, content: &str) -> Result<(), AppError> {
        validate_daily_memory_filename(filename)?;
        let dir = memory_dir();
        std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        write_text_file(&dir.join(filename), content)
    }

    pub fn search_daily_memory_files(
        query: &str,
    ) -> Result<Vec<DailyMemorySearchResult>, AppError> {
        let memory_dir = memory_dir();
        if !memory_dir.exists() || query.is_empty() {
            return Ok(Vec::new());
        }

        let query_lower = query.to_lowercase();
        let entries = std::fs::read_dir(&memory_dir).map_err(|e| AppError::io(&memory_dir, e))?;
        let mut results = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".md") {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(metadata) if metadata.is_file() => metadata,
                _ => continue,
            };

            let date = name.trim_end_matches(".md").to_string();
            let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let content_lower = content.to_lowercase();
            let content_matches = content_lower
                .match_indices(&query_lower)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let date_matches = date.to_lowercase().contains(&query_lower);

            if content_matches.is_empty() && !date_matches {
                continue;
            }

            results.push(DailyMemorySearchResult {
                filename: name,
                date,
                size_bytes: metadata.len(),
                modified_at: modified_unix_secs(&metadata),
                snippet: build_snippet(&content, content_matches.first().copied()),
                match_count: content_matches.len(),
            });
        }

        results.sort_by(|left, right| right.filename.cmp(&left.filename));
        Ok(results)
    }

    pub fn delete_daily_memory_file(filename: &str) -> Result<(), AppError> {
        validate_daily_memory_filename(filename)?;
        let path = memory_dir().join(filename);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| AppError::io(&path, e))?;
        }
        Ok(())
    }

    pub fn read_workspace_file(filename: &str) -> Result<Option<String>, AppError> {
        validate_workspace_filename(filename)?;
        let path = workspace_dir().join(filename);
        if !path.exists() {
            return Ok(None);
        }

        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| AppError::io(&path, e))
    }

    pub fn write_workspace_file(filename: &str, content: &str) -> Result<(), AppError> {
        validate_workspace_filename(filename)?;
        let dir = workspace_dir();
        std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        write_text_file(&dir.join(filename), content)
    }
}

fn workspace_dir() -> std::path::PathBuf {
    get_openclaw_dir().join("workspace")
}

fn memory_dir() -> std::path::PathBuf {
    workspace_dir().join("memory")
}

fn validate_workspace_filename(filename: &str) -> Result<(), AppError> {
    if ALLOWED_FILES.contains(&filename) {
        return Ok(());
    }

    Err(AppError::InvalidInput(format!(
        "Invalid workspace filename: {filename}. Allowed: {}",
        ALLOWED_FILES.join(", ")
    )))
}

fn validate_daily_memory_filename(filename: &str) -> Result<(), AppError> {
    if DAILY_MEMORY_RE.is_match(filename) {
        return Ok(());
    }

    Err(AppError::InvalidInput(format!(
        "Invalid daily memory filename: {filename}. Expected: YYYY-MM-DD.md"
    )))
}

fn modified_unix_secs(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn build_snippet(content: &str, first_match: Option<usize>) -> String {
    if let Some(first_pos) = first_match {
        let start = if first_pos > 50 {
            floor_char_boundary(content, first_pos - 50)
        } else {
            0
        };
        let end = ceil_char_boundary(content, (first_pos + 70).min(content.len()));
        let mut snippet = String::new();
        if start > 0 {
            snippet.push_str("...");
        }
        snippet.push_str(&content[start..end]);
        if end < content.len() {
            snippet.push_str("...");
        }
        return snippet;
    }

    let end = ceil_char_boundary(content, 120.min(content.len()));
    let mut snippet = content[..end].to_string();
    if end < content.len() {
        snippet.push_str("...");
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::WorkspaceService;
    use crate::error::AppError;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn workspace_files_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        WorkspaceService::write_workspace_file("AGENTS.md", "hello workspace")?;
        let content = WorkspaceService::read_workspace_file("AGENTS.md")?;
        assert_eq!(content.as_deref(), Some("hello workspace"));

        Ok(())
    }

    #[test]
    #[serial]
    fn daily_memory_search_returns_context() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        WorkspaceService::write_daily_memory_file(
            "2026-03-07.md",
            "Today we finished the phase one migration for cc-switch core.",
        )?;

        let results = WorkspaceService::search_daily_memory_files("phase one")?;
        assert_eq!(results.len(), 1);
        assert!(results[0].snippet.contains("phase one"));

        Ok(())
    }
}
