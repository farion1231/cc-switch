use crate::cursor::types::{
    CursorModelConfig, SidecarConfig, SidecarHomeMetricsConfig, SidecarModelAdapter,
    SidecarRoutingConfig,
};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use indexmap::IndexMap;

const CURSOR_APP_TYPE: &str = "cursor";

pub fn project_enabled_models(db: &Database) -> Result<SidecarConfig, AppError> {
    project_enabled_provider_map(&db.get_all_providers(CURSOR_APP_TYPE)?)
}

pub fn project_provider_changes(
    db: &Database,
    upserts: &[Provider],
    deleted_provider_ids: &[String],
) -> Result<SidecarConfig, AppError> {
    let mut providers = db.get_all_providers(CURSOR_APP_TYPE)?;
    for id in deleted_provider_ids {
        providers.shift_remove(id);
    }
    for provider in upserts {
        providers.insert(provider.id.clone(), provider.clone());
    }
    project_enabled_provider_map(&providers)
}

pub fn project_without_endpoint(
    db: &Database,
    endpoint_id: &str,
) -> Result<SidecarConfig, AppError> {
    let providers = db
        .get_all_providers(CURSOR_APP_TYPE)?
        .into_iter()
        .filter_map(|(id, provider)| {
            let config =
                serde_json::from_value::<CursorModelConfig>(provider.settings_config.clone());
            match config {
                Ok(config) if config.endpoint_id == endpoint_id => None,
                _ => Some((id, provider)),
            }
        })
        .collect();
    project_enabled_provider_map(&providers)
}

pub fn project_enabled_provider_map(
    providers: &IndexMap<String, Provider>,
) -> Result<SidecarConfig, AppError> {
    let mut adapters = Vec::new();
    for provider in providers.values() {
        let config: CursorModelConfig = serde_json::from_value(provider.settings_config.clone())
            .map_err(|error| {
                AppError::Config(format!(
                    "Cursor Provider '{}' 配置无效: {error}",
                    provider.name
                ))
            })?;
        if !config.enabled {
            continue;
        }
        validate_model(&provider.id, &provider.name, &config)?;
        adapters.push(SidecarModelAdapter {
            source_provider_id: provider.id.clone(),
            source_provider_name: provider.name.clone(),
            display_name: provider.name.clone(),
            provider_type: config.provider_type,
            base_url: config.base_url,
            api_key: config.api_key,
            tooltip_data: config.tooltip_data,
            model_id: config.model_id,
            pricing_model: config.pricing_model,
            reasoning_effort: config.reasoning_effort,
            open_ai_endpoint: config.open_ai_endpoint,
            open_ai_extra_params_enabled: config.open_ai_extra_params_enabled,
            open_ai_extra_params_json: config.open_ai_extra_params_json,
            custom_headers_enabled: config.custom_headers_enabled,
            custom_headers_json: config.custom_headers_json,
            anthropic_extra_params_enabled: config.anthropic_extra_params_enabled,
            anthropic_extra_params_json: config.anthropic_extra_params_json,
            context_window_tokens: config.context_window_tokens,
            max_completion_tokens: config.max_completion_tokens,
            anthropic_max_tokens: config.anthropic_max_tokens,
            anthropic_thinking_effort: config.anthropic_thinking_effort,
            thinking_budget_tokens: config.thinking_budget_tokens,
        });
    }

    Ok(SidecarConfig {
        log: true,
        provider_stream_idle_timeout: 240,
        backend_listen_addr: "127.0.0.1:18090".to_string(),
        proxy_listen_addr: "127.0.0.1:18080".to_string(),
        model_adapters: adapters,
        routing: SidecarRoutingConfig {
            mode: "local".to_string(),
        },
        home_metrics: SidecarHomeMetricsConfig::default(),
        last_agent_model_hash: String::new(),
    })
}

pub fn project_single_model(
    provider_id: &str,
    provider_name: &str,
    config: CursorModelConfig,
) -> Result<SidecarModelAdapter, AppError> {
    validate_model(provider_id, provider_name, &config)?;
    Ok(SidecarModelAdapter {
        source_provider_id: provider_id.to_string(),
        source_provider_name: provider_name.to_string(),
        display_name: provider_name.to_string(),
        provider_type: config.provider_type,
        base_url: config.base_url,
        api_key: config.api_key,
        tooltip_data: config.tooltip_data,
        model_id: config.model_id,
        pricing_model: config.pricing_model,
        reasoning_effort: config.reasoning_effort,
        open_ai_endpoint: config.open_ai_endpoint,
        open_ai_extra_params_enabled: config.open_ai_extra_params_enabled,
        open_ai_extra_params_json: config.open_ai_extra_params_json,
        custom_headers_enabled: config.custom_headers_enabled,
        custom_headers_json: config.custom_headers_json,
        anthropic_extra_params_enabled: config.anthropic_extra_params_enabled,
        anthropic_extra_params_json: config.anthropic_extra_params_json,
        context_window_tokens: config.context_window_tokens,
        max_completion_tokens: config.max_completion_tokens,
        anthropic_max_tokens: config.anthropic_max_tokens,
        anthropic_thinking_effort: config.anthropic_thinking_effort,
        thinking_budget_tokens: config.thinking_budget_tokens,
    })
}

fn validate_model(id: &str, name: &str, config: &CursorModelConfig) -> Result<(), AppError> {
    if id.trim().is_empty() || name.trim().is_empty() {
        return Err(AppError::Config(
            "Cursor Provider ID 和名称不能为空".to_string(),
        ));
    }
    if !matches!(config.provider_type.trim(), "openai" | "anthropic") {
        return Err(AppError::Config(
            "Cursor Provider 类型仅支持 openai 或 anthropic".to_string(),
        ));
    }
    if config.base_url.trim().is_empty()
        || config.api_key.trim().is_empty()
        || config.model_id.trim().is_empty()
    {
        return Err(AppError::Config(
            "Cursor Provider 的 Base URL、API Key 和模型 ID 不能为空".to_string(),
        ));
    }
    if config.provider_type == "openai"
        && !matches!(
            config.open_ai_endpoint.as_str(),
            "/v1/responses" | "/v1/chat/completions" | "/custom"
        )
    {
        return Err(AppError::Config("不支持的 OpenAI endpoint".to_string()));
    }
    for (enabled, raw, label) in [
        (
            config.open_ai_extra_params_enabled,
            config.open_ai_extra_params_json.as_str(),
            "OpenAI 额外参数",
        ),
        (
            config.custom_headers_enabled,
            config.custom_headers_json.as_str(),
            "自定义请求头",
        ),
        (
            config.anthropic_extra_params_enabled,
            config.anthropic_extra_params_json.as_str(),
            "Anthropic 额外参数",
        ),
    ] {
        if enabled {
            let value: serde_json::Value = serde_json::from_str(raw)
                .map_err(|error| AppError::Config(format!("{label} JSON 无效: {error}")))?;
            if !value.is_object() {
                return Err(AppError::Config(format!("{label} 必须是 JSON 对象")));
            }
        }
    }
    Ok(())
}
