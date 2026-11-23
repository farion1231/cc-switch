use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{
    atomic_write, delete_file, get_home_dir, path_is_within, read_json_file,
    write_json_file_private, write_text_file_private,
};
use crate::error::AppError;
use crate::model_capabilities::{image_input_capability_from_modalities, ImageInputCapability};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    "minimax.cn",     // MiniMax CN (current official endpoint)
    "minimaxi.com",   // MiniMax CN (legacy endpoint)
    // StepFun Responses API currently supports only `function` tools:
    // platform.stepfun.com/docs/zh/api-reference/responses/responses-create
    "stepfun.com",
    "stepfun.ai",
    // Conservative (unverified, not a confirmed reject): Baidu Qianfan's
    // pay-as-you-go Responses guide documents only `function` / `mcp` tools
    // (cloud.baidu.com/doc/qianfan-docs/s/4mi400l1m). Host-exact; Qianfan's
    // Chat plans on the same domain are ProxyChat and never consult this list.
    "qianfan.baidubce.com",
    // Conservative (unverified): iFlytek Astron Coding Plan fronts third-party
    // models behind one Responses gateway with no documented hosted-tool
    // support (www.xfyun.cn/doc/spark/CodingPlan.html).
    "xf-yun.com",
    // Zhipu GLM CN / global (open.bigmodel.cn, api.z.ai): the native Responses
    // gateway's tool-type enum is `function | web_search_preview |
    // code_interpreter | mcp` (verbatim from the #6944 400 body) — Codex's
    // `web_search` hosted tool is not in it. Matched on host labels (see
    // `codex_url_host_matches_any`), so `xyz.ai` never collides with `z.ai`.
    "bigmodel.cn",
    "z.ai",
];

/// Brand prefixes of models whose native gateways reject `web_search`, matched
/// against the model id's last `/`-segment so aggregator ids like
/// `MiniMaxAI/MiniMax-M3` are caught. Exact brand names (not a fuzzy heuristic),
/// so a supporting gateway is never wrongly matched.
const CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES: &[&str] =
    &["mimo", "longcat", "minimax", "qwen3-coder", "glm"];

/// The URL without its `scheme://`, or the whole input when it has none. Only a
/// real scheme (a letter, then letters, digits, `+`, `-`, `.`) counts, so a
/// `https://` inside a schemeless URL's path or query is not taken for one.
pub(crate) fn url_after_scheme(url: &str) -> &str {
    match url.split_once("://") {
        Some((scheme, rest))
            if scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            rest
        }
        _ => url,
    }
}

/// Host component of a base URL (or a bare host), lowercased, without scheme,
/// userinfo, port, path or query. Tolerates the loose forms users paste into
/// the provider form (`example.com`, `https://user@Example.com:8443/v1`).
pub(crate) fn codex_url_host(url_or_host: &str) -> String {
    let rest = url_after_scheme(url_or_host.trim());
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(ipv6) = host_port.strip_prefix('[') {
        ipv6.split(']').next().unwrap_or(ipv6)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Whether the URL's host IS one of `hosts` or a subdomain of it, matched on
/// DNS label boundaries. Vendor host lists must go through this rather than a
/// substring `contains`: a 4-char entry like `z.ai` would otherwise also match
/// `api.xyz.ai` / `viz.ai` and silently push an unrelated provider onto a
/// vendor-specific code path.
pub(crate) fn codex_url_host_matches_any(url_or_host: &str, hosts: &[&str]) -> bool {
    let host = codex_url_host(url_or_host);
    if host.is_empty() {
        return false;
    }
    hosts.iter().any(|candidate| {
        let candidate = candidate.trim_start_matches('.').to_ascii_lowercase();
        host == candidate || host.ends_with(&format!(".{candidate}"))
    })
}

/// Top-level `model` id from a Codex `config.toml`.
pub(crate) fn codex_top_level_model(config_text: &str) -> Option<String> {
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
pub(crate) fn codex_native_gateway_rejects_web_search(config_text: &str) -> bool {
    if let Some(base_url) = extract_codex_base_url(config_text) {
        if codex_url_host_matches_any(&base_url, CODEX_WEB_SEARCH_REJECT_HOSTS) {
            return true;
        }
    }
    codex_top_level_model(config_text).is_some_and(|model| codex_model_rejects_web_search(&model))
}

/// Whether `model`'s brand is on the reject list. Strips any aggregator
/// "vendor/" prefix first, e.g. "MiniMaxAI/MiniMax-M3" or "qwen/qwen3-coder-plus".
pub(crate) fn codex_model_rejects_web_search(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let model = model.rsplit('/').next().unwrap_or(model.as_str());
    CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES
        .iter()
        .any(|prefix| model.starts_with(prefix))
}

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

/// 测试用：Codex live 的四个文件（auth、config、模型目录、托管账号标记）的字节，用来断言
/// 「被拒的操作没有改动任何文件」。
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexLiveStateSnapshot {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

#[cfg(test)]
impl CodexLiveStateSnapshot {
    pub(crate) fn capture() -> Result<Self, AppError> {
        let paths = [
            get_codex_auth_path(),
            get_codex_config_path(),
            get_codex_model_catalog_path(),
            get_codex_managed_oauth_live_auth_marker_path(),
        ];
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => return Err(AppError::io(&path, err)),
            };
            files.push((path, bytes));
        }
        Ok(Self { files })
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
    /// Copilot selects Responses or Chat per model, but both paths run through
    /// the local proxy. Keep proxy-compatible tools while disabling Codex's
    /// hosted web-search endpoint, which Copilot does not expose.
    Copilot,
    NativeResponses,
    /// Codex talks (through cc-switch's proxy) to a native Anthropic Messages
    /// gateway. Like `NativeResponses` it must suppress Codex's freeform custom
    /// tools — the Responses→Anthropic transform keeps only `function` tools.
    /// Additionally the Codex `web_search` hosted tool is unusable on this path
    /// (the transform drops it), so it is always disabled — see
    /// `codex_disables_web_search`.
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
/// token in the ignored top level. The frontend copy in codexConfigUtils.ts
/// is checked against this by `frontend_parity_tests`.
pub(crate) const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "amazon-bedrock-runtime",
    "openai",
    "ollama",
    "lmstudio",
];

/// 获取 Codex 配置目录路径
pub fn get_codex_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_override_dir(&crate::app_config::AppType::Codex) {
        return custom;
    }

    get_home_dir().join(".codex")
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

pub(crate) fn get_codex_managed_oauth_live_auth_marker_path() -> PathBuf {
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
pub(crate) fn extract_codex_auth_user_identity(auth: &Value) -> Option<String> {
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

/// 托管账号登录标记的内容（和 [`record_codex_managed_oauth_live_auth`] 写的一样），给切换
/// 操作放进同一次提交用。`auth` 不是托管账号的登录形状时返回 `None`。
pub(crate) fn codex_managed_oauth_marker_bytes(
    auth: &Value,
    managed_account_id: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    let managed_account_id = managed_account_id.trim();
    let Some(chatgpt_account_id) = extract_codex_managed_oauth_account_id(auth) else {
        return Ok(None);
    };
    if managed_account_id.is_empty() {
        return Ok(None);
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
    serde_json::to_vec_pretty(&marker)
        .map(Some)
        .map_err(|e| AppError::Message(format!("序列化 Codex 托管账号标记失败: {e}")))
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
    // No marker is the normal state for a native ChatGPT login (no managed
    // account ever wrote auth.json, or it was cleaned up when switching away);
    // only a marker that exists but cannot be read is worth a warning.
    if !marker_path.exists() {
        return Ok(false);
    }
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

pub(crate) fn clear_codex_managed_oauth_live_auth_marker_for_account(
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

    write_json_file_private(&auth_path, refreshed_auth)?;
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
    write_json_file_private(&auth_path, auth)?;

    // 第二步：写 config.toml（失败则回滚 auth.json）
    if let Err(e) = write_text_file_private(&config_path, &cfg_text) {
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

/// `config.toml` 是不是用 `[features] multi_agent_v2` 强制了新版子 agent 工具：写成 `true`，
/// 或写成带 `enabled = true` 的表（codex-rs `features` 的 `FeatureToml`）。Codex 先看它再看
/// 模型目录，打开了它，目录里写的 [`CODEX_CLASSIC_SUBAGENTS`] 就不生效。解析不了时按没有。
pub(crate) fn codex_config_forces_multi_agent_v2(text: &str) -> bool {
    codex_config_feature_enabled(text, "multi_agent_v2")
}

/// `config.toml` 的 `[features] <key>` 是否开启：写成 `true`，或写成带 `enabled = true`
/// 的表（codex-rs `features` 的 `FeatureToml`）。解析不了时按没开。
pub(crate) fn codex_config_feature_enabled(text: &str, key: &str) -> bool {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    let Some(item) = doc.get("features").and_then(|features| features.get(key)) else {
        return false;
    };
    item.as_bool()
        .or_else(|| item.get("enabled").and_then(toml_edit::Item::as_bool))
        .unwrap_or(false)
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
    let id = id.trim();
    !id.is_empty() && !CODEX_RESERVED_MODEL_PROVIDER_IDS.contains(&id)
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
    active_provider_str(&doc, "base_url").map(str::to_string)
}

/// A string field of the active Codex provider: the active
/// `[model_providers.<model_provider>].<key>`, falling back to a top-level
/// `<key>`. A non-active `[model_providers.*]` section is never read.
pub fn active_provider_str<'a>(doc: &'a toml::Value, key: &str) -> Option<&'a str> {
    doc.get("model_provider")
        .and_then(|v| v.as_str())
        .and_then(|active| doc.get("model_providers")?.get(active)?.get(key)?.as_str())
        .or_else(|| doc.get(key)?.as_str())
}

/// `wire_api` values that mean OpenAI Chat Completions, matched trimmed and
/// case-insensitively. The frontend copy is checked against
/// `tests/config/backendConstants.json`.
pub const CHAT_WIRE_APIS: &[&str] = &[
    "chat",
    "chat_completions",
    "chat-completions",
    "openai_chat",
    "openai-chat",
    "openai_chat_completions",
];

/// `wire_api` values that mean Anthropic Messages, matched like [`CHAT_WIRE_APIS`].
pub const ANTHROPIC_WIRE_APIS: &[&str] = &[
    "anthropic",
    "anthropic_messages",
    "anthropic-messages",
    "messages",
    "claude",
];

pub fn is_chat_wire_api(value: &str) -> bool {
    CHAT_WIRE_APIS.contains(&value.trim().to_ascii_lowercase().as_str())
}

pub fn is_anthropic_wire_api(value: &str) -> bool {
    ANTHROPIC_WIRE_APIS.contains(&value.trim().to_ascii_lowercase().as_str())
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

/// The auth mode Codex resolves for an `auth.json` payload
/// (`AuthDotJson::resolved_mode`, `codex-rs/login/src/auth/manager.rs`,
/// 0.153.2): an explicit `auth_mode` wins outright; otherwise presence
/// decides in this order — `personal_access_token`, `bedrock_api_key`,
/// `bedrock_access_keys`, `OPENAI_API_KEY` — and everything else is
/// ChatGPT. Presence is `Option::is_some`, i.e. any non-null value, even an
/// empty one; the material itself is checked afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexResolvedAuthMode {
    ApiKey,
    Chatgpt,
    ChatgptAuthTokens,
    Headers,
    AgentIdentity,
    PersonalAccessToken,
    BedrockApiKey,
    BedrockAccessKeys,
    /// An `auth_mode` string Codex's serde rejects (`rename_all =
    /// "lowercase"` plus explicit camelCase renames, exact match): the whole
    /// file fails to load, which Codex reports as signed out.
    Unrecognized,
}

fn codex_auth_resolved_mode(auth: &serde_json::Map<String, Value>) -> CodexResolvedAuthMode {
    let present = |key: &str| auth.get(key).is_some_and(|value| !value.is_null());

    // `auth_mode: null` deserializes to `None` (serde default) and falls
    // through to the implicit precedence below.
    if let Some(mode) = auth.get("auth_mode").filter(|value| !value.is_null()) {
        return match mode.as_str() {
            Some("apikey") => CodexResolvedAuthMode::ApiKey,
            Some("chatgpt") => CodexResolvedAuthMode::Chatgpt,
            Some("chatgptAuthTokens") => CodexResolvedAuthMode::ChatgptAuthTokens,
            Some("headers") => CodexResolvedAuthMode::Headers,
            Some("agentIdentity") => CodexResolvedAuthMode::AgentIdentity,
            Some("personalAccessToken") => CodexResolvedAuthMode::PersonalAccessToken,
            Some("bedrockApiKey") => CodexResolvedAuthMode::BedrockApiKey,
            Some("bedrockAccessKeys") => CodexResolvedAuthMode::BedrockAccessKeys,
            _ => CodexResolvedAuthMode::Unrecognized,
        };
    }
    if present("personal_access_token") {
        return CodexResolvedAuthMode::PersonalAccessToken;
    }
    if present("bedrock_api_key") {
        return CodexResolvedAuthMode::BedrockApiKey;
    }
    if present("bedrock_access_keys") {
        return CodexResolvedAuthMode::BedrockAccessKeys;
    }
    if present("OPENAI_API_KEY") {
        return CodexResolvedAuthMode::ApiKey;
    }
    CodexResolvedAuthMode::Chatgpt
}

/// True when Codex would load `auth` as a signed-in OpenAI account for a
/// `requires_openai_auth` provider — the state its login screen and
/// `ConfiguredModelProvider::account_state` (0.149+) go by. The auth mode is
/// resolved exactly as Codex does (`codex_auth_resolved_mode`) and only then
/// is the matching credential checked, so a Bedrock credential outranks a
/// stale `OPENAI_API_KEY` sitting next to it just as it does in Codex, where
/// that probe returns `UnsupportedBedrockApiKeyAuth` and fails TUI startup.
/// Modes Codex cannot load from storage (`headers`, unrecognized) are signed
/// out. The credential must be non-blank (stricter than Codex's `is_some`,
/// erring toward "signed out"); metadata such as `last_refresh` never counts.
pub fn codex_auth_has_openai_account_material(auth: &Value) -> bool {
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
    let has = |key: &str| obj.get(key).is_some_and(value_present);

    match codex_auth_resolved_mode(obj) {
        CodexResolvedAuthMode::ApiKey => extract_codex_auth_api_key(auth).is_some(),
        CodexResolvedAuthMode::PersonalAccessToken => has("personal_access_token"),
        CodexResolvedAuthMode::AgentIdentity => has("agent_identity"),
        CodexResolvedAuthMode::Chatgpt | CodexResolvedAuthMode::ChatgptAuthTokens => obj
            .get("tokens")
            .and_then(Value::as_object)
            .is_some_and(|tokens| {
                ["id_token", "access_token", "refresh_token"]
                    .iter()
                    .any(|key| tokens.get(*key).is_some_and(value_present))
            }),
        CodexResolvedAuthMode::Headers
        | CodexResolvedAuthMode::BedrockApiKey
        | CodexResolvedAuthMode::BedrockAccessKeys
        | CodexResolvedAuthMode::Unrecognized => false,
    }
}

/// Where Codex keeps CLI auth, per the top-level `cli_auth_credentials_store`
/// key (`codex-rs/config/src/types.rs`, serde lowercase; unset = `file`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexAuthStoreMode {
    /// `auth.json` is the only store — the file decides login state.
    File,
    /// Keyring only; `auth.json` is never read and is deleted after a save.
    Keyring,
    /// Keyring first, `auth.json` as fallback for both load and save.
    Auto,
    /// In-process only; nothing on disk is ever a login.
    Ephemeral,
    /// Unparsable config or a value Codex would reject.
    Unknown,
}

pub(crate) fn codex_config_auth_store_mode(config_text: &str) -> CodexAuthStoreMode {
    if !config_text.contains("cli_auth_credentials_store") {
        return CodexAuthStoreMode::File;
    }
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return CodexAuthStoreMode::Unknown;
    };
    match doc
        .get("cli_auth_credentials_store")
        .and_then(|item| item.as_str())
    {
        None => CodexAuthStoreMode::File,
        Some("file") => CodexAuthStoreMode::File,
        Some("keyring") => CodexAuthStoreMode::Keyring,
        Some("auto") => CodexAuthStoreMode::Auto,
        Some("ephemeral") => CodexAuthStoreMode::Ephemeral,
        Some(_) => CodexAuthStoreMode::Unknown,
    }
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
    // Explicitly configured models must be listed even when the cached template is hidden.
    entry_obj.insert("visibility".to_string(), json!("list"));
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

    if !matches!(
        profile,
        CodexCatalogToolProfile::ProxyChat | CodexCatalogToolProfile::Copilot
    ) {
        // Native `/responses` and Anthropic gateways reject / drop Codex's freeform
        // `apply_patch` (type=="custom") tool. Strip any key that would make Codex
        // emit a custom/freeform tool, and rely on shell_type="shell_command" for
        // edits. Defensive even though the native template is already clean
        // (guards against template drift / an accidental classic-template clone).
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
    if profile == CodexCatalogToolProfile::Copilot {
        entry_obj.insert(
            "supports_parallel_tool_calls".to_string(),
            json!(spec.supports_parallel_tool_calls.unwrap_or(false)),
        );
    }

    if matches!(
        profile,
        CodexCatalogToolProfile::ProxyChat | CodexCatalogToolProfile::Copilot
    ) {
        // Codex's `original` image detail (full-resolution) is rejected by
        // strict Chat gateways with `400 invalid_request_error`, param
        // `messages.N.content`. Copilot can also select Chat per model, so
        // neither proxy profile may advertise it; Codex keeps to auto/high.
        entry_obj.insert("supports_image_detail_original".to_string(), json!(false));
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CodexCatalogModelSpec {
    model: String,
    /// Explicit user value only. Entries fall back to the model id — except
    /// official vendor catalog entries, which keep the vendor's display name.
    display_name: Option<String>,
    /// Explicit user value only. Entries fall back to the config's
    /// `model_context_window` (or 128k) — except official vendor catalog
    /// entries, which keep the vendor's declared window.
    context_window: Option<u64>,
    /// Per-row override for `supports_parallel_tool_calls` on native and
    /// Anthropic profiles, and Copilot's conservative false default.
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

/// Fixed candidates for locating the `codex` CLI when it is not on the process
/// PATH (common in GUI apps launched outside a terminal).
const CODEX_CLI_FIXED_CANDIDATES: &[&str] = &[
    "codex",                                // PATH (all platforms)
    "/opt/homebrew/bin/codex",              // macOS Apple Silicon Homebrew
    "/usr/local/bin/codex",                 // macOS Intel Homebrew / Linux
    "/home/linuxbrew/.linuxbrew/bin/codex", // Linux Homebrew
];

/// macOS 桌面版的 bundle 名，新的在前：改名前叫 Codex.app，改名迁移有时会两份都留着
/// （openai/codex#48052），这时 ChatGPT.app 是新的那份。
const CODEX_DESKTOP_APP_BUNDLES: &[&str] = &["ChatGPT.app", "Codex.app"];

/// bundle 里自带的 Codex CLI，新结构在前：`codex-cli/bin/codex` 是指向同目录 `CodexCLI.app`
/// 的垫片脚本，更早的版本直接放在 `Resources/codex`。只装了桌面版的人 PATH 上没有 `codex`，
/// 取 Codex 自带的模型列表只能靠它（#8087）。
const CODEX_DESKTOP_CLI_IN_BUNDLE: &[&str] = &[
    "Contents/Resources/codex-cli/bin/codex",
    "Contents/Resources/codex",
];

/// 新结构的 bundle 里，自带 CLI 的包描述（`version` 字段就是它的版本）。旧结构没有。
const CODEX_DESKTOP_CLI_PACKAGE_IN_BUNDLE: &str = "Contents/Resources/codex-cli/codex-package.json";

/// 桌面版自带的 Codex CLI 的版本，读文件、不跑子进程：
///
/// - macOS：ChatGPT.app / Codex.app 里 `codex-cli/codex-package.json` 的 `version`；
/// - Windows：`%LOCALAPPDATA%\OpenAI\Codex\bin` 下装着 `codex.exe` 的版本目录名。
///
/// 桌面版自动更新后、用它建新对话之前，Codex 线程里记的还都是旧版本，而只装桌面版的人 PATH
/// 上又没有 `codex`：不看这里就一直按旧版本拉官方模型列表。版本号认不认得由调用方判断。
pub(crate) fn codex_desktop_cli_versions() -> Vec<String> {
    codex_desktop_cli_versions_in(&get_home_dir())
}

fn codex_desktop_cli_versions_in(home: &Path) -> Vec<String> {
    let mut versions = Vec::new();
    for bundle in CODEX_DESKTOP_APP_BUNDLES {
        for applications in [PathBuf::from("/Applications"), home.join("Applications")] {
            let package = applications
                .join(bundle)
                .join(CODEX_DESKTOP_CLI_PACKAGE_IN_BUNDLE);
            let version = fs::read(&package)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|package| package.get("version")?.as_str().map(str::to_string));
            versions.extend(version);
        }
    }

    #[cfg(windows)]
    {
        if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
            versions.extend(codex_cli_version_dir_names(
                &PathBuf::from(local_appdata)
                    .join("OpenAI")
                    .join("Codex")
                    .join("bin"),
                "codex.exe",
            ));
        }
    }

    versions
}

/// `versions_dir` 下装着 `exe` 的子目录名（桌面版按版本号建目录）。
#[cfg(any(windows, test))]
fn codex_cli_version_dir_names(versions_dir: &Path, exe: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(versions_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(exe).is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

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

    let home = get_home_dir();
    push_env_codex_cli_candidates(&mut candidates, &mut seen);
    push_home_codex_cli_candidates(&mut candidates, &mut seen, &home);
    push_desktop_codex_cli_candidates(&mut candidates, &mut seen, &home);

    candidates
}

/// 桌面版自带的 Codex CLI 里先用的那个（macOS 新 bundle 优先，Windows 取最新的版本目录），
/// 没装桌面版时为 `None`。给找不到独立 CLI 时的运行时命令兜底（#9850）。
pub(crate) fn codex_desktop_cli() -> Option<PathBuf> {
    codex_desktop_cli_in(&get_home_dir())
}

fn codex_desktop_cli_in(home: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    push_desktop_codex_cli_candidates(&mut candidates, &mut HashSet::new(), home);
    candidates.into_iter().next()
}

/// 桌面版自带的 CLI。排在最后：装了独立 CLI 的人照旧先用独立的那个。
///
/// - macOS：`/Applications` 和 `~/Applications` 下的 ChatGPT.app、Codex.app，新 bundle
///   排在前面，不管装在哪个目录（别的系统上不存在，自然跳过）；
/// - Windows：`%LOCALAPPDATA%\OpenAI\Codex\bin\<版本>\codex.exe`，版本目录随桌面版更新而变。
fn push_desktop_codex_cli_candidates(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    home: &Path,
) {
    for bundle in CODEX_DESKTOP_APP_BUNDLES {
        for applications in [PathBuf::from("/Applications"), home.join("Applications")] {
            for layout in CODEX_DESKTOP_CLI_IN_BUNDLE {
                push_existing_codex_cli_candidate(
                    candidates,
                    seen,
                    applications.join(bundle).join(layout),
                );
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
            push_codex_cli_candidates_newest_first(
                candidates,
                seen,
                &PathBuf::from(local_appdata)
                    .join("OpenAI")
                    .join("Codex")
                    .join("bin"),
                "codex.exe",
            );
        }
    }
}

/// `versions_dir` 下各个版本目录里的 `exe`，按它的修改时间从新到旧（相同时按目录名）。
/// 不按目录名排：版本号按字符串比，`0.99.0` 会排在 `0.153.4` 前面。
#[cfg(any(windows, test))]
fn push_codex_cli_candidates_newest_first(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    versions_dir: &Path,
    exe: &str,
) {
    let Ok(entries) = fs::read_dir(versions_dir) else {
        return;
    };

    let mut discovered = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let candidate = entry.path().join(exe);
            let modified = fs::metadata(&candidate).ok()?.modified().ok()?;
            Some((modified, candidate))
        })
        .collect::<Vec<_>>();

    discovered.sort_by(|(a_time, a), (b_time, b)| b_time.cmp(a_time).then_with(|| a.cmp(b)));
    for (_, candidate) in discovered {
        push_codex_cli_candidate(candidates, seen, candidate);
    }
}

/// `login_path` 是登录 shell 的 PATH（解析不到为 `None`）。GUI 启动时继承的是 launchd 的
/// 窄 PATH，npm 装的 `codex` 是 `#!/usr/bin/env node` 脚本，找不到 node 就 exit 127，
/// 只能退到后面的候选（常是桌面版自带的旧版本，目录里缺新模型，#9764）。与运行时命令一样
/// （#8653），把候选自己的目录、登录 shell 的 PATH 补在继承的 PATH 前面。
fn codex_bundled_models_command(candidate: &Path, login_path: Option<&str>) -> Command {
    let mut command = Command::new(candidate);
    command
        .args(["debug", "models", "--bundled"])
        .stdin(Stdio::null());

    #[cfg(not(target_os = "windows"))]
    {
        let inherited = std::env::var("PATH").unwrap_or_default();
        let mut path = match login_path {
            Some(login_path) => crate::commands::merge_path_segments(login_path, &inherited),
            None => inherited,
        };
        // 裸名 `codex` 没有目录，不补。
        if let Some(dir) = candidate.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            path = crate::commands::merge_path_segments(&dir.to_string_lossy(), &path);
        }
        command.env("PATH", path);
    }
    #[cfg(target_os = "windows")]
    let _ = login_path;

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

/// 没跑出 Codex 自带的模型列表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexBundledModelsError {
    /// 每个候选都启动不了：本机找不到 Codex 命令行。
    NoCli,
    /// 找到了命令行，但没跑出能用的列表（退出码非零、输出不是 JSON、列表为空）。带最后
    /// 一个候选的情况，写日志用。
    Failed(String),
}

impl std::fmt::Display for CodexBundledModelsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCli => f.write_str("本机找不到 Codex 命令行"),
            Self::Failed(detail) => f.write_str(detail),
        }
    }
}

/// 本机 Codex 自带的完整模型列表（`codex debug models --bundled`）。和账号无关，版本和
/// 那个二进制一致。不能用不带 `--bundled` 的版本：那会读到 CC Switch 自己写的目录。
pub(crate) fn load_codex_bundled_models() -> Result<Vec<Value>, CodexBundledModelsError> {
    #[cfg(not(target_os = "windows"))]
    let login_path = crate::commands::cached_login_shell_path();
    #[cfg(target_os = "windows")]
    let login_path: Option<String> = None;
    first_bundled_catalog(codex_cli_candidates(), login_path.as_deref(), |catalog| {
        catalog
            .get("models")
            .and_then(Value::as_array)
            .filter(|models| !models.is_empty())
            .cloned()
    })
}

/// 依次跑各个候选的 `codex debug models --bundled`，返回第一份 `pick` 取得出东西的结果。
/// 候选里有不存在的路径（固定位置、PATH 上的裸名），启动不了不算失败；一个都启动不了才是
/// 找不到命令行。
fn first_bundled_catalog<T>(
    candidates: Vec<PathBuf>,
    login_path: Option<&str>,
    pick: impl Fn(&Value) -> Option<T>,
) -> Result<T, CodexBundledModelsError> {
    let mut failure = None;
    for candidate in candidates {
        let candidate_label = candidate.to_string_lossy();
        let output = match codex_bundled_models_command(&candidate, login_path).output() {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                log::debug!("failed to run `{candidate_label} debug models --bundled`: {err}");
                failure = Some(format!("`{candidate_label}` 启动失败: {err}"));
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::debug!("`{candidate_label} debug models --bundled` failed: {stderr}");
            failure = Some(format!(
                "`{candidate_label} debug models --bundled` 失败（{}）: {}",
                output.status,
                stderr.trim()
            ));
            continue;
        }

        let catalog: Value = match serde_json::from_slice(&output.stdout) {
            Ok(catalog) => catalog,
            Err(e) => {
                log::debug!(
                    "Failed to parse `{candidate_label} debug models --bundled` output: {e}"
                );
                failure = Some(format!(
                    "`{candidate_label} debug models --bundled` 的输出不是 JSON: {e}"
                ));
                continue;
            }
        };
        if let Some(found) = pick(&catalog) {
            return Ok(found);
        }
        failure = Some(format!(
            "`{candidate_label} debug models --bundled` 的输出里没有模型"
        ));
    }

    Err(failure.map_or(
        CodexBundledModelsError::NoCli,
        CodexBundledModelsError::Failed,
    ))
}

/// 官方原生行：逐行补 Codex 解析器必需的字段（不覆盖已有值）、补旧的指令字段，再校验。
/// 原生字段一律保留（不套 profile、`comp_hash` 和工具定义不动）。有一行不合格就整份作废
/// （`None`），不只丢那一行：那样会悄悄少一个官方模型。
pub(crate) fn normalize_codex_native_rows(rows: Vec<Value>) -> Option<Vec<Value>> {
    if rows.is_empty() {
        return None;
    }
    rows.into_iter()
        .map(|mut row| {
            fill_template_fields_from_static(&mut row);
            backfill_codex_base_instructions(&mut row);
            codex_native_row_is_valid(&row).then_some(row)
        })
        .collect()
}

/// Codex 的指令有新旧两种写法：新的 `model_messages.instructions_template`，旧的顶层
/// `base_instructions`，有一个就能解析。新版 Codex 写缓存、服务端返回的都只有新的，
/// 更早的 Codex 只认旧的。缺旧字段、有 template 时原样复制（空串也照抄），和 codex-rs
/// 的序列化器一样；已有旧字段的不动，新版以 template 为准。
fn backfill_codex_base_instructions(row: &mut Value) {
    let Some(obj) = row.as_object_mut() else {
        return;
    };
    if obj.get("base_instructions").is_some_and(Value::is_string) {
        return;
    }
    if let Some(template) = obj
        .get("model_messages")
        .and_then(|messages| messages.get("instructions_template"))
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        obj.insert("base_instructions".to_string(), json!(template));
    }
}

fn codex_native_row_is_valid(row: &Value) -> bool {
    let Some(obj) = row.as_object() else {
        return false;
    };
    let has_slug = obj
        .get("slug")
        .and_then(Value::as_str)
        .is_some_and(|slug| !slug.trim().is_empty());
    let has_required = CODEX_CATALOG_PARSER_REQUIRED_FIELDS
        .iter()
        .all(|key| obj.contains_key(*key));
    let has_instructions = obj.get("base_instructions").is_some_and(Value::is_string)
        || obj
            .get("model_messages")
            .and_then(|messages| messages.get("instructions_template"))
            .is_some_and(Value::is_string);
    has_slug && has_required && has_instructions
}

/// 经典工具契约模板：ProxyChat / Copilot 给第三方模型生成目录条目时复制的骨架。取自 Codex
/// 自带的 gpt-5.5 条目，借的是它那套工具形态（freeform apply_patch、unified_exec、没有
/// `tool_mode` 和 v2 子代理），转换层就是照它写的；名字、窗口、可见性都会被改写，跟调用哪个
/// 模型无关。只用仓库里这份，不读 `models_cache.json` 也不跑 `codex`：官方改了那边的条目
/// （标隐藏、加新工具模式、下线）会悄悄带进第三方模型。Codex 新增解析必需字段时，补进这里和
/// `CODEX_CATALOG_PARSER_REQUIRED_FIELDS`。也是没有 `codex` 时唯一的官方 gpt-5.5 条目。
fn load_codex_classic_tool_template() -> Value {
    let text = include_str!("resources/gpt5_5_template.json");
    serde_json::from_str(text).expect("bundled classic tool template must be valid JSON")
}

/// Bundled clean template for native `/responses` providers. Unlike the
/// classic tool template it carries NO freeform `apply_patch` / `web_search` tool
/// declarations and no GPT-5 base_instructions, so Codex never emits a
/// `type=="custom"` tool that native gateways (MiMo/MiniMax/…) reject. Edits
/// flow through `shell_type="shell_command"` instead. We deliberately do NOT
/// fall back to the classic tool template here (that would reintroduce its
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
/// runs through cc-switch's converter (classic tool template contract) and the
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
        // Unknown model: don't inherit the flagship entry's modalities —
        // resolve from the registry/fail-open logic instead, so a vision
        // variant (e.g. deepseek-v4-flash-vision-exp) is not declared
        // text-only merely because the flagship is.
        entry_obj.insert(
            "input_modalities".to_string(),
            json!(codex_catalog_input_modalities(
                &spec.model,
                spec.input_modalities.as_deref(),
            )),
        );
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
const CODEX_CATALOG_PARSER_REQUIRED_FIELDS: &[&str] = &[
    "supports_reasoning_summaries",
    // codex 0.148.0 rejects the catalog without it (#6661); official rows from
    // `codex debug models --bundled` and vendor files can lack it.
    "supports_parallel_tool_calls",
];

/// Rows that come from outside this repo (official rows from whichever `codex`
/// binary is on the machine, bundled vendor files) follow their writer's
/// `ModelInfo` shape and cannot be assumed to satisfy the current
/// external-catalog schema (observed live: 0.144.5 requires
/// `supports_reasoning_summaries` while a coexisting build omitted it).
/// Backfill ONLY parser-required fields from the classic tool template:
/// optional capability fields keep their missing-means-default semantics, and
/// existing values always win.
fn fill_template_fields_from_static(template: &mut Value) {
    let static_template = load_codex_classic_tool_template();
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

#[cfg(test)]
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
    let mut catalog = codex_catalog_from_specs_for_row(&specs, config_text, profile)?;
    if let Some(model) = codex_card_auto_review_model(settings) {
        apply_codex_auto_review_model(&mut catalog, model);
    }
    if codex_card_keeps_official_speed_tiers(settings) {
        apply_codex_official_speed_tiers(&mut catalog, config_text, profile);
    }
    Ok(Some(catalog))
}

/// 卡上的「保留官方 Fast 档」（`modelCatalog.officialSpeedTiers`）。和自动审批模型一样只在
/// 直连、路由下生效，聚合下不看。
fn codex_card_keeps_official_speed_tiers(settings: &Value) -> bool {
    settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("officialSpeedTiers"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// 把官方的速度档还给镜像官方的 GPT 行（[`codex_official_model_entry`] 默认清空）。
///
/// Codex 只在目录行声明了这一档时才发 `service_tier`：没声明时 `config.toml` 里的
/// `service_tier = "fast"` 会被略过并打一条警告，声明了就照发 `"priority"`（0.166 实测）。
/// 所以只有后面接官方账号的中转（如 CPA）才该打开，而且打开后之前在官方卡下选过的 Fast
/// 会立刻在这家生效。只认原生 Responses：Chat 转换会把这个字段原样转给不认识它的上游。
fn apply_codex_official_speed_tiers(
    catalog: &mut Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) {
    if profile != CodexCatalogToolProfile::NativeResponses
        || codex_official_vendor_catalog_models(config_text, profile).is_some()
    {
        return;
    }
    let Some(entries) = catalog.get_mut("models").and_then(Value::as_array_mut) else {
        return;
    };
    let official = codex_openai_official_models();
    for entry in entries.iter_mut() {
        let Some(slug) = entry.get("slug").and_then(Value::as_str) else {
            continue;
        };
        // 生成时按同一规则找官方条目：找得到的行就是镜像出来的那些。
        let Some(found) = find_codex_official_model(slug, &official) else {
            continue;
        };
        let tiers =
            ["service_tiers", "additional_speed_tiers"].map(|key| (key, found.get(key).cloned()));
        if let Some(obj) = entry.as_object_mut() {
            for (key, value) in tiers {
                if let Some(value) = value {
                    obj.insert(key.to_string(), value);
                }
            }
        }
    }
}

/// 卡上选的自动审批模型（`modelCatalog.autoReviewModel`），空串当作没选。只在直连、路由
/// 下生效：聚合下用聚合设置里的（`settings::codex_stack_auto_review_model`）。
fn codex_card_auto_review_model(settings: &Value) -> Option<&str> {
    settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("autoReviewModel"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
}

/// Codex 自动审批（Auto Review）用的模型：写进目录每一行的 `auto_review_model_override`。
/// Codex 按主模型那一行的这个字段选审批模型（codex-rs `ext/guardian-reviewer/src/model.rs`），
/// 主模型可以是目录里任何一行，所以每行都写。审批请求和主对话走同一家：直连、路由下只能
/// 选这一家目录里的模型，聚合下可以选别家发布的 `ccs-*` 模型。
///
/// 不写时 Codex 用供应商默认的 `codex-auto-review`，目录里没有它（第三方的目录）就由主模型
/// 自己审批。`model` 不在目录里时不写：Codex 照样会拿这个名字发请求，上游多半不认。
/// 返回是否写了。
pub(crate) fn apply_codex_auto_review_model(catalog: &mut Value, model: &str) -> bool {
    let Some(entries) = catalog.get_mut("models").and_then(Value::as_array_mut) else {
        return false;
    };
    if !entries
        .iter()
        .any(|entry| entry.get("slug").and_then(Value::as_str) == Some(model))
    {
        return false;
    }
    for entry in entries.iter_mut() {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("auto_review_model_override".to_string(), json!(model));
        }
    }
    true
}

/// Codex 自带的 OpenAI 官方模型列表（`codex debug models --bundled`），给第三方的 GPT
/// 行按模型名找官方条目用。只缓存成功读到的列表：Codex 升级后要重启 CC Switch 才换新。
#[cfg(not(test))]
static CODEX_OPENAI_OFFICIAL_MODELS_CACHE: std::sync::Mutex<Option<Vec<Value>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
thread_local! {
    /// 测试里的官方列表（`None` 时只有编译期内置的 gpt-5.5），不跑本机的 `codex`。
    static CODEX_OPENAI_OFFICIAL_MODELS_OVERRIDE: std::cell::RefCell<Option<Vec<Value>>> =
        const { std::cell::RefCell::new(None) };
}

/// 编译期内置的 gpt-5.5：本机没有可用的 `codex` 时唯一的官方条目。
fn codex_static_official_models() -> Vec<Value> {
    normalize_codex_native_rows(vec![load_codex_classic_tool_template()]).unwrap_or_default()
}

/// 和账号无关的 OpenAI 官方模型列表。不用 `models_cache.json`：它是哪个账号、哪个版本
/// 写的证明不了（见 `codex_official_models`）。
#[cfg(not(test))]
fn codex_openai_official_models() -> Vec<Value> {
    // 持锁加载：并发的首次调用排队等结果，不会同时各跑一遍 `codex`；没读到就不缓存。
    use crate::sync_utils::MutexExt;
    let mut cache = CODEX_OPENAI_OFFICIAL_MODELS_CACHE.lock_recover();
    if cache.is_none() {
        *cache = load_codex_bundled_models()
            .ok()
            .and_then(normalize_codex_native_rows);
    }
    cache.clone().unwrap_or_else(codex_static_official_models)
}

#[cfg(test)]
fn codex_openai_official_models() -> Vec<Value> {
    CODEX_OPENAI_OFFICIAL_MODELS_OVERRIDE
        .with(|rows| rows.borrow().clone())
        .unwrap_or_else(codex_static_official_models)
}

/// 按 Codex 自己查模型信息的规则找官方条目（codex-rs `models-manager/src/manager.rs` 的
/// `construct_model_info_from_candidates`）：先按最长前缀，没有再去掉一层命名空间
/// （`openai/gpt-5.5`）重试；区分大小写。结果就是没有 CC Switch 的目录时 Codex 会用的那条。
fn find_codex_official_model<'a>(model: &str, candidates: &'a [Value]) -> Option<&'a Value> {
    fn longest_prefix<'a>(model: &str, candidates: &'a [Value]) -> Option<&'a Value> {
        candidates
            .iter()
            .filter_map(|candidate| {
                let slug = candidate.get("slug").and_then(Value::as_str)?;
                (!slug.is_empty() && model.starts_with(slug)).then_some((slug.len(), candidate))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, candidate)| candidate)
    }

    longest_prefix(model, candidates).or_else(|| {
        let (namespace, suffix) = model.split_once('/')?;
        let simple_namespace = !namespace.is_empty()
            && namespace
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if suffix.contains('/') || !simple_namespace {
            return None;
        }
        longest_prefix(suffix, candidates)
    })
}

/// 第三方供应商上命中官方的 GPT 行：保留官方的提示词和工具，窗口与档位接受显式覆盖，
/// 未配置时保持官方默认。其余只改掉属于官方账号或官方后端的字段。
fn codex_official_model_entry(
    official: &Value,
    model: &str,
    priority: usize,
    profile: CodexCatalogToolProfile,
    context_window: Option<u64>,
    display_name: Option<&str>,
) -> Value {
    let mut entry = official.clone();
    let Some(obj) = entry.as_object_mut() else {
        return json!({});
    };
    // 前缀、命名空间命中的别名（`gpt-5.5-high`）不冒用官方的显示名；用户填的显示名只影响
    // 选择器里的文字，照用。
    if let Some(name) = display_name {
        obj.insert("display_name".to_string(), json!(name));
    } else if obj.get("slug").and_then(Value::as_str) != Some(model) {
        obj.insert("display_name".to_string(), json!(model));
    }
    obj.insert("slug".to_string(), json!(model));
    obj.insert("priority".to_string(), json!(1000 + priority));
    if let Some(window) = context_window {
        obj.insert("context_window".to_string(), json!(window));
        // Codex clamps model_context_window to max_context_window. A stale
        // official maximum would silently cap the user's explicit window.
        obj.insert("max_context_window".to_string(), json!(window));
    }
    // 官方隐藏的条目用户明确写了，也要出现在选择器里。
    obj.insert("visibility".to_string(), json!("list"));
    // 速度档、升级提示归官方账号。
    obj.insert("service_tiers".to_string(), json!([]));
    obj.insert("additional_speed_tiers".to_string(), json!([]));
    obj.insert("availability_nux".to_string(), Value::Null);
    obj.insert("upgrade".to_string(), Value::Null);
    // 第三方不支持 Responses Lite 协议。
    obj.insert("use_responses_lite".to_string(), Value::Bool(false));
    if profile == CodexCatalogToolProfile::ProxyChat {
        // 同 `codex_catalog_model_entry`：严格的 Chat 网关拒收 `original` 精度的图片。
        obj.insert("supports_image_detail_original".to_string(), json!(false));
    }
    entry
}

/// 官方镜像在没有用户覆盖时的样子（反向解析拿它比对）。
fn codex_official_mirror_baseline(official: &Value, model: &str) -> Value {
    codex_official_model_entry(
        official,
        model,
        0,
        CodexCatalogToolProfile::NativeResponses,
        None,
        None,
    )
}

/// 除窗口外，反向解析会保留的显示名、模态、并行工具调用是否和官方镜像一致。
/// 镜像的官方能力不用存回表单，显式窗口、显示名另行还原。用户改过显示名的镜像
/// 仍带官方的 `description`；旧版 ProxyChat 克隆模板时把 `description` 写成显示名，
/// 靠这一点区分，免得把旧行误当镜像、丢掉用户填的窗口。
fn is_codex_official_mirror(entry: &Value, model: &str, official: &Value) -> bool {
    let expected = codex_official_mirror_baseline(official, model);
    // 旧格式只有 base_instructions。提示词和工具也须匹配，避免把 Anthropic
    // 通用模板或从其他模型克隆的 Chat 条目误认成官方镜像。
    if entry
        .get("model_messages")
        .and_then(|messages| messages.get("instructions_template"))
        .is_none()
        && ["base_instructions", "apply_patch_tool_type"]
            .iter()
            .any(|key| entry.get(*key) != expected.get(*key))
    {
        return false;
    }
    let renamed_mirror = entry.get("description") == expected.get("description")
        && entry.get("description") != entry.get("display_name");
    (renamed_mirror || entry.get("display_name") == expected.get("display_name"))
        && ["input_modalities", "supports_parallel_tool_calls"]
            .iter()
            .all(|key| entry.get(*key) == expected.get(*key))
}

/// The catalog for one provider's models: its official vendor catalog when the
/// gateway ships one, otherwise the profile's template.
fn codex_catalog_from_specs_for_row(
    specs: &[CodexCatalogModelSpec],
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<Value, AppError> {
    let windows = RowWindows::of(config_text);
    // Vendors that publish an OFFICIAL Codex models.json for their native
    // `/responses` gateway get it mirrored verbatim instead of the neutral
    // template: its freeform apply_patch, vendor harness base_instructions and
    // reasoning levels are load-bearing (the harness tells the model to use
    // apply_patch, so catalog and harness must stay consistent).
    if let Some(vendor_models) = codex_official_vendor_catalog_models(config_text, profile) {
        let entries: Vec<Value> = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let mut entry = codex_vendor_catalog_model_entry(&vendor_models, spec, index);
                if windows.window.is_some()
                    || windows.limit.is_some()
                    || spec.context_window.is_some()
                {
                    sink_row_windows(&mut entry, &windows, true);
                }
                entry
            })
            .collect();
        return Ok(json!({ "models": entries }));
    }

    let configured_context_window = windows.window;
    let default_context_window = configured_context_window.unwrap_or(128_000);

    // Native providers use the bundled clean template (no freeform apply_patch);
    // ProxyChat and Copilot clone the bundled classic tool template so their
    // proxy paths retain custom<->function tool support.
    let template = match profile {
        CodexCatalogToolProfile::NativeResponses | CodexCatalogToolProfile::Anthropic => {
            load_codex_native_responses_template()
        }
        CodexCatalogToolProfile::ProxyChat | CodexCatalogToolProfile::Copilot => {
            load_codex_classic_tool_template()
        }
    };
    // 命中 OpenAI 官方条目的行照搬官方（写了目录之后 Codex 只认文件里的条目，通用模板
    // 会顶掉 GPT 自己的提示词），其余行照旧按模板生成。Responses→Anthropic 的转换会丢掉
    // 官方条目里的 custom 工具，这条路不照搬。Copilot 仍按自身的目录配置生成，
    // 保留账号模型的上下文窗口和并行工具能力，而不是套用官方 GPT 的能力。
    let official = match profile {
        CodexCatalogToolProfile::Anthropic | CodexCatalogToolProfile::Copilot => Vec::new(),
        CodexCatalogToolProfile::NativeResponses | CodexCatalogToolProfile::ProxyChat => {
            codex_openai_official_models()
        }
    };
    let entries: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let mut entry = match find_codex_official_model(&spec.model, &official) {
                Some(found) => {
                    let mut entry = codex_official_model_entry(
                        found,
                        &spec.model,
                        index,
                        profile,
                        // Match Codex's global override and Stack's sink_row_windows:
                        // a provider-wide window takes precedence over a model's.
                        configured_context_window.or(spec.context_window),
                        spec.display_name.as_deref(),
                    );
                    if let Some(obj) = entry.as_object_mut() {
                        apply_codex_reasoning_level_override(
                            obj,
                            found.get("default_reasoning_level").and_then(Value::as_str),
                            spec,
                        );
                    }
                    entry
                }
                None => codex_catalog_model_entry(
                    &template,
                    spec,
                    index,
                    profile,
                    default_context_window,
                ),
            };
            // A changed window needs a matching compaction threshold in single-provider
            // catalogs too. Otherwise an official/template threshold can stay at 244800
            // even after the user requests 1M. Keep untouched official defaults intact.
            if windows.window.is_some() || windows.limit.is_some() || spec.context_window.is_some()
            {
                sink_row_windows(&mut entry, &windows, true);
            }
            entry
        })
        .collect();
    Ok(json!({ "models": entries }))
}

/// 一个供应商的模型目录：没有配置模型时为 `None`，不生成、也不指向目录文件。
/// `web_search` 由 [`codex_disables_web_search`] 另算（切走时要按上一家算，不必生成目录）。
pub(crate) struct CodexCatalogPlan {
    pub catalog: Option<Value>,
}

/// 要不要关掉 web_search：Responses→Anthropic 的转换会丢掉这个内置工具，Copilot 也不
/// 提供这个托管工具，两者一律关；原生
/// Responses 网关按拒收名单判定（MiMo、LongCat、MiniMax 等按域名或模型品牌，Qwen3-Coder
/// 按模型）；其余保持 Codex 的默认。只在有模型目录时才看名单。
pub(crate) fn codex_disables_web_search(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> bool {
    match profile {
        CodexCatalogToolProfile::Anthropic | CodexCatalogToolProfile::Copilot => true,
        CodexCatalogToolProfile::NativeResponses => {
            !codex_catalog_model_specs(settings).is_empty()
                && codex_native_gateway_rejects_web_search(config_text)
        }
        CodexCatalogToolProfile::ProxyChat => false,
    }
}

/// 在内存里算出模型目录，不写盘。`config_text` 是归一化后的配置（选路、路由表地址、
/// 模型名、窗口），见 `CodexProjection::catalog_input_text`。
pub(crate) fn plan_codex_model_catalog(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<CodexCatalogPlan, AppError> {
    Ok(CodexCatalogPlan {
        catalog: codex_model_catalog_from_settings(settings, config_text, profile)?,
    })
}

/// 只含一个模型的目录（GPT 模型照样镜像官方条目）：行里没配目录、又得盖掉别家留下的目录
/// 指针时用（`-c` 删不掉键）。
pub(crate) fn codex_single_model_catalog(
    model: &str,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<Value, AppError> {
    let specs = [CodexCatalogModelSpec {
        model: model.to_string(),
        ..CodexCatalogModelSpec::default()
    }];
    let mut catalog = codex_catalog_from_specs_for_row(&specs, config_text, profile)?;
    // 行里本来就没配目录：Lite 跟 Codex 自己的默认（同 `codex_stack_third_party_rows`）。
    if let Some(Value::Array(entries)) = catalog.get_mut("models") {
        apply_codex_responses_lite(entries, &specs, profile, true);
    }
    Ok(catalog)
}

/// 一家第三方供应商发布的模型：行里的模型目录；没有配置目录时只有行的 `model`。
fn codex_published_specs(settings: &Value, config_text: &str) -> Vec<CodexCatalogModelSpec> {
    let specs = codex_catalog_model_specs(settings);
    if !specs.is_empty() {
        return specs;
    }
    codex_top_level_model(config_text)
        .map(|model| {
            vec![CodexCatalogModelSpec {
                model,
                ..CodexCatalogModelSpec::default()
            }]
        })
        .unwrap_or_default()
}

/// Stack 模型用：一家第三方供应商发布的模型名（按目录顺序）。`config_text` 是行里的
/// `config`（只读顶层 `model`）。
pub(crate) fn codex_published_models(settings: &Value, config_text: &str) -> Vec<String> {
    codex_published_specs(settings, config_text)
        .into_iter()
        .map(|spec| spec.model)
        .collect()
}

/// 合并目录里的一家第三方供应商。
pub(crate) struct CodexCatalogRow<'a> {
    pub settings: &'a Value,
    /// 这一家归一化后的配置（`CodexProjection::catalog_input_text`）：地址、模型名、窗口。
    pub config_text: &'a str,
    /// 这一家自己的工具 profile：各家的请求走各自的转换，目录要和转换对得上。
    pub profile: CodexCatalogToolProfile,
}

/// 合并目录里的一家 Stack 供应商。
pub(crate) struct CodexStackCatalogMember<'a> {
    pub key: &'a str,
    pub provider_name: &'a str,
    pub row: CodexCatalogRow<'a>,
}

/// 合并目录里路由那家的行。
pub(crate) enum CodexStackRoute<'a> {
    /// 第三方路由：按它的行生成。
    ThirdParty(CodexCatalogRow<'a>),
    /// 官方路由：官方模型列表的原生行（已补齐、已校验），原样保留；`config_text` 是
    /// 官方卡归一化后的配置，只取窗口键。
    Official {
        native: Vec<Value>,
        config_text: &'a str,
    },
}

/// 合并目录里 Stack 行统一的 `comp_hash`。Codex 在一个会话记下的值变了时，会先用**上一个
/// 模型**压缩一次（codex-rs `maybe_run_previous_model_inline_compact`）；模板带来的值会随
/// 来源漂移（DeepSeek 官方目录是 "3000"，镜像官方的 GPT 行也是），固定值才稳定。
///
/// 第三方路由那家的行也写这个值：否则在它和 Stack 行之间切换都要先用上一家压缩，上一家
/// 不可用（额度用完）时压缩失败，而自定义供应商不会改用当前模型重试，会话从此每轮都卡在
/// 上一家（#8485）。代价是加进第一家、移除最后一家时，路由上原本带值的会话会用路由那家
/// 自己的模型压缩一次。官方路由的原生行不改：失败时 Codex 会改用当前模型重试，官方的
/// 压缩语义也要保留。
const CODEX_STACK_COMP_HASH: &str = "cc-switch";

/// 让一行加入合并目录里共用历史的那组：`comp_hash` 按工具协议分组，同协议的行共用一个值。
///
/// 镜像官方条目的第三方 GPT 行带着 `tool_mode: "code_mode_only"`，shell 调用在历史里是
/// custom 工具 `exec`；其余第三方行是 direct，只有 `exec_command`。两者若共用一个值，切换时
/// 不压缩，direct 模型会照着历史调用不存在的 `exec`，每轮都报 `unsupported call: exec`
/// （#8676）。所以协议不同就分开：跨组切换先用上一家压缩，把对方协议的调用收进摘要；
/// 组内切换照旧不压缩（#8485）。代价是跨组切换时上一家若不可用，压缩会失败，#8485 在这条
/// 路径上回来；组内换一家或新开会话都能绕开。direct 行沿用 [`CODEX_STACK_COMP_HASH`]，
/// 老会话记下的值不变。
fn join_stack_history(obj: &mut serde_json::Map<String, Value>) {
    let comp_hash = match obj.get("tool_mode").and_then(Value::as_str) {
        Some(mode) => format!("{CODEX_STACK_COMP_HASH}-{mode}"),
        None => CODEX_STACK_COMP_HASH.to_string(),
    };
    obj.insert("comp_hash".to_string(), json!(comp_hash));
}

/// Stack 名单非空时的模型目录：路由那家的行在前，各 Stack 供应商的行按名单顺序在后，
/// `priority` 统一重新编号。
///
/// 窗口类全局键（`model_context_window`、`model_auto_compact_token_limit`）这时不写进
/// `config.toml`（Codex 会拿它覆盖所有行），改由各家写进自己的行，见 [`sink_row_windows`]。
///
/// `classic_subagents`：每一行都改用经典子 agent 工具，见 [`CODEX_CLASSIC_SUBAGENTS`]。
pub(crate) fn plan_codex_stack_catalog(
    route: CodexStackRoute<'_>,
    stack: &[CodexStackCatalogMember<'_>],
    classic_subagents: bool,
) -> Result<Value, AppError> {
    let mut entries = match route {
        CodexStackRoute::ThirdParty(row) => {
            let mut rows = codex_stack_third_party_rows(&row)?;
            for entry in &mut rows {
                if let Some(obj) = entry.as_object_mut() {
                    join_stack_history(obj);
                }
            }
            rows
        }
        CodexStackRoute::Official {
            mut native,
            config_text,
        } => {
            // 按官方的 priority 排好再重新编号，模型选择器里的顺序和默认模型都不变。
            native.sort_by_key(|entry| {
                entry
                    .get("priority")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::MAX)
            });
            let windows = RowWindows::of(config_text);
            for entry in &mut native {
                sink_row_windows(entry, &windows, false);
            }
            native
        }
    };
    for member in stack {
        for mut entry in codex_stack_third_party_rows(&member.row)? {
            let Some(obj) = entry.as_object_mut() else {
                continue;
            };
            join_stack_history(obj);
            let Some(model) = obj.get("slug").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let display = obj
                .get("display_name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| model.clone());
            obj.insert(
                "slug".to_string(),
                json!(crate::mode::stack::encode(
                    &crate::app_config::AppType::Codex,
                    member.key,
                    &model,
                    false,
                )),
            );
            obj.insert(
                "display_name".to_string(),
                json!(crate::mode::stack::display_name(
                    &display,
                    member.provider_name
                )),
            );
            let window = obj
                .get("context_window")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            obj.insert(
                "description".to_string(),
                json!(crate::mode::stack::model_description(&model, window)),
            );
            entries.push(entry);
        }
    }
    for (index, entry) in entries.iter_mut().enumerate() {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("priority".to_string(), json!(index + 1));
            if classic_subagents {
                obj.insert(
                    "multi_agent_version".to_string(),
                    json!(CODEX_CLASSIC_SUBAGENTS),
                );
            }
        }
    }
    Ok(json!({ "models": entries }))
}

/// Codex 按目录行的 `multi_agent_version` 决定子 agent 用哪套工具（codex-rs
/// `session/turn_context.rs`）。新版（`"v2"`）把派给子 agent 的任务交给主 agent 那家后端加密，
/// 子 agent 换到别家就解不开；经典（`"v1"`）的任务是明文。聚合模式下主、子 agent 可以是不同
/// 的供应商，用户打开设置后每一行都写经典（包括官方行：官方签发的密文第三方同样解不开）。
/// `config.toml` 里 `[features] multi_agent_v2 = true` 比目录优先，这时写了也不生效。
const CODEX_CLASSIC_SUBAGENTS: &str = "v1";

/// 一家第三方供应商在合并目录里的行（`comp_hash` 保持模板的值，由调用方经
/// [`join_stack_history`] 统一改写）。
pub(crate) fn codex_stack_third_party_rows(
    row: &CodexCatalogRow<'_>,
) -> Result<Vec<Value>, AppError> {
    let specs = codex_published_specs(row.settings, row.config_text);
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let catalog = codex_catalog_from_specs_for_row(&specs, row.config_text, row.profile)?;
    let mut entries = match catalog {
        Value::Object(mut obj) => match obj.remove("models") {
            Some(Value::Array(entries)) => entries,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let follows_codex_default = codex_catalog_model_specs(row.settings).is_empty();
    let windows = RowWindows::of(row.config_text);
    for entry in &mut entries {
        sink_row_windows(entry, &windows, true);
    }
    apply_codex_responses_lite(&mut entries, &specs, row.profile, follows_codex_default);
    Ok(entries)
}

/// 决定 CC Switch 替一家第三方生成的目录行用不用 Responses Lite。
///
/// 行里没配模型目录时，这家在直连、路由下不写目录，Codex 用自己的官方条目，GPT-6 系列
/// 走 Responses Lite（请求带 `reasoning.context = "all_turns"`）。CC Switch 不得不替它写
/// 目录时（聚合的合并目录、「打开终端」盖掉上一家留下的指针）照着来，同一家在各条路径上
/// 发的请求一致：按官方客户端转给官方后端的中转会自己加 Lite 头，请求里缺 all_turns 就被拒
/// （#9665）。显式配了目录的行、走转换的行仍不用 Lite（转换层不认识它）。
fn apply_codex_responses_lite(
    entries: &mut [Value],
    specs: &[CodexCatalogModelSpec],
    profile: CodexCatalogToolProfile,
    row_has_no_catalog: bool,
) {
    let official = if row_has_no_catalog && profile == CodexCatalogToolProfile::NativeResponses {
        codex_openai_official_models()
    } else {
        Vec::new()
    };
    for (entry, spec) in entries.iter_mut().zip(specs) {
        let lite = find_codex_official_model(&spec.model, &official)
            .and_then(|found| found.get("use_responses_lite"))
            == Some(&Value::Bool(true));
        if let Some(obj) = entry.as_object_mut() {
            if lite {
                obj.insert("use_responses_lite".to_string(), Value::Bool(true));
            } else if obj.get("use_responses_lite") == Some(&Value::Bool(true)) {
                obj.insert("use_responses_lite".to_string(), Value::Bool(false));
            }
        }
    }
}

/// 一家行里配置的窗口类全局键（见 [`sink_row_windows`]）。
struct RowWindows {
    window: Option<u64>,
    limit: Option<u64>,
}

impl RowWindows {
    fn of(config_text: &str) -> Self {
        Self {
            window: extract_codex_top_level_u64(config_text, "model_context_window"),
            limit: extract_codex_top_level_u64(config_text, "model_auto_compact_token_limit"),
        }
    }
}

/// 把一家行里的窗口类全局键写进它自己的行：`model_context_window` 写成行的窗口，
/// `model_auto_compact_token_limit` 写成行的压缩点。第三方行没有压缩点时写窗口的 90%
/// （Codex 自己的默认也是 90%，写出来是为了不受别的来源影响）；官方原生行只写行里
/// 明确配置的值，其余保持原样。
fn sink_row_windows(entry: &mut Value, windows: &RowWindows, third_party: bool) {
    let Some(obj) = entry.as_object_mut() else {
        return;
    };
    if let Some(window) = windows.window {
        obj.insert("context_window".to_string(), json!(window));
        obj.insert("max_context_window".to_string(), json!(window));
    }
    let limit = windows.limit.or_else(|| {
        third_party
            .then(|| obj.get("context_window").and_then(Value::as_u64))
            .flatten()
            .filter(|window| *window > 0)
            .map(|window| (u128::from(window) * 9 / 10) as u64)
    });
    if let Some(limit) = limit {
        obj.insert("auto_compact_token_limit".to_string(), json!(limit));
    }
}

/// Reverse of `plan_codex_model_catalog`: read the
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
    // 写入侧会重写指针（原生目录写绝对路径、WSL 形态目录写裸文件名），配置自愈。
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

    let mut official: Option<Vec<Value>> = None;
    // 镜像行默认清空速度档，带着就是卡上开了「保留官方 Fast 档」。
    let mut official_speed_tiers = false;
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
        // Stack 模型的行（保留前缀）不属于路由那家，不能进它的编辑表单、再被保存回库里。
        if !matches!(
            crate::mode::stack::decode(&crate::app_config::AppType::Codex, model),
            crate::mode::stack::Decoded::Plain
        ) {
            continue;
        }
        // 官方镜像只还原模型名及窗口、档位的显式差异，不能把官方能力当成用户填的存回库里。
        // 新旧指令字段都可用于识别；通用模板和旧版 Chat 克隆仍需经过下面的镜像比对。
        if entry
            .get("model_messages")
            .and_then(|messages| messages.get("instructions_template"))
            .is_some()
            || entry.get("base_instructions").is_some_and(Value::is_string)
        {
            let official = official.get_or_insert_with(codex_openai_official_models);
            if let Some(found) = find_codex_official_model(model, official)
                .filter(|found| is_codex_official_mirror(entry, model, found))
            {
                let mut restored = json!({ "model": model });
                let fallback_window =
                    extract_codex_top_level_u64(config_text, "model_context_window")
                        .or_else(|| found.get("context_window").and_then(Value::as_u64));
                if let Some(window) = entry
                    .get("context_window")
                    .and_then(Value::as_u64)
                    .filter(|window| *window > 0 && Some(*window) != fallback_window)
                {
                    restored["contextWindow"] = json!(window);
                }
                let mirrored_name = codex_official_mirror_baseline(found, model);
                if let Some(name) = entry
                    .get("display_name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| {
                        !name.is_empty()
                            && *name != model
                            && mirrored_name.get("display_name").and_then(Value::as_str)
                                != Some(*name)
                    })
                {
                    restored["displayName"] = json!(name);
                }
                official_speed_tiers |= entry
                    .get("additional_speed_tiers")
                    .and_then(Value::as_array)
                    .is_some_and(|tiers| !tiers.is_empty());
                // Compare effort IDs, not descriptions: wording changes in the official
                // catalog must not become sticky user overrides on recovery.
                let efforts = |row: &Value| {
                    row.get("supported_reasoning_levels")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|level| level.get("effort").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                };
                let levels = efforts(entry);
                if !levels.is_empty()
                    && (levels != efforts(found)
                        || entry.get("default_reasoning_level")
                            != found.get("default_reasoning_level"))
                {
                    restored["reasoningLevels"] = json!(levels);
                    if let Some(default) =
                        entry.get("default_reasoning_level").and_then(Value::as_str)
                    {
                        restored["defaultReasoningLevel"] = json!(default);
                    }
                }
                entries.push(restored);
                continue;
            }
        }

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

    // 卡上选的自动审批模型写在每一行上（见 `apply_codex_auto_review_model`）：这张卡的行都写着
    // 同一个、而且是这张卡自己的模型时才还原。聚合设置写的 `ccs-*` 不属于这张卡。
    let mut overrides = models.iter().filter_map(|entry| {
        let slug = entry.get("slug").and_then(Value::as_str)?.trim();
        matches!(
            crate::mode::stack::decode(&crate::app_config::AppType::Codex, slug),
            crate::mode::stack::Decoded::Plain
        )
        .then(|| {
            entry
                .get("auto_review_model_override")
                .and_then(Value::as_str)
        })
    });
    let auto_review = overrides.next().flatten().filter(|model| {
        overrides.all(|other| other == Some(*model))
            && entries
                .iter()
                .any(|entry| entry.get("model").and_then(Value::as_str) == Some(*model))
    });

    let mut simplified = json!({ "models": entries });
    if let Some(model) = auto_review {
        simplified["autoReviewModel"] = json!(model);
    }
    if official_speed_tiers {
        simplified["officialSpeedTiers"] = json!(true);
    }
    Some(simplified)
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

/// Whether a live Codex config is the official route projected by an older CC Switch
/// (`model_provider = "cc-switch-official"`).
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

/// live 的 `config.toml` 是不是现在的代理官方路由（`is_proxy_url` 认本地代理给 Codex 的
/// 地址）：没开统一会话历史时不选别的 provider、顶层 `openai_base_url` 改道到代理；开了
/// 时选 custom，表是指向代理的官方镜像。两种都没有占位 Key，只能按地址认。
pub fn codex_config_routes_official_to_proxy(
    config_text: &str,
    is_proxy_url: impl Fn(&str) -> bool,
) -> bool {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return false;
    };
    let same_url = |item: Option<&toml_edit::Item>| {
        item.and_then(|item| item.as_str())
            .is_some_and(|url| is_proxy_url(url.trim().trim_end_matches('/')))
    };
    match doc.get("model_provider").and_then(|item| item.as_str()) {
        None | Some("openai") => same_url(doc.get("openai_base_url")),
        Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID) => doc
            .get("model_providers")
            .and_then(|item| item.as_table_like())
            .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
            .and_then(|item| item.as_table_like())
            .is_some_and(|table| {
                table
                    .get("requires_openai_auth")
                    .and_then(|item| item.as_bool())
                    == Some(true)
                    && same_url(table.get("base_url"))
            }),
        Some(_) => false,
    }
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

#[cfg(test)]
mod tests;
