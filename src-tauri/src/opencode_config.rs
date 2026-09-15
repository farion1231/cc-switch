use crate::config::write_json_file_with_contents;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const STANDARD_OMO_PLUGIN_PREFIXES: [&str; 2] = ["oh-my-openagent", "oh-my-opencode"];
const SLIM_OMO_PLUGIN_PREFIXES: [&str; 1] = ["oh-my-opencode-slim"];
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

/// OpenCode 在全局配置目录里认三个文件名，`globalConfigFile()` 按这个顺序取第一个
/// 存在的，全都不存在时新建 `opencode.jsonc`，见
/// https://github.com/sst/opencode/blob/dev/packages/opencode/src/config/config.ts
///
/// 读取侧它三个都读并 `mergeDeep`，后合并的覆盖先合并的，所以 `opencode.jsonc`
/// 优先级最高：写进 `opencode.json` 的同名键会被它静默盖掉。
const OPENCODE_CONFIG_FILE_NAMES: [&str; 3] = ["opencode.jsonc", "opencode.json", "config.json"];

pub fn get_opencode_config_path() -> PathBuf {
    let dir = get_opencode_dir();

    for name in OPENCODE_CONFIG_FILE_NAMES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }

    // 一个都不存在时保持建 `opencode.json`：OpenCode 读取时会一并合并它，
    // 之后 `globalConfigFile()` 也会选中它，行为与本函数此前一致。
    dir.join("opencode.json")
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

/// 已存在的配置文件，按 OpenCode 的**合并顺序**返回（后者覆盖前者），
/// 即 `OPENCODE_CONFIG_FILE_NAMES` 的倒序。
fn existing_opencode_config_paths() -> Vec<PathBuf> {
    let dir = get_opencode_dir();

    OPENCODE_CONFIG_FILE_NAMES
        .iter()
        .rev()
        .map(|name| dir.join(name))
        .filter(|path| path.is_file())
        .collect()
}

/// 对齐 OpenCode 的 `mergeConfig`（remeda `mergeDeep`）：对象逐键递归合并，
/// 其余类型（含数组）由 source 整体替换。
fn merge_config_into(target: &mut Value, source: Value) {
    match source {
        Value::Object(source_obj) if target.is_object() => {
            let target_obj = target.as_object_mut().expect("checked by the guard");

            for (key, value) in source_obj {
                match target_obj.get_mut(&key) {
                    Some(existing) if existing.is_object() && value.is_object() => {
                        merge_config_into(existing, value);
                    }
                    _ => {
                        target_obj.insert(key, value);
                    }
                }
            }
        }
        other => *target = other,
    }
}

/// 读取「有效配置」：OpenCode 会把全局目录下的 `config.json`、`opencode.json`、
/// `opencode.jsonc` 全部读出来依次 `mergeDeep`，只看其中一个文件会漏掉另一个文件里
/// 的供应商——例如旧版 CC Switch 把供应商写进了 `opencode.json`，而 OpenCode 自己
/// 建了 `opencode.jsonc`。
pub fn read_opencode_config() -> Result<Value, AppError> {
    let paths = existing_opencode_config_paths();

    let Some((first, rest)) = paths.split_first() else {
        // 一个都不存在，保持与单文件读取相同的空配置。
        return read_opencode_config_from_path(&get_opencode_config_path());
    };

    let mut merged = read_opencode_config_from_path(first)?;
    for path in rest {
        merge_config_into(&mut merged, read_opencode_config_from_path(path)?);
    }

    Ok(merged)
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

/// 从**每个**已存在的配置文件里删掉 `<section>.<id>`。
///
/// 只删最高优先级的那份是不够的：OpenCode 合并全部三个文件，低优先级文件里的同名
/// 条目会在删除后重新生效，界面显示已删除而 OpenCode 仍在用它。
///
/// 只回写真正发生变化的文件——未命中的文件保持原样，避免整份重新序列化把用户的
/// 注释和格式抹掉。
fn remove_entry_from_all_configs(section: &str, id: &str) -> Result<(), AppError> {
    let paths = existing_opencode_config_paths();

    if paths.is_empty() {
        // 与此前一致：没有任何配置文件时仍在目标路径落一份空配置。
        let path = get_opencode_config_path();
        let config = read_opencode_config_from_path(&path)?;
        return write_opencode_config_to_path_with_contents(&path, &config).map(|_| ());
    }

    for path in paths {
        let mut config = read_opencode_config_from_path(&path)?;

        let removed = match config.get_mut(section).and_then(|v| v.as_object_mut()) {
            Some(entries) => entries.remove(id).is_some(),
            None => {
                if config.get(section).is_some() {
                    log::warn!("{} 的 {section} 不是对象，无法删除 '{id}'", path.display());
                }
                false
            }
        };

        if removed {
            write_opencode_config_to_path_with_contents(&path, &config)?;
        }
    }

    Ok(())
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let _guard = opencode_config_lock().lock()?;
    remove_entry_from_all_configs("provider", id)
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
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
    remove_entry_from_all_configs("mcp", id)
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

    struct TestHomeGuard(Option<std::ffi::OsString>);
    impl TestHomeGuard {
        fn set(home: &std::path::Path) -> Self {
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

    fn write_config(home: &std::path::Path, content: &str) {
        write_config_named(home, "opencode.json", content);
    }

    fn write_config_named(home: &std::path::Path, file_name: &str, content: &str) {
        let dir = home.join(".config").join("opencode");
        std::fs::create_dir_all(&dir).expect("create config dir");
        std::fs::write(dir.join(file_name), content).expect("write config");
    }

    #[test]
    #[serial_test::serial]
    fn config_path_prefers_existing_jsonc_over_json() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        write_config_named(temp.path(), "config.json", "{}");
        assert_eq!(
            get_opencode_config_path().file_name().unwrap(),
            "config.json",
            "legacy config.json is used when it is the only file present"
        );

        write_config_named(temp.path(), "opencode.json", "{}");
        assert_eq!(
            get_opencode_config_path().file_name().unwrap(),
            "opencode.json",
            "opencode.json outranks the legacy config.json"
        );

        write_config_named(temp.path(), "opencode.jsonc", "{}");
        assert_eq!(
            get_opencode_config_path().file_name().unwrap(),
            "opencode.jsonc",
            "opencode.jsonc outranks both, matching OpenCode's globalConfigFile()"
        );
    }

    #[test]
    #[serial_test::serial]
    fn config_path_falls_back_to_json_when_no_config_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        assert_eq!(
            get_opencode_config_path().file_name().unwrap(),
            "opencode.json",
            "a fresh install keeps creating opencode.json"
        );
    }

    #[test]
    #[serial_test::serial]
    fn config_path_ignores_a_directory_named_like_a_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        let dir = temp.path().join(".config").join("opencode");
        std::fs::create_dir_all(dir.join("opencode.jsonc")).expect("create decoy dir");

        assert_eq!(
            get_opencode_config_path().file_name().unwrap(),
            "opencode.json",
            "a directory must not be mistaken for a config file"
        );
    }

    #[test]
    #[serial_test::serial]
    fn set_provider_updates_existing_jsonc_instead_of_creating_a_second_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // OpenCode 新装时建的就是 opencode.jsonc，且允许注释。
        write_config_named(
            temp.path(),
            "opencode.jsonc",
            "{\n  // user config\n  \"model\": \"keep-me\"\n}",
        );

        set_provider("acme", json!({"npm": "@ai-sdk/openai-compatible"}))
            .expect("set_provider must succeed");

        let dir = temp.path().join(".config").join("opencode");
        assert!(
            !dir.join("opencode.json").exists(),
            "writing must not create a second config file next to opencode.jsonc"
        );

        let config = read_opencode_config().expect("reload");
        assert_eq!(
            config["provider"]["acme"]["npm"], "@ai-sdk/openai-compatible",
            "the provider must land in the file OpenCode actually reads last"
        );
        assert_eq!(
            config["model"], "keep-me",
            "unrelated user config must be preserved"
        );
    }

    #[test]
    #[serial_test::serial]
    fn read_sees_providers_declared_only_in_jsonc() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        write_config_named(
            temp.path(),
            "opencode.jsonc",
            "{\n  // trailing commas and comments are valid JSONC\n  \"provider\": {\n    \"acme\": { \"npm\": \"@ai-sdk/openai-compatible\" },\n  },\n}",
        );

        let providers = get_providers().expect("providers must load");
        assert!(
            providers.contains_key("acme"),
            "providers declared in opencode.jsonc must be visible"
        );
    }

    #[test]
    #[serial_test::serial]
    fn read_merges_every_config_file_the_way_opencode_does() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        // 常见的升级态：老版本 CC Switch 把供应商写进了 opencode.json，
        // 而 OpenCode 自己建了 opencode.jsonc。
        write_config_named(
            temp.path(),
            "config.json",
            r#"{"provider": {"legacy": {"npm": "old"}}, "model": "from-config-json"}"#,
        );
        write_config_named(
            temp.path(),
            "opencode.json",
            r#"{"provider": {"written-by-cc-switch": {"npm": "x"}}, "model": "from-opencode-json"}"#,
        );
        write_config_named(
            temp.path(),
            "opencode.jsonc",
            "{\n  // created by OpenCode\n  \"provider\": { \"hand-written\": { \"npm\": \"y\" } }\n}",
        );

        let providers = get_providers().expect("providers must load");
        let mut ids: Vec<&String> = providers.keys().collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["hand-written", "legacy", "written-by-cc-switch"],
            "providers from every config file must be visible"
        );

        let config = read_opencode_config().expect("read");
        assert_eq!(
            config["model"], "from-opencode-json",
            "later files win on conflicting scalars, as OpenCode's mergeDeep does"
        );
    }

    #[test]
    #[serial_test::serial]
    fn remove_provider_purges_every_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        write_config_named(
            temp.path(),
            "opencode.json",
            r#"{"provider": {"acme": {"npm": "stale"}, "keep": {"npm": "z"}}}"#,
        );
        write_config_named(
            temp.path(),
            "opencode.jsonc",
            r#"{"provider": {"acme": {"npm": "current"}}}"#,
        );

        remove_provider("acme").expect("remove must succeed");

        let providers = get_providers().expect("reload");
        assert!(
            !providers.contains_key("acme"),
            "a provider present in several files must not come back from a lower-priority one"
        );
        assert!(
            providers.contains_key("keep"),
            "unrelated providers must survive"
        );
    }

    #[test]
    #[serial_test::serial]
    fn remove_provider_leaves_files_without_that_entry_untouched() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = TestHomeGuard::set(temp.path());

        let untouched = "{\n  // hand-written, keep my comments\n  \"model\": \"m\",\n}";
        write_config_named(temp.path(), "opencode.jsonc", untouched);
        write_config_named(
            temp.path(),
            "opencode.json",
            r#"{"provider": {"acme": {"npm": "x"}}}"#,
        );

        remove_provider("acme").expect("remove must succeed");

        let dir = temp.path().join(".config").join("opencode");
        assert_eq!(
            std::fs::read_to_string(dir.join("opencode.jsonc")).unwrap(),
            untouched,
            "a file that does not hold the entry must not be rewritten"
        );
        assert!(
            !get_providers().unwrap().contains_key("acme"),
            "the entry must still be gone"
        );
    }

    #[test]
    fn merge_config_recurses_into_objects_and_replaces_everything_else() {
        let mut target = json!({
            "provider": {"a": {"npm": "1"}, "b": {"npm": "2"}},
            "instructions": ["one"],
            "model": "old",
        });
        merge_config_into(
            &mut target,
            json!({
                "provider": {"b": {"npm": "3"}, "c": {"npm": "4"}},
                "instructions": ["two"],
                "model": "new",
            }),
        );

        assert_eq!(
            target["provider"]["a"]["npm"], "1",
            "untouched key survives"
        );
        assert_eq!(target["provider"]["b"]["npm"], "3", "conflicting key wins");
        assert_eq!(target["provider"]["c"]["npm"], "4", "new key is added");
        assert_eq!(
            target["instructions"],
            json!(["two"]),
            "arrays are replaced, not concatenated, matching mergeDeep"
        );
        assert_eq!(target["model"], "new");
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
