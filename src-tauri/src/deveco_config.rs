//! DevEco Code 配置读写模块
//!
//! DevEco Code（`@deveco/deveco-code`）是 OpenCode 的二次开发分支，配置格式与
//! OpenCode 同构（`provider` / `mcp` / `plugin` 段一致），差异在路径与文件名：
//!
//! | 维度 | OpenCode | DevEco Code |
//! |---|---|---|
//! | 配置目录 | `~/.config/opencode` | `~/.config/deveco`（可由 `DEVECO_CONFIG_DIR` 覆盖） |
//! | 配置文件 | `opencode.json` | `deveco.jsonc` → `deveco.json` → `config.json` |
//! | 数据目录 | `~/.local/share/opencode` | `~/.local/share/deveco`（尊重 `XDG_DATA_HOME`） |
//!
//! DevEco Code 使用累加式（additive）供应商管理：所有供应商配置共存于同一配置
//! 文件中，cc-switch 只投影 provider / mcp 段，其余顶层配置（`model`、`agent`、
//! `theme` 等）原样保留。

use crate::config::write_json_file_with_contents;
use crate::error::AppError;
use crate::provider::DevEcoProviderConfig;
use crate::settings::get_deveco_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 配置文件探测顺序，与 DevEco Code 自身的加载顺序一致。
const CONFIG_FILENAMES: [&str; 3] = ["deveco.jsonc", "deveco.json", "config.json"];

fn deveco_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// 路径解析
// ============================================================================

/// 获取 DevEco Code 配置目录
///
/// 优先级：settings 覆盖 → `DEVECO_CONFIG_DIR` 环境变量 → `~/.config/deveco`
pub fn get_deveco_dir() -> PathBuf {
    if let Some(override_dir) = get_deveco_override_dir() {
        return override_dir;
    }

    if let Ok(dir) = std::env::var("DEVECO_CONFIG_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    crate::config::get_home_dir().join(".config").join("deveco")
}

/// 解析实际的配置文件路径
///
/// DevEco Code 按 `deveco.jsonc` → `deveco.json` → `config.json` 顺序加载，
/// **首个存在者生效**。因此必须探测而不是固定写 `deveco.jsonc`：如果用户的配置
/// 实际在 `deveco.json`，而我们新建了一个 `deveco.jsonc`，就会凭空多出一个遮蔽
/// 文件，用户的既有配置会被静默忽略。
///
/// 三者都不存在时返回 `deveco.jsonc`（DevEco 的默认落盘位置）。
pub fn resolve_config_path() -> PathBuf {
    resolve_config_path_in(&get_deveco_dir())
}

fn resolve_config_path_in(dir: &Path) -> PathBuf {
    for name in CONFIG_FILENAMES {
        let candidate = dir.join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    dir.join(CONFIG_FILENAMES[0])
}

/// 获取 DevEco Code 数据目录
///
/// 优先级：`XDG_DATA_HOME` → `~/.local/share/deveco`
/// （DevEco 使用 xdg-basedir，所有平台默认都落在 `~/.local/share`）
pub fn get_deveco_data_dir() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        let xdg_data = xdg_data.trim();
        if !xdg_data.is_empty() {
            return PathBuf::from(xdg_data).join("deveco");
        }
    }

    crate::config::get_home_dir()
        .join(".local")
        .join("share")
        .join("deveco")
}

/// 获取 DevEco Code SQLite 数据库路径
///
/// 优先级：`DEVECO_DB` 环境变量（绝对路径直接使用，相对路径基于数据目录）
/// → `<data_dir>/deveco.db`
pub fn get_deveco_db_path() -> PathBuf {
    if let Ok(custom_path) = std::env::var("DEVECO_DB") {
        let custom_path = custom_path.trim();
        if !custom_path.is_empty() {
            let path = PathBuf::from(custom_path);
            if path.is_absolute() {
                return path;
            }
            return get_deveco_data_dir().join(path);
        }
    }

    get_deveco_data_dir().join("deveco.db")
}

// ============================================================================
// 读取 / 写入
// ============================================================================

fn read_config_from_path(path: &Path) -> Result<Value, AppError> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({
                "$schema": "https://opencode.ai/config.json"
            }));
        }
        Err(err) => return Err(AppError::io(path, err)),
    };

    // deveco.jsonc 允许注释与尾逗号，必须用 JSON5 解析。
    let value: Value = json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse DevEco Code config: {}: {e}",
            path.display()
        ))
    })?;

    // 根节点必须是对象：下游 set_provider / set_mcp_server 都对它做
    // `config["key"] = …` 索引赋值，而 serde_json 只把 Null 自动升级成对象，
    // 数组或标量会直接 panic（panic 发生在 Tauri command 内、跨 FFI 展开）。
    //
    // 这里选择报错而不是重建根节点：配置里还有 model / agent / theme 等用户自有
    // 配置，静默重建等于删掉它们。
    if !value.is_object() {
        return Err(AppError::Config(format!(
            "DevEco Code 配置文件根节点必须是 JSON 对象: {}",
            path.display()
        )));
    }

    Ok(value)
}

pub fn read_deveco_config() -> Result<Value, AppError> {
    read_config_from_path(&resolve_config_path())
}

fn write_config_to_path_with_contents(path: &Path, config: &Value) -> Result<Vec<u8>, AppError> {
    let contents = write_json_file_with_contents(path, config)?;
    log::debug!("DevEco Code config written to {path:?}");
    Ok(contents)
}

// ============================================================================
// Provider 段
// ============================================================================

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_deveco_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    let _guard = deveco_config_lock().lock()?;
    let path = resolve_config_path();
    let mut full_config = read_config_from_path(&path)?;

    // 判空要连「存在但不是对象」一起算：否则下面 as_object_mut 拿不到，
    // 写入会静默失效——界面显示添加成功而文件里没有。provider 段是 cc-switch
    // 的投影区，归一化不会碰用户自有的 model / agent 等顶层配置。
    if !full_config.get("provider").is_some_and(Value::is_object) {
        if full_config.get("provider").is_some() {
            log::warn!("deveco.jsonc 的 provider 不是对象，已重置为空对象");
        }
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_config_to_path_with_contents(&path, &full_config).map(|_| ())
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let _guard = deveco_config_lock().lock()?;
    let path = resolve_config_path();
    let mut config = read_config_from_path(&path)?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    } else if config.get("provider").is_some() {
        log::warn!("deveco.jsonc 的 provider 不是对象，无法删除供应商 '{id}'");
    }

    write_config_to_path_with_contents(&path, &config).map(|_| ())
}

pub fn get_typed_providers() -> Result<IndexMap<String, DevEcoProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<DevEcoProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse DevEco Code provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &DevEcoProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

/// 校验 provider id 与配置结构
pub fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "DevEco Code provider key must not be empty".into(),
        ));
    }
    if !config.is_object() {
        return Err(AppError::localized(
            "provider.deveco.settings.not_object",
            "DevEco Code 供应商配置必须是 JSON 对象",
            "DevEco Code provider config must be a JSON object",
        ));
    }
    Ok(())
}

/// 检查某个 provider（或它的某个 model）是否被顶层配置引用。
///
/// DevEco Code 用 `<provider-id>/<model-id>` 的形式在 `model`、`small_model` 与
/// `agent.*.model` 中引用模型。删除被引用的 provider 会留下悬空引用，导致
/// DevEco Code 启动时报错——因此在删除前必须拦截，让用户先在原生界面改掉。
///
/// `model_id` 为 `None` 时检查整个 provider 是否被引用。
pub fn referenced_by_config(provider_id: &str, model_id: Option<&str>) -> Result<bool, AppError> {
    let config = read_deveco_config()?;
    let prefix = format!("{provider_id}/");

    let mut candidates: Vec<String> = Vec::new();
    for field in ["model", "small_model", "smallModel"] {
        if let Some(value) = config.get(field).and_then(Value::as_str) {
            candidates.push(value.to_string());
        }
    }
    if let Some(agents) = config.get("agent").and_then(Value::as_object) {
        for agent in agents.values() {
            if let Some(value) = agent.get("model").and_then(Value::as_str) {
                candidates.push(value.to_string());
            }
        }
    }

    Ok(candidates.iter().any(|reference| match model_id {
        // 精确到模型：provider/model 完全匹配
        Some(model_id) => reference == &format!("{prefix}{model_id}"),
        // 整个 provider：任何以 "<provider-id>/" 开头的引用都算命中
        None => reference.starts_with(&prefix),
    }))
}

// ============================================================================
// MCP 段
// ============================================================================

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_deveco_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let _guard = deveco_config_lock().lock()?;
    let path = resolve_config_path();
    let mut full_config = read_config_from_path(&path)?;

    if !full_config.get("mcp").is_some_and(Value::is_object) {
        if full_config.get("mcp").is_some() {
            log::warn!("deveco.jsonc 的 mcp 不是对象，已重置为空对象");
        }
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_config_to_path_with_contents(&path, &full_config).map(|_| ())
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let _guard = deveco_config_lock().lock()?;
    let path = resolve_config_path();
    let mut config = read_config_from_path(&path)?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    } else if config.get("mcp").is_some() {
        log::warn!("deveco.jsonc 的 mcp 不是对象，无法删除服务器 '{id}'");
    }

    write_config_to_path_with_contents(&path, &config).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct TestHomeGuard(Option<std::ffi::OsString>);
    impl TestHomeGuard {
        fn set(home: &Path) -> Self {
            let guard = Self(std::env::var_os("CC_SWITCH_TEST_HOME"));
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            guard
        }
    }
    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn write_config(home: &Path, name: &str, content: &str) {
        let dir = home.join(".config").join("deveco");
        std::fs::create_dir_all(&dir).expect("create config dir");
        std::fs::write(dir.join(name), content).expect("write config");
    }

    #[test]
    #[serial]
    fn resolve_config_path_prefers_jsonc_then_json_then_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());
        let dir = temp.path().join(".config").join("deveco");
        std::fs::create_dir_all(&dir).expect("create dir");

        // 都不存在 → 默认落盘到 deveco.jsonc
        assert_eq!(resolve_config_path(), dir.join("deveco.jsonc"));

        // 只有 config.json → 必须命中它，否则会凭空造出遮蔽文件
        std::fs::write(dir.join("config.json"), "{}").expect("write config.json");
        assert_eq!(resolve_config_path(), dir.join("config.json"));

        std::fs::write(dir.join("deveco.json"), "{}").expect("write deveco.json");
        assert_eq!(resolve_config_path(), dir.join("deveco.json"));

        std::fs::write(dir.join("deveco.jsonc"), "{}").expect("write deveco.jsonc");
        assert_eq!(resolve_config_path(), dir.join("deveco.jsonc"));
    }

    #[test]
    #[serial]
    fn read_rejects_non_object_root_instead_of_panicking_downstream() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        for malformed in ["[]", "[{\"a\":1}]", "42", "\"oops\""] {
            write_config(temp.path(), "deveco.jsonc", malformed);
            assert!(
                read_deveco_config().is_err(),
                "non-object root must be rejected: {malformed}"
            );
        }

        write_config(temp.path(), "deveco.jsonc", "{\"agent\": \"x\"}");
        assert!(
            read_deveco_config().is_ok(),
            "a normal object config must still load"
        );
    }

    #[test]
    #[serial]
    fn set_provider_preserves_agent_and_schema_and_parses_jsonc() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 真实形态：注释 + 尾逗号 + 用户自有的 agent 段
        write_config(
            temp.path(),
            "deveco.jsonc",
            r#"{
  "$schema": "https://opencode.ai/config.json",
  // 用户在原生界面配置的 UI 检查模型
  "agent": {
    "ui_verification": {
      "mode": "subagent",
      "model": "xai01/glm-5.3-flash",
      "hidden": true,
    },
  },
}"#,
        );

        set_provider("newprov", json!({"npm": "@ai-sdk/openai-compatible"})).expect("set provider");

        let config = read_deveco_config().expect("reload");
        assert_eq!(
            config["provider"]["newprov"]["npm"],
            "@ai-sdk/openai-compatible"
        );
        assert_eq!(
            config["agent"]["ui_verification"]["model"], "xai01/glm-5.3-flash",
            "用户自有 agent 配置必须原样保留"
        );
        assert_eq!(config["$schema"], "https://opencode.ai/config.json");
    }

    #[test]
    #[serial]
    fn typed_provider_accepts_omitted_npm_and_model_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 本机 deveco.jsonc 的真实形态：provider 省略 npm、model 只有 tool_call/limit
        write_config(
            temp.path(),
            "deveco.jsonc",
            r#"{
  "provider": {
    "xai01": {
      "name": "xai01",
      "models": {
        "glm-5.3-flash": {
          "tool_call": true,
          "limit": { "context": 1000000, "output": 128000 }
        }
      },
      "options": { "baseURL": "https://example.com/v1", "apiKey": "sk-x" }
    }
  }
}"#,
        );

        let providers = get_typed_providers().expect("typed providers");
        let provider = providers.get("xai01").expect("xai01 must parse");

        assert_eq!(provider.npm, "@ai-sdk/openai-compatible", "npm 缺省值");
        let model = provider.models.get("glm-5.3-flash").expect("model");
        assert_eq!(model.name, None, "模型 name 可省略");
        assert_eq!(model.limit.as_ref().and_then(|l| l.context), Some(1000000));
        assert_eq!(
            model.extra.get("tool_call"),
            Some(&Value::Bool(true)),
            "未知字段必须原样保留"
        );
    }

    #[test]
    #[serial]
    fn set_mcp_server_normalizes_non_object_section() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        write_config(
            temp.path(),
            "deveco.jsonc",
            "{\"model\": \"keep-me\", \"mcp\": []}",
        );

        set_mcp_server("echo", json!({"type": "local", "command": ["npx"]})).expect("set mcp");

        let config = read_deveco_config().expect("reload");
        assert_eq!(
            config["mcp"]["echo"]["type"], "local",
            "server must be written"
        );
        assert_eq!(
            config["model"], "keep-me",
            "unrelated config must be preserved"
        );
    }

    #[test]
    #[serial]
    fn referenced_by_config_detects_agent_and_top_level_references() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        write_config(
            temp.path(),
            "deveco.jsonc",
            r#"{
  "model": "xai02/deepseek-v4-flash",
  "agent": {
    "ui_verification": { "model": "xai01/glm-5.3-flash" }
  }
}"#,
        );

        // 被 agent 引用
        assert!(referenced_by_config("xai01", None).expect("check provider"));
        assert!(referenced_by_config("xai01", Some("glm-5.3-flash")).expect("check model"));
        // 被顶层 model 引用
        assert!(referenced_by_config("xai02", None).expect("check provider"));
        // 同 provider 的其它 model 未被引用
        assert!(!referenced_by_config("xai01", Some("other-model")).expect("check model"));
        // 未被引用的 provider
        assert!(!referenced_by_config("grok", None).expect("check provider"));
    }

    #[test]
    #[serial]
    fn env_overrides_win_over_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        let config_dir = temp.path().join("custom-config");
        let data_dir = temp.path().join("custom-data");
        std::env::set_var("DEVECO_CONFIG_DIR", &config_dir);
        std::env::set_var("XDG_DATA_HOME", &data_dir);

        let dir = get_deveco_dir();
        let db = get_deveco_db_path();

        std::env::remove_var("DEVECO_CONFIG_DIR");
        std::env::remove_var("XDG_DATA_HOME");

        assert_eq!(dir, config_dir);
        assert_eq!(db, data_dir.join("deveco").join("deveco.db"));
    }
}
