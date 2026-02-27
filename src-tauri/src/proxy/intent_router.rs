//! Claude CLI 意图路由模块
//!
//! 入口：从 `/v1/messages` 的请求体中读取对话内容，
//! 先做一次基于长度的粗分，再构造一个路由提示词，
//! 调用「已配置的 Claude 模型」之一作为路由模型，
//! 让它在当前 Provider 已配置的模型列表里做选择。
//!
//! 注意：
//! - 当前版本仅在 **单个 Claude Provider 的多个模型** 之间路由；
//! - 不跨 Codex / Gemini 应用路由（后续可以扩展）。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::http_client;
use crate::proxy::providers::get_adapter;
use serde_json::{json, Value};

/// 单个候选模型
#[derive(Debug, Clone)]
struct CandidateModel {
    /// 供提示词使用的友好名称（如 "Claude Haiku"）
    label: String,
    /// 实际要写回请求体的模型 id
    model_id: String,
}

/// Claude 请求意图路由器
///
/// 当前只在一个 Provider 的多个 Claude 模型之间做选择。
pub struct ClaudeIntentRouter;

impl ClaudeIntentRouter {
    /// 跨供应商意图路由：
    ///
    /// - 从数据库中读取所有 Claude 供应商（每个供应商对应一个 API Key）
    /// - 使用“路由专用供应商/模型”（若已配置）或当前供应商作为路由模型
    /// - 根据请求内容和每个供应商的描述（notes）选择最合适的供应商
    ///
    /// 返回：
    /// - `Ok(Some((providers_ordered, patched_body, chosen_model_id)))`
    ///   - `providers_ordered`：首个为选中的供应商，其余为按原顺序排序的候选（用于故障转移）
    ///   - `patched_body`：写入选中供应商主模型后的请求体
    ///   - `chosen_model_id`：选中的模型 ID
    /// - `Ok(None)`：候选不足或配置不完整，不进行跨供应商路由
    pub async fn route_across_providers(
        db: &Database,
        body: &Value,
        session_id: &str,
    ) -> Result<Option<(Vec<Provider>, Value, String)>, AppError> {
        let providers_map = db.get_all_providers("claude")?;
        if providers_map.is_empty() {
            return Ok(None);
        }

        // 会话级绑定：仅在“首轮”请求中执行跨供应商路由。
        //
        // 这里采用轻量级启发式：当 messages 中只有一条 user 消息，
        // 且没有 assistant 消息时，认为是新会话的首次请求。
        // 对于后续轮次（已经有 assistant 或多条 user/assistant 混合），
        // 不再做跨供应商路由，直接由上层使用当前供应商。
        if !Self::is_first_turn(body) {
            log::debug!(
                "[IntentRouter] Skip cross-provider routing for non-first turn (session={})",
                session_id
            );
            return Ok(None);
        }

        // 构造候选列表：每个供应商的主模型 + 描述
        let mut candidates: Vec<(Provider, String, String)> = Vec::new();
        for provider in providers_map.values() {
            if let Some(main_model) = Self::get_main_model(provider) {
                // 优先使用 meta.intent_description，其次回退到 notes
                let desc = provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.intent_description.clone())
                    .or_else(|| provider.notes.clone())
                    .unwrap_or_default();
                candidates.push((provider.clone(), main_model, desc));
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // 估算 token 长度
        let approx_tokens = Self::estimate_token_length(body);

        // 构造给路由模型看的候选文案
        let candidate_models: Vec<CandidateModel> = candidates
            .iter()
            .map(|(p, model_id, desc)| {
                let desc_trimmed = desc.trim();
                let label = if desc_trimmed.is_empty() {
                    p.name.clone()
                } else {
                    let shortened = if desc_trimmed.len() > 80 {
                        format!("{}…", &desc_trimmed[..80])
                    } else {
                        desc_trimmed.to_string()
                    };
                    format!("{}: {}", p.name, shortened)
                };
                CandidateModel {
                    label,
                    model_id: model_id.clone(),
                }
            })
            .collect();

        // 选择用于路由判断的供应商（优先使用设置中的“路由专用供应商”）
        let settings = crate::settings::get_settings();

        let router_provider = if let Some(router_id) =
            settings.claude_router_provider_id.as_ref()
        {
            match providers_map.get(router_id) {
                Some(p) => p.clone(),
                None => {
                    log::warn!(
                        "[IntentRouter] 配置的 Claude 路由供应商 {} 在数据库中不存在，将回退到当前供应商",
                        router_id
                    );
                    Self::resolve_fallback_router_provider(db, &providers_map)?
                }
            }
        } else {
            Self::resolve_fallback_router_provider(db, &providers_map)?
        };

        // 选择用于路由判断的模型（优先使用路由供应商的主模型）
        let router_model = Self::get_main_model(&router_provider)
            .unwrap_or_else(|| candidates[0].1.clone());

        // 调用路由模型选择候选索引
        let choice = match Self::call_router_model(
            &router_provider,
            &router_model,
            &candidate_models,
            approx_tokens,
            body,
        )
        .await
        {
            Ok(idx) => idx,
            Err(e) => {
                log::warn!(
                    "[IntentRouter] 跨供应商路由模型调用失败，回退到长度启发式: {e}"
                );
                Self::fallback_by_length(&candidate_models, approx_tokens)
            }
        };

        let (chosen_provider, chosen_model, _desc) = match candidates.get(choice) {
            Some(c) => c,
            None => return Ok(None),
        };

        // 构造按优先级排序的供应商列表：选中供应商在最前，其余保持原顺序
        let mut providers_ordered = Vec::with_capacity(candidates.len());
        providers_ordered.push(chosen_provider.clone());
        for (p, _, _) in &candidates {
            if p.id != chosen_provider.id {
                providers_ordered.push(p.clone());
            }
        }

        // 写回选中模型
        let mut new_body = body.clone();
        new_body["model"] = Value::String(chosen_model.clone());

        Ok(Some((providers_ordered, new_body, chosen_model.clone())))
    }

    /// 解析供应商的主模型：优先 ANTHROPIC_MODEL，其次 SONNET/HAIKU/OPUS/REASONING
    fn get_main_model(provider: &Provider) -> Option<String> {
        let env = provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())?;

        let keys = [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_REASONING_MODEL",
        ];

        for key in keys {
            if let Some(model) = env
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return Some(model.to_string());
            }
        }

        None
    }

    /// 判断当前请求是否为“首轮”对话：
    /// - messages 中只有一条 user 消息；
    /// - 且没有 assistant 消息。
    fn is_first_turn(body: &Value) -> bool {
        let messages = body
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if messages.is_empty() {
            return true;
        }

        let mut user_count = 0usize;
        let mut has_assistant = false;

        for m in &messages {
            if let Some(role) = m.get("role").and_then(|r| r.as_str()) {
                match role {
                    "user" => user_count += 1,
                    "assistant" => has_assistant = true,
                    _ => {}
                }
            }
        }

        user_count == 1 && !has_assistant
    }

    /// 回退选择路由供应商：
    /// 1. 尝试使用“当前 Claude 供应商”
    /// 2. 否则使用第一个候选供应商
    fn resolve_fallback_router_provider(
        db: &Database,
        providers: &indexmap::IndexMap<String, Provider>,
    ) -> Result<Provider, AppError> {
        if let Some(current_id) =
            crate::settings::get_effective_current_provider(db, &AppType::Claude)?
        {
            if let Some(p) = providers.get(&current_id) {
                return Ok(p.clone());
            }
        }

        providers
            .values()
            .next()
            .cloned()
            .ok_or_else(|| AppError::Message("未找到任何 Claude 供应商".to_string()))
    }

    /// 对 Claude `/v1/messages` 请求做意图路由。
    ///
    /// 返回：
    /// - `Ok(Some((patched_body, chosen_model_id)))`：已选择模型并写回
    /// - `Ok(None)`：不做修改（候选不足或路由失败）
    pub async fn route(
        provider: &Provider,
        body: &Value,
    ) -> Result<Option<(Value, String)>, AppError> {
        // 1. 从 Provider 配置中提取可用模型列表
        let candidates = Self::collect_candidates(provider);
        if candidates.len() <= 1 {
            // 候选模型不足，无需路由
            return Ok(None);
        }

        // 2. 估算请求 token 长度（简单近似）
        let approx_tokens = Self::estimate_token_length(body);

        // 3. 构造路由提示词，调用路由模型进行选择
        let router_model = Self::choose_router_model(provider, &candidates);
        let choice = match Self::call_router_model(
            provider,
            &router_model,
            &candidates,
            approx_tokens,
            body,
        )
        .await
        {
            Ok(idx) => idx,
            Err(e) => {
                log::warn!("[IntentRouter] 路由模型调用失败，回退到简单长度规则: {e}");
                Self::fallback_by_length(&candidates, approx_tokens)
            }
        };

        let chosen = match candidates.get(choice) {
            Some(c) => c,
            None => return Ok(None),
        };

        let mut new_body = body.clone();
        new_body["model"] = Value::String(chosen.model_id.clone());

        Ok(Some((new_body, chosen.model_id.clone())))
    }

    /// 从 Provider 的 env 中收集已配置的 Claude 模型列表
    fn collect_candidates(provider: &Provider) -> Vec<CandidateModel> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let env = provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object());
        let Some(env) = env else {
            return result;
        };

        // 主模型
        if let Some(model) = env
            .get("ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            if seen.insert(model.to_string()) {
                result.push(CandidateModel {
                    label: "Claude Default".to_string(),
                    model_id: model.to_string(),
                });
            }
        }

        // Haiku / Sonnet / Opus / Reasoning
        let mapping = [
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "Claude Haiku"),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", "Claude Sonnet"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "Claude Opus"),
            ("ANTHROPIC_REASONING_MODEL", "Claude Reasoning"),
        ];

        for (key, label) in mapping {
            if let Some(model) = env
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                if seen.insert(model.to_string()) {
                    result.push(CandidateModel {
                        label: label.to_string(),
                        model_id: model.to_string(),
                    });
                }
            }
        }

        result
    }

    /// 简单估算 token 长度：取最后一条 user 消息的文本长度
    fn estimate_token_length(body: &Value) -> usize {
        let messages = body
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

        let Some(msg) = last_user else {
            return 0;
        };

        // Anthropic content 可能是字符串或 block 数组
        let text = match msg.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };

        if text.is_empty() {
            return 0;
        }

        // 非严格 token 估算：字符数 / 3
        text.chars().count() / 3
    }

    /// 选择用于路由判断的模型（优先 Default/Sonnet）
    fn choose_router_model(provider: &Provider, candidates: &[CandidateModel]) -> String {
        // 优先使用默认模型
        if let Some(env) = provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
        {
            if let Some(model) = env
                .get("ANTHROPIC_MODEL")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return model.to_string();
            }

            if let Some(model) = env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return model.to_string();
            }
        }

        // 否则回退到候选列表的第一个
        candidates
            .first()
            .map(|c| c.model_id.clone())
            .unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string())
    }

    /// 调用路由模型，根据提示词选择候选模型索引
    async fn call_router_model(
        provider: &Provider,
        router_model: &str,
        candidates: &[CandidateModel],
        approx_tokens: usize,
        original_body: &Value,
    ) -> Result<usize, AppError> {
        let adapter = get_adapter(&AppType::Claude);
        let base_url = adapter
            .extract_base_url(provider)
            .map_err(|e| AppError::Message(format!("提取 Claude base_url 失败: {e}")))?;
        let url = adapter.build_url(&base_url, "/v1/messages");

        // 提取用户消息摘要（避免把整个大对话都丢给路由模型）
        let summary = Self::build_summary(original_body, 2000);

        // 构造候选列表文本
        let options_text = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}: {} ({})", i + 1, c.label, c.model_id))
            .collect::<Vec<_>>()
            .join("\n");

        let size_hint = if approx_tokens == 0 {
            "unknown"
        } else if approx_tokens <= 512 {
            "short"
        } else if approx_tokens <= 4096 {
            "medium"
        } else {
            "long"
        };

        let prompt = format!(
            "You are an expert router that chooses which LLM model to use.\n\
Estimated user request token length: ~{} tokens (category: {}).\n\
\n\
Available models:\n\
{}\n\
\n\
Given the last user request below, choose the MOST appropriate model index.\n\
You must respond with ONLY an integer between 1 and {} (no explanation):\n\
\n\
--- USER REQUEST START ---\n\
{}\n\
--- USER REQUEST END ---",
            approx_tokens,
            size_hint,
            options_text,
            candidates.len(),
            summary
        );

        let router_body = json!({
            "model": router_model,
            "max_tokens": 16,
            "stream": false,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let proxy_config = provider.meta.as_ref().and_then(|m| m.proxy_config.as_ref());
        let client = http_client::get_for_provider(proxy_config);
        let mut req = client.post(&url);

        // 添加认证头
        if let Some(auth) = adapter.extract_auth(provider) {
            req = adapter.add_auth_headers(req, &auth);
        }

        // anthropic-version：沿用 forwarder 默认值
        req = req.header("anthropic-version", "2023-06-01");
        req = req.header("accept-encoding", "identity");

        let resp = req
            .json(&router_body)
            .send()
            .await
            .map_err(|e| AppError::Message(format!("路由模型请求失败: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AppError::Message(format!(
                "路由模型返回错误状态码 {status}: {body_text}"
            )));
        }

        let value: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Message(format!("解析路由响应失败: {e}")))?;

        let text = value
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let idx: usize = text
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .map_err(|_| AppError::Message(format!("路由模型返回非法索引: '{text}'")))?;

        if idx == 0 || idx > candidates.len() {
            return Err(AppError::Message(format!(
                "路由模型索引越界: {} (候选数={})",
                idx,
                candidates.len()
            )));
        }

        Ok(idx - 1)
    }

    /// 长度回退策略：短文本倾向 Haiku，中等/长文本倾向 Sonnet/Opus/Reasoning，其次默认
    fn fallback_by_length(candidates: &[CandidateModel], approx_tokens: usize) -> usize {
        let size_hint = if approx_tokens == 0 {
            "unknown"
        } else if approx_tokens <= 512 {
            "short"
        } else if approx_tokens <= 4096 {
            "medium"
        } else {
            "long"
        };

        // 小文本优先 Haiku
        if size_hint == "short" {
            if let Some((idx, _)) = candidates
                .iter()
                .enumerate()
                .find(|(_, c)| c.label.contains("Haiku"))
            {
                return idx;
            }
        }

        // 中等/长文本优先 Reasoning / Sonnet / Opus
        if size_hint == "medium" || size_hint == "long" {
            if let Some((idx, _)) = candidates
                .iter()
                .enumerate()
                .find(|(_, c)| c.label.contains("Reasoning"))
            {
                return idx;
            }
            if let Some((idx, _)) = candidates
                .iter()
                .enumerate()
                .find(|(_, c)| c.label.contains("Sonnet"))
            {
                return idx;
            }
            if let Some((idx, _)) = candidates
                .iter()
                .enumerate()
                .find(|(_, c)| c.label.contains("Opus"))
            {
                return idx;
            }
        }

        // 兜底：第一个候选
        0
    }

    /// 根据请求体构造简要摘要，避免把整个大对话丢给路由模型
    fn build_summary(body: &Value, max_chars: usize) -> String {
        let messages = body
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

        let Some(msg) = last_user else {
            return String::new();
        };

        let text = match msg.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };

        if text.len() <= max_chars {
            text
        } else {
            format!("{}…", &text[..max_chars])
        }
    }
}
