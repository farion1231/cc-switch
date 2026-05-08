//! 模型列表获取服务
//!
//! 通过 OpenAI 兼容的 GET /v1/models 端点获取供应商可用模型列表。
//! 主要面向第三方聚合站（硅基流动、OpenRouter 等），以及把 Anthropic
//! 协议挂在兼容子路径上的官方供应商（DeepSeek、Kimi、智谱 GLM 等）。

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 获取到的模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModel {
    pub id: String,
    pub owned_by: Option<String>,
}

/// OpenAI 兼容的 /v1/models 响应格式
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Option<Vec<ModelEntry>>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    owned_by: Option<String>,
}

/// Google 原生 Gemini API（Generative Language API）的 /v1beta/models 响应格式
///
/// `nextPageToken` 非空时表示还有更多页，需追加 `?pageToken=<token>` 继续请求。
#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    models: Option<Vec<GeminiModelEntry>>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiModelEntry {
    /// 形如 "models/gemini-3.1-pro-preview"
    name: String,
    #[serde(default, rename = "supportedGenerationMethods")]
    supported_generation_methods: Vec<String>,
}

const FETCH_TIMEOUT_SECS: u64 = 15;

/// 404/405 响应体截断长度：避免把几十 KB HTML 404 页整页保留到错误串里。
const ERROR_BODY_MAX_CHARS: usize = 512;

/// 已知的「Anthropic 协议兼容子路径」后缀；按长度降序，最长前缀优先匹配。
/// baseURL 命中这些后缀时，候选列表会追加「剥离后缀再拼 /v1/models / /models」的版本。
const KNOWN_COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

/// 获取供应商的可用模型列表
///
/// 使用 OpenAI 兼容的 GET /v1/models 端点，按候选列表顺序尝试。
///
/// 特例：当 baseURL 指向 Google 原生 Gemini API（`generativelanguage.googleapis.com`）
/// 或显式 `models_url_override` 命中同源时，改走 `/v1beta/models` + `x-goog-api-key`
/// 头，返回 Gemini 响应格式（`name` 字段去掉 `models/` 前缀）。Google 不接 OpenAI
/// 风格的 `Authorization: Bearer`，必须走原生协议。
pub async fn fetch_models(
    base_url: &str,
    api_key: &str,
    is_full_url: bool,
    models_url_override: Option<&str>,
) -> Result<Vec<FetchedModel>, String> {
    if api_key.is_empty() {
        return Err("API Key is required to fetch models".to_string());
    }

    if is_google_native_gemini(base_url, models_url_override) {
        return fetch_models_gemini_native(base_url, api_key, models_url_override).await;
    }

    let candidates = build_models_url_candidates(base_url, is_full_url, models_url_override)?;
    let client = crate::proxy::http_client::get();
    let mut last_err: Option<String> = None;

    for url in &candidates {
        log::debug!("[ModelFetch] Trying endpoint: {url}");
        let response = match client
            .get(url)
            .header("Authorization", format!("Bearer {api_key}"))
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(format!("Request failed: {e}"));
            }
        };

        let status = response.status();

        if status.is_success() {
            let resp: ModelsResponse = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {e}"))?;

            let mut models: Vec<FetchedModel> = resp
                .data
                .unwrap_or_default()
                .into_iter()
                .map(|m| FetchedModel {
                    id: m.id,
                    owned_by: m.owned_by,
                })
                .collect();

            models.sort_by(|a, b| a.id.cmp(&b.id));
            return Ok(models);
        }

        if status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED {
            let body = truncate_body(response.text().await.unwrap_or_default());
            last_err = Some(format!("HTTP {status}: {body}"));
            continue;
        }

        let body = truncate_body(response.text().await.unwrap_or_default());
        return Err(format!("HTTP {status}: {body}"));
    }

    Err(format!(
        "All candidates failed: {}",
        last_err.unwrap_or_else(|| "no candidates".to_string())
    ))
}

/// 判断是否需要走 Google 原生 Gemini API 路径。
///
/// 命中条件：baseURL 或显式 `modelsUrl` 的 **host** 是 `generativelanguage.googleapis.com`。
/// 精确解析 host 而非 `contains()` 子串匹配，避免代理 URL 路径中带该域名时误判。
fn is_google_native_gemini(base_url: &str, models_url_override: Option<&str>) -> bool {
    const GOOGLE_GEMINI_HOST: &str = "generativelanguage.googleapis.com";

    let host_matches = |raw: &str| -> bool {
        url::Url::parse(raw)
            .map(|parsed| parsed.host_str() == Some(GOOGLE_GEMINI_HOST))
            .unwrap_or(false)
    };

    if host_matches(base_url) {
        return true;
    }
    if let Some(url) = models_url_override {
        if host_matches(url) {
            return true;
        }
    }
    false
}

/// 走 Google 原生 Gemini `/v1beta/models` 端点，全量拉取（含分页）。
///
/// 鉴权用 `x-goog-api-key` header（API Key），不是 Bearer。响应 `models[].name`
/// 形如 `models/gemini-3.1-pro-preview`，过滤后返回去掉 `models/` 前缀的 ID。
/// 仅保留支持 `generateContent` 的文本/多模态模型，TTS / Lyria 等其他生成方式
/// 的不会出现在 Gemini CLI 的可选清单里。
///
/// Gemini API 默认每页 50 个模型（`pageSize`），返回 `nextPageToken` 表示还有更多。
/// 本函数循环直至 `nextPageToken` 为空，保证不遗漏模型。
async fn fetch_models_gemini_native(
    base_url: &str,
    api_key: &str,
    models_url_override: Option<&str>,
) -> Result<Vec<FetchedModel>, String> {
    let base = if let Some(raw) = models_url_override {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            build_gemini_native_models_url(base_url)?
        } else {
            trimmed.to_string()
        }
    } else {
        build_gemini_native_models_url(base_url)?
    };

    log::debug!("[ModelFetch] Gemini native endpoint: {base}");
    let client = crate::proxy::http_client::get();
    let mut all_models: Vec<FetchedModel> = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let url = match &page_token {
            Some(token) => format!("{base}?pageToken={token}"),
            None => base.clone(),
        };

        let response = client
            .get(&url)
            .header("x-goog-api-key", api_key)
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = truncate_body(response.text().await.unwrap_or_default());
            return Err(format!("HTTP {status}: {body}"));
        }

        let resp: GeminiModelsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {e}"))?;

        if let Some(models) = resp.models {
            for m in models {
                if m.supported_generation_methods
                    .iter()
                    .any(|s| s == "generateContent")
                {
                    all_models.push(FetchedModel {
                        id: m
                            .name
                            .strip_prefix("models/")
                            .map(|s| s.to_string())
                            .unwrap_or(m.name),
                        owned_by: Some("google".to_string()),
                    });
                }
            }
        }

        match resp.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => break,
        }
    }

    all_models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(all_models)
}

/// 由 baseURL 推导 Google 原生 `/v1beta/models` URL。
///
/// - `https://generativelanguage.googleapis.com` → `.../v1beta/models`
/// - `https://generativelanguage.googleapis.com/` → `.../v1beta/models`
/// - 已含 `/v1beta` 后缀 → 直接拼 `/models`
/// - 已含 `/v1beta/models` → 原样保留
fn build_gemini_native_models_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Base URL is empty".to_string());
    }
    if trimmed.ends_with("/v1beta/models") {
        return Ok(trimmed.to_string());
    }
    if trimmed.ends_with("/v1beta") {
        return Ok(format!("{trimmed}/models"));
    }
    Ok(format!("{trimmed}/v1beta/models"))
}

/// 构造「模型列表端点」的候选 URL 列表
///
/// 候选顺序：
/// 1. `models_url_override` 非空 → 只返回它
/// 2. baseURL 直接拼 `/v1/models`（若已有 `/v1` 结尾则拼 `/models`）
/// 3. 若 baseURL 命中 [`KNOWN_COMPAT_SUFFIXES`]，剥离后缀再拼 `/v1/models`
/// 4. 同上，但拼 `/models`（部分站点如 DeepSeek 官方只暴露 `/models`）
///
/// 结果已去重且保持首次出现顺序。
pub fn build_models_url_candidates(
    base_url: &str,
    is_full_url: bool,
    models_url_override: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(raw) = models_url_override {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(vec![trimmed.to_string()]);
        }
    }

    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Base URL is empty".to_string());
    }

    let mut candidates: Vec<String> = Vec::new();

    if is_full_url {
        if let Some(idx) = trimmed.find("/v1/") {
            candidates.push(format!("{}/v1/models", &trimmed[..idx]));
        } else if let Some(idx) = trimmed.rfind('/') {
            let root = &trimmed[..idx];
            if root.contains("://") && root.len() > root.find("://").unwrap() + 3 {
                candidates.push(format!("{root}/v1/models"));
            }
        }
        if candidates.is_empty() {
            return Err("Cannot derive models endpoint from full URL".to_string());
        }
        return Ok(candidates);
    }

    let primary = if trimmed.ends_with("/v1") {
        format!("{trimmed}/models")
    } else {
        format!("{trimmed}/v1/models")
    };
    candidates.push(primary);

    if let Some(stripped) = strip_compat_suffix(trimmed) {
        let root = stripped.trim_end_matches('/');
        if !root.is_empty() && root.contains("://") {
            candidates.push(format!("{root}/v1/models"));
            candidates.push(format!("{root}/models"));
        }
    }

    // 候选最多 3 条，线性去重即可，不值得上 HashSet。
    let mut unique: Vec<String> = Vec::with_capacity(candidates.len());
    for url in candidates {
        if !unique.iter().any(|u| u == &url) {
            unique.push(url);
        }
    }

    Ok(unique)
}

/// 截断响应体到 [`ERROR_BODY_MAX_CHARS`] 字符，避免 HTML 404 页占用错误串。
fn truncate_body(body: String) -> String {
    if body.chars().count() <= ERROR_BODY_MAX_CHARS {
        body
    } else {
        let mut s: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
        s.push('…');
        s
    }
}

/// 若 baseURL 以任一已知兼容子路径结尾，返回剥离后的剩余部分；否则 `None`。
///
/// 依赖 [`KNOWN_COMPAT_SUFFIXES`] 按长度降序排列，确保最长前缀优先命中
/// （否则 `/anthropic` 会提前匹配掉 `/api/anthropic` 的场景）。
fn strip_compat_suffix(base_url: &str) -> Option<&str> {
    for suffix in KNOWN_COMPAT_SUFFIXES {
        if base_url.ends_with(*suffix) {
            return Some(&base_url[..base_url.len() - suffix.len()]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidates_plain_root() {
        let c = build_models_url_candidates("https://api.siliconflow.cn", false, None).unwrap();
        assert_eq!(c, vec!["https://api.siliconflow.cn/v1/models"]);
    }

    #[test]
    fn test_candidates_trailing_slash() {
        let c = build_models_url_candidates("https://api.example.com/", false, None).unwrap();
        assert_eq!(c, vec!["https://api.example.com/v1/models"]);
    }

    #[test]
    fn test_candidates_with_v1() {
        let c = build_models_url_candidates("https://api.example.com/v1", false, None).unwrap();
        assert_eq!(c, vec!["https://api.example.com/v1/models"]);
    }

    #[test]
    fn test_candidates_full_url() {
        let c = build_models_url_candidates(
            "https://proxy.example.com/v1/chat/completions",
            true,
            None,
        )
        .unwrap();
        assert_eq!(c, vec!["https://proxy.example.com/v1/models"]);
    }

    #[test]
    fn test_candidates_empty() {
        assert!(build_models_url_candidates("", false, None).is_err());
    }

    #[test]
    fn test_candidates_override_returns_single() {
        let c = build_models_url_candidates(
            "https://api.deepseek.com/anthropic",
            false,
            Some("https://api.deepseek.com/models"),
        )
        .unwrap();
        assert_eq!(c, vec!["https://api.deepseek.com/models"]);
    }

    #[test]
    fn test_candidates_override_empty_falls_through() {
        let c =
            build_models_url_candidates("https://api.siliconflow.cn", false, Some("   ")).unwrap();
        assert_eq!(c, vec!["https://api.siliconflow.cn/v1/models"]);
    }

    #[test]
    fn test_candidates_deepseek_strip_anthropic() {
        let c =
            build_models_url_candidates("https://api.deepseek.com/anthropic", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.deepseek.com/anthropic/v1/models",
                "https://api.deepseek.com/v1/models",
                "https://api.deepseek.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_zhipu_strip_api_anthropic() {
        let c = build_models_url_candidates("https://open.bigmodel.cn/api/anthropic", false, None)
            .unwrap();
        assert_eq!(
            c,
            vec![
                "https://open.bigmodel.cn/api/anthropic/v1/models",
                "https://open.bigmodel.cn/v1/models",
                "https://open.bigmodel.cn/models",
            ]
        );
    }

    #[test]
    fn test_candidates_bailian_strip_apps_anthropic() {
        let c = build_models_url_candidates(
            "https://dashscope.aliyuncs.com/apps/anthropic",
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            c,
            vec![
                "https://dashscope.aliyuncs.com/apps/anthropic/v1/models",
                "https://dashscope.aliyuncs.com/v1/models",
                "https://dashscope.aliyuncs.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_stepfun_strip_step_plan() {
        let c =
            build_models_url_candidates("https://api.stepfun.com/step_plan", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.stepfun.com/step_plan/v1/models",
                "https://api.stepfun.com/v1/models",
                "https://api.stepfun.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_doubao_strip_api_coding() {
        let c = build_models_url_candidates(
            "https://ark.cn-beijing.volces.com/api/coding",
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            c,
            vec![
                "https://ark.cn-beijing.volces.com/api/coding/v1/models",
                "https://ark.cn-beijing.volces.com/v1/models",
                "https://ark.cn-beijing.volces.com/models",
            ]
        );
    }

    #[test]
    fn test_candidates_rightcode_strip_claude() {
        let c = build_models_url_candidates("https://www.right.codes/claude", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://www.right.codes/claude/v1/models",
                "https://www.right.codes/v1/models",
                "https://www.right.codes/models",
            ]
        );
    }

    #[test]
    fn test_candidates_longer_suffix_wins() {
        // baseURL 以 /api/anthropic 结尾时，应剥离整个 /api/anthropic，
        // 而不是只剥离 /anthropic（那样会得到残缺的 https://.../api 根）。
        let c = build_models_url_candidates("https://api.z.ai/api/anthropic", false, None).unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.z.ai/api/anthropic/v1/models",
                "https://api.z.ai/v1/models",
                "https://api.z.ai/models",
            ]
        );
    }

    #[test]
    fn test_candidates_no_suffix_no_strip() {
        let c = build_models_url_candidates("https://openrouter.ai/api", false, None).unwrap();
        assert_eq!(c, vec!["https://openrouter.ai/api/v1/models"]);
    }

    #[test]
    fn test_candidates_deduplicate() {
        // 虚构 case：baseURL 就是 "scheme://host"，剥不出子路径，应只有一个候选。
        let c = build_models_url_candidates("https://host.example.com", false, None).unwrap();
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn test_parse_response() {
        let json = r#"{"object":"list","data":[{"id":"gpt-4","object":"model","owned_by":"openai"},{"id":"claude-3-sonnet","object":"model","owned_by":"anthropic"}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].id, "gpt-4");
        assert_eq!(data[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(data[1].id, "claude-3-sonnet");
    }

    #[test]
    fn test_parse_response_no_owned_by() {
        let json = r#"{"object":"list","data":[{"id":"my-model","object":"model"}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data[0].id, "my-model");
        assert!(data[0].owned_by.is_none());
    }

    #[test]
    fn test_parse_response_empty_data() {
        let json = r#"{"object":"list","data":[]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.unwrap().is_empty());
    }

    #[test]
    fn test_is_google_native_gemini_by_base_url() {
        assert!(is_google_native_gemini(
            "https://generativelanguage.googleapis.com",
            None
        ));
        assert!(is_google_native_gemini(
            "https://generativelanguage.googleapis.com/v1beta",
            None
        ));
        assert!(!is_google_native_gemini("https://api.openai.com/v1", None));
    }

    #[test]
    fn test_is_google_native_gemini_by_models_url() {
        assert!(is_google_native_gemini(
            "https://proxy.example.com",
            Some("https://generativelanguage.googleapis.com/v1beta/models"),
        ));
    }

    #[test]
    fn test_is_google_native_gemini_rejects_host_in_path() {
        // host 在路径中而不是真正 host → 旧版 contains() 会误判，新版不应
        assert!(!is_google_native_gemini(
            "https://myproxy.com/?target=generativelanguage.googleapis.com",
            None,
        ));
        assert!(!is_google_native_gemini(
            "https://myproxy.com/generativelanguage.googleapis.com/v1beta/models",
            None,
        ));
    }

    #[test]
    fn test_build_gemini_native_models_url() {
        assert_eq!(
            build_gemini_native_models_url("https://generativelanguage.googleapis.com").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        assert_eq!(
            build_gemini_native_models_url("https://generativelanguage.googleapis.com/").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        assert_eq!(
            build_gemini_native_models_url("https://generativelanguage.googleapis.com/v1beta")
                .unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        assert_eq!(
            build_gemini_native_models_url(
                "https://generativelanguage.googleapis.com/v1beta/models"
            )
            .unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
    }

    #[test]
    fn test_parse_gemini_native_response_filters_and_strips_prefix() {
        let json = r#"{
            "models": [
                {
                    "name": "models/gemini-3.1-pro-preview",
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/gemini-2.5-flash-preview-tts",
                    "supportedGenerationMethods": ["generateContent"]
                },
                {
                    "name": "models/embedding-001",
                    "supportedGenerationMethods": ["embedContent"]
                }
            ]
        }"#;
        let resp: GeminiModelsResponse = serde_json::from_str(json).unwrap();
        let entries = resp.models.unwrap();
        // 过滤前 3 条
        assert_eq!(entries.len(), 3);
        // generateContent 命中两条
        let kept: Vec<_> = entries
            .iter()
            .filter(|m| {
                m.supported_generation_methods
                    .iter()
                    .any(|s| s == "generateContent")
            })
            .collect();
        assert_eq!(kept.len(), 2);
        // 前缀剥离逻辑校验
        let stripped: Vec<&str> = kept
            .iter()
            .map(|m| m.name.strip_prefix("models/").unwrap_or(&m.name))
            .collect();
        assert_eq!(
            stripped,
            vec!["gemini-3.1-pro-preview", "gemini-2.5-flash-preview-tts"]
        );
    }

    #[test]
    fn test_gemini_next_page_token_deserialization() {
        // 带 nextPageToken 的响应，模拟多页场景的最后一页前
        let json = r#"{
            "models": [
                {
                    "name": "models/gemini-3.1-pro-preview",
                    "supportedGenerationMethods": ["generateContent"]
                }
            ],
            "nextPageToken": "page2-token"
        }"#;
        let resp: GeminiModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.next_page_token.as_deref(),
            Some("page2-token"),
            "should capture nextPageToken"
        );
        assert_eq!(resp.models.as_ref().map(|v| v.len()), Some(1));

        // 没有 nextPageToken 的响应 → 最后一页
        let json_last = r#"{"models":[]}"#;
        let resp_last: GeminiModelsResponse = serde_json::from_str(json_last).unwrap();
        assert_eq!(resp_last.next_page_token, None, "no token = last page");
    }

    #[test]
    fn test_gemini_response_no_next_page_token_defaults_to_none() {
        // 响应没带 nextPageToken 字段 → serde(default) 应解析为 None
        let json = r#"{"models":[]}"#;
        let resp: GeminiModelsResponse = serde_json::from_str(json).unwrap();
        assert!(resp.next_page_token.is_none());
    }
}
