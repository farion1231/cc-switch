use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;

const CLAUDE_DIR: &str = ".claude";
const CLAUDE_CONFIG_FILE: &str = "config.json";

fn claude_dir() -> Result<PathBuf, AppError> {
    // 优先使用设置中的覆盖目录
    if let Some(dir) = crate::settings::get_override_dir(&crate::app_config::AppType::Claude) {
        return Ok(dir);
    }
    Ok(crate::config::get_home_dir().join(CLAUDE_DIR))
}

pub fn claude_config_path() -> Result<PathBuf, AppError> {
    Ok(claude_dir()?.join(CLAUDE_CONFIG_FILE))
}

pub fn ensure_claude_dir_exists() -> Result<PathBuf, AppError> {
    let dir = claude_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    }
    Ok(dir)
}

/// 读取并解析 config.json：不存在或只有空白时视为空对象；
/// 不是合法 JSON 或根不是对象时报错，绝不当成空对象覆盖用户的其它设置。
fn load_config_object(
    path: &Path,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    if content.trim().is_empty() {
        return Ok(Some(serde_json::Map::new()));
    }
    match serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| AppError::json(path, e))?
    {
        serde_json::Value::Object(map) => Ok(Some(map)),
        _ => Err(AppError::localized(
            "claude_plugin.config_not_object",
            format!("{} 的内容不是 JSON 对象，请先修复该文件", path.display()),
            format!(
                "{} is not a JSON object; please fix the file first",
                path.display()
            ),
        )),
    }
}

fn write_config_object(
    path: &Path,
    map: serde_json::Map<String, serde_json::Value>,
) -> Result<(), AppError> {
    let serialized = serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    crate::config::atomic_write(path, format!("{serialized}\n").as_bytes())
}

pub fn write_claude_config() -> Result<bool, AppError> {
    ensure_claude_dir_exists()?;
    write_claude_config_at(&claude_config_path()?)
}

/// 增量写入：仅设置 primaryApiKey = "any"，保留其它字段
fn write_claude_config_at(path: &Path) -> Result<bool, AppError> {
    let existed = path.exists();
    let mut map = load_config_object(path)?.unwrap_or_default();

    let already = map.get("primaryApiKey").and_then(|v| v.as_str()) == Some("any");
    if already && existed {
        return Ok(false);
    }
    map.insert(
        "primaryApiKey".to_string(),
        serde_json::Value::String("any".to_string()),
    );
    write_config_object(path, map)?;
    Ok(true)
}

pub fn clear_claude_config() -> Result<bool, AppError> {
    clear_claude_config_at(&claude_config_path()?)
}

fn clear_claude_config_at(path: &Path) -> Result<bool, AppError> {
    let Some(mut map) = load_config_object(path)? else {
        return Ok(false);
    };
    if map.remove("primaryApiKey").is_none() {
        return Ok(false);
    }
    write_config_object(path, map)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(content: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CLAUDE_CONFIG_FILE);
        if let Some(content) = content {
            fs::write(&path, content).unwrap();
        }
        (dir, path)
    }

    #[test]
    fn write_keeps_other_fields() {
        let (_dir, path) = temp_config(Some(r#"{"theme":"dark","primaryApiKey":"old"}"#));
        assert!(write_claude_config_at(&path).unwrap());
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["primaryApiKey"], "any");
        assert!(!write_claude_config_at(&path).unwrap());
    }

    #[test]
    fn write_creates_missing_or_blank_file() {
        let (_dir, path) = temp_config(None);
        assert!(write_claude_config_at(&path).unwrap());
        let (_dir2, blank) = temp_config(Some("  \n"));
        assert!(write_claude_config_at(&blank).unwrap());
        assert!(fs::read_to_string(&blank).unwrap().contains("\"any\""));
    }

    #[test]
    fn invalid_or_non_object_file_is_left_untouched() {
        for content in [r#"{"theme":"dark","#, "[1,2]"] {
            let (_dir, path) = temp_config(Some(content));
            assert!(write_claude_config_at(&path).is_err());
            assert!(clear_claude_config_at(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
    }

    #[test]
    fn clear_only_removes_primary_api_key() {
        let (_dir, path) = temp_config(Some(r#"{"theme":"dark","primaryApiKey":"any"}"#));
        assert!(clear_claude_config_at(&path).unwrap());
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"theme":"dark"}));
        assert!(!clear_claude_config_at(&path).unwrap());
    }
}
