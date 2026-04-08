use crate::bridges::support::{convert, map_core_err};
use crate::error::AppError;
use crate::openclaw_config::get_openclaw_dir;

pub fn legacy_list_daily_memory_files() -> Result<Vec<cc_switch_core::DailyMemoryFileInfo>, AppError>
{
    let memory_dir = get_openclaw_dir().join("workspace").join("memory");
    if !memory_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<cc_switch_core::DailyMemoryFileInfo> = Vec::new();
    let entries = std::fs::read_dir(&memory_dir).map_err(|e| AppError::Message(e.to_string()))?;

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

        files.push(cc_switch_core::DailyMemoryFileInfo {
            filename: name.clone(),
            date: name.trim_end_matches(".md").to_string(),
            size_bytes: metadata.len(),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            preview,
        });
    }

    files.sort_by(|left, right| right.filename.cmp(&left.filename));
    Ok(files)
}

pub fn list_daily_memory_files() -> Result<Vec<cc_switch_core::DailyMemoryFileInfo>, AppError> {
    cc_switch_core::WorkspaceService::list_daily_memory_files().map_err(map_core_err)
}

pub fn legacy_read_daily_memory_file(filename: &str) -> Result<Option<String>, AppError> {
    let path = get_openclaw_dir()
        .join("workspace")
        .join("memory")
        .join(filename);
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn read_daily_memory_file(filename: &str) -> Result<Option<String>, AppError> {
    cc_switch_core::WorkspaceService::read_daily_memory_file(filename).map_err(map_core_err)
}

pub fn legacy_write_daily_memory_file(filename: &str, content: &str) -> Result<(), AppError> {
    let memory_dir = get_openclaw_dir().join("workspace").join("memory");
    std::fs::create_dir_all(&memory_dir).map_err(|e| AppError::Message(e.to_string()))?;
    crate::config::write_text_file(&memory_dir.join(filename), content)
}

pub fn write_daily_memory_file(filename: &str, content: &str) -> Result<(), AppError> {
    cc_switch_core::WorkspaceService::write_daily_memory_file(filename, content)
        .map_err(map_core_err)
}

pub fn legacy_search_daily_memory_files(
    query: &str,
) -> Result<Vec<cc_switch_core::DailyMemorySearchResult>, AppError> {
    let results =
        cc_switch_core::WorkspaceService::search_daily_memory_files(query).map_err(map_core_err)?;
    convert(results)
}

pub fn search_daily_memory_files(
    query: &str,
) -> Result<Vec<cc_switch_core::DailyMemorySearchResult>, AppError> {
    cc_switch_core::WorkspaceService::search_daily_memory_files(query).map_err(map_core_err)
}

pub fn legacy_delete_daily_memory_file(filename: &str) -> Result<(), AppError> {
    let path = get_openclaw_dir()
        .join("workspace")
        .join("memory")
        .join(filename);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| AppError::Message(e.to_string()))?;
    }
    Ok(())
}

pub fn delete_daily_memory_file(filename: &str) -> Result<(), AppError> {
    cc_switch_core::WorkspaceService::delete_daily_memory_file(filename).map_err(map_core_err)
}

pub fn legacy_read_workspace_file(filename: &str) -> Result<Option<String>, AppError> {
    let path = get_openclaw_dir().join("workspace").join(filename);
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn read_workspace_file(filename: &str) -> Result<Option<String>, AppError> {
    cc_switch_core::WorkspaceService::read_workspace_file(filename).map_err(map_core_err)
}

pub fn legacy_write_workspace_file(filename: &str, content: &str) -> Result<(), AppError> {
    let workspace_dir = get_openclaw_dir().join("workspace");
    std::fs::create_dir_all(&workspace_dir).map_err(|e| AppError::Message(e.to_string()))?;
    crate::config::write_text_file(&workspace_dir.join(filename), content)
}

pub fn write_workspace_file(filename: &str, content: &str) -> Result<(), AppError> {
    cc_switch_core::WorkspaceService::write_workspace_file(filename, content).map_err(map_core_err)
}
