use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use crate::config::{
    atomic_write_private, delete_file, get_home_dir, read_json_file, write_text_file,
};
use crate::error::AppError;
use crate::provider::Provider;

pub const DEFAULT_MODEL: &str = "grok-4.5";
pub const DEFAULT_API_BACKEND: &str = "responses";
pub const DEFAULT_CONTEXT_WINDOW: i64 = 500_000;

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

/// Grok Build live credential path (`~/.grok/auth.json`).
///
/// Official login stores an OIDC/session map here. Each official provider
/// snapshots this file so multiple Grok accounts can be switched like Codex.
pub fn get_grok_auth_path() -> PathBuf {
    get_grok_config_dir().join("auth.json")
}

/// SuperGrok (OIDC) scope prefix used as the live auth.json map key.
pub const GROK_OIDC_SCOPE_PREFIX: &str = "https://auth.x.ai::";
/// Legacy `grok login` session scope.
pub const GROK_LEGACY_SESSION_SCOPE: &str = "https://accounts.x.ai/sign-in";

fn is_grok_oauth_scope(scope: &str) -> bool {
    scope.starts_with(GROK_OIDC_SCOPE_PREFIX)
        || scope == GROK_LEGACY_SESSION_SCOPE
        || scope.contains("/sign-in")
}

fn grok_oauth_entry_has_material(entry: &Value) -> bool {
    let Some(obj) = entry.as_object() else {
        return false;
    };
    ["key", "refresh_token"].iter().any(|field| {
        obj.get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    })
}

/// Whether stored/live Grok `auth.json` carries an official login session.
///
/// Grok prefers a session token over `api_key` in config.toml, so a leftover
/// OAuth scope silently keeps the previous account active after a switch.
pub fn grok_auth_has_login_material(auth: &Value) -> bool {
    auth.as_object().is_some_and(|root| {
        root.iter().any(|(scope, entry)| {
            is_grok_oauth_scope(scope) && grok_oauth_entry_has_material(entry)
        })
    })
}

fn grok_oauth_identity(auth: &Value) -> Option<String> {
    let root = auth.as_object()?;
    for (scope, entry) in root {
        if !is_grok_oauth_scope(scope) || !grok_oauth_entry_has_material(entry) {
            continue;
        }
        let obj = entry.as_object()?;
        for key in ["user_id", "principal_id", "email"] {
            if let Some(value) = obj
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(format!("{key}:{value}"));
            }
        }
    }
    None
}

pub fn grok_auth_same_identity(left: &Value, right: &Value) -> bool {
    match (grok_oauth_identity(left), grok_oauth_identity(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Prefer live tokens when they belong to the same official account as `stored`.
pub fn adopt_live_grok_auth(stored: &Value, live: &Value) -> Value {
    if grok_auth_has_login_material(live) && grok_auth_same_identity(stored, live) {
        live.clone()
    } else {
        stored.clone()
    }
}

/// Outcome of parking live Grok OAuth onto a stored official card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokOAuthMergeResult {
    /// Live auth was written into settings.
    Applied,
    /// Settings already hold this exact live snapshot — already preserved.
    AlreadyIdentical,
    /// A different official account is stored and must not be overwritten.
    IdentityConflict,
    /// Nothing to copy: no live login material, or settings is not an object.
    Skipped,
}

impl GrokOAuthMergeResult {
    /// Settings were mutated and should be persisted.
    pub fn applied(self) -> bool {
        matches!(self, Self::Applied)
    }

    /// Live OAuth is (now) on this card, so it is safe to strip live scopes.
    pub fn preserved(self) -> bool {
        matches!(self, Self::Applied | Self::AlreadyIdentical)
    }
}

/// Copy live OAuth into `settings`.
///
/// `bind_new_account` applies to the current card: a newer `grok login` must
/// not be overwritten by the stored snapshot. Without it, a different account
/// already saved on a card is left untouched (used when parking live auth on
/// the official seed before a third-party takeover).
///
/// `AlreadyIdentical` is successful preservation, not a failure: `switch_normal`
/// may have just backfilled this exact snapshot onto the seeded official row.
pub fn merge_live_grok_oauth_into_settings(
    settings: &mut Value,
    live_auth: &Value,
    bind_new_account: bool,
) -> GrokOAuthMergeResult {
    if !grok_auth_has_login_material(live_auth) {
        return GrokOAuthMergeResult::Skipped;
    }
    let stored = settings.get("auth").cloned().unwrap_or_else(|| json!({}));
    if grok_auth_has_login_material(&stored) && !grok_auth_same_identity(&stored, live_auth) {
        if !bind_new_account {
            return GrokOAuthMergeResult::IdentityConflict;
        }
    } else if stored == *live_auth {
        return GrokOAuthMergeResult::AlreadyIdentical;
    }
    if let Some(object) = settings.as_object_mut() {
        object.insert("auth".to_string(), live_auth.clone());
        return GrokOAuthMergeResult::Applied;
    }
    GrokOAuthMergeResult::Skipped
}

/// Drop OIDC / legacy session scopes, keeping unrelated scopes (e.g. API keys).
pub fn strip_grok_oauth_scopes(auth: &Value) -> Value {
    let Some(root) = auth.as_object() else {
        return json!({});
    };
    let kept: serde_json::Map<String, Value> = root
        .iter()
        .filter(|(scope, _)| !is_grok_oauth_scope(scope))
        .map(|(scope, entry)| (scope.clone(), entry.clone()))
        .collect();
    Value::Object(kept)
}

pub fn read_grok_auth() -> Result<Value, AppError> {
    let path = get_grok_auth_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    read_json_file(&path)
}

fn write_grok_auth_file(auth: &Value) -> Result<(), AppError> {
    let path = get_grok_auth_path();
    if auth.is_null() || auth.as_object().is_some_and(serde_json::Map::is_empty) {
        return delete_file(&path);
    }
    if !auth.is_object() {
        return Err(AppError::localized(
            "provider.grokbuild.auth.not_object",
            "Grok Build auth.json 必须是 JSON 对象",
            "Grok Build auth.json must be a JSON object",
        ));
    }
    let json = serde_json::to_string_pretty(auth)
        .map_err(|error| AppError::JsonSerialize { source: error })?;
    atomic_write_private(&path, json.as_bytes())
}

/// Atomically write `config.toml` and optionally `auth.json`.
///
/// `auth = None` leaves the live credential file untouched (empty official
/// cards and syntax-only backup restores). Empty `{}` deletes the file so
/// Grok CLI falls back to `api_key` / a fresh `grok login`.
pub fn write_grok_live_atomic(auth: Option<&Value>, config: &str) -> Result<(), AppError> {
    validate_config_toml_syntax(config)?;
    let auth_path = get_grok_auth_path();
    let old_auth = if auth.is_some() && auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|error| AppError::io(&auth_path, error))?)
    } else {
        None
    };

    if let Some(auth) = auth {
        write_grok_auth_file(auth)?;
    }

    if let Err(error) = write_text_file(&get_grok_config_path(), config) {
        if auth.is_some() {
            if let Some(bytes) = old_auth {
                let _ = atomic_write_private(&auth_path, &bytes);
            } else {
                let _ = delete_file(&auth_path);
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Remove official OAuth scopes from live `auth.json` so config.toml `api_key`
/// (or a later `grok login`) is what Grok CLI actually uses.
pub fn strip_grok_oauth_from_live_auth() -> Result<bool, AppError> {
    let path = get_grok_auth_path();
    if !path.exists() {
        return Ok(false);
    }
    let live = read_json_file::<Value>(&path)?;
    if !grok_auth_has_login_material(&live) {
        return Ok(false);
    }
    write_grok_auth_file(&strip_grok_oauth_scopes(&live))?;
    Ok(true)
}

/// After switching to an official provider that has no stored login, drop the
/// live OAuth session. Callers must only invoke this after a successful
/// backfill — that DB copy is the only other snapshot of the previous account.
pub fn clear_grok_oauth_live_auth_after_official_switch(db_auth: &Value) -> Result<bool, AppError> {
    if grok_auth_has_login_material(db_auth) {
        return Ok(false);
    }
    strip_grok_oauth_from_live_auth()
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
/// 官方凭据存在 `settings_config.auth`，切换时写回 `~/.grok/auth.json`。
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
    let updated = update_selected_model_string(&updated, "api_key", token_placeholder)?;
    update_selected_model_string(&updated, "api_backend", DEFAULT_API_BACKEND)
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

/// Read the live `~/.grok/config.toml` and `auth.json` as a provider snapshot.
///
/// 只做 TOML 语法校验：live 处于官方态（无自定义模型表）时同样需要能被
/// 读取，供切换回填与界面展示使用。需要"完整自定义模型配置"的导入路径
/// 由调用方自行叠加 `validate_config_toml`。缺失的 `auth.json` 折叠为 `{}`。
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
    let auth = read_grok_auth()?;
    Ok(json!({ "auth": auth, "config": config }))
}

pub fn write_grok_provider_live(provider: &Provider) -> Result<(), AppError> {
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
    if let Some(auth) = settings.get("auth") {
        if !auth.is_object() {
            return Err(AppError::localized(
                "provider.grokbuild.auth.not_object",
                "Grok Build auth 配置必须是 JSON 对象",
                "Grok Build auth configuration must be a JSON object",
            ));
        }
    }
    let auth = settings.get("auth").cloned().unwrap_or_else(|| json!({}));

    // 官方条目不注入自定义模型表：按快照原样写回（首次为空文件），
    // Grok CLI 回落到官方内置模型 + 自带 OAuth 登录；MCP 投影随后由
    // 切换流程重新补写。带登录材料的官方供应商同时写回 auth.json，
    // 才能在多账号之间切换。同一账号的 live 刷新优先于库存快照。
    // 非官方供应商只写 config.toml；清 OAuth 必须由调用方在把 live
    // 登录回填/快照到官方卡之后再做，否则会丢掉唯一一份 grok login。
    if provider.category.as_deref() != Some("official") {
        validate_config_toml(config)?;
        write_grok_live_atomic(None, config)?;
        return Ok(());
    }

    validate_config_toml_syntax(config)?;
    if grok_auth_has_login_material(&auth) {
        let live = read_grok_auth().unwrap_or_else(|_| json!({}));
        let auth_to_write = adopt_live_grok_auth(&auth, &live);
        write_grok_live_atomic(Some(&auth_to_write), config)
    } else {
        write_grok_live_atomic(None, config)
    }
}

/// Raw live-file writer, mirroring `read_grok_live_settings` (syntax-only).
///
/// 代理接管的备份/恢复也走这里：官方态 live（无自定义模型表）必须可以
/// 原样写回。完整形状校验由 `write_grok_provider_live` 的非官方分支负责。
/// `auth` 缺省时不碰 live `auth.json`，兼容旧备份。
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
    write_grok_live_atomic(settings.get("auth"), config)
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
        let direct_config = valid_env_key_config().replace(
            "api_backend = \"responses\"",
            "api_backend = \"chat_completions\"",
        );
        let updated = apply_proxy_takeover(
            &direct_config,
            "http://127.0.0.1:15721/grokbuild/v1",
            "PROXY_MANAGED",
        )
        .expect("takeover config");
        let selected = extract_model_config(&updated).expect("updated selected model");

        assert_eq!(selected.profile, "grok-env");
        assert_eq!(selected.env_key.as_deref(), Some("GROK_TEST_API_KEY"));
        assert_eq!(selected.api_key.as_deref(), Some("PROXY_MANAGED"));
        assert_eq!(selected.api_backend, DEFAULT_API_BACKEND);
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
        write_grok_provider_live(&official).expect("official empty config is writable");
        assert_eq!(
            fs::read_to_string(get_grok_config_path()).expect("read config"),
            ""
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
        let custom = Provider::with_id(
            "custom".to_string(),
            "Custom".to_string(),
            json!({ "config": "" }),
            None,
        );
        assert!(write_grok_provider_live(&custom).is_err());

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
        assert_eq!(
            fs::read_to_string(path).expect("read config"),
            valid_config()
        );
        assert_eq!(
            read_grok_live_settings()
                .expect("read live settings")
                .get("config")
                .and_then(Value::as_str),
            Some(valid_config())
        );

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    fn oidc_auth(token: &str, email: &str) -> Value {
        json!({
            "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
                "key": token,
                "auth_mode": "oidc",
                "email": email,
                "user_id": email,
                "refresh_token": format!("{token}-refresh"),
                "oidc_issuer": "https://auth.x.ai",
                "oidc_client_id": "b1a00492-073a-47ea-816f-4c329264a828"
            }
        })
    }

    #[test]
    fn merge_distinguishes_identical_snapshot_from_identity_conflict() {
        let live = oidc_auth("session-token", "a@example.com");
        let mut identical = json!({ "auth": live.clone(), "config": "" });
        assert_eq!(
            merge_live_grok_oauth_into_settings(&mut identical, &live, false),
            GrokOAuthMergeResult::AlreadyIdentical
        );

        let other_auth = oidc_auth("other-token", "b@example.com");
        let mut other = json!({ "auth": other_auth.clone(), "config": "" });
        assert_eq!(
            merge_live_grok_oauth_into_settings(&mut other, &live, false),
            GrokOAuthMergeResult::IdentityConflict
        );
        assert_eq!(
            other.get("auth"),
            Some(&other_auth),
            "identity conflict must leave the stored account untouched"
        );
    }

    #[test]
    fn detects_oauth_login_material_and_strips_only_session_scopes() {
        let auth = json!({
            "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
                "key": "session-token",
                "refresh_token": "refresh"
            },
            "xai::api_key": { "key": "image-key" }
        });
        assert!(grok_auth_has_login_material(&auth));
        assert_eq!(
            strip_grok_oauth_scopes(&auth),
            json!({ "xai::api_key": { "key": "image-key" } })
        );
        assert!(!grok_auth_has_login_material(&json!({})));
        assert!(!grok_auth_has_login_material(&json!({
            "xai::api_key": { "key": "image-key" }
        })));
    }

    #[test]
    #[serial]
    fn official_provider_writes_and_reads_auth_json() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let auth = oidc_auth("account-a-token", "a@example.com");
        let mut official = Provider::with_id(
            "grokbuild-official".to_string(),
            "Grok Official".to_string(),
            json!({ "auth": auth, "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        write_grok_provider_live(&official).expect("write official auth");

        let live_auth: Value =
            serde_json::from_str(&fs::read_to_string(get_grok_auth_path()).expect("read auth"))
                .expect("parse auth");
        let scope = "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";
        assert_eq!(
            live_auth.get(scope).and_then(|entry| entry.get("key")),
            auth.get(scope).and_then(|entry| entry.get("key"))
        );

        let settings = read_grok_live_settings().expect("read live settings");
        assert_eq!(settings.get("auth"), Some(&auth));
        assert_eq!(settings.get("config").and_then(Value::as_str), Some(""));

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn third_party_write_does_not_strip_oauth_without_caller_preserve() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let session = oidc_auth("session-token", "a@example.com");
        write_grok_live_atomic(Some(&session), "").expect("seed official session");

        let custom = Provider::with_id(
            "relay".to_string(),
            "Relay".to_string(),
            json!({ "config": valid_config() }),
            None,
        );
        write_grok_provider_live(&custom).expect("write third-party live");

        let live_auth: Value =
            serde_json::from_str(&fs::read_to_string(get_grok_auth_path()).expect("read auth"))
                .expect("parse auth");
        assert_eq!(
            live_auth, session,
            "stripping OAuth is the caller's job after snapshotting the live login"
        );
        assert_eq!(
            fs::read_to_string(get_grok_config_path()).expect("read config"),
            valid_config()
        );

        strip_grok_oauth_from_live_auth().expect("caller may strip after preserve");
        assert!(
            !get_grok_auth_path().exists(),
            "explicit strip still removes oauth so config.toml api_key wins"
        );

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn official_write_adopts_live_refresh_for_the_same_account() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let live = oidc_auth("refreshed-token", "a@example.com");
        write_grok_live_atomic(Some(&live), "").expect("seed refreshed live auth");

        let mut official = Provider::with_id(
            "official-a".to_string(),
            "Grok Official A".to_string(),
            json!({
                "auth": oidc_auth("stale-token", "a@example.com"),
                "config": ""
            }),
            None,
        );
        official.category = Some("official".to_string());
        write_grok_provider_live(&official).expect("write official");

        let written = read_grok_auth().expect("read adopted auth");
        assert_eq!(
            written
                .get("https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828")
                .and_then(|entry| entry.get("key"))
                .and_then(Value::as_str),
            Some("refreshed-token"),
            "same-account live refresh must win over the stored snapshot"
        );

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn empty_official_write_leaves_live_auth_until_explicit_clear() {
        let temp = TempDir::new().expect("temp dir");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let auth = oidc_auth("keep-me", "a@example.com");
        write_grok_live_atomic(Some(&auth), "").expect("seed live auth");

        let mut official = Provider::with_id(
            "official-b".to_string(),
            "Grok Official B".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        write_grok_provider_live(&official).expect("empty official is config-only");

        let live_auth: Value =
            serde_json::from_str(&fs::read_to_string(get_grok_auth_path()).expect("read auth"))
                .expect("parse auth");
        assert_eq!(
            live_auth, auth,
            "config-only official write must not clobber live auth"
        );

        clear_grok_oauth_live_auth_after_official_switch(&json!({})).expect("clear after backfill");
        assert!(
            !get_grok_auth_path().exists(),
            "empty official after backfill should drop the live session so grok login can start a new account"
        );

        match original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}
