//! CodeBuddy（腾讯 CodeBuddy CLI）本地配置文件适配器。
//!
//! CC Switch 以 "additive" 方式托管 CodeBuddy：一个供应商 = `models.json` 里的
//! 一条模型端点（`{id,name,vendor,url,apiKey,supportsToolCall,supportsImages,
//! supportsReasoning}`），当前生效的模型记录在 `settings.json` 的 `"model"` 字段。
//!
//! 本模块负责这两个文件的**安全读写**：
//! - 一律「读 → 合并 → 原子写回」，保留文件里所有未知字段（CodeBuddy 自己写的
//!   `enabledPlugins` / `trustedDirectories` 等）与其它 models 条目，绝不整文件覆盖；
//! - 写前比对内容修订号：读取之后文件若被 CodeBuddy 自己改过，宁可报冲突也不能
//!   用陈旧快照覆盖（进程内 Mutex 协调不了外部进程）；
//! - 解析失败一律报错，绝不降级成空文档——那会把 CodeBuddy 维护的字段一并抹掉；
//! - 每次写前把旧文件备份为 `<name>.bak`（只保留最近一份），便于人工回滚；
//! - 文件与备份都含明文 `apiKey`，因此一律以 0600 落盘、新建目录为 0700；
//! - 单进程写锁（static Mutex），避免与用量/会话同步路径竞争。
//!
//! 目录解析统一走 `crate::config::get_codebuddy_config_dir()`（设置中的配置目录
//! 覆盖 > `$CODEBUDDY_HOME` > `~/.codebuddy`），测试直接传入目录。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::{Map, Value};

use crate::config::{
    atomic_write_private, ensure_file_revision, ensure_private_parent, file_revision,
    read_file_if_exists, serialize_json_sorted, MISSING_FILE_REVISION,
};
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
    let doc = read_document_with_revision(&path)?.0;
    Ok(doc
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

/// 设置当前选中的模型 id；传 None 表示清除选择。合并写回、保留其它设置字段。
/// settings.json 已存在但解析失败时返回错误，绝不按空对象覆盖（避免丢 CodeBuddy 自有字段）。
pub(crate) fn set_current_model_id(id: Option<&str>) -> Result<(), AppError> {
    let path = get_codebuddy_settings_path();
    let _guard = lock_models_file()?;
    set_current_model_at(&path, id)
}

// ===================== 测试/内部：按目录注入 =====================

fn read_entries_locked(path: &Path) -> Result<Vec<Value>, AppError> {
    let (doc, _) = read_document_with_revision(path)?;
    let Some(entries) = doc.get("models").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    Ok(entries.clone())
}

fn upsert_entry_locked(path: &Path, entry: &Value) -> Result<bool, AppError> {
    validate_entry(entry)?;
    let id = entry_id(entry)?;

    let (mut doc, expected_revision) = read_document_with_revision(path)?;
    let mut entries = doc
        .get("models")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 同 id 覆盖为更新，否则追加；两种情况都在校验过修订号之后才落盘
    let inserted = match entries.iter_mut().find(|e| entry_id(e).ok() == Some(id)) {
        Some(slot) => {
            *slot = entry.clone();
            false
        }
        None => {
            entries.push(entry.clone());
            true
        }
    };

    doc.insert("models".to_string(), Value::Array(entries));
    write_document(path, &Value::Object(doc), &expected_revision)?;
    Ok(inserted)
}

fn remove_entry_locked(path: &Path, id: &str) -> Result<bool, AppError> {
    let (mut doc, expected_revision) = read_document_with_revision(path)?;
    let Some(models) = doc.get_mut("models").and_then(|v| v.as_array_mut()) else {
        return Ok(false);
    };
    let before = models.len();
    models.retain(|e| entry_id(e).ok() != Some(id));
    if models.len() == before {
        return Ok(false);
    }
    write_document(path, &Value::Object(doc), &expected_revision)?;
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

/// 读取 JSON 对象文档并返回其内容修订号。
///
/// 文件缺失 → 空对象 + `MISSING_FILE_REVISION`。解析失败一律返回错误：
/// `settings.json` 里还有 CodeBuddy 自己维护的 `enabledPlugins` /
/// `trustedDirectories`，把解析失败当成空文档写回等于直接抹掉它们。
fn read_document_with_revision(path: &Path) -> Result<(Map<String, Value>, String), AppError> {
    let Some(bytes) = read_file_if_exists(path)? else {
        return Ok((Map::new(), MISSING_FILE_REVISION.to_string()));
    };
    let revision = file_revision(&bytes);
    Ok((parse_json_object(path, &bytes)?, revision))
}

fn parse_json_object(path: &Path, bytes: &[u8]) -> Result<Map<String, Value>, AppError> {
    match serde_json::from_slice::<Value>(bytes) {
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

/// 设置当前选中的模型 id（按目录注入版本，公共 API 与测试共用同一实现）。
fn set_current_model_at(path: &Path, id: Option<&str>) -> Result<(), AppError> {
    let (mut doc, expected_revision) = read_document_with_revision(path)?;
    match id {
        Some(id) if !id.trim().is_empty() => {
            doc.insert("model".to_string(), Value::String(id.trim().to_string()));
        }
        _ => {
            doc.remove("model");
        }
    }
    write_document(path, &Value::Object(doc), &expected_revision)
}

/// 写回文档。
///
/// 顺序很关键：先确认文件自读取以来没有被外部改动（进程内 Mutex 协调不了
/// CodeBuddy 自己），再把「刚校验过的那一份」备份为 `<name>.bak`，最后原子落盘。
/// 文件与备份都含明文 `apiKey`，一律 0600。
fn write_document(path: &Path, doc: &Value, expected_revision: &str) -> Result<(), AppError> {
    let bytes = serialize_json_sorted(doc)?;
    ensure_private_parent(path, "CodeBuddy")?;
    ensure_file_revision(path, expected_revision, "CodeBuddy")?;

    if let Some(previous) = read_file_if_exists(path)? {
        // 备份是尽力而为的便利功能，失败不应阻断切换
        let _ = atomic_write_private(&backup_path(path), &previous);
    }

    atomic_write_private(path, &bytes)
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

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
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

        let after = read_document_with_revision(&path).unwrap().0;
        assert_eq!(after["version"], 2, "未知顶层字段必须保留");
        let entries = after["models"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn write_document_backs_up_previous_contents() {
        let tmp = tempdir().unwrap();
        let settings = tmp.path().join("settings.json");
        let original = serde_json::json!({
            "model": "old-model",
            "enabledPlugins": { "plugin-x": true },
            "trustedDirectories": ["/tmp/proj"]
        });
        std::fs::write(&settings, serde_json::to_vec(&original).unwrap()).unwrap();

        let (mut doc, revision) = read_document_with_revision(&settings).unwrap();
        doc.insert("model".to_string(), Value::String("glm-5.3".to_string()));
        write_document(&settings, &Value::Object(doc), &revision).unwrap();

        let after = read_document_with_revision(&settings).unwrap().0;
        assert_eq!(after["model"], "glm-5.3");
        assert_eq!(
            after["enabledPlugins"]["plugin-x"], true,
            "CodeBuddy 自己维护的字段必须保留"
        );

        let backup: Value =
            serde_json::from_slice(&std::fs::read(backup_path(&settings)).unwrap()).unwrap();
        assert_eq!(backup, original, "备份必须是写入前的那一份内容");
    }

    #[test]
    fn set_current_model_merges_settings() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let doc = serde_json::json!({
            "enabledPlugins": { "plugin-x": true }
        });
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();

        set_current_model_at(&path, Some("glm-5.3")).unwrap();
        let after = read_document_with_revision(&path).unwrap().0;
        assert_eq!(after["model"], "glm-5.3");
        assert_eq!(
            after["enabledPlugins"]["plugin-x"], true,
            "其它设置字段必须保留"
        );

        set_current_model_at(&path, None).unwrap();
        let after = read_document_with_revision(&path).unwrap().0;
        assert!(after.get("model").is_none());
        assert_eq!(after["enabledPlugins"]["plugin-x"], true);
    }

    #[test]
    fn set_current_model_refuses_to_clobber_unparseable_settings() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let corrupt = b"{ this is not json";
        std::fs::write(&path, corrupt).unwrap();

        let error = set_current_model_at(&path, Some("glm-5.3")).unwrap_err();

        assert!(matches!(error, AppError::Config(_)), "实际: {error:?}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            corrupt,
            "解析失败时必须报错，绝不能把文件改写成只含 model 的文档"
        );
    }

    #[test]
    fn upsert_rejects_concurrent_external_change() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("models.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "models": [sample_entry("existing", "https://a.example/v1")]
            }))
            .unwrap(),
        )
        .unwrap();

        let (doc, stale_revision) = read_document_with_revision(&path).unwrap();

        // CodeBuddy 在本次读取之后自己往文件里补了一条
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "models": [
                    sample_entry("existing", "https://a.example/v1"),
                    sample_entry("added-by-codebuddy", "https://b.example/v1")
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        let error = write_document(&path, &Value::Object(doc), &stale_revision).unwrap_err();
        assert!(
            matches!(error, AppError::Conflict(_)),
            "外部改动必须报冲突，实际: {error:?}"
        );

        let after = read_document_with_revision(&path).unwrap().0;
        assert_eq!(
            after["models"].as_array().unwrap().len(),
            2,
            "被外部添加的条目不能被陈旧快照抹掉"
        );
    }

    #[test]
    fn set_current_model_rejects_concurrent_external_change() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "model": "old-model",
                "trustedDirectories": ["/tmp/proj"]
            }))
            .unwrap(),
        )
        .unwrap();

        let (mut doc, stale_revision) = read_document_with_revision(&path).unwrap();

        // 切换模型期间，CodeBuddy 自己把新目录记进了 trustedDirectories
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "model": "old-model",
                "trustedDirectories": ["/tmp/proj", "/tmp/other"]
            }))
            .unwrap(),
        )
        .unwrap();

        doc.insert("model".to_string(), Value::String("glm-5.3".to_string()));
        let error = write_document(&path, &Value::Object(doc), &stale_revision).unwrap_err();
        assert!(matches!(error, AppError::Conflict(_)), "实际: {error:?}");

        let after = read_document_with_revision(&path).unwrap().0;
        assert_eq!(
            after["trustedDirectories"].as_array().unwrap().len(),
            2,
            "CodeBuddy 记下的受信目录不能丢"
        );
    }

    #[cfg(unix)]
    #[test]
    fn written_config_files_are_owner_only() {
        let tmp = tempdir().unwrap();
        // 目录由本模块创建，因此应被收紧为 0700
        let dir = tmp.path().join("codebuddy");
        let path = dir.join("models.json");

        upsert_entry_locked(&path, &sample_entry("glm-5.3", "https://a.example/v1")).unwrap();

        assert_eq!(mode_of(&path), 0o600, "models.json 含明文 apiKey");
        assert_eq!(mode_of(&dir), 0o700, "新建的配置目录不应被他人遍历");
    }

    #[cfg(unix)]
    #[test]
    fn backup_file_is_owner_only() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("models.json");

        upsert_entry_locked(&path, &sample_entry("glm-5.3", "https://a.example/v1")).unwrap();
        upsert_entry_locked(&path, &sample_entry("deepseek", "https://b.example/v1")).unwrap();

        let backup = backup_path(&path);
        assert!(backup.exists(), "第二次写入应产生备份");
        assert_eq!(mode_of(&backup), 0o600, "备份同样含明文 apiKey");
    }

    #[test]
    fn reject_invalid_entry() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("models.json");
        assert!(upsert_entry_locked(&path, &serde_json::json!({ "name": "no-id" })).is_err());
        assert!(upsert_entry_locked(&path, &serde_json::json!({ "id": "x" })).is_err());
        assert!(!path.exists());
    }
}
