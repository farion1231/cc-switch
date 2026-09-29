use crate::config::write_json_file_with_contents;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

mod credentials;

const STANDARD_OMO_PLUGIN_PREFIXES: [&str; 2] = ["oh-my-openagent", "oh-my-opencode"];
const SLIM_OMO_PLUGIN_PREFIXES: [&str; 1] = ["oh-my-opencode-slim"];
const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
fn opencode_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_config_contents(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match std::fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(AppError::io(path, err)),
    }
}

fn matches_plugin_prefix(plugin_name: &str, prefix: &str) -> bool {
    plugin_name == prefix
        || plugin_name
            .strip_prefix(prefix)
            .map(|suffix| suffix.starts_with('@'))
            .unwrap_or(false)
}

fn matches_any_plugin_prefix(plugin_name: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| matches_plugin_prefix(plugin_name, prefix))
}

fn canonicalize_plugin_name(plugin_name: &str) -> String {
    if let Some(suffix) = plugin_name.strip_prefix("oh-my-opencode") {
        if suffix.is_empty() || suffix.starts_with('@') {
            return format!("oh-my-openagent{suffix}");
        }
    }
    plugin_name.to_string()
}

pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("opencode")
}

pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

/// 获取 OpenCode SQLite 数据库路径
/// 优先级: OPENCODE_DB 环境变量 > XDG_DATA_HOME > ~/.local/share/opencode
pub fn get_opencode_db_path() -> PathBuf {
    // 支持 OPENCODE_DB 环境变量覆盖（忽略空字符串）
    if let Ok(custom_path) = std::env::var("OPENCODE_DB") {
        if !custom_path.is_empty() {
            let path = PathBuf::from(&custom_path);
            if path.is_absolute() {
                return path;
            }
            // 相对路径基于数据目录
            return get_opencode_data_dir().join(path);
        }
    }

    get_opencode_data_dir().join("opencode.db")
}

fn get_opencode_data_dir() -> PathBuf {
    // 尊重 XDG_DATA_HOME（按 XDG 规范，空字符串视为未设置）
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.is_empty() {
            return PathBuf::from(xdg_data).join("opencode");
        }
    }

    // OpenCode 使用 xdg-basedir，不遵守 macOS/Windows 平台约定，
    // 所有平台默认都落在 ~/.local/share/opencode
    crate::config::get_home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
}

#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

fn read_opencode_config_from_path(path: &Path) -> Result<Value, AppError> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({
                "$schema": "https://opencode.ai/config.json"
            }));
        }
        Err(err) => return Err(AppError::io(path, err)),
    };
    // An empty file contains no configuration to preserve (e.g. after a v2 login).
    if content.trim().is_empty() {
        return Ok(json!({ "$schema": "https://opencode.ai/config.json" }));
    }
    let value: Value = json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse OpenCode config: {}: {e}",
            path.display()
        ))
    })?;

    // 根节点必须是对象：下游 set_provider / set_mcp_server / add_plugin 都对它做
    // `config["key"] = …` 索引赋值，而 serde_json 只把 Null 自动升级成对象，
    // 数组或标量会直接 panic（panic 发生在 Tauri command 内、跨 FFI 展开）。
    //
    // 这里选择报错而不是重建根节点：opencode.json 里还有 model / theme 等用户自有
    // 配置，静默重建等于删掉它们。让用户自己修文件，与 read_claude_live 的做法一致。
    if !value.is_object() {
        return Err(AppError::Config(format!(
            "OpenCode 配置文件根节点必须是 JSON 对象: {}",
            path.display()
        )));
    }

    Ok(value)
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    read_opencode_config_from_path(&get_opencode_config_path())
}

fn write_opencode_config_to_path_with_contents(
    path: &Path,
    config: &Value,
) -> Result<Vec<u8>, AppError> {
    let contents = write_json_file_with_contents(path, config)?;

    log::debug!("OpenCode config written to {path:?}");
    Ok(contents)
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    let path = get_opencode_config_path();
    let mut full_config = read_opencode_config_from_path(&path)?;

    // 判空要连「存在但不是对象」一起算：否则下面 as_object_mut 拿不到，
    // 写入会静默失效——界面显示添加成功而文件里没有。provider 段是 cc-switch
    // 的投影区，归一化不会碰用户自有的 model / theme 等顶层配置。
    if !full_config.get("provider").is_some_and(Value::is_object) {
        if full_config.get("provider").is_some() {
            log::warn!("opencode.json 的 provider 不是对象，已重置为空对象");
        }
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config_to_path_with_contents(&path, &full_config).map(|_| ())
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    let path = get_opencode_config_path();
    let mut config = read_opencode_config_from_path(&path)?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    } else if config.get("provider").is_some() {
        log::warn!("opencode.json 的 provider 不是对象，无法删除供应商 '{id}'");
    }

    write_opencode_config_to_path_with_contents(&path, &config).map(|_| ())
}

/// Persist missing v2 API-key providers before importing the file into CC Switch.
/// The file is authoritative, including incomplete entries: never fill or replace
/// an existing provider using a different credential from the database.
/// Only the OpenCode Go subscription gets a default endpoint for model fetching.
pub fn import_credential_providers(
    existing_ids: &std::collections::HashSet<String>,
) -> Result<(), AppError> {
    let mut providers = match credentials::read_providers(&get_opencode_db_path()) {
        Ok(providers) => providers,
        Err(err) => {
            // v1/config-file imports must still work if the optional v2 DB is
            // locked, unreadable, or uses a schema we do not understand.
            log::warn!("Unable to read OpenCode credential providers: {err}");
            Map::new()
        }
    };
    // A provider already managed by CC Switch may have been removed from live
    // config intentionally. Import must not silently enable it again at startup.
    providers.retain(|id, _| !existing_ids.contains(id));
    merge_credential_providers(&get_opencode_config_path(), providers)
}

fn merge_credential_providers(path: &Path, providers: Map<String, Value>) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    let previous_contents = read_config_contents(path)?;
    let mut config = read_opencode_config_from_path(path)?;
    if !providers.is_empty()
        && config
            .get("provider")
            .is_some_and(|value| !value.is_object())
    {
        return Err(AppError::Config(
            "OpenCode provider must be an object before importing credentials".into(),
        ));
    }

    let mut changed = false;
    for (id, provider) in providers {
        // Native v2 entries also own their IDs; do not add a competing legacy
        // entry. Native provider editing is handled separately from this import.
        if config.get("provider").and_then(|v| v.get(&id)).is_some()
            || config.get("providers").and_then(|v| v.get(&id)).is_some()
        {
            continue;
        }
        if config.get("provider").is_none() {
            config["provider"] = json!({});
        }
        config["provider"][&id] = provider;
        changed = true;
    }
    // Also repair previously imported Go entries, even when the credential DB
    // is missing/unreadable or the provider is already managed by CC Switch.
    changed |= fill_go_base_url(&mut config);
    if changed {
        if read_config_contents(path)? != previous_contents {
            return Err(AppError::Config(
                "OpenCode config changed on disk. Please reload and try again.".into(),
            ));
        }
        write_opencode_config_to_path_with_contents(path, &config)?;
    }
    Ok(())
}

fn fill_go_base_url(config: &mut Value) -> bool {
    // Native v2 entries own all of their settings, including provider/model
    // baseURL, variants and canonical provider selection. Never inject a legacy
    // default beside them, even when a same-ID legacy entry is also present.
    if config
        .get("providers")
        .and_then(|v| v.get("opencode-go"))
        .is_some()
    {
        return false;
    }
    // The integration/provider ID identifies the Go subscription. A display
    // name or an empty URL on another provider must never opt it into Go.
    let Some(provider) = config
        .get_mut("provider")
        .and_then(|v| v.get_mut("opencode-go"))
    else {
        return false;
    };
    // Do not rewrite invalid provider fragments as a side effect of importing.
    if serde_json::from_value::<OpenCodeProviderConfig>(provider.clone()).is_err() {
        return false;
    }
    // Preserve v1 api routes and v2 settings, including model options that v2
    // migrates to settings. A provider-wide default must not replace a user's
    // routing choices. Environment references count as explicit addresses.
    let has_api = |value: &Value| value.get("api").is_some_and(Value::is_string);
    let has_base_url = |settings: &Value| {
        settings.get("baseURL").is_some_and(|url| match url {
            Value::Null => false,
            Value::String(url) => !url.trim().is_empty(),
            _ => true,
        })
    };
    let has_settings_url = |value: &Value| {
        ["options", "settings"]
            .iter()
            .any(|field| value.get(field).is_some_and(has_base_url))
    };
    if has_api(provider)
        || has_settings_url(provider)
        || provider
            .get("models")
            .and_then(Value::as_object)
            .is_some_and(|models| {
                models.values().any(|model| {
                    model.get("provider").is_some_and(has_api)
                        || has_settings_url(model)
                        || model
                            .get("variants")
                            .and_then(Value::as_object)
                            .is_some_and(|variants| variants.values().any(has_base_url))
                })
            })
    {
        return false;
    }
    let Some(provider) = provider.as_object_mut() else {
        return false;
    };
    let Some(options) = provider
        .entry("options")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return false;
    };
    let needs_base_url = match options.get("baseURL") {
        None | Some(Value::Null) => true,
        Some(Value::String(url)) => url.trim().is_empty(),
        _ => false,
    };
    if needs_base_url {
        options.insert("baseURL".into(), json!(OPENCODE_GO_BASE_URL));
    }
    needs_base_url
}

pub fn get_validated_providers() -> Result<IndexMap<String, Value>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(_) if !id.trim().is_empty() => {
                // The Rust type is only a validator. Serializing it back would
                // discard provider extensions and insert defaults into overrides.
                result.insert(id, value);
            }
            Ok(_) => log::warn!("Skipping OpenCode provider with empty ID"),
            Err(_) => {
                // Deserialization errors may contain credential values.
                log::warn!("Skipping invalid OpenCode provider '{id}'");
            }
        }
    }

    Ok(result)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    let path = get_opencode_config_path();
    let mut full_config = read_opencode_config_from_path(&path)?;

    if !full_config.get("mcp").is_some_and(Value::is_object) {
        if full_config.get("mcp").is_some() {
            log::warn!("opencode.json 的 mcp 不是对象，已重置为空对象");
        }
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config_to_path_with_contents(&path, &full_config).map(|_| ())
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    let path = get_opencode_config_path();
    let mut config = read_opencode_config_from_path(&path)?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    } else if config.get("mcp").is_some() {
        log::warn!("opencode.json 的 mcp 不是对象，无法删除服务器 '{id}'");
    }

    write_opencode_config_to_path_with_contents(&path, &config).map(|_| ())
}

pub fn add_plugin(path: &Path, plugin_name: &str) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    let mut config = read_opencode_config_from_path(path)?;
    let normalized_plugin_name = canonicalize_plugin_name(plugin_name);
    let target_is_omo =
        matches_any_plugin_prefix(&normalized_plugin_name, &STANDARD_OMO_PLUGIN_PREFIXES)
            || matches_any_plugin_prefix(&normalized_plugin_name, &SLIM_OMO_PLUGIN_PREFIXES);
    let mut changed = false;

    let plugins = config.get_mut("plugin").and_then(|v| v.as_array_mut());

    match plugins {
        Some(arr) => {
            let mut found_target = false;
            arr.retain(|value| {
                let Some(existing_name) = value.as_str() else {
                    return true;
                };
                if existing_name == normalized_plugin_name {
                    if found_target {
                        changed = true;
                        return false;
                    }
                    found_target = true;
                    return true;
                }

                // Standard OMO and OMO Slim are mutually exclusive.
                if target_is_omo
                    && (matches_any_plugin_prefix(existing_name, &STANDARD_OMO_PLUGIN_PREFIXES)
                        || matches_any_plugin_prefix(existing_name, &SLIM_OMO_PLUGIN_PREFIXES))
                {
                    changed = true;
                    return false;
                }
                true
            });

            if !found_target {
                arr.push(Value::String(normalized_plugin_name));
                changed = true;
            }
        }
        None => {
            config["plugin"] = json!([normalized_plugin_name]);
            changed = true;
        }
    }

    if !changed {
        return Ok(());
    }

    write_opencode_config_to_path_with_contents(path, &config).map(|_| ())
}

pub fn remove_plugins_by_prefixes(path: &Path, prefixes: &[&str]) -> Result<bool, AppError> {
    let _guard = opencode_config_lock().lock()?;
    let previous_contents = read_config_contents(path)?;
    let mut config = read_opencode_config_from_path(path)?;

    let mut changed = false;
    if let Some(arr) = config.get_mut("plugin").and_then(|v| v.as_array_mut()) {
        let previous_len = arr.len();
        arr.retain(|v| {
            v.as_str()
                .map(|s| !matches_any_plugin_prefix(s, prefixes))
                .unwrap_or(true)
        });
        changed = arr.len() != previous_len;

        if changed && arr.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("plugin"));
        }
    }

    if !changed {
        return Ok(false);
    }

    let current_contents = read_config_contents(path)?;
    if current_contents != previous_contents {
        return Err(AppError::Config(
            "OpenCode config changed on disk. Please reload and try again.".to_string(),
        ));
    }

    write_opencode_config_to_path_with_contents(path, &config)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHomeGuard {
        previous_home: Option<std::ffi::OsString>,
        _settings: crate::settings::TestSettingsGuard,
    }
    impl TestHomeGuard {
        fn set(home: &std::path::Path) -> Self {
            let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            Self {
                previous_home,
                _settings: crate::settings::TestSettingsGuard::new(),
            }
        }
    }
    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn write_config(home: &std::path::Path, content: &str) {
        let dir = home.join(".config").join("opencode");
        std::fs::create_dir_all(&dir).expect("create config dir");
        std::fs::write(dir.join("opencode.json"), content).expect("write config");
    }

    fn credential_provider() -> Map<String, Value> {
        serde_json::from_value(json!({
            "opencode-go": {"name": "OpenCode Go", "options": {"apiKey": "db-test-key"}}
        }))
        .unwrap()
    }

    #[test]
    fn credential_import_creates_config_from_missing_empty_or_empty_object_file() {
        for content in [None, Some(" \n"), Some("{}")] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("config").join("opencode.json");
            if let Some(content) = content {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, content).unwrap();
            }
            merge_credential_providers(&path, credential_provider()).unwrap();
            let config = read_opencode_config_from_path(&path).unwrap();
            assert_eq!(
                config["provider"]["opencode-go"]["options"]["apiKey"],
                "db-test-key"
            );
            assert!(config["provider"]["opencode-go"].get("npm").is_none());
            assert_eq!(
                config["provider"]["opencode-go"]["options"]["baseURL"],
                OPENCODE_GO_BASE_URL
            );
            let contents = std::fs::read(&path).unwrap();
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            merge_credential_providers(&path, credential_provider()).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), contents);
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                modified
            );
        }
    }

    #[test]
    fn credential_import_preserves_file_provider_and_unrelated_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.json");
        let original = json!({
            "theme": "dark", "model": "custom/model", "plugin": ["example"],
            "provider": {
                "custom": {"npm": "custom-sdk", "options": {"apiKey": "file-test-key"}, "futureField": true}
            }
        });
        std::fs::write(&path, original.to_string()).unwrap();
        let mut from_db = credential_provider();
        from_db.insert(
            "custom".into(),
            json!({"options": {"apiKey":"other-test-key"}}),
        );
        merge_credential_providers(&path, from_db).unwrap();
        let result = read_opencode_config_from_path(&path).unwrap();
        assert_eq!(result["provider"]["custom"], original["provider"]["custom"]);
        for field in ["theme", "model", "plugin"] {
            assert_eq!(result[field], original[field]);
        }
        assert_eq!(result["provider"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn credential_import_fills_missing_go_endpoint_without_credentials() {
        for base_url in [None, Some(Value::Null), Some(json!("")), Some(json!(" \t"))] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("opencode.json");
            let mut config = json!({
                "theme": "dark",
                "provider": {"opencode-go": {
                    "name": "Go from file",
                    "options": {"apiKey": "file-test-key", "headers": {"X-Custom": "keep"}},
                    "models": {"glm-5.2": {"limit": {"input": 64000}}},
                    "futureField": true
                }}
            });
            if let Some(url) = base_url {
                config["provider"]["opencode-go"]["options"]["baseURL"] = url;
            }
            std::fs::write(&path, config.to_string()).unwrap();
            merge_credential_providers(&path, Map::new()).unwrap();
            config["provider"]["opencode-go"]["options"]["baseURL"] = json!(OPENCODE_GO_BASE_URL);
            assert_eq!(read_opencode_config_from_path(&path).unwrap(), config);
            let contents = std::fs::read(&path).unwrap();
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            merge_credential_providers(&path, Map::new()).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), contents);
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                modified
            );
        }
    }

    #[test]
    fn credential_import_preserves_v1_go_api_routes() {
        for route in ["https://custom.example/v1", "{env:GO_URL}"] {
            for provider in [
                json!({"api": route, "options": {"apiKey": "file-test-key"}}),
                json!({
                    "options": {"apiKey": "file-test-key", "baseURL": ""},
                    "models": {"custom-model": {"provider": {"api": route}}}
                }),
            ] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("opencode.json");
                let original = json!({"provider": {"opencode-go": provider}}).to_string();
                std::fs::write(&path, &original).unwrap();

                merge_credential_providers(&path, credential_provider()).unwrap();

                // Import keeps both the selected file credential and v1's
                // effective route, without rewriting the source unnecessarily.
                assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            }
        }
    }

    #[test]
    fn credential_import_preserves_v2_go_model_endpoints_in_compatible_config() {
        for route in ["https://custom.example/v1", "{env:GO_URL}"] {
            for model in [
                json!({"options": {"baseURL": route}}),
                json!({"variants": {"custom": {"baseURL": route}}}),
            ] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("opencode.json");
                let original = json!({"provider": {"opencode-go": {
                    "options": {"apiKey": "file-test-key"},
                    "models": {"custom-model": model}
                }}})
                .to_string();
                std::fs::write(&path, &original).unwrap();

                merge_credential_providers(&path, credential_provider()).unwrap();

                assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            }
        }
    }

    #[test]
    fn credential_import_preserves_native_v2_go_routes_with_or_without_legacy_entry() {
        for route in ["https://custom.example/v1", "{env:GO_URL}"] {
            for native in [
                json!({"settings": {"baseURL": route}}),
                json!({"models": {"custom-model": {"settings": {"baseURL": route}}}}),
                json!({"models": {"custom-model": {
                    "variants": [{"id": "custom", "settings": {"baseURL": route}}]
                }}}),
                json!({"canonical": "openai", "settings": {"baseURL": route}}),
            ] {
                for with_legacy in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    let path = temp.path().join("opencode.json");
                    let mut config = json!({"providers": {"opencode-go": native}});
                    if with_legacy {
                        config["provider"] = json!({"opencode-go": {
                            "options": {"apiKey": "file-test-key"}
                        }});
                    }
                    let original = config.to_string();
                    std::fs::write(&path, &original).unwrap();

                    merge_credential_providers(&path, credential_provider()).unwrap();

                    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
                }
            }
        }
    }

    #[test]
    fn credential_import_only_fills_base_url_for_go_subscription_id() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.json");
        let mut providers = credential_provider();
        for id in ["openai", "anthropic", "opencode", "custom-go"] {
            // Even a misleading label must not select the Go subscription.
            providers.insert(
                id.into(),
                json!({"name": "OpenCode Go", "options": {"apiKey": "test-key"}}),
            );
        }
        let mut expected = json!({"provider": providers});
        std::fs::write(&path, expected.to_string()).unwrap();

        merge_credential_providers(&path, providers.clone()).unwrap();

        expected["provider"]["opencode-go"]["options"]["baseURL"] = json!(OPENCODE_GO_BASE_URL);
        assert_eq!(read_opencode_config_from_path(&path).unwrap(), expected);

        // The same restriction applies to freshly imported DB credentials.
        std::fs::write(&path, "{}").unwrap();
        merge_credential_providers(&path, providers).unwrap();
        assert_eq!(read_opencode_config_from_path(&path).unwrap(), expected);
    }

    #[test]
    fn credential_import_does_not_replace_existing_entries_or_rewrite_source() {
        for original in [
            r#"{ // already configured
 provider: { "opencode-go": {options: {baseURL: "https://custom.example/v1", apiKey: "file-test-key"}} }}"#,
            r#"{ provider: { "opencode-go": {options: {baseURL: "{env:GO_URL}"}} } }"#,
            "{ provider: { 'opencode-go': null } }",
            "{ providers: { 'opencode-go': {settings: {apiKey: 'native-test-key'}} } }",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("opencode.json");
            std::fs::write(&path, original).unwrap();
            merge_credential_providers(&path, credential_provider()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn credential_import_rejects_malformed_config_without_replacing_it() {
        for original in ["{broken", "[]", "{provider: []}"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("opencode.json");
            std::fs::write(&path, original).unwrap();
            assert!(merge_credential_providers(&path, credential_provider()).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn credential_import_reports_failure_to_persist_config() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("blocked");
        std::fs::write(&parent, "a file, not a directory").unwrap();
        assert!(
            merge_credential_providers(&parent.join("opencode.json"), credential_provider())
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(&parent).unwrap(),
            "a file, not a directory"
        );
    }

    #[test]
    #[serial_test::serial]
    fn read_rejects_non_object_root_instead_of_panicking_downstream() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 顶层数组/标量会让下游 `config["provider"] = …` 触发 serde_json panic。
        // 顶层 null 例外——serde_json 会把它自动升级成对象，本来就不炸。
        for malformed in ["[]", "[{\"a\":1}]", "42", "\"oops\""] {
            write_config(temp.path(), malformed);
            let result = read_opencode_config();
            assert!(
                result.is_err(),
                "non-object root must be rejected: {malformed}"
            );
        }

        write_config(temp.path(), "{\"model\": \"x\"}");
        assert!(
            read_opencode_config().is_ok(),
            "a normal object config must still load"
        );
    }

    #[test]
    #[serial_test::serial]
    fn set_mcp_server_normalizes_non_object_section() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // `"mcp": []` 时旧代码的 as_object_mut 返回 None → 写入静默失效
        write_config(temp.path(), "{\"model\": \"keep-me\", \"mcp\": []}");

        set_mcp_server("echo", json!({"command": "npx"})).expect("set must succeed");

        let config = read_opencode_config().expect("reload");
        assert_eq!(
            config["mcp"]["echo"]["command"], "npx",
            "server must actually be written"
        );
        assert_eq!(
            config["model"], "keep-me",
            "unrelated user config must be preserved"
        );
    }

    #[test]
    fn remove_missing_plugin_does_not_create_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");

        let result = remove_plugins_by_prefixes(&path, &["oh-my-openagent"]).unwrap();

        assert!(!result);
        assert!(!path.exists());
    }

    #[test]
    fn remove_missing_plugin_preserves_existing_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        let original = r#"{
  // Keep formatting when the target plugin is absent.
  "plugin": ["unrelated-plugin"],
  "theme": "dark",
}"#;
        std::fs::write(&path, original).unwrap();

        let result = remove_plugins_by_prefixes(&path, &["oh-my-openagent"]).unwrap();

        assert!(!result);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn add_existing_plugin_preserves_existing_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");
        let original = r#"{
  // Keep comments and formatting when the plugin is already configured.
  plugin: ['oh-my-openagent@latest'],
  theme: 'dark',
}"#;
        std::fs::write(&path, original).unwrap();

        add_plugin(&path, "oh-my-openagent@latest").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
