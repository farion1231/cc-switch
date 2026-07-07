//! 管理 API：通过 HTTP 增删改查 provider，写入独立 DB。
//!
//! 路由挂载于 `/admin/*`，与代理路由共用 `ProxyState`（含 `db`）作 axum state。
//! 调用方（standalone）构建 `Router<ProxyState>` 但**不** `with_state`，由
//! `ProxyServer::build_router` 末尾统一注入。
//!
//! 安全：无鉴权，依赖 `standalone::run` 限制监听地址为回环（见设计文档 §12）。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{
    CodexModelConfig, Provider, ProviderMeta, UniversalProvider, UniversalProviderApps,
};
use crate::proxy::server::ProxyState;

const CODEX: &str = "codex";

/// 创建 provider 的请求 DTO。
#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    /// `"openai_chat"` 触发 Responses→Chat 转换；`"openai_responses"` 透传。
    pub api_format: Option<String>,
    /// `true` 时同时设为当前 provider。
    #[serde(default)]
    pub enable: bool,
}

/// 列表项（脱敏，不含 api_key）。
#[derive(Debug, Serialize)]
struct ProviderSummary {
    id: String,
    name: String,
    base_url: String,
    model: String,
    api_format: Option<String>,
    is_current: bool,
}

/// DTO → cc-switch `Provider`，复用 `UniversalProvider::to_codex_provider()`。
fn build_codex_provider_from_dto(dto: &CreateProviderRequest) -> Result<Provider, String> {
    let api_format = match dto.api_format.as_deref() {
        Some("openai_chat") => "openai_chat",
        Some("openai_responses") => "openai_responses",
        Some(other) => return Err(format!("不支持的 api_format: {other}")),
        None => "openai_responses",
    };

    // 用 UUID v4 作 id 后缀，避免基于时间戳在并发/时钟回拨时碰撞导致静默覆盖。
    let id = format!("cli-{}", uuid::Uuid::new_v4().simple());
    let mut universal = UniversalProvider::new(
        id,
        dto.name.clone(),
        "custom".to_string(),
        dto.base_url.clone(),
        dto.api_key.clone(),
    );
    universal.apps = UniversalProviderApps {
        codex: true,
        ..Default::default()
    };
    universal.models.codex = Some(CodexModelConfig {
        model: Some(dto.model.clone()),
        reasoning_effort: dto.reasoning_effort.clone().or(Some("high".to_string())),
    });
    universal.meta = Some(ProviderMeta {
        api_format: Some(api_format.to_string()),
        ..Default::default()
    });

    universal
        .to_codex_provider()
        .ok_or_else(|| "to_codex_provider 返回 None（apps.codex 未启用）".to_string())
}

/// `AppError` → `(StatusCode, 对外文案)`。完整错误记日志，避免对外泄露 SQL/路径。
fn map_err(e: AppError) -> (StatusCode, String) {
    let msg = e.to_string();
    if msg.contains("not found") || msg.contains("不存在") {
        (StatusCode::NOT_FOUND, msg)
    } else {
        log::error!("[admin] 数据库错误: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, "数据库错误".to_string())
    }
}

pub fn build_admin_router() -> Router<ProxyState> {
    Router::new()
        .route("/admin/providers", get(list_providers).post(create_provider))
        .route("/admin/providers/:id", delete(delete_provider))
        .route("/admin/providers/:id/enable", post(enable_provider))
        .route("/admin/status", get(status))
}

async fn create_provider(
    State(state): State<ProxyState>,
    Json(dto): Json<CreateProviderRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let provider = build_codex_provider_from_dto(&dto).map_err(|m| (StatusCode::BAD_REQUEST, m))?;
    state.db.save_provider(CODEX, &provider).map_err(map_err)?;
    if dto.enable {
        state
            .db
            .set_current_provider(CODEX, &provider.id)
            .map_err(map_err)?;
    }
    Ok(Json(json!({ "ok": true, "id": provider.id, "name": provider.name })))
}

async fn list_providers(State(state): State<ProxyState>) -> Result<Json<Value>, (StatusCode, String)> {
    let all = state.db.get_all_providers(CODEX).map_err(map_err)?;
    let current = state.db.get_current_provider(CODEX).unwrap_or(None);
    let items: Vec<ProviderSummary> = all
        .values()
        .map(|p| {
            let (base_url, _api_key) = p.resolve_usage_credentials(&AppType::Codex);
            let model = p
                .settings_config
                .get("config")
                .and_then(|c| c.as_str())
                .and_then(extract_model_from_toml)
                .unwrap_or_default();
            ProviderSummary {
                id: p.id.clone(),
                name: p.name.clone(),
                base_url,
                model,
                api_format: p.meta.as_ref().and_then(|m| m.api_format.clone()),
                is_current: current.as_deref() == Some(p.id.as_str()),
            }
        })
        .collect();
    Ok(Json(json!({ "providers": items })))
}

async fn delete_provider(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.db.delete_provider(CODEX, &id).map_err(map_err)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn enable_provider(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if state
        .db
        .get_provider_by_id(&id, CODEX)
        .map_err(map_err)?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, format!("provider 不存在: {id}")));
    }
    state
        .db
        .set_current_provider(CODEX, &id)
        .map_err(map_err)?;
    // 防御并发 TOCTOU：设置后再读回验证，避免「客户端收 200 但实际无 current」。
    let current = state.db.get_current_provider(CODEX).unwrap_or(None);
    if current.as_deref() != Some(id.as_str()) {
        return Err((
            StatusCode::CONFLICT,
            "provider 在启用期间被移除，当前无启用的 provider".to_string(),
        ));
    }
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn status(State(state): State<ProxyState>) -> impl IntoResponse {
    let mut s = state.status.read().await.clone();
    // last_error 可能承载上游返回的敏感信息，对外只暴露是否存在。
    if s.last_error.is_some() {
        s.last_error = Some("[error] 上游返回错误，详见代理日志".to_string());
    }
    Json(serde_json::to_value(s).unwrap_or(json!({})))
}

/// 从 codex `config.toml` 粗提取 `model = "..."` 的值（仅用于列表展示）。
/// 容忍 `model=`/`model =`/`model  =` 等空格变体。
fn extract_model_from_toml(toml: &str) -> Option<String> {
    let line = toml.lines().find(|l| {
        let t = l.trim_start();
        t.starts_with("model")
            && t
                .as_bytes()
                .get(5)
                .map(|&c| c == b' ' || c == b'\t' || c == b'=')
                .unwrap_or(false)
    })?;
    let v = line.split('=').nth(1)?;
    let v = v.trim().trim_matches('"');
    Some(v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_maps_to_codex_provider_with_chat_format() {
        let dto = CreateProviderRequest {
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com".into(),
            api_key: "sk-test".into(),
            model: "deepseek-chat".into(),
            reasoning_effort: None,
            api_format: Some("openai_chat".into()),
            enable: false,
        };
        let provider = build_codex_provider_from_dto(&dto).expect("map dto");

        assert!(provider.id.starts_with("universal-codex-cli-"));
        assert_eq!(provider.name, "DeepSeek");
        assert_eq!(
            provider
                .settings_config
                .pointer("/auth/OPENAI_API_KEY")
                .and_then(|v| v.as_str()),
            Some("sk-test")
        );
        let toml = provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(toml.contains("base_url = \"https://api.deepseek.com/v1\""));
        assert!(toml.contains("model = \"deepseek-chat\""));
        assert_eq!(
            provider.meta.as_ref().unwrap().api_format.as_deref(),
            Some("openai_chat")
        );
    }

    #[test]
    fn invalid_api_format_rejected() {
        let dto = CreateProviderRequest {
            name: "X".into(),
            base_url: "https://x".into(),
            api_key: "k".into(),
            model: "m".into(),
            reasoning_effort: None,
            api_format: Some("bogus".into()),
            enable: false,
        };
        assert!(build_codex_provider_from_dto(&dto).is_err());
    }

    #[test]
    fn extract_model_handles_spacing_variants() {
        assert_eq!(extract_model_from_toml(r#"model = "gpt-4o""#), Some("gpt-4o".into()));
        assert_eq!(extract_model_from_toml(r#"model="gpt-4o""#), Some("gpt-4o".into()));
        assert_eq!(extract_model_from_toml(r#"model  =  "gpt-4o""#), Some("gpt-4o".into()));
        // 不能误匹配 model_reasoning_effort
        assert_eq!(extract_model_from_toml("model_reasoning_effort = \"high\""), None);
    }
}
