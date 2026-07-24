//! 模型列表获取服务
//!
//! 通过 OpenAI 兼容的 GET /v1/models 端点获取供应商可用模型列表。
//! 主要面向第三方聚合站（硅基流动、OpenRouter 等），以及把 Anthropic
//! 协议挂在兼容子路径上的官方供应商（DeepSeek、Kimi、智谱 GLM 等）。

use reqwest::header::{HeaderValue, USER_AGENT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// 获取到的模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModel {
    pub id: String,
    pub owned_by: Option<String>,
    pub context_window: Option<u64>,
    pub input_modalities: Option<Vec<String>>,
    pub supports_image: Option<bool>,
}

/// 模型列表获取服务的统一入参。
///
/// 通用 OpenAI-compatible `/models` 字段和火山 Plan 管控面字段放在同一个请求对象里，
/// 让服务入口保持稳定；新增 provider 专用取模逻辑时优先扩展子结构而不是继续增加函数参数。
#[derive(Debug, Clone)]
pub struct FetchModelsRequest<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub is_full_url: bool,
    pub models_url_override: Option<&'a str>,
    pub user_agent: Option<HeaderValue>,
    pub volcengine: VolcengineModelListRequest<'a>,
}

/// 火山 Agent/Coding Plan 模型枚举的管控面参数。
///
/// 这些字段不是推理 API 的 bearer key，而是火山 OpenAPI AK/SK；没有 action 时
/// 主流程仍回到普通 OpenAI-compatible `/models` 候选逻辑。
#[derive(Debug, Clone, Copy, Default)]
pub struct VolcengineModelListRequest<'a> {
    pub action: Option<&'a str>,
    pub access_key_id: Option<&'a str>,
    pub secret_access_key: Option<&'a str>,
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
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

const FETCH_TIMEOUT_SECS: u64 = 15;

/// 智谱官方模型概览 markdown。
///
/// 智谱 `/models` 只返回账号可用模型 id，不返回上下文窗口；官方 Mintlify 文档提供
/// `.md` 形态，更适合程序解析。只在智谱域名且 `/models` 缺上下文字段时作为补充来源。
const ZHIPU_MODEL_OVERVIEW_MD_URL: &str =
    "https://docs.bigmodel.cn/cn/guide/start/model-overview.md";

/// models.dev 公共模型目录。
///
/// 原版只把它用于定价导入，但该 API 也包含 `limit.context`。这里作为通用兜底：
/// 仅当 provider 的 `api` 前缀能匹配当前成功的 `/models` endpoint 时才使用。
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";

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

const VOLCENGINE_AGENT_PLAN_MODEL_LIST_ACTION: &str = "ListArkAgentPlanModel";
const VOLCENGINE_CODING_PLAN_MODEL_LIST_ACTION: &str = "ListArkCodingPlanModel";

/// 归一化前端传入的火山模型列表 Action，只允许官方已确认的 Plan 模型枚举接口。
fn normalize_volcengine_model_list_action(
    action: Option<&str>,
) -> Result<Option<&'static str>, String> {
    let Some(action) = action.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match action {
        VOLCENGINE_AGENT_PLAN_MODEL_LIST_ACTION => {
            Ok(Some(VOLCENGINE_AGENT_PLAN_MODEL_LIST_ACTION))
        }
        VOLCENGINE_CODING_PLAN_MODEL_LIST_ACTION => {
            Ok(Some(VOLCENGINE_CODING_PLAN_MODEL_LIST_ACTION))
        }
        other => Err(format!("Unsupported Volcengine model list action: {other}")),
    }
}

/// 通过火山方舟管控面 OpenAPI 获取 Agent/Coding Plan 支持的模型列表。
///
/// 这里不使用数据面推理 API Key，也不猜 `/models`；火山官方模型枚举走
/// `open.volcengineapi.com` + AK/SK 签名，base_url 只用于提取 Region。
async fn fetch_volcengine_plan_models(
    base_url: &str,
    action: &str,
    access_key_id: Option<&str>,
    secret_access_key: Option<&str>,
) -> Result<Vec<FetchedModel>, String> {
    let access_key_id = access_key_id.unwrap_or("").trim();
    let secret_access_key = secret_access_key.unwrap_or("").trim();
    if base_url.trim().is_empty() {
        return Err("Base URL is required to derive Volcengine region".to_string());
    }
    if access_key_id.is_empty() || secret_access_key.is_empty() {
        return Err(
            "Volcengine AgentPlan model list requires AccessKey ID and SecretAccessKey".to_string(),
        );
    }

    let region = crate::services::coding_plan::volcengine_region(base_url);
    match crate::services::coding_plan::volcengine_openapi_call(
        &region,
        access_key_id,
        secret_access_key,
        action,
    )
    .await
    {
        crate::services::coding_plan::VolcCall::Body(body) => {
            let models = parse_volcengine_plan_models(&body);
            if models.is_empty() {
                Err(format!("{action} returned no models"))
            } else {
                Ok(models)
            }
        }
        crate::services::coding_plan::VolcCall::Auth(detail) => Err(detail),
        crate::services::coding_plan::VolcCall::Soft(detail) => Err(detail),
    }
}

/// 获取供应商的可用模型列表
///
/// 使用 OpenAI 兼容的 GET /v1/models 端点，按候选列表顺序尝试。
pub async fn fetch_models(options: FetchModelsRequest<'_>) -> Result<Vec<FetchedModel>, String> {
    if let Some(action) = normalize_volcengine_model_list_action(options.volcengine.action)? {
        return fetch_volcengine_plan_models(
            options.base_url,
            action,
            options.volcengine.access_key_id,
            options.volcengine.secret_access_key,
        )
        .await;
    }

    if options.api_key.is_empty() {
        return Err("API Key is required to fetch models".to_string());
    }

    let candidates = build_models_url_candidates(
        options.base_url,
        options.is_full_url,
        options.models_url_override,
    )?;
    let client = crate::proxy::http_client::get();
    let mut last_err: Option<String> = None;

    for url in &candidates {
        log::debug!("[ModelFetch] Trying endpoint: {url}");
        let mut request_builder = client
            .get(url)
            .header("Authorization", format!("Bearer {}", options.api_key))
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS));
        // 自定义 User-Agent：部分 /models 端点同样有 UA 白名单（如 Kimi Coding Plan），
        // 与转发 / 检测路径共用同一 UA，避免"代理可用但取模型失败"。
        if let Some(ua) = &options.user_agent {
            request_builder = request_builder.header(USER_AGENT, ua.clone());
        }
        let response = match request_builder.send().await {
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
                    context_window: extract_context_window(&m.extra),
                    id: m.id,
                    input_modalities: None,
                    owned_by: m.owned_by,
                    supports_image: None,
                })
                .collect();

            enrich_missing_context_windows(&client, url, &mut models).await;
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

/// 构造「模型列表端点」的候选 URL 列表
///
/// 候选顺序：
/// 1. `models_url_override` 非空 → 只返回它
/// 2. baseURL 拼 `/v1/models`；若已以版本段 `/v{N}` 结尾（`/v1`、智谱
///    `/api/coding/paas/v4` 等），版本号已在路径里，改拼 `/models`
/// 3. 版本段非 `/v1`（如 `/v4`）时再追加 `/v1/models` 作为兜底次候选
/// 4. 若 baseURL 命中 [`KNOWN_COMPAT_SUFFIXES`]，剥离后缀再拼 `/v1/models`、`/models`
///
/// 从不同供应商的 `/models` 条目中提取上下文窗口。
///
/// OpenAI-compatible 聚合器没有统一字段名，这里只接受明确的正整数，
/// 避免把价格、限流等其它数值误当作 context window。
fn extract_context_window(obj: &serde_json::Map<String, serde_json::Value>) -> Option<u64> {
    const KEYS: &[&str] = &[
        "context",
        "context_window",
        "context_length",
        "contextLength",
        "ContextWindow",
        "ContextLength",
        "max_context_window",
        "max_context_length",
        "contextWindow",
        "maxContextWindow",
        "maxContextLength",
        "max_model_len",
        "maxModelLen",
        "max_input_tokens",
        "maxInputTokens",
        "max_prompt_tokens",
        "maxPromptTokens",
        "input_token_limit",
        "inputTokenLimit",
    ];

    let direct = KEYS
        .iter()
        .filter_map(|key| obj.get(*key))
        .find_map(parse_positive_u64);
    if direct.is_some() {
        return direct;
    }

    ["limit", "limits", "capabilities", "metadata"]
        .iter()
        .filter_map(|key| obj.get(*key).and_then(|value| value.as_object()))
        .find_map(extract_context_window)
}

/// 解析火山 `ListArkAgentPlanModel` / `ListArkCodingPlanModel` 的模型列表。
///
/// 官方文档示例是 `Result.Datas[].ModelID`；这里额外兼容少量常见字段名，
/// 以便 OpenAPI 后续扩展字段时不破坏已有模型列表刷新。
fn parse_volcengine_plan_models(body: &serde_json::Value) -> Vec<FetchedModel> {
    let candidates = [
        body.pointer("/Result/Datas"),
        body.pointer("/Result/Models"),
        body.pointer("/Result/ModelList"),
        body.pointer("/Result/Items"),
        body.get("Result"),
        body.get("Datas"),
        Some(body),
    ];
    let Some(entries) = candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_array())
    else {
        return Vec::new();
    };

    let mut models = entries
        .iter()
        .filter_map(parse_volcengine_plan_model_entry)
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

/// 解析火山模型列表中的单个模型条目。
fn parse_volcengine_plan_model_entry(entry: &serde_json::Value) -> Option<FetchedModel> {
    if let Some(model_id) = entry
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(FetchedModel {
            id: model_id.to_string(),
            owned_by: Some("volcengine".to_string()),
            context_window: None,
            input_modalities: None,
            supports_image: None,
        });
    }

    let obj = entry.as_object()?;
    let id = [
        "ModelID",
        "ModelId",
        "modelID",
        "modelId",
        "ModelName",
        "Model",
        "Id",
        "id",
    ]
    .iter()
    .filter_map(|key| obj.get(*key).and_then(|value| value.as_str()))
    .map(str::trim)
    .find(|value| !value.is_empty())?;

    Some(FetchedModel {
        id: id.to_string(),
        owned_by: Some("volcengine".to_string()),
        context_window: extract_context_window(obj),
        input_modalities: None,
        supports_image: None,
    })
}

/// 为缺失上下文窗口的模型执行分层补齐。
///
/// 顺序保持保守：`/models` 显式 metadata 已在调用前解析完成；这里仅对仍为空的模型
/// 先尝试 provider 官方来源，再尝试能按 API 前缀匹配的公共目录。
async fn enrich_missing_context_windows(
    client: &reqwest::Client,
    endpoint_url: &str,
    models: &mut [FetchedModel],
) {
    if !models.iter().any(|model| model.context_window.is_none()) {
        return;
    }

    enrich_zhipu_context_windows(client, endpoint_url, models).await;
    enrich_models_dev_context_windows(client, endpoint_url, models).await;
}

/// 为智谱模型补齐官方文档里的上下文窗口。
///
/// 智谱 Coding Plan 的 `/models` 响应只证明账号可用模型集合，模型规格需要从官方文档取。
/// 这里只填充缺失值，不覆盖上游未来可能直接返回的真实 metadata。
async fn enrich_zhipu_context_windows(
    client: &reqwest::Client,
    endpoint_url: &str,
    models: &mut [FetchedModel],
) {
    if !is_zhipu_models_endpoint(endpoint_url)
        || !models
            .iter()
            .any(|model| model.context_window.is_none() && is_glm_model_id(&model.id))
    {
        return;
    }

    let mut contexts = fetch_zhipu_model_overview_contexts(client).await;
    enrich_missing_zhipu_detail_contexts(client, models, &mut contexts).await;

    apply_missing_context_windows(models, |model_id| {
        let key = normalize_zhipu_model_id(model_id);
        contexts.get(&key).copied()
    });
}

/// 通过 models.dev 的 `limit.context` 补齐上下文窗口。
///
/// 只在 endpoint 与 provider.api 前缀匹配时使用，避免跨供应商同名模型误配。该来源是
/// 公共目录，因此优先级低于上游 `/models` 显式字段和 provider 官方文档。
async fn enrich_models_dev_context_windows(
    client: &reqwest::Client,
    endpoint_url: &str,
    models: &mut [FetchedModel],
) {
    if !models.iter().any(|model| model.context_window.is_none()) {
        return;
    }

    let Some(catalog) = fetch_models_dev_catalog(client).await else {
        return;
    };
    let Some(provider_models) = find_models_dev_provider_models(&catalog, endpoint_url) else {
        return;
    };

    apply_missing_context_windows(models, |model_id| {
        lookup_models_dev_context(provider_models, model_id)
    });
}

/// 将外部来源查到的上下文窗口应用到模型列表。
///
/// 这是所有补齐来源共享的不变量：只填充 `/models` 未返回上下文的条目，永远不覆盖
/// 上游已经明确给出的真实 metadata。
fn apply_missing_context_windows<F>(models: &mut [FetchedModel], mut lookup: F)
where
    F: FnMut(&str) -> Option<u64>,
{
    for model in models.iter_mut() {
        if model.context_window.is_none() {
            model.context_window = lookup(&model.id);
        }
    }
}

/// 拉取 models.dev 目录；失败时返回 `None`，不影响 `/models` 主流程。
async fn fetch_models_dev_catalog(client: &reqwest::Client) -> Option<serde_json::Value> {
    let response = client
        .get(MODELS_DEV_API_URL)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<serde_json::Value>().await.ok()
}

/// 从 models.dev 目录中找到与当前 endpoint 匹配的 provider models 对象。
fn find_models_dev_provider_models<'a>(
    catalog: &'a serde_json::Value,
    endpoint_url: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    catalog
        .as_object()?
        .values()
        .filter_map(|provider| provider.as_object())
        .find(|provider| {
            provider
                .get("api")
                .and_then(|value| value.as_str())
                .is_some_and(|api| endpoint_matches_provider_api(endpoint_url, api))
        })
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.as_object())
}

/// 判断 `/models` endpoint 是否属于 models.dev provider.api。
fn endpoint_matches_provider_api(endpoint_url: &str, provider_api: &str) -> bool {
    let endpoint = normalize_url_prefix(endpoint_url);
    let api = normalize_url_prefix(provider_api);
    !endpoint.is_empty() && !api.is_empty() && endpoint.starts_with(&api)
}

/// 归一化 URL 前缀：大小写、尾斜杠和最终 `/models` 不影响 provider 匹配。
fn normalize_url_prefix(value: &str) -> String {
    let mut normalized = value.trim().trim_end_matches('/').to_ascii_lowercase();
    if let Some(stripped) = normalized.strip_suffix("/models") {
        normalized = stripped.to_string();
    }
    normalized
}

/// 在已匹配 provider 的 models.dev 模型表里查找 context window。
fn lookup_models_dev_context(
    provider_models: &serde_json::Map<String, serde_json::Value>,
    model_id: &str,
) -> Option<u64> {
    let fetched = normalize_models_dev_model_id(model_id);
    let mut suffix_matches = Vec::new();

    for (key, value) in provider_models {
        let Some(model_obj) = value.as_object() else {
            continue;
        };
        let catalog_id = model_obj.get("id").and_then(|id| id.as_str());
        let Some(context) = extract_models_dev_entry_context(model_obj) else {
            continue;
        };
        if models_dev_model_id_matches_exact(&fetched, key, catalog_id) {
            return Some(context);
        }
        if models_dev_model_id_matches_suffix(&fetched, key, catalog_id) {
            suffix_matches.push(context);
        }
    }

    if suffix_matches.len() == 1 {
        suffix_matches.into_iter().next()
    } else {
        None
    }
}

/// 从 models.dev 单个模型条目中提取正数 `limit.context`。
fn extract_models_dev_entry_context(
    model_obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<u64> {
    model_obj
        .get("limit")
        .and_then(|limit| limit.as_object())
        .and_then(|limit| limit.get("context"))
        .and_then(parse_positive_u64)
        .filter(|context| *context > 0)
}

/// 判断 `/models` 返回的模型 id 是否精确匹配 models.dev 的 key 或 `id` 字段。
fn models_dev_model_id_matches_exact(
    fetched_model_id: &str,
    catalog_key: &str,
    catalog_id: Option<&str>,
) -> bool {
    let key = normalize_models_dev_model_id(catalog_key);
    let id = catalog_id.map(normalize_models_dev_model_id);

    fetched_model_id == key || id.as_deref() == Some(fetched_model_id)
}

/// 判断 `/models` 返回的模型 id 是否唯一后缀匹配 models.dev 的 key 或 `id` 字段。
fn models_dev_model_id_matches_suffix(
    fetched_model_id: &str,
    catalog_key: &str,
    catalog_id: Option<&str>,
) -> bool {
    let key = normalize_models_dev_model_id(catalog_key);
    let id = catalog_id.map(normalize_models_dev_model_id);

    key.rsplit('/').next() == Some(fetched_model_id)
        || id.as_deref().and_then(|value| value.rsplit('/').next()) == Some(fetched_model_id)
}

/// 归一化 models.dev 模型 id，去掉大小写、`:free` 等后缀和 `[1m]` 标记差异。
fn normalize_models_dev_model_id(value: &str) -> String {
    let mut normalized = value.trim().to_ascii_lowercase();
    if let Some(before_colon) = normalized.split(':').next() {
        normalized = before_colon.to_string();
    }
    if normalized.ends_with("[1m]") {
        normalized = normalized[..normalized.len() - "[1m]".len()]
            .trim()
            .to_string();
    }
    normalized
}

/// 判断当前成功的 `/models` endpoint 是否属于智谱/Z.AI。
fn is_zhipu_models_endpoint(endpoint_url: &str) -> bool {
    let lower = endpoint_url.to_ascii_lowercase();
    lower.contains("open.bigmodel.cn") || lower.contains("api.z.ai")
}

/// 判断模型 id 是否是 GLM 文本/视觉模型。
fn is_glm_model_id(model_id: &str) -> bool {
    let normalized = normalize_zhipu_model_id(model_id);
    normalized.starts_with("glm-") || normalized.contains("/glm-")
}

/// 拉取智谱模型概览表，并解析模型上下文列。
///
/// 网络或文档格式异常只会导致补充 metadata 缺失，不阻断 `/models` 本身成功返回。
async fn fetch_zhipu_model_overview_contexts(client: &reqwest::Client) -> HashMap<String, u64> {
    let text = match client
        .get(ZHIPU_MODEL_OVERVIEW_MD_URL)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response.text().await.ok(),
        _ => None,
    };

    text.as_deref()
        .map(parse_zhipu_model_overview_contexts)
        .unwrap_or_default()
}

/// 对概览表没覆盖的 GLM 模型，按官方单模型 markdown 页面补查上下文。
///
/// 例如 `glm-4.5` 的规格在 `glm-4.5.md` 概览卡片中，而概览表当前只列出
/// `GLM-4.5-Air`；补查可以让真实 `/models` 返回的 `glm-4.5` 也得到 128K。
async fn enrich_missing_zhipu_detail_contexts(
    client: &reqwest::Client,
    models: &[FetchedModel],
    contexts: &mut HashMap<String, u64>,
) {
    for model in models {
        if model.context_window.is_some() {
            continue;
        }
        let key = normalize_zhipu_model_id(&model.id);
        if contexts.contains_key(&key) {
            continue;
        }
        let Some(slug) = zhipu_docs_slug_for_model_id(&model.id) else {
            continue;
        };
        if let Some(context) = fetch_zhipu_detail_context(client, &slug).await {
            contexts.insert(key, context);
        }
    }
}

/// 将 GLM 模型 id 映射到智谱文档 markdown slug。
fn zhipu_docs_slug_for_model_id(model_id: &str) -> Option<String> {
    let normalized = normalize_zhipu_model_id(model_id);
    if !normalized.starts_with("glm-") {
        return None;
    }
    if normalized.starts_with("glm-4.5") {
        return Some("glm-4.5".to_string());
    }
    Some(normalized)
}

/// 拉取单模型 markdown 页面，并解析“上下文窗口”卡片。
async fn fetch_zhipu_detail_context(client: &reqwest::Client, slug: &str) -> Option<u64> {
    let url = format!("https://docs.bigmodel.cn/cn/guide/models/text/{slug}.md");
    let response = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.text().await.ok()?;
    parse_zhipu_detail_context(&text)
}

/// 解析智谱模型概览 markdown 表格中的“模型 / 上下文”列。
fn parse_zhipu_model_overview_contexts(markdown: &str) -> HashMap<String, u64> {
    let mut contexts = HashMap::new();
    let mut pending_row = String::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if pending_row.is_empty() {
            if !trimmed.starts_with("| [") {
                continue;
            }
            pending_row.push_str(trimmed);
        } else {
            pending_row.push('\n');
            pending_row.push_str(trimmed);
        }

        if pending_row.matches('|').count() < 5 || !trimmed.ends_with('|') {
            continue;
        }

        if let Some((model, context)) = parse_zhipu_overview_table_row(&pending_row) {
            contexts.insert(model, context);
        }
        pending_row.clear();
    }

    contexts
}

/// 解析单行智谱模型表格。
fn parse_zhipu_overview_table_row(row: &str) -> Option<(String, u64)> {
    let columns = row
        .split('|')
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .collect::<Vec<_>>();
    if columns.len() < 4 {
        return None;
    }

    let model = extract_markdown_link_label(columns[0]).unwrap_or(columns[0]);
    let context = parse_context_tokens(columns[2])?;
    Some((normalize_zhipu_model_id(model), context))
}

/// 从 markdown 链接 `[label](url)` 中取 label；非链接返回 `None`。
fn extract_markdown_link_label(value: &str) -> Option<&str> {
    let start = value.find('[')? + 1;
    let end = value[start..].find(']')? + start;
    Some(&value[start..end])
}

/// 解析单模型页概览卡片中的“上下文窗口”值。
fn parse_zhipu_detail_context(markdown: &str) -> Option<u64> {
    let mut seen_context_card = false;
    let mut waiting_for_value = false;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.contains("title=\"上下文窗口\"") {
            seen_context_card = true;
            waiting_for_value = trimmed.contains("}>");
            continue;
        }
        if seen_context_card && trimmed.contains("}>") {
            waiting_for_value = true;
            continue;
        }
        if waiting_for_value
            && !trimmed.is_empty()
            && !trimmed.starts_with('<')
            && !trimmed.starts_with('}')
        {
            return parse_context_tokens(trimmed);
        }
    }

    None
}

/// 统一模型 id：大小写、空格、`ZhipuAI/` 命名空间和 `[1m]` 后缀都不影响匹配。
fn normalize_zhipu_model_id(value: &str) -> String {
    let mut normalized = value.trim().to_ascii_lowercase();
    if let Some(stripped) = normalized.strip_prefix("zhipuai/") {
        normalized = stripped.to_string();
    }
    normalized = normalized.replace(' ', "-");
    normalized = normalized.replace("[1m]", "");
    normalized = normalized.replace("（即将下线）", "");
    normalized.trim().to_string()
}

/// 将智谱文档里的 `1M`、`200K`、纯数字解析为 token 数。
fn parse_context_tokens(value: &str) -> Option<u64> {
    let compact = value
        .trim()
        .trim_matches('`')
        .replace([',', ' '], "")
        .to_ascii_lowercase();
    if compact.is_empty() || compact.contains('：') || compact.contains("≤") {
        return None;
    }
    if let Some(number) = compact.strip_suffix('m') {
        return number.parse::<u64>().ok().map(|value| value * 1_000_000);
    }
    if let Some(number) = compact.strip_suffix('k') {
        return number.parse::<u64>().ok().map(|value| value * 1_000);
    }
    compact.parse::<u64>().ok().filter(|value| *value > 0)
}

/// 将 JSON 数字或纯数字字符串解析为正整数。
///
/// 浮点数、负数、空字符串和带单位的文本都会被忽略，调用方保留原有兜底逻辑。
fn parse_positive_u64(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64().filter(|v| *v > 0),
        serde_json::Value::String(text) => {
            text.trim().parse::<u64>().ok().filter(|value| *value > 0)
        }
        _ => None,
    }
}

/// 构造「模型列表端点」的候选 URL 列表。
///
/// 候选顺序：
/// 1. `models_url_override` 非空 → 只返回它
/// 2. baseURL 拼 `/v1/models`；若已以版本段 `/v{N}` 结尾（`/v1`、智谱
///    `/api/coding/paas/v4` 等），版本号已在路径里，改拼 `/models`
/// 3. 版本段非 `/v1`（如 `/v4`）时再追加 `/v1/models` 作为兜底次候选
/// 4. 若 baseURL 命中 [`KNOWN_COMPAT_SUFFIXES`]，剥离后缀再拼 `/v1/models`、`/models`
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

    // baseURL 已以版本段 /v{N} 结尾时（如 `/v1`、智谱 `/api/coding/paas/v4`），
    // OpenAI 惯例的模型端点是 `{base}/models`，不能再补 `/v1`
    // （否则 .../coding/paas/v4/v1/models → 404）。
    if ends_with_version_segment(trimmed) {
        candidates.push(format!("{trimmed}/models"));
        // 版本段非 /v1 时，保留旧的 /v1/models 作为兜底次候选（正确路径已在前）。
        if !trimmed.ends_with("/v1") {
            candidates.push(format!("{trimmed}/v1/models"));
        }
    } else {
        candidates.push(format!("{trimmed}/v1/models"));
    }

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

/// 判断 baseURL 是否以 OpenAI 风格的版本段 `/v{N}` 结尾（`N` 为一个或多个数字），
/// 例如 `/v1`、`.../paas/v4`。这类 URL 版本号已在路径中，模型端点应为
/// `{base}/models`，不能再补 `/v1`（智谱 Coding Plan 即 `.../coding/paas/v4`）。
fn ends_with_version_segment(url: &str) -> bool {
    let last = url.rsplit('/').next().unwrap_or("");
    last.strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
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
    fn test_candidates_zhipu_coding_paas_v4() {
        // 智谱 Coding Plan 端点以 /v4 版本段结尾：模型端点是 {base}/models，
        // 正确路径必须排在 .../v4/v1/models（404）之前。
        let c =
            build_models_url_candidates("https://open.bigmodel.cn/api/coding/paas/v4", false, None)
                .unwrap();
        assert_eq!(
            c,
            vec![
                "https://open.bigmodel.cn/api/coding/paas/v4/models",
                "https://open.bigmodel.cn/api/coding/paas/v4/v1/models",
            ]
        );
    }

    #[test]
    fn test_candidates_zai_coding_paas_v4() {
        let c = build_models_url_candidates("https://api.z.ai/api/coding/paas/v4", false, None)
            .unwrap();
        assert_eq!(
            c,
            vec![
                "https://api.z.ai/api/coding/paas/v4/models",
                "https://api.z.ai/api/coding/paas/v4/v1/models",
            ]
        );
    }

    #[test]
    fn test_ends_with_version_segment() {
        assert!(ends_with_version_segment("https://x.com/v1"));
        assert!(ends_with_version_segment(
            "https://open.bigmodel.cn/api/coding/paas/v4"
        ));
        assert!(ends_with_version_segment("https://x.com/v10"));
        assert!(!ends_with_version_segment("https://x.com/api"));
        assert!(!ends_with_version_segment("https://x.com/vX"));
        assert!(!ends_with_version_segment("https://x.com/models"));
        assert!(!ends_with_version_segment("https://api.siliconflow.cn"));
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
    fn test_parse_response_extracts_context_window() {
        let json = r#"{"object":"list","data":[{"id":"model-a","context_window":262144},{"id":"model-b","maxContextWindow":"1000000"},{"id":"model-c","max_model_len":262144},{"id":"model-d","maxModelLen":"131072"},{"id":"model-e","contextWindow":"128000 tokens"},{"id":"model-f","context_length":200000},{"id":"model-g","max_context_length":"204800"},{"id":"model-h","max_input_tokens":1000000},{"id":"model-i","limit":{"context":262144}},{"id":"model-j","limits":{"context_window":"32768"}},{"id":"model-k","metadata":{"maxContextLength":65536}}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        let data = resp
            .data
            .unwrap()
            .into_iter()
            .map(|entry| FetchedModel {
                context_window: extract_context_window(&entry.extra),
                id: entry.id,
                input_modalities: None,
                owned_by: entry.owned_by,
                supports_image: None,
            })
            .collect::<Vec<_>>();

        assert_eq!(data[0].context_window, Some(262_144));
        assert_eq!(data[1].context_window, Some(1_000_000));
        assert_eq!(data[2].context_window, Some(262_144));
        assert_eq!(data[3].context_window, Some(131_072));
        assert_eq!(data[4].context_window, None);
        assert_eq!(data[5].context_window, Some(200_000));
        assert_eq!(data[6].context_window, Some(204_800));
        assert_eq!(data[7].context_window, Some(1_000_000));
        assert_eq!(data[8].context_window, Some(262_144));
        assert_eq!(data[9].context_window, Some(32_768));
        assert_eq!(data[10].context_window, Some(65_536));
    }

    #[test]
    fn test_parse_volcengine_plan_models_from_official_agentplan_shape() {
        let body = serde_json::json!({
            "ResponseMetadata": {
                "Action": "ListArkAgentPlanModel",
                "Version": "2024-01-01",
                "Service": "ark",
                "Region": "cn-beijing"
            },
            "Result": {
                "Datas": [
                    { "ModelID": "doubao-seed-1.6" },
                    { "ModelID": "ark-code-latest", "ContextWindow": 262144 }
                ]
            }
        });

        let models = parse_volcengine_plan_models(&body);

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "ark-code-latest");
        assert_eq!(models[0].owned_by.as_deref(), Some("volcengine"));
        assert_eq!(models[0].context_window, Some(262_144));
        assert_eq!(models[1].id, "doubao-seed-1.6");
    }

    #[test]
    fn test_parse_volcengine_plan_models_accepts_string_entries_and_deduplicates() {
        let body = serde_json::json!({
            "Result": {
                "Datas": [
                    "ark-code-latest",
                    { "ModelId": "ark-code-latest" },
                    { "modelId": "doubao-seed-1.6" }
                ]
            }
        });

        let models = parse_volcengine_plan_models(&body);

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ark-code-latest", "doubao-seed-1.6"]
        );
    }

    #[test]
    fn test_normalize_volcengine_model_list_action_accepts_only_plan_actions() {
        assert_eq!(
            normalize_volcengine_model_list_action(Some("ListArkAgentPlanModel")).unwrap(),
            Some("ListArkAgentPlanModel")
        );
        assert_eq!(
            normalize_volcengine_model_list_action(Some("ListArkCodingPlanModel")).unwrap(),
            Some("ListArkCodingPlanModel")
        );
        assert!(normalize_volcengine_model_list_action(Some("GetAFPUsage")).is_err());
        assert_eq!(normalize_volcengine_model_list_action(None).unwrap(), None);
    }

    #[test]
    fn test_parse_zhipu_model_overview_contexts() {
        let markdown = r#"
| 模型 | 特点 | 上下文 | 最大输出 |
| :--- | :--- | :--- | :--- |
| [GLM-5.2](/cn/guide/models/text/glm-5.2) | 1M 上下文，支撑复杂长程任务稳定执行
Coding 能力开源 SOTA，从代码生成走向工程交付 | 1M | 128K |
| [GLM-5.1](/cn/guide/models/text/glm-5.1) | 长程任务显著提升 | 200K | 128K |
| [GLM-OCR](/cn/guide/models/vlm/glm-ocr) | 文档理解 | 输入：单图 ≤ 10 MB | |
"#;
        let contexts = parse_zhipu_model_overview_contexts(markdown);

        assert_eq!(contexts.get("glm-5.2"), Some(&1_000_000));
        assert_eq!(contexts.get("glm-5.1"), Some(&200_000));
        assert!(!contexts.contains_key("glm-ocr"));
    }

    #[test]
    fn test_parse_zhipu_model_overview_contexts_skips_unparseable_rows() {
        let markdown = r#"
| 模型 | 特点 | 上下文 | 最大输出 |
| :--- | :--- | :--- | :--- |
| [GLM-4.6（即将下线）](/cn/guide/models/text/glm-4.6) | 老版本 | 200K | 128K |
| [GLM-OCR](/cn/guide/models/vlm/glm-ocr) | 文档理解 | 输入：单图 ≤ 10 MB | |
| [GLM-Pending](/cn/guide/models/text/glm-pending) | 待发布 | 即将上线 | |
"#;
        let contexts = parse_zhipu_model_overview_contexts(markdown);

        assert_eq!(contexts.get("glm-4.6"), Some(&200_000));
        assert!(!contexts.contains_key("glm-ocr"));
        assert!(!contexts.contains_key("glm-pending"));
        assert!(parse_zhipu_model_overview_contexts("").is_empty());
        assert!(
            parse_zhipu_model_overview_contexts("| 模型 | 特点 | 上下文 | 最大输出 |").is_empty()
        );
    }

    #[test]
    fn test_parse_zhipu_detail_context() {
        let markdown = r#"
<Card title="上下文窗口" icon={<svg />}>
  128K
</Card>
"#;

        assert_eq!(parse_zhipu_detail_context(markdown), Some(128_000));
    }

    #[test]
    fn test_parse_zhipu_detail_context_ignores_other_cards() {
        let markdown = r#"
<Card title="最大输出" icon={<svg />}>
  128K
</Card>
<Card title="上下文窗口" icon={<svg />}>
  1M
</Card>
"#;

        assert_eq!(parse_zhipu_detail_context(markdown), Some(1_000_000));
        assert_eq!(
            parse_zhipu_detail_context("<Card title=\"最大输出\">128K</Card>"),
            None
        );
    }

    #[test]
    fn test_models_dev_endpoint_matches_provider_api() {
        assert!(endpoint_matches_provider_api(
            "https://router.requesty.ai/v1/models",
            "https://router.requesty.ai/v1"
        ));
        assert!(endpoint_matches_provider_api(
            "https://openrouter.ai/api/v1/models",
            "https://openrouter.ai/api/v1/"
        ));
        assert!(!endpoint_matches_provider_api(
            "https://api.other.example/v1/models",
            "https://openrouter.ai/api/v1"
        ));
    }

    #[test]
    fn test_lookup_models_dev_context_matches_exact_and_suffix_ids() {
        let provider_models = serde_json::json!({
            "openai/gpt-5.2-chat": {
                "id": "openai/gpt-5.2-chat",
                "limit": { "context": 128000 }
            },
            "xai/grok-4-fast": {
                "id": "xai/grok-4-fast",
                "limit": { "context": 2000000 }
            },
            "image-model": {
                "id": "image-model",
                "limit": { "context": 0 }
            }
        });
        let models = provider_models.as_object().unwrap();

        assert_eq!(
            lookup_models_dev_context(models, "gpt-5.2-chat"),
            Some(128_000)
        );
        assert_eq!(
            lookup_models_dev_context(models, "xai/grok-4-fast:free"),
            Some(2_000_000)
        );
        assert_eq!(lookup_models_dev_context(models, "image-model"), None);
        assert_eq!(lookup_models_dev_context(models, "missing-model"), None);
    }

    #[test]
    fn test_lookup_models_dev_context_rejects_ambiguous_suffix_ids() {
        let provider_models = serde_json::json!({
            "provider-a/shared-model": {
                "id": "provider-a/shared-model",
                "limit": { "context": 100000 }
            },
            "provider-b/shared-model": {
                "id": "provider-b/shared-model",
                "limit": { "context": 200000 }
            }
        });
        let models = provider_models.as_object().unwrap();

        assert_eq!(lookup_models_dev_context(models, "shared-model"), None);
        assert_eq!(
            lookup_models_dev_context(models, "provider-b/shared-model"),
            Some(200_000)
        );
    }

    #[test]
    fn test_find_models_dev_provider_models_uses_api_prefix() {
        let catalog = serde_json::json!({
            "requesty": {
                "api": "https://router.requesty.ai/v1",
                "models": {
                    "xai/grok-4-fast": { "limit": { "context": 2000000 } }
                }
            },
            "other": {
                "api": "https://api.other.example/v1",
                "models": {
                    "xai/grok-4-fast": { "limit": { "context": 1 } }
                }
            }
        });

        let models =
            find_models_dev_provider_models(&catalog, "https://router.requesty.ai/v1/models")
                .unwrap();
        assert_eq!(
            lookup_models_dev_context(models, "xai/grok-4-fast"),
            Some(2_000_000)
        );
    }

    #[test]
    fn test_apply_missing_context_windows_preserves_explicit_model_metadata() {
        let mut models = vec![
            FetchedModel {
                id: "glm-5.2".to_string(),
                owned_by: Some("zhipu".to_string()),
                context_window: Some(123_456),
                input_modalities: None,
                supports_image: None,
            },
            FetchedModel {
                id: "glm-5.1".to_string(),
                owned_by: Some("zhipu".to_string()),
                context_window: None,
                input_modalities: None,
                supports_image: None,
            },
        ];

        apply_missing_context_windows(&mut models, |model_id| match model_id {
            "glm-5.2" => Some(1_000_000),
            "glm-5.1" => Some(200_000),
            _ => None,
        });

        assert_eq!(models[0].context_window, Some(123_456));
        assert_eq!(models[1].context_window, Some(200_000));
    }

    #[test]
    fn test_zhipu_endpoint_and_glm_model_detection_boundaries() {
        assert!(is_zhipu_models_endpoint(
            "https://open.bigmodel.cn/api/coding/paas/v4/models"
        ));
        assert!(is_zhipu_models_endpoint(
            "https://OPEN.BIGMODEL.CN/api/coding/paas/v4/models"
        ));
        assert!(is_zhipu_models_endpoint("https://api.z.ai/api/v1/models"));
        assert!(!is_zhipu_models_endpoint(
            "https://api.deepseek.com/v1/models"
        ));
        assert!(!is_zhipu_models_endpoint(""));

        assert!(is_glm_model_id("glm-5.2"));
        assert!(is_glm_model_id("GLM-5.2"));
        assert!(is_glm_model_id("ZhipuAI/glm-5.2[1m]"));
        assert!(!is_glm_model_id("deepseek-chat"));
        assert!(!is_glm_model_id(""));
    }

    #[test]
    fn test_zhipu_model_id_normalization_and_slug() {
        assert_eq!(normalize_zhipu_model_id("ZhipuAI/GLM-5.2[1m]"), "glm-5.2");
        assert_eq!(
            zhipu_docs_slug_for_model_id("glm-4.5-air").as_deref(),
            Some("glm-4.5")
        );
        assert_eq!(
            zhipu_docs_slug_for_model_id("glm-5-turbo").as_deref(),
            Some("glm-5-turbo")
        );
    }

    #[test]
    fn test_parse_response_empty_data() {
        let json = r#"{"object":"list","data":[]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.unwrap().is_empty());
    }
}
