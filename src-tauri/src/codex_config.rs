use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{
    atomic_write, delete_file, get_home_dir, read_json_file, sanitize_provider_name,
    write_json_file, write_text_file,
};
use crate::error::AppError;
use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use toml_edit::{DocumentMut, Item, TableLike};

pub const CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "custom";
pub const CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME: &str = "cc-switch-model-catalog.json";
const CODEX_MODEL_CATALOG_TEMPLATE_SLUG: &str = "gpt-5.5";
const CODEX_OPENAI_MODEL_PROVIDER_ID: &str = "openai";

/// Reserved built-in provider IDs from OpenAI Codex's config/model-provider
/// catalog. Keep in sync with Codex `RESERVED_MODEL_PROVIDER_IDS` and legacy
/// removed provider aliases.
const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "openai",
    "ollama",
    "lmstudio",
    "oss",
    "ollama-chat",
];

/// 获取 Codex 配置目录路径
pub fn get_codex_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_codex_override_dir() {
        return custom;
    }

    get_home_dir().join(".codex")
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// 获取 Codex config.toml 路径
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

pub fn get_codex_model_catalog_path() -> PathBuf {
    get_codex_config_dir().join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
}

/// 获取 Codex 供应商配置文件路径
#[allow(dead_code)]
pub fn get_codex_provider_paths(
    provider_id: &str,
    provider_name: Option<&str>,
) -> (PathBuf, PathBuf) {
    let base_name = provider_name
        .map(sanitize_provider_name)
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    let auth_path = get_codex_config_dir().join(format!("auth-{base_name}.json"));
    let config_path = get_codex_config_dir().join(format!("config-{base_name}.toml"));

    (auth_path, config_path)
}

/// 删除 Codex 供应商配置文件
#[allow(dead_code)]
pub fn delete_codex_provider_config(
    provider_id: &str,
    provider_name: &str,
) -> Result<(), AppError> {
    let (auth_path, config_path) = get_codex_provider_paths(provider_id, Some(provider_name));

    delete_file(&auth_path).ok();
    delete_file(&config_path).ok();

    Ok(())
}

/// 原子写 Codex 的 `auth.json` 与 `config.toml`，在第二步失败时回滚第一步
pub fn write_codex_live_atomic(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let config_path = get_codex_config_path();

    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 读取旧内容用于回滚
    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|e| AppError::io(&auth_path, e))?)
    } else {
        None
    };
    let _old_config = if config_path.exists() {
        Some(fs::read(&config_path).map_err(|e| AppError::io(&config_path, e))?)
    } else {
        None
    };

    // 准备写入内容
    let cfg_text = match config_text_opt {
        Some(s) => s.to_string(),
        None => String::new(),
    };
    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    // 第一步：写 auth.json
    write_json_file(&auth_path, auth)?;

    // 第二步：写 config.toml（失败则回滚 auth.json）
    if let Err(e) = write_text_file(&config_path, &cfg_text) {
        // 回滚 auth.json
        if let Some(bytes) = old_auth {
            let _ = atomic_write(&auth_path, &bytes);
        } else {
            let _ = delete_file(&auth_path);
        }
        return Err(e);
    }

    Ok(())
}

/// 读取 `~/.codex/config.toml`，若不存在返回空字符串
pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

/// 对非空的 TOML 文本进行语法校验
pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| AppError::toml(Path::new("config.toml"), e))
}

/// 读取并校验 `~/.codex/config.toml`，返回文本（可能为空）
pub fn read_and_validate_codex_config_text() -> Result<String, AppError> {
    let s = read_codex_config_text()?;
    validate_config_toml(&s)?;
    Ok(s)
}

fn active_codex_model_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub(crate) fn is_custom_codex_model_provider_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && !CODEX_RESERVED_MODEL_PROVIDER_IDS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(id))
}

/// Write only Codex `config.toml` for provider switching.
///
/// Codex login state lives in `auth.json`; provider routing, endpoint, model,
/// and provider-scoped bearer tokens live in `config.toml`. Provider switches
/// should not overwrite the user's ChatGPT login cache.
pub fn write_codex_live_config_atomic(config_text_opt: Option<&str>) -> Result<(), AppError> {
    let config_path = get_codex_config_path();
    let cfg_text = match config_text_opt {
        Some(config_text) => config_text.to_string(),
        None => String::new(),
    };

    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    write_text_file(&config_path, &cfg_text)
}

pub fn extract_codex_auth_api_key(auth: &Value) -> Option<String> {
    auth.get("OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

pub fn extract_codex_api_key(auth: Option<&Value>, config_text: Option<&str>) -> Option<String> {
    auth.and_then(extract_codex_auth_api_key)
        .or_else(|| config_text.and_then(extract_codex_experimental_bearer_token))
}

/// Extract the upstream base URL from a Codex `config.toml` string.
///
/// Prefers the active `[model_providers.<model_provider>].base_url`, falling
/// back to a top-level `base_url` when no model provider is selected.
pub fn extract_codex_base_url(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;

    let active_provider = doc
        .get("model_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty());

    if active_provider
        .is_none_or(|provider| provider.eq_ignore_ascii_case(CODEX_OPENAI_MODEL_PROVIDER_ID))
    {
        if let Some(base_url) = doc.get("openai_base_url").and_then(|v| v.as_str()) {
            return Some(base_url.to_string());
        }
    }

    if let Some(active_provider) = active_provider {
        if let Some(base_url) = doc
            .get("model_providers")
            .and_then(|providers| providers.get(active_provider))
            .and_then(|provider| provider.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(base_url.to_string());
        }
    }

    doc.get("base_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub fn codex_auth_has_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    obj.iter().any(|(key, value)| {
        if key == "auth_mode" {
            return false;
        }

        if key == "OPENAI_API_KEY" {
            return value
                .as_str()
                .map(str::trim)
                .is_some_and(|token| !token.is_empty());
        }

        match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        }
    })
}

pub fn codex_auth_has_oauth_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    obj.iter().any(|(key, value)| {
        if key == "auth_mode" || key == "OPENAI_API_KEY" {
            return false;
        }

        match value {
            Value::Null => false,
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => !map.is_empty(),
            _ => true,
        }
    })
}

pub fn should_restore_codex_provider_token_for_backfill(
    category: Option<&str>,
    template_settings: &Value,
) -> bool {
    if category == Some("official") {
        return false;
    }

    let Some(auth) = template_settings.get("auth") else {
        return true;
    };

    let has_provider_api_key = extract_codex_auth_api_key(auth).is_some();
    let has_oauth_login = codex_auth_has_oauth_login_material(auth);
    !has_oauth_login || has_provider_api_key
}

fn parse_codex_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(n)) => n.as_u64().filter(|v| *v > 0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok().filter(|v| *v > 0),
        _ => None,
    }
}

fn extract_codex_top_level_u64(config_text: &str, field: &str) -> Option<u64> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get(field)
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
}

/// 判断模型是否只能按文本模型写入 Codex catalog。
///
/// Spark 和 DeepSeek V4 兼容 Responses 文本工具调用，但 Codex 会根据
/// `input_modalities` 里的 `image` 自动注入 hosted `image_generation` 工具；
/// 这些模型不支持该工具，所以生成 catalog 时必须覆盖模板里的图片模态。
fn codex_catalog_model_name_is_text_only(model: &str) -> bool {
    let normalized = model
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>();

    normalized == "gpt53codexspark" || normalized.starts_with("deepseekv4")
}

/// 判断生成 catalog 时是否保留 OpenAI 官方 GPT 的速度/服务档。
///
/// 第三方和本地模型不应该继承 GPT 官方的 priority/fast 展示项，否则 UI 会暗示
/// 上游支持 Codex 官方服务档；但 OpenAI 官方 GPT 模型需要保留这些字段，避免
/// router catalog 吃掉 Codex 原生的速度选择。
fn codex_catalog_model_preserves_openai_service_tiers(model: &str) -> bool {
    let lower = model.trim().to_ascii_lowercase();
    matches!(lower.as_str(), "gpt-5.5" | "gpt-5.4")
}

/// 从 `codexRouting.routes` 中读取指定模型的能力声明。
///
/// route 能力优先于历史模型名兜底；这样用户新增任意上游时，可以通过 UI 声明 text-only /
/// image capability，而不需要把模型名写死进后端。
fn codex_routing_capabilities_for_model<'a>(settings: &'a Value, model: &str) -> Option<&'a Value> {
    let routing = settings.get("codexRouting")?;
    if routing
        .get("enabled")
        .and_then(|value| value.as_bool())
        .is_some_and(|enabled| !enabled)
    {
        return None;
    }

    let routes = routing.get("routes").and_then(|value| value.as_array())?;
    routes
        .iter()
        .find(|route| codex_catalog_route_matches_model(route, model))
        .or_else(|| {
            routing
                .get("defaultRouteId")
                .or_else(|| routing.get("default_route_id"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .and_then(|default_id| {
                    routes.iter().find(|route| {
                        route
                            .get("id")
                            .and_then(|value| value.as_str())
                            .is_some_and(|id| id.eq_ignore_ascii_case(default_id))
                    })
                })
        })
        .and_then(|route| route.get("capabilities"))
}

/// 判断 catalog 里的模型是否命中 route 的 model/prefix 匹配规则。
fn codex_catalog_route_matches_model(route: &Value, model: &str) -> bool {
    if route
        .get("enabled")
        .and_then(|value| value.as_bool())
        .is_some_and(|enabled| !enabled)
    {
        return false;
    }

    let match_config = route.get("match").unwrap_or(route);
    if match_config
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .any(|candidate| candidate.trim().eq_ignore_ascii_case(model))
    {
        return true;
    }

    let lower_model = model.to_ascii_lowercase();
    match_config
        .get("prefixes")
        .or_else(|| match_config.get("modelPrefixes"))
        .or_else(|| match_config.get("model_prefixes"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
        .any(|prefix| lower_model.starts_with(&prefix.to_ascii_lowercase()))
}

/// 根据 route capability 判断 catalog 是否应写成 text-only。
fn codex_catalog_capabilities_are_text_only(capabilities: &Value) -> Option<bool> {
    if let Some(text_only) = capabilities
        .get("textOnly")
        .or_else(|| capabilities.get("text_only"))
        .and_then(|value| value.as_bool())
    {
        return Some(text_only);
    }

    capabilities
        .get("inputModalities")
        .or_else(|| capabilities.get("input_modalities"))
        .and_then(|value| value.as_array())
        .map(|modalities| {
            !modalities
                .iter()
                .filter_map(|value| value.as_str())
                .any(|modality| modality.eq_ignore_ascii_case("image"))
        })
}

fn codex_catalog_model_entry(
    template: &Value,
    spec: &CodexCatalogModelSpec,
    priority: usize,
) -> Value {
    let mut entry = template.clone();
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    entry_obj.insert("slug".to_string(), json!(spec.model));
    entry_obj.insert("display_name".to_string(), json!(spec.display_name));
    entry_obj.insert("description".to_string(), json!(spec.display_name));
    entry_obj.insert("context_window".to_string(), json!(spec.context_window));
    entry_obj.insert("max_context_window".to_string(), json!(spec.context_window));
    entry_obj.insert("priority".to_string(), json!(1000 + priority));
    if !codex_catalog_model_preserves_openai_service_tiers(&spec.model) {
        entry_obj.insert("additional_speed_tiers".to_string(), json!([]));
        entry_obj.insert("service_tiers".to_string(), json!([]));
    }
    entry_obj.insert("availability_nux".to_string(), Value::Null);
    entry_obj.insert("upgrade".to_string(), Value::Null);
    if spec.text_only {
        entry_obj.insert("input_modalities".to_string(), json!(["text"]));
        entry_obj.insert("supports_image_detail_original".to_string(), json!(false));
        entry_obj.insert("web_search_tool_type".to_string(), json!("text"));
    }

    entry
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCatalogModelSpec {
    model: String,
    display_name: String,
    context_window: u64,
    text_only: bool,
}

fn codex_catalog_model_specs(settings: &Value, config_text: &str) -> Vec<CodexCatalogModelSpec> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
    else {
        return Vec::new();
    };

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);
    let mut seen = std::collections::HashSet::new();
    let mut specs = Vec::new();

    for model_config in models {
        let Some(model) = model_config
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };

        if !seen.insert(model.to_string()) {
            continue;
        }

        let display_name = model_config
            .get("displayName")
            .or_else(|| model_config.get("display_name"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(model);
        let context_window = parse_codex_positive_u64(
            model_config
                .get("contextWindow")
                .or_else(|| model_config.get("context_window")),
        )
        .unwrap_or(default_context_window);

        let text_only = codex_routing_capabilities_for_model(settings, model)
            .and_then(codex_catalog_capabilities_are_text_only)
            .unwrap_or_else(|| codex_catalog_model_name_is_text_only(model));

        specs.push(CodexCatalogModelSpec {
            model: model.to_string(),
            display_name: display_name.to_string(),
            context_window,
            text_only,
        });
    }

    specs
}

fn find_codex_model_template(catalog: &Value) -> Option<Value> {
    catalog
        .get("models")
        .and_then(|models| models.as_array())
        .and_then(|models| {
            models.iter().find(|model| {
                model.get("slug").and_then(|slug| slug.as_str())
                    == Some(CODEX_MODEL_CATALOG_TEMPLATE_SLUG)
            })
        })
        .cloned()
}

fn load_codex_model_template_from_cache() -> Result<Option<Value>, AppError> {
    let path = get_codex_config_dir().join("models_cache.json");
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let catalog: Value = serde_json::from_str(&text).map_err(|e| AppError::json(&path, e))?;
    Ok(find_codex_model_template(&catalog))
}

/// Fixed candidates for locating the `codex` CLI when it is not on the process
/// PATH (common in GUI apps launched outside a terminal).
const CODEX_CLI_FIXED_CANDIDATES: &[&str] = &[
    "codex",                                // PATH (all platforms)
    "/opt/homebrew/bin/codex",              // macOS Apple Silicon Homebrew
    "/usr/local/bin/codex",                 // macOS Intel Homebrew / Linux
    "/home/linuxbrew/.linuxbrew/bin/codex", // Linux Homebrew
];

fn push_codex_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    candidate: PathBuf,
) {
    let key = candidate.to_string_lossy().into_owned();
    if seen.insert(key) {
        candidates.push(candidate);
    }
}

fn push_existing_codex_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    candidate: PathBuf,
) {
    if candidate.exists() {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

fn push_codex_cli_candidates_from_version_dirs(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    versions_dir: PathBuf,
    suffix: &[&str],
) {
    let Ok(entries) = fs::read_dir(versions_dir) else {
        return;
    };

    let mut discovered = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let mut candidate = entry.path();
            for component in suffix {
                candidate.push(component);
            }
            candidate
        })
        .filter(|candidate| candidate.exists())
        .collect::<Vec<_>>();

    // Prefer newer-looking version directories before older global installs.
    discovered.sort_by(|a, b| b.cmp(a));
    for candidate in discovered {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

fn push_home_codex_cli_candidates(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    home: &Path,
) {
    for relative in [
        ".nvm/current/bin/codex",
        ".volta/bin/codex",
        ".asdf/shims/codex",
        ".local/share/mise/shims/codex",
        ".config/mise/shims/codex",
        ".local/bin/codex",
        ".npm-global/bin/codex",
        ".npm-packages/bin/codex",
        ".local/share/pnpm/codex",
        "Library/pnpm/codex",
    ] {
        push_existing_codex_cli_candidate(candidates, seen, home.join(relative));
    }

    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join(".nvm/versions/node"),
        &["bin", "codex"],
    );
    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join(".local/share/fnm/node-versions"),
        &["installation", "bin", "codex"],
    );
    push_codex_cli_candidates_from_version_dirs(
        candidates,
        seen,
        home.join("Library/Application Support/fnm/node-versions"),
        &["installation", "bin", "codex"],
    );
}

fn push_env_codex_cli_candidates(candidates: &mut Vec<PathBuf>, seen: &mut HashSet<String>) {
    for (env_key, suffix) in [
        ("NPM_CONFIG_PREFIX", &["bin", "codex"][..]),
        ("VOLTA_HOME", &["bin", "codex"][..]),
        ("ASDF_DATA_DIR", &["shims", "codex"][..]),
        ("MISE_DATA_DIR", &["shims", "codex"][..]),
        ("PNPM_HOME", &["codex"][..]),
    ] {
        let Some(prefix) = std::env::var_os(env_key) else {
            continue;
        };
        let mut candidate = PathBuf::from(prefix);
        for component in suffix {
            candidate.push(component);
        }
        push_existing_codex_cli_candidate(candidates, seen, candidate);
    }

    if let Some(nvm_dir) = std::env::var_os("NVM_DIR") {
        push_codex_cli_candidates_from_version_dirs(
            candidates,
            seen,
            PathBuf::from(nvm_dir).join("versions/node"),
            &["bin", "codex"],
        );
    }

    if let Some(fnm_dir) = std::env::var_os("FNM_DIR") {
        push_codex_cli_candidates_from_version_dirs(
            candidates,
            seen,
            PathBuf::from(fnm_dir).join("node-versions"),
            &["installation", "bin", "codex"],
        );
    }

    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let npm_dir = PathBuf::from(appdata).join("npm");
            for name in ["codex.cmd", "codex.exe", "codex"] {
                push_existing_codex_cli_candidate(candidates, seen, npm_dir.join(name));
            }
        }
    }
}

fn codex_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for candidate in CODEX_CLI_FIXED_CANDIDATES {
        push_codex_cli_candidate(&mut candidates, &mut seen, PathBuf::from(candidate));
    }

    push_env_codex_cli_candidates(&mut candidates, &mut seen);
    push_home_codex_cli_candidates(&mut candidates, &mut seen, &get_home_dir());

    candidates
}

fn load_codex_model_template_from_bundled() -> Result<Option<Value>, AppError> {
    for candidate in codex_cli_candidates() {
        let candidate_label = candidate.to_string_lossy();
        let output = match Command::new(&candidate)
            .args(["debug", "models", "--bundled"])
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                log::debug!("failed to run `{candidate_label} debug models --bundled`: {err}");
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::debug!("`{candidate_label} debug models --bundled` failed: {stderr}");
            continue;
        }

        let catalog: Value = match serde_json::from_slice(&output.stdout) {
            Ok(catalog) => catalog,
            Err(e) => {
                log::debug!(
                    "Failed to parse `{candidate_label} debug models --bundled` output: {e}"
                );
                continue;
            }
        };
        if let Some(template) = find_codex_model_template(&catalog) {
            return Ok(Some(template));
        }
    }

    Ok(None)
}

fn load_codex_model_template_static() -> Option<Value> {
    let text = include_str!("resources/gpt5_5_template.json");
    match serde_json::from_str(text) {
        Ok(template) => Some(template),
        Err(e) => {
            log::warn!("Failed to parse bundled gpt-5.5 template: {e}");
            None
        }
    }
}

fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    // ① models_cache.json (created by Codex when it connects to OpenAI)
    if let Some(template) = load_codex_model_template_from_cache()? {
        return Ok(template);
    }
    // ② codex CLI (PATH + platform-specific common paths)
    if let Some(template) = load_codex_model_template_from_bundled()? {
        return Ok(template);
    }
    // ③ Static fallback bundled at compile time
    if let Some(template) = load_codex_model_template_static() {
        return Ok(template);
    }

    Err(AppError::Message(format!(
        "Codex model catalog template `{CODEX_MODEL_CATALOG_TEMPLATE_SLUG}` not found. Please start Codex once so models_cache.json is available, or ensure the `codex` CLI is on PATH."
    )))
}

fn codex_model_catalog_from_specs(specs: &[CodexCatalogModelSpec], template: &Value) -> Value {
    let entries: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| codex_catalog_model_entry(template, spec, index))
        .collect();

    json!({ "models": entries })
}

fn codex_model_catalog_from_settings(
    settings: &Value,
    config_text: &str,
) -> Result<Option<Value>, AppError> {
    let specs = codex_catalog_model_specs(settings, config_text);
    if specs.is_empty() {
        return Ok(None);
    }

    let template = load_codex_model_catalog_template()?;
    Ok(Some(codex_model_catalog_from_specs(&specs, &template)))
}

fn set_codex_model_catalog_json_field(
    config_text: &str,
    catalog_path: Option<&Path>,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    match catalog_path {
        Some(_) => {
            doc["model_catalog_json"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        }
        None => {
            let should_remove = doc
                .get("model_catalog_json")
                .and_then(|item| item.as_str())
                .map(codex_model_catalog_path_is_cc_switch_owned)
                .unwrap_or(false);
            if should_remove {
                doc.as_table_mut().remove("model_catalog_json");
            }
        }
    }

    Ok(doc.to_string())
}

fn codex_model_catalog_path_is_cc_switch_owned(path: &str) -> bool {
    Path::new(path).file_name().and_then(|name| name.to_str())
        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
}

/// Generate Codex `model_catalog_json` from provider settings and inject/remove
/// the top-level TOML field that points Codex to the generated file.
pub fn prepare_codex_config_text_with_model_catalog(
    settings: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    let catalog_path = get_codex_model_catalog_path();

    if let Some(catalog) = codex_model_catalog_from_settings(settings, config_text)? {
        let config_text = set_codex_model_catalog_json_field(config_text, Some(&catalog_path))?;
        write_json_file(&catalog_path, &catalog)?;
        Ok(config_text)
    } else {
        set_codex_model_catalog_json_field(config_text, None)
    }
}

/// Reverse of `prepare_codex_config_text_with_model_catalog`: read the
/// cc-switch–maintained catalog file referenced by `~/.codex/config.toml` and
/// convert it back into the simplified shape the frontend table uses:
/// `{ "models": [{ "model", "displayName"?, "contextWindow"? }, ...] }`.
///
/// We only reverse-parse catalogs whose `model_catalog_json` path is the
/// cc-switch–generated file (identified by filename
/// `cc-switch-model-catalog.json`). A user-managed external catalog file is
/// left alone — surfacing its richer structure as the simplified table would
/// be a downgrade we can't safely round-trip.
///
/// `displayName` and `contextWindow` are omitted from the returned entry when
/// the on-disk value matches the fallback that
/// `codex_model_catalog_from_settings` injects for unset inputs (slug for
/// display_name, `model_context_window` or 128_000 for context_window). This
/// preserves the "user left it blank" intent across round-trip; an unavoidable
/// edge case is that a user-typed value that happens to equal the fallback
/// will also collapse to blank, but the next save writes the same fallback so
/// behavior stays consistent.
///
/// All failure modes (missing file, parse error, no `model_catalog_json`,
/// entries without `slug`) collapse to `Ok(None)` so callers can treat this
/// as best-effort enrichment without making `read_live_settings` brittle.
pub fn read_codex_model_catalog_simplified_from_live() -> Result<Option<Value>, AppError> {
    let config_text = read_codex_config_text()?;
    let generated_path = get_codex_model_catalog_path();
    let Some(catalog_path) = resolve_cc_switch_catalog_path(&config_text, &generated_path) else {
        return Ok(None);
    };
    if !catalog_path.exists() {
        return Ok(None);
    }
    let Ok(catalog_text) = fs::read_to_string(&catalog_path) else {
        return Ok(None);
    };
    Ok(build_simplified_catalog_from_texts(
        &config_text,
        &catalog_text,
    ))
}

/// Given `config.toml` text, resolve the on-disk path of the cc-switch–owned
/// catalog file (returns `None` if `model_catalog_json` is absent or points at
/// a file we don't own). Relative paths fall back to `generated_path`.
pub(crate) fn resolve_cc_switch_catalog_path(
    config_text: &str,
    generated_path: &Path,
) -> Option<PathBuf> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let catalog_path_str = doc
        .get("model_catalog_json")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    let referenced_path = Path::new(catalog_path_str);
    let is_cc_switch_owned = referenced_path.file_name().and_then(|name| name.to_str())
        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    if !is_cc_switch_owned {
        return None;
    }

    if referenced_path.is_absolute() {
        Some(referenced_path.to_path_buf())
    } else {
        Some(generated_path.to_path_buf())
    }
}

/// Pure reverse-parsing core: convert Codex catalog JSON text back into the
/// frontend's simplified `{ models: [{ model, displayName?, contextWindow? }] }`
/// shape. Returns `None` when the catalog is unparseable, has no `models`
/// array, or yields zero valid entries.
fn build_simplified_catalog_from_texts(config_text: &str, catalog_text: &str) -> Option<Value> {
    let catalog: Value = serde_json::from_str(catalog_text).ok()?;
    let models = catalog.get("models").and_then(|m| m.as_array())?;

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);

    let mut entries = Vec::with_capacity(models.len());
    for entry in models {
        let Some(model) = entry
            .get("slug")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };

        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), json!(model));

        if let Some(display_name) = entry
            .get("display_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != model)
        {
            obj.insert("displayName".to_string(), json!(display_name));
        }

        if let Some(context_window) = entry
            .get("context_window")
            .and_then(|v| v.as_u64())
            .filter(|v| *v > 0 && *v != default_context_window)
        {
            obj.insert("contextWindow".to_string(), json!(context_window));
        }

        entries.push(Value::Object(obj));
    }

    if entries.is_empty() {
        return None;
    }

    Some(json!({ "models": entries }))
}

/// Decide the `config.toml` text to write during a takeover-off restore,
/// projecting the model catalog **only when `settings` carries an inline
/// `modelCatalog`**.
///
/// Restore feeds back a stored backup, and Codex backups come in two shapes that
/// need opposite handling:
///
/// - **Snapshot backup** (`read_codex_live_settings`): `{ auth, config }` with no
///   inline `modelCatalog`. Its `config.toml` text already carries whatever
///   `model_catalog_json` pointer existed at backup time, and the generated
///   catalog file on disk is untouched. Here we must keep the config **raw** —
///   running catalog projection would see "no specs" and strip the live pointer.
/// - **Provider-rebuilt backup** (`update_live_backup_from_provider`): the DB
///   provider's settings, i.e. `{ auth, config (no pointer), modelCatalog
///   (inline DB SSOT) }`. Here the pointer/catalog file must be (re)generated
///   from the inline `modelCatalog`, or the mapping is lost on restore.
///
/// Gating on the presence of the inline `modelCatalog` key routes each shape
/// correctly; an empty inline catalog still projects (and so correctly drops a
/// now-stale pointer), while an absent key leaves the text untouched. This is
/// **orthogonal to auth** — a provider-rebuilt backup can pair an inline
/// `modelCatalog` with empty `auth.json` (the API key living in the config's
/// `experimental_bearer_token`), so the caller must decide config projection
/// independently of whether it writes or deletes `auth.json`.
pub fn prepare_codex_live_config_text_with_optional_catalog(
    settings: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    if settings.get("modelCatalog").is_some() {
        prepare_codex_config_text_with_model_catalog(settings, config_text)
    } else {
        Ok(config_text.to_string())
    }
}

/// 判断 TOML 节点是否是表结构。
///
/// provider 切换时，顶层标量（model / model_provider / catalog 指针等）
/// 属于当前 provider；而 `[features]`、`[desktop]`、`[memories]`、
/// `[projects]`、`[mcp_servers]` 等表结构属于用户全局配置，不能被历史
/// provider 快照覆盖。
fn codex_toml_item_is_table_like(item: &toml_edit::Item) -> bool {
    item.as_table().is_some() || item.as_array_of_tables().is_some()
}

/// 将 provider 表结构里 live 缺失的子项补进 live 配置，已有项一律保留 live。
///
/// 这用于兼容 CC Switch 的 common config snippet：snippet 可能给 Codex 增加
/// `[mcp_servers.*]` 等表段；但如果用户 live 配置已经有同名项，历史 provider
/// 快照不能覆盖用户当前值。
fn merge_missing_codex_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            merge_missing_codex_toml_table_like(target_table, source_table);
            return;
        }
    }

    if target.is_none() {
        *target = source.clone();
    }
}

/// 递归补齐 TOML 表中缺失的键，冲突时保留 target。
fn merge_missing_codex_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_missing_codex_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

// 只移除 CC Switch 自己生成的模型目录指针，避免误删用户手写的 catalog。
fn remove_cc_switch_model_catalog_json_if_stale(doc: &mut DocumentMut) {
    let should_remove = doc
        .get("model_catalog_json")
        .and_then(|item| item.as_str())
        .map(codex_model_catalog_path_is_cc_switch_owned)
        .unwrap_or(false);
    if should_remove {
        doc.as_table_mut().remove("model_catalog_json");
    }
}

// 退出官方兜底时清掉当前自定义 provider 表，避免旧 router 的本地 base_url 残留。
fn remove_active_custom_codex_model_provider_section(doc: &mut DocumentMut) {
    let Some(provider_id) = active_codex_model_provider_id(doc) else {
        return;
    };
    if !is_custom_codex_model_provider_id(&provider_id) {
        return;
    }

    let should_remove_container = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_like_mut())
        .map(|table| {
            table.remove(&provider_id);
            table.is_empty()
        })
        .unwrap_or(false);

    if should_remove_container {
        doc.as_table_mut().remove("model_providers");
    }
}

// provider 未声明的私有字段不能沿用 live 里的旧值，否则 official 会残留 router。
fn remove_codex_provider_owned_fields_missing_from_provider(
    live_doc: &mut DocumentMut,
    provider_doc: &DocumentMut,
) {
    if provider_doc.get("model_provider").is_none() {
        remove_active_custom_codex_model_provider_section(live_doc);
    }

    for key in ["model", "model_provider", "model_context_window"] {
        if provider_doc.get(key).is_none() {
            live_doc.as_table_mut().remove(key);
        }
    }

    if provider_doc.get("openai_base_url").is_none() {
        live_doc.as_table_mut().remove("openai_base_url");
    }

    if provider_doc.get("model_catalog_json").is_none() {
        remove_cc_switch_model_catalog_json_if_stale(live_doc);
    }

    if provider_doc.get("experimental_bearer_token").is_none() {
        live_doc.as_table_mut().remove("experimental_bearer_token");
    }
}

// 空 official provider 配置表示回到 Codex 默认 provider，同时保留用户全局配置。
fn strip_codex_provider_owned_fields_from_live(live_config_text: &str) -> Result<String, AppError> {
    if live_config_text.trim().is_empty() {
        return Ok(String::new());
    }

    let mut live_doc = live_config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid live Codex config.toml: {e}")))?;

    remove_active_custom_codex_model_provider_section(&mut live_doc);
    for key in [
        "model",
        "model_provider",
        "model_context_window",
        "openai_base_url",
        "experimental_bearer_token",
    ] {
        live_doc.as_table_mut().remove(key);
    }
    remove_cc_switch_model_catalog_json_if_stale(&mut live_doc);

    Ok(live_doc.to_string())
}

/// 将待切换 provider 的 Codex 配置叠加到当前 live `config.toml`。
///
/// CC Switch 的 provider 记录只应该负责 provider 相关的字段；如果直接把
/// DB 中保存的 `config` 原样写回 `~/.codex/config.toml`，会清空用户后来新增
/// 的 memories、desktop、projects、MCP 和插件等配置，导致切换模型后 Codex
/// 行为突然退回旧状态。这里以 live 配置为底，叠加 provider 顶层标量和当前
/// provider 的 `[model_providers.<id>]` 表，从而既完成模型切换，又保留用户配置。
pub(crate) fn merge_codex_provider_config_texts(
    live_config_text: &str,
    provider_config_text: &str,
) -> Result<String, AppError> {
    if provider_config_text.trim().is_empty() {
        return strip_codex_provider_owned_fields_from_live(live_config_text);
    }

    if live_config_text.trim().is_empty() {
        return Ok(provider_config_text.to_string());
    }

    let mut live_doc = live_config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid live Codex config.toml: {e}")))?;
    let provider_doc = provider_config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid provider Codex config.toml: {e}")))?;

    remove_codex_provider_owned_fields_missing_from_provider(&mut live_doc, &provider_doc);

    for (key, item) in provider_doc.as_table().iter() {
        if key == "model_providers" || codex_toml_item_is_table_like(item) {
            continue;
        }
        live_doc[key] = item.clone();
    }

    for (key, item) in provider_doc.as_table().iter() {
        if key == "model_providers" || !codex_toml_item_is_table_like(item) {
            continue;
        }

        match live_doc.as_table_mut().get_mut(key) {
            Some(live_item) => merge_missing_codex_toml_item(live_item, item),
            None => {
                live_doc.as_table_mut().insert(key, item.clone());
            }
        }
    }

    let provider_id = active_codex_model_provider_id(&provider_doc);
    if let Some(provider_id) = provider_id.as_deref() {
        if let Some(provider_item) = provider_doc
            .get("model_providers")
            .and_then(|item| item.as_table())
            .and_then(|table| table.get(provider_id))
            .cloned()
        {
            if live_doc.get("model_providers").is_none() {
                live_doc["model_providers"] = toml_edit::table();
            }
            if let Some(live_providers) = live_doc
                .get_mut("model_providers")
                .and_then(|item| item.as_table_mut())
            {
                live_providers.insert(provider_id, provider_item);
            }
        }
    }

    Ok(live_doc.to_string())
}

/// 读取当前 live 配置，并把 provider 配置叠加进去。
pub(crate) fn merge_codex_provider_config_with_live(config_text: &str) -> Result<String, AppError> {
    let live_config = read_codex_config_text()?;
    merge_codex_provider_config_texts(&live_config, config_text)
}

pub fn write_codex_provider_live_with_catalog(
    settings: &Value,
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    let prepared_config = config_text
        .map(|text| prepare_codex_config_text_with_model_catalog(settings, text))
        .transpose()?;
    write_codex_live_for_provider(category, auth, prepared_config.as_deref())
}

/// Extract a provider-scoped `experimental_bearer_token` from Codex `config.toml`.
///
/// Mobile compat: third-party providers may store the API key inside
/// `[model_providers.<id>].experimental_bearer_token` while keeping the
/// user's ChatGPT login cache intact in `auth.json`. Falls back to the
/// top-level `experimental_bearer_token` when no active model provider is set.
pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String> {
    if !config_text.contains("experimental_bearer_token") {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = active_codex_model_provider_id(&doc);

    let top_level_token = || {
        doc.get("experimental_bearer_token")
            .and_then(|item| item.as_str())
    };
    let token = match provider_id.as_deref() {
        Some(id) if is_custom_codex_model_provider_id(id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table())
            .and_then(|table| table.get(id))
            .and_then(|item| item.as_table())
            .and_then(|table| table.get("experimental_bearer_token"))
            .and_then(|item| item.as_str())
            .or_else(top_level_token),
        Some(_) => top_level_token(),
        None => top_level_token(),
    };

    token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn set_codex_experimental_bearer_token(config_text: &str, token: &str) -> Result<String, AppError> {
    if config_text.trim().is_empty() {
        return Err(AppError::localized(
            "provider.codex.config.missing",
            "Codex 第三方供应商缺少 config.toml 配置，无法写入 bearer token",
            "Codex third-party provider is missing config.toml, cannot write bearer token",
        ));
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let Some(provider_id) = active_codex_model_provider_id(&doc) else {
        doc["experimental_bearer_token"] = toml_edit::value(token);
        return Ok(doc.to_string());
    };

    if !is_custom_codex_model_provider_id(&provider_id) {
        // Reserved Codex provider IDs are owned by the CLI. Keep third-party
        // bearer tokens at the top level so we do not shadow built-in tables.
        doc["experimental_bearer_token"] = toml_edit::value(token);
        return Ok(doc.to_string());
    }

    if let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_mut())
    {
        if let Some(provider_table) = model_providers
            .get_mut(provider_id.as_str())
            .and_then(|item| item.as_table_mut())
        {
            provider_table["experimental_bearer_token"] = toml_edit::value(token);
            return Ok(doc.to_string());
        }
    }

    doc["experimental_bearer_token"] = toml_edit::value(token);
    Ok(doc.to_string())
}

pub fn remove_codex_experimental_bearer_token_if(
    config_text: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<String, AppError> {
    if config_text.trim().is_empty() || !config_text.contains("experimental_bearer_token") {
        return Ok(config_text.to_string());
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if let Some(provider_id) = active_codex_model_provider_id(&doc) {
        if let Some(provider_table) = doc
            .get_mut("model_providers")
            .and_then(|item| item.as_table_mut())
            .and_then(|table| table.get_mut(provider_id.as_str()))
            .and_then(|item| item.as_table_mut())
        {
            let should_remove = provider_table
                .get("experimental_bearer_token")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .is_some_and(&predicate);
            if should_remove {
                provider_table.remove("experimental_bearer_token");
            }
        }
    }

    let should_remove_top_level = doc
        .get("experimental_bearer_token")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .is_some_and(&predicate);
    if should_remove_top_level {
        doc.as_table_mut().remove("experimental_bearer_token");
    }
    Ok(doc.to_string())
}

fn remove_codex_experimental_bearer_token(config_text: &str) -> Result<String, AppError> {
    remove_codex_experimental_bearer_token_if(config_text, |_| true)
}

/// Read the current Codex live settings as a `{ auth, config }` object.
///
/// Missing `auth.json` collapses to `{}` so a config-only third-party install
/// is still importable; both files empty is treated as "no live install".
pub fn read_codex_live_settings() -> Result<Value, AppError> {
    let auth_path = get_codex_auth_path();
    let auth_present = auth_path.exists();
    let auth: Value = if auth_present {
        read_json_file(&auth_path)?
    } else {
        json!({})
    };
    let cfg_text = read_and_validate_codex_config_text()?;
    if !auth_present && cfg_text.trim().is_empty() {
        return Err(AppError::localized(
            "codex.live.missing",
            "Codex 配置文件不存在",
            "Codex configuration is missing",
        ));
    }
    Ok(json!({ "auth": auth, "config": cfg_text }))
}

/// Route a Codex live write between full auth+config or config-only.
///
/// Official providers with usable login material own `auth.json`. Third-party
/// providers only touch `config.toml` when the compatibility setting is enabled
/// so the user's ChatGPT login cache survives provider switches.
pub fn write_codex_live_for_provider(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    let should_write_auth = (category == Some("official") && codex_auth_has_login_material(auth))
        || (category != Some("official")
            && !crate::settings::preserve_codex_official_auth_on_switch());
    let merged_config = config_text
        .map(merge_codex_provider_config_with_live)
        .transpose()?;

    if should_write_auth {
        write_codex_live_atomic(auth, merged_config.as_deref())
    } else {
        let live_config =
            prepare_codex_provider_live_config(auth, merged_config.as_deref().unwrap_or(""))?;
        write_codex_live_config_atomic(Some(&live_config))
    }
}

/// Build the live Codex config for provider switching.
///
/// The stored provider keeps its API key in `auth.OPENAI_API_KEY`. Live Codex
/// requests can use a provider-scoped `experimental_bearer_token`, so switching
/// providers only needs to update `config.toml`; `auth.json` stays as the user's
/// long-lived ChatGPT login cache.
pub fn prepare_codex_provider_live_config(
    auth: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    let token = extract_codex_auth_api_key(auth)
        .or_else(|| extract_codex_experimental_bearer_token(config_text));

    Ok(match token {
        Some(token) => set_codex_experimental_bearer_token(config_text, &token)?,
        None => config_text.to_string(),
    })
}

/// During DB backfill, lift a live `experimental_bearer_token` back into
/// `auth.OPENAI_API_KEY` so the stored provider keeps its canonical shape
/// and generated live tokens don't leak into stored provider TOML.
///
/// Only intervenes when the live config actually carries a bearer token —
/// otherwise the function is a no-op so the caller's normal backfill path
/// (which keeps live `auth` as the authoritative source) is unaffected.
pub fn restore_codex_provider_token_for_backfill(
    settings: &mut Value,
    template_settings: &Value,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };

    let Some(token) = extract_codex_experimental_bearer_token(&config_text) else {
        return Ok(());
    };

    let cleaned_config = remove_codex_experimental_bearer_token(&config_text)?;

    if let Some(obj) = settings.as_object_mut() {
        obj.insert("config".to_string(), Value::String(cleaned_config));

        let mut auth = template_settings
            .get("auth")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        if let Some(auth_obj) = auth.as_object_mut() {
            auth_obj.insert("OPENAI_API_KEY".to_string(), Value::String(token));
        }
        obj.insert("auth".to_string(), auth);
    }

    Ok(())
}

pub fn restore_codex_settings_for_backfill(
    settings: &mut Value,
    template_settings: &Value,
    restore_provider_token: bool,
) -> Result<(), AppError> {
    if restore_provider_token {
        restore_codex_provider_token_for_backfill(settings, template_settings)?;
    }
    Ok(())
}

/// Update a field in Codex config.toml using toml_edit (syntax-preserving).
///
/// Supported fields:
/// - `"base_url"`: writes to `[model_providers.<current>].base_url` if `model_provider` exists,
///   otherwise falls back to top-level `base_url`.
/// - `"wire_api"`: writes to `[model_providers.<current>].wire_api` if `model_provider` exists,
///   otherwise falls back to top-level `wire_api`.
/// - `"model"` / `"model_catalog_json"`: writes to top-level field.
///
/// Empty value removes the field.
pub fn update_codex_toml_field(toml_str: &str, field: &str, value: &str) -> Result<String, String> {
    let mut doc = toml_str
        .parse::<DocumentMut>()
        .map_err(|e| format!("TOML parse error: {e}"))?;

    let trimmed = value.trim();

    match field {
        "base_url" | "wire_api" => {
            let model_provider = doc
                .get("model_provider")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string);

            if model_provider
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case(CODEX_OPENAI_MODEL_PROVIDER_ID))
            {
                if field == "base_url" {
                    if trimmed.is_empty() {
                        doc.as_table_mut().remove("openai_base_url");
                    } else {
                        doc["openai_base_url"] = toml_edit::value(trimmed);
                    }
                }
                return Ok(doc.to_string());
            }

            if let Some(provider_key) = model_provider {
                // Ensure [model_providers] table exists
                if doc.get("model_providers").is_none() {
                    doc["model_providers"] = toml_edit::table();
                }

                if let Some(model_providers) = doc["model_providers"].as_table_mut() {
                    // Ensure [model_providers.<provider_key>] table exists
                    if !model_providers.contains_key(&provider_key) {
                        model_providers[&provider_key] = toml_edit::table();
                    }

                    if let Some(provider_table) = model_providers[&provider_key].as_table_mut() {
                        if trimmed.is_empty() {
                            provider_table.remove(field);
                        } else {
                            provider_table[field] = toml_edit::value(trimmed);
                        }
                        return Ok(doc.to_string());
                    }
                }
            }

            // Fallback: no model_provider or structure mismatch → top-level field
            if trimmed.is_empty() {
                doc.as_table_mut().remove(field);
            } else {
                doc[field] = toml_edit::value(trimmed);
            }
        }
        "model" | "model_catalog_json" => {
            if trimmed.is_empty() {
                doc.as_table_mut().remove(field);
            } else {
                doc[field] = toml_edit::value(trimmed);
            }
        }
        _ => return Err(format!("unsupported field: {field}")),
    }

    Ok(doc.to_string())
}

/// Remove `base_url` from the active model_provider section only if it matches `predicate`.
/// Also removes top-level `base_url` if it matches.
/// Used by proxy cleanup to strip local proxy URLs without touching user-configured URLs.
pub fn remove_codex_toml_base_url_if(toml_str: &str, predicate: impl Fn(&str) -> bool) -> String {
    let mut doc = match toml_str.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return toml_str.to_string(),
    };

    let model_provider = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::to_string);

    if let Some(provider_key) = model_provider {
        if let Some(model_providers) = doc
            .get_mut("model_providers")
            .and_then(|v| v.as_table_mut())
        {
            if let Some(provider_table) = model_providers
                .get_mut(provider_key.as_str())
                .and_then(|v| v.as_table_mut())
            {
                let should_remove = provider_table
                    .get("base_url")
                    .and_then(|item| item.as_str())
                    .map(&predicate)
                    .unwrap_or(false);
                if should_remove {
                    provider_table.remove("base_url");
                }
            }
        }
    }

    // Fallback: also clean up top-level base_url if it matches
    let should_remove_root = doc
        .get("base_url")
        .and_then(|item| item.as_str())
        .map(&predicate)
        .unwrap_or(false);
    if should_remove_root {
        doc.as_table_mut().remove("base_url");
    }

    let should_remove_openai = doc
        .get("openai_base_url")
        .and_then(|item| item.as_str())
        .map(&predicate)
        .unwrap_or(false);
    if should_remove_openai {
        doc.as_table_mut().remove("openai_base_url");
    }

    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prepare_provider_live_config_rejects_key_without_config() {
        let err = prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), "")
            .expect_err("empty config with API key should not truncate live config");

        assert!(
            err.to_string().contains("config.toml"),
            "error should explain missing config.toml, got: {err}"
        );
    }

    #[test]
    fn prepare_provider_live_config_uses_top_level_token_for_reserved_provider() {
        let input = r#"model_provider = "openai"
model = "gpt-5"
"#;

        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");

        assert_eq!(
            parsed
                .get("experimental_bearer_token")
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert!(
            parsed.get("model_providers").is_none(),
            "reserved provider tables should not be synthesized"
        );
    }

    #[test]
    fn extract_bearer_uses_top_level_token_for_reserved_provider() {
        let input = r#"model_provider = "openai"
experimental_bearer_token = "top-level-key"

[model_providers.openai]
experimental_bearer_token = "stale-table-key"
"#;

        assert_eq!(
            extract_codex_experimental_bearer_token(input).as_deref(),
            Some("top-level-key")
        );
    }

    #[test]
    fn should_not_restore_provider_token_for_oauth_only_template() {
        let oauth_template = json!({
            "auth": {
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": "oauth-access"
                }
            }
        });
        let api_key_template = json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            }
        });

        assert!(
            !should_restore_codex_provider_token_for_backfill(Some("custom"), &oauth_template),
            "OAuth-only templates should not backfill bearer tokens into OPENAI_API_KEY"
        );
        assert!(
            should_restore_codex_provider_token_for_backfill(Some("custom"), &api_key_template),
            "custom API-key providers should still restore provider bearer tokens"
        );
        assert!(
            !should_restore_codex_provider_token_for_backfill(Some("official"), &api_key_template),
            "official providers should never restore third-party bearer tokens"
        );
    }

    #[test]
    fn prepare_provider_live_config_does_not_create_incomplete_provider_table() {
        let input = r#"model_provider = "vendor_x"
model = "gpt-5"
"#;

        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");

        assert_eq!(
            parsed
                .get("experimental_bearer_token")
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert!(
            parsed.get("model_providers").is_none(),
            "missing provider tables should not be synthesized without endpoint fields"
        );
    }

    #[test]
    fn merge_provider_config_preserves_live_user_sections() {
        let live_config = r#"model = "gpt-5.5"
model_provider = "openai"
approval_policy = "on-request"
experimental_bearer_token = "old-top-level-token"

[features]
goals = true
memories = true

[desktop]
show-context-window-usage = true

[memories]
generate_memories = true
use_memories = true

[projects."C:\\Users\\sunda\\Documents\\trace"]
trust_level = "trusted"

[mcp_servers.matrix]
command = "matrix-websearch"

[model_providers.openai]
name = "OpenAI"
"#;

        let provider_config = r#"model = "gpt-5.4"
model_provider = "codex_model_router_v2"
model_catalog_json = "cc-switch-model-catalog.json"

[features]
memories = false

[mcp_servers.matrix]
command = "stale-matrix"

[mcp_servers.shared]
command = "shared-command"

[model_providers.codex_model_router_v2]
name = "OpenAI Multi-Model Router"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
experimental_bearer_token = "provider-token"
"#;

        let merged =
            merge_codex_provider_config_texts(live_config, provider_config).expect("merge config");
        let parsed: toml::Value = toml::from_str(&merged).expect("parse merged config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("codex_model_router_v2")
        );
        assert_eq!(
            parsed.get("model").and_then(|value| value.as_str()),
            Some("gpt-5.4")
        );
        assert_eq!(
            parsed
                .get("model_catalog_json")
                .and_then(|value| value.as_str()),
            Some("cc-switch-model-catalog.json")
        );
        assert!(
            parsed.get("experimental_bearer_token").is_none(),
            "stale live top-level provider token should not survive a provider-scoped switch"
        );
        assert_eq!(
            parsed
                .get("features")
                .and_then(|value| value.get("memories"))
                .and_then(|value| value.as_bool()),
            Some(true),
            "live user feature flags should win over stale provider snapshots"
        );
        assert_eq!(
            parsed
                .get("desktop")
                .and_then(|value| value.get("show-context-window-usage"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            parsed
                .get("memories")
                .and_then(|value| value.get("use_memories"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert!(
            parsed
                .get("projects")
                .and_then(|value| value.get(r"C:\Users\sunda\Documents\trace"))
                .is_some(),
            "project trust table should be preserved"
        );
        assert!(
            parsed
                .get("mcp_servers")
                .and_then(|value| value.get("matrix"))
                .is_some(),
            "live MCP tables should be preserved"
        );
        assert_eq!(
            parsed
                .get("mcp_servers")
                .and_then(|value| value.get("matrix"))
                .and_then(|value| value.get("command"))
                .and_then(|value| value.as_str()),
            Some("matrix-websearch"),
            "provider snapshots should not overwrite existing live MCP entries"
        );
        assert_eq!(
            parsed
                .get("mcp_servers")
                .and_then(|value| value.get("shared"))
                .and_then(|value| value.get("command"))
                .and_then(|value| value.as_str()),
            Some("shared-command"),
            "common config snippets should still be able to add missing MCP entries"
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("openai"))
                .is_some(),
            "existing provider tables should remain available"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("codex_model_router_v2"))
                .and_then(|value| value.get("experimental_bearer_token"))
                .and_then(|value| value.as_str()),
            Some("provider-token")
        );
    }

    #[test]
    fn merge_empty_official_config_clears_provider_fields_but_keeps_user_sections() {
        let live_config = r#"model = "deepseek-v4-flash"
model_provider = "codex_model_router_v2"
model_catalog_json = "cc-switch-model-catalog.json"
openai_base_url = "http://127.0.0.1:15721/v1"
experimental_bearer_token = "stale-token"
approval_policy = "on-request"

[projects."C:\\Users\\sunda\\Documents\\LLMservice"]
trust_level = "trusted"

[mcp_servers.matrix]
command = "matrix-websearch"

[model_providers.codex_model_router_v2]
name = "OpenAI Multi-Model Router"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#;

        let merged =
            merge_codex_provider_config_texts(live_config, "").expect("merge official config");
        let parsed: toml::Value = toml::from_str(&merged).expect("parse merged config");

        assert!(parsed.get("model").is_none());
        assert!(parsed.get("model_provider").is_none());
        assert!(parsed.get("model_catalog_json").is_none());
        assert!(parsed.get("openai_base_url").is_none());
        assert!(parsed.get("experimental_bearer_token").is_none());
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("codex_model_router_v2"))
                .is_none(),
            "official fallback must remove the active cc-switch router table"
        );
        assert_eq!(
            parsed
                .get("approval_policy")
                .and_then(|value| value.as_str()),
            Some("on-request")
        );
        assert!(
            parsed
                .get("projects")
                .and_then(|value| value.get(r"C:\Users\sunda\Documents\LLMservice"))
                .is_some(),
            "official fallback must preserve Codex project trust/history context"
        );
        assert!(
            parsed
                .get("mcp_servers")
                .and_then(|value| value.get("matrix"))
                .is_some(),
            "official fallback must preserve MCP servers"
        );
    }

    #[test]
    fn merge_openai_router_config_uses_builtin_openai_history_bucket() {
        let live_config = r#"model = "gpt-5.5"
approval_policy = "on-request"

[projects."C:\\Users\\sunda\\Documents\\LLMservice"]
trust_level = "trusted"
"#;
        let provider_config = r#"model = "gpt-5.5"
model_provider = "openai"
openai_base_url = "http://127.0.0.1:15721/v1"
model_catalog_json = "cc-switch-model-catalog.json"
"#;

        let merged = merge_codex_provider_config_texts(live_config, provider_config)
            .expect("merge openai router config");
        let parsed: toml::Value = toml::from_str(&merged).expect("parse merged config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("openai")
        );
        assert_eq!(
            parsed
                .get("openai_base_url")
                .and_then(|value| value.as_str()),
            Some("http://127.0.0.1:15721/v1")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("openai"))
                .is_none(),
            "built-in OpenAI must not be shadowed by an ignored configured table"
        );
        assert!(
            parsed
                .get("projects")
                .and_then(|value| value.get(r"C:\Users\sunda\Documents\LLMservice"))
                .is_some(),
            "router switch must preserve Codex project history context"
        );
    }

    #[test]
    fn merge_provider_without_catalog_removes_stale_cc_switch_catalog_pointer() {
        let live_config = r#"model = "gpt-5.5"
model_provider = "codex_model_router_v2"
model_catalog_json = "cc-switch-model-catalog.json"

[projects."C:\\Users\\sunda\\Documents\\LLMservice"]
trust_level = "trusted"

[model_providers.codex_model_router_v2]
name = "OpenAI Multi-Model Router"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#;
        let provider_config = r#"model = "gpt-5.4"
model_provider = "custom"

[model_providers.custom]
name = "Plain Custom"
base_url = "https://plain.example/v1"
wire_api = "responses"
"#;

        let merged = merge_codex_provider_config_texts(live_config, provider_config)
            .expect("merge provider config");
        let parsed: toml::Value = toml::from_str(&merged).expect("parse merged config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(
            parsed.get("model_catalog_json").is_none(),
            "stale cc-switch catalog pointer must not survive provider switches"
        );
        assert!(parsed
            .get("projects")
            .and_then(|value| value.get(r"C:\Users\sunda\Documents\LLMservice"))
            .is_some());
    }

    #[test]
    fn prepare_provider_live_config_preserves_custom_provider_id() {
        let input = r#"model_provider = "vendor_alpha"
model = "gpt-5.4"
profile = "work"

[model_providers.vendor_alpha]
name = "Vendor Alpha"
base_url = "https://alpha.example/v1"
wire_api = "responses"

[profiles.work]
model_provider = "vendor_alpha"
model = "gpt-5.4"
"#;

        let result =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("vendor_alpha")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("custom"))
                .is_none(),
            "provider writes should not force custom provider ids"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("vendor_alpha"))
                .and_then(|v| v.get("experimental_bearer_token"))
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|v| v.get("work"))
                .and_then(|v| v.get("model_provider"))
                .and_then(|v| v.as_str()),
            Some("vendor_alpha"),
            "profile provider references should be preserved"
        );
    }

    #[test]
    fn backfill_preserves_live_model_provider_id() {
        let mut live_settings = json!({
            "auth": {},
            "config": r#"model_provider = "vendor_beta"

[model_providers.vendor_beta]
name = "Vendor Beta"
base_url = "https://beta.example/v1"
wire_api = "responses"
"#,
        });
        let template_settings = json!({
            "auth": {},
            "config": r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://custom.example/v1"
wire_api = "responses"
"#,
        });

        restore_codex_settings_for_backfill(&mut live_settings, &template_settings, false).unwrap();
        let config = live_settings.get("config").and_then(Value::as_str).unwrap();
        let parsed: toml::Value = toml::from_str(config).unwrap();

        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("vendor_beta")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("vendor_beta"))
                .is_some(),
            "backfill should not rewrite user-selected provider tables"
        );
    }

    #[test]
    fn base_url_writes_into_correct_model_provider_section() {
        let input = r#"model_provider = "any"
model = "gpt-5.1-codex"

[model_providers.any]
name = "any"
wire_api = "responses"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://example.com/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .expect("base_url should be in model_providers.any");
        assert_eq!(base_url, "https://example.com/v1");

        // Should NOT have top-level base_url
        assert!(parsed.get("base_url").is_none());

        // wire_api preserved
        let wire_api = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("wire_api"))
            .and_then(|v| v.as_str());
        assert_eq!(wire_api, Some("responses"));
    }

    #[test]
    fn wire_api_writes_into_correct_model_provider_section() {
        let input = r#"model_provider = "chat_only"
model = "gpt-5.1-codex"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://example.com/v1"
wire_api = "chat"
"#;

        let result = update_codex_toml_field(input, "wire_api", "responses").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let provider = parsed
            .get("model_providers")
            .and_then(|v| v.get("chat_only"))
            .expect("model_providers.chat_only should exist");

        assert_eq!(
            provider.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
        assert_eq!(
            provider.get("base_url").and_then(|v| v.as_str()),
            Some("https://example.com/v1")
        );
        assert!(parsed.get("wire_api").is_none());
    }

    #[test]
    fn base_url_creates_section_when_missing() {
        let input = r#"model_provider = "custom"
model = "gpt-4"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://custom.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("custom"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .expect("should create section and set base_url");
        assert_eq!(base_url, "https://custom.api/v1");
    }

    #[test]
    fn base_url_uses_openai_base_url_for_builtin_openai_provider() {
        let input = r#"model_provider = "openai"
model = "gpt-5.5"
"#;

        let result =
            update_codex_toml_field(input, "base_url", "http://127.0.0.1:15721/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("openai_base_url").and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/v1")
        );
        assert!(parsed.get("base_url").is_none());
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("openai"))
                .is_none(),
            "configured model_providers.openai is ignored by Codex and must not be generated"
        );
    }

    #[test]
    fn wire_api_noops_for_builtin_openai_provider() {
        let input = r#"model_provider = "openai"
model = "gpt-5.5"
openai_base_url = "http://127.0.0.1:15721/v1"
"#;

        let result = update_codex_toml_field(input, "wire_api", "responses").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert!(parsed.get("wire_api").is_none());
        assert!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("openai"))
                .is_none(),
            "built-in OpenAI already uses Responses and must not get a shadow table"
        );
        assert_eq!(
            parsed.get("openai_base_url").and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/v1")
        );
    }

    #[test]
    fn base_url_falls_back_to_top_level_without_model_provider() {
        let input = r#"model = "gpt-4"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://fallback.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("base_url")
            .and_then(|v| v.as_str())
            .expect("should set top-level base_url");
        assert_eq!(base_url, "https://fallback.api/v1");
    }

    #[test]
    fn clearing_base_url_removes_only_from_correct_section() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "https://old.api/v1"
wire_api = "responses"

[mcp_servers.context7]
command = "npx"
"#;

        let result = update_codex_toml_field(input, "base_url", "").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        // base_url removed from model_providers.any
        let any_section = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .expect("model_providers.any should exist");
        assert!(any_section.get("base_url").is_none());

        // wire_api preserved
        assert_eq!(
            any_section.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );

        // mcp_servers untouched
        assert!(parsed.get("mcp_servers").is_some());
    }

    #[test]
    fn model_field_operates_on_top_level() {
        let input = r#"model_provider = "any"
model = "gpt-4"

[model_providers.any]
name = "any"
"#;

        let result = update_codex_toml_field(input, "model", "gpt-5").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(parsed.get("model").and_then(|v| v.as_str()), Some("gpt-5"));

        // Clear model
        let result2 = update_codex_toml_field(&result, "model", "").unwrap();
        let parsed2: toml::Value = toml::from_str(&result2).unwrap();
        assert!(parsed2.get("model").is_none());
    }

    #[test]
    fn preserves_comments_and_whitespace() {
        let input = r#"# My Codex config
model_provider = "any"
model = "gpt-4"

# Provider section
[model_providers.any]
name = "any"
base_url = "https://old.api/v1"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();

        // Comments should be preserved
        assert!(result.contains("# My Codex config"));
        assert!(result.contains("# Provider section"));
    }

    #[test]
    fn does_not_misplace_when_profiles_section_follows() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "https://old.api/v1"

[profiles.default]
model = "gpt-4"
"#;

        let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        // base_url in correct section
        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str());
        assert_eq!(base_url, Some("https://new.api/v1"));

        // profiles section untouched
        let profile_model = parsed
            .get("profiles")
            .and_then(|v| v.get("default"))
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str());
        assert_eq!(profile_model, Some("gpt-4"));
    }

    #[test]
    fn remove_base_url_if_predicate() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
base_url = "http://127.0.0.1:5000/v1"
wire_api = "responses"
"#;

        let result =
            remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let any_section = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .unwrap();
        assert!(any_section.get("base_url").is_none());
        assert_eq!(
            any_section.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
    }

    #[test]
    fn remove_base_url_if_keeps_non_matching() {
        let input = r#"model_provider = "any"

[model_providers.any]
base_url = "https://production.api/v1"
"#;

        let result =
            remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str());
        assert_eq!(base_url, Some("https://production.api/v1"));
    }

    #[test]
    fn remove_base_url_if_cleans_openai_base_url() {
        let input = r#"model_provider = "openai"
openai_base_url = "http://127.0.0.1:15721/v1"
"#;

        let result =
            remove_codex_toml_base_url_if(input, |url| url.starts_with("http://127.0.0.1"));
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert!(parsed.get("openai_base_url").is_none());
        assert_eq!(
            parsed.get("model_provider").and_then(|v| v.as_str()),
            Some("openai"),
            "cleanup should remove the local proxy URL without changing the history bucket"
        );
    }

    #[test]
    fn codex_model_catalog_uses_provider_models_and_context() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "description": "Frontier model",
            "base_instructions": "gpt-5.5 base instructions",
            "model_messages": {
                "instructions_template": "gpt-5.5 instructions template",
                "instructions_variables": {
                    "personality_default": "",
                    "personality_friendly": "",
                    "personality_pragmatic": ""
                }
            },
            "additional_speed_tiers": ["fast"],
            "service_tiers": [
                {
                    "id": "priority",
                    "name": "Fast",
                    "description": "1.5x speed, increased usage"
                }
            ],
            "availability_nux": {
                "message": "GPT-5.5 is now available."
            },
            "upgrade": {
                "target": "gpt-5.5"
            },
            "context_window": 272000,
            "max_context_window": 272000,
            "supports_image_detail_original": true,
            "input_modalities": ["text", "image"],
            "web_search_tool_type": "text_and_image"
        });
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "displayName": "DeepSeek V4 Flash",
                        "contextWindow": "64000"
                    },
                    {
                        "model": "kimi-k2",
                        "display_name": "Kimi K2"
                    }
                ]
            }
        });
        let specs = codex_catalog_model_specs(&settings, r#"model_context_window = 128000"#);
        let catalog = codex_model_catalog_from_specs(&specs, &template);
        let models = catalog
            .get("models")
            .and_then(|value| value.as_array())
            .expect("models should be an array");

        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0].get("slug").and_then(|value| value.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            models[0]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(64_000)
        );
        assert_eq!(
            models[0].get("input_modalities"),
            Some(&json!(["text"])),
            "DeepSeek V4 must stay text-only so Codex does not inject image_generation"
        );
        assert_eq!(
            models[0]
                .get("supports_image_detail_original")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            models[0]
                .get("web_search_tool_type")
                .and_then(|value| value.as_str()),
            Some("text")
        );
        assert_eq!(
            models[1]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(128_000)
        );
        assert_eq!(
            models[1].get("input_modalities"),
            Some(&json!(["text", "image"])),
            "models without a text-only override should keep the template modalities"
        );
        assert!(
            models[0].get("model_messages").is_some(),
            "Codex requires model_messages in custom catalogs"
        );
        assert_eq!(
            models[0]
                .get("base_instructions")
                .and_then(|value| value.as_str()),
            Some("gpt-5.5 base instructions")
        );
        assert_eq!(
            models[0].get("model_messages"),
            template.get("model_messages"),
            "custom catalog entries should keep the gpt-5.5 agent template"
        );
        assert_eq!(
            models[0].get("additional_speed_tiers"),
            Some(&json!([])),
            "generated third-party entries should not inherit OpenAI speed tiers"
        );
        assert!(
            models[0]
                .get("availability_nux")
                .is_some_and(|value| value.is_null()),
            "generated third-party entries should not inherit GPT-5.5 launch messaging"
        );
    }

    #[test]
    fn codex_model_catalog_keeps_spark_text_only() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "context_window": 272000,
            "max_context_window": 272000,
            "supports_image_detail_original": true,
            "input_modalities": ["text", "image"],
            "web_search_tool_type": "text_and_image"
        });
        let spec = CodexCatalogModelSpec {
            model: "gpt-5.3-codex-spark".to_string(),
            display_name: "Codex Spark".to_string(),
            context_window: 128_000,
            text_only: true,
        };
        let entry = codex_catalog_model_entry(&template, &spec, 0);

        assert_eq!(
            entry.get("input_modalities"),
            Some(&json!(["text"])),
            "Spark rejects hosted image_generation, so it must not inherit image modality"
        );
        assert_eq!(
            entry
                .get("supports_image_detail_original")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            entry
                .get("web_search_tool_type")
                .and_then(|value| value.as_str()),
            Some("text")
        );
    }

    #[test]
    fn codex_model_catalog_preserves_openai_gpt_speed_tiers() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "context_window": 272000,
            "max_context_window": 272000,
            "additional_speed_tiers": ["fast"],
            "service_tiers": [
                {
                    "id": "priority",
                    "name": "Fast",
                    "description": "1.5x speed, increased usage"
                }
            ],
            "availability_nux": {
                "message": "GPT-5.5 is now available."
            },
            "upgrade": {
                "target": "gpt-5.5"
            }
        });
        let spec = CodexCatalogModelSpec {
            model: "gpt-5.4".to_string(),
            display_name: "GPT-5.4".to_string(),
            context_window: 272_000,
            text_only: false,
        };
        let entry = codex_catalog_model_entry(&template, &spec, 0);

        assert_eq!(
            entry.get("additional_speed_tiers"),
            Some(&json!(["fast"])),
            "OpenAI official GPT entries must keep Codex speed choices"
        );
        assert_eq!(
            entry.get("service_tiers"),
            template.get("service_tiers"),
            "OpenAI official GPT entries must keep Codex service tiers"
        );
        assert!(
            entry
                .get("availability_nux")
                .is_some_and(|value| value.is_null()),
            "generated entries should still drop template launch messaging"
        );
    }

    #[test]
    fn codex_model_catalog_clears_non_priority_gpt_speed_tiers() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "context_window": 272000,
            "max_context_window": 272000,
            "additional_speed_tiers": ["fast"],
            "service_tiers": [
                {
                    "id": "priority",
                    "name": "Fast",
                    "description": "1.5x speed, increased usage"
                }
            ]
        });
        let spec = CodexCatalogModelSpec {
            model: "gpt-5.4-mini".to_string(),
            display_name: "GPT-5.4 Mini".to_string(),
            context_window: 128_000,
            text_only: false,
        };
        let entry = codex_catalog_model_entry(&template, &spec, 0);

        assert_eq!(
            entry.get("additional_speed_tiers"),
            Some(&json!([])),
            "GPT mini entries should not inherit GPT-5.5 speed choices"
        );
        assert_eq!(
            entry.get("service_tiers"),
            Some(&json!([])),
            "GPT mini entries should not inherit GPT-5.5 service tiers"
        );
    }

    #[test]
    fn codex_catalog_text_only_capabilities_override_hardcoded_name() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "context_window": 272000,
            "max_context_window": 272000,
            "supports_image_detail_original": true,
            "input_modalities": ["text", "image"],
            "web_search_tool_type": "text_and_image",
            "model_messages": []
        });
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "displayName": "DeepSeek Flash"
                    }
                ]
            },
            "codexRouting": {
                "routes": [{
                    "id": "openai",
                    "match": { "models": ["deepseek-v4-flash"] },
                    "capabilities": {
                        "inputModalities": ["text"]
                    }
                }]
            }
        });
        let specs = codex_catalog_model_specs(&settings, r#"model_context_window = 64000"#);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].model, "deepseek-v4-flash");
        assert!(specs[0].text_only);

        let catalog = codex_model_catalog_from_specs(&specs, &template);
        let models = catalog
            .get("models")
            .and_then(|value| value.as_array())
            .expect("models should be an array");
        assert_eq!(
            models[0].get("slug").and_then(|value| value.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(models[0].get("input_modalities"), Some(&json!(["text"])));

        let settings_without_capability = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "gpt-5.3-codex-spark",
                        "displayName": "Codex Spark"
                    }
                ]
            }
        });
        let fallback = codex_catalog_model_specs(
            &settings_without_capability,
            r#"model_context_window = 64000"#,
        );
        assert_eq!(fallback.len(), 1);
        assert!(fallback[0].text_only);
    }

    #[test]
    fn codex_model_catalog_marks_deepseekv4_aliases_text_only() {
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "DeepSeek V4 Pro",
                        "displayName": "DeepSeek V4 Pro"
                    }
                ]
            }
        });

        let specs = codex_catalog_model_specs(&settings, r#"model_context_window = 64000"#);

        assert_eq!(specs.len(), 1);
        assert!(specs[0].text_only);
    }

    #[test]
    fn model_catalog_json_field_writes_relative_filename() {
        let input = r#"model_provider = "any"

[model_providers.any]
name = "any"
"#;
        let catalog_path = Path::new("/tmp/cc-switch-model-catalog.json");

        let result = set_codex_model_catalog_json_field(input, Some(catalog_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed
                .get("model_catalog_json")
                .and_then(|value| value.as_str()),
            Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("any"))
                .and_then(|value| value.get("model_catalog_json"))
                .is_none(),
            "model_catalog_json should stay top-level"
        );
    }

    #[test]
    fn resolve_catalog_path_returns_none_when_config_missing_field() {
        let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
        assert!(resolve_cc_switch_catalog_path("", &generated).is_none());
        assert!(
            resolve_cc_switch_catalog_path("model = \"gpt-5\"", &generated).is_none(),
            "no model_catalog_json field should yield None"
        );
    }

    #[test]
    fn resolve_catalog_path_accepts_cc_switch_owned_file() {
        let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
        let config = r#"model_catalog_json = "/tmp/.codex/cc-switch-model-catalog.json"
"#;
        let resolved = resolve_cc_switch_catalog_path(config, &generated).expect("path resolves");
        assert_eq!(resolved, generated);
    }

    #[test]
    fn resolve_catalog_path_rejects_user_owned_external_file() {
        let generated = PathBuf::from("/tmp/.codex/cc-switch-model-catalog.json");
        let config = r#"model_catalog_json = "/Users/me/.codex/my-handwritten-catalog.json"
"#;
        assert!(
            resolve_cc_switch_catalog_path(config, &generated).is_none(),
            "external catalog files should be left alone"
        );
    }

    #[test]
    fn build_simplified_catalog_round_trips_user_input() {
        let config = "";
        let catalog = r#"{
            "models": [
                { "slug": "deepseek-v4-pro", "display_name": "deepseek-v4-pro", "context_window": 1000000 },
                { "slug": "deepseek-v4-flash", "display_name": "DeepSeek Flash", "context_window": 1000000 }
            ]
        }"#;
        let result = build_simplified_catalog_from_texts(config, catalog).expect("entries found");
        let models = result
            .get("models")
            .and_then(|m| m.as_array())
            .expect("models array");
        assert_eq!(models.len(), 2);

        // First entry: display_name == slug → displayName squashed; explicit
        // context_window != default 128_000 → preserved.
        assert_eq!(
            models[0].get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-pro")
        );
        assert!(models[0].get("displayName").is_none());
        assert_eq!(
            models[0].get("contextWindow").and_then(|v| v.as_u64()),
            Some(1_000_000)
        );

        // Second entry: display_name distinct from slug → preserved.
        assert_eq!(
            models[1].get("displayName").and_then(|v| v.as_str()),
            Some("DeepSeek Flash")
        );
    }

    #[test]
    fn build_simplified_catalog_squashes_default_context_window() {
        // Default fallback is 128_000 when config.toml has no model_context_window.
        let catalog = r#"{
            "models": [{ "slug": "kimi", "display_name": "kimi", "context_window": 128000 }]
        }"#;
        let result = build_simplified_catalog_from_texts("", catalog).expect("entry");
        let entry = &result.get("models").unwrap().as_array().unwrap()[0];
        assert!(
            entry.get("contextWindow").is_none(),
            "default 128_000 should be squashed so the form shows blank, matching the user's blank input"
        );
    }

    #[test]
    fn build_simplified_catalog_respects_explicit_model_context_window() {
        // When config.toml sets model_context_window, that becomes the default fallback.
        let config = r#"model_context_window = 200000
"#;
        let catalog = r#"{
            "models": [
                { "slug": "a", "display_name": "a", "context_window": 200000 },
                { "slug": "b", "display_name": "b", "context_window": 500000 }
            ]
        }"#;
        let result = build_simplified_catalog_from_texts(config, catalog).expect("entries");
        let models = result.get("models").unwrap().as_array().unwrap();
        // Matches default → squashed.
        assert!(models[0].get("contextWindow").is_none());
        // Different from default → preserved.
        assert_eq!(
            models[1].get("contextWindow").and_then(|v| v.as_u64()),
            Some(500_000)
        );
    }

    #[test]
    fn build_simplified_catalog_returns_none_when_unparseable() {
        assert!(build_simplified_catalog_from_texts("", "not json").is_none());
        assert!(build_simplified_catalog_from_texts("", "{}").is_none());
        assert!(
            build_simplified_catalog_from_texts("", r#"{"models": []}"#).is_none(),
            "empty models array should yield None so the field is not inserted at all"
        );
        assert!(
            build_simplified_catalog_from_texts(
                "",
                r#"{"models": [{"display_name": "no slug"}]}"#,
            )
            .is_none(),
            "entries lacking slug are skipped; a fully-skipped catalog yields None"
        );
    }

    #[test]
    fn codex_cli_candidates_are_non_empty() {
        let candidates = codex_cli_candidates();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == Path::new("codex")),
            "codex CLI candidates must include the PATH entry"
        );
    }

    #[test]
    fn codex_cli_candidates_include_user_node_manager_bins() {
        let temp_home = tempfile::tempdir().expect("create temp home");
        let home = temp_home.path();
        let expected = [
            home.join(".nvm/versions/node/v22.14.0/bin/codex"),
            home.join(".volta/bin/codex"),
            home.join(".asdf/shims/codex"),
            home.join(".local/share/mise/shims/codex"),
            home.join(".local/share/fnm/node-versions/v22.14.0/installation/bin/codex"),
        ];

        for candidate in &expected {
            std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
                .expect("create candidate parent");
            std::fs::write(candidate, "").expect("create candidate");
        }

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

        for candidate in expected {
            assert!(
                candidates.contains(&candidate),
                "user-level Codex CLI candidate should be discovered: {}",
                candidate.display()
            );
        }
    }

    #[test]
    fn codex_cli_candidates_deduplicate_entries() {
        let temp_home = tempfile::tempdir().expect("create temp home");
        let home = temp_home.path();
        let candidate = home.join(".volta/bin/codex");
        std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
            .expect("create candidate parent");
        std::fs::write(&candidate, "").expect("create candidate");

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        push_existing_codex_cli_candidate(&mut candidates, &mut seen, candidate.clone());
        push_home_codex_cli_candidates(&mut candidates, &mut seen, home);

        assert_eq!(
            candidates.iter().filter(|path| **path == candidate).count(),
            1,
            "duplicate candidates should be removed"
        );
    }

    #[test]
    fn static_template_is_valid_json_with_slug() {
        let template =
            load_codex_model_template_static().expect("static template must parse as valid JSON");
        assert_eq!(
            template.get("slug").and_then(|v| v.as_str()),
            Some("gpt-5.5"),
            "static template slug must be gpt-5.5"
        );
    }

    #[test]
    fn static_template_has_required_keys() {
        let template =
            load_codex_model_template_static().expect("static template must parse as valid JSON");
        for key in &[
            "model_messages",
            "base_instructions",
            "context_window",
            "display_name",
        ] {
            assert!(
                template.get(key).is_some(),
                "static template must contain key '{key}'"
            );
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn set_catalog_json_field_writes_filename_ignoring_unc_path() {
        let input = r#"model_provider = "custom"
model = "glm-5"
"#;
        // Simulate a WSL UNC path as cc-switch would see it on Windows;
        // the function now writes just the relative filename.
        let unc_path =
            Path::new(r"\\wsl.localhost\Ubuntu\home\user\.codex\cc-switch-model-catalog.json");

        let result = set_codex_model_catalog_json_field(input, Some(unc_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        let written_path = parsed
            .get("model_catalog_json")
            .and_then(|v| v.as_str())
            .expect("model_catalog_json should be set");
        assert_eq!(
            written_path, CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME,
            "should write only the relative filename, not the UNC path"
        );
    }

    #[test]
    fn set_catalog_json_field_writes_filename_for_any_path() {
        let input = r#"model_provider = "custom"
model = "glm-5"
"#;
        let regular_path = Path::new("/home/user/.codex/cc-switch-model-catalog.json");

        let result = set_codex_model_catalog_json_field(input, Some(regular_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed.get("model_catalog_json").and_then(|v| v.as_str()),
            Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME),
            "should write only the relative filename, not the full path"
        );
    }

    #[test]
    fn set_catalog_json_none_removes_cc_switch_owned_by_filename() {
        // After the WSL fix, TOML may contain a Linux-style path.
        // The None arm must still remove it (file_name match catches any format).
        let input = r#"model_catalog_json = "/home/user/.codex/cc-switch-model-catalog.json"
"#;
        let result = set_codex_model_catalog_json_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert!(
            parsed.get("model_catalog_json").is_none(),
            "None arm should remove cc-switch-owned field regardless of path format"
        );
    }

    #[test]
    fn set_catalog_json_none_preserves_user_owned_catalog() {
        let input = r#"model_catalog_json = "/Users/me/.codex/my-custom-catalog.json"
"#;
        let result = set_codex_model_catalog_json_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed.get("model_catalog_json").and_then(|v| v.as_str()),
            Some("/Users/me/.codex/my-custom-catalog.json"),
            "None arm should NOT remove user-owned catalog"
        );
    }

    #[test]
    fn resolve_catalog_finds_relative_filename() {
        let config_text = r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let generated_path = PathBuf::from("/home/user/.codex/cc-switch-model-catalog.json");
        let result = resolve_cc_switch_catalog_path(config_text, &generated_path);
        assert_eq!(
            result,
            Some(generated_path),
            "relative filename should resolve to generated_path for file I/O"
        );
    }

    #[test]
    fn resolve_catalog_ignores_user_owned_relative() {
        let config_text = r#"model_catalog_json = "my-custom-catalog.json"
"#;
        let generated_path = PathBuf::from("/home/user/.codex/cc-switch-model-catalog.json");
        let result = resolve_cc_switch_catalog_path(config_text, &generated_path);
        assert_eq!(
            result, None,
            "user-owned catalog should not be claimed by cc-switch"
        );
    }

    #[test]
    fn set_catalog_json_none_removes_relative_path() {
        let input = r#"model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let result = set_codex_model_catalog_json_field(input, None).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert!(
            parsed.get("model_catalog_json").is_none(),
            "None arm should remove relative cc-switch-owned field"
        );
    }
}
