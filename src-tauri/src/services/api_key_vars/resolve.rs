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
    matches!(
        app,
        AppType::Claude | AppType::Codex | AppType::Gemini | AppType::GrokBuild
    )
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

fn json_key_pointers(app: &AppType) -> &'static [&'static str] {
    match app {
        AppType::Claude => &["/env/ANTHROPIC_AUTH_TOKEN", "/env/ANTHROPIC_API_KEY"],
        AppType::Codex => &["/auth/OPENAI_API_KEY"],
        AppType::Gemini => &["/env/GEMINI_API_KEY", "/env/GOOGLE_API_KEY"],
        _ => &[],
    }
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
    for ptr in json_key_pointers(app) {
        let Some(current) = settings.pointer(ptr).and_then(Value::as_str) else {
            continue;
        };
        if let Some(new) = f(ptr, current) {
            let target = out.get_or_insert_with(|| settings.clone());
            if let Some(slot) = target.pointer_mut(ptr) {
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
    out
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
            let loc = format!("model.{}.api_key", name.get());
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
        // 不支持的应用原样返回
        let oc = json!({"apiKey": "{{RT_CODEX}}"});
        assert!(matches!(
            resolve_settings(&AppType::OpenCode, &oc).unwrap(),
            Cow::Borrowed(_)
        ));
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
}
