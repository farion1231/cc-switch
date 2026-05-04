//! Axum handlers for the remote management API

use super::html::REMOTE_HTML;
use super::RemoteState;
use crate::app_config::AppType;
use crate::services::ProviderService;
use crate::store::AppState;
use axum::extract::State as AxumState;
use axum::http::header;
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Json};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};

const REMOTE_APP: AppType = AppType::Claude;

#[derive(Deserialize)]
pub struct SwitchRequest {
    pub provider_id: String,
}

fn app_state(state: &RemoteState) -> tauri::State<'_, AppState> {
    state.app_handle.state::<AppState>()
}

/// GET / — HTML 页面
pub async fn index(AxumState(state): AxumState<Arc<RemoteState>>) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Remote server is stopped",
        )
            .into_response();
    }
    Html(REMOTE_HTML).into_response()
}

/// GET /api/health
pub async fn health_check(AxumState(state): AxumState<Arc<RemoteState>>) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "stopped"})),
        )
            .into_response();
    }
    Json(json!({
        "status": "ok",
        "version": "1.0.0"
    }))
    .into_response()
}

/// GET /api/providers
pub async fn get_providers(AxumState(state): AxumState<Arc<RemoteState>>) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Server is stopped"})),
        )
            .into_response();
    }

    let app_state = app_state(&state);

    match ProviderService::list(app_state.inner(), REMOTE_APP) {
        Ok(providers) => {
            // 获取当前 provider ID
            let current_id =
                ProviderService::current(app_state.inner(), REMOTE_APP).unwrap_or_default();

            let provider_list: Vec<serde_json::Value> = providers
                .iter()
                .map(|(id, p)| {
                    json!({
                        "id": id,
                        "name": p.name,
                        "is_current": id == &current_id,
                        "category": p.category,
                        "icon": p.icon,
                        "icon_color": p.icon_color,
                    })
                })
                .collect();

            Json(json!({"providers": provider_list})).into_response()
        }
        Err(e) => Json(json!({"error": e.to_string()})).into_response(),
    }
}

/// GET /api/current
pub async fn get_current(AxumState(state): AxumState<Arc<RemoteState>>) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Server is stopped"})),
        )
            .into_response();
    }

    let app_state = app_state(&state);

    match ProviderService::current(app_state.inner(), REMOTE_APP) {
        Ok(current_id) => {
            if current_id.is_empty() {
                return Json(json!({"current": null})).into_response();
            }
            let providers =
                ProviderService::list(app_state.inner(), REMOTE_APP).unwrap_or_default();
            match providers.get(&current_id) {
                Some(p) => {
                    Json(json!({"current": {"id": current_id, "name": p.name}})).into_response()
                }
                None => Json(json!({"current": null})).into_response(),
            }
        }
        Err(_) => Json(json!({"current": null})).into_response(),
    }
}

/// POST /api/switch
pub async fn switch_provider(
    AxumState(state): AxumState<Arc<RemoteState>>,
    Json(body): Json<SwitchRequest>,
) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Server is stopped"})),
        )
            .into_response();
    }

    if body.provider_id.is_empty() {
        return Json(json!({"success": false, "error": "Missing provider_id"})).into_response();
    }

    let app_state = app_state(&state);

    // 核心：通过 ProviderService::switch() 切换，确保 backfill 正确
    match ProviderService::switch(app_state.inner(), REMOTE_APP, &body.provider_id) {
        Ok(result) => {
            // 获取 provider 名称
            let providers =
                ProviderService::list(app_state.inner(), REMOTE_APP).unwrap_or_default();
            let name = providers
                .get(&body.provider_id)
                .map(|p| p.name.clone())
                .unwrap_or_default();

            // 广播 SSE 给远程浏览器
            let sse_data = json!({
                "type": "switch",
                "provider_id": body.provider_id,
                "name": name
            });
            let _ = state.sse_tx.send(sse_data.to_string());

            // 通知 Tauri 前端刷新（使用与 tray 相同的事件名和格式）
            let _ = state.app_handle.emit(
                "provider-switched",
                json!({
                    "appType": REMOTE_APP.as_str(),
                    "providerId": body.provider_id
                }),
            );

            // 刷新托盘菜单
            if let Ok(new_menu) =
                crate::tray::create_tray_menu(&state.app_handle, app_state.inner())
            {
                if let Some(tray) = state.app_handle.tray_by_id("main") {
                    let _ = tray.set_menu(Some(new_menu));
                }
            }

            log::info!(
                "[Remote] Switched to provider '{}' ({})",
                name,
                body.provider_id
            );

            Json(json!({
                "success": true,
                "name": name,
                "warnings": result.warnings
            }))
            .into_response()
        }
        Err(e) => {
            log::error!("[Remote] Switch failed: {e}");
            Json(json!({"success": false, "error": e.to_string()})).into_response()
        }
    }
}

/// GET /api/events — SSE endpoint
pub async fn sse_events(
    AxumState(state): AxumState<Arc<RemoteState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.sse_tx.subscribe();

    let stream = async_stream::stream! {
        loop {
            if !state.running.load(Ordering::SeqCst) {
                // Server stopped: send shutdown event and close stream
                yield Ok(Event::default().data(r#"{"type":"shutdown"}"#));
                break;
            }
            match rx.recv().await {
                Ok(data) => {
                    yield Ok(Event::default().data(data));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Skip lagged messages, continue
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping"),
    )
}

/// GET /api/icon — 返回 CC Switch 图标 (PNG)
pub async fn get_icon() -> impl IntoResponse {
    let icon_bytes = include_bytes!("../../icons/icon.png");
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        icon_bytes.as_slice(),
    )
}

#[derive(Deserialize)]
pub struct IconQuery {
    pub color: Option<String>,
}

/// GET /api/provider-icons/:name?color=<hex> — 返回 provider 图标 (SVG)
/// color 参数：将 SVG 中的 currentColor 替换为指定颜色，使 <img> 标签也能正确着色
pub async fn get_provider_icon(
    axum::extract::Path(name): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<IconQuery>,
) -> impl IntoResponse {
    // 在编译时嵌入全部图标
    let svg_template: &'static str = match name.as_str() {
        "aicodemirror" => include_str!("../../../src/icons/extracted/aicodemirror.svg"),
        "aicoding" => include_str!("../../../src/icons/extracted/aicoding.svg"),
        "aihubmix-color" => include_str!("../../../src/icons/extracted/aihubmix-color.svg"),
        "algocode" => include_str!("../../../src/icons/extracted/algocode.svg"),
        "alibaba" => include_str!("../../../src/icons/extracted/alibaba.svg"),
        "anthropic" => include_str!("../../../src/icons/extracted/anthropic.svg"),
        "aws" => include_str!("../../../src/icons/extracted/aws.svg"),
        "azure" => include_str!("../../../src/icons/extracted/azure.svg"),
        "baidu" => include_str!("../../../src/icons/extracted/baidu.svg"),
        "bailian" => include_str!("../../../src/icons/extracted/bailian.svg"),
        "bytedance" => include_str!("../../../src/icons/extracted/bytedance.svg"),
        "catcoder" => include_str!("../../../src/icons/extracted/catcoder.svg"),
        "chatglm" => include_str!("../../../src/icons/extracted/chatglm.svg"),
        "claude" => include_str!("../../../src/icons/extracted/claude.svg"),
        "claw" => include_str!("../../../src/icons/extracted/claw.svg"),
        "cloudflare" => include_str!("../../../src/icons/extracted/cloudflare.svg"),
        "cohere" => include_str!("../../../src/icons/extracted/cohere.svg"),
        "copilot" => include_str!("../../../src/icons/extracted/copilot.svg"),
        "crazyrouter" => include_str!("../../../src/icons/extracted/crazyrouter.svg"),
        "ctok" => include_str!("../../../src/icons/extracted/ctok.svg"),
        "cubence" => include_str!("../../../src/icons/extracted/cubence.svg"),
        "dds" => include_str!("../../../src/icons/extracted/dds.svg"),
        "deepseek" => include_str!("../../../src/icons/extracted/deepseek.svg"),
        "doubao" => include_str!("../../../src/icons/extracted/doubao.svg"),
        "gemini" => include_str!("../../../src/icons/extracted/gemini.svg"),
        "gemma" => include_str!("../../../src/icons/extracted/gemma.svg"),
        "github" => include_str!("../../../src/icons/extracted/github.svg"),
        "githubcopilot" => include_str!("../../../src/icons/extracted/githubcopilot.svg"),
        "google" => include_str!("../../../src/icons/extracted/google.svg"),
        "googlecloud" => include_str!("../../../src/icons/extracted/googlecloud.svg"),
        "grok" => include_str!("../../../src/icons/extracted/grok.svg"),
        "huawei" => include_str!("../../../src/icons/extracted/huawei.svg"),
        "huggingface" => include_str!("../../../src/icons/extracted/huggingface.svg"),
        "hunyuan" => include_str!("../../../src/icons/extracted/hunyuan.svg"),
        "kimi" => include_str!("../../../src/icons/extracted/kimi.svg"),
        "lioncc" => include_str!("../../../src/icons/extracted/lioncc.svg"),
        "longcat-color" => include_str!("../../../src/icons/extracted/longcat-color.svg"),
        "mcp" => include_str!("../../../src/icons/extracted/mcp.svg"),
        "meta" => include_str!("../../../src/icons/extracted/meta.svg"),
        "micu" => include_str!("../../../src/icons/extracted/micu.svg"),
        "midjourney" => include_str!("../../../src/icons/extracted/midjourney.svg"),
        "minimax" => include_str!("../../../src/icons/extracted/minimax.svg"),
        "mistral" => include_str!("../../../src/icons/extracted/mistral.svg"),
        "modelscope-color" => include_str!("../../../src/icons/extracted/modelscope-color.svg"),
        "newapi" => include_str!("../../../src/icons/extracted/newapi.svg"),
        "notion" => include_str!("../../../src/icons/extracted/notion.svg"),
        "novita" => include_str!("../../../src/icons/extracted/novita.svg"),
        "nvidia" => include_str!("../../../src/icons/extracted/nvidia.svg"),
        "ollama" => include_str!("../../../src/icons/extracted/ollama.svg"),
        "openai" => include_str!("../../../src/icons/extracted/openai.svg"),
        "opencode-logo-light" => {
            include_str!("../../../src/icons/extracted/opencode-logo-light.svg")
        }
        "openrouter" => include_str!("../../../src/icons/extracted/openrouter.svg"),
        "packycode" => include_str!("../../../src/icons/extracted/packycode.svg"),
        "palm" => include_str!("../../../src/icons/extracted/palm.svg"),
        "perplexity" => include_str!("../../../src/icons/extracted/perplexity.svg"),
        "qwen" => include_str!("../../../src/icons/extracted/qwen.svg"),
        "rc" => include_str!("../../../src/icons/extracted/rc.svg"),
        "shengsuanyun" => include_str!("../../../src/icons/extracted/shengsuanyun.svg"),
        "siliconflow" => include_str!("../../../src/icons/extracted/siliconflow.svg"),
        "sssaicode" => include_str!("../../../src/icons/extracted/sssaicode.svg"),
        "stability" => include_str!("../../../src/icons/extracted/stability.svg"),
        "stepfun" => include_str!("../../../src/icons/extracted/stepfun.svg"),
        "tencent" => include_str!("../../../src/icons/extracted/tencent.svg"),
        "ucloud" => include_str!("../../../src/icons/extracted/ucloud.svg"),
        "vercel" => include_str!("../../../src/icons/extracted/vercel.svg"),
        "wenxin" => include_str!("../../../src/icons/extracted/wenxin.svg"),
        "xai" => include_str!("../../../src/icons/extracted/xai.svg"),
        "xiaomimimo" => include_str!("../../../src/icons/extracted/xiaomimimo.svg"),
        "yi" => include_str!("../../../src/icons/extracted/yi.svg"),
        "zeroone" => include_str!("../../../src/icons/extracted/zeroone.svg"),
        "zhipu" => include_str!("../../../src/icons/extracted/zhipu.svg"),
        _ => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1em" height="1em" viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="10"/></svg>"#
        }
    };

    // 用实际颜色替换 currentColor，使 <img> 标签也能正确着色
    let svg_content = match &query.color {
        Some(color) if !color.is_empty() && color != "currentColor" => {
            svg_template.replace("currentColor", color)
        }
        _ => svg_template.to_string(),
    };

    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        svg_content,
    )
}
