use crate::config::atomic_write;
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use crate::provider::OpenCodeConfigFormat;
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
    try_edit_config(resolve_path, |config| {
        edit(config);
        Ok(())
    })
}

/// Like `edit_config`, but the edit may reject the change; nothing is written then.
fn try_edit_config(
    resolve_path: impl FnOnce() -> Result<PathBuf, AppError>,
    edit: impl FnOnce(&mut Value) -> Result<(), AppError>,
) -> Result<bool, AppError> {
    let _guard = opencode_config_lock().lock()?;
    let path = resolve_path()?;
    let mut document = OpenCodeDocument::load(&path)?;
    let mut desired = document.document.value().clone();
    edit(&mut desired)?;
    if !document.document.apply(&desired)? {
        return Ok(false);
    }
    document.save()?;
    Ok(true)
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    Ok(get_providers_with_format()?
        .into_iter()
        .map(|(id, (value, _))| (id, value))
        .collect())
}

/// Preserve the declaration's format alongside its JSON, including package-less
/// built-in overrides. Do not recursively mix legacy and native entries.
pub fn get_providers_with_format(
) -> Result<IndexMap<String, (Value, OpenCodeConfigFormat)>, AppError> {
    let mut config = read_opencode_config()?;
    let mut take = |key| match config.get_mut(key).map(Value::take) {
        Some(Value::Object(providers)) => providers,
        _ => Map::new(),
    };
    let (legacy, native) = (take("provider"), take("providers"));
    let mut providers: IndexMap<_, _> = legacy
        .into_iter()
        .map(|(id, value)| (id, (value, OpenCodeConfigFormat::V1)))
        .collect();
    for (id, value) in native {
        match native_provider_problem(&value) {
            None => {
                providers.insert(id, (value, OpenCodeConfigFormat::V2));
            }
            Some(path) => {
                let field = if path.is_empty() { "<root>" } else { &path };
                log::warn!(
                    "Invalid native OpenCode provider '{id}' at {field}, leaving its source untouched"
                );
            }
        }
    }
    Ok(providers)
}

/// Validate known native fields before giving V2 precedence over V1. Keep the
/// original JSON (including unknown extensions) rather than projecting it into
/// a partial type. Constraints follow OpenCode v2.0.12, commit 2670273ff17d:
/// packages/schema/src/config/provider.ts, model.ts and provider.ts.
pub fn is_native_provider(value: &Value) -> bool {
    native_provider_problem(value).is_none()
}

/// The dotted path of the first known field that does not match the native
/// schema; an empty path means the declaration is not an object. Like OpenCode,
/// which decodes with `onExcessProperty: "ignore"`, treat V1-only keys such as
/// `npm` and `options` as unknown extensions rather than as a format mismatch.
pub fn native_provider_problem(value: &Value) -> Option<String> {
    let Some(obj) = value.as_object() else {
        return Some(String::new());
    };
    if let Some(key) = first_invalid_field(
        obj,
        &[
            ("name", Value::is_string),
            ("package", Value::is_string),
            ("canonical", Value::is_string),
            ("env", native_string_array),
            ("settings", native_provider_settings),
            ("headers", native_headers),
            ("body", Value::is_object),
        ],
    ) {
        return Some(key.to_string());
    }
    match obj.get("models") {
        None => None,
        Some(Value::Object(models)) => models.iter().find_map(|(id, model)| {
            native_model_problem(model).map(|key| match key {
                "" => format!("models.{id}"),
                key => format!("models.{id}.{key}"),
            })
        }),
        Some(_) => Some("models".to_string()),
    }
}

type NativeFieldCheck<'a> = (&'a str, fn(&Value) -> bool);

/// Optional means absent, not null. Unknown keys remain untouched at every level.
fn first_invalid_field<'a>(
    obj: &Map<String, Value>,
    fields: &[NativeFieldCheck<'a>],
) -> Option<&'a str> {
    fields
        .iter()
        .find(|(key, check)| obj.get(*key).is_some_and(|value| !check(value)))
        .map(|(key, _)| *key)
}

fn native_fields_valid(value: &Value, fields: &[NativeFieldCheck<'_>]) -> bool {
    value
        .as_object()
        .is_some_and(|obj| first_invalid_field(obj, fields).is_none())
}

fn native_string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}

fn native_finite(value: &Value) -> bool {
    value.as_f64().is_some_and(f64::is_finite)
}

fn native_integer(value: &Value) -> bool {
    // Effect Schema.Int uses JavaScript's safe integer range, including 1.0.
    value
        .as_f64()
        .is_some_and(|n| n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0)
}

fn native_compaction(value: &Value) -> bool {
    value.as_object().is_some_and(|obj| {
        matches!(
            obj.get("type").and_then(Value::as_str),
            Some("summary" | "native")
        )
    })
}

fn native_provider_settings(value: &Value) -> bool {
    native_fields_valid(
        value,
        &[
            ("timeout", |v| v == &Value::Bool(false) || native_finite(v)),
            ("chunkTimeout", native_finite),
            ("compaction", native_compaction),
            ("transport", |v| {
                matches!(v.as_str(), Some("http" | "websocket"))
            }),
        ],
    )
}

fn native_headers(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|headers| headers.values().all(Value::is_string))
}

// Model/variant settings only constrain compaction; timeout and transport here
// are package-specific extensions, unlike provider settings.
fn native_model_settings(value: &Value) -> bool {
    native_fields_valid(value, &[("compaction", native_compaction)])
}

fn native_model_overlays(value: &Value) -> bool {
    native_fields_valid(
        value,
        &[
            ("settings", native_model_settings),
            ("headers", native_headers),
            ("body", Value::is_object),
        ],
    )
}

/// The first invalid field of a model; an empty name means it is not an object.
fn native_model_problem(value: &Value) -> Option<&'static str> {
    let Some(obj) = value.as_object() else {
        return Some("");
    };
    first_invalid_field(
        obj,
        &[
            ("modelID", Value::is_string),
            ("family", Value::is_string),
            ("name", Value::is_string),
            ("package", Value::is_string),
            ("disabled", Value::is_boolean),
            ("settings", native_model_settings),
            ("headers", native_headers),
            ("body", Value::is_object),
            ("compatibility", native_compatibility),
            ("capabilities", |v| {
                v.as_object().is_some_and(|obj| {
                    obj.get("tools").is_some_and(Value::is_boolean)
                        && obj.get("input").is_some_and(native_string_array)
                        && obj.get("output").is_some_and(native_string_array)
                })
            }),
            ("variants", |v| {
                v.as_array().is_some_and(|variants| {
                    variants.iter().all(|variant| {
                        variant.get("id").is_some_and(Value::is_string)
                            && native_model_overlays(variant)
                    })
                })
            }),
            ("cost", |v| match v.as_array() {
                Some(costs) => costs.iter().all(native_cost),
                None => native_cost(v),
            }),
            ("limit", |v| {
                native_fields_valid(
                    v,
                    &[
                        ("context", native_integer),
                        ("input", native_integer),
                        ("output", native_integer),
                    ],
                )
            }),
        ],
    )
}

fn native_compatibility(value: &Value) -> bool {
    native_fields_valid(
        value,
        &[
            ("reasoningField", Value::is_string),
            ("requireReasoning", Value::is_boolean),
            ("maxTokensField", |v| {
                matches!(v.as_str(), Some("max_completion_tokens" | "max_tokens"))
            }),
            ("requireFinishReason", Value::is_boolean),
            ("requireAssistantAfterTool", Value::is_boolean),
            ("supportsPromptCacheKey", Value::is_boolean),
        ],
    )
}

fn native_cost(value: &Value) -> bool {
    value.get("input").is_some_and(native_finite)
        && value.get("output").is_some_and(native_finite)
        && native_fields_valid(
            value,
            &[
                ("tier", |v| {
                    v.as_object().is_some_and(|obj| {
                        obj.get("type").and_then(Value::as_str) == Some("context")
                            && obj.get("size").is_some_and(native_integer)
                    })
                }),
                ("cache", |v| {
                    native_fields_valid(v, &[("read", native_finite), ("write", native_finite)])
                }),
            ],
        )
}

// Keys only a V1 declaration has, and keys only a native one has. Mirrored by
// isNativeOpencodeConfig in src/components/providers/forms/helpers/opencodeFormUtils.ts.
const LEGACY_ONLY_KEYS: [&str; 3] = ["npm", "options", "api"];
const NATIVE_ONLY_KEYS: [&str; 5] = ["package", "settings", "headers", "body", "canonical"];

/// Infer the format of a declaration whose source is unknown. Unlike validation,
/// V1-only keys decide here: a native declaration would not normally carry them.
pub fn provider_format(
    value: &Value,
    source: Option<OpenCodeConfigFormat>,
) -> OpenCodeConfigFormat {
    source.unwrap_or_else(|| {
        let has_any = |keys: &[&str]| keys.iter().any(|key| value.get(*key).is_some());
        if !has_any(&LEGACY_ONLY_KEYS) && has_any(&NATIVE_ONLY_KEYS) {
            OpenCodeConfigFormat::V2
        } else {
            OpenCodeConfigFormat::V1
        }
    })
}

/// The declaration to write for a stored provider, and its format. Settings are
/// normally the declaration itself but may hold a full config (older copies or a
/// pasted file); pick from it as the reader does, preferring a valid native entry.
pub fn provider_fragment<'a>(
    id: &str,
    settings: &'a Value,
    source: Option<OpenCodeConfigFormat>,
) -> Result<(&'a Value, OpenCodeConfigFormat), AppError> {
    let Some(obj) = settings.as_object().filter(|obj| {
        ["$schema", "provider", "providers"]
            .iter()
            .any(|key| obj.contains_key(*key))
    }) else {
        return Ok((settings, provider_format(settings, source)));
    };
    log::warn!(
        "OpenCode provider '{id}' has full config structure in settings_config, attempting to extract fragment"
    );
    let native = obj.get("providers").and_then(|providers| providers.get(id));
    let legacy = obj.get("provider").and_then(|providers| providers.get(id));
    match (native, legacy) {
        (Some(native), legacy) if legacy.is_none() || is_native_provider(native) => {
            Ok((native, OpenCodeConfigFormat::V2))
        }
        (_, Some(legacy)) => Ok((legacy, OpenCodeConfigFormat::V1)),
        _ if obj.contains_key("provider") || obj.contains_key("providers") => {
            Err(AppError::localized(
                "provider.opencode.fragment_missing",
                format!("OpenCode 配置中没有供应商「{id}」"),
                format!("OpenCode config does not contain provider '{id}'"),
            ))
        }
        _ => Ok((settings, provider_format(settings, source))),
    }
}

/// Reject a native declaration OpenCode would skip, naming the offending field.
pub fn validate_native_provider(id: &str, value: &Value) -> Result<(), AppError> {
    match native_provider_problem(value) {
        None => Ok(()),
        Some(path) if path.is_empty() => Err(AppError::localized(
            "provider.opencode.native_not_object",
            format!("OpenCode 原生供应商「{id}」必须是 JSON 对象"),
            format!("Native OpenCode provider '{id}' must be a JSON object"),
        )),
        Some(path) => Err(AppError::localized(
            "provider.opencode.native_invalid_field",
            format!("OpenCode 原生供应商「{id}」的字段 {path} 不符合 OpenCode V2 配置格式"),
            format!("Native OpenCode provider '{id}' has an invalid field: {path}"),
        )),
    }
}

/// Test convenience: writes with the format inferred from the content alone.
#[cfg(test)]
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let format = provider_format(&config, None);
    set_provider_with_format(id, config, format)
}

pub fn set_provider_with_format(
    id: &str,
    config: Value,
    format: OpenCodeConfigFormat,
) -> Result<(), AppError> {
    try_edit_config(get_opencode_config_path, |full_config| {
        let key = match format {
            OpenCodeConfigFormat::V1 => {
                if full_config
                    .get("providers")
                    .and_then(|providers| providers.get(id))
                    .is_some_and(is_native_provider)
                {
                    return Err(AppError::localized(
                        "provider.opencode.native_shadows_legacy",
                        format!("OpenCode 配置中已有原生 V2 格式的供应商「{id}」，请重新导入供应商后再编辑"),
                        format!(
                            "OpenCode provider '{id}' has a native V2 declaration. Reload providers before editing it."
                        ),
                    ));
                }
                "provider"
            }
            OpenCodeConfigFormat::V2 => {
                validate_native_provider(id, &config)?;
                "providers"
            }
        };

        // 判空要连「存在但不是对象」一起算：否则写入会静默失效——界面显示添加成功而
        // 文件里没有。provider 段是 cc-switch 的投影区，归一化不会碰用户自有的
        // model / theme 等顶层配置；providers 是原生声明，格式有误时拒绝而不是清空。
        match full_config.get(key) {
            Some(Value::Object(_)) => {}
            Some(_) if format == OpenCodeConfigFormat::V2 => {
                return Err(AppError::localized(
                    "provider.opencode.providers_not_object",
                    "OpenCode 配置中的 providers 必须是 JSON 对象",
                    "OpenCode providers must be a JSON object",
                ));
            }
            Some(_) => {
                log::warn!("OpenCode 的供应商配置格式有误，将清空原有供应商配置，再保存当前供应商");
                full_config[key] = json!({});
            }
            None => full_config[key] = json!({}),
        }
        full_config[key][id] = config;
        Ok(())
    })
    .map(|_| ())
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    edit_config(get_opencode_config_path, |config| {
        let removed_legacy = config
            .get_mut("provider")
            .and_then(Value::as_object_mut)
            .is_some_and(|providers| providers.remove(id).is_some());
        // A valid native declaration shadowed the legacy one; remove both so the
        // older one cannot resurface. An invalid one never took effect: leave it.
        if let Some(providers) = config.get_mut("providers").and_then(Value::as_object_mut) {
            if !removed_legacy || providers.get(id).is_some_and(is_native_provider) {
                providers.remove(id);
            }
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
    fn native_providers_take_precedence_independently_of_key_order() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        let legacy = r#""provider":{"shared":{"npm":"@ai-sdk/anthropic"},"legacy":{"npm":"@ai-sdk/openai"}}"#;
        let native = r#""providers":{"shared":{"settings":{"baseURL":"https://native.example"}},"builtin":{"models":{"alias":{"modelID":"upstream"}}}}"#;
        for text in [
            format!("{{{legacy},{native}}}"),
            format!("{{{native},{legacy}}}"),
        ] {
            write_config(temp.path(), &text);
            let providers = get_providers_with_format().unwrap();
            assert_eq!(providers.len(), 3);
            assert_eq!(providers["legacy"].1, OpenCodeConfigFormat::V1);
            assert_eq!(providers["builtin"].1, OpenCodeConfigFormat::V2);
            assert_eq!(
                providers["shared"].0,
                json!({"settings":{"baseURL":"https://native.example"}})
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn native_provider_write_and_remove_preserve_other_declarations() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        let original = json!({
            "model": "shared/model",
            "provider": {"shared": {"npm": "@ai-sdk/anthropic"}, "legacy": {"npm": "@ai-sdk/openai"}},
            "providers": {"shared": {}, "builtin": {"body": {"metadata": {"keep": true}}}},
            "mcp": {"servers": {"example": {"type": "remote", "url": "https://mcp.example"}}},
            "plugins": [{"package": "example", "options": {"keep": true}}]
        });
        write_config(temp.path(), &original.to_string());
        let native = json!({"models": {"model": {"variants": [
            {"id": "low", "settings": {"reasoningEffort": "low"}},
            {"id": "high", "body": {"reasoning": {"effort": "high"}}}
        ]}}});
        set_provider_with_format("shared", native.clone(), OpenCodeConfigFormat::V2).unwrap();
        let mut expected = original;
        expected["providers"]["shared"] = native;
        assert_eq!(read_opencode_config().unwrap(), expected);

        let before = std::fs::read(get_opencode_config_path().unwrap()).unwrap();
        assert!(set_provider("shared", json!({"npm": "@ai-sdk/anthropic"})).is_err());
        assert!(set_provider_with_format(
            "shared",
            json!({"settings": []}),
            OpenCodeConfigFormat::V2
        )
        .is_err());
        assert_eq!(
            std::fs::read(get_opencode_config_path().unwrap()).unwrap(),
            before
        );

        remove_provider("shared").unwrap();
        expected["providers"]
            .as_object_mut()
            .unwrap()
            .remove("shared");
        expected["provider"]
            .as_object_mut()
            .unwrap()
            .remove("shared");
        assert_eq!(read_opencode_config().unwrap(), expected);
        assert!(!get_providers().unwrap().contains_key("shared"));
    }

    #[test]
    #[serial_test::serial]
    fn malformed_native_provider_does_not_hide_legacy_or_rewrite_source() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        let original = r#"{
            "provider": {"shared": {"npm": "@ai-sdk/openai"}},
            "providers": {"shared": {"package": false}, "valid": {}}
        }"#;
        write_config(temp.path(), original);
        let providers = get_providers_with_format().unwrap();
        assert_eq!(providers["shared"].1, OpenCodeConfigFormat::V1);
        assert_eq!(providers["valid"].1, OpenCodeConfigFormat::V2);
        assert_eq!(
            std::fs::read_to_string(get_opencode_config_path().unwrap()).unwrap(),
            original
        );
        let updated = json!({"npm": "@ai-sdk/openai", "options": {"apiKey": "fake-new"}});
        set_provider_with_format("shared", updated.clone(), OpenCodeConfigFormat::V1).unwrap();
        let mut expected: Value = serde_json::from_str(original).unwrap();
        expected["provider"]["shared"] = updated;
        assert_eq!(read_opencode_config().unwrap(), expected);
    }

    #[test]
    #[serial_test::serial]
    fn malformed_native_nested_fields_fall_back_to_legacy() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        let legacy = json!({"npm": "@ai-sdk/openai", "options": {"apiKey": "fake-old"}});
        let mut invalid = vec![
            json!({"models": {"m": {"variants": {}}}}),
            json!({"settings": {"timeout": "1000"}}),
            json!({"settings": {"timeout": true}}),
            json!({"settings": {"timeout": null}}),
            json!({"settings": {"chunkTimeout": false}}),
            json!({"settings": {"compaction": {}}}),
            json!({"settings": {"compaction": {"type": "unknown"}}}),
            json!({"settings": {"transport": "sse"}}),
            json!({"headers": {"X-Tenant": 1}}),
            json!({"env": [1]}),
        ];
        for model in [
            json!(null),
            json!({"modelID": false}),
            json!({"family": []}),
            json!({"name": null}),
            json!({"package": {}}),
            json!({"disabled": "false"}),
            json!({"settings": null}),
            json!({"settings": {"compaction": "native"}}),
            json!({"headers": {"X-Tenant": false}}),
            json!({"body": []}),
            json!({"variants": [null]}),
            json!({"variants": [{}]}),
            json!({"variants": [{"id": 1}]}),
            json!({"variants": [{"id": "low", "settings": {"compaction": {"type": false}}}]}),
            json!({"variants": [{"id": "low", "headers": {"X-Tenant": null}}]}),
            json!({"variants": [{"id": "low", "body": []}]}),
            json!({"limit": []}),
            json!({"limit": {"context": "1000"}}),
            json!({"limit": {"input": 1.5}}),
            json!({"limit": {"output": null}}),
            json!({"limit": {"context": 9007199254740992_u64}}),
            json!({"capabilities": {"tools": true}}),
            json!({"capabilities": {"tools": "true", "input": [], "output": []}}),
            json!({"capabilities": {"tools": true, "input": "text", "output": []}}),
            json!({"capabilities": {"tools": true, "input": [], "output": [false]}}),
            json!({"compatibility": false}),
            json!({"compatibility": {"reasoningField": false}}),
            json!({"compatibility": {"maxTokensField": "tokens"}}),
            json!({"cost": {}}),
            json!({"cost": [{"input": 1}]}),
            json!({"cost": {"input": "1", "output": 2}}),
            json!({"cost": {"input": 1, "output": 2, "cache": {"read": false}}}),
            json!({"cost": {"input": 1, "output": 2, "cache": {"write": null}}}),
            json!({"cost": {"input": 1, "output": 2, "tier": {"type": "context"}}}),
            json!({"cost": {"input": 1, "output": 2, "tier": {"type": "other", "size": 10}}}),
            json!({"cost": {"input": 1, "output": 2, "tier": {"type": "context", "size": 1.5}}}),
        ] {
            invalid.push(json!({"models": {"m": model}}));
        }
        for key in [
            "requireReasoning",
            "requireFinishReason",
            "requireAssistantAfterTool",
            "supportsPromptCacheKey",
        ] {
            invalid.push(json!({"models": {"m": {"compatibility": {key: "true"}}}}));
        }
        for native in invalid {
            let original = json!({
                "provider": {"shared": legacy},
                "providers": {"shared": native, "native-only": native}
            });
            let source = original.to_string();
            write_config(temp.path(), &source);
            let providers = get_providers_with_format().unwrap();
            assert_eq!(providers.len(), 1, "{native}");
            assert_eq!(
                providers["shared"],
                (legacy.clone(), OpenCodeConfigFormat::V1),
                "{native}"
            );
            assert!(
                set_provider_with_format("shared", native.clone(), OpenCodeConfigFormat::V2)
                    .is_err(),
                "{native}"
            );
            assert_eq!(
                std::fs::read_to_string(get_opencode_config_path().unwrap()).unwrap(),
                source
            );
            let updated = json!({"npm": "@ai-sdk/openai", "options": {"apiKey": "fake-new"}});
            set_provider_with_format("shared", updated.clone(), OpenCodeConfigFormat::V1).unwrap();
            let mut expected = original;
            expected["provider"]["shared"] = updated;
            assert_eq!(read_opencode_config().unwrap(), expected);
        }
    }

    #[test]
    #[serial_test::serial]
    fn native_nested_fields_and_unknown_extensions_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        let native = json!({
            "name": "Native", "package": "@opencode/ai/providers/openai", "canonical": "openai",
            "env": ["TEST_API_KEY"], "extension": [null, {"keep": true}],
            "settings": {"timeout": false, "chunkTimeout": 1.5, "transport": "websocket",
                "compaction": {"type": "native", "extension": true}, "custom": {"keep": null}},
            "headers": {"X-Tenant": "example"}, "body": {"metadata": {"keep": true}},
            "models": {"m": {
                "modelID": "upstream", "family": "gpt", "name": "Model", "package": "custom",
                "disabled": false, "extension": {"keep": [1, 2]},
                "settings": {"compaction": {"type": "summary"}, "timeout": "package-specific"},
                "headers": {"X-Model": "example"}, "body": {"custom": [null]},
                "limit": {"context": 1000.0, "input": -1, "output": 0, "extension": true},
                "capabilities": {"tools": false, "input": ["text", "custom"], "output": [], "extension": []},
                "compatibility": {"reasoningField": "custom", "requireReasoning": true,
                    "maxTokensField": "max_tokens", "requireFinishReason": false,
                    "requireAssistantAfterTool": true, "supportsPromptCacheKey": false, "extension": null},
                "cost": [{"input": 1.5, "output": 2, "cache": {"read": 0.5, "extension": true},
                    "tier": {"type": "context", "size": 1000, "extension": null}, "extension": []}],
                "variants": [
                    {"id": "high", "settings": {"compaction": {"type": "native"}, "transport": "custom"},
                        "headers": {"X-Variant": "example"}, "body": {"custom": true}, "extension": null},
                    {"id": "low"}
                ]
            }}
        });
        for value in [
            native,
            json!({}),
            json!({"settings": {"timeout": 1000.5, "transport": "http"}}),
            json!({"models": {"m": {"cost": {"input": 0, "output": 0, "cache": {}}, "variants": []}}}),
            json!({"models": {"m": {"cost": [], "limit": {}, "compatibility": {"maxTokensField": "max_completion_tokens"}}}}),
        ] {
            let mut expected = json!({"provider": {"shared": {"npm": "@ai-sdk/openai"}}});
            write_config(temp.path(), &expected.to_string());
            set_provider_with_format("shared", value.clone(), OpenCodeConfigFormat::V2).unwrap();
            expected["providers"] = json!({"shared": value});
            assert_eq!(read_opencode_config().unwrap(), expected);
            assert_eq!(
                get_providers_with_format().unwrap()["shared"],
                (value, OpenCodeConfigFormat::V2)
            );
            assert!(set_provider_with_format(
                "shared",
                json!({"npm": "@ai-sdk/openai"}),
                OpenCodeConfigFormat::V1
            )
            .is_err());
            assert_eq!(read_opencode_config().unwrap(), expected);
        }
    }

    #[test]
    #[serial_test::serial]
    fn native_provider_with_v1_only_keys_still_takes_precedence() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        // OpenCode ignores keys the native schema does not know, V1-only ones included.
        let native = json!({"settings": {"apiKey": "native"}, "options": {}});
        let renamed = json!({"npm": "@ai-sdk/openai", "options": {"apiKey": "k"}, "models": {"m": {"name": "M"}}});
        let source = json!({
            "provider": {"shared": {"npm": "@ai-sdk/openai", "options": {"apiKey": "legacy"}}},
            "providers": {"shared": native, "renamed": renamed}
        })
        .to_string();
        write_config(temp.path(), &source);

        let providers = get_providers_with_format().unwrap();
        assert_eq!(
            providers["shared"],
            (native.clone(), OpenCodeConfigFormat::V2)
        );
        assert_eq!(providers["renamed"], (renamed, OpenCodeConfigFormat::V2));
        assert!(set_provider_with_format(
            "shared",
            json!({"npm": "@ai-sdk/openai", "options": {"apiKey": "edited"}}),
            OpenCodeConfigFormat::V1
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(get_opencode_config_path().unwrap()).unwrap(),
            source
        );
        // Format inference still treats V1-only keys as legacy when the source is unknown.
        assert_eq!(provider_format(&native, None), OpenCodeConfigFormat::V1);
        set_provider_with_format("shared", native.clone(), OpenCodeConfigFormat::V2).unwrap();
        assert_eq!(
            read_opencode_config().unwrap()["providers"]["shared"],
            native
        );
    }

    #[test]
    fn native_provider_problem_names_the_invalid_field() {
        for (value, path) in [
            (json!(5), Some("")),
            (json!({}), None),
            (json!({"settings": {"timeout": "1"}}), Some("settings")),
            (json!({"headers": {"X": 1}}), Some("headers")),
            (json!({"models": []}), Some("models")),
            (json!({"models": {"m": null}}), Some("models.m")),
            (
                json!({"models": {"m": {"limit": {"input": "1"}}}}),
                Some("models.m.limit"),
            ),
            (
                json!({"models": {"m": {"variants": {}}}}),
                Some("models.m.variants"),
            ),
            (
                json!({"models": {"m": {"settings": {"compaction": {}}}}}),
                Some("models.m.settings"),
            ),
        ] {
            assert_eq!(native_provider_problem(&value).as_deref(), path, "{value}");
        }
        let err = validate_native_provider("p", &json!({"models": {"m": {"cost": {}}}}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("models.m.cost"), "{err}");
    }

    #[test]
    #[serial_test::serial]
    fn removing_a_legacy_provider_leaves_an_invalid_native_declaration() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = TestHomeGuard::set(temp.path());
        write_config(
            temp.path(),
            r#"{"provider":{"shared":{"npm":"@ai-sdk/openai"}},"providers":{"shared":{"package":false},"orphan":{"package":false}}}"#,
        );
        remove_provider("shared").unwrap();
        remove_provider("orphan").unwrap();
        assert_eq!(
            read_opencode_config().unwrap(),
            json!({"provider": {}, "providers": {"shared": {"package": false}}})
        );
    }

    #[test]
    fn provider_fragment_picks_from_a_full_config_like_the_reader() {
        let legacy = json!({"npm": "@ai-sdk/openai"});
        let full = |native: Value| json!({"provider": {"shared": legacy}, "providers": {"shared": native}});
        assert_eq!(
            provider_fragment("shared", &full(json!({"package": false})), None).unwrap(),
            (&legacy, OpenCodeConfigFormat::V1)
        );
        assert_eq!(
            provider_fragment("shared", &full(json!({})), None).unwrap(),
            (&json!({}), OpenCodeConfigFormat::V2)
        );
        // Without a legacy entry an invalid native one is still chosen, so that
        // validation reports its field instead of an unrelated fallback.
        assert_eq!(
            provider_fragment(
                "shared",
                &json!({"providers": {"shared": {"package": false}}}),
                None
            )
            .unwrap()
            .1,
            OpenCodeConfigFormat::V2
        );
        assert!(provider_fragment("other", &full(json!({})), None).is_err());
        assert_eq!(
            provider_fragment("shared", &json!({}), Some(OpenCodeConfigFormat::V2)).unwrap(),
            (&json!({}), OpenCodeConfigFormat::V2)
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
