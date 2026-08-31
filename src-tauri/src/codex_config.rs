use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{
    atomic_write, delete_file, get_home_dir, path_is_within, read_json_file,
    sanitize_provider_name, write_json_file, write_text_file,
};
use crate::error::AppError;
use crate::model_capabilities::{image_input_capability_from_modalities, ImageInputCapability};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::process::{Command, Stdio};
use toml_edit::DocumentMut;

pub const CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "custom";
/// Temporary model-provider id used while the built-in `codex-official`
/// provider is routed through CC Switch.  A dedicated id is an ownership
/// marker: unlike a generic localhost `base_url`, it can be detected and
/// cleaned up without mistaking a user's own local provider for takeover.
pub const CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID: &str = "cc-switch-official";
pub const CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME: &str = "cc-switch-model-catalog.json";
const CODEX_PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// Generating a ProxyChat catalog only needs one stable Codex model template per
// process. Without this cache every provider switch/takeover can start the
// Codex CLI again, which is especially expensive for npm-installed `codex.cmd`
// on Windows. Tests deliberately bypass the global cache because they isolate
// CODEX_HOME and seed different model templates.
#[cfg(not(test))]
static CODEX_MODEL_CATALOG_TEMPLATE_CACHE: OnceCell<Value> = OnceCell::new();

/// Top-level `config.toml` key that controls Codex's built-in web-search tool.
pub(crate) const CODEX_WEB_SEARCH_FIELD: &str = "web_search";
/// Value that disables the web-search tool. Some native `/responses` gateways
/// reject a `web_search` tool with `responses_feature_not_supported` ("tool type
/// 'web_search' is not supported by this gateway phase"), so for those we write
/// this per the vendors' official Codex docs. Also doubles as cc-switch's
/// ownership sentinel: we only ever remove a `web_search` key whose value equals
/// this string, never a user's own setting.
pub(crate) const CODEX_WEB_SEARCH_DISABLED: &str = "disabled";

/// Native `/responses` gateways whose first-party models do NOT support the Codex
/// `web_search` hosted tool. A BLACKLIST (default-on): everything not listed keeps
/// Codex's default, so relays/aggregators fronting real GPT — and any unknown
/// provider — are never touched. This avoids a whitelist's dangerous failure mode
/// (a fragile "is this GPT?" heuristic wrongly keeping web_search ON → hard 400);
/// the blacklist's failure mode is the safe, recoverable one (a not-yet-listed
/// broken gateway errors once → add it here).
///
/// Matched two ways so an aggregator (e.g. SiliconFlow) fronting these vendors'
/// models is also caught:
/// - `base_url` host substring, and
/// - the model id's brand prefix (after stripping any `vendor/` path segment).
///
/// Verified 2026-06-28 doc audit — reject: MiMo (hard 400), LongCat (official
/// config ships `web_search = "disabled"`), MiniMax (tool-type enum `['function']`
/// only), and Qwen3-Coder models (百炼 marks built-in tools unsupported for
/// the coder series). Deliberately NOT listed by host: 火山方舟豆包, general
/// 阿里百炼 Qwen models that support built-in web_search, and GPT-native relays.
const CODEX_WEB_SEARCH_REJECT_HOSTS: &[&str] = &[
    "xiaomimimo.com", // Xiaomi MiMo (api.xiaomimimo.com, token-plan-cn.xiaomimimo.com)
    "longcat.chat",   // Meituan LongCat (api.longcat.chat)
    "minimax.io",     // MiniMax global (api.minimax.io)
    "minimaxi.com",   // MiniMax CN (api.minimaxi.com)
];

/// Brand prefixes of models whose native gateways reject `web_search`, matched
/// against the model id's last `/`-segment so aggregator ids like
/// `MiniMaxAI/MiniMax-M3` are caught. Exact brand names (not a fuzzy heuristic),
/// so a supporting gateway is never wrongly matched.
const CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES: &[&str] =
    &["mimo", "longcat", "minimax", "qwen3-coder"];

/// Top-level `model` id from a Codex `config.toml`.
fn codex_top_level_model(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get("model")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Whether a native `/responses` provider's gateway is known to reject the Codex
/// `web_search` hosted tool — by `base_url` host OR by the active model's brand
/// (so an aggregator fronting a reject vendor's model is caught too). Driven by
/// the live `config.toml`, so it applies to existing providers without a re-save.
fn codex_native_gateway_rejects_web_search(config_text: &str) -> bool {
    if let Some(base_url) = extract_codex_base_url(config_text) {
        let base_url = base_url.to_ascii_lowercase();
        if CODEX_WEB_SEARCH_REJECT_HOSTS
            .iter()
            .any(|host| base_url.contains(host))
        {
            return true;
        }
    }
    if let Some(model) = codex_top_level_model(config_text) {
        let model = model.to_ascii_lowercase();
        // Strip any aggregator "vendor/" prefix, e.g. "MiniMaxAI/MiniMax-M3"
        // or "qwen/qwen3-coder-plus".
        let model = model.rsplit('/').next().unwrap_or(model.as_str());
        if CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES
            .iter()
            .any(|prefix| model.starts_with(prefix))
        {
            return true;
        }
    }
    false
}
const CODEX_MODEL_CATALOG_TEMPLATE_SLUG: &str = "gpt-5.5";
const CODEX_MANAGED_OAUTH_LIVE_AUTH_MARKER_FILENAME: &str = "codex_managed_oauth_live_auth.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexManagedOAuthLiveAuthMarker {
    version: u32,
    /// cc-switch 本地托管账号 ID，用于区分同一 ChatGPT workspace 下的登录。
    account_id: String,
    /// 原生 auth.json 的 `tokens.account_id`，即 ChatGPT workspace ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chatgpt_account_id: Option<String>,
    /// id_token 中跨刷新稳定的用户身份，防止同 workspace 的原生登录串号。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_identity: Option<String>,
}

pub(crate) struct CodexManagedLiveRefresh {
    pub(crate) refresh_token: String,
    pub(crate) id_token: Option<String>,
    pub(crate) last_refresh_ms: Option<i64>,
    pub(crate) chatgpt_account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexLiveFileState {
    path: PathBuf,
    contents: Option<Vec<u8>>,
    #[cfg(unix)]
    mode: Option<u32>,
}

impl CodexLiveFileState {
    fn capture(path: PathBuf) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self {
                path,
                contents: None,
                #[cfg(unix)]
                mode: None,
            });
        }

        let contents = fs::read(&path).map_err(|error| AppError::io(&path, error))?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            Some(
                fs::metadata(&path)
                    .map_err(|error| AppError::io(&path, error))?
                    .permissions()
                    .mode(),
            )
        };

        Ok(Self {
            path,
            contents: Some(contents),
            #[cfg(unix)]
            mode,
        })
    }

    fn restore(&self) -> Result<(), AppError> {
        match self.contents.as_deref() {
            Some(contents) => {
                atomic_write(&self.path, contents)?;
                #[cfg(unix)]
                if let Some(mode) = self.mode {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&self.path, fs::Permissions::from_mode(mode))
                        .map_err(|error| AppError::io(&self.path, error))?;
                }
                Ok(())
            }
            None => delete_file(&self.path),
        }
    }
}

/// Rollback point for the cc-switch-owned model catalog. Catalog projection
/// writes this file before the caller commits `config.toml`, so guarded restore
/// paths use this snapshot when a concurrently changing `auth.json` cancels the
/// commit.
pub(crate) struct CodexModelCatalogFileSnapshot(CodexLiveFileState);

impl CodexModelCatalogFileSnapshot {
    pub(crate) fn capture() -> Result<Self, AppError> {
        CodexLiveFileState::capture(get_codex_model_catalog_path()).map(Self)
    }

    pub(crate) fn restore(&self) -> Result<(), AppError> {
        self.0.restore()
    }
}

/// Exact rollback state for a managed Codex live write. The generated catalog
/// and ownership marker are part of the same logical commit as auth/config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexLiveStateSnapshot {
    auth: CodexLiveFileState,
    config: CodexLiveFileState,
    catalog: CodexLiveFileState,
    managed_marker: CodexLiveFileState,
}

impl CodexLiveStateSnapshot {
    pub(crate) fn capture() -> Result<Self, AppError> {
        Ok(Self {
            auth: CodexLiveFileState::capture(get_codex_auth_path())?,
            config: CodexLiveFileState::capture(get_codex_config_path())?,
            catalog: CodexLiveFileState::capture(get_codex_model_catalog_path())?,
            managed_marker: CodexLiveFileState::capture(
                get_codex_managed_oauth_live_auth_marker_path(),
            )?,
        })
    }

    /// Roll back config/catalog exactly while retaining a demonstrably newer
    /// ChatGPT auth generation for the same account. OAuth refresh can advance
    /// auth.json after a provider transaction captures its snapshot; restoring
    /// that snapshot blindly would invalidate the CLI's newly rotated token.
    ///
    /// Cross-account writes are still rolled back exactly: an A -> B transaction
    /// that fails must restore A even if B refreshed while it was briefly live.
    /// The marker follows auth as one generation bundle.
    pub(crate) fn restore_preserving_newer_same_account_auth(&self) -> Result<(), AppError> {
        let mut failures = Vec::new();
        let current_auth = match CodexLiveFileState::capture(get_codex_auth_path()) {
            Ok(state) => Some(state),
            Err(error) => {
                // Inspection failure must not prevent config/catalog and the
                // remaining rollback files from being attempted.
                failures.push(format!("inspect current auth: {error}"));
                None
            }
        };
        let current_marker =
            match CodexLiveFileState::capture(get_codex_managed_oauth_live_auth_marker_path()) {
                Ok(state) => Some(state),
                Err(error) => {
                    failures.push(format!("inspect current managed marker: {error}"));
                    None
                }
            };
        let snapshot_generation = Self::chatgpt_auth_generation(&self.auth, &self.managed_marker);
        let current_generation = current_auth
            .as_ref()
            .zip(current_marker.as_ref())
            .and_then(|(auth, marker)| Self::chatgpt_auth_generation(auth, marker));
        let preserve_current_auth = match (snapshot_generation, current_generation) {
            (Some((snapshot_account, snapshot_time)), Some((current_account, current_time)))
                if snapshot_account == current_account =>
            {
                match (snapshot_time, current_time) {
                    (Some(snapshot_time), Some(current_time)) => current_time > snapshot_time,
                    (None, Some(_)) => true,
                    _ => false,
                }
            }
            _ => false,
        };

        for (label, state) in [("catalog", &self.catalog), ("config", &self.config)] {
            if let Err(error) = state.restore() {
                failures.push(format!("{label}: {error}"));
            }
        }
        if !preserve_current_auth {
            for (label, state) in [
                ("auth", &self.auth),
                ("managed marker", &self.managed_marker),
            ] {
                if let Err(error) = state.restore() {
                    failures.push(format!("{label}: {error}"));
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "恢复 Codex Live 状态失败: {}",
                failures.join("; ")
            )))
        }
    }

    fn chatgpt_auth_generation(
        auth_state: &CodexLiveFileState,
        marker_state: &CodexLiveFileState,
    ) -> Option<(String, Option<i64>)> {
        let auth: Value = serde_json::from_slice(auth_state.contents.as_deref()?).ok()?;
        let chatgpt_account_id = extract_codex_managed_oauth_account_id(&auth)?;
        let user_identity = extract_codex_auth_user_identity(&auth);
        let marker = marker_state.contents.as_deref().and_then(|contents| {
            serde_json::from_slice::<CodexManagedOAuthLiveAuthMarker>(contents).ok()
        });
        let generation_id = match marker {
            Some(marker)
                if matches!(marker.version, 1 | 2)
                    && marker.account_id == chatgpt_account_id
                    && user_identity.is_some() =>
            {
                format!(
                    "managed:{}:{}",
                    marker.account_id,
                    user_identity.as_deref().expect("checked above")
                )
            }
            Some(marker)
                if marker.version == 3
                    && marker.chatgpt_account_id.as_deref()
                        == Some(chatgpt_account_id.as_str())
                    && marker
                        .user_identity
                        .as_deref()
                        .is_some_and(|identity| user_identity.as_deref() == Some(identity)) =>
            {
                format!(
                    "managed:{}:{}",
                    marker.account_id,
                    marker.user_identity.as_deref().expect("checked above")
                )
            }
            _ => format!(
                "native:{}",
                user_identity.as_deref().unwrap_or(&chatgpt_account_id)
            ),
        };
        let last_refresh_ms = auth
            .get("last_refresh")
            .and_then(Value::as_str)
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp_millis());
        Some((generation_id, last_refresh_ms))
    }
}

/// Which Codex tool surface the generated model catalog should target.
///
/// - `ProxyChat`: cc-switch's proxy takes over and converts Responses<->Chat,
///   so the catalog keeps Codex's default tool set (incl. the freeform
///   `apply_patch` custom tool, which the proxy rewrites to a function tool).
/// - `NativeResponses`: Codex talks directly to a provider's native
///   `/responses` endpoint (no proxy). Such gateways (e.g. Xiaomi MiMo,
///   MiniMax) reject `type=="custom"` tools, so the catalog must suppress the
///   freeform `apply_patch` and rely on `shell_type="shell_command"` for edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexCatalogToolProfile {
    ProxyChat,
    NativeResponses,
    /// Codex talks (through cc-switch's proxy) to a native Anthropic Messages
    /// gateway. Like `NativeResponses` it must suppress Codex's freeform custom
    /// tools — the Responses→Anthropic transform keeps only `function` tools.
    /// Additionally the Codex `web_search` hosted tool is unusable on this path
    /// (the transform drops it), so it is always disabled — see
    /// `prepare_codex_config_text_with_model_catalog`.
    Anthropic,
}

impl CodexCatalogToolProfile {
    /// Pick the catalog tool profile from a provider's `apiFormat` meta value.
    ///
    /// Prefer [`crate::proxy::providers::codex::resolve_codex_catalog_tool_profile`],
    /// which also honors settings-level `apiFormat` and the TOML `wire_api` (matching
    /// the proxy router). This string-only mapping is the fallback for non-Anthropic
    /// cases.
    pub fn from_api_format(api_format: Option<&str>) -> Self {
        match api_format {
            Some("anthropic") => CodexCatalogToolProfile::Anthropic,
            // Native (direct) Responses gateways reject Codex's freeform custom
            // tools (apply_patch, etc.); strip them via the NativeResponses profile.
            Some("openai_responses") => CodexCatalogToolProfile::NativeResponses,
            _ => CodexCatalogToolProfile::ProxyChat,
        }
    }
}

/// Reserved built-in provider IDs from OpenAI Codex's config/model-provider
/// catalog. Keep in sync with Codex `RESERVED_MODEL_PROVIDER_IDS` (0.149:
/// exactly these five; 0.148 is the same minus `amazon-bedrock-runtime`).
/// `oss` / `ollama-chat` are NOT reserved on 0.148/0.149 — both load as
/// ordinary custom tables — so listing them here would strand their bearer
/// token in the ignored top level. Mirror: providerConfigUtils.ts.
const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "amazon-bedrock-runtime",
    "openai",
    "ollama",
    "lmstudio",
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

fn get_codex_managed_oauth_live_auth_marker_path() -> PathBuf {
    crate::config::get_app_config_dir().join(CODEX_MANAGED_OAUTH_LIVE_AUTH_MARKER_FILENAME)
}

#[cfg(test)]
pub(crate) fn codex_managed_oauth_live_auth_marker_exists() -> bool {
    get_codex_managed_oauth_live_auth_marker_path().exists()
}

/// 从 live/备份的 Codex `auth` 中提取上游 ChatGPT workspace ID。
///
/// 仅接受 ChatGPT 登录形状（`auth_mode == "chatgpt"`、`OPENAI_API_KEY` 可清空）。
/// 托管账号写入的完整 bundle 会额外带 `tokens.refresh_token` 与顶层 `last_refresh`，
/// 这里一并容忍。Codex CLI 自刷新会轮换 access_token，因此短期 token 指纹不能
/// 作为稳定的所有权谓词；cc-switch 的本地账号 ID 单独记录在 marker 中。
fn extract_codex_managed_oauth_account_id(auth: &Value) -> Option<String> {
    let auth_obj = auth.as_object()?;

    if auth_obj.keys().any(|key| {
        !matches!(
            key.as_str(),
            "auth_mode" | "OPENAI_API_KEY" | "tokens" | "last_refresh"
        )
    }) {
        return None;
    }

    if auth.get("auth_mode").and_then(|value| value.as_str()) != Some("chatgpt") {
        return None;
    }

    let api_key_is_clearable = auth
        .get("OPENAI_API_KEY")
        .is_none_or(|value| value.is_null() || value.as_str() == Some("PROXY_MANAGED"));
    if !api_key_is_clearable {
        return None;
    }

    let tokens = auth.get("tokens").and_then(|value| value.as_object())?;

    if tokens.keys().any(|key| {
        !matches!(
            key.as_str(),
            "access_token" | "account_id" | "id_token" | "refresh_token"
        )
    }) {
        return None;
    }

    let account_id = tokens
        .get("account_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    tokens
        .get("access_token")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|token| !token.is_empty())?;

    Some(account_id.to_string())
}

/// 从原生 auth.json 的 id_token 提取跨刷新稳定的用户身份。
fn extract_codex_auth_user_identity(auth: &Value) -> Option<String> {
    let id_token = auth.pointer("/tokens/id_token")?.as_str()?;
    extract_codex_id_token_user_identity(id_token)
}

pub(crate) fn extract_codex_id_token_user_identity(id_token: &str) -> Option<String> {
    extract_codex_id_token_subject(id_token).map(|subject| format!("sub:{subject}"))
}

pub(crate) fn extract_codex_id_token_subject(id_token: &str) -> Option<String> {
    let mut segments = id_token.split('.');
    let header = segments.next()?;
    let payload = segments.next()?;
    segments.next()?;
    if segments.next().is_some() {
        return None;
    }

    let header: Value = URL_SAFE_NO_PAD
        .decode(header)
        .ok()
        .and_then(|decoded| serde_json::from_slice(&decoded).ok())?;
    header
        .get("alg")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let claims: Value = URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .and_then(|decoded| serde_json::from_slice(&decoded).ok())?;
    claims
        .get("sub")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
pub(crate) fn test_codex_id_token(subject: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let payload = URL_SAFE_NO_PAD.encode(json!({ "sub": subject }).to_string());
    format!("{header}.{payload}.")
}

/// Build the native-shaped ChatGPT auth bundle shared by cc-switch and Codex CLI.
pub fn codex_managed_oauth_auth_value(
    account_id: &str,
    access_token: &str,
    id_token: Option<&str>,
    refresh_token: &str,
    last_refresh: &str,
) -> Value {
    let mut tokens = serde_json::Map::new();
    if let Some(id_token) = id_token {
        tokens.insert("id_token".to_string(), Value::String(id_token.to_string()));
    }
    tokens.insert(
        "access_token".to_string(),
        Value::String(access_token.to_string()),
    );
    tokens.insert(
        "refresh_token".to_string(),
        Value::String(refresh_token.to_string()),
    );
    tokens.insert(
        "account_id".to_string(),
        Value::String(account_id.to_string()),
    );
    json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": Value::Object(tokens),
        "last_refresh": last_refresh,
    })
}

pub fn record_codex_managed_oauth_live_auth(
    auth: &Value,
    managed_account_id: &str,
) -> Result<(), AppError> {
    let managed_account_id = managed_account_id.trim();
    let Some(chatgpt_account_id) = extract_codex_managed_oauth_account_id(auth) else {
        return Ok(());
    };
    if managed_account_id.is_empty() {
        return Ok(());
    }
    let user_identity = extract_codex_auth_user_identity(auth).ok_or_else(|| {
        AppError::Message(
            "Codex 托管 OAuth auth.json 的 id_token 缺少稳定用户身份，无法安全记录账号所有权"
                .to_string(),
        )
    })?;

    let marker = CodexManagedOAuthLiveAuthMarker {
        version: 3,
        account_id: managed_account_id.to_string(),
        chatgpt_account_id: Some(chatgpt_account_id),
        user_identity: Some(user_identity),
    };
    crate::config::write_json_file(&get_codex_managed_oauth_live_auth_marker_path(), &marker)
}

fn migrate_legacy_codex_managed_oauth_live_auth_marker(
    auth: &Value,
    managed_account_id: &str,
    managed_id_token: Option<&str>,
) -> Result<(), AppError> {
    let marker_path = get_codex_managed_oauth_live_auth_marker_path();
    if !marker_path.exists() {
        return Ok(());
    }
    let marker: CodexManagedOAuthLiveAuthMarker = read_json_file(&marker_path)?;
    if !matches!(marker.version, 1 | 2) || marker.account_id != managed_account_id {
        return Ok(());
    }

    let auth_account_id = extract_codex_managed_oauth_account_id(auth);
    let auth_user_identity = extract_codex_auth_user_identity(auth);
    let managed_user_identity = managed_id_token.and_then(extract_codex_id_token_user_identity);
    if auth_account_id.as_deref() != Some(managed_account_id)
        || auth_user_identity.as_deref() != managed_user_identity.as_deref()
        || managed_user_identity.is_none()
    {
        return Err(AppError::Message(format!(
            "旧版 Codex OAuth 账号 {managed_account_id} 无法通过稳定用户身份确认磁盘凭据所有权；为避免覆盖或串用 auth.json，本次操作已取消，请在认证中心重新登录该账号"
        )));
    }

    record_codex_managed_oauth_live_auth(auth, managed_account_id)
}

/// Before removing a manager record, make any legacy live-auth ownership
/// provable with the manager's persisted user identity. Failure is surfaced so
/// callers keep the manager record and marker instead of orphaning auth.json.
pub(crate) fn prepare_codex_live_auth_for_managed_account_removal(
    managed_account_id: &str,
    managed_id_token: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Ok(());
    }
    let auth: Value = read_json_file(&auth_path)?;
    migrate_legacy_codex_managed_oauth_live_auth_marker(&auth, managed_account_id, managed_id_token)
}

pub fn codex_auth_matches_recorded_managed_oauth(
    auth: &Value,
    account_id: &str,
) -> Result<bool, AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(false);
    }

    let Some(auth_account_id) = extract_codex_managed_oauth_account_id(auth) else {
        return Ok(false);
    };
    let auth_user_identity = extract_codex_auth_user_identity(auth);
    let marker_path = get_codex_managed_oauth_live_auth_marker_path();
    let marker: CodexManagedOAuthLiveAuthMarker = match read_json_file(&marker_path) {
        Ok(marker) => marker,
        Err(err) => {
            log::warn!(
                "Failed to read Codex managed OAuth auth marker at {}: {err}",
                marker_path.display()
            );
            return Ok(false);
        }
    };

    // v1/v2 markers do not carry a stable user identity. Since multiple users
    // can share one workspace, those markers cannot safely authorize adopting
    // or deleting credentials. The next explicit activation replaces them
    // with a v3 marker.
    Ok(marker.account_id == account_id
        && match marker.version {
            3 => {
                marker.chatgpt_account_id.as_deref() == Some(auth_account_id.as_str())
                    && marker
                        .user_identity
                        .as_deref()
                        .is_some_and(|identity| auth_user_identity.as_deref() == Some(identity))
            }
            _ => false,
        })
}

/// Verify that a proxied Codex request still uses the exact live access token
/// owned by the selected local account. Workspace IDs alone are not sufficient:
/// different Team users can share one value.
pub(crate) fn codex_live_auth_matches_managed_request(
    account_id: &str,
    request_access_token: &str,
) -> Result<bool, AppError> {
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Ok(false);
    }
    let auth: Value = read_json_file(&auth_path)?;
    if !codex_auth_matches_recorded_managed_oauth(&auth, account_id)? {
        return Ok(false);
    }
    let live_access_token = auth
        .pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty());
    Ok(live_access_token == Some(request_access_token.trim()))
}

fn clear_codex_managed_oauth_live_auth_marker_for_account(
    account_id: &str,
) -> Result<(), AppError> {
    let marker_path = get_codex_managed_oauth_live_auth_marker_path();
    if !marker_path.exists() {
        return Ok(());
    }
    let marker: CodexManagedOAuthLiveAuthMarker = match read_json_file(&marker_path) {
        Ok(marker) => marker,
        Err(error) => {
            log::warn!(
                "Failed to read Codex managed OAuth auth marker at {} while cleaning account {}: {error}",
                marker_path.display(),
                account_id
            );
            // A malformed marker cannot establish ownership for any account
            // and is unusable for rollback/synchronization; remove the stale
            // bookkeeping file while leaving non-matching live auth untouched.
            return delete_file(&marker_path);
        }
    };
    if marker.account_id == account_id.trim() {
        delete_file(&marker_path)?;
    }
    Ok(())
}

/// 切走托管 provider 或从认证中心删除账号时，清理其残留在
/// `~/.codex/auth.json` 的 ChatGPT 登录。
///
/// 删除谓词同时校验 cc-switch marker 中的本地账号 ID 与原生 auth.json 中的
/// workspace ID，不依赖会被 Codex CLI 自刷新破坏的 access-token 指纹。切换路径必须
/// 先把盘上轮换后的 refresh token 采纳回 manager，再调用本函数。
pub fn clear_codex_live_auth_for_managed_account(account_id: &str) -> Result<(), AppError> {
    clear_codex_live_auth_for_managed_account_if_unchanged(account_id, None)
}

/// Verify that the outgoing account's live refresh generation has not changed
/// since it was adopted into the OAuth manager.
pub fn ensure_codex_live_auth_unchanged_for_managed_account(
    account_id: &str,
    expected_refresh_token: &str,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Err(AppError::Message(format!(
            "Codex CLI 账号 {account_id} 的 live auth 已在切换期间被移除，请重试"
        )));
    }
    let auth: Value = read_json_file(&auth_path)?;
    let current_refresh_token = auth
        .pointer("/tokens/refresh_token")
        .and_then(Value::as_str)
        .map(str::trim);
    if !codex_live_auth_is_managed_chatgpt_login(&auth, account_id)
        || current_refresh_token != Some(expected_refresh_token.trim())
    {
        return Err(AppError::Message(format!(
            "Codex CLI 账号 {account_id} 的 live 凭据在切换期间已刷新；为避免覆盖新 refresh token，本次操作已取消，请重试"
        )));
    }
    Ok(())
}

/// Content-based cleanup with an optional compare-before-delete guard.
pub fn clear_codex_live_auth_for_managed_account_if_unchanged(
    account_id: &str,
    expected_refresh_token: Option<&str>,
) -> Result<(), AppError> {
    let auth_path = get_codex_auth_path();
    let mut removed_matching_auth = false;
    if auth_path.exists() {
        let auth: Value = read_json_file(&auth_path)?;
        if codex_live_auth_is_managed_chatgpt_login(&auth, account_id) {
            if let Some(expected_refresh_token) = expected_refresh_token {
                let current_refresh_token = auth
                    .pointer("/tokens/refresh_token")
                    .and_then(Value::as_str)
                    .map(str::trim);
                if current_refresh_token != Some(expected_refresh_token.trim()) {
                    return Err(AppError::Message(format!(
                        "Codex CLI 账号 {account_id} 的 live 凭据在切换期间已刷新；为避免删除新 refresh token，本次操作已取消，请重试"
                    )));
                }
            }
            delete_file(&auth_path)?;
            removed_matching_auth = true;
        }
    }

    if removed_matching_auth {
        // Once the matching live file is gone, any marker is stale regardless
        // of version or parseability.
        delete_file(&get_codex_managed_oauth_live_auth_marker_path())?;
    } else {
        clear_codex_managed_oauth_live_auth_marker_for_account(account_id)?;
    }
    Ok(())
}

/// 判断给定的 Codex `auth` 是否属于指定的 cc-switch 本地托管账号。
///
/// 原生 `tokens.account_id` 是 workspace ID，可能被多个本地账号共享；因此必须同时
/// 命中 cc-switch marker 中的本地账号 ID，不能只按 auth.json 内容判断。
///
/// 用于 Live 备份剥离：避免把托管账号的可刷新 token 持久化进备份配置。
pub fn codex_live_auth_is_managed_chatgpt_login(auth: &Value, account_id: &str) -> bool {
    codex_auth_matches_recorded_managed_oauth(auth, account_id).unwrap_or(false)
}

/// 读回 Codex CLI 当前 `~/.codex/auth.json` 中属于 `account_id` 的 refresh_token /
/// id_token（仅当磁盘上的登录账号与之一致时）。
///
/// 用于切换回托管 provider 前，采纳 CLI 自行刷新时轮换出的最新 refresh_token，避免
/// 用陈腐 token 覆盖 CLI 的有效登录（“裸跑 codex” 反复切换场景）。
pub fn read_codex_live_auth_refresh_for_account(
    account_id: &str,
) -> Option<(String, Option<String>, Option<i64>)> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return None;
    }
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return None;
    }
    let auth: Value = read_json_file(&auth_path).ok()?;
    // 仅在磁盘上确是「该 account_id 的 ChatGPT 登录」时才采纳其 refresh_token，
    // 避免从非 chatgpt/异常 auth 里误取 token。
    if !codex_live_auth_is_managed_chatgpt_login(&auth, account_id) {
        return None;
    }
    let tokens = auth.get("tokens")?.as_object()?;
    let refresh_token = tokens.get("refresh_token")?.as_str()?.trim().to_string();
    if refresh_token.is_empty() {
        return None;
    }
    let id_token = tokens
        .get("id_token")
        .and_then(|value| value.as_str())
        .map(|token| token.to_string());
    let last_refresh_ms = auth
        .get("last_refresh")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis());
    Some((refresh_token, id_token, last_refresh_ms))
}

/// Read a managed live credential after safely upgrading a legacy marker.
/// v1/v2 markers only identify a workspace, so the manager's persisted
/// id_token must prove the live user's identity before the marker can become
/// authoritative again.
pub(crate) fn read_codex_live_auth_refresh_for_managed_account(
    account_id: &str,
    managed_id_token: Option<&str>,
) -> Result<Option<CodexManagedLiveRefresh>, AppError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(None);
    }
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Ok(None);
    }
    let auth: Value = read_json_file(&auth_path)?;
    migrate_legacy_codex_managed_oauth_live_auth_marker(&auth, account_id, managed_id_token)?;
    if !codex_auth_matches_recorded_managed_oauth(&auth, account_id)? {
        return Ok(None);
    }
    let Some((refresh_token, id_token, last_refresh_ms)) =
        read_codex_live_auth_refresh_for_account(account_id)
    else {
        return Ok(None);
    };
    let chatgpt_account_id = extract_codex_managed_oauth_account_id(&auth)
        .ok_or_else(|| AppError::Message("Codex live auth 缺少 workspace ID".to_string()))?;
    Ok(Some(CodexManagedLiveRefresh {
        refresh_token,
        id_token,
        last_refresh_ms,
        chatgpt_account_id,
    }))
}

/// Keep Codex CLI's live auth in the same refresh-token generation after the
/// manager refreshes a managed account.
///
/// The write is compare-and-swap-like: immediately before replacing auth.json,
/// it verifies that the file still contains the refresh token used for the
/// network request. Codex CLI does not share cc-switch's process lock, so this
/// is a best-effort guard that narrows (but cannot make atomic) the cross-process
/// check-to-replace window.
/// Ownership is local-account scoped through the marker, while auth.json keeps
/// the upstream workspace ID required by Codex.
pub fn sync_codex_managed_oauth_live_auth_after_refresh(
    account_id: &str,
    expected_refresh_token: &str,
    refreshed_auth: &Value,
) -> Result<bool, AppError> {
    let account_id = account_id.trim();
    let expected_refresh_token = expected_refresh_token.trim();
    if account_id.is_empty() || expected_refresh_token.is_empty() {
        return Ok(false);
    }

    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Ok(false);
    }
    let current_auth: Value = read_json_file(&auth_path)?;
    if !codex_live_auth_is_managed_chatgpt_login(&current_auth, account_id) {
        return Ok(false);
    }
    let current_refresh_token = current_auth
        .pointer("/tokens/refresh_token")
        .and_then(Value::as_str)
        .map(str::trim);
    if current_refresh_token != Some(expected_refresh_token) {
        return Ok(false);
    }

    let marker_path = get_codex_managed_oauth_live_auth_marker_path();
    let was_recorded_managed = marker_path.exists()
        && codex_auth_matches_recorded_managed_oauth(&current_auth, account_id)?;

    write_json_file(&auth_path, refreshed_auth)?;
    if was_recorded_managed {
        record_codex_managed_oauth_live_auth(refreshed_auth, account_id)?;
    }
    Ok(true)
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
    // Exact match, mirroring upstream: both the built-in provider lookup and
    // validate_reserved_model_provider_ids are case-sensitive, so `OpenAI`
    // etc. are legitimate custom ids whose tables must receive the token.
    // Keep in sync with the frontend list in src/utils/providerConfigUtils.ts.
    let id = id.trim();
    !id.is_empty() && !CODEX_RESERVED_MODEL_PROVIDER_IDS.contains(&id)
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
/// back to a top-level `base_url`. Deliberately never reads a non-active
/// `[model_providers.*]` section — the frontend `extractCodexBaseUrl`
/// (`getRecoverableBaseUrlAssignments`) excludes those too, and a leftover
/// section unrelated to the active provider must not leak into `{{baseUrl}}`.
pub fn extract_codex_base_url(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;

    if let Some(active_provider) = doc.get("model_provider").and_then(|v| v.as_str()) {
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

/// True only when the auth carries material Codex itself authenticates with
/// ahead of the API-key fallback: OAuth tokens or another first-class login
/// carrier. Unlike `codex_auth_has_oauth_login_material`, pure metadata such
/// as `last_refresh` or `tokens.account_id` does NOT count — metadata must not
/// shield a stale third-party `OPENAI_API_KEY` from post-switch cleanup.
pub fn codex_auth_has_credential_login_material(auth: &Value) -> bool {
    let Some(obj) = auth.as_object() else {
        return false;
    };

    let value_present = |value: &Value| match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    };

    if ["personal_access_token", "agent_identity", "bedrock_api_key"]
        .iter()
        .any(|key| obj.get(*key).is_some_and(value_present))
    {
        return true;
    }

    obj.get("tokens")
        .and_then(Value::as_object)
        .is_some_and(|tokens| {
            ["id_token", "access_token", "refresh_token"]
                .iter()
                .any(|key| tokens.get(*key).is_some_and(value_present))
        })
}

/// True when live `auth.json` is the shape a preserve-off third-party switch
/// leaves behind: an `OPENAI_API_KEY` (possibly alongside metadata like
/// `auth_mode` / `last_refresh`) with no real login credential next to it.
pub fn codex_live_auth_is_stale_third_party_residue(live_auth: &Value) -> bool {
    if codex_auth_has_credential_login_material(live_auth) {
        return false;
    }
    live_auth
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|key| !key.is_empty())
}

/// After a normal switch to an official provider that carries no login
/// material of its own, delete a live `auth.json` that only holds a stale
/// third-party API key, so Codex shows its login screen instead of sending
/// the wrong key to the official endpoint (401 with no way to re-login).
///
/// Deleting the file — not writing `{}` — is deliberate: Codex resolves an
/// empty object to ChatGPT mode without tokens and errors at bootstrap,
/// while a missing file yields NotAuthenticated and the login screen,
/// matching Codex's own logout.
///
/// Callers must only invoke this after the outgoing provider was
/// successfully backfilled into the DB — that backfill holds the only other
/// copy of the third-party key. The switch backfill intentionally lacks the
/// proxy-side "no credentials in the builtin official row" guard
/// (`services/proxy.rs` `sync_live_config_to_provider`): that asymmetry is
/// what heals official API-key logins into the DB row, and this cleanup's
/// safety depends on it — do not align the two guards.
///
/// Returns Ok(true) when the file was deleted.
pub fn clear_stale_codex_live_auth_after_official_switch(
    db_auth: &Value,
) -> Result<bool, AppError> {
    if codex_auth_has_login_material(db_auth) {
        // A material-carrying official provider gets a full auth write;
        // nothing stale can remain.
        return Ok(false);
    }
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Ok(false);
    }
    let live_auth: Value = read_json_file(&auth_path)?;
    if !codex_live_auth_is_stale_third_party_residue(&live_auth) {
        return Ok(false);
    }
    delete_file(&auth_path)?;
    Ok(true)
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

fn codex_catalog_input_modalities(
    model: &str,
    declared_modalities: Option<&[String]>,
) -> Vec<String> {
    let modalities = match image_input_capability_from_modalities(model, declared_modalities) {
        ImageInputCapability::Unsupported => &["text"][..],
        ImageInputCapability::Supported | ImageInputCapability::Unknown => &["text", "image"][..],
    };
    modalities.iter().map(|item| (*item).to_string()).collect()
}

/// Canonical reasoning effort levels Codex understands, with the same
/// descriptions the official gpt-5.5 template uses. `none` disables thinking.
const CODEX_REASONING_LEVEL_DESCRIPTIONS: &[(&str, &str)] = &[
    ("none", "Disable Thinking"),
    ("minimal", "Minimal reasoning"),
    ("low", "Fast responses with lighter reasoning"),
    (
        "medium",
        "Balances speed and reasoning depth for everyday tasks",
    ),
    ("high", "Greater reasoning depth for complex problems"),
    ("xhigh", "Extra high reasoning depth for complex problems"),
    ("max", "Maximum reasoning depth for the hardest problems"),
    ("ultra", "Ultra reasoning depth"),
];

fn codex_reasoning_level_description(effort: &str) -> Option<&'static str> {
    CODEX_REASONING_LEVEL_DESCRIPTIONS
        .iter()
        .find(|(candidate, _)| *candidate == effort)
        .map(|(_, description)| *description)
}

/// User-declared levels reduced to the canonical efforts Codex understands,
/// in canonical (lowest → highest) order regardless of declaration order.
/// Unknown efforts are dropped so a typo can never produce an entry Codex
/// would reject.
fn codex_canonical_efforts(levels: &[String]) -> Vec<&str> {
    CODEX_REASONING_LEVEL_DESCRIPTIONS
        .iter()
        .filter(|(effort, _)| levels.iter().any(|candidate| candidate == effort))
        .map(|(effort, _)| *effort)
        .collect()
}

/// Build a `supported_reasoning_levels` array from user-declared effort values.
fn codex_supported_reasoning_levels(levels: &[String]) -> Value {
    let entries: Vec<Value> = codex_canonical_efforts(levels)
        .into_iter()
        .map(|effort| {
            let description = codex_reasoning_level_description(effort)
                .expect("canonical effort always has a description");
            json!({ "effort": effort, "description": description })
        })
        .collect();
    json!(entries)
}

/// Apply a per-model reasoning-level override onto a catalog entry. Returns
/// true when the override was applied (so callers can skip further work).
/// `template_default` is the base entry's `default_reasoning_level` (from the
/// profile template or an official vendor entry) used as the fallback when the
/// user did not declare one explicitly.
fn apply_codex_reasoning_level_override(
    entry_obj: &mut serde_json::Map<String, Value>,
    template_default: Option<&str>,
    spec: &CodexCatalogModelSpec,
) -> bool {
    let Some(levels) = spec.reasoning_levels.as_deref() else {
        return false;
    };
    let canonical = codex_canonical_efforts(levels);
    if canonical.is_empty() {
        return false;
    }
    let supported = codex_supported_reasoning_levels(levels);
    entry_obj.insert("supported_reasoning_levels".to_string(), supported);

    // Default: explicit user value wins; otherwise keep the base default when
    // it is still supported; otherwise fall back to the highest supported
    // level in canonical order. All candidates are validated against the
    // canonical set so the default can never reference a dropped effort.
    let default_level = spec
        .default_reasoning_level
        .as_deref()
        .filter(|level| canonical.contains(level))
        .or_else(|| template_default.filter(|level| canonical.contains(level)))
        .or_else(|| canonical.last().copied());
    if let Some(default_level) = default_level {
        entry_obj.insert("default_reasoning_level".to_string(), json!(default_level));
    }
    true
}

fn codex_catalog_model_entry(
    template: &Value,
    spec: &CodexCatalogModelSpec,
    priority: usize,
    profile: CodexCatalogToolProfile,
    default_context_window: u64,
) -> Value {
    let mut entry = template.clone();
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    let display_name = spec.display_name.as_deref().unwrap_or(&spec.model);
    let context_window = spec.context_window.unwrap_or(default_context_window);
    entry_obj.insert("slug".to_string(), json!(spec.model));
    entry_obj.insert("display_name".to_string(), json!(display_name));
    entry_obj.insert("description".to_string(), json!(display_name));
    entry_obj.insert("context_window".to_string(), json!(context_window));
    entry_obj.insert("max_context_window".to_string(), json!(context_window));
    entry_obj.insert("priority".to_string(), json!(1000 + priority));
    entry_obj.insert("additional_speed_tiers".to_string(), json!([]));
    entry_obj.insert("service_tiers".to_string(), json!([]));
    entry_obj.insert("availability_nux".to_string(), Value::Null);
    entry_obj.insert("upgrade".to_string(), Value::Null);

    // Image support is a model capability, not a tool-profile capability.
    // Trust hidden preset metadata first, then the confirmed text-only registry;
    // every unknown model fails open so GPT/relay aliases are never declared
    // text-only merely because a template had a conservative default.
    entry_obj.insert(
        "input_modalities".to_string(),
        json!(codex_catalog_input_modalities(
            &spec.model,
            spec.input_modalities.as_deref(),
        )),
    );

    if profile != CodexCatalogToolProfile::ProxyChat {
        // Native `/responses` and Anthropic gateways reject / drop Codex's freeform
        // `apply_patch` (type=="custom") tool. Strip any key that would make Codex
        // emit a custom/freeform tool, and rely on shell_type="shell_command" for
        // edits. Defensive even though the native template is already clean
        // (guards against template drift / an accidental gpt-5.5 clone).
        //
        // NOTE: `base_instructions` is NOT stripped — Codex's catalog parser
        // treats it as a REQUIRED field and refuses to load the file without
        // it ("missing field `base_instructions`"). The template carries a
        // neutral identity default; per-vendor official text overrides below.
        for key in [
            "apply_patch_tool_type",
            "web_search_tool_type",
            "tools",
            "model_messages",
        ] {
            entry_obj.remove(key);
        }
        entry_obj.insert("shell_type".to_string(), json!("shell_command"));

        if let Some(base_instructions) = spec
            .base_instructions
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry_obj.insert("base_instructions".to_string(), json!(base_instructions));
        }
        if let Some(parallel) = spec.supports_parallel_tool_calls {
            entry_obj.insert("supports_parallel_tool_calls".to_string(), json!(parallel));
        }
    }

    // Per-model reasoning levels override the template's conservative
    // none/high default (e.g. a LiteLLM gateway serving a model that accepts
    // low/medium/high/xhigh/max). Applies to every profile.
    let template_default = template
        .get("default_reasoning_level")
        .and_then(|value| value.as_str());
    apply_codex_reasoning_level_override(entry_obj, template_default, spec);

    entry
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCatalogModelSpec {
    model: String,
    /// Explicit user value only. Entries fall back to the model id — except
    /// official vendor catalog entries, which keep the vendor's display name.
    display_name: Option<String>,
    /// Explicit user value only. Entries fall back to the config's
    /// `model_context_window` (or 128k) — except official vendor catalog
    /// entries, which keep the vendor's declared window.
    context_window: Option<u64>,
    /// Per-row override for the native template's `supports_parallel_tool_calls`
    /// (e.g. MiniMax=true, MiMo=false). Only consulted for `NativeResponses`.
    supports_parallel_tool_calls: Option<bool>,
    /// Hidden per-row capability declaration from built-in provider metadata.
    /// When omitted, all catalog profiles consult the shared text-only model
    /// registry and otherwise default to `["text", "image"]`.
    input_modalities: Option<Vec<String>>,
    /// Per-row override for the native template's `base_instructions` (the
    /// model identity / system preamble). Carries each vendor's OFFICIAL value
    /// (e.g. MiMo "developed by Xiaomi", MiniMax "based on MiniMax-M3"); falls
    /// back to the template default when absent. Only consulted for
    /// `NativeResponses`.
    base_instructions: Option<String>,
    /// Per-row override for the generated catalog's `supported_reasoning_levels`
    /// (e.g. ["none", "low", "medium", "high", "xhigh", "max"]). When omitted
    /// the template's conservative default (none/high) is kept. Consulted for
    /// every profile; the vendor-catalog path applies it on top of the
    /// official entry.
    reasoning_levels: Option<Vec<String>>,
    /// Per-row override for the generated catalog's `default_reasoning_level`.
    /// Only meaningful together with `reasoning_levels`; when absent the
    /// template default is kept if it is still in the list, otherwise the last
    /// (highest) declared level wins.
    default_reasoning_level: Option<String>,
}

fn codex_catalog_model_specs(settings: &Value) -> Vec<CodexCatalogModelSpec> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
    else {
        return Vec::new();
    };

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
            .map(str::to_string);
        let context_window = parse_codex_positive_u64(
            model_config
                .get("contextWindow")
                .or_else(|| model_config.get("context_window")),
        );

        let supports_parallel_tool_calls = model_config
            .get("supportsParallelToolCalls")
            .or_else(|| model_config.get("supports_parallel_tool_calls"))
            .and_then(|value| value.as_bool());
        let input_modalities = model_config
            .get("inputModalities")
            .or_else(|| model_config.get("input_modalities"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|items| !items.is_empty());

        let base_instructions = model_config
            .get("baseInstructions")
            .or_else(|| model_config.get("base_instructions"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);

        let reasoning_levels = model_config
            .get("reasoningLevels")
            .or_else(|| model_config.get("reasoning_levels"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::trim)
                    .filter(|level| !level.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|levels| !levels.is_empty());
        let default_reasoning_level = model_config
            .get("defaultReasoningLevel")
            .or_else(|| model_config.get("default_reasoning_level"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .map(str::to_string);

        specs.push(CodexCatalogModelSpec {
            model: model.to_string(),
            display_name,
            context_window,
            supports_parallel_tool_calls,
            input_modalities,
            base_instructions,
            reasoning_levels,
            default_reasoning_level,
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

fn codex_bundled_models_command(candidate: &Path) -> Command {
    let mut command = Command::new(candidate);
    command
        .args(["debug", "models", "--bundled"])
        .stdin(Stdio::null());

    // A release build uses the Windows GUI subsystem, so a console child that
    // is created without this flag gets its own transient console window. npm
    // installs Codex as `codex.cmd`, which Windows launches through cmd.exe.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

fn load_codex_model_template_from_bundled() -> Result<Option<Value>, AppError> {
    for candidate in codex_cli_candidates() {
        let candidate_label = candidate.to_string_lossy();
        let output = match codex_bundled_models_command(&candidate).output() {
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

/// Bundled clean template for native `/responses` providers. Unlike the
/// gpt-5.5 template it carries NO freeform `apply_patch` / `web_search` tool
/// declarations and no GPT-5 base_instructions, so Codex never emits a
/// `type=="custom"` tool that native gateways (MiMo/MiniMax/…) reject. Edits
/// flow through `shell_type="shell_command"` instead. We deliberately do NOT
/// fall back to `models_cache.json` here (that would reintroduce gpt-5.5's
/// freeform apply_patch).
fn load_codex_native_responses_template() -> Value {
    let text = include_str!("resources/codex_native_responses_template.json");
    serde_json::from_str(text).expect("bundled codex native responses template must be valid JSON")
}

/// Hosts whose native `/responses` gateway publishes an OFFICIAL Codex model
/// catalog (models.json) that cc-switch mirrors verbatim. Matched against
/// `base_url` ONLY — deliberately NOT by model brand, unlike
/// `CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES`: the official entries GRANT
/// capabilities (freeform `apply_patch`, vendor harness), and an aggregator
/// merely hosting the same model may not honor them. The safe failure
/// direction for aggregators is the neutral template (degraded but working);
/// wrongly granting freeform apply_patch would reintroduce the custom-tool
/// rejection bug.
const CODEX_DEEPSEEK_OFFICIAL_CATALOG_HOSTS: &[&str] = &["deepseek.com"];

/// Bundled copy of DeepSeek's official Codex models.json — the exact file
/// their one-click integration script writes (api-docs.deepseek.com →
/// quick_start/agent_integrations/codex): freeform apply_patch, GPT-5 harness
/// base_instructions, low/high/max reasoning levels, web_search supported,
/// 1m context. Declares `minimal_client_version` 0.144.0.
fn load_codex_deepseek_official_catalog_models() -> Vec<Value> {
    let text = include_str!("resources/codex_deepseek_catalog_template.json");
    let catalog: Value =
        serde_json::from_str(text).expect("bundled DeepSeek official catalog must be valid JSON");
    catalog
        .get("models")
        .and_then(|models| models.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Official vendor catalog entries for the provider in `config_text`, if its
/// gateway ships one. Only the `NativeResponses` profile qualifies: ProxyChat
/// runs through cc-switch's converter (gpt-5.5 template contract) and the
/// Anthropic transform drops custom tools, so both must keep their existing
/// templates. Host-driven like the web_search blacklist, so existing providers
/// pick it up on their next switch without a re-save.
fn codex_official_vendor_catalog_models(
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Option<Vec<Value>> {
    if profile != CodexCatalogToolProfile::NativeResponses {
        return None;
    }
    let base_url = extract_codex_base_url(config_text)?.to_ascii_lowercase();
    if CODEX_DEEPSEEK_OFFICIAL_CATALOG_HOSTS
        .iter()
        .any(|host| base_url.contains(host))
    {
        let models = load_codex_deepseek_official_catalog_models();
        if !models.is_empty() {
            return Some(models);
        }
    }
    None
}

/// Build one catalog entry from an official vendor catalog: match the user's
/// model id against the vendor entries by slug; an unknown id clones the
/// vendor's first (flagship) entry so it keeps the gateway's capability
/// profile without impersonating the flagship. The official entry is
/// authoritative — no tool-profile stripping — but explicit per-row user
/// overrides still win.
fn codex_vendor_catalog_model_entry(
    vendor_models: &[Value],
    spec: &CodexCatalogModelSpec,
    priority: usize,
) -> Value {
    let matched = vendor_models.iter().find(|entry| {
        entry
            .get("slug")
            .and_then(|slug| slug.as_str())
            .is_some_and(|slug| slug.eq_ignore_ascii_case(&spec.model))
    });
    let mut entry = match matched {
        Some(found) => found.clone(),
        None => vendor_models.first().cloned().unwrap_or_else(|| json!({})),
    };
    // Capture before the mutable borrow: the vendor entry's own default is the
    // fallback when the user declares reasoning levels without a default.
    let vendor_default = entry
        .get("default_reasoning_level")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    if matched.is_none() {
        let display_name = spec.display_name.as_deref().unwrap_or(&spec.model);
        entry_obj.insert("slug".to_string(), json!(spec.model));
        entry_obj.insert("display_name".to_string(), json!(display_name));
        entry_obj.insert("description".to_string(), json!(display_name));
        entry_obj.insert("priority".to_string(), json!(1000 + priority));
    }

    // Explicit user overrides win over the official entry; absent values keep
    // the vendor's declarations (context window, modalities, harness, ...).
    if let Some(display_name) = spec.display_name.as_deref() {
        entry_obj.insert("display_name".to_string(), json!(display_name));
    }
    if let Some(context_window) = spec.context_window {
        entry_obj.insert("context_window".to_string(), json!(context_window));
        entry_obj.insert("max_context_window".to_string(), json!(context_window));
    }
    if let Some(parallel) = spec.supports_parallel_tool_calls {
        entry_obj.insert("supports_parallel_tool_calls".to_string(), json!(parallel));
    }
    if let Some(modalities) = spec.input_modalities.as_deref() {
        entry_obj.insert("input_modalities".to_string(), json!(modalities));
    }
    if let Some(base_instructions) = spec
        .base_instructions
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        entry_obj.insert("base_instructions".to_string(), json!(base_instructions));
    }

    // Per-model reasoning levels win over the official vendor entry too.
    // The vendor file is the base (its own levels stay when no override is
    // declared); its default_reasoning_level is the fallback.
    apply_codex_reasoning_level_override(entry_obj, vendor_default.as_deref(), spec);

    // Defensive: if a future codex parser requires a field the vendor file
    // predates, backfill only whitelisted parser-required keys.
    fill_template_fields_from_static(&mut entry);
    entry
}

/// Fields Codex's external-catalog parser REQUIRES (no serde default): when
/// one is missing Codex rejects the whole catalog file at startup ("missing
/// field ..."). `base_instructions` is the other known required field; the
/// templates always carry it and `codex_catalog_model_entry` handles it.
/// When Codex requires a new field, add it here AND to the static templates.
const CODEX_CATALOG_PARSER_REQUIRED_FIELDS: &[&str] = &["supports_reasoning_summaries"];

/// `models_cache.json` is shared by every Codex install on the machine (npm
/// CLI, desktop-bundled binary, ...), and each version serializes its own
/// `ModelInfo` shape — the cache's field set follows whichever process wrote
/// it last, so it cannot be assumed to satisfy the current external-catalog
/// schema (observed live: 0.144.5 requires `supports_reasoning_summaries`
/// while a coexisting build kept rewriting the cache without it). Backfill
/// ONLY parser-required fields from the bundled static template: optional
/// capability fields keep their missing-means-default semantics, and existing
/// values always win.
fn fill_template_fields_from_static(template: &mut Value) {
    let Some(static_template) = load_codex_model_template_static() else {
        return;
    };
    let (Some(template_obj), Some(static_obj)) =
        (template.as_object_mut(), static_template.as_object())
    else {
        return;
    };
    for key in CODEX_CATALOG_PARSER_REQUIRED_FIELDS {
        if !template_obj.contains_key(*key) {
            if let Some(value) = static_obj.get(*key) {
                template_obj.insert((*key).to_string(), value.clone());
            }
        }
    }
}

fn load_codex_model_catalog_template_uncached() -> Result<Value, AppError> {
    // ① models_cache.json (created by Codex when it connects to OpenAI)
    if let Some(mut template) = load_codex_model_template_from_cache()? {
        fill_template_fields_from_static(&mut template);
        return Ok(template);
    }
    // ② codex CLI (PATH + platform-specific common paths)
    if let Some(mut template) = load_codex_model_template_from_bundled()? {
        fill_template_fields_from_static(&mut template);
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

fn get_or_load_codex_model_catalog_template<F>(
    cache: &OnceCell<Value>,
    loader: F,
) -> Result<Value, AppError>
where
    F: FnOnce() -> Result<Value, AppError>,
{
    cache.get_or_try_init(loader).cloned()
}

#[cfg(not(test))]
fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    get_or_load_codex_model_catalog_template(
        &CODEX_MODEL_CATALOG_TEMPLATE_CACHE,
        load_codex_model_catalog_template_uncached,
    )
}

#[cfg(test)]
fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    load_codex_model_catalog_template_uncached()
}

fn codex_model_catalog_from_specs(
    specs: &[CodexCatalogModelSpec],
    template: &Value,
    profile: CodexCatalogToolProfile,
    default_context_window: u64,
) -> Value {
    let entries: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            codex_catalog_model_entry(template, spec, index, profile, default_context_window)
        })
        .collect();

    json!({ "models": entries })
}

fn codex_model_catalog_from_settings(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<Option<Value>, AppError> {
    let specs = codex_catalog_model_specs(settings);
    if specs.is_empty() {
        return Ok(None);
    }

    // Vendors that publish an OFFICIAL Codex models.json for their native
    // `/responses` gateway get it mirrored verbatim instead of the neutral
    // template: its freeform apply_patch, vendor harness base_instructions and
    // reasoning levels are load-bearing (the harness tells the model to use
    // apply_patch, so catalog and harness must stay consistent).
    if let Some(vendor_models) = codex_official_vendor_catalog_models(config_text, profile) {
        let entries: Vec<Value> = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| codex_vendor_catalog_model_entry(&vendor_models, spec, index))
            .collect();
        return Ok(Some(json!({ "models": entries })));
    }

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);

    // Native providers use the bundled clean template (no freeform apply_patch,
    // no cache dependency); proxy-chat providers keep cloning Codex's gpt-5.5
    // entry so the proxy can rewrite custom<->function tools as before.
    let template = match profile {
        CodexCatalogToolProfile::NativeResponses | CodexCatalogToolProfile::Anthropic => {
            load_codex_native_responses_template()
        }
        CodexCatalogToolProfile::ProxyChat => load_codex_model_catalog_template()?,
    };
    Ok(Some(codex_model_catalog_from_specs(
        &specs,
        &template,
        profile,
        default_context_window,
    )))
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
            // Only claim the pointer when it is absent or already cc-switch-owned.
            // A user-managed external catalog file (custom filename or path) is
            // left untouched, mirroring the None arm's ownership rule that
            // `resolve_cc_switch_catalog_path` relies on.
            let is_cc_switch_owned = doc
                .get("model_catalog_json")
                .and_then(|item| item.as_str())
                .map(|path| {
                    Path::new(path).file_name().and_then(|name| name.to_str())
                        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
                })
                .unwrap_or(true);
            if is_cc_switch_owned {
                doc["model_catalog_json"] =
                    toml_edit::value(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
            }
        }
        None => {
            let should_remove = doc
                .get("model_catalog_json")
                .and_then(|item| item.as_str())
                .map(|path| {
                    Path::new(path).file_name().and_then(|name| name.to_str())
                        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
                })
                .unwrap_or(false);
            if should_remove {
                doc.as_table_mut().remove("model_catalog_json");
            }
        }
    }

    Ok(doc.to_string())
}

/// Pure toggle for the top-level `web_search` field that turns Codex's built-in
/// web-search tool off. When `disable` is true we write `web_search = "disabled"`
/// (the catalog's `supports_search_tool` does NOT gate this — the request-time
/// tool comes from the config, defaulting on). When false we *remove* the field,
/// but only when it carries cc-switch's own `"disabled"` sentinel, so switching
/// back to a web-search-capable provider re-enables it without clobbering a
/// user's manual setting.
///
/// The caller decides `disable` (see `codex_native_gateway_rejects_web_search`);
/// lifecycle is bound to the cc-switch catalog pointer so the field is set/cleaned
/// up wherever the native catalog is written/removed.
fn set_codex_native_web_search_field(config_text: &str, disable: bool) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if disable {
        doc[CODEX_WEB_SEARCH_FIELD] = toml_edit::value(CODEX_WEB_SEARCH_DISABLED);
    } else {
        let owned = doc
            .get(CODEX_WEB_SEARCH_FIELD)
            .and_then(|item| item.as_str())
            == Some(CODEX_WEB_SEARCH_DISABLED);
        if owned {
            doc.as_table_mut().remove(CODEX_WEB_SEARCH_FIELD);
        }
    }

    Ok(doc.to_string())
}

/// Generate Codex `model_catalog_json` from provider settings and inject/remove
/// the top-level TOML field that points Codex to the generated file.
pub fn prepare_codex_config_text_with_model_catalog(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<String, AppError> {
    let catalog_path = get_codex_model_catalog_path();

    if let Some(catalog) = codex_model_catalog_from_settings(settings, config_text, profile)? {
        let config_text = set_codex_model_catalog_json_field(config_text, Some(&catalog_path))?;
        // Disable web_search only for native gateways on the reject blacklist
        // (MiMo/LongCat/MiniMax by host or model brand; Qwen3-Coder by model).
        // Everything else — relays, DouBao, web-search-capable Qwen models,
        // unknown providers — keeps Codex's default.
        let disable_web_search = match profile {
            // The Responses→Anthropic transform silently drops the Codex web_search
            // hosted tool, so always disable it here rather than present a dead tool.
            CodexCatalogToolProfile::Anthropic => true,
            CodexCatalogToolProfile::NativeResponses => {
                codex_native_gateway_rejects_web_search(&config_text)
            }
            CodexCatalogToolProfile::ProxyChat => false,
        };
        let config_text = set_codex_native_web_search_field(&config_text, disable_web_search)?;
        write_json_file(&catalog_path, &catalog)?;
        Ok(config_text)
    } else {
        let config_text = set_codex_model_catalog_json_field(config_text, None)?;
        // Even without a generated catalog, the Responses→Anthropic transform drops the
        // Codex web_search hosted tool, so keep the invariant that an Anthropic provider
        // never presents it as a dead tool.
        let disable_web_search = profile == CodexCatalogToolProfile::Anthropic;
        set_codex_native_web_search_field(&config_text, disable_web_search)
    }
}

/// Reverse of `prepare_codex_config_text_with_model_catalog`: read the
/// cc-switch–maintained catalog file referenced by `~/.codex/config.toml` and
/// convert it back into the simplified shape the frontend table uses:
/// `{ "models": [{ "model", "displayName"?, "contextWindow"?, hidden overrides... }, ...] }`.
///
/// We only reverse-parse catalogs whose `model_catalog_json` path is the
/// cc-switch–generated file (identified by filename
/// `cc-switch-model-catalog.json`). A user-managed external catalog file is
/// left alone — surfacing its richer structure as the simplified table would
/// be a downgrade we can't safely round-trip.
///
/// `displayName`, `contextWindow`, and `inputModalities` are omitted from the
/// returned entry when the on-disk value matches the fallback that
/// `codex_model_catalog_from_settings` injects for unset inputs (slug for
/// display_name, `model_context_window` or 128_000 for context_window, and the
/// shared confirmed-text-only inference for input modalities). This preserves
/// the "user left it blank" intent across round-trip; an unavoidable edge case
/// is that a user-typed value that happens to equal the fallback also collapses
/// to blank, but the next save writes the same fallback so behavior is stable.
///
/// All failure modes (missing file, parse error, no `model_catalog_json`,
/// entries without `slug`) collapse to `Ok(None)` so callers can treat this
/// as best-effort enrichment without making `read_live_settings` brittle.
/// 模型目录文件读取上限（32 MiB）。目录 JSON 正常只有几百 KiB；超过则视为异常，
/// 避免指向外部大文件时耗尽内存。
const MAX_CODEX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;

pub fn read_codex_model_catalog_simplified_from_live() -> Result<Option<Value>, AppError> {
    let config_text = read_codex_config_text()?;
    let config_dir = get_codex_config_dir();
    let Some(catalog_path) = resolve_cc_switch_catalog_path(&config_text, &config_dir) else {
        return Ok(None);
    };
    if !catalog_path.exists() {
        return Ok(None);
    }
    let catalog_text = match read_limited_string(&catalog_path, MAX_CODEX_CATALOG_BYTES) {
        Ok(text) => text,
        Err(error) => {
            log::warn!(
                "拒绝读取越界或过大的 Codex 模型目录 {}: {error}",
                catalog_path.display()
            );
            return Ok(None);
        }
    };
    Ok(build_simplified_catalog_from_texts(
        &config_text,
        &catalog_text,
    ))
}

/// 安全地读取文件为字符串，并在超过字节上限时返回错误。
pub(crate) fn read_limited_string(path: &Path, max_bytes: u64) -> Result<String, AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.len() > max_bytes {
        return Err(AppError::Config(format!(
            "文件 {} 超过大小上限 {} 字节",
            path.display(),
            max_bytes
        )));
    }
    fs::read_to_string(path).map_err(|error| AppError::io(path, error))
}

/// Read the cc-switch Codex model catalog file with a size cap.
pub(crate) fn read_codex_model_catalog_text(path: &Path) -> Result<String, AppError> {
    read_limited_string(path, MAX_CODEX_CATALOG_BYTES)
}

/// Given `config.toml` text, resolve the on-disk path of the cc-switch–owned
/// catalog file (returns `None` if `model_catalog_json` is absent or points at
/// a file we don't own). Relative paths are resolved under `base_dir`;
/// absolute paths must still be inside `base_dir`.
pub(crate) fn resolve_cc_switch_catalog_path(
    config_text: &str,
    base_dir: &Path,
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

    // 注意（有意的行为变更）：Windows 上 `/…` 形式的旧 WSL 风格 Linux 路径也会
    // 被视为绝对路径，从而在下方的包含性校验中失败——此前这类路径会因无法匹配
    // 生成文件名而回退为按文件名解析、碰巧能工作。可接受：下一次切换供应商时
    // 写入侧会重新落一个裸文件名，配置自愈（见
    // `set_catalog_json_none_removes_cc_switch_owned_by_filename` 的场景注释）。
    let is_unix_absolute = catalog_path_str.starts_with('/');
    let resolved = if referenced_path.is_absolute() || is_unix_absolute {
        referenced_path.to_path_buf()
    } else {
        base_dir.join(referenced_path)
    };

    if !path_is_within(base_dir, &resolved) {
        log::warn!(
            "Codex model_catalog_json 指向配置目录外: {}（允许目录: {}）",
            resolved.display(),
            base_dir.display()
        );
        return None;
    }

    // 词法包含不等于运行时包含：配置目录内的符号链接（如 ~/.codex/link ->
    // /etc）能让 `link/cc-switch-model-catalog.json` 通过上面的检查，读取却
    // 落到目录外。文件存在时把真实路径 canonicalize 出来再校验一次，并把
    // canonical 路径返回给调用方——后续读取不再经过 symlink 组件。
    if resolved.exists() {
        let canonical = match fs::canonicalize(&resolved) {
            Ok(path) => path,
            Err(error) => {
                log::warn!(
                    "Codex model_catalog_json canonicalize 失败: {}: {error}",
                    resolved.display()
                );
                return None;
            }
        };
        // base 同样 canonicalize，保证两侧前缀一致（Windows \\?\、
        // macOS /tmp -> /private/tmp）；base 失败时退回词法 base——
        // 词法 base 与 canonical 路径比较只会误拒（退化为不读），不会误放。
        let canonical_base = fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
        if !path_is_within(&canonical_base, &canonical) {
            log::warn!(
                "Codex model_catalog_json 经符号链接解析到配置目录外: {} -> {}（允许目录: {}）",
                resolved.display(),
                canonical.display(),
                canonical_base.display()
            );
            return None;
        }
        return Some(canonical);
    }

    Some(resolved)
}

/// Pure reverse-parsing core: convert Codex catalog JSON text back into the
/// frontend's simplified model-mapping shape. Returns `None` when the catalog
/// is unparseable, has no `models` array, or yields zero valid entries.
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

        // Preserve native-profile per-row overrides so a DB-SSOT-missing
        // fallback round-trip doesn't silently drop them.
        if let Some(parallel) = entry
            .get("supports_parallel_tool_calls")
            .and_then(|v| v.as_bool())
        {
            obj.insert("supportsParallelToolCalls".to_string(), json!(parallel));
        }
        if let Some(modalities) = entry.get("input_modalities").and_then(|v| v.as_array()) {
            let mods: Vec<String> = modalities
                .iter()
                .filter_map(|m| m.as_str())
                .map(str::to_string)
                .collect();
            let inferred = codex_catalog_input_modalities(model, None);
            if !mods.is_empty() && mods != inferred {
                obj.insert("inputModalities".to_string(), json!(mods));
            }
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
    profile: CodexCatalogToolProfile,
) -> Result<String, AppError> {
    if settings.get("modelCatalog").is_some() {
        prepare_codex_config_text_with_model_catalog(settings, config_text, profile)
    } else {
        Ok(config_text.to_string())
    }
}

pub fn write_codex_provider_live_with_catalog(
    settings: &Value,
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
    profile: CodexCatalogToolProfile,
) -> Result<(), AppError> {
    let prepared_config = config_text
        .map(|text| prepare_codex_config_text_with_model_catalog(settings, text, profile))
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
        // `as_table_like` (not `as_table`): user configs may use inline tables
        // (`model_providers = { foo = {...} }`), which `as_table` rejects.
        Some(id) if is_custom_codex_model_provider_id(id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get(id))
            .and_then(|item| item.as_table_like())
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

/// Whether a provider's `http_headers` / `env_http_headers` table carries an
/// `Authorization` entry. Header names are case-insensitive on the wire, so
/// match TOML keys case-insensitively too.
fn table_declares_authorization_header(item: Option<&toml_edit::Item>) -> bool {
    item.and_then(|item| item.as_table_like())
        .is_some_and(|table| {
            table
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("authorization"))
        })
}

/// Whether this provider table resolves its auth from `auth.json` on Codex
/// 0.149. `resolve_provider_auth` short-circuits on `env_key` /
/// `experimental_bearer_token`; with neither,
/// `requires_openai_auth = false` resolves to the unauthenticated provider —
/// it never reads `auth.json`, no matter what the table carries (x-api-key
/// headers, query params, or nothing at all for local servers). Only
/// `requires_openai_auth = true` without a short-circuit falls through to
/// the official login.
///
/// `auth` / `aws` are deliberately NOT short-circuits here: 0.149 validates
/// both as mutually exclusive with `requires_openai_auth` (and `aws` is
/// Bedrock-only anyway), so a `requires_openai_auth = true` table carrying
/// them is a dead config the whole file fails to load with. Treating them
/// as "own credentials" would wave that dead config through the safety
/// gate; flagging it keeps it from being written.
fn codex_provider_table_falls_back_to_official_auth(table: &dyn toml_edit::TableLike) -> bool {
    table
        .get("requires_openai_auth")
        .and_then(|item| item.as_bool())
        .unwrap_or(false)
        && table.get("env_key").is_none()
        && table.get("experimental_bearer_token").is_none()
}

/// Codex 0.149 guard: a provider table that already declares its own
/// credential source must not receive an injected bearer token. `auth` /
/// `aws` sub-tables hard-conflict with `experimental_bearer_token` at
/// deserialization — the whole config.toml fails to parse and Codex refuses
/// to start. `env_key` outranks the token at runtime, so injection buys
/// nothing and only leaks the key into config.toml. An explicit
/// `Authorization` in `http_headers` / `env_http_headers` is how header-auth
/// providers survive on 0.149 — auth is applied after provider headers and
/// would overwrite it.
///
/// `requires_openai_auth` is deliberately NOT part of this guard, and it
/// even disables the header check: without an injected token,
/// `requires_openai_auth = true` routes auth to the preserved `auth.json`
/// OAuth login, which is applied after provider headers and would send the
/// official credentials to the third-party endpoint. The injected token
/// short-circuits that (the preservation-mode bridge contract); a
/// contradictory Authorization header loses either way on 0.149.
fn codex_provider_table_declares_auth(table: &dyn toml_edit::TableLike) -> bool {
    let requires_openai_auth = table
        .get("requires_openai_auth")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    table.get("auth").is_some()
        || table.get("aws").is_some()
        || table.get("env_key").is_some()
        || (!requires_openai_auth
            && (table_declares_authorization_header(table.get("http_headers"))
                || table_declares_authorization_header(table.get("env_http_headers"))))
}

/// Whether a config routes requests away from the official provider while
/// offering no custom provider table to carry a bearer token: a custom
/// `model_provider` whose table is missing, or a built-in/unset provider
/// rerouted by a top-level `openai_base_url`. In both shapes the token can
/// only land at the top level, which Codex 0.149 ignores — on a config-only
/// switch the preserved `auth.json` credentials would be sent to the
/// third-party endpoint. Configs without any routing directive are fine:
/// they leave Codex on the official provider, and the top-level token is
/// cc-switch's own record (extract/backfill), never read by Codex.
fn codex_config_routes_third_party_without_token_slot(config_text: &str) -> bool {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        // Syntactically invalid TOML is rejected later by the write validators.
        return false;
    };
    match active_codex_model_provider_id(&doc) {
        Some(id) if is_custom_codex_model_provider_id(&id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get(&id))
            .and_then(|item| item.as_table_like())
            .is_none(),
        _ => doc
            .get("openai_base_url")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .is_some_and(|url| !url.is_empty()),
    }
}

/// Whether a config with NO injectable API key still routes third-party
/// traffic through the `auth.json` fallback. On 0.149 a custom provider
/// with `requires_openai_auth = true` and no `env_key` /
/// `experimental_bearer_token` short-circuit resolves to whatever `auth.json`
/// holds — under login preservation that is the official OAuth login,
/// applied after provider headers, so even an explicit
/// `http_headers.Authorization` is overwritten and the ChatGPT access
/// token + account id go to the third-party endpoint. A top-level
/// `openai_base_url` reroutes the built-in `openai` provider the same way
/// (other built-ins never read the OAuth login). With a token present the
/// injected bearer short-circuits the fallback instead (bridge contract),
/// so this predicate only matters on the no-token path.
fn codex_config_falls_back_to_official_auth_for_third_party(config_text: &str) -> bool {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        // Syntactically invalid TOML is rejected later by the write validators.
        return false;
    };
    let openai_base_url_reroutes = || {
        doc.get("openai_base_url")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .is_some_and(|url| !url.is_empty())
    };
    match active_codex_model_provider_id(&doc) {
        Some(id) if is_custom_codex_model_provider_id(&id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get(&id))
            .and_then(|item| item.as_table_like())
            .is_some_and(codex_provider_table_falls_back_to_official_auth),
        Some(id) if id == "openai" => openai_base_url_reroutes(),
        None => openai_base_url_reroutes(),
        // Other reserved built-ins (ollama, lmstudio, bedrock…) have their
        // own auth paths and never fall back to the OAuth login.
        Some(_) => false,
    }
}

/// cc-switch-owned provider id used by the legacy-shape normalization below.
/// Not a Codex reserved id, so an injected token lands inside the table.
const CODEX_MIGRATED_PROVIDER_ID: &str = "cc-switch";

/// Pick the first free cc-switch-owned provider id (`cc-switch`,
/// `cc-switch-2`, …) so migrations never overwrite a user-authored table.
fn first_free_cc_switch_provider_id(model_providers: Option<&dyn toml_edit::TableLike>) -> String {
    let mut candidate = CODEX_MIGRATED_PROVIDER_ID.to_string();
    let mut suffix = 2usize;
    while model_providers.is_some_and(|table| table.get(&candidate).is_some()) {
        candidate = format!("{CODEX_MIGRATED_PROVIDER_ID}-{suffix}");
        suffix += 1;
    }
    candidate
}

/// The reserved built-in ids whose `[model_providers.<id>]` tables make
/// Codex reject the WHOLE config at load (`validate_reserved_model_provider_ids`,
/// present since 0.148, case-sensitive; the bedrock ids are exempt).
const CODEX_STALE_RESERVED_TABLE_IDS: &[&str] = &["openai", "ollama", "lmstudio"];

/// Migrate stale reserved provider tables (`[model_providers.openai]`,
/// `.ollama`, `.lmstudio`). Codex rejects the WHOLE config at load when one
/// of these reserved built-in ids is overridden, so any surviving table
/// means "switch reports success, Codex refuses to start" — older cc-switch
/// takeover projections created exactly these shapes.
///
/// The reserved-id match is EXACT, mirroring upstream: `OpenAI` and other
/// case variants are legitimate custom ids and must not be touched. Each
/// table is renamed losslessly to the first free cc-switch id (nothing
/// proves which of its keys the user cares about), with
/// `wire_api = "responses"` defaulted in — all three built-ins speak
/// Responses on 0.149.
///
/// Route policy: when the renamed table was the active route, a third-party
/// write follows to the migrated id unless the table would resolve its auth
/// from auth.json (`codex_provider_table_falls_back_to_official_auth`) with
/// no injectable token to short-circuit it. Tables that never fall back —
/// own credentials (env_key / experimental_bearer_token),
/// header or query-param auth, or unauthenticated local servers — keep
/// their legitimate route; only a credential-less
/// `requires_openai_auth = true` table without a token snaps back to the
/// built-in provider, because following it would send the preserved OAuth
/// login to a stale address. The renamed table is also normalized into a
/// shape 0.149 will load: `wire_api` forced to "responses" (the chat wire
/// API was removed; any other value fails deserialization of the whole
/// config) and an empty/missing `name` backfilled (rejected at load
/// otherwise, active or not). The shape never loaded since 0.148, so there
/// is no prior behavior to preserve. Official writes never follow — an
/// official card's route belongs to the built-in provider. Returns None
/// when there is nothing to migrate.
fn migrate_stale_reserved_provider_tables(
    config_text: &str,
    official: bool,
    has_token: bool,
) -> Result<Option<String>, AppError> {
    if !config_text.contains("model_providers") {
        return Ok(None);
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let stale_ids: Vec<&str> = CODEX_STALE_RESERVED_TABLE_IDS
        .iter()
        .copied()
        .filter(|id| {
            doc.get("model_providers")
                .and_then(|item| item.as_table_like())
                .and_then(|table| table.get(id))
                .and_then(|item| item.as_table_like())
                .is_some()
        })
        .collect();
    if stale_ids.is_empty() {
        return Ok(None);
    }

    for stale_id in stale_ids {
        let migrated_id = first_free_cc_switch_provider_id(
            doc.get("model_providers")
                .and_then(|item| item.as_table_like()),
        );
        // `model_provider` unset defaults to the built-in openai provider.
        let table_is_active_route = match active_codex_model_provider_id(&doc) {
            None => stale_id == "openai",
            Some(active) => active == stale_id,
        };

        let Some(model_providers) = doc
            .get_mut("model_providers")
            .and_then(|item| item.as_table_like_mut())
        else {
            return Ok(None);
        };
        let Some(mut stale_item) = model_providers.remove(stale_id) else {
            continue;
        };
        let mut falls_back_to_official = false;
        if let Some(table) = stale_item.as_table_like_mut() {
            // 0.149 removed the chat wire API entirely: `wire_api = "chat"`
            // (or any other non-"responses" value) fails deserialization for
            // the WHOLE config, so normalize unconditionally. These tables
            // never loaded since 0.148 — there is no prior behavior to keep.
            if table.get("wire_api").and_then(|item| item.as_str()) != Some("responses") {
                table.insert("wire_api", toml_edit::value("responses"));
            }
            // Non-bedrock tables with an empty/missing `name` are rejected at
            // load ("provider name must not be empty"), active or not — the
            // legacy update path created name-less tables.
            if table
                .get("name")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .is_none()
            {
                table.insert("name", toml_edit::value("Custom"));
            }
            falls_back_to_official = codex_provider_table_falls_back_to_official_auth(&*table);
        }
        model_providers.insert(&migrated_id, stale_item);

        // Follow the rename whenever the table cannot leak the official
        // login: an injected token short-circuits the auth.json fallback,
        // and a table that never falls back (own credentials, header/query
        // auth, or unauthenticated local servers) keeps its legitimate
        // third-party route. Only a credential-less
        // `requires_openai_auth = true` table without a token snaps back to
        // the built-in provider — following it would send the preserved
        // OAuth login to the stale base_url.
        if table_is_active_route && !official && (has_token || !falls_back_to_official) {
            doc["model_provider"] = toml_edit::value(migrated_id.as_str());
        }
    }

    Ok(Some(doc.to_string()))
}

/// Codex 0.149 rejects the WHOLE config at deserialization when any
/// non-Bedrock provider table has an empty/missing `name` — active or not
/// ("provider name must not be empty"). Historic cc-switch updates and
/// hand-written configs created tables carrying only `base_url`, so every
/// live write normalizes custom tables into a loadable shape; the name is
/// cosmetic, so the table id is as good a value as any. Bedrock tables are
/// the opposite: 0.149 only lets them override
/// base_url/auth/http_headers/aws.*, and any other non-default field —
/// `name` included — fails the built-in merge for the whole config, so the
/// reserved ids are skipped entirely.
fn backfill_codex_custom_provider_names(config_text: &str) -> Result<Option<String>, AppError> {
    if !config_text.contains("model_providers") {
        return Ok(None);
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_like_mut())
    else {
        return Ok(None);
    };

    let ids: Vec<String> = model_providers
        .iter()
        .filter(|(id, item)| {
            is_custom_codex_model_provider_id(id) && item.as_table_like().is_some()
        })
        .map(|(id, _)| id.to_string())
        .collect();
    let mut changed = false;
    for id in ids {
        let Some(table) = model_providers
            .get_mut(&id)
            .and_then(toml_edit::Item::as_table_like_mut)
        else {
            continue;
        };
        if table
            .get("name")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .is_none()
        {
            table.insert("name", toml_edit::value(id.as_str()));
            changed = true;
        }
    }
    Ok(changed.then(|| doc.to_string()))
}

/// Codex 0.149 validates EVERY provider table at deserialization — active
/// or not — and rejects the whole config over field combinations it
/// forbids: `aws` outside the two Bedrock built-ins, and a command-backed
/// `auth` combined with `requires_openai_auth` / `env_key` /
/// `experimental_bearer_token` (ModelProviderInfo::validate). None of these
/// can be normalized away (dropping user-authored fields is not ours to
/// do), so the switch path refuses up front with an actionable error
/// instead of writing a config Codex refuses to start on. Deliberately
/// called only from plan_codex_live_write: the gate-less paths (proxy
/// backup/restore) must not fail closed on the user's own backup.
fn preflight_codex_provider_table_conflicts(config_text: &str) -> Result<(), AppError> {
    if !config_text.contains("model_providers") {
        return Ok(());
    }
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        // Syntactically invalid TOML is rejected later by the write validators.
        return Ok(());
    };
    let Some(model_providers) = doc
        .get("model_providers")
        .and_then(|item| item.as_table_like())
    else {
        return Ok(());
    };
    for (id, item) in model_providers.iter() {
        let Some(table) = item.as_table_like() else {
            continue;
        };
        let is_bedrock = matches!(id, "amazon-bedrock" | "amazon-bedrock-runtime");
        if !is_bedrock && table.get("aws").is_some() {
            return Err(AppError::localized(
                "provider.codex.config.invalid_provider_table",
                format!(
                    "Codex 0.149 拒绝加载该配置：`aws` 字段仅允许用于内置的 amazon-bedrock / amazon-bedrock-runtime，[model_providers.{id}] 不能携带它。请移除该字段或改用 Bedrock 内置 id"
                ),
                format!(
                    "Codex 0.149 refuses to load this config: `aws` is only supported on the built-in amazon-bedrock / amazon-bedrock-runtime providers, so [model_providers.{id}] must not carry it. Remove the field or use a Bedrock built-in id"
                ),
            ));
        }
        if table.get("auth").is_some() {
            let requires_openai_auth = table
                .get("requires_openai_auth")
                .and_then(|item| item.as_bool())
                .unwrap_or(false);
            let conflict = if requires_openai_auth {
                Some("requires_openai_auth")
            } else if table.get("env_key").is_some() {
                Some("env_key")
            } else if table.get("experimental_bearer_token").is_some() {
                Some("experimental_bearer_token")
            } else {
                None
            };
            if let Some(conflict) = conflict {
                return Err(AppError::localized(
                    "provider.codex.config.invalid_provider_table",
                    format!(
                        "Codex 0.149 拒绝加载该配置：[model_providers.{id}] 的 `auth` 不能与 `{conflict}` 同时存在。请移除其中之一"
                    ),
                    format!(
                        "Codex 0.149 refuses to load this config: `auth` on [model_providers.{id}] cannot be combined with `{conflict}`. Remove one of them"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Rewrite the legacy "reroute the built-in openai provider" shape —
/// `model_provider` unset/"openai" plus a top-level `openai_base_url` — into
/// a custom provider table named `cc-switch`. Before Codex 0.149 this shape
/// worked because the built-in provider read the third-party key from
/// auth.json (ambient auth); auth.json no longer carries third-party keys,
/// so the key needs a provider-scoped slot. The built-in `openai` provider
/// speaks the Responses wire protocol, so the table pins
/// `wire_api = "responses"` and traffic semantics stay unchanged.
fn normalize_codex_legacy_openai_reroute(config_text: &str) -> Result<Option<String>, AppError> {
    if !config_text.contains("openai_base_url") {
        return Ok(None);
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    // Exact match, mirroring upstream: `openai_base_url` reroutes only the
    // built-in provider, and the built-in lookup is case-sensitive — a
    // config routing to `OpenAI` targets a custom table, not the knob.
    let targets_built_in_openai = match active_codex_model_provider_id(&doc) {
        None => true,
        Some(id) => id == "openai",
    };
    if !targets_built_in_openai {
        return Ok(None);
    }
    let Some(base_url) = doc
        .get("openai_base_url")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
    else {
        return Ok(None);
    };
    // `model_providers` present but not any table shape (scalar garbage):
    // leave it to the safety gates instead of guessing. Inline tables ARE
    // handled — proxy backup/restore call prepare without the gates, so
    // skipping them would leave the key in a dead top-level field next to
    // live auth.json credentials.
    if let Some(item) = doc.get("model_providers") {
        if item.as_table_like().is_none() {
            return Ok(None);
        }
    }

    // A user-authored table may already claim our id: nothing proves it is
    // ours to overwrite (their headers/query params would be lost and later
    // backfilled into the DB for good), so pick the first free suffixed id
    // instead. Idempotency is unaffected: a normalized config routes to the
    // migrated id, so this function early-returns before reaching here.
    let migrated_id = first_free_cc_switch_provider_id(
        doc.get("model_providers")
            .and_then(|item| item.as_table_like()),
    );

    doc.as_table_mut().remove("openai_base_url");
    doc["model_provider"] = toml_edit::value(migrated_id.as_str());

    // Match the container's own style: a standard table gets a sub-table, an
    // inline `model_providers = { … }` gets an inline member.
    let container_is_inline = doc
        .get("model_providers")
        .is_some_and(|item| item.as_table().is_none());
    if doc.get("model_providers").is_none() {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc.insert("model_providers", toml_edit::Item::Table(table));
    }
    let Some(model_providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_like_mut())
    else {
        return Ok(None);
    };
    if container_is_inline {
        let mut provider_table = toml_edit::InlineTable::new();
        provider_table.insert("name", "Custom".into());
        provider_table.insert("base_url", base_url.into());
        provider_table.insert("wire_api", "responses".into());
        model_providers.insert(
            &migrated_id,
            toml_edit::Item::Value(toml_edit::Value::InlineTable(provider_table)),
        );
    } else {
        let mut provider_table = toml_edit::Table::new();
        provider_table.insert("name", toml_edit::value("Custom"));
        provider_table.insert("base_url", toml_edit::value(base_url));
        provider_table.insert("wire_api", toml_edit::value("responses"));
        model_providers.insert(&migrated_id, toml_edit::Item::Table(provider_table));
    }

    Ok(Some(doc.to_string()))
}

/// Align the active custom provider table's `requires_openai_auth` with the
/// login-preservation setting on a third-party switch.
///
/// On Codex 0.149 the flag never decides request auth for these tables —
/// `resolve_provider_auth` short-circuits on `env_key` /
/// `experimental_bearer_token` before consulting it — but it does drive the
/// login UX: `true` with no login in `auth.json` traps the TUI in the
/// login/onboarding screen (preservation off deletes the file on every
/// third-party switch), while `false` next to a preserved ChatGPT login
/// makes Codex treat the session as logged out (account state hidden, the
/// preserved tokens never refreshed). Stored third-party configs cannot be
/// trusted here: presets and the custom template carried
/// `requires_openai_auth = true` from the pre-0.149 era when auth.json held
/// the third-party key, so the stamp overrides whatever the card says.
///
/// Only tables that short-circuit request auth (`env_key` or an
/// injected/stored `experimental_bearer_token`) are touched. Stamping
/// `true` on a table without a short-circuit would route request auth to
/// the preserved official OAuth login — the exact leak the safety gates
/// refuse — and keyless header-auth or local-server tables must keep their
/// user-authored shape (0.149 keeps them unauthenticated either way).
fn align_codex_requires_openai_auth_with_login_preservation(
    config_text: &str,
    preserve_official_login: bool,
) -> Result<String, AppError> {
    if !config_text.contains("model_providers") {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let Some(provider_id) = active_codex_model_provider_id(&doc) else {
        return Ok(config_text.to_string());
    };
    if !is_custom_codex_model_provider_id(&provider_id) {
        return Ok(config_text.to_string());
    }
    let Some(provider_table) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_like_mut())
        .and_then(|table| table.get_mut(provider_id.as_str()))
        .and_then(|item| item.as_table_like_mut())
    else {
        return Ok(config_text.to_string());
    };
    let short_circuits_request_auth = provider_table.get("experimental_bearer_token").is_some()
        || provider_table.get("env_key").is_some();
    if !short_circuits_request_auth {
        return Ok(config_text.to_string());
    }
    if provider_table
        .get("requires_openai_auth")
        .and_then(|item| item.as_bool())
        == Some(preserve_official_login)
    {
        return Ok(config_text.to_string());
    }
    provider_table.insert(
        "requires_openai_auth",
        toml_edit::value(preserve_official_login),
    );
    Ok(doc.to_string())
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

    // `as_table_like_mut` (not `as_table_mut`): inline tables would return
    // None and silently divert the token to the top level, where Codex 0.149
    // has no such field and ignores it (401 persists). Same pitfall as
    // `update_codex_toml_field`.
    if let Some(provider_table) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_like_mut())
        .and_then(|table| table.get_mut(provider_id.as_str()))
        .and_then(|item| item.as_table_like_mut())
    {
        if codex_provider_table_declares_auth(&*provider_table) {
            return Ok(config_text.to_string());
        }
        provider_table.insert("experimental_bearer_token", toml_edit::value(token));
        return Ok(doc.to_string());
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
            .and_then(|item| item.as_table_like_mut())
            .and_then(|table| table.get_mut(provider_id.as_str()))
            .and_then(|item| item.as_table_like_mut())
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
/// is still importable; both files missing is treated as "no live install".
/// A `config.toml` that exists but is empty is a valid state — e.g. the
/// official seed after stale-auth cleanup — and must stay readable.
pub fn read_codex_live_settings() -> Result<Value, AppError> {
    let auth_path = get_codex_auth_path();
    let auth_present = auth_path.exists();
    let auth: Value = if auth_present {
        read_json_file(&auth_path)?
    } else {
        json!({})
    };
    let cfg_text = read_and_validate_codex_config_text()?;
    if !auth_present && !get_codex_config_path().exists() {
        return Err(AppError::localized(
            "codex.live.missing",
            "Codex 配置文件不存在",
            "Codex configuration is missing",
        ));
    }
    Ok(json!({ "auth": auth, "config": cfg_text }))
}

/// `[model_providers.custom]` entry that makes an official (ChatGPT OAuth)
/// provider behave like Codex's built-in `openai` entry while running under
/// the shared custom id: `requires_openai_auth` routes auth to the ChatGPT
/// login in `auth.json` (base_url then defaults to the official Codex
/// backend), `name = "OpenAI"` keeps Codex's `is_openai()` feature gates
/// (web search, remote compaction), and `supports_websockets` restores the
/// built-in default that custom entries otherwise lose.
fn codex_official_provider_table(
    base_url: Option<&str>,
    supports_websockets: bool,
) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table["name"] = toml_edit::value("OpenAI");
    table["requires_openai_auth"] = toml_edit::value(true);
    table["supports_websockets"] = toml_edit::value(supports_websockets);
    table["wire_api"] = toml_edit::value("responses");
    if let Some(base_url) = base_url {
        table["base_url"] = toml_edit::value(base_url.trim_end_matches('/'));
    }
    table
}

fn codex_unified_official_provider_table() -> toml_edit::Table {
    codex_official_provider_table(None, true)
}

fn remove_codex_proxy_placeholders_from_providers(providers: &mut toml_edit::Table) {
    for (_, item) in providers.iter_mut() {
        if let Some(table) = item.as_table_mut() {
            let should_remove = table
                .get("experimental_bearer_token")
                .and_then(|item| item.as_str())
                == Some(CODEX_PROXY_AUTH_PLACEHOLDER);
            if should_remove {
                table.remove("experimental_bearer_token");
            }
        } else if let Some(table) = item.as_inline_table_mut() {
            let should_remove = table
                .get("experimental_bearer_token")
                .and_then(|value| value.as_str())
                == Some(CODEX_PROXY_AUTH_PLACEHOLDER);
            if should_remove {
                table.remove("experimental_bearer_token");
            }
        }
    }
}

/// Project a Codex official account card through the local proxy while keeping
/// authentication owned by Codex itself.
///
/// The resulting custom provider explicitly opts into OpenAI authentication,
/// so Codex forwards its existing ChatGPT login to the local `/responses`
/// endpoint.  No API key or bearer placeholder is written to `auth.json`.
pub fn apply_codex_official_proxy_route(
    config_text: &str,
    proxy_base_url: &str,
) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    // A third-party takeover may have left the proxy placeholder in config.toml.
    // The official route must use Codex's native OpenAI login instead.
    doc.as_table_mut().remove("experimental_bearer_token");
    doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID);

    let mut providers = match doc.as_table_mut().remove("model_providers") {
        Some(item) => item.into_table().map_err(|_| {
            AppError::Message(
                "Invalid Codex config.toml: model_providers must be a table".to_string(),
            )
        })?,
        None => {
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            table
        }
    };

    // Clean only CC Switch's placeholder from every stale provider table. Real
    // user bearer tokens are preserved, as are all unrelated provider fields.
    remove_codex_proxy_placeholders_from_providers(&mut providers);

    // The local proxy currently exposes HTTP/SSE, not Codex websocket routes.
    let table = codex_official_provider_table(Some(proxy_base_url), false);

    providers.insert(
        CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID,
        toml_edit::Item::Table(table),
    );
    doc["model_providers"] = toml_edit::Item::Table(providers);
    Ok(doc.to_string())
}

/// Whether a live Codex config is the official route projected by CC Switch.
pub fn codex_config_has_official_proxy_route(config_text: &str) -> bool {
    if !config_text.contains(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID) {
        return false;
    }
    config_text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|doc| {
            doc.get("model_provider")
                .and_then(|item| item.as_str())
                .map(str::to_string)
        })
        .as_deref()
        == Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
}

/// Remove only the official takeover route owned by CC Switch. This is a
/// last-resort crash cleanup when no live backup or provider SSOT is usable.
pub fn remove_codex_official_proxy_route(config_text: &str) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    if doc.get("model_provider").and_then(|item| item.as_str())
        != Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
    {
        return Ok(config_text.to_string());
    }

    doc.as_table_mut().remove("model_provider");
    if let Some(item) = doc.as_table_mut().remove("model_providers") {
        let mut providers = item.into_table().map_err(|_| {
            AppError::Message(
                "Invalid Codex config.toml: model_providers must be a table".to_string(),
            )
        })?;
        providers.remove(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID);
        remove_codex_proxy_placeholders_from_providers(&mut providers);
        if !providers.is_empty() {
            doc["model_providers"] = toml_edit::Item::Table(providers);
        }
    }
    Ok(doc.to_string())
}

fn table_matches_codex_unified_official_provider(table: &toml_edit::Table) -> bool {
    table.len() == 4
        && table.get("name").and_then(|item| item.as_str()) == Some("OpenAI")
        && table
            .get("requires_openai_auth")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table
            .get("supports_websockets")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table.get("wire_api").and_then(|item| item.as_str()) == Some("responses")
}

/// 统一 Codex 会话历史：把官方供应商的 live 配置改写为以共享的
/// `custom` model_provider 标识运行（认证仍走 `auth.json` 的 ChatGPT 登录），
/// 使开关开启后创建的官方会话与第三方会话共用同一个 resume 历史桶。
///
/// 两种情况拒绝注入、原样返回：
/// - 配置已有显式 `model_provider`：用户手工指定的路由不被覆盖；
/// - 配置已有形态不同的 `[model_providers.custom]` 表：设置 `model_provider`
///   会激活这张我们不认识的表（可能带第三方 base_url/token，会把 ChatGPT
///   OAuth 流量路由到错误后端），宁可让开关对该配置不生效。
pub fn inject_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").is_some() {
        return Ok(config_text.to_string());
    }

    let existing_custom_conflicts = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(|table| !table_matches_codex_unified_official_provider(table));
    if existing_custom_conflicts {
        log::warn!(
            "官方 Codex 配置已存在自定义 [model_providers.custom]，跳过统一会话路由注入以避免激活未知路由"
        );
        return Ok(config_text.to_string());
    }

    doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);

    if doc.get("model_providers").is_none() {
        let mut parent = toml_edit::Table::new();
        parent.set_implicit(true);
        doc["model_providers"] = toml_edit::Item::Table(parent);
    }
    if let Some(providers) = doc["model_providers"].as_table_mut() {
        if !providers.contains_key(CC_SWITCH_CODEX_MODEL_PROVIDER_ID) {
            providers.insert(
                CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
                toml_edit::Item::Table(codex_unified_official_provider_table()),
            );
        }
    }
    Ok(doc.to_string())
}

/// `inject_codex_unified_session_bucket` 的反向操作：从配置文本里剥掉注入的
/// 统一会话路由，保证切换回填不会把它带进数据库的存储配置（关闭开关后
/// 切换即可完全还原）。仅当形态与注入产物完全一致时才剥离；第三方模板和
/// 用户自定义的 `custom` 条目（带 base_url 等差异字段）原样保留。
pub fn strip_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    if !config_text.contains("model_provider") {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").and_then(|item| item.as_str())
        != Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
    {
        return Ok(config_text.to_string());
    }
    let matches_injected = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(table_matches_codex_unified_official_provider);
    if !matches_injected {
        return Ok(config_text.to_string());
    }

    doc.as_table_mut().remove("model_provider");
    let providers_empty = doc["model_providers"]
        .as_table_mut()
        .map(|providers| {
            providers.remove(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
            providers.is_empty()
        })
        .unwrap_or(false);
    if providers_empty {
        doc.as_table_mut().remove("model_providers");
    }
    Ok(doc.to_string())
}

/// 统一会话开关开启时，把官方供应商 `{ auth, config }` 设置对象中的
/// config 文本注入共享 custom 路由；开关关闭或非官方供应商时不做改动。
///
/// 普通 live 写入（`write_codex_live_for_provider`）与代理接管备份
/// （`update_live_backup_from_provider`）两条落盘路径共用：接管期间
/// live 归代理所有，注入必须进备份，接管释放恢复的 live 才带统一路由。
pub fn apply_codex_unified_session_bucket_to_settings(
    category: Option<&str>,
    settings: &mut Value,
) -> Result<(), AppError> {
    if category != Some("official") || !crate::settings::unify_codex_session_history() {
        return Ok(());
    }
    let config_text = settings
        .get("config")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let injected = inject_codex_unified_session_bucket(&config_text)?;
    if injected != config_text {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(injected));
        }
    }
    Ok(())
}

/// Backfill helper: strip the unified-session injection from a live
/// `{ auth, config }` settings object before it is stored back to the DB.
pub fn strip_codex_unified_session_bucket_from_settings(
    settings: &mut Value,
) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };
    let stripped = strip_codex_unified_session_bucket(&config_text)?;
    if stripped != config_text {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(stripped));
        }
    }
    Ok(())
}

/// Backfill helper: strip `[mcp_servers]` from a live `{ auth, config }`
/// settings object before it is stored back to the DB.
///
/// MCP 服务器的 SSOT 是 DB 的 mcp_servers 表，live `config.toml` 里的
/// `[mcp_servers]` 只是每次写 live 之后由 MCP 同步重新投影的产物。若回填时
/// 烙进供应商存储配置，已在应用里删除的服务器会随下次激活该供应商被写回
/// live，而逐条 reconcile 只认识 DB 现存条目、永远清不掉这种孤儿。
pub fn strip_codex_mcp_servers_from_settings(settings: &mut Value) -> Result<(), AppError> {
    let Some(config_text) = settings
        .get("config")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return Ok(());
    };
    if !config_text.contains("mcp") {
        return Ok(());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let mut changed = doc.as_table_mut().remove("mcp_servers").is_some();
    // 历史错误格式 [mcp.servers] 一并清理（live 侧 MCP 同步也做同样迁移）
    if let Some(mcp_tbl) = doc.get_mut("mcp").and_then(|item| item.as_table_like_mut()) {
        if mcp_tbl.remove("servers").is_some() {
            changed = true;
        }
        if mcp_tbl.is_empty() {
            doc.as_table_mut().remove("mcp");
        }
    }
    if changed {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("config".to_string(), Value::String(doc.to_string()));
        }
    }
    Ok(())
}

/// Route a Codex live write between full auth+config or config-only.
///
/// Official providers with usable login material own `auth.json`. Third-party
/// providers only touch `config.toml` when the compatibility setting is enabled
/// so the user's ChatGPT login cache survives provider switches.
///
/// 统一会话开关开启时，官方配置在落盘前注入共享的 `custom` 路由
/// （见 `inject_codex_unified_session_bucket`）。
/// A computed Codex live write. All validation (legacy-shape normalization,
/// safety gates, token injection, TOML parsing) happens while building the
/// plan, so callers can preflight a switch — build and discard — before
/// committing any state, then execute the same computation for the real
/// write. Keeping validation and execution in one builder makes it
/// impossible for the two to drift apart.
struct CodexLiveWritePlan {
    write_full_auth: bool,
    config_text: Option<String>,
    remove_auth_file: bool,
}

fn plan_codex_live_write(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
    preserve_official_login: bool,
) -> Result<CodexLiveWritePlan, AppError> {
    // Semantic preflight over EVERY provider table (official and
    // third-party alike, idle tables included): field combinations 0.149
    // rejects at load can't be normalized away, so refuse the switch with
    // an actionable error instead of writing a config Codex won't start on.
    // Independent of the two auth-safety gates below — those only judge the
    // active route and are skipped when a key is carried.
    if let Some(text) = config_text {
        preflight_codex_provider_table_conflicts(text)?;
    }
    if category == Some("official") {
        // Official configs seeded by older cc-switch versions can carry
        // stale reserved tables too — Codex refuses those at load, so
        // migrate on every write path, not only third-party. Official
        // context: the route never follows the renamed table.
        let migrated = match config_text {
            Some(text) => migrate_stale_reserved_provider_tables(text, true, false)?,
            None => None,
        };
        let config_text = migrated.as_deref().or(config_text);
        // Official writes never go through prepare_codex_provider_live_config,
        // so normalize name-less custom tables here too — 0.149 validates
        // EVERY provider table at load, and an official config can carry
        // idle leftovers from older cc-switch versions.
        let named = match config_text {
            Some(text) => backfill_codex_custom_provider_names(text)?,
            None => None,
        };
        let config_text = named.as_deref().or(config_text);
        let unified_official_config = if crate::settings::unify_codex_session_history() {
            Some(inject_codex_unified_session_bucket(
                config_text.unwrap_or(""),
            )?)
        } else {
            None
        };
        let config_text = unified_official_config.as_deref().or(config_text);
        // Official cards own auth.json: a material-carrying login is written
        // in full, a material-less card follows the live login and only
        // writes config. Official auth never travels through config.toml.
        return Ok(CodexLiveWritePlan {
            write_full_auth: codex_auth_has_login_material(auth),
            config_text: config_text.map(str::to_string),
            remove_auth_file: false,
        });
    }

    // Third-party switches are config-only. Since Codex 0.149
    // (openai/codex#39214) custom providers no longer inherit ambient auth
    // from auth.json, so the API key travels as a provider-scoped
    // `experimental_bearer_token` in config.toml (honored since Codex 0.48).
    // auth.json is reserved for the official ChatGPT login: kept when the
    // preservation setting is on, deleted otherwise. It never carries
    // third-party keys, so a `requires_openai_auth = true` fallback has no
    // third-party credential to mis-send and pre-0.48 auth.json-only Codex
    // releases are the only casualty.
    // The key may live in auth.OPENAI_API_KEY or already sit in the config
    // text (e.g. `auth = {}` raw-edited providers) — mirror
    // prepare_codex_provider_live_config's token sources.
    let carried_key = extract_codex_api_key(Some(auth), config_text);

    // Stale reserved tables are migrated BEFORE the safety gates so the
    // gates judge the same text prepare will write (a mixed stale-table +
    // openai_base_url shape would otherwise be mis-refused). prepare
    // migrates again internally (idempotent) for the gate-less proxy paths.
    let migrated = match config_text {
        Some(text) => migrate_stale_reserved_provider_tables(text, false, carried_key.is_some())?,
        None => None,
    };
    let config_text = migrated.as_deref().or(config_text);

    // The legacy reroute shape (built-in `openai` provider + top-level
    // `openai_base_url`) has no provider table to carry the key — rewrite it
    // into a cc-switch-owned custom table before the safety gates run.
    // prepare_codex_provider_live_config normalizes again internally
    // (idempotent); the gates need the normalized text here.
    let normalized = match config_text {
        Some(text) if carried_key.is_some() => normalize_codex_legacy_openai_reroute(text)?,
        _ => None,
    };
    let config_text = normalized.as_deref().or(config_text);

    // The preservation setting decides whether the official login in
    // auth.json survives a third-party switch. Off means the file is
    // deleted — a lingering login next to a third-party route is the leak
    // shape the gates exist to prevent, and `{}` is not logout, the file
    // must go (see clear_stale_codex_live_auth_after_official_switch). The
    // active table's `requires_openai_auth` is stamped to match below, so
    // Codex's login UX agrees with the file state either way.
    let remove_auth_file = !preserve_official_login;

    let live_config = match config_text {
        Some(text) if !text.trim().is_empty() => {
            // Both safety gates protect the same invariant: the auth Codex
            // resolves for a third-party route must never come from
            // auth.json (official OAuth under preservation, nothing at all
            // otherwise — either way the switch would be broken or unsafe).
            if carried_key.is_some() && codex_config_routes_third_party_without_token_slot(text) {
                return Err(AppError::localized(
                    "provider.codex.config.no_custom_provider",
                    "Codex 第三方配置必须包含自定义 model_providers 条目以承载 API 密钥（Codex 不识别顶层 experimental_bearer_token）",
                    "A Codex third-party config must define a custom model_providers entry to carry the API key (Codex ignores a top-level experimental_bearer_token)",
                ));
            }
            if carried_key.is_none()
                && codex_config_falls_back_to_official_auth_for_third_party(text)
            {
                return Err(AppError::localized(
                    "provider.codex.config.official_auth_fallback",
                    "该 Codex 配置没有可用的 API 密钥，而 requires_openai_auth = true（或顶层 openai_base_url）会让 Codex 回退使用 auth.json 里的登录凭据访问第三方地址。请为供应商填写 API 密钥，或移除该回退指令",
                    "This Codex config has no usable API key, and requires_openai_auth = true (or a top-level openai_base_url) would make Codex fall back to whatever login auth.json holds for a third-party route. Add an API key to the provider or remove the fallback directive",
                ));
            }
            prepare_codex_provider_live_config(auth, text)?
        }
        // Empty config: with a key to carry this errs inside
        // set_codex_experimental_bearer_token (no table to attach it to);
        // without a key the empty config is passed through as-is.
        other => prepare_codex_provider_live_config(auth, other.unwrap_or(""))?,
    };
    // After injection, so the stamp sees the final credential shape. Only
    // this direct-switch plan stamps: the takeover subsystem preserves the
    // login unconditionally and keeps its existing config shapes.
    let live_config = align_codex_requires_openai_auth_with_login_preservation(
        &live_config,
        preserve_official_login,
    )?;

    Ok(CodexLiveWritePlan {
        write_full_auth: false,
        config_text: Some(live_config),
        remove_auth_file,
    })
}

/// Validate a Codex live write without touching the filesystem. Callers use
/// this to fail a provider switch BEFORE committing `current`: a write-layer
/// refusal after `current` moved would let the next switch backfill the old
/// live config into the new provider's DB row.
pub fn preflight_codex_live_write(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    plan_codex_live_write(
        category,
        auth,
        config_text,
        crate::settings::preserve_codex_official_auth_on_switch(),
    )
    .map(|_| ())
}

pub fn write_codex_live_for_provider(
    category: Option<&str>,
    auth: &Value,
    config_text: Option<&str>,
) -> Result<(), AppError> {
    let plan = plan_codex_live_write(
        category,
        auth,
        config_text,
        crate::settings::preserve_codex_official_auth_on_switch(),
    )?;
    if plan.write_full_auth {
        return write_codex_live_atomic(auth, plan.config_text.as_deref());
    }
    write_codex_live_config_atomic(plan.config_text.as_deref())?;
    // Config is already committed at this point, so a cleanup failure
    // degrades to a warning instead of reporting an unswitched state.
    if plan.remove_auth_file {
        remove_codex_live_auth_after_third_party_switch();
    }
    Ok(())
}

fn remove_codex_live_auth_after_third_party_switch() {
    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return;
    }
    if let Err(e) = delete_file(&auth_path) {
        log::warn!("Failed to remove auth.json after a third-party Codex switch: {e}");
    }
}

/// Build the live Codex config for provider switching.
///
/// The stored provider keeps its API key in `auth.OPENAI_API_KEY`. Live Codex
/// requests can use a provider-scoped `experimental_bearer_token`, so switching
/// providers only needs to update `config.toml`; `auth.json` stays as the user's
/// long-lived ChatGPT login cache.
///
/// This is the single normalize→inject entry point: every caller — provider
/// switches, takeover backup rebuilds (`preserve_codex_auth_in_backup`), and
/// restore (`preserve_codex_oauth_login_on_restore`) — gets the legacy
/// reroute migration, so a pre-0.149 `openai_base_url` shape can never leave
/// its key in a top-level field Codex ignores while auth.json credentials
/// stay live. Idempotent on already-normalized text.
pub fn prepare_codex_provider_live_config(
    auth: &Value,
    config_text: &str,
) -> Result<String, AppError> {
    let token = extract_codex_auth_api_key(auth)
        .or_else(|| extract_codex_experimental_bearer_token(config_text));

    // Unconditional: a stale reserved table makes Codex refuse the whole
    // config (0.148+), token or not. Third-party context — the route may
    // follow the renamed table when it can authenticate (see the migrator).
    let migrated = migrate_stale_reserved_provider_tables(config_text, false, token.is_some())?;
    let config_text = migrated.as_deref().unwrap_or(config_text);

    // Also unconditional (covers the keyless third-party path; the official
    // branch of plan_codex_live_write calls it separately): 0.149 rejects
    // the whole config over any name-less custom table, active or not.
    let named = backfill_codex_custom_provider_names(config_text)?;
    let config_text = named.as_deref().unwrap_or(config_text);

    let Some(token) = token else {
        return Ok(config_text.to_string());
    };
    let normalized = normalize_codex_legacy_openai_reroute(config_text)?;
    let config_text = normalized.as_deref().unwrap_or(config_text);
    set_codex_experimental_bearer_token(config_text, &token)
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
                .map(str::to_string);

            if let Some(provider_key) = model_provider {
                // validate_reserved_model_provider_ids（0.148 起）对配置里出现
                // `[model_providers.openai]` 等保留 id 表整份报错（"Built-in
                // providers cannot be overridden"），Codex 直接起不来。上游的
                // 保留判定是**大小写精确**的——`OpenAI` 等变体是合法自定义
                // id，照常走建表分支；bedrock 两个 id 被上游豁免，覆盖表合法。
                if provider_key == "openai" {
                    // 内置 openai 的改址走它的正统机制——顶层
                    // `openai_base_url`；wire_api 由 CLI 内置固定，无需写。
                    if field == "base_url" {
                        if trimmed.is_empty() {
                            doc.as_table_mut().remove("openai_base_url");
                        } else {
                            doc["openai_base_url"] = toml_edit::value(trimmed);
                        }
                    }
                    return Ok(doc.to_string());
                }
                if provider_key == "ollama" || provider_key == "lmstudio" {
                    // 这两个保留 id 没有等价的顶层旋钮：建表=生成 Codex 拒绝
                    // 加载的配置（接管期间整个 CLI 起不来），明确报错优于
                    // 静默写出致命配置。
                    return Err(format!(
                        "Codex 禁止覆盖内置 provider `{provider_key}`（0.148 起会拒绝加载整份配置），无法改写其 {field}；请改用自定义 provider id"
                    ));
                }

                // Ensure [model_providers] table exists
                //
                // 用 as_table_like_mut 而非 as_table_mut：用户把配置写成 inline table
                // （`model_providers = { foo = {...} }`，TOML 合法）时 as_table_mut
                // 返回 None，会一路掉进下面的顶层 fallback——用户改的 base_url 被写到
                // 了错误层级且毫无提示。
                if doc
                    .get("model_providers")
                    .is_none_or(|item| item.as_table_like().is_none())
                {
                    // 键存在但不是表（`model_providers = 42`）时，下面这行会把用户
                    // 手写的值替换掉。旧代码在这种形状下会掉进顶层 fallback 而不动
                    // 它，所以归一化必须留痕——与 mcp/codex.rs、mcp/grokbuild.rs、
                    // opencode_config.rs 的同款处理保持一致。
                    if doc
                        .get("model_providers")
                        .is_some_and(|item| !item.is_none())
                    {
                        log::warn!("config.toml 的 model_providers 不是表，已重置为空表");
                    }
                    doc["model_providers"] = toml_edit::table();
                }

                if let Some(model_providers) = doc
                    .get_mut("model_providers")
                    .and_then(toml_edit::Item::as_table_like_mut)
                {
                    // Ensure [model_providers.<provider_key>] table exists
                    if !model_providers.contains_key(&provider_key) {
                        model_providers.insert(&provider_key, toml_edit::table());
                    }

                    if let Some(provider_table) = model_providers
                        .get_mut(&provider_key)
                        .and_then(toml_edit::Item::as_table_like_mut)
                    {
                        // 0.149 在反序列化时就拒绝 name 为空/缺失的非 bedrock
                        // 表（"provider name must not be empty"，整份配置拒
                        // 载）——本函数正是历史上无 name 表的制造源头，建表
                        // /改表时必须保证 name 非空。反向豁免 bedrock 两个保留
                        // id（此分支唯一能到达的保留 id）：0.149 只允许它们覆盖
                        // base_url/auth/http_headers/aws.*，写入 name 会让内置
                        // 合并校验拒绝整份配置——代理接管改 base_url 正走此路。
                        if is_custom_codex_model_provider_id(&provider_key)
                            && provider_table
                                .get("name")
                                .and_then(|item| item.as_str())
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .is_none()
                        {
                            provider_table.insert("name", toml_edit::value(provider_key.as_str()));
                        }
                        if trimmed.is_empty() {
                            provider_table.remove(field);
                        } else {
                            provider_table.insert(field, toml_edit::value(trimmed));
                        }
                        return Ok(doc.to_string());
                    }
                }

                log::warn!(
                    "config.toml 的 [model_providers.{provider_key}] 结构异常，{field} 改写为顶层字段"
                );
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

    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::ffi::OsString;

    #[test]
    fn codex_id_token_user_identity_requires_a_nonempty_subject() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let subject_payload = URL_SAFE_NO_PAD.encode(json!({ "sub": "stable-user" }).to_string());
        assert_eq!(
            extract_codex_id_token_user_identity(&test_codex_id_token("stable-user")),
            Some("sub:stable-user".to_string())
        );
        assert_eq!(extract_codex_id_token_user_identity("not-a-jwt"), None);
        assert_eq!(
            extract_codex_id_token_user_identity(&format!("{header}.{subject_payload}")),
            None
        );
        assert_eq!(
            extract_codex_id_token_user_identity(&format!("{header}.{subject_payload}..extra")),
            None
        );
        assert_eq!(
            extract_codex_id_token_user_identity(&format!("invalid.{subject_payload}.signature")),
            None
        );
        assert_eq!(
            extract_codex_id_token_user_identity(&test_codex_id_token("   ")),
            None
        );

        let payload = URL_SAFE_NO_PAD.encode(json!({ "email": "user@example.test" }).to_string());
        assert_eq!(
            extract_codex_id_token_user_identity(&format!("{header}.{payload}.")),
            None
        );
    }

    struct CodexLiveTestHome {
        _dir: tempfile::TempDir,
        original_test_home: Option<OsString>,
    }

    impl CodexLiveTestHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create isolated Codex live test home");
            let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings for isolated test home");

            Self {
                _dir: dir,
                original_test_home,
            }
        }
    }

    impl Drop for CodexLiveTestHome {
        fn drop(&mut self) {
            match &self.original_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[derive(Debug, PartialEq)]
    struct CodexLiveTestState {
        auth_bytes: Vec<u8>,
        auth_value: Value,
        config_bytes: Vec<u8>,
        config_value: toml::Value,
        catalog_bytes: Vec<u8>,
        catalog_value: Value,
        marker_bytes: Vec<u8>,
        marker_value: Value,
    }

    fn capture_codex_live_test_state() -> CodexLiveTestState {
        let auth_bytes = fs::read(get_codex_auth_path()).expect("read live auth bytes");
        let config_bytes = fs::read(get_codex_config_path()).expect("read live config bytes");
        let catalog_bytes =
            fs::read(get_codex_model_catalog_path()).expect("read live catalog bytes");
        let marker_bytes = fs::read(get_codex_managed_oauth_live_auth_marker_path())
            .expect("read managed auth marker bytes");

        CodexLiveTestState {
            auth_value: serde_json::from_slice(&auth_bytes).expect("parse live auth"),
            config_value: toml::from_str(
                std::str::from_utf8(&config_bytes).expect("live config must be UTF-8"),
            )
            .expect("parse live config"),
            catalog_value: serde_json::from_slice(&catalog_bytes).expect("parse live catalog"),
            marker_value: serde_json::from_slice(&marker_bytes).expect("parse managed auth marker"),
            auth_bytes,
            config_bytes,
            catalog_bytes,
            marker_bytes,
        }
    }

    fn seed_rotated_managed_codex_live_state() -> CodexLiveTestState {
        let id_token = test_codex_id_token("user-a");
        let auth = codex_managed_oauth_auth_value(
            "account-a",
            "access-r1",
            Some(&id_token),
            "refresh-r1",
            "2026-08-06T00:00:01Z",
        );
        crate::config::write_json_file(&get_codex_auth_path(), &auth).expect("seed live auth R1");
        crate::config::write_text_file(
            &get_codex_config_path(),
            "# cas-guard-sentinel\nmodel = \"gpt-5.5\"\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n",
        )
        .expect("seed live config");
        crate::config::write_json_file(
            &get_codex_model_catalog_path(),
            &json!({ "models": [{ "slug": "cas-guard-sentinel" }] }),
        )
        .expect("seed live catalog");
        record_codex_managed_oauth_live_auth(&auth, "account-a").expect("seed managed auth marker");

        capture_codex_live_test_state()
    }

    #[test]
    #[serial]
    fn ensure_live_auth_guard_rejects_rotated_refresh_without_mutating_live_bundle() {
        let _home = CodexLiveTestHome::new();
        let before = seed_rotated_managed_codex_live_state();

        let result =
            ensure_codex_live_auth_unchanged_for_managed_account("account-a", "refresh-r0");

        assert!(result.is_err(), "R1 live auth must reject an expected R0");
        assert_eq!(capture_codex_live_test_state(), before);
    }

    #[test]
    #[serial]
    fn clear_live_auth_guard_rejects_rotated_refresh_without_mutating_live_bundle() {
        let _home = CodexLiveTestHome::new();
        let before = seed_rotated_managed_codex_live_state();

        let result =
            clear_codex_live_auth_for_managed_account_if_unchanged("account-a", Some("refresh-r0"));

        assert!(result.is_err(), "R1 live auth must reject an expected R0");
        assert_eq!(capture_codex_live_test_state(), before);
    }

    #[test]
    fn catalog_tool_profile_from_api_format() {
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(Some("anthropic")),
            CodexCatalogToolProfile::Anthropic
        );
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(Some("openai_responses")),
            CodexCatalogToolProfile::NativeResponses
        );
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(Some("openai_chat")),
            CodexCatalogToolProfile::ProxyChat
        );
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(None),
            CodexCatalogToolProfile::ProxyChat
        );
    }

    #[test]
    fn unified_session_bucket_injects_for_empty_official_config() {
        let injected = inject_codex_unified_session_bucket("").expect("inject");
        let doc: toml::Table = toml::from_str(&injected).expect("parse injected config");

        assert_eq!(
            doc.get("model_provider").and_then(|v| v.as_str()),
            Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
        );
        let custom = doc["model_providers"][CC_SWITCH_CODEX_MODEL_PROVIDER_ID]
            .as_table()
            .expect("custom provider table");
        assert_eq!(custom.get("name").and_then(|v| v.as_str()), Some("OpenAI"));
        assert_eq!(
            custom.get("requires_openai_auth").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            custom.get("supports_websockets").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            custom.get("wire_api").and_then(|v| v.as_str()),
            Some("responses")
        );
    }

    #[test]
    fn official_proxy_route_uses_native_auth_and_local_responses_provider() {
        let input = r#"model = "gpt-5.4"
experimental_bearer_token = "PROXY_MANAGED"

[mcp_servers.example]
command = "example"
"#;
        let output = apply_codex_official_proxy_route(input, "http://127.0.0.1:15721/v1")
            .expect("apply official proxy route");
        let doc: toml::Value = toml::from_str(&output).expect("parse output");

        assert_eq!(
            doc.get("model_provider").and_then(toml::Value::as_str),
            Some(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
        );
        assert!(doc.get("experimental_bearer_token").is_none());
        assert!(
            doc.get("mcp_servers").is_some(),
            "unrelated config survives"
        );

        let provider = &doc["model_providers"][CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID];
        assert_eq!(
            provider.get("base_url").and_then(toml::Value::as_str),
            Some("http://127.0.0.1:15721/v1")
        );
        assert_eq!(
            provider
                .get("requires_openai_auth")
                .and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            provider
                .get("supports_websockets")
                .and_then(toml::Value::as_bool),
            Some(false)
        );
        assert!(codex_config_has_official_proxy_route(&output));
    }

    #[test]
    fn official_proxy_route_cleanup_only_removes_owned_provider() {
        let projected =
            apply_codex_official_proxy_route("model = \"gpt-5.4\"\n", "http://127.0.0.1:15721/v1")
                .expect("project");
        let cleaned = remove_codex_official_proxy_route(&projected).expect("clean");
        let doc: toml::Value = toml::from_str(&cleaned).expect("parse cleaned");
        assert!(doc.get("model_provider").is_none());
        assert!(doc.get("model_providers").is_none());
        assert_eq!(
            doc.get("model").and_then(toml::Value::as_str),
            Some("gpt-5.4")
        );
    }

    #[test]
    fn official_proxy_route_rejects_non_table_model_providers_without_panicking() {
        for input in [
            "model_providers = 3\n",
            "[[model_providers]]\nname = \"broken\"\n",
        ] {
            let result = apply_codex_official_proxy_route(input, "http://127.0.0.1:15721/v1");
            assert!(result.is_err());
        }
    }

    #[test]
    fn official_proxy_route_normalizes_inline_tables_and_cleans_stale_placeholder() {
        let input = r#"model_provider = "rightcode"
model_providers = { rightcode = { name = "RightCode", experimental_bearer_token = "PROXY_MANAGED" } }
"#;
        let projected = apply_codex_official_proxy_route(input, "http://127.0.0.1:15721/v1")
            .expect("project inline provider table");
        let projected_doc: toml::Value = toml::from_str(&projected).expect("parse projected");
        assert!(projected_doc["model_providers"]["rightcode"]
            .get("experimental_bearer_token")
            .is_none());
        assert!(projected_doc["model_providers"]
            .get(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
            .is_some());

        let cleaned = remove_codex_official_proxy_route(&projected).expect("clean projected");
        let cleaned_doc: toml::Value = toml::from_str(&cleaned).expect("parse cleaned");
        assert!(cleaned_doc.get("model_provider").is_none());
        assert!(cleaned_doc["model_providers"].get("rightcode").is_some());
        assert!(cleaned_doc["model_providers"]
            .get(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID)
            .is_none());
    }

    #[test]
    fn unified_session_bucket_preserves_other_keys_and_explicit_routing() {
        let with_catalog = "model_catalog_json = \"cc-switch-model-catalog.json\"\n";
        let injected = inject_codex_unified_session_bucket(with_catalog).expect("inject");
        assert!(injected.contains("model_catalog_json"));
        assert!(injected.contains("model_provider = \"custom\""));

        // 用户显式指定过 model_provider 的官方配置不被覆盖
        let explicit = "model_provider = \"openai_https\"\n";
        let unchanged = inject_codex_unified_session_bucket(explicit).expect("inject");
        assert_eq!(unchanged, explicit);
    }

    #[test]
    fn unified_session_bucket_skips_conflicting_custom_table() {
        // 残留的非注入形态 custom 表：设置 model_provider 会把官方流量
        // 路由到表里的第三方端点，必须整体拒绝注入。
        let stale = r#"[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
"#;
        let unchanged = inject_codex_unified_session_bucket(stale).expect("inject");
        assert_eq!(unchanged, stale);

        // 已是注入形态的 custom 表（如重复注入）则照常补上 model_provider
        let injected_once = inject_codex_unified_session_bucket("").expect("inject");
        let reinjected = inject_codex_unified_session_bucket(&injected_once).expect("re-inject");
        assert_eq!(reinjected, injected_once);
    }

    #[test]
    fn unified_session_bucket_strip_round_trips_injection() {
        let injected = inject_codex_unified_session_bucket("").expect("inject");
        let stripped = strip_codex_unified_session_bucket(&injected).expect("strip");
        assert_eq!(stripped.trim(), "");

        let with_catalog = "model_catalog_json = \"cc-switch-model-catalog.json\"\n";
        let injected = inject_codex_unified_session_bucket(with_catalog).expect("inject");
        let stripped = strip_codex_unified_session_bucket(&injected).expect("strip");
        assert_eq!(stripped, with_catalog);
    }

    #[test]
    fn unified_session_bucket_strip_keeps_third_party_custom_entry() {
        // 第三方模板同样用 custom 路由，但条目带 base_url 等差异字段，
        // 形态不等于注入产物，必须原样保留。
        let third_party = r#"model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let untouched = strip_codex_unified_session_bucket(third_party).expect("strip");
        assert_eq!(untouched, third_party);
    }

    #[test]
    fn unified_session_bucket_strip_from_settings_only_touches_config() {
        let injected = inject_codex_unified_session_bucket("").expect("inject");
        let mut settings = json!({
            "auth": { "tokens": { "access_token": "secret" } },
            "config": injected,
        });
        strip_codex_unified_session_bucket_from_settings(&mut settings).expect("strip settings");
        assert_eq!(
            settings
                .get("config")
                .and_then(|v| v.as_str())
                .map(str::trim),
            Some("")
        );
        assert!(settings.pointer("/auth/tokens/access_token").is_some());
    }

    #[test]
    fn strip_mcp_servers_from_settings_removes_table_and_legacy_form() {
        let mut settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-test" },
            "config": "# user comment\nmodel = \"gpt-5.5\"\n\n[mcp_servers.echo]\ntype = \"stdio\"\ncommand = \"echo\"\n\n[mcp.servers.legacy]\ncommand = \"noop\"\n",
        });
        strip_codex_mcp_servers_from_settings(&mut settings).expect("strip mcp");
        let config = settings
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config text");
        assert!(!config.contains("mcp_servers"), "got: {config}");
        assert!(
            !config.contains("[mcp"),
            "legacy [mcp.servers] gone: {config}"
        );
        assert!(config.contains("# user comment"), "comments preserved");
        assert!(config.contains("model = \"gpt-5.5\""));
    }

    #[test]
    fn strip_mcp_servers_from_settings_is_noop_without_mcp() {
        let original = "# comment\nmodel = \"gpt-5.5\"\n";
        let mut settings = json!({
            "auth": {},
            "config": original,
        });
        strip_codex_mcp_servers_from_settings(&mut settings).expect("strip mcp");
        assert_eq!(
            settings.get("config").and_then(|v| v.as_str()),
            Some(original),
            "config text must be byte-identical when nothing is stripped"
        );
    }

    #[test]
    fn extract_base_url_prefers_active_provider_section() {
        let input = r#"model_provider = "azure"

[model_providers.azure]
base_url = "https://azure.example.com/v1"

[model_providers.other]
base_url = "https://other.example.com/v1"
"#;

        assert_eq!(
            extract_codex_base_url(input).as_deref(),
            Some("https://azure.example.com/v1")
        );
    }

    #[test]
    fn extract_base_url_falls_back_to_top_level_only() {
        let top_level = r#"base_url = "https://top-level.example.com/v1""#;
        assert_eq!(
            extract_codex_base_url(top_level).as_deref(),
            Some("https://top-level.example.com/v1")
        );
    }

    // Mirrors the frontend extractCodexBaseUrl: a non-active provider section
    // is never a credential source, whether the active provider points
    // elsewhere (e.g. the built-in "openai") or none is selected at all.
    #[test]
    fn extract_base_url_ignores_non_active_provider_sections() {
        let mismatched = r#"model_provider = "openai"

[model_providers.custom]
base_url = "https://leftover.example.com/v1"
"#;
        assert_eq!(extract_codex_base_url(mismatched), None);

        let no_active = r#"[model_providers.any]
base_url = "https://single.example.com/v1"
"#;
        assert_eq!(extract_codex_base_url(no_active), None);
    }

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
    #[serial]
    fn managed_chatgpt_login_matches_local_marker_and_workspace() {
        let _home = CodexLiveTestHome::new();
        let shared_chatgpt_user_token = |subject: &str| {
            let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
            let payload = URL_SAFE_NO_PAD.encode(
                json!({
                    "sub": subject,
                    "https://api.openai.com/auth": {
                        "chatgpt_user_id": "shared-team-user-id"
                    }
                })
                .to_string(),
            );
            format!("{header}.{payload}.")
        };
        // 原生 auth 保留 workspace ID；marker 用本地 ID 区分同 workspace 登录。
        let full_bundle = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": shared_chatgpt_user_token("user-a"),
                "access_token": "access",
                "refresh_token": "refresh-secret",
                "account_id": "workspace-shared"
            },
            "last_refresh": "2026-01-02T03:04:05.000000000Z"
        });
        record_codex_managed_oauth_live_auth(&full_bundle, "local-account-a")
            .expect("record managed auth marker");
        crate::config::write_json_file(&get_codex_auth_path(), &full_bundle)
            .expect("write managed live auth");
        assert!(
            codex_live_auth_matches_managed_request("local-account-a", "access").unwrap(),
            "the selected account's exact live bearer must match"
        );
        assert!(
            !codex_live_auth_matches_managed_request("local-account-a", "other-access").unwrap(),
            "another user's bearer in the same workspace must not match"
        );
        let managed_id_token = full_bundle
            .pointer("/tokens/id_token")
            .and_then(Value::as_str)
            .expect("managed id token");
        assert!(
            codex_live_auth_is_managed_chatgpt_login(&full_bundle, "local-account-a"),
            "a full refreshable bundle for the managed account must be recognized"
        );
        assert!(
            !codex_live_auth_is_managed_chatgpt_login(&full_bundle, "local-account-b"),
            "another local login in the same workspace must not match"
        );
        let mut other_user = full_bundle.clone();
        other_user["tokens"]["id_token"] = json!(shared_chatgpt_user_token("user-b"));
        assert!(
            !codex_live_auth_is_managed_chatgpt_login(&other_user, "local-account-a"),
            "a native login for another user in the same workspace must not match"
        );
        crate::config::write_json_file(&get_codex_auth_path(), &other_user)
            .expect("write other user's native login");
        assert!(
            read_codex_live_auth_refresh_for_account("local-account-a").is_none(),
            "another user's refresh token must not be adopted"
        );
        clear_codex_live_auth_for_managed_account("local-account-a")
            .expect("clear stale local ownership marker");
        assert!(
            get_codex_auth_path().exists(),
            "removing local account A must not delete native account B"
        );

        crate::config::write_json_file(
            &get_codex_managed_oauth_live_auth_marker_path(),
            &json!({
                "version": 2,
                "account_id": "workspace-shared"
            }),
        )
        .expect("write legacy managed auth marker");
        assert!(
            read_codex_live_auth_refresh_for_managed_account(
                "workspace-shared",
                Some(managed_id_token),
            )
            .is_err(),
            "a legacy marker must not migrate across users in one workspace"
        );
        assert!(
            !codex_live_auth_is_managed_chatgpt_login(&other_user, "workspace-shared"),
            "a legacy marker without user identity must not establish ownership"
        );
        assert!(
            read_codex_live_auth_refresh_for_account("workspace-shared").is_none(),
            "a legacy marker must not authorize refresh-token adoption"
        );
        clear_codex_live_auth_for_managed_account("workspace-shared")
            .expect("clear ambiguous legacy marker");
        assert!(
            get_codex_auth_path().exists(),
            "clearing an ambiguous legacy marker must preserve native auth"
        );

        // 非 chatgpt 模式（API key）不应命中。
        let api_key_auth = json!({ "OPENAI_API_KEY": "sk-live" });
        assert!(!codex_live_auth_is_managed_chatgpt_login(
            &api_key_auth,
            "local-account-a"
        ));
    }

    #[test]
    #[serial]
    fn legacy_managed_marker_migrates_by_user_without_breaking_refresh_rollback() {
        let _home = CodexLiveTestHome::new();
        let id_token = test_codex_id_token("legacy-user");
        let auth_r0 = codex_managed_oauth_auth_value(
            "legacy-workspace",
            "access-r0",
            Some(&id_token),
            "refresh-r0",
            "2026-01-01T00:00:00Z",
        );
        crate::config::write_json_file(&get_codex_auth_path(), &auth_r0)
            .expect("write legacy live auth");
        crate::config::write_json_file(
            &get_codex_managed_oauth_live_auth_marker_path(),
            &json!({
                "version": 2,
                "account_id": "legacy-workspace"
            }),
        )
        .expect("write legacy marker");
        let snapshot = CodexLiveStateSnapshot::capture().expect("capture legacy generation");

        let migrated =
            read_codex_live_auth_refresh_for_managed_account("legacy-workspace", Some(&id_token))
                .expect("migrate matching legacy marker")
                .expect("read matching live refresh");
        assert_eq!(migrated.refresh_token, "refresh-r0");
        assert!(codex_live_auth_is_managed_chatgpt_login(
            &auth_r0,
            "legacy-workspace"
        ));

        let auth_r1 = codex_managed_oauth_auth_value(
            "legacy-workspace",
            "access-r1",
            Some(&id_token),
            "refresh-r1",
            "2026-01-02T00:00:00Z",
        );
        crate::config::write_json_file(&get_codex_auth_path(), &auth_r1)
            .expect("write rotated live auth");
        snapshot
            .restore_preserving_newer_same_account_auth()
            .expect("rollback after marker migration");

        let restored: Value = crate::config::read_json_file(&get_codex_auth_path())
            .expect("read preserved rotated auth");
        assert_eq!(restored, auth_r1);
        assert!(codex_live_auth_is_managed_chatgpt_login(
            &restored,
            "legacy-workspace"
        ));
    }

    #[test]
    #[serial]
    fn legacy_managed_marker_removal_requires_manager_identity() {
        let _home = CodexLiveTestHome::new();
        let id_token = test_codex_id_token("legacy-user");
        let auth = codex_managed_oauth_auth_value(
            "legacy-workspace",
            "access",
            Some(&id_token),
            "refresh",
            "2026-01-01T00:00:00Z",
        );
        crate::config::write_json_file(&get_codex_auth_path(), &auth)
            .expect("write legacy live auth");
        crate::config::write_json_file(
            &get_codex_managed_oauth_live_auth_marker_path(),
            &json!({
                "version": 2,
                "account_id": "legacy-workspace"
            }),
        )
        .expect("write legacy marker");

        let other_user = test_codex_id_token("other-user");
        assert!(prepare_codex_live_auth_for_managed_account_removal(
            "legacy-workspace",
            Some(&other_user),
        )
        .is_err());
        assert!(get_codex_auth_path().exists());
        assert!(get_codex_managed_oauth_live_auth_marker_path().exists());

        prepare_codex_live_auth_for_managed_account_removal("legacy-workspace", Some(&id_token))
            .expect("prove and migrate legacy ownership");
        clear_codex_live_auth_for_managed_account("legacy-workspace")
            .expect("remove proven managed live auth");
        assert!(!get_codex_auth_path().exists());
        assert!(!get_codex_managed_oauth_live_auth_marker_path().exists());
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
    fn bearer_token_round_trips_through_inline_provider_tables() {
        // Inline tables (`model_providers = { ... }`) are valid TOML that
        // `as_table` rejects; the token must still land inside the provider
        // table — a top-level fallback is ignored by Codex 0.149 (401 persists).
        let input = r#"model_provider = "aihubmix"
model_providers = { aihubmix = { name = "AiHubMix", base_url = "https://aihubmix.example/v1" } }
"#;

        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-inline"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("aihubmix"))
                .and_then(|v| v.get("experimental_bearer_token"))
                .and_then(|v| v.as_str()),
            Some("sk-inline"),
            "token must land inside the inline provider table; got:\n{output}"
        );
        assert!(
            parsed.get("experimental_bearer_token").is_none(),
            "token must not leak to the top level for a custom provider"
        );

        assert_eq!(
            extract_codex_experimental_bearer_token(&output).as_deref(),
            Some("sk-inline"),
            "extraction must read the token back out of an inline provider table"
        );

        let cleaned = remove_codex_experimental_bearer_token(&output).expect("remove token");
        assert!(
            !cleaned.contains("experimental_bearer_token"),
            "removal must strip the token from an inline provider table; got:\n{cleaned}"
        );
    }

    #[test]
    fn prepare_provider_live_config_skips_tables_with_explicit_auth() {
        // Codex 0.149 rejects `experimental_bearer_token` alongside `auth` /
        // `aws` at deserialization (the whole config fails to parse), and
        // `env_key` outranks the token at runtime, so injection buys nothing.
        // All three shapes must be left untouched.
        for provider_table in [
            "env_key = \"AZURE_OPENAI_API_KEY\"",
            "auth = { command = \"my-auth-helper\" }",
            "aws = { region = \"us-east-1\" }",
            // Header-based auth survives 0.149 only if we leave it alone: the
            // injected bearer would be applied after provider headers and
            // overwrite the explicit Authorization. Header names are
            // case-insensitive.
            "http_headers = { Authorization = \"Bearer header-token\" }",
            "http_headers = { authorization = \"Bearer header-token\" }",
            "env_http_headers = { AUTHORIZATION = \"MY_AUTH_ENV_VAR\" }",
        ] {
            let input = format!(
                r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://example.com/v1"
{provider_table}
"#
            );

            let output =
                prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), &input)
                    .expect("prepare live config");
            assert_eq!(
                output, input,
                "provider table declaring `{provider_table}` must not receive an injected token"
            );
        }

        // `requires_openai_auth = true` must NOT suppress injection: the token
        // outranks it at runtime, which is exactly what keeps a preserved
        // official OAuth login from being sent to a third-party endpoint.
        let bridge_input = r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://example.com/v1"
requires_openai_auth = true
"#;
        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), bridge_input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("custom"))
                .and_then(|v| v.get("experimental_bearer_token"))
                .and_then(|v| v.as_str()),
            Some("sk-test"),
            "requires_openai_auth tables must still receive the token (bridge contract)"
        );

        // requires_openai_auth = true disables the header guard too: without
        // the token, Codex would fall back to the preserved official OAuth
        // (applied after provider headers) and send it to the third-party
        // endpoint. The bridge contract outranks a contradictory header.
        let contradictory_input = r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://example.com/v1"
requires_openai_auth = true
http_headers = { Authorization = "Bearer header-token" }
"#;
        let output = prepare_codex_provider_live_config(
            &json!({"OPENAI_API_KEY": "sk-test"}),
            contradictory_input,
        )
        .expect("prepare live config");
        assert!(
            output.contains("experimental_bearer_token = \"sk-test\""),
            "requires_openai_auth must re-enable injection despite an Authorization header; got:\n{output}"
        );

        // Non-Authorization headers are not credentials — injection proceeds.
        let plain_headers_input = r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://example.com/v1"
http_headers = { x-api-version = "2026-01-01" }
"#;
        let output = prepare_codex_provider_live_config(
            &json!({"OPENAI_API_KEY": "sk-test"}),
            plain_headers_input,
        )
        .expect("prepare live config");
        assert!(
            output.contains("experimental_bearer_token = \"sk-test\""),
            "plain http_headers without Authorization must not suppress injection; got:\n{output}"
        );
    }

    #[test]
    fn third_party_route_without_token_slot_detection() {
        // Dangerous shapes: routing points away from the official provider
        // but the token has no provider table to land in.
        for dangerous in [
            // custom id but its table is missing
            "model_provider = \"aihubmix\"\n",
            // built-in provider rerouted to a third party
            "openai_base_url = \"https://relay.example/v1\"\n",
            "model_provider = \"openai\"\nopenai_base_url = \"https://relay.example/v1\"\n",
        ] {
            assert!(
                codex_config_routes_third_party_without_token_slot(dangerous),
                "shape must be flagged (third-party route, no token slot):\n{dangerous}"
            );
        }

        // Safe shapes: either the token has a landing spot, or nothing
        // reroutes requests away from the official provider (top-level token
        // stays a cc-switch-only record).
        let custom_with_table = r#"model_provider = "aihubmix"

[model_providers.aihubmix]
base_url = "https://aihubmix.example/v1"
"#;
        let custom_inline_table = r#"model_provider = "aihubmix"
model_providers = { aihubmix = { base_url = "https://aihubmix.example/v1" } }
"#;
        for safe in [
            custom_with_table,
            custom_inline_table,
            // no routing directive at all (e.g. an MCP-only config)
            "model = \"gpt-5\"\n",
            "[mcp_servers.echo]\ncommand = \"echo\"\n",
            // explicit built-in provider without a reroute
            "model_provider = \"openai\"\n",
        ] {
            assert!(
                !codex_config_routes_third_party_without_token_slot(safe),
                "shape must not be flagged:\n{safe}"
            );
        }
    }

    #[test]
    fn official_auth_fallback_for_third_party_detection() {
        // Dangerous shapes: with no injectable key, auth resolution falls
        // back to `auth.json` while requests go to a third-party endpoint.
        let header_auth_with_fallback = r#"model_provider = "custom"

[model_providers.custom]
name = "Custom"
base_url = "https://relay.example/v1"
requires_openai_auth = true
http_headers = { Authorization = "Bearer explicit-header-token" }
"#;
        for dangerous in [
            header_auth_with_fallback,
            // bare fallback flag, no credentials anywhere
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nrequires_openai_auth = true\n",
            // built-in openai rerouted to a third party
            "openai_base_url = \"https://relay.example/v1\"\n",
            "model_provider = \"openai\"\nopenai_base_url = \"https://relay.example/v1\"\n",
            // auth/aws are NOT own-credential short-circuits: 0.149 rejects
            // both as mutually exclusive with requires_openai_auth (aws is
            // Bedrock-only on top), so these are dead configs the whole file
            // fails to load with — flag them instead of writing them out
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nrequires_openai_auth = true\nauth = { command = \"my-auth\" }\n",
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nrequires_openai_auth = true\naws = { region = \"us-east-1\" }\n",
        ] {
            assert!(
                codex_config_falls_back_to_official_auth_for_third_party(dangerous),
                "shape must be flagged (auth.json fallback on a third-party route):\n{dangerous}"
            );
        }

        for safe in [
            // no fallback flag: 0.149 resolves this as unauthenticated and
            // the provider's own headers survive (header-auth contract)
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nhttp_headers = { Authorization = \"Bearer k\" }\n",
            // provider-own credentials outrank / replace the fallback
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nrequires_openai_auth = true\nenv_key = \"MY_KEY\"\n",
            // a scoped token is second in the 0.149 short-circuit chain
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nrequires_openai_auth = true\nexperimental_bearer_token = \"tok\"\n",
            // auth/aws without the fallback flag are loadable own-credential
            // shapes (command-backed auth; aws on its Bedrock-only ids never
            // reaches this custom-table arm) — requires_openai_auth unset
            // means no auth.json fallback either way
            "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nauth = { command = \"my-auth\" }\n",
            // no routing directive at all: stays on the official provider
            "model = \"gpt-5\"\n",
            "[mcp_servers.echo]\ncommand = \"echo\"\n",
            "model_provider = \"openai\"\n",
            // custom id with a missing table: Codex refuses to start, no leak
            "model_provider = \"custom\"\n",
            // openai_base_url is inert for non-openai built-ins
            "model_provider = \"ollama\"\nopenai_base_url = \"https://relay.example/v1\"\n",
        ] {
            assert!(
                !codex_config_falls_back_to_official_auth_for_third_party(safe),
                "shape must not be flagged:\n{safe}"
            );
        }
    }

    #[test]
    fn legacy_openai_reroute_is_normalized_into_a_custom_table() {
        let legacy = r#"# keep me
model = "gpt-5.4"
model_provider = "openai"
openai_base_url = "https://relay.example/v1"
"#;
        let normalized = normalize_codex_legacy_openai_reroute(legacy)
            .expect("normalize")
            .expect("legacy shape must be rewritten");

        assert!(
            !normalized.contains("openai_base_url"),
            "the top-level reroute must be removed; got:\n{normalized}"
        );
        assert!(
            normalized.contains("model_provider = \"cc-switch\""),
            "routing must move to the cc-switch table; got:\n{normalized}"
        );
        assert!(
            normalized.contains("[model_providers.cc-switch]"),
            "a custom provider table must be created; got:\n{normalized}"
        );
        assert!(
            normalized.contains("base_url = \"https://relay.example/v1\""),
            "the reroute URL must land in the table; got:\n{normalized}"
        );
        assert!(
            normalized.contains("wire_api = \"responses\""),
            "the built-in openai provider speaks Responses; got:\n{normalized}"
        );
        assert!(
            normalized.contains("# keep me") && normalized.contains("model = \"gpt-5.4\""),
            "unrelated content must survive; got:\n{normalized}"
        );

        // Idempotent: the normalized shape no longer matches.
        assert!(
            normalize_codex_legacy_openai_reroute(&normalized)
                .expect("normalize")
                .is_none(),
            "re-running normalization must be a no-op"
        );

        // The rewritten shape gives the key a provider-scoped slot.
        let injected =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), &normalized)
                .expect("prepare live config");
        assert!(
            injected.contains("experimental_bearer_token = \"sk-test\""),
            "token must land inside the cc-switch table; got:\n{injected}"
        );
        assert_eq!(
            extract_codex_experimental_bearer_token(&injected).as_deref(),
            Some("sk-test"),
        );
    }

    #[test]
    fn legacy_reroute_normalization_covers_exact_built_in_openai_only() {
        // Unset model_provider defaults to the built-in openai provider.
        assert!(
            normalize_codex_legacy_openai_reroute(
                "openai_base_url = \"https://relay.example/v1\"\n"
            )
            .expect("normalize")
            .is_some(),
            "unset model_provider defaults to the built-in openai provider"
        );

        for untouched in [
            // upstream built-in lookup is case-sensitive: `OpenAI` targets a
            // custom table, the reroute knob is inert for it
            "model_provider = \"OpenAI\"\nopenai_base_url = \"https://relay.example/v1\"\n",
            // custom provider: openai_base_url is not what routes it
            "model_provider = \"custom\"\nopenai_base_url = \"https://relay.example/v1\"\n\n[model_providers.custom]\nbase_url = \"https://aihubmix.example/v1\"\n",
            // openai_base_url is inert for non-openai built-ins
            "model_provider = \"ollama\"\nopenai_base_url = \"https://relay.example/v1\"\n",
            // nothing to rewrite
            "model_provider = \"openai\"\n",
            "openai_base_url = \"\"\n",
        ] {
            assert!(
                normalize_codex_legacy_openai_reroute(untouched)
                    .expect("normalize")
                    .is_none(),
                "shape must be left alone:\n{untouched}"
            );
        }
    }

    #[test]
    fn legacy_reroute_normalization_never_overwrites_a_user_cc_switch_table() {
        // A user-authored [model_providers.cc-switch] proves nothing about
        // ownership — overwriting it would drop their headers/query params
        // and backfill the loss into the DB. Migration continues under the
        // first free suffixed id instead: refusing outright would let proxy
        // backup/restore (which call prepare without the safety gates) write
        // an unmigrated reroute with live auth.json credentials.
        let conflicted = r#"model_provider = "openai"
openai_base_url = "https://relay.example/v1"

[model_providers.cc-switch]
name = "Mine"
base_url = "https://mine.example/v1"
http_headers = { x-team = "42" }
"#;
        let normalized = normalize_codex_legacy_openai_reroute(conflicted)
            .expect("normalize")
            .expect("conflicted shape must still migrate");
        assert!(
            normalized.contains("model_provider = \"cc-switch-2\"")
                && normalized.contains("[model_providers.cc-switch-2]"),
            "migration must pick the first free suffixed id; got:\n{normalized}"
        );
        assert!(
            normalized.contains("name = \"Mine\"")
                && normalized.contains("base_url = \"https://mine.example/v1\"")
                && normalized.contains("x-team"),
            "the user's own table must survive untouched; got:\n{normalized}"
        );
        assert!(
            !normalized.contains("openai_base_url"),
            "the reroute must still be rewritten away; got:\n{normalized}"
        );
    }

    #[test]
    fn stale_reserved_tables_are_renamed_with_fallback_aware_routing() {
        // Older cc-switch takeover projections created reserved
        // [model_providers.openai]/[.ollama]/[.lmstudio] tables; Codex 0.148+
        // rejects the whole config at load. Tables are renamed and made
        // loadable; the route follows unless the table would resolve
        // auth.json with no injected token to short-circuit it.
        let stale = r#"model_provider = "openai"
model = "gpt-5.4"

[model_providers.openai]
name = "OpenAI"
base_url = "https://relay.example/v1"
http_headers = { x-team = "42" }
"#;

        // With an injectable key: follow the renamed table and inject into it
        // — snapping back to the built-in provider would silently bill the
        // preserved official account (or 401 with preservation off).
        let prepared =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), stale)
                .expect("prepare live config");
        assert!(
            !prepared.contains("[model_providers.openai]")
                && prepared.contains("[model_providers.cc-switch]")
                && prepared.contains("x-team")
                && prepared.contains("wire_api = \"responses\""),
            "the table must be renamed losslessly (wire_api defaulted); got:\n{prepared}"
        );
        assert!(
            prepared.contains("model_provider = \"cc-switch\""),
            "with a key the route must follow the renamed table; got:\n{prepared}"
        );
        assert_eq!(
            extract_codex_experimental_bearer_token(&prepared).as_deref(),
            Some("sk-test"),
            "the key must land in the followed table"
        );

        // Keyless but the table carries its own credentials (plain
        // Authorization header): follow — 0.149 resolves it unauthenticated
        // and the provider headers survive. The name-less table is also
        // backfilled so the renamed table loads at all.
        let header_auth_stale = r#"model_provider = "openai"

[model_providers.openai]
base_url = "https://relay.example/v1"
http_headers = { Authorization = "Bearer own-key" }
"#;
        let keyless = prepare_codex_provider_live_config(&json!({}), header_auth_stale)
            .expect("prepare live config without token");
        assert!(
            keyless.contains("model_provider = \"cc-switch\"") && keyless.contains("own-key"),
            "self-authenticating tables must keep their route; got:\n{keyless}"
        );
        assert!(
            keyless.contains("name = \"Custom\""),
            "a missing name must be backfilled — 0.149 rejects the whole config otherwise; got:\n{keyless}"
        );

        // Keyless with no credentials at all (requires_openai_auth defaults
        // to false): follow — 0.149 resolves such a table unauthenticated
        // and never reads auth.json, so the local/relay route is kept. A
        // stale `wire_api = "chat"` is normalized: 0.149 removed the chat
        // wire API and rejects the whole config on any non-"responses"
        // value.
        let unauthenticated_stale = r#"model_provider = "openai"

[model_providers.openai]
name = "Local Ollama"
base_url = "http://127.0.0.1:11434/v1"
wire_api = "chat"
"#;
        let local = prepare_codex_provider_live_config(&json!({}), unauthenticated_stale)
            .expect("prepare live config without token");
        assert!(
            local.contains("model_provider = \"cc-switch\"")
                && local.contains("wire_api = \"responses\"")
                && !local.contains("wire_api = \"chat\""),
            "unauthenticated tables keep their route and chat wire_api is normalized; got:\n{local}"
        );

        // Keyless but the table carries its own scoped token: follow —
        // `experimental_bearer_token` is second in the 0.149 short-circuit
        // chain, the table authenticates itself.
        let scoped_token_stale = r#"model_provider = "openai"

[model_providers.openai]
name = "Relay"
base_url = "https://relay.example/v1"
experimental_bearer_token = "own-scoped-token"
"#;
        let scoped = prepare_codex_provider_live_config(&json!({}), scoped_token_stale)
            .expect("prepare live config without token");
        assert!(
            scoped.contains("model_provider = \"cc-switch\"")
                && scoped.contains("own-scoped-token"),
            "tables with a scoped token must keep their route; got:\n{scoped}"
        );

        // Keyless with no usable credentials (requires_openai_auth only):
        // never follow — the route snaps back to the built-in provider so a
        // requires_openai_auth fallback cannot resolve auth.json against a
        // stale third-party address.
        let fallback_stale = r#"model_provider = "openai"

[model_providers.openai]
base_url = "https://relay.example/v1"
requires_openai_auth = true
"#;
        let snapped = prepare_codex_provider_live_config(&json!({}), fallback_stale)
            .expect("prepare live config without token");
        assert!(
            snapped.contains("model_provider = \"openai\"")
                && !snapped.contains("[model_providers.openai]")
                && snapped.contains("[model_providers.cc-switch]"),
            "credential-less tables are renamed but the route snaps back; got:\n{snapped}"
        );

        // Official context never follows, even with credentials in the table.
        let official = migrate_stale_reserved_provider_tables(header_auth_stale, true, true)
            .expect("migrate")
            .expect("stale table must still be renamed");
        assert!(
            official.contains("model_provider = \"openai\"")
                && official.contains("[model_providers.cc-switch]"),
            "official routes never follow a renamed table; got:\n{official}"
        );

        // All three reserved ids are migrated; inactive ones never retarget
        // the route.
        let multi_stale = r#"model_provider = "third"

[model_providers.third]
base_url = "https://third.example/v1"

[model_providers.ollama]
base_url = "http://127.0.0.1:11434/v1"

[model_providers.lmstudio]
base_url = "http://127.0.0.1:1234/v1"
"#;
        let cleaned = migrate_stale_reserved_provider_tables(multi_stale, false, true)
            .expect("migrate")
            .expect("stale tables must be renamed");
        assert!(
            !cleaned.contains("[model_providers.ollama]")
                && !cleaned.contains("[model_providers.lmstudio]")
                && cleaned.contains("[model_providers.cc-switch]")
                && cleaned.contains("[model_providers.cc-switch-2]")
                && cleaned.contains("model_provider = \"third\""),
            "every reserved table is renamed, the active route stays; got:\n{cleaned}"
        );

        // Upstream reserved-id validation is case-sensitive: `OpenAI` is a
        // legitimate custom id and must not be touched.
        let custom_case_variant = r#"model_provider = "OpenAI"

[model_providers.OpenAI]
base_url = "https://mine.example/v1"
"#;
        assert!(
            migrate_stale_reserved_provider_tables(custom_case_variant, false, true)
                .expect("migrate")
                .is_none(),
            "case-variant custom ids are not stale residue"
        );

        // Nothing to migrate -> no rewrite.
        assert!(
            migrate_stale_reserved_provider_tables("model = \"gpt-5\"\n", false, true)
                .expect("migrate")
                .is_none()
        );
    }

    #[test]
    fn case_variant_reserved_ids_are_custom_providers() {
        // Upstream's built-in lookup and reserved-id validation are both
        // case-sensitive, so [model_providers.OpenAI] is a legitimate custom
        // provider — the token must land inside its table, not in a dead
        // top-level field.
        let case_variant = r#"model_provider = "OpenAI"
model = "gpt-5.4"

[model_providers.OpenAI]
name = "Mine"
base_url = "https://mine.example/v1"
wire_api = "responses"
"#;
        assert!(is_custom_codex_model_provider_id("OpenAI"));
        assert!(is_custom_codex_model_provider_id("Ollama"));
        assert!(!is_custom_codex_model_provider_id("openai"));
        // `oss` / `ollama-chat` are NOT reserved on 0.148/0.149 — both load
        // as ordinary custom tables, so the token must reach them too.
        assert!(is_custom_codex_model_provider_id("oss"));
        assert!(is_custom_codex_model_provider_id("ollama-chat"));

        let legacy_alias = r#"model_provider = "oss"

[model_providers.oss]
name = "My OSS Relay"
base_url = "https://oss.example/v1"
"#;
        let alias_prepared =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-oss"}), legacy_alias)
                .expect("prepare live config");
        let alias_parsed: toml::Value = toml::from_str(&alias_prepared).expect("parse output");
        assert!(
            alias_parsed
                .get("model_providers")
                .and_then(|mp| mp.get("oss"))
                .and_then(|t| t.get("experimental_bearer_token"))
                .is_some(),
            "the token must land inside the oss custom table; got:\n{alias_prepared}"
        );
        assert!(
            alias_parsed.get("experimental_bearer_token").is_none(),
            "no dead top-level token for legacy-alias ids; got:\n{alias_prepared}"
        );

        let prepared =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), case_variant)
                .expect("prepare live config");
        assert!(
            prepared.contains("[model_providers.OpenAI]"),
            "the custom table must survive; got:\n{prepared}"
        );
        assert_eq!(
            extract_codex_experimental_bearer_token(&prepared).as_deref(),
            Some("sk-test"),
        );
        let parsed: toml::Value = toml::from_str(&prepared).expect("parse output");
        assert!(
            parsed
                .get("model_providers")
                .and_then(|mp| mp.get("OpenAI"))
                .and_then(|t| t.get("experimental_bearer_token"))
                .is_some(),
            "the token must land inside the case-variant custom table; got:\n{prepared}"
        );
        assert!(
            parsed.get("experimental_bearer_token").is_none(),
            "no dead top-level token; got:\n{prepared}"
        );
    }

    #[test]
    fn legacy_reroute_normalization_handles_inline_model_providers() {
        // Proxy backup/restore call prepare without the safety gates, so an
        // inline `model_providers = { … }` next to a legacy reroute must be
        // migrated too — skipping it would leave the key in a dead top-level
        // field beside live auth.json credentials.
        let inline_shape = r#"model_provider = "openai"
openai_base_url = "https://relay.example/v1"
model_providers = { mine = { name = "Mine", base_url = "https://mine.example/v1" } }
"#;
        let prepared =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), inline_shape)
                .expect("prepare live config");
        assert!(
            !prepared.contains("openai_base_url"),
            "the reroute must be rewritten away; got:\n{prepared}"
        );
        assert!(
            prepared.contains("model_provider = \"cc-switch\"")
                && prepared.contains("cc-switch = {"),
            "migration must add an inline member matching the container style; got:\n{prepared}"
        );
        assert!(
            prepared.contains("mine = {") && prepared.contains("https://mine.example/v1"),
            "the user's inline table must survive untouched; got:\n{prepared}"
        );
        assert_eq!(
            extract_codex_experimental_bearer_token(&prepared).as_deref(),
            Some("sk-test"),
            "the key must resolve for the migrated inline provider"
        );
    }

    #[test]
    fn update_toml_field_refuses_other_reserved_built_in_ids() {
        // ollama/lmstudio have no top-level reroute knob and Codex 0.148+
        // rejects any [model_providers.ollama]/[model_providers.lmstudio]
        // table at load — refusing beats writing a config Codex cannot start
        // with.
        for reserved in ["ollama", "lmstudio"] {
            let input = format!("model_provider = \"{reserved}\"\n");
            let err = update_codex_toml_field(&input, "base_url", "http://127.0.0.1:5000/v1")
                .expect_err("reserved id must be refused");
            assert!(
                err.contains(reserved),
                "the error must name the offending id; got: {err}"
            );
        }

        // Case variants are legitimate custom ids upstream — they keep the
        // normal custom-table path.
        let output = update_codex_toml_field(
            "model_provider = \"Ollama\"\n",
            "base_url",
            "http://127.0.0.1:5000/v1",
        )
        .expect("case-variant custom id must stay writable");
        assert!(
            output.contains("[model_providers.Ollama]"),
            "case variants take the custom table path; got:\n{output}"
        );
    }

    #[test]
    fn update_toml_field_backfills_provider_name() {
        // 0.149 rejects the whole config when any non-bedrock provider table
        // has an empty/missing `name` — and this function historically
        // created exactly such tables. Creating or touching a table must
        // leave it loadable.
        let created = update_codex_toml_field(
            "model_provider = \"myrelay\"\n",
            "base_url",
            "https://relay.example/v1",
        )
        .expect("update");
        assert!(
            created.contains("name = \"myrelay\""),
            "a newly created table must get a non-empty name; got:\n{created}"
        );

        let existing_nameless = r#"model_provider = "myrelay"

[model_providers.myrelay]
base_url = "https://old.example/v1"
"#;
        let touched =
            update_codex_toml_field(existing_nameless, "base_url", "https://new.example/v1")
                .expect("update");
        assert!(
            touched.contains("name = \"myrelay\""),
            "touching a name-less table must backfill the name; got:\n{touched}"
        );

        let existing_named = r#"model_provider = "myrelay"

[model_providers.myrelay]
name = "My Relay"
base_url = "https://old.example/v1"
"#;
        let kept = update_codex_toml_field(existing_named, "base_url", "https://new.example/v1")
            .expect("update");
        assert!(
            kept.contains("name = \"My Relay\"") && !kept.contains("name = \"myrelay\""),
            "an existing name must never be overwritten; got:\n{kept}"
        );
    }

    #[test]
    fn update_toml_field_leaves_bedrock_tables_nameless() {
        // 0.149 lets the Bedrock built-ins override only
        // base_url/auth/http_headers/aws.*; any other non-default field —
        // `name` included — fails the built-in merge for the whole config.
        // The proxy takeover rewrites base_url + wire_api through this
        // function, so the name backfill must skip both reserved ids.
        // (wire_api survives because "responses" is the only value 0.149
        // deserializes, which equals the default.)
        for id in ["amazon-bedrock", "amazon-bedrock-runtime"] {
            let input = format!(
                "model_provider = \"{id}\"\n\n[model_providers.{id}]\nbase_url = \"https://bedrock.example/v1\"\n"
            );
            let rerouted = update_codex_toml_field(&input, "base_url", "http://127.0.0.1:5000/v1")
                .expect("update base_url");
            let rerouted = update_codex_toml_field(&rerouted, "wire_api", "responses")
                .expect("update wire_api");
            assert!(
                rerouted.contains("base_url = \"http://127.0.0.1:5000/v1\"")
                    && rerouted.contains("wire_api = \"responses\""),
                "the takeover overrides must land in the table; got:\n{rerouted}"
            );
            assert!(
                !rerouted.contains("name ="),
                "bedrock tables must never receive a name; got:\n{rerouted}"
            );
        }
    }

    #[test]
    fn prepare_normalizes_legacy_reroute_for_every_caller() {
        // prepare_codex_provider_live_config is the single normalize→inject
        // entry point — takeover backup rebuilds and restore call it directly,
        // so the legacy shape must be migrated here, not only in the switch
        // path's plan.
        let legacy = r#"model_provider = "openai"
model = "gpt-5.4"
openai_base_url = "https://relay.example/v1"
"#;
        let prepared =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), legacy)
                .expect("prepare live config");
        assert!(
            !prepared.contains("openai_base_url")
                && prepared.contains("[model_providers.cc-switch]"),
            "prepare must rewrite the legacy reroute shape; got:\n{prepared}"
        );
        assert_eq!(
            extract_codex_experimental_bearer_token(&prepared).as_deref(),
            Some("sk-test"),
            "the key must land in the rewritten provider table"
        );
        // No token → nothing to protect, the shape passes through untouched.
        let untouched = prepare_codex_provider_live_config(&json!({}), legacy)
            .expect("prepare live config without token");
        assert_eq!(untouched, legacy);
    }

    #[test]
    fn prepare_backfills_names_on_plain_custom_tables() {
        // 0.149 rejects the whole config over any name-less custom table,
        // active or not — plain config-only switches never go through the
        // update path, so prepare itself must normalize. Bedrock tables are
        // the mirror image: adding `name` there fails the built-in merge,
        // so the reserved ids must stay untouched.
        let config = r#"model_provider = "myrelay"

[model_providers.myrelay]
base_url = "https://relay.example/v1"

[model_providers.idle]
base_url = "https://idle.example/v1"

[model_providers.amazon-bedrock]
base_url = "https://bedrock.example/v1"
"#;
        let prepared =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), config)
                .expect("prepare live config");
        assert!(
            prepared.contains("name = \"myrelay\"") && prepared.contains("name = \"idle\""),
            "custom tables (active or not) must get a non-empty name; got:\n{prepared}"
        );
        assert!(
            !prepared.contains("name = \"amazon-bedrock\""),
            "bedrock tables must never receive a name; got:\n{prepared}"
        );

        // The keyless path (official cards, key-less providers) writes the
        // same file and must normalize too.
        let keyless = prepare_codex_provider_live_config(&json!({}), config)
            .expect("prepare live config without token");
        assert!(
            keyless.contains("name = \"myrelay\"") && keyless.contains("name = \"idle\""),
            "the keyless path must backfill names too; got:\n{keyless}"
        );
    }

    #[test]
    fn official_plan_backfills_custom_table_names() {
        // The official branch of plan_codex_live_write never goes through
        // prepare_codex_provider_live_config, so it must normalize name-less
        // custom tables itself — 0.149 validates every table at load, and an
        // official config can carry idle leftovers from older versions.
        let config = r#"model = "gpt-5.4"

[model_providers.idle]
base_url = "https://idle.example/v1"

[model_providers.amazon-bedrock]
base_url = "https://bedrock.example/v1"
"#;
        let plan = plan_codex_live_write(Some("official"), &json!({}), Some(config), false)
            .expect("official plan");
        let written = plan.config_text.expect("official plan carries config");
        assert!(
            written.contains("name = \"idle\""),
            "the official write must backfill idle custom-table names; got:\n{written}"
        );
        assert!(
            !written.contains("name = \"amazon-bedrock\""),
            "bedrock tables must never receive a name; got:\n{written}"
        );
    }

    #[test]
    fn third_party_plan_stamps_requires_openai_auth_to_match_preservation() {
        // Presets and the custom template shipped `requires_openai_auth =
        // true` from the pre-0.149 era (auth.json carried the third-party
        // key back then). On 0.149 the injected bearer decides request auth
        // either way, but the flag drives the login UX: true with auth.json
        // deleted (preservation off) traps the TUI in the login screen,
        // false next to a preserved login hides the official account and
        // lets its tokens go stale. The plan overrides the stored value
        // with the preservation setting.
        let auth = json!({"OPENAI_API_KEY": "sk-test"});
        let stale_true = "model_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n";

        let off = plan_codex_live_write(None, &auth, Some(stale_true), false)
            .expect("third-party plan with preservation off");
        let off_text = off.config_text.expect("plan carries config");
        assert!(
            off_text.contains("requires_openai_auth = false")
                && !off_text.contains("requires_openai_auth = true"),
            "preservation off must stamp the stale flag to false; got:\n{off_text}"
        );
        assert!(
            off_text.contains("experimental_bearer_token = \"sk-test\""),
            "the bearer injection must be unaffected; got:\n{off_text}"
        );
        assert!(off.remove_auth_file, "preservation off deletes auth.json");

        let on = plan_codex_live_write(None, &auth, Some(stale_true), true)
            .expect("third-party plan with preservation on");
        let on_text = on.config_text.expect("plan carries config");
        assert!(
            on_text.contains("requires_openai_auth = true"),
            "preservation on must keep/stamp the flag true; got:\n{on_text}"
        );
        assert!(!on.remove_auth_file, "preservation on keeps auth.json");

        // A card that never carried the flag gets it stamped too — the
        // preserved login stays visible to Codex (account state + token
        // refresh) only through requires_openai_auth = true.
        let flagless = "model_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n";
        let on_flagless = plan_codex_live_write(None, &auth, Some(flagless), true)
            .expect("third-party plan for a flagless card");
        let on_flagless_text = on_flagless.config_text.expect("plan carries config");
        assert!(
            on_flagless_text.contains("requires_openai_auth = true"),
            "preservation on must stamp flagless cards; got:\n{on_flagless_text}"
        );
    }

    #[test]
    fn requires_openai_auth_stamp_only_touches_tables_with_a_request_auth_short_circuit() {
        // Keyless header-auth card: no env_key / bearer short-circuit, so
        // stamping true would route request auth to the preserved OAuth
        // login (applied after provider headers — the leak the gates
        // refuse). It must keep its user-authored shape under both
        // settings; 0.149 resolves it as unauthenticated and the static
        // header survives.
        let header_auth = "model_provider = \"hdr\"\n\n[model_providers.hdr]\nname = \"Header\"\nbase_url = \"https://hdr.example/v1\"\nwire_api = \"responses\"\nhttp_headers = { Authorization = \"Bearer sk-static\" }\n";
        for preserve in [false, true] {
            let plan = plan_codex_live_write(None, &json!({}), Some(header_auth), preserve)
                .expect("keyless header-auth plan");
            let text = plan.config_text.expect("plan carries config");
            assert!(
                !text.contains("requires_openai_auth"),
                "header-auth cards must not be stamped (preserve={preserve}); got:\n{text}"
            );
        }

        // env_key short-circuits request auth on 0.149 just like the
        // bearer, so the stamp applies: a stale true would otherwise trap
        // the TUI in the login screen once auth.json is deleted.
        let env_key = "model_provider = \"envd\"\n\n[model_providers.envd]\nname = \"EnvKey\"\nbase_url = \"https://envd.example/v1\"\nwire_api = \"responses\"\nenv_key = \"MY_KEY\"\nrequires_openai_auth = true\n";
        let plan = plan_codex_live_write(None, &json!({}), Some(env_key), false)
            .expect("env_key plan with preservation off");
        let text = plan.config_text.expect("plan carries config");
        assert!(
            text.contains("requires_openai_auth = false"),
            "env_key cards must be stamped like bearer cards; got:\n{text}"
        );
    }

    #[test]
    fn requires_openai_auth_stamp_is_a_noop_when_already_aligned() {
        let aligned = "model_provider = \"relay\"\n\n[model_providers.relay]\nname = \"Relay\"\nbase_url = \"https://relay.example/v1\"\nexperimental_bearer_token = \"sk-test\"\nrequires_openai_auth = false\n";
        let output = align_codex_requires_openai_auth_with_login_preservation(aligned, false)
            .expect("align");
        assert_eq!(
            output, aligned,
            "an aligned config must pass through untouched"
        );

        // No custom-table route → nothing to stamp.
        let no_route = "model = \"gpt-5.6\"\n";
        let output = align_codex_requires_openai_auth_with_login_preservation(no_route, true)
            .expect("align");
        assert_eq!(output, no_route);
    }

    #[test]
    fn preflight_rejects_provider_table_conflicts_codex_refuses_to_load() {
        // 0.149 validates EVERY provider table (idle ones included) and
        // rejects: aws outside the Bedrock built-ins, and auth combined with
        // requires_openai_auth / env_key / experimental_bearer_token. These
        // can't be normalized away, so the switch must refuse up front —
        // with or without a carried key, official or third-party.
        let with_key = json!({"OPENAI_API_KEY": "sk-test"});
        let rejected = [
            // bare aws on a custom table, no requires_openai_auth anywhere
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"https://relay.example/v1\"\naws = { region = \"us-east-1\" }\n",
            // auth × requires_openai_auth — carried key skips the fallback
            // gate, so the preflight must catch it independently
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"https://relay.example/v1\"\nrequires_openai_auth = true\nauth = { command = \"my-auth\" }\n",
            // auth × env_key / experimental_bearer_token
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"https://relay.example/v1\"\nenv_key = \"MY_KEY\"\nauth = { command = \"my-auth\" }\n",
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"https://relay.example/v1\"\nexperimental_bearer_token = \"tok\"\nauth = { command = \"my-auth\" }\n",
            // an IDLE conflicting table poisons the whole config too
            "model_provider = \"active\"\n\n[model_providers.active]\nname = \"Active\"\nbase_url = \"https://relay.example/v1\"\n\n[model_providers.idle]\nname = \"Idle\"\nbase_url = \"https://idle.example/v1\"\naws = { region = \"us-east-1\" }\n",
        ] ;
        for config in rejected {
            assert!(
                preflight_codex_live_write(None, &with_key, Some(config)).is_err(),
                "third-party preflight must refuse:\n{config}"
            );
            assert!(
                preflight_codex_live_write(Some("official"), &json!({}), Some(config)).is_err(),
                "official preflight must refuse the same shapes:\n{config}"
            );
        }

        // Loadable shapes stay accepted: command-backed auth alone, and aws
        // on the Bedrock built-ins.
        let accepted = [
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"https://relay.example/v1\"\nauth = { command = \"my-auth\" }\n",
            "model_provider = \"amazon-bedrock\"\n\n[model_providers.amazon-bedrock]\nbase_url = \"https://bedrock.example/v1\"\naws = { region = \"us-east-1\" }\n",
        ];
        for config in accepted {
            assert!(
                preflight_codex_live_write(None, &with_key, Some(config)).is_ok(),
                "loadable shape must pass the preflight:\n{config}"
            );
        }
    }

    #[test]
    fn update_toml_field_reroutes_built_in_openai_via_top_level_knob() {
        // Codex 0.149 refuses any [model_providers.openai] table outright
        // (validate_reserved_model_provider_ids), so rewriting base_url for
        // the built-in provider must use the top-level openai_base_url knob.
        let input = "model_provider = \"openai\"\nmodel = \"gpt-5.4\"\n";
        let output = update_codex_toml_field(input, "base_url", "http://127.0.0.1:5000/v1")
            .expect("update base_url");
        assert!(
            !output.contains("[model_providers.openai]") && !output.contains("model_providers"),
            "no reserved provider table may be created; got:\n{output}"
        );
        assert!(
            output.contains("openai_base_url = \"http://127.0.0.1:5000/v1\""),
            "the reroute must use the top-level knob; got:\n{output}"
        );

        // Clearing the value removes the knob again.
        let cleared = update_codex_toml_field(&output, "base_url", "").expect("clear base_url");
        assert!(!cleared.contains("openai_base_url"));

        // wire_api is fixed by the CLI for built-ins — a no-op, not a table.
        let wire = update_codex_toml_field(input, "wire_api", "responses").expect("set wire_api");
        assert!(!wire.contains("model_providers"));
    }

    #[test]
    fn bedrock_runtime_is_a_reserved_provider_id() {
        // `amazon-bedrock-runtime` is reserved by Codex 0.149; treating it as
        // custom would inject a token into a table whose `aws` config
        // hard-conflicts with it. Reserved IDs keep the top-level fallback.
        assert!(!is_custom_codex_model_provider_id("amazon-bedrock-runtime"));

        let input = r#"model_provider = "amazon-bedrock-runtime"
"#;
        let output =
            prepare_codex_provider_live_config(&json!({"OPENAI_API_KEY": "sk-test"}), input)
                .expect("prepare live config");
        let parsed: toml::Value = toml::from_str(&output).expect("parse output");
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
    fn credential_login_material_only_counts_real_credentials() {
        assert!(codex_auth_has_credential_login_material(&json!({
            "tokens": { "access_token": "t" }
        })));
        assert!(codex_auth_has_credential_login_material(&json!({
            "tokens": { "refresh_token": "r" }
        })));
        assert!(codex_auth_has_credential_login_material(&json!({
            "personal_access_token": "pat"
        })));

        // API key and pure metadata are not credentials in this predicate's
        // sense — they must not shield a stale key from cleanup.
        assert!(!codex_auth_has_credential_login_material(&json!({
            "OPENAI_API_KEY": "sk-x"
        })));
        assert!(!codex_auth_has_credential_login_material(&json!({
            "OPENAI_API_KEY": "sk-x",
            "last_refresh": "2026-01-01T00:00:00Z",
            "tokens": { "account_id": "acct-meta-only" }
        })));
        assert!(!codex_auth_has_credential_login_material(&json!({})));
    }

    #[test]
    fn stale_third_party_residue_detection() {
        // Shapes a preserve-off third-party switch leaves behind: cleared.
        assert!(codex_live_auth_is_stale_third_party_residue(&json!({
            "OPENAI_API_KEY": "sk-third-party"
        })));
        assert!(codex_live_auth_is_stale_third_party_residue(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "sk-third-party"
        })));
        assert!(codex_live_auth_is_stale_third_party_residue(&json!({
            "OPENAI_API_KEY": "sk-third-party",
            "last_refresh": "2026-01-01T00:00:00Z",
            "tokens": { "account_id": "acct-meta-only" }
        })));

        // Anything carrying a real credential must survive untouched.
        assert!(!codex_live_auth_is_stale_third_party_residue(&json!({
            "OPENAI_API_KEY": "sk-x",
            "tokens": { "access_token": "t" }
        })));
        assert!(!codex_live_auth_is_stale_third_party_residue(&json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": { "access_token": "official-oauth-token" }
        })));

        // Nothing to clear.
        assert!(!codex_live_auth_is_stale_third_party_residue(&json!({})));
        assert!(!codex_live_auth_is_stale_third_party_residue(&json!({
            "OPENAI_API_KEY": ""
        })));
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
    fn base_url_writes_into_inline_table_provider_section() {
        // inline table 是合法 TOML，但 as_table_mut() 对它返回 None。旧代码会因此
        // 掉进「写顶层字段」的 fallback：用户改的 base_url 落在错误层级，
        // Codex 读不到，且界面毫无提示。
        let input = r#"model_provider = "any"
model_providers = { any = { name = "any", base_url = "https://old.api/v1", wire_api = "responses" } }
"#;

        let result = update_codex_toml_field(input, "base_url", "https://new.api/v1").unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();

        assert_eq!(
            parsed["model_providers"]["any"]["base_url"].as_str(),
            Some("https://new.api/v1"),
            "must update the provider section, not a top-level field"
        );
        assert!(
            parsed.get("base_url").is_none(),
            "must not leak a top-level base_url fallback"
        );
        assert_eq!(
            parsed["model_providers"]["any"]["wire_api"].as_str(),
            Some("responses"),
            "sibling fields must survive"
        );
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
    fn dynamic_template_backfills_parser_required_fields_from_static() {
        // Simulate a template cloned from a models_cache.json written by a
        // Codex build whose ModelInfo lacks parser-side required fields such
        // as `supports_reasoning_summaries` (codex >= 0.144.5 rejects the
        // whole catalog file without it).
        let mut template = json!({
            "slug": "gpt-5.5",
            "context_window": 272_000,
            "supports_parallel_tool_calls": false
        });
        fill_template_fields_from_static(&mut template);

        assert_eq!(
            template
                .get("supports_reasoning_summaries")
                .and_then(Value::as_bool),
            Some(true)
        );
        // Keys already present in the dynamic template are never overwritten.
        assert_eq!(
            template
                .get("supports_parallel_tool_calls")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            template.get("context_window").and_then(Value::as_u64),
            Some(272_000)
        );
        // Optional capability fields must NOT be backfilled: for the catalog
        // parser "missing" means the parser default, not the static
        // template's value.
        assert!(template.get("supports_search_tool").is_none());
        assert!(template.get("supports_image_detail_original").is_none());
        assert!(template.get("web_search_tool_type").is_none());
    }

    #[test]
    fn proxy_chat_catalog_entries_carry_reasoning_summaries_flag() {
        // End to end: a stale dynamic template, once backfilled, must yield
        // catalog entries codex 0.144.5+ can parse.
        let mut template = json!({ "slug": "gpt-5.5" });
        fill_template_fields_from_static(&mut template);
        let specs = vec![CodexCatalogModelSpec {
            model: "k3".to_string(),
            display_name: Some("Kimi K3".to_string()),
            context_window: Some(262_144),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        }];
        let catalog = codex_model_catalog_from_specs(
            &specs,
            &template,
            CodexCatalogToolProfile::ProxyChat,
            128_000,
        );
        assert_eq!(
            catalog["models"][0]
                .get("supports_reasoning_summaries")
                .and_then(Value::as_bool),
            Some(true)
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
            "max_context_window": 272000
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
        let specs = codex_catalog_model_specs(&settings);
        let catalog = codex_model_catalog_from_specs(
            &specs,
            &template,
            CodexCatalogToolProfile::ProxyChat,
            128_000,
        );
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
            models[1]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(128_000)
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
    fn native_responses_catalog_honors_per_model_reasoning_levels() {
        // The native template only declares none/high. A per-model
        // reasoningLevels override must replace supported_reasoning_levels and
        // pick a sensible default_reasoning_level.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "reasoningLevels": ["none", "low", "medium", "high", "xhigh", "max"],
                        "defaultReasoningLevel": "xhigh"
                    },
                    {
                        "model": "no-default-model",
                        "reasoningLevels": ["low", "medium", "high"]
                    },
                    {
                        "model": "template-default-model",
                        "reasoningLevels": ["none", "high", "xhigh"]
                    },
                    {
                        "model": "dirty-levels",
                        "reasoningLevels": ["none", "bogus", "high", ""]
                    },
                    {
                        "model": "unordered-model",
                        "reasoningLevels": ["xhigh", "low", "bogus", "low"],
                        "defaultReasoningLevel": "bogus"
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let models = catalog["models"].as_array().expect("models array");
        let efforts = |index: usize| -> Vec<String> {
            models[index]["supported_reasoning_levels"]
                .as_array()
                .expect("supported_reasoning_levels array")
                .iter()
                .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
                .map(str::to_string)
                .collect()
        };

        // Explicit default wins.
        assert_eq!(
            efforts(0),
            vec!["none", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            models[0]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );

        // No explicit default: falls back to the last (highest) declared level.
        assert_eq!(efforts(1), vec!["low", "medium", "high"]);
        assert_eq!(
            models[1]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("high")
        );

        // Template default ("high") is kept when it is still in the list.
        assert_eq!(efforts(2), vec!["none", "high", "xhigh"]);
        assert_eq!(
            models[2]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("high")
        );

        // Unknown / empty efforts are dropped; the default still resolves to
        // a supported level (the template default, "high").
        assert_eq!(efforts(3), vec!["none", "high"]);
        assert_eq!(
            models[3]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("high")
        );

        // Declaration order is normalized to canonical order, duplicates and
        // an unknown explicit default are dropped, and the fallback picks the
        // highest supported level in canonical order (not the last declared
        // one, and never an unknown effort).
        assert_eq!(efforts(4), vec!["low", "xhigh"]);
        assert_eq!(
            models[4]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );
    }

    #[test]
    fn vendor_catalog_honors_per_model_reasoning_levels() {
        // The DeepSeek official catalog declares low/high/max; a per-model
        // override must win over the official entry.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "reasoningLevels": ["none", "low", "medium", "high", "xhigh", "max"],
                        "defaultReasoningLevel": "xhigh"
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        let efforts: Vec<&str> = entry["supported_reasoning_levels"]
            .as_array()
            .expect("supported_reasoning_levels array")
            .iter()
            .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(
            efforts,
            vec!["none", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            entry
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );
    }

    #[test]
    fn native_responses_profile_suppresses_apply_patch_and_keeps_shell() {
        // Native (direct) /responses providers must NOT emit a freeform
        // apply_patch (type=="custom") tool — gateways like MiMo reject it.
        // The native profile uses the bundled clean template and relies on
        // shell_type="shell_command" for edits, plus per-row overrides.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "MiniMax-M3",
                        "displayName": "MiniMax-M3",
                        "contextWindow": 1_000_000,
                        "supportsParallelToolCalls": true,
                        "inputModalities": ["text", "image"],
                        "baseInstructions": "You are Codex, a coding agent based on MiniMax-M3."
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("native catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        assert_eq!(
            entry.get("slug").and_then(|v| v.as_str()),
            Some("MiniMax-M3")
        );
        assert_eq!(
            entry.get("shell_type").and_then(|v| v.as_str()),
            Some("shell_command"),
            "native entries edit via shell, not the custom apply_patch tool"
        );
        assert!(
            entry.get("apply_patch_tool_type").is_none(),
            "native entries must NOT declare a freeform apply_patch tool"
        );
        // `base_instructions` is REQUIRED by Codex's catalog parser, so it must
        // be present — and the per-row official override must win over the
        // template default.
        assert_eq!(
            entry.get("base_instructions").and_then(|v| v.as_str()),
            Some("You are Codex, a coding agent based on MiniMax-M3."),
            "per-row baseInstructions override must apply (and field must exist)"
        );
        assert!(
            entry.get("model_messages").is_none(),
            "native entries must not carry the gpt-5.5 model_messages persona text"
        );
        assert_eq!(
            entry.get("supports_parallel_tool_calls"),
            Some(&json!(true)),
            "per-row supportsParallelToolCalls override must apply"
        );
        assert_eq!(
            entry.get("input_modalities"),
            Some(&json!(["text", "image"])),
            "per-row inputModalities override must apply"
        );
        assert_eq!(
            entry.get("context_window").and_then(|v| v.as_u64()),
            Some(1_000_000)
        );
    }

    #[test]
    fn catalog_infers_image_input_independently_of_tool_profile() {
        // Start from a deliberately text-only template to prove that every
        // profile overwrites template defaults with shared capability logic.
        let template = json!({
            "input_modalities": ["text"],
            "apply_patch_tool_type": "freeform"
        });
        let specs = vec![
            CodexCatalogModelSpec {
                model: "gpt-5.4".to_string(),
                display_name: Some("GPT 5.4".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: None,
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "deepseek/deepseek-v4-pro".to_string(),
                display_name: Some("DeepSeek V4 Pro".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: None,
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "glm-5.2v".to_string(),
                display_name: Some("GLM 5.2V".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: None,
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "deepseek-v4-flash".to_string(),
                display_name: Some("Explicit Visual Override".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: Some(vec!["text".to_string(), "image".to_string()]),
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "custom-text-alias".to_string(),
                display_name: Some("Explicit Text Override".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: Some(vec!["text".to_string()]),
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
        ];

        for profile in [
            CodexCatalogToolProfile::ProxyChat,
            CodexCatalogToolProfile::NativeResponses,
            CodexCatalogToolProfile::Anthropic,
        ] {
            let catalog = codex_model_catalog_from_specs(&specs, &template, profile, 128_000);
            let models = catalog["models"].as_array().expect("models array");
            let modalities = |slug: &str| {
                models
                    .iter()
                    .find(|entry| entry["slug"] == slug)
                    .and_then(|entry| entry.get("input_modalities"))
                    .cloned()
                    .unwrap_or(Value::Null)
            };

            assert_eq!(modalities("gpt-5.4"), json!(["text", "image"]));
            assert_eq!(modalities("deepseek/deepseek-v4-pro"), json!(["text"]));
            assert_eq!(modalities("glm-5.2v"), json!(["text", "image"]));
            assert_eq!(
                modalities("deepseek-v4-flash"),
                json!(["text", "image"]),
                "explicit provider metadata must override the text-only registry"
            );
            assert_eq!(modalities("custom-text-alias"), json!(["text"]));
        }
    }

    #[test]
    fn native_responses_catalog_always_carries_base_instructions() {
        // Regression guard for the "missing field `base_instructions`" parse
        // error: Codex refuses to load a model catalog whose entries lack
        // base_instructions. Synthesized presets carry no per-row override, so
        // the entry MUST inherit the template's neutral default rather than
        // dropping the field entirely.
        let settings = json!({
            "modelCatalog": { "models": [{ "model": "qwen3-coder-plus" }] }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("native catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let base = catalog["models"][0]
            .get("base_instructions")
            .and_then(|v| v.as_str());
        assert!(
            base.is_some_and(|s| !s.trim().is_empty()),
            "every native entry must carry a non-empty base_instructions (Codex requires it)"
        );
    }

    const DEEPSEEK_NATIVE_CONFIG: &str = r#"model = "deepseek-v4-flash"
model_provider = "custom"

[model_providers.custom]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#;

    #[test]
    fn deepseek_host_native_catalog_mirrors_official_entries() {
        // DeepSeek publishes an official Codex models.json (freeform
        // apply_patch + GPT-5 harness + low/high/max reasoning levels). For a
        // deepseek.com native provider the generated catalog must mirror it
        // verbatim instead of the stripped neutral template — the harness
        // tells the model to use apply_patch, so stripping the tool while
        // keeping the harness would be self-inconsistent.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash" },
                    { "model": "deepseek-v4-pro", "contextWindow": 500_000 }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let flash = &catalog["models"][0];
        assert_eq!(
            flash.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            flash.get("apply_patch_tool_type").and_then(|v| v.as_str()),
            Some("freeform"),
            "official DeepSeek entries keep the freeform apply_patch grant"
        );
        assert!(
            flash
                .get("base_instructions")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.starts_with("You are Codex, an agent based on GPT-5")),
            "official GPT-5 harness must survive verbatim"
        );
        let efforts: Vec<&str> = flash["supported_reasoning_levels"]
            .as_array()
            .expect("official reasoning levels array")
            .iter()
            .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(efforts, vec!["low", "high", "max"]);
        assert_eq!(flash.get("supports_search_tool"), Some(&json!(true)));
        assert_eq!(
            flash.get("web_search_tool_type").and_then(|v| v.as_str()),
            Some("text")
        );
        assert_eq!(
            flash.get("supports_reasoning_summaries"),
            Some(&json!(true))
        );
        assert_eq!(flash.get("input_modalities"), Some(&json!(["text"])));
        assert!(
            flash.get("model_messages").is_some(),
            "official entries are mirrored verbatim, incl. model_messages"
        );
        // No explicit contextWindow on the row: the official 1m window must
        // survive instead of being clobbered by the 128k default.
        assert_eq!(
            flash.get("context_window").and_then(|v| v.as_u64()),
            Some(1_048_576)
        );
        // Explicit user display name still wins over the official one.
        assert_eq!(
            flash.get("display_name").and_then(|v| v.as_str()),
            Some("DeepSeek V4 Flash")
        );

        let pro = &catalog["models"][1];
        assert_eq!(
            pro.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-v4-pro")
        );
        // Explicit user context window override wins…
        assert_eq!(
            pro.get("context_window").and_then(|v| v.as_u64()),
            Some(500_000)
        );
        assert_eq!(
            pro.get("max_context_window").and_then(|v| v.as_u64()),
            Some(500_000)
        );
        // …while the untouched official display name is kept.
        assert_eq!(
            pro.get("display_name").and_then(|v| v.as_str()),
            Some("DeepSeek-V4-Pro")
        );
    }

    #[test]
    fn deepseek_official_catalog_unknown_model_clones_flagship() {
        // A user-added model id the official file doesn't know keeps the
        // gateway's capability profile (clone of the flagship entry) without
        // impersonating it: own slug/name, demoted priority, and the official
        // context window rather than the 128k synthetic default.
        let settings = json!({
            "modelCatalog": { "models": [{ "model": "deepseek-v4-lite" }] }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        assert_eq!(
            entry.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-v4-lite")
        );
        assert_eq!(
            entry.get("display_name").and_then(|v| v.as_str()),
            Some("deepseek-v4-lite")
        );
        assert!(
            entry
                .get("priority")
                .and_then(|v| v.as_u64())
                .is_some_and(|p| p >= 1000),
            "clones must sort after official entries"
        );
        assert_eq!(
            entry.get("apply_patch_tool_type").and_then(|v| v.as_str()),
            Some("freeform")
        );
        assert_eq!(
            entry.get("context_window").and_then(|v| v.as_u64()),
            Some(1_048_576),
            "absent contextWindow keeps the flagship's official window"
        );
        assert!(entry
            .get("base_instructions")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty()));
    }

    #[test]
    fn official_vendor_catalog_gated_by_native_profile_and_host() {
        // The official mirror is a capability GRANT, so the gate must be
        // narrow: native `/responses` profile AND the vendor's own host. Chat
        // runs through the proxy converter (gpt-5.5 contract), the Anthropic
        // transform drops custom tools, and aggregators hosting the same
        // model may reject freeform tools — all of them keep their templates.
        assert!(codex_official_vendor_catalog_models(
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses
        )
        .is_some_and(|models| !models.is_empty()));

        for profile in [
            CodexCatalogToolProfile::ProxyChat,
            CodexCatalogToolProfile::Anthropic,
        ] {
            assert!(
                codex_official_vendor_catalog_models(DEEPSEEK_NATIVE_CONFIG, profile).is_none(),
                "only the NativeResponses profile may mirror the official catalog"
            );
        }

        let minimax_config = r#"model = "MiniMax-M3"
model_provider = "custom"

[model_providers.custom]
name = "minimax"
base_url = "https://api.minimaxi.com/v1"
wire_api = "responses"
"#;
        assert!(
            codex_official_vendor_catalog_models(
                minimax_config,
                CodexCatalogToolProfile::NativeResponses
            )
            .is_none(),
            "non-DeepSeek native hosts keep the neutral template"
        );
        assert!(
            codex_official_vendor_catalog_models("", CodexCatalogToolProfile::NativeResponses)
                .is_none()
        );
    }

    #[test]
    fn proxy_chat_profile_still_keeps_apply_patch() {
        // Regression guard for Mode A: the proxy-chat profile must keep the
        // freeform apply_patch tool (the proxy rewrites custom<->function).
        let template = load_codex_native_responses_template();
        let specs = vec![CodexCatalogModelSpec {
            model: "x".to_string(),
            display_name: Some("x".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        }];
        // Using a gpt-5.5-shaped template under ProxyChat must NOT strip
        // apply_patch_tool_type. (The native template lacks it, so synthesize
        // one with the field present to prove ProxyChat leaves it intact.)
        let mut proxy_template = template.clone();
        proxy_template["apply_patch_tool_type"] = json!("freeform");
        let catalog = codex_model_catalog_from_specs(
            &specs,
            &proxy_template,
            CodexCatalogToolProfile::ProxyChat,
            128_000,
        );
        assert_eq!(
            catalog["models"][0]
                .get("apply_patch_tool_type")
                .and_then(|v| v.as_str()),
            Some("freeform"),
            "ProxyChat must preserve apply_patch_tool_type (no native stripping)"
        );
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
    fn native_web_search_field_disables_at_top_level() {
        // Native `/responses` gateways reject the web_search tool, so the
        // NativeResponses profile must write the top-level disable line even
        // when sections are present (it must NOT land inside a section).
        let input = r#"model_provider = "custom"

[model_providers.custom]
name = "xiaomi_mimo"
"#;
        let result = set_codex_native_web_search_field(input, true).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed.get("web_search").and_then(|value| value.as_str()),
            Some("disabled")
        );
        assert!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("web_search"))
                .is_none(),
            "web_search should stay top-level"
        );
    }

    #[test]
    fn native_web_search_field_removes_own_sentinel_when_not_disabled() {
        // Switching away from a native provider must re-enable web search by
        // removing cc-switch's own "disabled" sentinel.
        let input = r#"model = "gpt-5.5"
web_search = "disabled"
"#;
        let result = set_codex_native_web_search_field(input, false).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert!(
            parsed.get("web_search").is_none(),
            "cc-switch's disabled sentinel should be removed when not native"
        );
    }

    #[test]
    fn native_web_search_field_preserves_user_value() {
        // A user's own web_search value must never be clobbered by cleanup,
        // only cc-switch's "disabled" sentinel is owned/removable.
        let input = r#"web_search = "enabled"
"#;
        let result = set_codex_native_web_search_field(input, false).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed.get("web_search").and_then(|value| value.as_str()),
            Some("enabled"),
            "a user-set web_search value must be preserved"
        );
    }

    #[test]
    fn anthropic_profile_disables_web_search_without_catalog() {
        // Regression: even when no model catalog is generated (empty/absent
        // modelCatalog), an Anthropic provider must still disable web_search — the
        // Responses→Anthropic transform drops the hosted tool, so leaving it on
        // exposes a dead tool. The None-catalog branch previously always left it on.
        let config = "model = \"claude-sonnet-4-6\"\n";
        let settings = serde_json::json!({});

        let anthropic = prepare_codex_config_text_with_model_catalog(
            &settings,
            config,
            CodexCatalogToolProfile::Anthropic,
        )
        .unwrap();
        let parsed: toml::Value = toml::from_str(&anthropic).unwrap();
        assert_eq!(
            parsed.get("web_search").and_then(|v| v.as_str()),
            Some("disabled"),
            "Anthropic profile must disable web_search even with no catalog"
        );

        // ProxyChat on the same no-catalog path must NOT add a disable line.
        let proxy = prepare_codex_config_text_with_model_catalog(
            &settings,
            config,
            CodexCatalogToolProfile::ProxyChat,
        )
        .unwrap();
        let parsed: toml::Value = toml::from_str(&proxy).unwrap();
        assert!(
            parsed.get("web_search").is_none(),
            "ProxyChat profile must not disable web_search on the no-catalog path"
        );
    }

    #[test]
    fn web_search_blacklist_disables_only_known_reject_gateways() {
        let cfg = |model: &str, base_url: &str| {
            format!(
                "model_provider = \"custom\"\nmodel = \"{model}\"\n\n[model_providers.custom]\nname = \"x\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
            )
        };

        // Blacklisted by host (first-party reject gateways) → disable.
        for (model, host) in [
            ("mimo-v2.5-pro", "https://api.xiaomimimo.com/v1"),
            ("mimo-v2.5", "https://token-plan-cn.xiaomimimo.com/v1"),
            ("LongCat-2.0", "https://api.longcat.chat/openai/v1"),
            ("MiniMax-M3", "https://api.minimax.io/v1"),
            ("MiniMax-M3", "https://api.minimaxi.com/v1"),
        ] {
            assert!(
                codex_native_gateway_rejects_web_search(&cfg(model, host)),
                "{host} should be blacklisted"
            );
        }

        // Blacklisted by MODEL brand even on an aggregator host (SiliconFlow
        // fronting a reject vendor's model) → disable.
        for (model, host) in [
            ("MiniMax-M3", "https://api.siliconflow.cn/v1"),
            ("MiniMaxAI/MiniMax-M3", "https://api.siliconflow.cn/v1"),
            ("mimo-v2.5-pro", "https://some-aggregator.example/v1"),
            (
                "qwen/qwen3-coder-plus",
                "https://some-aggregator.example/v1",
            ),
        ] {
            assert!(
                codex_native_gateway_rejects_web_search(&cfg(model, host)),
                "{model} @ {host} should be blacklisted by model brand"
            );
        }

        // Qwen3-Coder is blacklisted by model, not by DashScope host. This keeps
        // general Qwen models that support built-in web_search on the same host
        // enabled while protecting the native qwen3-coder-plus preset.
        assert!(codex_native_gateway_rejects_web_search(&cfg(
            "qwen3-coder-plus",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        )));
        assert!(!codex_native_gateway_rejects_web_search(&cfg(
            "qwen3.7-plus",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        )));

        // NOT blacklisted → keep Codex default (relays/GPT, DouBao, general Qwen,
        // and any unknown provider incl. an aggregator serving a non-reject model).
        for (model, host) in [
            ("gpt-5.5", "https://www.packyapi.com/v1"),
            ("gpt-5-codex", "https://aihubmix.com/v1"),
            (
                "doubao-seed-2-1-pro-260628",
                "https://ark.cn-beijing.volces.com/api/v3",
            ),
            ("Pro/moonshotai/Kimi-K2.6", "https://api.siliconflow.cn/v1"),
        ] {
            assert!(
                !codex_native_gateway_rejects_web_search(&cfg(model, host)),
                "{model} @ {host} should NOT be blacklisted"
            );
        }
    }

    #[test]
    fn resolve_catalog_path_returns_none_when_config_missing_field() {
        let base = PathBuf::from("/tmp/.codex");
        assert!(resolve_cc_switch_catalog_path("", &base).is_none());
        assert!(
            resolve_cc_switch_catalog_path("model = \"gpt-5\"", &base).is_none(),
            "no model_catalog_json field should yield None"
        );
    }

    #[test]
    fn resolve_catalog_path_accepts_cc_switch_owned_file() {
        let base = PathBuf::from("/tmp/.codex");
        let config = r#"model_catalog_json = "/tmp/.codex/cc-switch-model-catalog.json"
"#;
        let resolved = resolve_cc_switch_catalog_path(config, &base).expect("path resolves");
        assert_eq!(resolved, base.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME));
    }

    #[test]
    fn resolve_catalog_path_rejects_user_owned_external_file() {
        let base = PathBuf::from("/tmp/.codex");
        let config = r#"model_catalog_json = "/Users/me/.codex/my-handwritten-catalog.json"
"#;
        assert!(
            resolve_cc_switch_catalog_path(config, &base).is_none(),
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
    fn build_simplified_catalog_squashes_inferred_modalities_and_keeps_overrides() {
        let catalog = r#"{
            "models": [
                { "slug": "gpt-5.4", "input_modalities": ["text", "image"] },
                { "slug": "deepseek-v4-pro", "input_modalities": ["text"] },
                { "slug": "gpt-text-override", "input_modalities": ["text"] },
                { "slug": "deepseek-v4-flash", "input_modalities": ["text", "image"] }
            ]
        }"#;

        let result = build_simplified_catalog_from_texts("", catalog).expect("entries");
        let models = result.get("models").unwrap().as_array().unwrap();

        assert!(
            models[0].get("inputModalities").is_none(),
            "GPT text+image is inferred and must not become a sticky hidden override"
        );
        assert!(
            models[1].get("inputModalities").is_none(),
            "confirmed text-only capability is inferred and must remain registry-driven"
        );
        assert_eq!(
            models[2].get("inputModalities"),
            Some(&json!(["text"])),
            "an unknown model explicitly forced to text-only must round-trip"
        );
        assert_eq!(
            models[3].get("inputModalities"),
            Some(&json!(["text", "image"])),
            "an explicit image override for a registered text-only model must round-trip"
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
    fn codex_bundled_models_command_uses_expected_program_and_args() {
        let command = codex_bundled_models_command(Path::new("codex"));
        assert_eq!(command.get_program(), "codex");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["debug", "models", "--bundled"]
        );
    }

    #[test]
    fn successful_model_catalog_template_load_is_cached() {
        use std::cell::Cell;

        let cache = OnceCell::new();
        let calls = Cell::new(0);
        let first = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Ok(json!({ "slug": "first" }))
        })
        .expect("first template load");
        let second = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Ok(json!({ "slug": "second" }))
        })
        .expect("cached template load");

        assert_eq!(first, json!({ "slug": "first" }));
        assert_eq!(second, first);
        assert_eq!(calls.get(), 1, "successful template should load only once");
    }

    #[test]
    fn failed_model_catalog_template_load_can_retry() {
        use std::cell::Cell;

        let cache = OnceCell::new();
        let calls = Cell::new(0);
        let first = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Err(AppError::Message("temporary failure".to_string()))
        });
        assert!(first.is_err());

        let second = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Ok(json!({ "slug": "recovered" }))
        })
        .expect("retry template load");

        assert_eq!(second, json!({ "slug": "recovered" }));
        assert_eq!(calls.get(), 2, "failed loads must not poison the cache");
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
    fn set_catalog_json_some_preserves_user_owned_catalog() {
        // When CC Switch generates a catalog (Some arm), it must still respect a
        // user-managed external catalog file instead of clobbering it with the
        // cc-switch-owned filename. Only an absent or cc-switch-owned pointer is
        // claimed; this mirrors the None arm's ownership rule.
        let input = r#"model_provider = "custom"
model = "glm-5"
model_catalog_json = "/Users/me/.codex/my-custom-catalog.json"
"#;
        let catalog_path = Path::new("/tmp/cc-switch-model-catalog.json");
        let result = set_codex_model_catalog_json_field(input, Some(catalog_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed.get("model_catalog_json").and_then(|v| v.as_str()),
            Some("/Users/me/.codex/my-custom-catalog.json"),
            "Some arm should NOT clobber a user-owned catalog (full path)"
        );
    }

    #[test]
    fn set_catalog_json_some_preserves_user_owned_relative_filename() {
        // A bare custom filename (no directory component) is also user-owned
        // and must be preserved by the Some arm.
        let input = r#"model_provider = "custom"
model = "glm-5"
model_catalog_json = "my-custom-catalog.json"
"#;
        let catalog_path = Path::new("/tmp/cc-switch-model-catalog.json");
        let result = set_codex_model_catalog_json_field(input, Some(catalog_path)).unwrap();
        let parsed: toml::Value = toml::from_str(&result).unwrap();
        assert_eq!(
            parsed.get("model_catalog_json").and_then(|v| v.as_str()),
            Some("my-custom-catalog.json"),
            "Some arm should NOT clobber a relative user-owned catalog"
        );
    }

    #[test]
    fn resolve_catalog_finds_relative_filename() {
        let config_text = r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result,
            Some(base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)),
            "relative filename should resolve under base_dir for file I/O"
        );
    }

    #[test]
    fn resolve_catalog_rejects_absolute_path_outside_config_dir() {
        let config_text = r#"model_catalog_json = "/tmp/secret/cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result, None,
            "absolute path outside ~/.codex must not be accepted"
        );
    }

    #[test]
    fn resolve_catalog_accepts_absolute_path_inside_config_dir() {
        let config_text = r#"model_catalog_json = "/home/user/.codex/cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result,
            Some(base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)),
            "absolute path inside ~/.codex should be accepted"
        );
    }

    #[test]
    fn resolve_catalog_rejects_traversal_to_parent_directory() {
        let config_text = r#"model_catalog_json = "../cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result, None,
            "relative traversal outside ~/.codex must not be accepted"
        );
    }

    #[test]
    fn resolve_catalog_rejects_symlink_escaping_config_dir() {
        // 词法包含可被符号链接绕过：~/.codex/link -> 外部目录，
        // "link/cc-switch-model-catalog.json" 词法上在 base 内，真实读取却落到
        // base 外。canonicalize 之后的二次校验必须拒绝。
        let temp = tempfile::tempdir().expect("tempdir");
        let base_dir = temp.path().join("codex");
        let outside_dir = temp.path().join("outside");
        fs::create_dir_all(&base_dir).expect("create base");
        fs::create_dir_all(&outside_dir).expect("create outside");
        let escaped_file = outside_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        fs::write(&escaped_file, r#"{"models":[]}"#).expect("write escaped catalog");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_dir, base_dir.join("link")).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside_dir, base_dir.join("link")).expect("symlink");

        let config_text = r#"model_catalog_json = "link/cc-switch-model-catalog.json"
"#;
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result, None,
            "symlink escaping the config dir must be rejected after canonicalization"
        );
    }

    #[test]
    fn resolve_catalog_accepts_real_file_inside_config_dir() {
        // 存在于 base 内的真实文件：canonical 校验通过后仍应接受
        let temp = tempfile::tempdir().expect("tempdir");
        let base_dir = temp.path().join("codex");
        fs::create_dir_all(&base_dir).expect("create base");
        let catalog_file = base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        fs::write(&catalog_file, r#"{"models":[]}"#).expect("write catalog");

        let config_text = r#"model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        let resolved = result.expect("real file inside config dir should be accepted");
        assert_eq!(
            resolved.file_name().and_then(|n| n.to_str()),
            Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
    }

    #[test]
    fn read_limited_string_rejects_oversized_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("huge.json");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_CODEX_CATALOG_BYTES + 1).expect("set_len");

        let result = read_limited_string(&path, MAX_CODEX_CATALOG_BYTES);
        assert!(
            result.is_err(),
            "file larger than MAX_CODEX_CATALOG_BYTES must be rejected"
        );
    }
}
