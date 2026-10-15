//! API Key 变量引用 `{{NAME}}` 的解析。
//!
//! 数据库里的供应商配置保存字面模板（整字段引用：trim 后恰好是 `{{NAME}}`），只在真正
//! 用到 Key 的地方（写 live、代理、用量查询等）替换成变量的当前值。变量值放在进程内缓存
//! 里，热路径（代理请求、投影）只读内存。
//!
//! 支持引用的字段（第一期）：
//! - Claude：`env.ANTHROPIC_AUTH_TOKEN`、`env.ANTHROPIC_API_KEY`
//! - Codex：`auth.OPENAI_API_KEY`
//! - Gemini：`env.GEMINI_API_KEY`、`env.GOOGLE_API_KEY`
//! - Grok Build：`config` 中 `[model."<名称>"]` 表的 `api_key`

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use serde::Serialize;
use serde_json::Value;

use crate::app_config::AppType;
use crate::error::AppError;

/// 变量名最大长度
pub const NAME_MAX_LEN: usize = 64;
/// 保留前缀（脚本契约的环境变量）
pub const RESERVED_PREFIX: &str = "CC_SWITCH_";
/// 每个变量记住的历史值个数（用于写回保护）
const PREVIOUS_KEEP: usize = 3;

fn name_chars_ok(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// 变量名是否合法：`^[A-Z_][A-Z0-9_]*$`，长度 1..=64，且不以 `CC_SWITCH_` 开头
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX_LEN
        && name_chars_ok(name)
        && !name.starts_with(RESERVED_PREFIX)
}

/// 整字段引用：trim 后恰好是 `{{NAME}}` 时返回 NAME
pub fn parse_ref(value: &str) -> Option<&str> {
    let inner = value
        .trim()
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))?;
    (!inner.is_empty() && inner.len() <= NAME_MAX_LEN && name_chars_ok(inner)).then_some(inner)
}

/// 看起来像引用但格式不对（含 `{{` 或 `}}`，又不是合法整字段引用）
pub fn is_malformed_ref(value: &str) -> bool {
    (value.contains("{{") || value.contains("}}")) && parse_ref(value).is_none()
}

pub fn supports_app(app: &AppType) -> bool {
    match app {
        AppType::Claude
        | AppType::ClaudeDesktop
        | AppType::Codex
        | AppType::Gemini
        | AppType::GrokBuild
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes
        | AppType::Pi
        | AppType::Mcode => true,
    }
}

pub fn supported_apps() -> impl Iterator<Item = AppType> {
    AppType::all().filter(supports_app)
}

// ---------------------------------------------------------------------------
// 值缓存
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct CacheEntry {
    enabled: bool,
    value: Option<String>,
    previous: Vec<String>,
}

fn cache() -> &'static RwLock<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// 写入/更新一个变量的缓存（值变化时把旧值记入历史）
pub fn cache_set(name: &str, enabled: bool, value: Option<&str>) {
    let mut guard = cache().write().unwrap_or_else(|e| e.into_inner());
    let entry = guard.entry(name.to_string()).or_default();
    entry.enabled = enabled;
    let new_value = value.filter(|v| !v.is_empty()).map(str::to_string);
    if let Some(old) = entry.value.take() {
        if Some(&old) != new_value.as_ref() {
            entry.previous.insert(0, old);
            entry.previous.truncate(PREVIOUS_KEEP);
        } else {
            entry.value = Some(old);
        }
    }
    if new_value.is_some() {
        entry.value = new_value;
    }
}

pub fn cache_remove(name: &str) {
    cache()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(name);
}

pub fn cache_rename(old: &str, new: &str) {
    let mut guard = cache().write().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = guard.remove(old) {
        guard.insert(new.to_string(), entry);
    }
}

/// 用数据库中的全部变量替换缓存（启动时）
pub fn cache_load(entries: impl IntoIterator<Item = (String, bool, Option<String>)>) {
    let mut map = HashMap::new();
    for (name, enabled, value) in entries {
        map.insert(
            name,
            CacheEntry {
                enabled,
                value: value.filter(|v| !v.is_empty()),
                previous: Vec::new(),
            },
        );
    }
    *cache().write().unwrap_or_else(|e| e.into_inner()) = map;
}

/// 变量的所有已知值（当前值 + 最近历史值），用于写回保护与日志脱敏
pub fn known_values(name: &str) -> Vec<String> {
    let guard = cache().read().unwrap_or_else(|e| e.into_inner());
    guard
        .get(name)
        .map(|e| e.value.iter().chain(e.previous.iter()).cloned().collect())
        .unwrap_or_default()
}

/// 引用的解析状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefStatus {
    Ok,
    Unknown,
    Disabled,
    NoValue,
    Malformed,
}

/// 查找变量当前值
pub fn lookup(name: &str) -> (RefStatus, Option<String>) {
    let guard = cache().read().unwrap_or_else(|e| e.into_inner());
    match guard.get(name) {
        None => (RefStatus::Unknown, None),
        Some(e) if !e.enabled => (RefStatus::Disabled, None),
        Some(CacheEntry { value: None, .. }) => (RefStatus::NoValue, None),
        Some(CacheEntry { value: Some(v), .. }) => (RefStatus::Ok, Some(v.clone())),
    }
}

pub fn unresolved_error(name: &str, status: &RefStatus) -> AppError {
    let (zh, en) = match status {
        RefStatus::Unknown => ("未定义", "is not defined"),
        RefStatus::Disabled => ("已停用", "is disabled"),
        RefStatus::NoValue => ("尚无可用值", "has no value yet"),
        RefStatus::Malformed => ("格式错误", "is malformed"),
        RefStatus::Ok => ("", ""),
    };
    AppError::localized(
        "api_key_vars.unresolved",
        format!("API Key 变量 {{{{{name}}}}} {zh}，请先在「设置 → API Key 变量」中处理"),
        format!("API key variable {{{{{name}}}}} {en}; fix it in Settings → API Key Variables"),
    )
}

/// 解析单个值：非引用原样返回；引用无法解析时报错
pub fn resolve_value(value: &str) -> Result<Cow<'_, str>, AppError> {
    if is_malformed_ref(value) {
        return Err(unresolved_error("?", &RefStatus::Malformed));
    }
    let Some(name) = parse_ref(value) else {
        return Ok(Cow::Borrowed(value));
    };
    match lookup(name) {
        (RefStatus::Ok, Some(v)) => Ok(Cow::Owned(v)),
        (status, _) => Err(unresolved_error(name, &status)),
    }
}

/// 解析"即将使用"的 Key（代理 / 用量等）：引用无法解析时返回 `None`（视为没有 Key），
/// 绝不把字面 `{{...}}` 发给上游。
pub fn resolve_key_for_use(value: &str) -> Option<String> {
    match resolve_value(value) {
        Ok(v) => Some(v.into_owned()),
        Err(e) => {
            log::warn!("[ApiKeyVars] {e}");
            None
        }
    }
}

/// 与 `resolve_key_for_use` 相同，但无法解析时返回空字符串
pub fn resolve_key_or_empty(value: &str) -> String {
    resolve_key_for_use(value).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Key 字段遍历
// ---------------------------------------------------------------------------

fn pointer(parts: &[String]) -> String {
    parts.iter().fold(String::new(), |mut out, part| {
        out.push('/');
        out.push_str(&part.replace('~', "~0").replace('/', "~1"));
        out
    })
}

/// Fields that carry request metadata or managed credentials, rather than a provider's API key.
/// This is deliberately tested only for the structural field being entered: provider IDs and model
/// names are user-controlled and may themselves be named `headers`, `body`, or `oauth-relay`.
fn excluded_branch(key: &str) -> bool {
    matches!(
        key,
        "headers"
            | "http_headers"
            | "env_http_headers"
            | "body"
            | "request_body"
            | "requestBody"
            | "localProxyRequestOverrides"
            | "local_proxy_request_overrides"
            | "metadata"
            | "meta"
            | "auth"
            | "authConfig"
            | "auth_config"
            | "authentication"
            | "tokens"
            | "authProfiles"
            | "auth_profiles"
            | "oauth"
            | "oauthConfig"
            | "oauth_config"
            | "credentials"
            | "managedCredentials"
            | "managed_credentials"
            | "secretRef"
            | "secret_ref"
    )
}

fn accepts_api_key_field(key: &str) -> bool {
    matches!(key, "apiKey" | "api_key")
}

fn opaque_map_key(parts: &[String]) -> bool {
    matches!(
        parts.last().map(String::as_str),
        Some("provider" | "providers" | "model" | "models")
    )
}

fn collect_json_keys(app: &AppType, value: &Value, parts: &mut Vec<String>, out: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        if let Some(array) = value.as_array() {
            for (index, item) in array.iter().enumerate() {
                parts.push(index.to_string());
                collect_json_keys(app, item, parts, out);
                parts.pop();
            }
        }
        return;
    };

    for (key, child) in object {
        if excluded_branch(key) && !opaque_map_key(parts) {
            // Codex's static key is the one supported field in auth.json;
            // account token bundles and authentication metadata stay native.
            if matches!(app, AppType::Codex)
                && parts.is_empty()
                && key == "auth"
                && child.get("OPENAI_API_KEY").is_some_and(Value::is_string)
            {
                out.push("/auth/OPENAI_API_KEY".to_string());
            }
            continue;
        }
        parts.push(key.clone());
        let is_env_key = parts
            .get(parts.len().saturating_sub(2))
            .is_some_and(|parent| parent == "env")
            && (key == "ANTHROPIC_AUTH_TOKEN"
                || key == "AWS_BEARER_TOKEN_BEDROCK"
                || key == "API_KEY"
                || key.ends_with("_API_KEY"));
        let is_codex_auth =
            matches!(app, AppType::Codex) && parts.as_slice() == ["auth", "OPENAI_API_KEY"];
        // Nested model/provider API-key fields are a common cross-client shape.  Their field name,
        // rather than the surrounding wrapper (provider/providers/options/settings), is stable.
        let is_config_key = is_env_key || is_codex_auth || accepts_api_key_field(key);
        if is_config_key && child.is_string() {
            out.push(pointer(parts));
        }
        // Native SecretRef and managed credential values are objects.  Do not descend through an
        // `apiKey`/`api_key` object: only the whole string field is an API-key reference contract.
        if accepts_api_key_field(key) && !child.is_string() {
            parts.pop();
            continue;
        }
        collect_json_keys(app, child, parts, out);
        parts.pop();
    }
}

fn json_key_pointers(app: &AppType, settings: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_json_keys(app, settings, &mut Vec::new(), &mut paths);
    paths
}

/// 遍历 `app` 的 Key 字段：`f(location, value)` 返回 `Some(new)` 时替换该字段。
/// 有任何替换时返回修改后的副本。location 对 JSON 字段是 JSON Pointer，对 Grok 是
/// `model.<表名>.api_key`。
pub fn map_key_fields(
    app: &AppType,
    settings: &Value,
    f: &mut dyn FnMut(&str, &str) -> Option<String>,
) -> Option<Value> {
    let mut out: Option<Value> = None;
    for ptr in json_key_pointers(app, settings) {
        let Some(current) = settings.pointer(&ptr).and_then(Value::as_str) else {
            continue;
        };
        if let Some(new) = f(&ptr, current) {
            let target = out.get_or_insert_with(|| settings.clone());
            if let Some(slot) = target.pointer_mut(&ptr) {
                *slot = Value::String(new);
            }
        }
    }
    if matches!(app, AppType::GrokBuild) {
        if let Some(text) = settings.get("config").and_then(Value::as_str) {
            if let Some(new_text) = visit_grok(text, f) {
                let target = out.get_or_insert_with(|| settings.clone());
                if let Some(obj) = target.as_object_mut() {
                    obj.insert("config".to_string(), Value::String(new_text));
                }
            }
        }
    }
    if matches!(app, AppType::Codex) {
        if let Some(text) = settings.get("config").and_then(Value::as_str) {
            if let Some(new_text) = visit_codex(text, f) {
                let target = out.get_or_insert_with(|| settings.clone());
                if let Some(obj) = target.as_object_mut() {
                    obj.insert("config".to_string(), Value::String(new_text));
                }
            }
        }
    }
    out
}

fn replace_toml_key(
    table: &mut dyn toml_edit::TableLike,
    key: &str,
    loc: &str,
    f: &mut dyn FnMut(&str, &str) -> Option<String>,
) -> bool {
    let Some(current) = table.get(key).and_then(toml_edit::Item::as_str) else {
        return false;
    };
    let Some(new) = f(loc, current) else {
        return false;
    };
    let Some(slot) = table.get_mut(key) else {
        return false;
    };
    let decor = slot.as_value().map(|v| v.decor().clone());
    let mut value = toml_edit::Value::from(new);
    if let Some(decor) = decor {
        *value.decor_mut() = decor;
    }
    *slot = toml_edit::Item::Value(value);
    true
}

fn visit_codex(text: &str, f: &mut dyn FnMut(&str, &str) -> Option<String>) -> Option<String> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let mut changed = false;
    changed |= replace_toml_key(
        doc.as_table_mut(),
        "experimental_bearer_token",
        "toml:/experimental_bearer_token",
        f,
    );
    if let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (name, item) in providers.iter_mut() {
            if let Some(table) = item.as_table_like_mut() {
                changed |= replace_toml_key(
                    table,
                    "experimental_bearer_token",
                    &format!(
                        "toml:/model_providers/{}/experimental_bearer_token",
                        name.get().replace('~', "~0").replace('/', "~1")
                    ),
                    f,
                );
            }
        }
    }
    changed.then(|| doc.to_string())
}

fn visit_grok(text: &str, f: &mut dyn FnMut(&str, &str) -> Option<String>) -> Option<String> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let mut changed = false;
    if let Some(models) = doc
        .get_mut("model")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for (name, item) in models.iter_mut() {
            let Some(table) = item.as_table_like_mut() else {
                continue;
            };
            let Some(current) = table.get("api_key").and_then(toml_edit::Item::as_str) else {
                continue;
            };
            let loc = format!(
                "toml:/model/{}/api_key",
                name.get().replace('~', "~0").replace('/', "~1")
            );
            if let Some(new) = f(&loc, current) {
                if let Some(slot) = table.get_mut("api_key") {
                    let decor = slot.as_value().map(|v| v.decor().clone());
                    let mut value = toml_edit::Value::from(new);
                    if let Some(decor) = decor {
                        *value.decor_mut() = decor;
                    }
                    *slot = toml_edit::Item::Value(value);
                    changed = true;
                }
            }
        }
    }
    changed.then(|| doc.to_string())
}

/// Key 字段的 (location, value) 列表
pub fn key_field_entries(app: &AppType, settings: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    map_key_fields(app, settings, &mut |loc, v| {
        out.push((loc.to_string(), v.to_string()));
        None
    });
    out
}

/// 供应商配置中引用到的变量名（去重，保持顺序）
pub fn referenced_vars(app: &AppType, settings: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if !supports_app(app) {
        return names;
    }
    for (_, v) in key_field_entries(app, settings) {
        if let Some(name) = parse_ref(&v) {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// 把 Key 字段中的引用替换为当前值；任一引用无法解析时报错
pub fn resolve_settings<'a>(
    app: &AppType,
    settings: &'a Value,
) -> Result<Cow<'a, Value>, AppError> {
    if !supports_app(app) {
        return Ok(Cow::Borrowed(settings));
    }
    let mut error: Option<AppError> = None;
    let out = map_key_fields(app, settings, &mut |_, v| {
        if is_malformed_ref(v) {
            if error.is_none() {
                error = Some(unresolved_error("?", &RefStatus::Malformed));
            }
            return None;
        }
        let name = parse_ref(v)?;
        match lookup(name) {
            (RefStatus::Ok, Some(value)) => Some(value),
            (status, _) => {
                if error.is_none() {
                    error = Some(unresolved_error(name, &status));
                }
                None
            }
        }
    });
    if let Some(e) = error {
        return Err(e);
    }
    Ok(out.map(Cow::Owned).unwrap_or(Cow::Borrowed(settings)))
}

/// 与 `resolve_settings` 相同，但无法解析的引用保持字面值（由写入引擎最后一道检查拦下）
pub fn resolve_settings_lossy<'a>(app: &AppType, settings: &'a Value) -> Cow<'a, Value> {
    if !supports_app(app) {
        return Cow::Borrowed(settings);
    }
    map_key_fields(app, settings, &mut |_, v| {
        let name = parse_ref(v)?;
        match lookup(name) {
            (RefStatus::Ok, Some(value)) => Some(value),
            _ => None,
        }
    })
    .map(Cow::Owned)
    .unwrap_or(Cow::Borrowed(settings))
}

/// 引用均可解析时返回 Ok
pub fn ensure_resolvable(app: &AppType, settings: &Value) -> Result<(), AppError> {
    resolve_settings(app, settings).map(|_| ())
}

/// 把引用 `{{old}}` 改写成 `{{new}}`；有修改时返回新配置
pub fn rewrite_refs(app: &AppType, settings: &Value, old: &str, new: &str) -> Option<Value> {
    if !supports_app(app) {
        return None;
    }
    map_key_fields(app, settings, &mut |_, v| {
        (parse_ref(v) == Some(old)).then(|| format!("{{{{{new}}}}}"))
    })
}

/// 写回保护：旧配置某 Key 字段是 `{{NAME}}`，新配置同一字段变成了该变量的当前值或
/// 最近历史值（从 live 读回 / 编辑器保存了解析后的内容），则恢复为模板。
pub fn preserve_refs(app: &AppType, old: &Value, new: &Value) -> Option<Value> {
    if !supports_app(app) {
        return None;
    }
    let old_refs: HashMap<String, String> = key_field_entries(app, old)
        .into_iter()
        .filter(|(_, v)| parse_ref(v).is_some())
        .collect();
    if old_refs.is_empty() {
        return None;
    }
    map_key_fields(app, new, &mut |loc, v| {
        let template = old_refs.get(loc)?;
        if parse_ref(v).is_some() {
            return None;
        }
        let name = parse_ref(template)?;
        known_values(name)
            .iter()
            .any(|known| known == v.trim())
            .then(|| template.clone())
    })
}

/// Restore API-key variable templates from the persisted provider row after a
/// derived client-config readback.  The persisted template is authoritative in
/// this direction: client files contain resolved values and cache history may
/// be unavailable after restart.  User-originated saves must use their
/// separate DAO entry point so an explicit literal can still detach a ref.
pub fn preserve_refs_from_stored_template(
    app: &AppType,
    old: &Value,
    new: &Value,
) -> Option<Value> {
    if !supports_app(app) {
        return None;
    }
    let old_refs: HashMap<String, String> = key_field_entries(app, old)
        .into_iter()
        .filter(|(_, value)| parse_ref(value).is_some())
        .collect();
    if old_refs.is_empty() {
        return None;
    }
    map_key_fields(app, new, &mut |loc, value| {
        let template = old_refs.get(loc)?;
        parse_ref(value).is_none().then(|| template.clone())
    })
}

/// 写入引擎的最后一道检查：新内容里出现了旧内容没有的 `"{{NAME}}"` / `={{NAME}}`
/// 字面引用时返回该变量名。
pub fn new_literal_ref(pre: Option<&[u8]>, planned: Option<&[u8]>) -> Option<String> {
    let planned = std::str::from_utf8(planned?).ok()?;
    if !planned.contains("{{") {
        return None;
    }
    let pre_refs = pre
        .and_then(|b| std::str::from_utf8(b).ok())
        .map(literal_refs)
        .unwrap_or_default();
    literal_refs(planned)
        .into_iter()
        .find(|name| !pre_refs.contains(name))
}

fn literal_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(pos) = text[i..].find("{{") {
        let start = i + pos;
        let Some(end_rel) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end_rel;
        let name = &text[start + 2..end];
        let before = start.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(end + 2).copied();
        let quoted = matches!(
            (before, after),
            (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\''))
        );
        let dotenv_value =
            before == Some(b'=') && after.is_none_or(|c| c == b'\n' || c == b'\r' || c == b' ');
        if (quoted || dotenv_value) && !name.is_empty() && name_chars_ok(name) {
            out.push(name.to_string());
        }
        i = end + 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn name_validation() {
        assert!(is_valid_name("MY_KEY"));
        assert!(is_valid_name("_X1"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("my_key"));
        assert!(!is_valid_name("1KEY"));
        assert!(!is_valid_name("MY-KEY"));
        assert!(!is_valid_name("CC_SWITCH_X"));
        assert!(!is_valid_name(&"A".repeat(65)));
    }

    #[test]
    fn codex_top_level_token_uses_the_same_reference_lifecycle() {
        cache_set("TOP_TOKEN_REF", true, Some("top-token-value"));
        let input = json!({"config": "experimental_bearer_token = '{{TOP_TOKEN_REF}}' # keep\n"});
        assert_eq!(
            referenced_vars(&AppType::Codex, &input),
            vec!["TOP_TOKEN_REF"]
        );
        let resolved = resolve_settings(&AppType::Codex, &input).unwrap();
        let doc = resolved["config"]
            .as_str()
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            doc["experimental_bearer_token"].as_str(),
            Some("top-token-value")
        );
        assert!(resolved["config"].as_str().unwrap().contains("# keep"));
        let renamed =
            rewrite_refs(&AppType::Codex, &input, "TOP_TOKEN_REF", "NEW_TOP_TOKEN").unwrap();
        assert!(renamed["config"]
            .as_str()
            .unwrap()
            .contains("{{NEW_TOP_TOKEN}}"));
        let restored = preserve_refs(&AppType::Codex, &input, &resolved).unwrap();
        let doc = restored["config"]
            .as_str()
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            doc["experimental_bearer_token"].as_str(),
            Some("{{TOP_TOKEN_REF}}")
        );
        assert!(restored["config"].as_str().unwrap().contains("# keep"));
        let malformed =
            json!({"config": "experimental_bearer_token = 'Bearer {{TOP_TOKEN_REF}}'\n"});
        assert!(ensure_resolvable(&AppType::Codex, &malformed).is_err());
    }

    #[test]
    fn auth_containers_keep_native_credentials_except_codex_static_key() {
        let input = json!({
            "auth": {"OPENAI_API_KEY":"{{AUTH_STATIC_KEY}}", "apiKey":"{{KEEP}}"},
            "authConfig":{"api_key":"{{KEEP}}"},
            "authentication":{"apiKey":"{{KEEP}}"},
            "tokens":{"apiKey":"{{KEEP}}"},
            "models":{"auth":{"apiKey":"{{MODEL_AUTH_KEY}}"}}
        });
        cache_set("AUTH_STATIC_KEY", true, Some("auth-static-value"));
        cache_set("MODEL_AUTH_KEY", true, Some("model-static-value"));
        for app in supported_apps() {
            let names = referenced_vars(&app, &input);
            assert!(!names.contains(&"KEEP".to_string()), "{}", app.as_str());
            let resolved = resolve_settings(&app, &input).unwrap();
            assert_eq!(resolved["auth"]["apiKey"], "{{KEEP}}");
            assert_eq!(resolved["tokens"]["apiKey"], "{{KEEP}}");
            assert_eq!(resolved["models"]["auth"]["apiKey"], "model-static-value");
            if matches!(app, AppType::Codex) {
                assert_eq!(resolved["auth"]["OPENAI_API_KEY"], "auth-static-value");
            } else {
                assert_eq!(resolved["auth"]["OPENAI_API_KEY"], "{{AUTH_STATIC_KEY}}");
            }
        }
    }

    #[test]
    fn parse_ref_whole_field_only() {
        assert_eq!(parse_ref("{{MY_KEY}}"), Some("MY_KEY"));
        assert_eq!(parse_ref("  {{MY_KEY}} "), Some("MY_KEY"));
        assert_eq!(parse_ref("{{ MY_KEY }}"), None);
        assert_eq!(parse_ref("{{my_key}}"), None);
        assert_eq!(parse_ref("{{1A}}"), None);
        assert_eq!(parse_ref("Bearer {{X}}"), None);
        assert_eq!(parse_ref("{{X}}y"), None);
        assert_eq!(parse_ref("{X}"), None);
        assert_eq!(parse_ref("{{}}"), None);
        assert_eq!(parse_ref(""), None);
        assert!(is_malformed_ref("Bearer {{X}}"));
        assert!(is_malformed_ref("{{ x }}"));
        assert!(!is_malformed_ref("{{X}}"));
        assert!(!is_malformed_ref("sk-abc"));
    }

    #[test]
    fn resolve_per_app() {
        cache_set("RT_CLAUDE", true, Some("sk-claude"));
        cache_set("RT_CODEX", true, Some("sk-codex"));
        let claude = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "{{RT_CLAUDE}}", "ANTHROPIC_BASE_URL": "{{RT_CLAUDE}}"}});
        let r = resolve_settings(&AppType::Claude, &claude).unwrap();
        assert_eq!(r["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-claude");
        // 非 Key 字段不解析
        assert_eq!(r["env"]["ANTHROPIC_BASE_URL"], "{{RT_CLAUDE}}");
        let claude2 = json!({"env": {"ANTHROPIC_API_KEY": "{{RT_CLAUDE}}"}});
        assert_eq!(
            resolve_settings(&AppType::Claude, &claude2).unwrap()["env"]["ANTHROPIC_API_KEY"],
            "sk-claude"
        );

        let codex = json!({"auth": {"OPENAI_API_KEY": "{{RT_CODEX}}"}, "config": ""});
        assert_eq!(
            resolve_settings(&AppType::Codex, &codex).unwrap()["auth"]["OPENAI_API_KEY"],
            "sk-codex"
        );
        let gemini = json!({"env": {"GEMINI_API_KEY": "{{RT_CODEX}}"}});
        assert_eq!(
            resolve_settings(&AppType::Gemini, &gemini).unwrap()["env"]["GEMINI_API_KEY"],
            "sk-codex"
        );
        let grok = json!({"config": "[models]\ndefault = \"m\"\n\n[model.m]\nmodel = \"grok\"\napi_key = \"{{RT_CODEX}}\" # note\n"});
        let r = resolve_settings(&AppType::GrokBuild, &grok).unwrap();
        let text = r["config"].as_str().unwrap();
        assert!(text.contains("api_key = \"sk-codex\" # note"), "{text}");

        // 无引用：借用，不克隆
        let plain = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-plain"}});
        assert!(matches!(
            resolve_settings(&AppType::Claude, &plain).unwrap(),
            Cow::Borrowed(_)
        ));
        // OpenCode provider Key 也支持引用。
        let oc = json!({"apiKey": "{{RT_CODEX}}"});
        assert_eq!(
            resolve_settings(&AppType::OpenCode, &oc).unwrap()["apiKey"],
            "sk-codex"
        );
    }

    #[test]
    fn unresolvable_refs_error() {
        cache_set("RT_DISABLED", false, Some("v"));
        cache_set("RT_NOVALUE", true, None);
        for (name, key) in [
            ("RT_UNKNOWN_XYZ", "未定义"),
            ("RT_DISABLED", "已停用"),
            ("RT_NOVALUE", "尚无可用值"),
        ] {
            let s = json!({"env": {"ANTHROPIC_AUTH_TOKEN": format!("{{{{{name}}}}}")}});
            let e = resolve_settings(&AppType::Claude, &s).unwrap_err();
            assert!(
                e.to_string().contains(key) || format!("{e:?}").contains(key),
                "{e:?}"
            );
            assert!(ensure_resolvable(&AppType::Claude, &s).is_err());
            // lossy 保留字面值
            assert_eq!(
                resolve_settings_lossy(&AppType::Claude, &s)["env"]["ANTHROPIC_AUTH_TOKEN"],
                format!("{{{{{name}}}}}")
            );
            assert_eq!(resolve_key_for_use(&format!("{{{{{name}}}}}")), None);
        }
        assert_eq!(resolve_key_for_use("sk-plain").as_deref(), Some("sk-plain"));
    }

    #[test]
    fn referenced_and_rewrite() {
        let s =
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "{{A_VAR}}", "ANTHROPIC_API_KEY": "{{A_VAR}}"}});
        assert_eq!(referenced_vars(&AppType::Claude, &s), vec!["A_VAR"]);
        let r = rewrite_refs(&AppType::Claude, &s, "A_VAR", "B_VAR").unwrap();
        assert_eq!(r["env"]["ANTHROPIC_AUTH_TOKEN"], "{{B_VAR}}");
        assert_eq!(r["env"]["ANTHROPIC_API_KEY"], "{{B_VAR}}");
        assert!(rewrite_refs(&AppType::Claude, &s, "OTHER", "B_VAR").is_none());
        let grok = json!({"config": "[model.m]\napi_key = \"{{A_VAR}}\"\n"});
        assert_eq!(referenced_vars(&AppType::GrokBuild, &grok), vec!["A_VAR"]);
        let r = rewrite_refs(&AppType::GrokBuild, &grok, "A_VAR", "C_VAR").unwrap();
        assert!(r["config"].as_str().unwrap().contains("{{C_VAR}}"));
    }

    #[test]
    fn preserve_refs_restores_template() {
        cache_set("RT_PRESERVE", true, Some("sk-old-value"));
        cache_set("RT_PRESERVE", true, Some("sk-new-value"));
        let old = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "{{RT_PRESERVE}}"}});
        for v in ["sk-new-value", "sk-old-value"] {
            let new = json!({"env": {"ANTHROPIC_AUTH_TOKEN": v, "X": 1}});
            let r = preserve_refs(&AppType::Claude, &old, &new).unwrap();
            assert_eq!(r["env"]["ANTHROPIC_AUTH_TOKEN"], "{{RT_PRESERVE}}");
            assert_eq!(r["env"]["X"], 1);
        }
        // 用户主动换成别的明文 Key：不恢复
        let new = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-user-typed"}});
        assert!(preserve_refs(&AppType::Claude, &old, &new).is_none());
        let grok_old = json!({"config": "[model.m]\napi_key = \"{{RT_PRESERVE}}\"\n"});
        let grok_new = json!({"config": "[model.m]\napi_key = \"sk-new-value\"\n"});
        let r = preserve_refs(&AppType::GrokBuild, &grok_old, &grok_new).unwrap();
        assert!(r["config"].as_str().unwrap().contains("{{RT_PRESERVE}}"));
    }

    #[test]
    fn stored_template_readback_does_not_depend_on_cache_history() {
        cache_load([(
            String::from("RT_STORED"),
            true,
            Some(String::from("sk-current")),
        )]);
        let old = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "{{RT_STORED}}"}});
        let readback = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-stale"}});

        // A restarted process has no historical value for `sk-stale`, but a
        // client-config readback still has to preserve the database template.
        assert!(preserve_refs(&AppType::Claude, &old, &readback).is_none());
        let restored =
            preserve_refs_from_stored_template(&AppType::Claude, &old, &readback).unwrap();
        assert_eq!(restored["env"]["ANTHROPIC_AUTH_TOKEN"], "{{RT_STORED}}");
    }

    #[test]
    fn literal_ref_detection() {
        assert_eq!(
            new_literal_ref(None, Some(br#"{"env":{"K":"{{MY_KEY}}"}}"#)).as_deref(),
            Some("MY_KEY")
        );
        assert_eq!(
            new_literal_ref(None, Some(b"GEMINI_API_KEY={{G_KEY}}\n")).as_deref(),
            Some("G_KEY")
        );
        assert_eq!(
            new_literal_ref(None, Some(b"api_key = '{{T_KEY}}'")).as_deref(),
            Some("T_KEY")
        );
        // 已经存在于原文件中的不算
        let pre: &[u8] = br#"{"a":"{{MY_KEY}}"}"#;
        assert_eq!(
            new_literal_ref(Some(pre), Some(br#"{"a":"{{MY_KEY}}","b":1}"#)),
            None
        );
        // 内嵌 / 小写模板（如用量脚本 {{apiKey}}）不算
        assert_eq!(
            new_literal_ref(None, Some(br#"{"a":"Bearer {{MY_KEY}}"}"#)),
            None
        );
        assert_eq!(new_literal_ref(None, Some(br#"{"a":"{{apiKey}}"}"#)), None);
        assert_eq!(new_literal_ref(None, None), None);
    }

    #[test]
    fn all_apps_and_nested_api_key_locations() {
        cache_set("RT_ALL_APPS", true, Some("sk-all"));
        assert_eq!(supported_apps().count(), 10);
        let cases = [
            (
                AppType::Claude,
                json!({"env": {"OPENROUTER_API_KEY": "{{RT_ALL_APPS}}"}}),
            ),
            (
                AppType::Claude,
                json!({"env": {"AWS_BEARER_TOKEN_BEDROCK": "{{RT_ALL_APPS}}"}}),
            ),
            (AppType::Claude, json!({"apiKey": "{{RT_ALL_APPS}}"})),
            (
                AppType::ClaudeDesktop,
                json!({"env": {"API_KEY": "{{RT_ALL_APPS}}"}}),
            ),
            (
                AppType::Gemini,
                json!({"env": {"GOOGLE_API_KEY": "{{RT_ALL_APPS}}"}}),
            ),
            (
                AppType::OpenCode,
                json!({"options": {"apiKey": "{{RT_ALL_APPS}}"}}),
            ),
            (
                AppType::OpenClaw,
                json!({"providers": {"x/y": {"apiKey": "{{RT_ALL_APPS}}"}}}),
            ),
            (
                AppType::Hermes,
                json!({"models": [{"api_key": "{{RT_ALL_APPS}}"}]}),
            ),
            (
                AppType::Pi,
                json!({"provider": {"apiKey": "{{RT_ALL_APPS}}"}}),
            ),
            (
                AppType::Mcode,
                json!({"settings": {"apiKey": "{{RT_ALL_APPS}}"}}),
            ),
        ];
        for (app, settings) in cases {
            assert!(supports_app(&app));
            let entries = key_field_entries(&app, &settings);
            assert_eq!(entries.len(), 1, "{}", app.as_str());
            assert_eq!(entries[0].1, "{{RT_ALL_APPS}}", "{}", app.as_str());
            assert_eq!(
                resolve_settings(&app, &settings)
                    .unwrap()
                    .pointer(&entries[0].0),
                Some(&json!("sk-all")),
                "{}",
                app.as_str()
            );
        }
        assert_eq!(
            key_field_entries(
                &AppType::OpenClaw,
                &json!({"headers":{"apiKey":"{{RT_ALL_APPS}}"}, "apiKey": 1})
            )
            .len(),
            0
        );
        let escaped = json!({"providers": {"a/b~c": {"apiKey": "{{RT_ALL_APPS}}"}}});
        assert_eq!(
            key_field_entries(&AppType::OpenCode, &escaped)[0].0,
            "/providers/a~1b~0c/apiKey"
        );
        let opaque_ids = json!({"providers": {
            "headers": {"apiKey": "{{RT_ALL_APPS}}"},
            "body": {"models": {"oauth-relay": {"api_key": "{{RT_ALL_APPS}}"}}}
        }});
        assert_eq!(key_field_entries(&AppType::Pi, &opaque_ids).len(), 2);
        let excluded = json!({"oauth": {"apiKey": "{{RT_ALL_APPS}}"}, "metadata": {"api_key": "{{RT_ALL_APPS}}"}, "secretRef": {"apiKey": "{{RT_ALL_APPS}}"}});
        assert!(key_field_entries(&AppType::Pi, &excluded).is_empty());
        let native_ref =
            json!({"apiKey": {"source": "env", "id": "X", "api_key": "{{KEEP_LITERAL}}"}});
        assert!(key_field_entries(&AppType::OpenCode, &native_ref).is_empty());
        assert!(matches!(
            resolve_settings(&AppType::OpenCode, &native_ref).unwrap(),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn codex_toml_and_malformed_recognized_keys_are_strict() {
        cache_set("RT_TOML", true, Some("sk-toml"));
        let settings = json!({"config": "experimental_bearer_token = \"{{RT_TOML}}\"\n[model_providers.foo]\nexperimental_bearer_token = \"{{RT_TOML}}\" # keep\n"});
        let resolved = resolve_settings(&AppType::Codex, &settings).unwrap();
        assert!(resolved["config"]
            .as_str()
            .unwrap()
            .contains("\"sk-toml\" # keep"));
        let malformed = json!({"env": {"GEMINI_API_KEY": "Bearer {{RT_TOML}}"}});
        assert!(resolve_settings(&AppType::Gemini, &malformed).is_err());
        assert!(resolve_value("Bearer {{RT_TOML}}").is_err());
    }
}
