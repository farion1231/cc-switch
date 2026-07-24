//! Codex OAuth model list service.
//!
//! ChatGPT Codex exposes models through `chatgpt.com/backend-api/codex/models`,
//! which is not an OpenAI-compatible `/v1/models` endpoint.

use crate::proxy::providers::CODEX_OAUTH_ORIGINATOR;
use crate::services::model_fetch::FetchedModel;
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const CODEX_OAUTH_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const CODEX_OAUTH_FETCH_TIMEOUT_SECS: u64 = 15;
const ERROR_BODY_MAX_CHARS: usize = 512;
const CODEX_OAUTH_CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CODEX_MODELS_CACHE_FILENAME: &str = "models_cache.json";
const CODEX_MODELS_CACHE_BACKUP_FILENAME: &str = "models_cache.cc-switch-backup.json";

/// 使用 ChatGPT OAuth access token 在线读取官方 Codex 模型列表。
///
/// 这里的失败分两层：HTTP 状态码失败说明请求已经到达 ChatGPT 后端；`send`
/// 失败则是 DNS、TLS、代理、超时或本机网络层问题，调用方可以再尝试本地缓存兜底。
pub async fn fetch_models_with_token(
    token: &str,
    account_id: &str,
) -> Result<Vec<FetchedModel>, String> {
    let client = crate::proxy::http_client::get();
    let response = client
        .get(CODEX_OAUTH_MODELS_URL)
        .query(&[("client_version", CODEX_OAUTH_CLIENT_VERSION)])
        .header("Authorization", format!("Bearer {token}"))
        .header("originator", CODEX_OAUTH_ORIGINATOR)
        .header("chatgpt-account-id", account_id)
        .timeout(Duration::from_secs(CODEX_OAUTH_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(format_codex_oauth_request_error)?;

    let status = response.status();
    if !status.is_success() {
        let body = truncate_body(response.text().await.unwrap_or_default());
        return Err(format!("HTTP {status}: {body}"));
    }

    let value: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    Ok(parse_models(value))
}

/// 格式化 OAuth 模型列表请求的网络层错误。
///
/// `reqwest::Error` 的默认文本经常只显示 `error sending request for url`，
/// 不足以区分超时、连接、TLS 或代理问题。这里展开错误链和 CCSM 全局代理状态，
/// 但不包含任何 token、账号明文或请求头。
fn format_codex_oauth_request_error(error: reqwest::Error) -> String {
    let mut hints = Vec::new();
    if error.is_timeout() {
        hints.push("timeout");
    }
    if error.is_connect() {
        hints.push("connect");
    }
    if error.is_builder() {
        hints.push("request_builder");
    }
    if error.is_decode() {
        hints.push("decode");
    }

    let proxy_hint = crate::proxy::http_client::get_current_proxy_url()
        .map(|_| "CCSwitchMulti 全局代理已配置".to_string())
        .unwrap_or_else(|| {
            "CCSwitchMulti 全局代理未配置；Windows/浏览器系统代理不一定会被后端 reqwest 使用"
                .to_string()
        });

    let mut source_parts = Vec::new();
    let mut source = error.source();
    while let Some(current) = source {
        source_parts.push(current.to_string());
        if source_parts.len() >= 4 {
            break;
        }
        source = current.source();
    }

    let kind = if hints.is_empty() {
        "unknown".to_string()
    } else {
        hints.join(",")
    };
    let source_chain = if source_parts.is_empty() {
        "无底层错误链".to_string()
    } else {
        source_parts.join(" -> ")
    };

    format!("Request failed: {error}; kind={kind}; {proxy_hint}; source={source_chain}")
}

/// 读取 Codex 本地模型缓存，作为 OAuth 在线获取失败时的离线兜底。
///
/// CCSwitchMulti 接管时会把原始 `models_cache.json` 备份到
/// `models_cache.cc-switch-backup.json`，因此这里优先读取备份，避免把
/// MultiRouter 合并进去的第三方模型误当作官方 Codex 模型。若没有任何可用缓存，
/// 返回空列表而不是伪造静态模型。
pub fn fetch_cached_models_from_disk() -> Result<Vec<FetchedModel>, String> {
    let mut parse_errors = Vec::new();
    for path in codex_oauth_model_cache_candidates() {
        if !path.exists() {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                parse_errors.push(format!("Failed to read {}: {error}", path.display()));
                continue;
            }
        };
        let value: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(error) => {
                parse_errors.push(format!("Failed to parse {}: {error}", path.display()));
                continue;
            }
        };
        let models = parse_cached_models(value);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    if parse_errors.is_empty() {
        Ok(Vec::new())
    } else {
        Err(parse_errors.join("; "))
    }
}

/// 返回本地官方模型缓存候选路径；备份优先，当前缓存作为兜底。
fn codex_oauth_model_cache_candidates() -> Vec<PathBuf> {
    let codex_dir = crate::codex_config::get_codex_config_dir();
    vec![
        codex_dir.join(CODEX_MODELS_CACHE_BACKUP_FILENAME),
        codex_dir.join(CODEX_MODELS_CACHE_FILENAME),
    ]
}

/// 从 Codex 缓存结构里解析官方模型，并剔除 MultiRouter 合并进去的第三方模型。
fn parse_cached_models(value: Value) -> Vec<FetchedModel> {
    parse_models(value)
        .into_iter()
        .filter(|model| is_likely_codex_oauth_model_id(&model.id))
        .collect()
}

/// 判断缓存条目是否像官方 Codex/ChatGPT 模型。
///
/// 该函数只用于离线 fallback，宁可漏掉不认识的新第三方条目，也不能把 Qwen、
/// DeepSeek 等用户自定义模型灌进 official route。在线接口成功时不走这层过滤。
fn is_likely_codex_oauth_model_id(model_id: &str) -> bool {
    let id = model_id.trim().to_ascii_lowercase();
    if id.starts_with("gpt-") || id.starts_with("codex-") || id.starts_with("chatgpt-") {
        return true;
    }
    ["o1", "o3", "o4", "o5"].iter().any(|prefix| {
        id == *prefix
            || id
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('-'))
    })
}

/// 解析 ChatGPT Codex 模型列表响应，兼容数组、`data`、`items` 和 map 形态。
fn parse_models(value: Value) -> Vec<FetchedModel> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());

    let mut models = Vec::new();

    if let Some(entries) = entries {
        for entry in entries {
            push_model_entry(&mut models, entry, None);
        }
    }

    if let Some(model_map) = value.get("models").and_then(Value::as_object) {
        for (key, entry) in model_map {
            push_model_entry(&mut models, entry, Some(key));
        }
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

/// 将单个响应条目追加到模型列表。
///
/// 条目可能只是字符串模型名，也可能是包含 `slug/id/model/name` 的对象；
/// `fallback_id` 仅用于 map 形态，避免对象里没有显式 id 时丢失 key。
fn push_model_entry(models: &mut Vec<FetchedModel>, entry: &Value, fallback_id: Option<&str>) {
    if let Some(id) = entry.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        models.push(FetchedModel {
            context_window: None,
            id: id.to_string(),
            input_modalities: None,
            owned_by: Some("Codex".to_string()),
            supports_image: None,
        });
        return;
    }

    let Some(obj) = entry.as_object() else {
        if let Some(id) = fallback_id.map(str::trim).filter(|id| !id.is_empty()) {
            models.push(FetchedModel {
                context_window: None,
                id: id.to_string(),
                input_modalities: None,
                owned_by: Some("Codex".to_string()),
                supports_image: None,
            });
        }
        return;
    };

    if model_entry_is_explicitly_unavailable(obj) {
        return;
    }

    let Some(id) = string_field(obj, &["slug", "id", "model", "name"]).or_else(|| {
        fallback_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
    }) else {
        return;
    };
    let owned_by = string_field(
        obj,
        &[
            "owned_by", "ownedBy", "provider", "vendor", "category", "owner",
        ],
    )
    .or_else(|| Some("Codex".to_string()));

    let context_window = extract_context_window(obj);
    let input_modalities = extract_input_modalities(obj);
    let supports_image = extract_supports_image(obj, input_modalities.as_deref());

    models.push(FetchedModel {
        context_window,
        id,
        input_modalities,
        owned_by,
        supports_image,
    });
}

/// 判断官方 Codex 模型条目是否显式标为不可调用。
///
/// ChatGPT 后端有时会返回“存在但当前账号/API 不可用”的模型元数据；这类模型
/// 不能写进 MultiRouter catalog，否则 Codex 选择器会展示它，但 `/responses`
/// 随后返回 `Model not found`。缺少可用性字段时保守保留，只过滤明确否定值。
fn model_entry_is_explicitly_unavailable(obj: &serde_json::Map<String, Value>) -> bool {
    let false_flags = [
        "supported_in_api",
        "supportedInApi",
        "available",
        "is_available",
        "isAvailable",
        "enabled",
    ];
    if false_flags
        .iter()
        .any(|key| obj.get(*key).and_then(Value::as_bool) == Some(false))
    {
        return true;
    }

    if obj.get("disabled").and_then(Value::as_bool) == Some(true) {
        return true;
    }

    let hidden_visibility = string_field(obj, &["visibility", "status", "availability"])
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|value| {
            matches!(
                value.as_str(),
                "hide" | "hidden" | "disabled" | "unavailable" | "unsupported" | "denied"
            )
        });
    hidden_visibility
}

fn string_field(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// 从 Codex OAuth 模型条目中提取上下文窗口。
///
/// 官方接口字段可能随客户端版本变化，只有明确的正整数才会被接受。
fn extract_context_window(obj: &serde_json::Map<String, Value>) -> Option<u64> {
    const KEYS: &[&str] = &[
        "context_window",
        "max_context_window",
        "contextWindow",
        "maxContextWindow",
    ];

    KEYS.iter()
        .filter_map(|key| obj.get(*key))
        .find_map(parse_positive_u64)
}

/// 从官方 Codex 模型条目读取输入模态。
///
/// 这是 Codex Desktop 判断图片入口的关键能力字段；不能在 OAuth 动态目录同步时丢掉。
fn extract_input_modalities(obj: &serde_json::Map<String, Value>) -> Option<Vec<String>> {
    ["input_modalities", "inputModalities", "modalities"]
        .iter()
        .filter_map(|key| obj.get(*key))
        .find_map(parse_input_modalities)
}

fn parse_input_modalities(value: &Value) -> Option<Vec<String>> {
    let values = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        Value::Object(obj) => obj
            .get("input")
            .or_else(|| obj.get("inputs"))
            .and_then(parse_input_modalities)?,
        _ => return None,
    };

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn extract_supports_image(
    obj: &serde_json::Map<String, Value>,
    input_modalities: Option<&[String]>,
) -> Option<bool> {
    if let Some(value) = [
        "supports_image",
        "supportsImage",
        "vision",
        "supports_image_detail_original",
        "supportsImageDetailOriginal",
    ]
    .iter()
    .filter_map(|key| obj.get(*key))
    .find_map(Value::as_bool)
    {
        return Some(value);
    }

    input_modalities.map(|modalities| {
        modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("image"))
    })
}

/// 将 JSON 数字或纯数字字符串转换为正整数。
///
/// 带单位的文本会保留为未知值，让前端继续使用用户填写或默认兜底。
fn parse_positive_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().filter(|v| *v > 0),
        Value::String(text) => text.trim().parse::<u64>().ok().filter(|value| *value > 0),
        _ => None,
    }
}

fn truncate_body(body: String) -> String {
    if body.chars().count() <= ERROR_BODY_MAX_CHARS {
        body
    } else {
        let mut s: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
        s.push_str("...");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_codex_oauth_models_accepts_openai_style_data() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4", "owned_by": "openai" },
                { "id": "gpt-5.4-mini", "ownedBy": "openai" }
            ]
        }));

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.4");
        assert_eq!(models[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(models[1].id, "gpt-5.4-mini");
        assert_eq!(models[1].owned_by.as_deref(), Some("openai"));
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_list_shape() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.3-codex", "display_name": "GPT-5.3 Codex" },
                "gpt-5.5"
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.3-codex".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_codex_oauth_models_deduplicates_ids() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4" },
                { "model": "gpt-5.4" }
            ]
        }));

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.4");
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_map_shape() {
        let models = parse_models(json!({
            "models": {
                "gpt-5.4": { "display_name": "GPT-5.4" },
                "gpt-5.5": { "slug": "gpt-5.5" }
            }
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_codex_oauth_models_extracts_context_window() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.4", "context_window": 272000 },
                { "slug": "gpt-5.5", "maxContextWindow": "1000000" },
                { "slug": "bad", "contextWindow": "128000 tokens" }
            ]
        }));

        assert_eq!(models[0].context_window, None);
        assert_eq!(models[1].context_window, Some(272_000));
        assert_eq!(models[2].context_window, Some(1_000_000));
    }

    #[test]
    fn parse_codex_oauth_models_preserves_image_modalities() {
        let models = parse_models(json!({
            "models": [
                {
                    "slug": "gpt-5.6-sol",
                    "input_modalities": ["text", "image"],
                    "supports_image_detail_original": true
                },
                {
                    "slug": "gpt-5.3-codex-spark",
                    "input_modalities": ["text"],
                    "supports_image_detail_original": false
                }
            ]
        }));

        let sol = models
            .iter()
            .find(|model| model.id == "gpt-5.6-sol")
            .unwrap();
        assert_eq!(
            sol.input_modalities.as_deref(),
            Some(&["text".to_string(), "image".to_string()][..])
        );
        assert_eq!(sol.supports_image, Some(true));

        let spark = models
            .iter()
            .find(|model| model.id == "gpt-5.3-codex-spark")
            .unwrap();
        assert_eq!(
            spark.input_modalities.as_deref(),
            Some(&["text".to_string()][..])
        );
        assert_eq!(spark.supports_image, Some(false));
    }

    #[test]
    fn parse_codex_oauth_models_filters_explicitly_unavailable_entries() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.6-luna", "supported_in_api": false },
                { "slug": "gpt-5.6-hidden", "visibility": "hide" },
                { "slug": "gpt-5.6-disabled", "disabled": true },
                { "slug": "gpt-5.5", "supportedInApi": true },
                { "slug": "gpt-5.4" }
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_cached_models_keeps_official_codex_models_only() {
        let models = parse_cached_models(json!({
            "models": [
                { "slug": "gpt-5.5", "owned_by": "openai" },
                { "slug": "gpt-5.6-luna", "provider": "Codex" },
                { "slug": "codex-auto-review", "provider": "Codex" },
                { "slug": "o4-mini", "provider": "OpenAI" },
                { "slug": "deepseek-chat", "provider": "deepseek" },
                { "slug": "qwen3-coder", "provider": "qwen" }
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec![
                "codex-auto-review".to_string(),
                "gpt-5.5".to_string(),
                "gpt-5.6-luna".to_string(),
                "o4-mini".to_string()
            ]
        );
    }
}
