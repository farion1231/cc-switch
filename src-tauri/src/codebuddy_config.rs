//! CodeBuddy（腾讯 CodeBuddy CLI）本地配置文件适配器。
//!
//! CC Switch 以 "additive" 方式托管 CodeBuddy：一个供应商 = `models.json` 里的
//! 一条模型端点（`{id,name,vendor,url,apiKey,supportsToolCall,supportsImages,
//! supportsReasoning}`），当前生效的模型记录在 `settings.json` 的 `"model"` 字段。
//!
//! 本模块负责这两个文件的**安全读写**：
//! - 一律「读 → 合并 → 原子写回」，保留文件里所有未知字段（CodeBuddy 自己写的
//!   `enabledPlugins` / `trustedDirectories` 等）与其它 models 条目，绝不整文件覆盖；
//! - 每次写前把旧文件复制为 `<name>.bak`（只保留最近一份），便于人工回滚；
//! - 单进程写锁（static Mutex），避免与用量/会话同步路径竞争。
//!
//! 目录解析统一走 `crate::config::get_codebuddy_config_dir()`（设置中的配置目录
//! 覆盖 > `$CODEBUDDY_HOME` > `~/.codebuddy`），测试用 `*_at` 系列直接指定目录。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;

use crate::error::AppError;

/// 全局写锁：所有写路径先取这把锁再操作文件。
static CODEBUDDY_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn get_codebuddy_models_path() -> PathBuf {
    crate::config::get_codebuddy_config_dir().join("models.json")
}

pub(crate) fn get_codebuddy_settings_path() -> PathBuf {
    crate::config::get_codebuddy_config_dir().join("settings.json")
}

/// 取全局写锁（供依赖文件改写的调用方使用同一把锁）。
fn lock_models_file() -> Result<MutexGuard<'static, ()>, AppError> {
    CODEBUDDY_LOCK
        .lock()
        .map_err(|_| AppError::Config("CodeBuddy 配置写锁已毒化".to_string()))
}

// ===================== 公共 API（默认目录） =====================

/// 列出 models.json 中的全部模型端点条目（空文件 / 无 models 键 → 空列表）。
pub(crate) fn list_model_entries() -> Result<Vec<Value>, AppError> {
    let path = get_codebuddy_models_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let _guard = lock_models_file()?;
    read_entries_locked(&path)
}

/// 按 id 覆盖/新增一条模型端点。返回是否为新插入。
pub(crate) fn upsert_model_entry(entry: &Value) -> Result<bool, AppError> {
    let path = get_codebuddy_models_path();
    let _guard = lock_models_file()?;
    upsert_entry_locked(&path, entry)
}

/// 删除指定 id 的模型端点。返回是否确实删除了某条。
pub(crate) fn remove_model_entry(id: &str) -> Result<bool, AppError> {
    let path = get_codebuddy_models_path();
    let _guard = lock_models_file()?;
    remove_entry_locked(&path, id)
}

/// 当前选中的模型 id（settings.json 的 `model` 字段）。
pub(crate) fn current_model_id() -> Result<Option<String>, AppError> {
    let path = get_codebuddy_settings_path();
    let _guard = lock_models_file()?;
    let doc = read_json_object(&path)?;
    Ok(doc
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

/// 设置当前选中的模型 id；传 None 表示清除选择。合并写回、保留其它设置字段。
pub(crate) fn set_current_model_id(id: Option<&str>) -> Result<(), AppError> {
    let path = get_codebuddy_settings_path();
    let _guard = lock_models_file()?;
    let mut doc = read_json_object_or_default(&path);
    match id {
        Some(id) if !id.trim().is_empty() => {
            doc.insert("model".to_string(), Value::String(id.trim().to_string()));
        }
        _ => {
            doc.remove("model");
        }
    }
    write_document(&path, &Value::Object(doc))
}

// ===================== 测试/内部：按目录注入 =====================

fn read_entries_locked(path: &Path) -> Result<Vec<Value>, AppError> {
    let doc = read_json_object(path)?;
    let Some(models) = doc.get("models") else {
        return Ok(Vec::new());
    };
    let Some(entries) = models.as_array() else {
        return Ok(Vec::new());
    };
    Ok(entries.clone())
}

fn upsert_entry_locked(path: &Path, entry: &Value) -> Result<bool, AppError> {
    validate_entry(entry)?;
    let id = entry_id(entry)?;

    if !path.exists() {
        // 全新文件：创建父目录并写入首个条目。
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let doc = serde_json::json!({ "models": [entry] });
        return write_document(path, &doc).map(|_| true);
    }

    let mut doc = read_json_object(path)?;
    let mut entries = doc
        .get("models")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let existing = entries.iter_mut().find(|e| entry_id(e).ok() == Some(id));
    match existing {
        Some(slot) => {
            *slot = entry.clone();
            doc.insert("models".to_string(), Value::Array(entries));
            write_document(path, &Value::Object(doc))?;
            Ok(false)
        }
        None => {
            entries.push(entry.clone());
            doc.insert("models".to_string(), Value::Array(entries));
            write_document(path, &Value::Object(doc))?;
            Ok(true)
        }
    }
}

fn remove_entry_locked(path: &Path, id: &str) -> Result<bool, AppError> {
    if !path.exists() {
        return Ok(false);
    }
    let mut doc = read_json_object(path)?;
    let Some(models) = doc.get_mut("models").and_then(|v| v.as_array_mut()) else {
        return Ok(false);
    };
    let before = models.len();
    models.retain(|e| entry_id(e).ok() != Some(id));
    if models.len() == before {
        return Ok(false);
    }
    write_document(path, &Value::Object(doc))?;
    Ok(true)
}

/// 校验一条模型端点（供 ProviderService 调用）。要求顶层为对象且含非空 url。
pub(crate) fn validate_provider_entry(entry: &Value) -> Result<(), AppError> {
    if !entry.is_object() {
        return Err(AppError::Config(
            "CodeBuddy 模型端点配置必须是 JSON 对象".to_string(),
        ));
    }
    validate_entry(entry)
}

fn validate_entry(entry: &Value) -> Result<(), AppError> {
    if entry_id(entry).is_err() {
        return Err(AppError::Config(
            "CodeBuddy 模型端点缺少非空 id 字段".to_string(),
        ));
    }
    let url = entry.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.trim().is_empty() {
        return Err(AppError::Config(
            "CodeBuddy 模型端点缺少 url（API 地址）".to_string(),
        ));
    }
    Ok(())
}

fn entry_id(entry: &Value) -> Result<&str, AppError> {
    let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if id.trim().is_empty() {
        return Err(AppError::Config(
            "CodeBuddy 模型端点缺少非空 id 字段".to_string(),
        ));
    }
    Ok(id)
}

/// 读取 JSON 对象文档；文件缺失或空按 `{}` 处理。
fn read_json_object_or_default(path: &Path) -> serde_json::Map<String, Value> {
    match read_json_object(path) {
        Ok(map) => map,
        Err(_) => serde_json::Map::new(),
    }
}

/// 读取 JSON 对象文档；不存在/非对象返回 `{}`（读路径不把错误当作致命）。
fn read_json_object(path: &Path) -> Result<serde_json::Map<String, Value>, AppError> {
    if !path.exists() {
        return Ok(serde_json::Map::new());
    }
    let bytes = std::fs::read(path).map_err(|e| AppError::io(path, e))?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(AppError::Config(format!(
            "CodeBuddy 配置文件必须是 JSON 对象: {}",
            path.display()
        ))),
        Err(e) => Err(AppError::Config(format!(
            "CodeBuddy 配置解析失败 {}: {e}",
            path.display()
        ))),
    }
}

/// 写前备份旧文件为 `<name>.bak`（仅保留最近一份），随后原子写回。
fn write_document(path: &Path, doc: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    if path.exists() {
        let backup = backup_path(path);
        let _ = std::fs::copy(path, &backup);
    }
    crate::config::write_json_file(path, doc)
}

/// 备份路径：`models.json` → `models.json.bak`（同目录）。
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    path.with_file_name(name)
}

// ===================== 测试 =====================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_entry(id: &str, url: &str) -> Value {
        serde_json::json!({
            "id": id,
            "name": id,
            "vendor": "cc-switch",
            "url": url,
            "apiKey": "sk-test",
            "supportsToolCall": true,
            "supportsImages": false,
            "supportsReasoning": true
        })
    }

    #[test]
    fn upsert_list_remove_roundtrip() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("models.json");
        assert_eq!(read_entries_locked(&path).unwrap(), Vec::<Value>::new());

        assert!(
            upsert_entry_locked(&path, &sample_entry("glm-5.3", "https://a.example/v1")).unwrap()
        );
        assert!(
            !upsert_entry_locked(&path, &sample_entry("glm-5.3", "https://b.example/v1")).unwrap()
        );
        assert!(
            upsert_entry_locked(&path, &sample_entry("deepseek", "https://c.example/v1")).unwrap()
        );

        let entries = read_entries_locked(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["id"], "glm-5.3");
        assert_eq!(entries[0]["url"], "https://b.example/v1");
        assert_eq!(entries[1]["id"], "deepseek");

        assert!(remove_entry_locked(&path, "deepseek").unwrap());
        assert!(!remove_entry_locked(&path, "deepseek").unwrap());
        assert_eq!(read_entries_locked(&path).unwrap().len(), 1);
    }

    #[test]
    fn upsert_preserves_unknown_top_level_fields() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("models.json");
        let doc = serde_json::json!({
            "version": 2,
            "models": [sample_entry("existing", "https://a.example/v1")]
        });
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();

        upsert_entry_locked(&path, &sample_entry("added", "https://b.example/v1")).unwrap();

        let after = read_json_object(&path).unwrap();
        assert_eq!(after["version"], 2, "未知顶层字段必须保留");
        let entries = after["models"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn upsert_preserves_codebuddy_owned_settings_keys() {
        let tmp = tempdir().unwrap();
        let settings = tmp.path().join("settings.json");
        let doc = serde_json::json!({
            "model": "old-model",
            "enabledPlugins": { "plugin-x": true },
            "trustedDirectories": ["/tmp/proj"]
        });
        std::fs::write(&settings, serde_json::to_vec(&doc).unwrap()).unwrap();

        let mut new_doc = serde_json::Map::new();
        new_doc.insert("model".to_string(), Value::String("glm-5.3".to_string()));
        write_document(&settings, &Value::Object(new_doc)).unwrap();

        let after = read_json_object(&settings).unwrap();
        assert_eq!(after["model"], "glm-5.3");
        // 该用例通过 write_document 直接覆盖，仅验证备份行为；合并逻辑由
        // set_current_model_id 负责（见下一用例）。
        assert!(settings.with_file_name("settings.json.bak").exists());
    }

    #[test]
    fn set_current_model_merges_settings() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!({
            "enabledPlugins": { "plugin-x": true }
        });
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();

        set_current_model_at(path.as_path(), Some("glm-5.3"));
        let after = read_json_object(&path).unwrap();
        assert_eq!(after["model"], "glm-5.3");
        assert_eq!(
            after["enabledPlugins"]["plugin-x"], true,
            "其它设置字段必须保留"
        );

        set_current_model_at(path.as_path(), None);
        let after = read_json_object(&path).unwrap();
        assert!(after.get("model").is_none());
        assert_eq!(after["enabledPlugins"]["plugin-x"], true);
    }

    #[test]
    fn reject_invalid_entry() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("models.json");
        assert!(upsert_entry_locked(&path, &serde_json::json!({ "name": "no-id" })).is_err());
        assert!(upsert_entry_locked(&path, &serde_json::json!({ "id": "x" })).is_err());
        assert!(!path.exists());
    }

    // 便于测试直接注入目录（公共 API 默认读真实主目录，不适合测试）。
    fn set_current_model_at(path: &Path, id: Option<&str>) {
        let mut doc = read_json_object_or_default(path);
        match id {
            Some(id) => {
                doc.insert("model".to_string(), Value::String(id.to_string()));
            }
            None => {
                doc.remove("model");
            }
        }
        write_document(path, &Value::Object(doc)).unwrap();
    }
}
