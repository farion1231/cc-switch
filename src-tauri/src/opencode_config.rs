use crate::config::atomic_write;
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use crate::settings::get_opencode_override_dir;
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

pub fn get_opencode_config_path() -> Result<PathBuf, AppError> {
    resolve_config_path(&get_opencode_dir())
}

fn resolve_config_path(dir: &Path) -> Result<PathBuf, AppError> {
    for name in ["opencode.jsonc", "opencode.json"] {
        let path = dir.join(name);
        if path.try_exists().map_err(|e| AppError::io(&path, e))? {
            return Ok(path);
        }
    }
    Ok(dir.join("opencode.json"))
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

struct OpenCodeDocument {
    path: PathBuf,
    previous_contents: Option<Vec<u8>>,
    document: JsoncDocument,
}

impl OpenCodeDocument {
    fn load(path: &Path) -> Result<Self, AppError> {
        let previous_contents = read_config_contents(path)?;
        let source = match &previous_contents {
            Some(contents) => std::str::from_utf8(contents).map_err(|e| {
                AppError::Config(format!(
                    "Invalid UTF-8 in OpenCode config {}: {e}",
                    path.display()
                ))
            })?,
            None => "{\n  \"$schema\": \"https://opencode.ai/config.json\"\n}\n",
        };
        let document = JsoncDocument::parse(source).map_err(|e| {
            AppError::Config(format!("Invalid OpenCode config {}: {e}", path.display()))
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            previous_contents,
            document,
        })
    }

    // The caller holds opencode_config_lock from path selection through commit.
    fn save(self) -> Result<(), AppError> {
        let source = self.document.validated_source()?;
        if read_config_contents(&self.path)? != self.previous_contents {
            return Err(AppError::Config(format!(
                "OpenCode config changed on disk. Please reload and try again: {}",
                self.path.display()
            )));
        }
        if self.previous_contents.as_deref() != Some(source.as_bytes()) {
            atomic_write(&self.path, source.as_bytes())?;
        }
        Ok(())
    }
}

pub(crate) fn read_opencode_config_from_path(path: &Path) -> Result<Value, AppError> {
    Ok(OpenCodeDocument::load(path)?.document.value().clone())
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    read_opencode_config_from_path(&get_opencode_config_path()?)
}

fn edit_config(
    resolve_path: impl FnOnce() -> Result<PathBuf, AppError>,
    edit: impl FnOnce(&mut Value),
) -> Result<bool, AppError> {
    let _guard = opencode_config_lock().lock()?;
    let path = resolve_path()?;
    let mut document = OpenCodeDocument::load(&path)?;
    let mut desired = document.document.value().clone();
    edit(&mut desired);
    if !document.document.apply(&desired)? {
        return Ok(false);
    }
    document.save()?;
    Ok(true)
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
    edit_config(get_opencode_config_path, |full_config| {
        if !full_config.get("provider").is_some_and(Value::is_object) {
            if full_config.get("provider").is_some() {
                log::warn!("OpenCode 的供应商配置格式有误，将清空原有供应商配置，再保存当前供应商");
            }
            full_config["provider"] = json!({});
        }
        full_config["provider"][id] = config;
    })
    .map(|_| ())
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    edit_config(get_opencode_config_path, |config| {
        if let Some(providers) = config.get_mut("provider").and_then(Value::as_object_mut) {
            providers.remove(id);
        }
    })
    .map(|_| ())
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
    edit_config(get_opencode_config_path, |full_config| {
        if !full_config.get("mcp").is_some_and(Value::is_object) {
            if full_config.get("mcp").is_some() {
                log::warn!(
                    "OpenCode 的 MCP 服务配置格式有误，将清空原有 MCP 服务配置，再保存当前服务"
                );
            }
            full_config["mcp"] = json!({});
        }
        full_config["mcp"][id] = config;
    })
    .map(|_| ())
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    edit_config(get_opencode_config_path, |config| {
        if let Some(mcp) = config.get_mut("mcp").and_then(Value::as_object_mut) {
            mcp.remove(id);
        }
    })
    .map(|_| ())
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    edit_config(get_opencode_config_path, |config| {
        insert_plugin(config, plugin_name)
    })
    .map(|_| ())
}

fn insert_plugin(config: &mut Value, plugin_name: &str) {
    let normalized = canonicalize_plugin_name(plugin_name);
    let target_is_omo = matches_any_plugin_prefix(&normalized, &STANDARD_OMO_PLUGIN_PREFIXES)
        || matches_any_plugin_prefix(&normalized, &SLIM_OMO_PLUGIN_PREFIXES);
    if let Some(plugins) = config.get_mut("plugin").and_then(Value::as_array_mut) {
        let mut found = false;
        plugins.retain(|value| {
            let Some(name) = value.as_str() else {
                return true;
            };
            if name == normalized {
                let keep = !found;
                found = true;
                return keep;
            }
            // Standard OMO and OMO Slim remain mutually exclusive.
            !(target_is_omo
                && (matches_any_plugin_prefix(name, &STANDARD_OMO_PLUGIN_PREFIXES)
                    || matches_any_plugin_prefix(name, &SLIM_OMO_PLUGIN_PREFIXES)))
        });
        if !found {
            plugins.push(Value::String(normalized));
        }
    } else {
        config["plugin"] = json!([normalized]);
    }
}

pub fn remove_plugins_by_prefixes(prefixes: &[&str]) -> Result<bool, AppError> {
    edit_config(get_opencode_config_path, |config| {
        remove_plugins(config, prefixes)
    })
}

fn remove_plugins(config: &mut Value, prefixes: &[&str]) {
    if let Some(plugins) = config.get_mut("plugin").and_then(Value::as_array_mut) {
        let previous_len = plugins.len();
        plugins.retain(|value| {
            value
                .as_str()
                .is_none_or(|name| !matches_any_plugin_prefix(name, prefixes))
        });
        if plugins.len() != previous_len && plugins.is_empty() {
            config
                .as_object_mut()
                .expect("validated object root")
                .remove("plugin");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHomeGuard(Option<std::ffi::OsString>, crate::settings::AppSettings);
    impl TestHomeGuard {
        fn set(home: &std::path::Path) -> Self {
            let previous_env = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", home);
            let guard = Self(previous_env, crate::settings::get_settings());
            crate::settings::update_settings(Default::default()).unwrap();
            guard
        }
    }
    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            crate::settings::update_settings(self.1.clone()).unwrap();
            match self.0.take() {
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
    #[serial_test::serial]
    fn unicode_line_comments_do_not_panic_or_poison_later_writes() {
        let temp = tempfile::tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        std::fs::create_dir_all(get_opencode_dir()).unwrap();
        let path = get_opencode_dir().join("opencode.jsonc");

        for suffix in [
            "// 中文",
            "// 😀",
            "/* 中文 */ // 尾",
            "// ab\u{2028}",
            "// 中文\u{2028}",
            "// ab\u{2029}",
            "// 中文\u{2029}",
        ] {
            let source = format!("{{\"provider\":{{}}}} {suffix}");
            std::fs::write(&path, &source).unwrap();

            // Startup import uses this read path without holding the write lock.
            let read = std::panic::catch_unwind(read_opencode_config)
                .expect("valid Unicode line comments must not panic during reads")
                .unwrap();
            assert_eq!(read, json!({"provider":{}}));
            assert_eq!(std::fs::read(&path).unwrap(), source.as_bytes());

            // These inputs used to unwind while holding opencode_config_lock.
            std::panic::catch_unwind(|| set_provider("first", json!({"name":"First"})))
                .expect("valid Unicode line comments must not panic during edits")
                .unwrap();
            assert!(!opencode_config_lock().is_poisoned());
            assert!(std::fs::read_to_string(&path).unwrap().ends_with(suffix));

            // Repeat the reported recovery scenario: replace the config with plain
            // JSON and verify all three writers still work in the same process.
            std::fs::write(&path, "{}").unwrap();
            set_provider("next", json!({"name":"Next"})).unwrap();
            set_mcp_server("tool", json!({"type":"local","command":["echo"]})).unwrap();
            add_plugin("oh-my-openagent@latest").unwrap();
            assert_eq!(
                read_opencode_config().unwrap(),
                json!({
                    "provider":{"next":{"name":"Next"}},
                    "mcp":{"tool":{"type":"local","command":["echo"]}},
                    "plugin":["oh-my-openagent@latest"]
                })
            );
        }
    }

    #[test]
    fn remove_missing_plugin_does_not_create_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("opencode.json");

        let result = edit_config(
            || Ok(path.clone()),
            |config| remove_plugins(config, &["oh-my-openagent"]),
        )
        .unwrap();

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

        let result = edit_config(
            || Ok(path.clone()),
            |config| remove_plugins(config, &["oh-my-openagent"]),
        )
        .unwrap();

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

        edit_config(
            || Ok(path.clone()),
            |config| insert_plugin(config, "oh-my-openagent@latest"),
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    #[serial_test::serial]
    fn selection_and_crud_preserve_selected_file_and_leave_other_file_untouched() {
        for custom in [false, true] {
            for (has_json, has_jsonc) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let temp = tempfile::tempdir().unwrap();
                let _guard = TestHomeGuard::set(temp.path());
                let dir = if custom {
                    let dir = temp.path().join("custom");
                    crate::settings::update_settings(crate::settings::AppSettings {
                        opencode_config_dir: Some(dir.to_string_lossy().into_owned()),
                        ..Default::default()
                    })
                    .unwrap();
                    dir
                } else {
                    temp.path().join(".config/opencode")
                };
                std::fs::create_dir_all(&dir).unwrap();
                let json_path = dir.join("opencode.json");
                let jsonc_path = dir.join("opencode.jsonc");
                let original = "{\r\n\t/* 中文注释 */\r\n\t\"model\": \"keep\", // 模型\r\n\t\"provider\": {},\r\n\t\"mcp\": {},\r\n}\r\n";
                if has_json {
                    std::fs::write(&json_path, original).unwrap();
                }
                if has_jsonc {
                    std::fs::write(&jsonc_path, original).unwrap();
                }
                let selected = if has_jsonc { &jsonc_path } else { &json_path };
                assert_eq!(get_opencode_config_path().unwrap(), *selected);
                remove_provider("missing").unwrap();
                remove_mcp_server("missing").unwrap();
                assert_eq!(selected.exists(), has_json || has_jsonc);

                for value in ["first", "updated"] {
                    set_provider("escaped\"供应商", json!({"options":{"apiKey":value}})).unwrap();
                    set_mcp_server("tool", json!({"type":"local","command":[value]})).unwrap();
                    assert_eq!(
                        get_providers().unwrap()["escaped\"供应商"]["options"]["apiKey"],
                        value
                    );
                    assert_eq!(get_mcp_servers().unwrap()["tool"]["command"][0], value);
                    let before = std::fs::read(selected).unwrap();
                    let modified = std::fs::metadata(selected).unwrap().modified().unwrap();
                    set_provider("escaped\"供应商", json!({"options":{"apiKey":value}})).unwrap();
                    set_mcp_server("tool", json!({"type":"local","command":[value]})).unwrap();
                    assert_eq!(std::fs::read(selected).unwrap(), before);
                    assert_eq!(
                        std::fs::metadata(selected).unwrap().modified().unwrap(),
                        modified
                    );
                }
                add_plugin("unrelated").unwrap();
                add_plugin("oh-my-opencode@latest").unwrap();
                assert_eq!(
                    read_opencode_config().unwrap()["plugin"],
                    json!(["unrelated", "oh-my-openagent@latest"])
                );
                add_plugin("oh-my-opencode-slim@latest").unwrap();
                assert_eq!(
                    read_opencode_config().unwrap()["plugin"],
                    json!(["unrelated", "oh-my-opencode-slim@latest"])
                );
                assert!(remove_plugins_by_prefixes(&SLIM_OMO_PLUGIN_PREFIXES).unwrap());
                assert_eq!(
                    read_opencode_config().unwrap()["plugin"],
                    json!(["unrelated"])
                );
                remove_provider("escaped\"供应商").unwrap();
                remove_mcp_server("tool").unwrap();
                assert!(get_providers().unwrap().is_empty());
                assert!(get_mcp_servers().unwrap().is_empty());
                let saved = std::fs::read_to_string(selected).unwrap();
                if has_json || has_jsonc {
                    assert!(
                        saved.contains("\t/* 中文注释 */\r\n\t\"model\": \"keep\", // 模型\r\n")
                    );
                    assert!(!saved.replace("\r\n", "").contains('\n'));
                }
                if has_json && has_jsonc {
                    assert_eq!(std::fs::read_to_string(&json_path).unwrap(), original);
                }
                assert_eq!(jsonc_path.exists(), has_jsonc);
                assert_eq!(json_path.exists(), has_json || !has_jsonc);
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn invalid_selected_config_never_falls_back_or_overwrites_files() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        write_config(temp.path(), "{\"model\":\"fallback\"}");
        let path = get_opencode_dir().join("opencode.jsonc");
        for invalid in ["{broken", "[]", "null", "42", "\"text\""] {
            std::fs::write(&path, invalid).unwrap();
            assert!(read_opencode_config().is_err());
            assert!(set_provider("p", json!({})).is_err());
            assert!(remove_provider("p").is_err());
            assert!(set_mcp_server("m", json!({})).is_err());
            assert!(remove_mcp_server("m").is_err());
            assert!(add_plugin("oh-my-openagent").is_err());
            assert!(remove_plugins_by_prefixes(&STANDARD_OMO_PLUGIN_PREFIXES).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        }
        assert_eq!(
            std::fs::read_to_string(get_opencode_dir().join("opencode.json")).unwrap(),
            "{\"model\":\"fallback\"}"
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(read_opencode_config().is_err());
        assert!(set_provider("p", json!({})).is_err());
    }

    #[test]
    fn edits_detect_external_changes_and_pin_the_selected_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.json");
        for original in [None, Some("{}")] {
            if let Some(original) = original {
                std::fs::write(&path, original).unwrap();
            }
            let error = edit_config(
                || resolve_config_path(temp.path()),
                |value| {
                    value["model"] = json!("ours");
                    std::fs::write(&path, "{\"model\":\"external\"}").unwrap();
                },
            )
            .unwrap_err();
            assert!(error.to_string().contains("changed on disk"));
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "{\"model\":\"external\"}"
            );
        }
        edit_config(
            || resolve_config_path(temp.path()),
            |value| {
                value["model"] = json!("ours");
                std::fs::write(temp.path().join("opencode.jsonc"), "{}").unwrap();
            },
        )
        .unwrap();
        assert_eq!(
            read_opencode_config_from_path(&path).unwrap()["model"],
            "ours"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("opencode.jsonc")).unwrap(),
            "{}"
        );
        assert!(edit_config(
            || Ok(path.clone()),
            |value| {
                value["model"] = json!("next");
                std::fs::remove_file(&path).unwrap();
            }
        )
        .is_err());
        assert!(!path.exists());
    }

    #[test]
    fn path_lookup_propagates_access_errors() {
        let temp = tempfile::tempdir().unwrap();
        assert!(resolve_config_path(&temp.path().join("invalid\0directory")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_selected_file_never_falls_back() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.jsonc");
        std::fs::write(&path, "{}").unwrap();
        std::fs::write(temp.path().join("opencode.json"), "{\"keep\":true}").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(edit_config(
            || resolve_config_path(temp.path()),
            |value| value["new"] = json!(true)
        )
        .is_err());
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(
            std::fs::read_to_string(temp.path().join("opencode.json")).unwrap(),
            "{\"keep\":true}"
        );
    }

    #[test]
    fn invalid_output_never_overwrites_original_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.jsonc");
        let original = "{/* keep */\"model\":\"original\"}";
        std::fs::write(&path, original).unwrap();
        for parseable in [false, true] {
            let _guard = opencode_config_lock().lock().unwrap();
            let mut document = OpenCodeDocument::load(&path).unwrap();
            document
                .document
                .apply(&json!({"model":"changed"}))
                .unwrap();
            document.document.corrupt_output_for_test(parseable);
            assert!(document.save().is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }
}
