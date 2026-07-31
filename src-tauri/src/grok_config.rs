use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{get_home_dir, write_text_file};
use crate::error::AppError;
use crate::provider::Provider;

pub const DEFAULT_MODEL: &str = "grok-4.5";
pub const DEFAULT_API_BACKEND: &str = "responses";
pub const DEFAULT_CONTEXT_WINDOW: i64 = 500_000;
pub const DEFAULT_IMAGE_MODEL: &str = "grok-imagine-image";
const IMAGE_AUTH_SCOPE: &str = "xai::api_key";
const IMAGE_AUTH_OWNER: &str = "cc-switch";
const IMAGE_AUTH_STATE_FILE: &str = ".cc-switch-image-auth.json";

#[derive(Debug, Clone, PartialEq)]
enum ImageCredentialAction {
    SetManaged(String),
    RestorePrevious,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedGrokLive {
    pub(crate) settings: Value,
    credential_action: Option<ImageCredentialAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ManagedImageAuthState {
    // Preserve the user's prior scope so switching to Official can restore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original_scope: Option<Value>,
    managed_key_sha256: String,
}

#[derive(Clone)]
struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
    permissions: Option<fs::Permissions>,
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self {
                path,
                content: None,
                permissions: None,
            });
        }

        let content = fs::read(&path).map_err(|error| AppError::io(&path, error))?;
        let permissions = fs::metadata(&path)
            .map_err(|error| AppError::io(&path, error))?
            .permissions();
        Ok(Self {
            path,
            content: Some(content),
            permissions: Some(permissions),
        })
    }

    fn restore(&self) -> Result<(), AppError> {
        match &self.content {
            Some(content) => {
                crate::config::atomic_write(&self.path, content)?;
                if let Some(permissions) = &self.permissions {
                    fs::set_permissions(&self.path, permissions.clone())
                        .map_err(|error| AppError::io(&self.path, error))?;
                }
            }
            None if self.path.exists() => {
                fs::remove_file(&self.path).map_err(|error| AppError::io(&self.path, error))?;
            }
            None => {}
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct GrokLiveSnapshot {
    files: Vec<FileSnapshot>,
}

impl GrokLiveSnapshot {
    pub(crate) fn capture() -> Result<Self, AppError> {
        Ok(Self {
            files: vec![
                FileSnapshot::capture(get_grok_config_path())?,
                FileSnapshot::capture(get_grok_auth_path())?,
                FileSnapshot::capture(get_grok_image_auth_state_path())?,
            ],
        })
    }

    pub(crate) fn restore(&self) -> Result<(), AppError> {
        let mut errors = Vec::new();
        for file in &self.files {
            if let Err(error) = file.restore() {
                errors.push(error.to_string());
            }
        }
        if !errors.is_empty() {
            return Err(AppError::Message(errors.join("; ")));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokModelConfig {
    pub profile: String,
    pub model: String,
    pub base_url: String,
    pub name: String,
    pub api_key: Option<String>,
    pub env_key: Option<String>,
    pub api_backend: String,
    pub context_window: i64,
}

/// Grok Build configuration directory (`~/.grok`).
pub fn get_grok_config_dir() -> PathBuf {
    crate::settings::get_grok_override_dir().unwrap_or_else(|| get_home_dir().join(".grok"))
}

/// Grok Build live configuration path (`~/.grok/config.toml`).
pub fn get_grok_config_path() -> PathBuf {
    get_grok_config_dir().join("config.toml")
}

/// Grok Build credential store (`~/.grok/auth.json`).
pub fn get_grok_auth_path() -> PathBuf {
    get_grok_config_dir().join("auth.json")
}

fn get_grok_image_auth_state_path() -> PathBuf {
    get_grok_config_dir().join(IMAGE_AUTH_STATE_FILE)
}

fn set_owner_only_permissions(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| AppError::io(path, error))?;
    }
    Ok(())
}

fn write_owner_only_json(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    let content =
        serde_json::to_string_pretty(value).map_err(|source| AppError::JsonSerialize { source })?;
    write_text_file(path, &format!("{content}\n"))?;
    set_owner_only_permissions(path)
}

fn read_auth_root(path: &Path) -> Result<Map<String, Value>, AppError> {
    if !path.exists() {
        return Ok(Map::new());
    }

    let content = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(root)) => Ok(root),
        Ok(_) => Err(AppError::localized(
            "provider.grokbuild.auth.not_object",
            format!("Grok Build 凭据文件必须是 JSON 对象: {}", path.display()),
            format!(
                "Grok Build credential file must be a JSON object: {}",
                path.display()
            ),
        )),
        Err(source) => Err(AppError::json(path, source)),
    }
}

fn read_managed_auth_state(path: &Path) -> Result<Option<ManagedImageAuthState>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|source| AppError::json(path, source))
}

fn image_credential_files_are_readable() -> Result<(), AppError> {
    read_auth_root(&get_grok_auth_path())?;
    read_managed_auth_state(&get_grok_image_auth_state_path())?;
    Ok(())
}

fn api_key_sha256(api_key: &str) -> String {
    format!("{:x}", Sha256::digest(api_key.as_bytes()))
}

fn is_managed_image_scope(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_object)
        .and_then(|scope| scope.get("user_id"))
        .and_then(Value::as_str)
        == Some(IMAGE_AUTH_OWNER)
}

fn managed_image_scope_matches(value: Option<&Value>, state: &ManagedImageAuthState) -> bool {
    is_managed_image_scope(value)
        && value
            .and_then(Value::as_object)
            .and_then(|scope| scope.get("key"))
            .and_then(Value::as_str)
            .is_some_and(|key| api_key_sha256(key) == state.managed_key_sha256)
}

fn apply_image_credential_action(action: &ImageCredentialAction) -> Result<(), AppError> {
    let auth_path = get_grok_auth_path();
    let state_path = get_grok_image_auth_state_path();

    match action {
        ImageCredentialAction::SetManaged(api_key) => {
            let mut auth = read_auth_root(&auth_path)?;
            let current_scope = auth.get(IMAGE_AUTH_SCOPE).cloned();
            let existing_state = read_managed_auth_state(&state_path)?;
            let original_scope = match existing_state {
                Some(state) if managed_image_scope_matches(current_scope.as_ref(), &state) => {
                    state.original_scope
                }
                _ => current_scope,
            };
            let state = ManagedImageAuthState {
                original_scope,
                managed_key_sha256: api_key_sha256(api_key),
            };

            write_owner_only_json(&state_path, &state)?;
            auth.insert(
                IMAGE_AUTH_SCOPE.to_string(),
                json!({
                    "auth_mode": "api_key",
                    "key": api_key,
                    "create_time": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                    "user_id": IMAGE_AUTH_OWNER,
                }),
            );
            write_owner_only_json(&auth_path, &Value::Object(auth))
        }
        ImageCredentialAction::RestorePrevious => {
            let state = read_managed_auth_state(&state_path)?;
            if state.is_none() && !auth_path.exists() {
                return Ok(());
            }

            let mut auth = match read_auth_root(&auth_path) {
                Ok(auth) => auth,
                Err(error) if state.is_none() => {
                    log::warn!(
                        "Leaving invalid Grok auth.json unchanged because there is no CC Switch image credential state: {error}"
                    );
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            let current_is_managed = state
                .as_ref()
                .map(|state| managed_image_scope_matches(auth.get(IMAGE_AUTH_SCOPE), state))
                .unwrap_or_else(|| is_managed_image_scope(auth.get(IMAGE_AUTH_SCOPE)));
            let mut auth_changed = false;
            let mut remove_state = false;

            if let Some(state) = state {
                if current_is_managed {
                    match state.original_scope {
                        Some(original) => {
                            auth.insert(IMAGE_AUTH_SCOPE.to_string(), original);
                        }
                        None => {
                            auth.remove(IMAGE_AUTH_SCOPE);
                        }
                    }
                    auth_changed = true;
                    remove_state = true;
                }
            } else if current_is_managed {
                auth.remove(IMAGE_AUTH_SCOPE);
                auth_changed = true;
            }

            if auth_changed {
                write_owner_only_json(&auth_path, &Value::Object(auth))?;
            }
            if remove_state && state_path.exists() {
                fs::remove_file(&state_path).map_err(|error| AppError::io(&state_path, error))?;
            }
            Ok(())
        }
    }
}

pub fn sync_image_endpoints_in_config_toml(
    config_toml: &str,
    base_url: &str,
    enabled: bool,
) -> Result<String, AppError> {
    let mut document = config_toml
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })?;

    if enabled {
        let base_url = base_url.trim();
        if base_url.is_empty() {
            return Err(AppError::localized(
                "provider.grokbuild.field.missing",
                "Grok Build 配置缺少有效的 base_url 字段",
                "Grok Build configuration is missing a valid base_url field",
            ));
        }
        let image_model_override = |key: &str| {
            document
                .get("features")
                .and_then(|item| item.get(key))
                .and_then(toml_edit::Item::as_str)
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .unwrap_or(DEFAULT_IMAGE_MODEL)
                .to_string()
        };
        let image_gen_model = image_model_override("image_gen_model_override");
        let image_edit_model = image_model_override("image_edit_model_override");
        if document.get("endpoints").is_none() {
            document["endpoints"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let endpoints = document
            .get_mut("endpoints")
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| {
                AppError::localized(
                    "provider.grokbuild.endpoints.not_table",
                    "Grok Build 配置中的 endpoints 必须是表结构",
                    "Grok Build endpoints configuration must be a table",
                )
            })?;
        endpoints.insert("xai_api_base_url", toml_edit::value(base_url));

        if document.get("features").is_none() {
            document["features"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let features = document
            .get_mut("features")
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| {
                AppError::localized(
                    "provider.grokbuild.features.not_table",
                    "Grok Build 配置中的 features 必须是表结构",
                    "Grok Build features configuration must be a table",
                )
            })?;
        features.insert("image_gen", toml_edit::value(true));
        features.insert("image_edit", toml_edit::value(true));
        features.insert(
            "image_gen_model_override",
            toml_edit::value(&image_gen_model),
        );
        features.insert(
            "image_edit_model_override",
            toml_edit::value(&image_edit_model),
        );
    } else {
        if let Some(endpoints) = document
            .get_mut("endpoints")
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            endpoints.remove("xai_api_base_url");
            if endpoints.is_empty() {
                document.as_table_mut().remove("endpoints");
            }
        }
        if let Some(features) = document
            .get_mut("features")
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            features.remove("image_gen");
            features.remove("image_edit");
            features.remove("image_gen_model_override");
            features.remove("image_edit_model_override");
            if features.is_empty() {
                document.as_table_mut().remove("features");
            }
        }
    }

    Ok(document.to_string())
}

fn required_non_empty_string<'a>(
    table: &'a toml::value::Table,
    key: &str,
) -> Result<&'a str, AppError> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.field.missing",
                format!("Grok Build 配置缺少有效的 {key} 字段"),
                format!("Grok Build configuration is missing a valid {key} field"),
            )
        })
}

fn optional_non_empty_string(table: &toml::value::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// Syntax-only validation for a Grok Build config document (empty allowed).
///
/// 官方条目走 Grok CLI 自带的 xAI OAuth 登录，config.toml 不需要（通常也没有）
/// 自定义模型表：空文档合法，非空只要求 TOML 语法合法。live 层的读写与官方
/// 快照校验都用它；"必须有完整自定义模型表"的强校验见 `validate_config_toml`。
pub fn validate_config_toml_syntax(config_toml: &str) -> Result<(), AppError> {
    if config_toml.trim().is_empty() {
        return Ok(());
    }
    config_toml
        .parse::<toml::Value>()
        .map(|_| ())
        .map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })
}

/// Whether a live config document represents the official login state.
///
/// 官方态 = 语法合法且完全没有自定义模型痕迹（无 `[models]` 也无 `[model.*]`，
/// 允许 `[mcp_servers]` 等其它内容）。只要出现过任一自定义键就返回 false，
/// 让残缺的自定义配置继续走 `validate_config_toml` 报出真实错误，
/// 而不是被误判成官方态静默吞掉。语法不合法同样返回 false。
pub fn is_official_live_config(config_toml: &str) -> bool {
    let Ok(document) = config_toml.parse::<toml::Value>() else {
        return false;
    };
    document
        .as_table()
        .is_some_and(|root| !root.contains_key("models") && !root.contains_key("model"))
}

/// Validate the provider-owned Grok Build TOML document.
pub fn validate_config_toml(config_toml: &str) -> Result<(), AppError> {
    let document = config_toml.parse::<toml::Value>().map_err(|error| {
        AppError::localized(
            "provider.grokbuild.config.invalid_toml",
            format!("Grok Build config.toml 格式错误: {error}"),
            format!("Invalid Grok Build config.toml: {error}"),
        )
    })?;

    let root = document.as_table().ok_or_else(|| {
        AppError::localized(
            "provider.grokbuild.config.not_table",
            "Grok Build 配置必须是 TOML 表结构",
            "Grok Build configuration must be a TOML table",
        )
    })?;
    let models = root
        .get("models")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.models.missing",
                "Grok Build 配置缺少 [models]",
                "Grok Build configuration is missing [models]",
            )
        })?;
    let default_model = required_non_empty_string(models, "default")?;
    let model_entries = root
        .get("model")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.model.missing",
                "Grok Build 配置缺少 [model.<name>]",
                "Grok Build configuration is missing [model.<name>]",
            )
        })?;
    let selected_model = model_entries
        .get(default_model)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.default_model.missing",
                format!("Grok Build 配置缺少 [model.\"{default_model}\"]"),
                format!("Grok Build configuration is missing [model.\"{default_model}\"]"),
            )
        })?;

    required_non_empty_string(selected_model, "model")?;
    required_non_empty_string(selected_model, "base_url")?;
    required_non_empty_string(selected_model, "name")?;
    if optional_non_empty_string(selected_model, "api_key").is_none()
        && optional_non_empty_string(selected_model, "env_key").is_none()
    {
        return Err(AppError::localized(
            "provider.grokbuild.credentials.missing",
            "Grok Build 配置缺少有效的 api_key 或 env_key 字段",
            "Grok Build configuration is missing a valid api_key or env_key field",
        ));
    }
    required_non_empty_string(selected_model, "api_backend")?;

    selected_model
        .get("context_window")
        .and_then(toml::Value::as_integer)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.context_window.invalid",
                "Grok Build context_window 必须是正整数",
                "Grok Build context_window must be a positive integer",
            )
        })?;

    Ok(())
}

pub fn extract_model_config(config_toml: &str) -> Option<GrokModelConfig> {
    let document = config_toml.parse::<toml::Value>().ok()?;
    let root = document.as_table()?;
    let default_model = root
        .get("models")?
        .as_table()?
        .get("default")?
        .as_str()?
        .trim();
    let selected_model = root
        .get("model")?
        .as_table()?
        .get(default_model)?
        .as_table()?;
    Some(GrokModelConfig {
        profile: default_model.to_string(),
        model: selected_model.get("model")?.as_str()?.trim().to_string(),
        base_url: selected_model
            .get("base_url")?
            .as_str()?
            .trim_end_matches('/')
            .to_string(),
        name: selected_model.get("name")?.as_str()?.trim().to_string(),
        api_key: optional_non_empty_string(selected_model, "api_key"),
        env_key: optional_non_empty_string(selected_model, "env_key"),
        api_backend: selected_model
            .get("api_backend")?
            .as_str()?
            .trim()
            .to_string(),
        context_window: selected_model.get("context_window")?.as_integer()?,
    })
}

pub fn extract_credentials(config_toml: &str) -> Option<(String, String)> {
    let config = extract_model_config(config_toml)?;
    // Credentials only come from two explicit, config-declared sources:
    //   1. an inline `api_key`, or
    //   2. the process env var named by `env_key`.
    //
    // Deliberately NO unconditional fallback to `XAI_API_KEY`: silently
    // substituting a different account's key (when the declared `env_key` var is
    // unset) would leak that key to whatever `base_url` this config points at.
    // An unset/missing declared credential must surface as "no credential"
    // (None) so callers can fail loudly rather than transmit the wrong secret.
    let api_key = config.api_key.or_else(|| {
        config
            .env_key
            .as_deref()
            .and_then(|key| std::env::var(key).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })?;
    Some((config.base_url, api_key))
}

pub fn extract_inline_api_key(config_toml: &str) -> Option<String> {
    extract_model_config(config_toml)?.api_key
}

pub fn extract_base_url(config_toml: &str) -> Option<String> {
    Some(extract_model_config(config_toml)?.base_url)
}

fn update_selected_model_string(
    config_toml: &str,
    field: &str,
    value: &str,
) -> Result<String, AppError> {
    let mut document = config_toml
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            AppError::localized(
                "provider.grokbuild.config.invalid_toml",
                format!("Grok Build config.toml 格式错误: {error}"),
                format!("Invalid Grok Build config.toml: {error}"),
            )
        })?;
    let default_model = document
        .get("models")
        .and_then(|item| item.get("default"))
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.default_model.missing",
                "Grok Build 配置缺少 models.default",
                "Grok Build configuration is missing models.default",
            )
        })?
        .to_string();

    let selected_model = document
        .get_mut("model")
        .and_then(|item| item.get_mut(&default_model))
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.default_model.missing",
                format!("Grok Build 配置缺少 [model.\"{default_model}\"]"),
                format!("Grok Build configuration is missing [model.\"{default_model}\"]"),
            )
        })?;
    selected_model.insert(field, toml_edit::value(value));
    Ok(document.to_string())
}

pub fn apply_proxy_takeover(
    config_toml: &str,
    proxy_base_url: &str,
    token_placeholder: &str,
) -> Result<String, AppError> {
    let updated = update_selected_model_string(config_toml, "base_url", proxy_base_url)?;
    update_selected_model_string(&updated, "api_key", token_placeholder)
}

pub fn update_api_key(config_toml: &str, api_key: &str) -> Result<String, AppError> {
    update_selected_model_string(config_toml, "api_key", api_key)
}

pub fn has_proxy_placeholder(config_toml: &str, token_placeholder: &str) -> bool {
    extract_model_config(config_toml)
        .and_then(|config| config.api_key)
        .is_some_and(|api_key| api_key == token_placeholder)
}

pub fn base_url_matches(config_toml: &str, predicate: impl FnOnce(&str) -> bool) -> bool {
    extract_model_config(config_toml).is_some_and(|config| predicate(&config.base_url))
}

/// Remove MCP projections from a provider-owned Grok Build settings snapshot.
/// MCP servers are owned by the database and projected into live config.toml.
pub fn strip_grok_mcp_servers_from_settings(settings: &mut Value) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(());
    };
    if !config_text.contains("mcp") {
        return Ok(());
    }

    let mut document = config_text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| AppError::Message(format!("Invalid Grok Build config.toml: {error}")))?;
    let mut changed = document.as_table_mut().remove("mcp_servers").is_some();
    if let Some(mcp_table) = document
        .get_mut("mcp")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        if mcp_table.remove("servers").is_some() {
            changed = true;
        }
        if mcp_table.is_empty() {
            document.as_table_mut().remove("mcp");
        }
    }

    if changed {
        if let Some(object) = settings.as_object_mut() {
            object.insert("config".to_string(), Value::String(document.to_string()));
        }
    }
    Ok(())
}

/// Read the live `~/.grok/config.toml` as a provider settings snapshot.
///
/// 只做 TOML 语法校验：live 处于官方态（无自定义模型表）时同样需要能被
/// 读取，供切换回填与界面展示使用。需要"完整自定义模型配置"的导入路径
/// 由调用方自行叠加 `validate_config_toml`。
pub fn read_grok_live_settings() -> Result<Value, AppError> {
    let path = get_grok_config_path();
    if !path.exists() {
        return Err(AppError::localized(
            "grokbuild.config.missing",
            "Grok Build 配置文件不存在",
            "Grok Build configuration file not found",
        ));
    }

    let config = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    validate_config_toml_syntax(&config)?;
    Ok(json!({ "config": config }))
}

pub(crate) fn prepare_grok_provider_live(
    provider: &Provider,
) -> Result<PreparedGrokLive, AppError> {
    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.grokbuild.settings.not_object",
            "Grok Build 配置必须是 JSON 对象",
            "Grok Build configuration must be a JSON object",
        )
    })?;
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.config.missing",
                "Grok Build 配置缺少 config 字段",
                "Grok Build configuration is missing the config field",
            )
        })?;

    // 官方条目不注入自定义模型表：按快照原样写回（首次为空文件），
    // Grok CLI 回落到官方内置模型 + 自带 OAuth 登录；MCP 投影随后由
    // 切换流程重新补写。非官方供应商必须携带完整的自定义模型配置。
    let credential_files_ready = match image_credential_files_are_readable() {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "Disabling Grok image sync and leaving credentials unchanged because the credential files cannot be read safely: {error}"
            );
            false
        }
    };
    let is_official = provider.category.as_deref() == Some("official");
    let (config, credential_action) = if is_official {
        validate_config_toml_syntax(config)?;
        (
            config.to_string(),
            if credential_files_ready {
                Some(ImageCredentialAction::RestorePrevious)
            } else {
                None
            },
        )
    } else {
        validate_config_toml(config)?;
        let model = extract_model_config(config).ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.model.missing",
                "Grok Build 配置缺少可用的模型配置",
                "Grok Build configuration is missing a usable model configuration",
            )
        })?;
        let image_key = model.api_key.or_else(|| {
            model
                .env_key
                .as_deref()
                .and_then(|key| std::env::var(key).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        });
        match (image_key, credential_files_ready) {
            (Some(image_key), true) => (
                sync_image_endpoints_in_config_toml(config, &model.base_url, true)?,
                Some(ImageCredentialAction::SetManaged(image_key)),
            ),
            (image_key, _) => {
                if image_key.is_none() {
                    log::warn!(
                        "Disabling Grok image sync because no inline or configured environment key is available"
                    );
                }
                let credential_action = if credential_files_ready {
                    Some(ImageCredentialAction::RestorePrevious)
                } else {
                    None
                };
                log::warn!(
                    "Grok image endpoint and feature overrides will not be written for this provider switch"
                );
                (
                    sync_image_endpoints_in_config_toml(config, "", false)?,
                    credential_action,
                )
            }
        }
    };

    let mut prepared_settings = provider.settings_config.clone();
    prepared_settings["config"] = Value::String(config);
    Ok(PreparedGrokLive {
        settings: prepared_settings,
        credential_action,
    })
}

pub(crate) fn write_prepared_grok_live(
    prepared: &PreparedGrokLive,
) -> Result<GrokLiveSnapshot, AppError> {
    let snapshot = GrokLiveSnapshot::capture()?;
    let result = (|| {
        if let Some(action) = &prepared.credential_action {
            apply_image_credential_action(action)?;
        }
        write_grok_live_settings(&prepared.settings)
    })();

    if let Err(error) = result {
        if let Err(rollback_error) = snapshot.restore() {
            return Err(AppError::Message(format!(
                "{error}; additionally failed to restore the previous Grok Build live files: {rollback_error}"
            )));
        }
        return Err(error);
    }

    Ok(snapshot)
}

pub fn write_grok_provider_live(provider: &Provider) -> Result<(), AppError> {
    let prepared = prepare_grok_provider_live(provider)?;
    write_prepared_grok_live(&prepared).map(|_| ())
}

/// Raw live-file writer, mirroring `read_grok_live_settings` (syntax-only).
///
/// 代理接管的备份/恢复也走这里：官方态 live（无自定义模型表）必须可以
/// 原样写回。完整形状校验由 `write_grok_provider_live` 的非官方分支负责。
pub fn write_grok_live_settings(settings: &Value) -> Result<(), AppError> {
    let config = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.config.missing",
                "Grok Build 配置缺少 config 字段",
                "Grok Build configuration is missing the config field",
            )
        })?;
    validate_config_toml_syntax(config)?;
    write_text_file(&get_grok_config_path(), config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn valid_config() -> &'static str {
        r#"[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://example.com/v1"
name = "Example"
api_key = "secret"
api_backend = "responses"
context_window = 500000
"#
    }

    fn valid_env_key_config() -> &'static str {
        r#"[models]
default = "grok-env"

[model."grok-env"]
model = "grok-4.5"
base_url = "https://example.com/v1"
name = "Example Env"
env_key = "GROK_TEST_API_KEY"
api_backend = "responses"
context_window = 500000
"#
    }

    #[test]
    fn validates_expected_config_shape() {
        validate_config_toml(valid_config()).expect("valid Grok Build config");
        validate_config_toml(valid_env_key_config()).expect("valid env_key configuration");
    }

    #[test]
    fn syntax_validation_accepts_official_snapshots() {
        validate_config_toml_syntax("").expect("empty official snapshot");
        validate_config_toml_syntax("[mcp_servers.echo]\ncommand = \"echo\"\n")
            .expect("official-mode config without model tables");
        assert!(validate_config_toml_syntax("not = [valid").is_err());
    }

    #[test]
    fn official_live_config_detection() {
        // 官方态：完全没有自定义模型痕迹
        assert!(is_official_live_config(""));
        assert!(is_official_live_config("  \n# comment only\n"));
        assert!(is_official_live_config(
            "[mcp_servers.echo]\ncommand = \"echo\"\n"
        ));

        // 出现过任一自定义键（哪怕残缺）都不是官方态，交给强校验报错
        assert!(!is_official_live_config(valid_config()));
        assert!(!is_official_live_config("[models]\ndefault = \"x\"\n"));
        assert!(!is_official_live_config("[model.x]\nmodel = \"x\"\n"));

        // 语法不合法不是官方态
        assert!(!is_official_live_config("not = [valid"));
    }

    #[test]
    fn rejects_missing_selected_model_table() {
        let error = validate_config_toml("[models]\ndefault = \"grok-4.5\"\n")
            .expect_err("missing model table should fail");
        assert!(error.to_string().contains("model"));
    }

    #[test]
    fn rejects_config_without_api_key_or_env_key() {
        let config = valid_config().replace("api_key = \"secret\"\n", "");
        let error = validate_config_toml(&config).expect_err("credentials should be required");
        assert!(error.to_string().contains("api_key"));
        assert!(error.to_string().contains("env_key"));
    }

    #[test]
    fn extracts_selected_model_and_updates_takeover_fields() {
        let selected = extract_model_config(valid_config()).expect("selected model");
        assert_eq!(selected.profile, "grok-4.5");
        assert_eq!(selected.model, "grok-4.5");
        assert_eq!(selected.base_url, "https://example.com/v1");

        let updated = apply_proxy_takeover(
            valid_config(),
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("takeover config");
        let selected = extract_model_config(&updated).expect("updated selected model");
        assert_eq!(selected.base_url, "http://127.0.0.1:15721/grokbuild/v1");
        assert_eq!(selected.api_key.as_deref(), Some("PROXY_MANAGED"));
        assert!(has_proxy_placeholder(&updated, "PROXY_MANAGED"));
    }

    #[test]
    fn takeover_preserves_env_key_profile_and_injects_inline_placeholder() {
        let updated = apply_proxy_takeover(
            valid_env_key_config(),
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("takeover config");
        let selected = extract_model_config(&updated).expect("updated selected model");

        assert_eq!(selected.profile, "grok-env");
        assert_eq!(selected.env_key.as_deref(), Some("GROK_TEST_API_KEY"));
        assert_eq!(selected.api_key.as_deref(), Some("PROXY_MANAGED"));
    }

    #[test]
    fn image_sync_preserves_unrelated_config_and_proxy_takeover_keeps_upstream_media() {
        let config = format!(
            "{}\n[mcp_servers.echo]\ncommand = \"echo\"\n\n[features]\nunrelated = true\nimage_gen_model_override = \"custom-image-model\"\nimage_edit_model_override = \"custom-edit-model\"\n",
            valid_config()
        );
        let synced = sync_image_endpoints_in_config_toml(&config, "https://example.com/v1", true)
            .expect("sync image config");
        let document = synced.parse::<toml::Value>().expect("parse synced config");

        assert_eq!(
            document["endpoints"]["xai_api_base_url"].as_str(),
            Some("https://example.com/v1")
        );
        assert_eq!(document["features"]["image_gen"].as_bool(), Some(true));
        assert_eq!(
            document["features"]["image_gen_model_override"].as_str(),
            Some("custom-image-model")
        );
        assert_eq!(
            document["features"]["image_edit_model_override"].as_str(),
            Some("custom-edit-model")
        );
        assert_eq!(document["features"]["unrelated"].as_bool(), Some(true));
        assert_eq!(
            document["mcp_servers"]["echo"]["command"].as_str(),
            Some("echo")
        );

        let takeover = apply_proxy_takeover(
            &synced,
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("takeover config");
        let takeover_document = takeover
            .parse::<toml::Value>()
            .expect("parse takeover config");
        assert_eq!(
            takeover_document["model"]["grok-4.5"]["base_url"].as_str(),
            Some("http://127.0.0.1:15721/grokbuild/v1")
        );
        assert_eq!(
            takeover_document["endpoints"]["xai_api_base_url"].as_str(),
            Some("https://example.com/v1")
        );
    }

    #[test]
    #[serial]
    fn resolves_api_key_from_configured_environment_variable() {
        let original = std::env::var_os("GROK_TEST_API_KEY");
        std::env::set_var("GROK_TEST_API_KEY", "env-secret");

        let credentials = extract_credentials(valid_env_key_config()).expect("credentials");

        assert_eq!(credentials.0, "https://example.com/v1");
        assert_eq!(credentials.1, "env-secret");
        match original {
            Some(value) => std::env::set_var("GROK_TEST_API_KEY", value),
            None => std::env::remove_var("GROK_TEST_API_KEY"),
        }
    }

    /// 构造一个 `env_key` 指向未设置环境变量的 config——这是"声明了间接引用但
    /// 该变量不存在"的场景，修复前会静默兜底到 `XAI_API_KEY`。
    fn env_key_unset_config() -> &'static str {
        r#"[models]
default = "grok-env"

[model."grok-env"]
model = "grok-4.5"
base_url = "https://attacker.example/v1"
name = "Attacker Env"
env_key = "GROK_TEST_DEFINITELY_UNSET_VAR"
api_backend = "responses"
context_window = 500000
"#
    }

    #[test]
    #[serial]
    fn does_not_fall_back_to_xai_api_key_when_declared_env_key_is_unset() {
        // 即使进程里恰好设了 XAI_API_KEY，也不能被静默借用到别的 base_url 上。
        let original_xai = std::env::var_os("XAI_API_KEY");
        let original_unset = std::env::var_os("GROK_TEST_DEFINITELY_UNSET_VAR");
        std::env::set_var("XAI_API_KEY", "xai-secret-should-not-leak");
        std::env::remove_var("GROK_TEST_DEFINITELY_UNSET_VAR");

        let credentials = extract_credentials(env_key_unset_config());

        assert!(
            credentials.is_none(),
            "declared env_key unset must yield None, never a borrowed XAI_API_KEY; got {credentials:?}"
        );

        match original_xai {
            Some(value) => std::env::set_var("XAI_API_KEY", value),
            None => std::env::remove_var("XAI_API_KEY"),
        }
        match original_unset {
            Some(value) => std::env::set_var("GROK_TEST_DEFINITELY_UNSET_VAR", value),
            None => std::env::remove_var("GROK_TEST_DEFINITELY_UNSET_VAR"),
        }
    }

    #[test]
    #[serial]
    fn writes_env_key_to_image_auth_scope() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let original_api_key = std::env::var_os("GROK_TEST_API_KEY");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::set_var("GROK_TEST_API_KEY", "env-image-secret");

        let provider = Provider::with_id(
            "grok-env".to_string(),
            "Env Example".to_string(),
            json!({ "config": valid_env_key_config() }),
            None,
        );
        write_grok_provider_live(&provider).expect("write env-key provider");

        let auth: Value = serde_json::from_str(
            &fs::read_to_string(get_grok_auth_path()).expect("read env-key auth"),
        )
        .expect("parse env-key auth");
        assert_eq!(
            auth[IMAGE_AUTH_SCOPE]["key"].as_str(),
            Some("env-image-secret")
        );

        match original_api_key {
            Some(value) => std::env::set_var("GROK_TEST_API_KEY", value),
            None => std::env::remove_var("GROK_TEST_API_KEY"),
        }
        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn missing_env_key_disables_image_sync_and_restores_previous_scope() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let original_api_key = std::env::var_os("GROK_TEST_API_KEY");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::remove_var("GROK_TEST_API_KEY");

        write_text_file(
            &get_grok_auth_path(),
            r#"{"xai::api_key":{"auth_mode":"api_key","key":"user-key"}}"#,
        )
        .expect("seed user auth");
        apply_image_credential_action(&ImageCredentialAction::SetManaged(
            "previous-provider-key".to_string(),
        ))
        .expect("seed managed provider auth");

        let provider = Provider::with_id(
            "grok-env".to_string(),
            "Env Example".to_string(),
            json!({ "config": valid_env_key_config() }),
            None,
        );
        write_grok_provider_live(&provider).expect("write provider without resolved env key");

        let config = fs::read_to_string(get_grok_config_path()).expect("read env-key live config");
        let document = config.parse::<toml::Value>().expect("parse live config");
        assert!(document.get("endpoints").is_none());
        assert!(document.get("features").is_none());

        let auth: Value = serde_json::from_str(
            &fs::read_to_string(get_grok_auth_path()).expect("read restored auth"),
        )
        .expect("parse restored auth");
        assert_eq!(auth[IMAGE_AUTH_SCOPE]["key"].as_str(), Some("user-key"));
        assert!(!get_grok_image_auth_state_path().exists());

        match original_api_key {
            Some(value) => std::env::set_var("GROK_TEST_API_KEY", value),
            None => std::env::remove_var("GROK_TEST_API_KEY"),
        }
        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    fn strips_projected_mcp_servers_without_touching_model_config() {
        let mut settings = json!({
            "config": format!(
                "{}\n[mcp_servers.echo]\ncommand = \"echo\"\n",
                valid_config()
            )
        });

        strip_grok_mcp_servers_from_settings(&mut settings).expect("strip MCP servers");

        let config = settings.get("config").and_then(Value::as_str).unwrap();
        assert!(!config.contains("mcp_servers"));
        assert!(config.contains("model = \"grok-4.5\""));
        validate_config_toml(config).expect("stripped config remains valid");
    }

    #[test]
    #[serial]
    fn official_provider_roundtrips_without_custom_model_tables() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        // 官方条目：空 config 可写（清掉自定义模型表，交还 Grok CLI 官方登录）
        let mut official = Provider::with_id(
            "grokbuild-official".to_string(),
            "Grok Official".to_string(),
            json!({ "config": "" }),
            None,
        );
        official.category = Some("official".to_string());

        let auth_path = get_grok_auth_path();
        write_text_file(
            &auth_path,
            r#"{
  "xai::api_key": {"auth_mode": "api_key", "key": "user-secret"},
  "https://auth.x.ai::client": {"auth_mode": "oauth", "key": "oauth-secret"}
}"#,
        )
        .expect("seed auth");

        let custom = Provider::with_id(
            "custom".to_string(),
            "Custom".to_string(),
            json!({ "config": valid_config() }),
            None,
        );
        write_grok_provider_live(&custom).expect("write managed custom provider");
        let managed_auth: Value =
            serde_json::from_str(&fs::read_to_string(&auth_path).expect("read managed auth"))
                .expect("parse managed auth");
        assert_eq!(
            managed_auth[IMAGE_AUTH_SCOPE]["key"].as_str(),
            Some("secret")
        );
        assert!(get_grok_image_auth_state_path().exists());

        write_grok_provider_live(&official).expect("official empty config is writable");
        assert_eq!(
            fs::read_to_string(get_grok_config_path()).expect("read config"),
            ""
        );
        let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).expect("read auth"))
            .expect("parse auth");
        assert_eq!(auth[IMAGE_AUTH_SCOPE]["key"].as_str(), Some("user-secret"));
        assert_eq!(
            auth["https://auth.x.ai::client"]["key"].as_str(),
            Some("oauth-secret")
        );
        assert!(!get_grok_image_auth_state_path().exists());

        let official_snapshot =
            "[endpoints]\nxai_api_base_url = \"https://official.example/v1\"\n\n[features]\nimage_gen = false\n";
        official.settings_config = json!({ "config": official_snapshot });
        write_grok_provider_live(&official).expect("official snapshot is preserved");
        assert_eq!(
            fs::read_to_string(get_grok_config_path()).expect("read official snapshot"),
            official_snapshot
        );

        // 官方态 live（如 MCP 投影补写后）无自定义模型表，读取与原样写回都必须可用
        let official_live = "[mcp_servers.echo]\ncommand = \"echo\"\n";
        write_grok_live_settings(&json!({ "config": official_live }))
            .expect("official-mode live is writable for backup restore");
        let settings = read_grok_live_settings().expect("official-mode live is readable");
        assert_eq!(
            settings.get("config").and_then(Value::as_str),
            Some(official_live)
        );

        // 非官方供应商仍要求完整的自定义模型配置
        let invalid_custom = Provider::with_id(
            "custom".to_string(),
            "Custom".to_string(),
            json!({ "config": "" }),
            None,
        );
        assert!(write_grok_provider_live(&invalid_custom).is_err());

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn writes_and_reads_live_config() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let provider = Provider::with_id(
            "grok".to_string(),
            "Example".to_string(),
            json!({ "config": valid_config() }),
            None,
        );
        write_grok_provider_live(&provider).expect("write live config");

        let path = get_grok_config_path();
        assert_eq!(path, temp.path().join(".grok").join("config.toml"));
        let expected_config =
            sync_image_endpoints_in_config_toml(valid_config(), "https://example.com/v1", true)
                .expect("expected image config");
        assert_eq!(
            fs::read_to_string(path).expect("read config"),
            expected_config
        );
        assert_eq!(
            read_grok_live_settings()
                .expect("read live settings")
                .get("config")
                .and_then(Value::as_str),
            Some(expected_config.as_str())
        );
        let auth_path = get_grok_auth_path();
        let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).expect("read auth"))
            .expect("parse auth");
        assert_eq!(auth[IMAGE_AUTH_SCOPE]["key"].as_str(), Some("secret"));
        assert_eq!(
            auth[IMAGE_AUTH_SCOPE]["auth_mode"].as_str(),
            Some("api_key")
        );
        let auth_state =
            fs::read_to_string(get_grok_image_auth_state_path()).expect("read managed auth state");
        assert!(!auth_state.contains("secret"));
        let auth_state: ManagedImageAuthState =
            serde_json::from_str(&auth_state).expect("parse managed auth state");
        assert_eq!(auth_state.managed_key_sha256, api_key_sha256("secret"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [auth_path, get_grok_image_auth_state_path()] {
                assert_eq!(
                    fs::metadata(&path)
                        .expect("managed credential metadata")
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn image_credentials_restore_user_owned_scope_and_reject_invalid_auth() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        let auth_path = get_grok_auth_path();

        write_text_file(
            &auth_path,
            r#"{
  "xai::api_key": {"auth_mode":"api_key","key":"user-key"},
  "other::scope": {"auth_mode":"oauth","key":"preserve-me"}
}"#,
        )
        .expect("seed auth");
        apply_image_credential_action(&ImageCredentialAction::SetManaged("first-key".to_string()))
            .expect("first sync");
        apply_image_credential_action(&ImageCredentialAction::SetManaged("second-key".to_string()))
            .expect("second sync");

        let auth: Value =
            serde_json::from_str(&fs::read_to_string(&auth_path).expect("read merged auth"))
                .expect("parse merged auth");
        assert_eq!(auth["other::scope"]["key"].as_str(), Some("preserve-me"));
        assert_eq!(auth[IMAGE_AUTH_SCOPE]["key"].as_str(), Some("second-key"));
        assert_eq!(
            auth[IMAGE_AUTH_SCOPE]["user_id"].as_str(),
            Some(IMAGE_AUTH_OWNER)
        );

        apply_image_credential_action(&ImageCredentialAction::RestorePrevious)
            .expect("restore previous scope");
        let restored: Value =
            serde_json::from_str(&fs::read_to_string(&auth_path).expect("read restored auth"))
                .expect("parse restored auth");
        assert_eq!(restored[IMAGE_AUTH_SCOPE]["key"].as_str(), Some("user-key"));
        assert_eq!(
            restored["other::scope"]["key"].as_str(),
            Some("preserve-me")
        );
        assert!(!get_grok_image_auth_state_path().exists());

        apply_image_credential_action(&ImageCredentialAction::SetManaged("third-key".to_string()))
            .expect("third sync");
        let mut manually_edited: Value = serde_json::from_str(
            &fs::read_to_string(&auth_path).expect("read auth before manual edit"),
        )
        .expect("parse auth before manual edit");
        manually_edited[IMAGE_AUTH_SCOPE]["key"] = json!("user-took-over");
        write_owner_only_json(&auth_path, &manually_edited).expect("write manual auth edit");

        apply_image_credential_action(&ImageCredentialAction::RestorePrevious)
            .expect("leave manually edited scope");
        let manually_preserved: Value = serde_json::from_str(
            &fs::read_to_string(&auth_path).expect("read manually preserved auth"),
        )
        .expect("parse manually preserved auth");
        assert_eq!(
            manually_preserved[IMAGE_AUTH_SCOPE]["key"].as_str(),
            Some("user-took-over")
        );
        assert!(get_grok_image_auth_state_path().exists());

        apply_image_credential_action(&ImageCredentialAction::RestorePrevious)
            .expect("leave manually edited scope on repeated restore");
        let repeatedly_preserved: Value = serde_json::from_str(
            &fs::read_to_string(&auth_path).expect("read repeatedly preserved auth"),
        )
        .expect("parse repeatedly preserved auth");
        assert_eq!(
            repeatedly_preserved[IMAGE_AUTH_SCOPE]["key"].as_str(),
            Some("user-took-over")
        );
        assert!(get_grok_image_auth_state_path().exists());

        write_text_file(&auth_path, "{not-json").expect("seed invalid auth");
        assert!(
            apply_image_credential_action(&ImageCredentialAction::SetManaged(
                "recovered-key".to_string()
            ))
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(&auth_path).expect("read unchanged invalid auth"),
            "{not-json"
        );
        assert!(get_grok_image_auth_state_path().exists());

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}
