//! Pi 配置文件读写模块
//!
//! 处理 `~/.pi/agent/models.json` 的读写。Pi 使用 additive 模式:
//! 所有 provider 以 `<provider-id>` 为键共存于 `providers` 映射下,
//! 与 Oh-My-Pi 的 models.yml 结构一致(但为 JSON)。本模块只负责
//! models.json 的 providers 段读写,保留其它顶层段(如有)原样回写。
//!
//! ## 配置结构示例
//!
//! ```json
//! {
//!   "providers": {
//!     "anthropic": {
//!       "baseUrl": "https://api.anthropic.com",
//!       "api": "anthropic-messages",
//!       "apiKey": "sk-...",
//!       "models": [{ "id": "claude-opus-4-8", "name": "Claude Opus 4.8" }]
//!     },
//!     "deepseek": {
//!       "baseUrl": "https://api.deepseek.com",
//!       "api": "openai-completions",
//!       "apiKey": "sk-...",
//!       "models": [{ "id": "deepseek-chat" }]
//!     }
//!   }
//! }
//! ```

use crate::config::{atomic_write, get_pi_agent_dir};
use crate::error::AppError;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

/// `~/.pi/agent/models.json`(与读取侧 `read_live_settings` 的 Pi 分支一致)。
pub fn get_models_path() -> PathBuf {
    get_pi_agent_dir().join("models.json")
}

/// 进程级写锁,防止并发读-改-写产生 TOCTOU。
fn pi_write_lock() -> &'static Mutex<()> {
    static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    &LOCK
}

/// 读取 models.json 完整文档;文件不存在或内容为空时返回空 object。
pub fn read_models_config() -> Result<Value, AppError> {
    let path = get_models_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

/// 读取 `providers` 段作为 JSON(用于命令侧 `get_pi_live_provider_ids`)。
pub fn get_providers_json() -> Result<Value, AppError> {
    let config = read_models_config()?;
    Ok(config
        .get("providers")
        .cloned()
        .unwrap_or_else(|| json!({})))
}

/// 返回 `providers` 映射的全部键(即 live 中已存在的 provider id 列表,有序)。
pub fn get_provider_ids() -> Result<Vec<String>, AppError> {
    let providers = get_providers_json()?;
    let mut ids: Vec<String> = providers
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    ids.sort();
    Ok(ids)
}

/// upsert 一个 provider entry 到 models.json 的 `providers.<id>`。
///
/// - `provider_id`:provider 在 `providers` 下的 key(与 cc-switch 的 provider.id 一致)。
/// - `entry`:models.json provider entry({baseUrl, api, apiKey, models})。
///
/// 保留 models.json 中其它顶层段与其它已存在的 provider;同 id 整体替换(cc-switch 权威)。
pub fn set_provider(provider_id: &str, entry: Value) -> Result<(), AppError> {
    let _guard = pi_write_lock().lock()?;

    let mut config = read_models_config()?;
    if !config.is_object() {
        // 顶层不是 object(理论上不该发生),用空 object 顶替。
        config = json!({});
    }

    let obj = config.as_object_mut().expect("checked above");
    let providers_entry = obj
        .entry("providers".to_string())
        .or_insert_with(|| json!({}));
    if !providers_entry.is_object() {
        // 防御:非 object 形态无法容纳 provider,重置为空对象。
        *providers_entry = json!({});
    }

    providers_entry
        .as_object_mut()
        .expect("ensured object above")
        .insert(provider_id.to_string(), entry);

    write_models_config(&config)
}

/// 从 models.json 的 `providers` 段移除一个 provider。
///
/// 不存在则 no-op。保留其它 provider 及所有非 providers 顶层段。
pub fn remove_provider(provider_id: &str) -> Result<(), AppError> {
    let _guard = pi_write_lock().lock()?;

    let mut config = read_models_config()?;
    let Some(providers) = config.as_object_mut().and_then(|m| m.get_mut("providers")) else {
        return Ok(());
    };
    if let Some(providers_map) = providers.as_object_mut() {
        providers_map.remove(provider_id);
    }
    write_models_config(&config)
}

fn write_models_config(config: &Value) -> Result<(), AppError> {
    let path = get_models_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json_str =
        serde_json::to_string_pretty(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&path, json_str.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_test_home<F: FnOnce() -> ()>(f: F) {
        // 使用 CC_SWITCH_TEST_HOME 隔离,与 config::get_home_dir 的可测试入口一致。
        let temp = tempfile::tempdir().expect("temp dir");
        let prev = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        let _ = f();
        match prev {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn set_provider_creates_and_upserts() {
        with_test_home(|| {
            set_provider(
                "anthropic",
                json!({
                    "baseUrl": "https://api.anthropic.com",
                    "apiKey": "sk-1",
                    "api": "anthropic-messages",
                    "models": [{"id": "claude-opus-4-8", "name": "Claude Opus 4.8"}]
                }),
            )
            .expect("set first");
            set_provider(
                "deepseek",
                json!({"baseUrl": "https://api.deepseek.com", "apiKey": "sk-2", "api": "openai-completions"}),
            )
            .expect("set second");

            let ids = get_provider_ids().expect("ids");
            assert_eq!(ids, vec!["anthropic".to_string(), "deepseek".to_string()]);

            // upsert:覆盖 anthropic,不应删除 deepseek
            set_provider(
                "anthropic",
                json!({"baseUrl": "https://new.example.com", "apiKey": "sk-3"}),
            )
            .expect("upsert");
            let ids = get_provider_ids().expect("ids after upsert");
            assert_eq!(ids, vec!["anthropic".to_string(), "deepseek".to_string()]);
            let providers = get_providers_json().expect("providers");
            assert_eq!(providers["anthropic"]["baseUrl"], "https://new.example.com");
            assert_eq!(providers["deepseek"]["apiKey"], "sk-2");
        });
    }

    #[test]
    #[serial_test::serial]
    fn remove_provider_preserves_others_and_other_sections() {
        with_test_home(|| {
            set_provider("anthropic", json!({"baseUrl": "a", "apiKey": "k1"})).expect("set a");
            set_provider("deepseek", json!({"baseUrl": "b", "apiKey": "k2"})).expect("set b");

            // 注入一个非 providers 顶层段,验证 remove 不会破坏它
            {
                let _g = pi_write_lock().lock().expect("lock");
                let mut config = read_models_config().expect("read");
                if let Some(m) = config.as_object_mut() {
                    m.insert("equivalence".to_string(), json!({}));
                }
                write_models_config(&config).expect("write");
            }

            remove_provider("anthropic").expect("remove a");
            let ids = get_provider_ids().expect("ids");
            assert_eq!(ids, vec!["deepseek".to_string()]);
            // 确认 equivalence 段仍在
            let config = read_models_config().expect("read");
            assert!(
                config.get("equivalence").is_some(),
                "equivalence section must survive remove"
            );
        });
    }

    #[test]
    #[serial_test::serial]
    fn read_returns_empty_when_missing() {
        with_test_home(|| {
            let config = read_models_config().expect("read");
            assert!(config.is_object());
            // pi_agent_dir 尚未创建,get_provider_ids 应为空
            let ids = get_provider_ids().expect("ids");
            assert!(ids.is_empty());
        });
    }
}
