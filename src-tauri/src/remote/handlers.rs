//! Axum handlers for the remote management API

use super::auth::{SessionAudience, SessionIdentity};
use super::html::REMOTE_HTML;
use super::redact;
use super::{BindScope, RemoteState};
use crate::app_config::AppType;
use crate::commands::xai_oauth::XaiOAuthState;
use crate::commands::{self, CopilotAuthState};
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::services::{ProviderService, ProviderSortUpdate};
use crate::store::AppState;
use axum::extract::{Extension, State as AxumState};
use axum::http::header;
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Json};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

const REMOTE_APP: AppType = AppType::Claude;

#[derive(Deserialize)]
pub struct SwitchRequest {
    pub provider_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderRequest {
    /// 被拖拽移动的 provider ID
    pub moved_id: String,
    /// 目标位置的 provider ID（移动到这个 provider 之前）
    pub before_id: String,
}

/// `app_state()` 的取值结果：两种来源，对调用点一律只暴露 `&AppState`。
///
/// - [`AppStateRef::Managed`]：GUI 运行时下 Tauri managed state 的借用，
///   与本 seam 落地前的语义完全一致。
/// - [`AppStateRef::Headless`]：无 `AppHandle` 时进程内自持的实例。
enum AppStateRef<'a> {
    Managed(tauri::State<'a, AppState>),
    Headless(&'static AppState),
}

impl AppStateRef<'_> {
    fn inner(&self) -> &AppState {
        match self {
            Self::Managed(s) => s.inner(),
            Self::Headless(s) => s,
        }
    }
}

impl std::ops::Deref for AppStateRef<'_> {
    type Target = AppState;

    fn deref(&self) -> &AppState {
        self.inner()
    }
}

/// 无 `AppHandle` 时使用的自持 `AppState`（headless 接缝）。
///
/// 走 [`AppState::new`] 这条生产构造，不另拼一个字段更少的替身 —— 否则
/// headless 验到的就不是 GUI 真正跑的那套对象，seam 会悄悄漂移。
///
/// 数据库用内存库：没有运行时就没有真实的 `~/.cc-switch`，夹具绝不能读用户
/// 的真实配置。进程内单例（`OnceLock`），因此多个 headless server 共享同一份
/// 状态，与 Tauri managed state 的单例语义一致。
///
/// 只有 `RemoteApp::headless()`（`#[cfg(test)]`）能走到这里，生产构建下
/// `RemoteApp::inner` 恒为 `Some`，本函数不可达。
///
/// `pub(super)` 是为了让 `e2e`（remote 的子模块）能拿到**同一个**实例往里塞
/// 夹具 provider：E2E 若自建一个 `AppState`，handler 读到的就不是它，验到的
/// 也就不是生产路径。
pub(super) fn headless_app_state() -> &'static AppState {
    static HEADLESS: std::sync::OnceLock<AppState> = std::sync::OnceLock::new();
    HEADLESS.get_or_init(|| {
        let db = Arc::new(
            crate::database::Database::memory()
                .expect("headless remote server requires an in-memory database"),
        );
        AppState::new(db)
    })
}

/// 取 managed [`AppState`]，headless 下退回到自持实例。
///
/// 直接判 `RemoteApp::inner` 而不是走 `Deref`：`Deref` 在无 `AppHandle` 时
/// 是 `expect` panic，请求会在连接里炸掉。`inner` 是 `remote` 模块内的私有
/// 字段，而本模块是它的子模块，因此这里读得到、又不必改动 `Deref` 的 GUI
/// 语义（GUI 分支仍是 `app.state::<AppState>()`）。
fn app_state(state: &RemoteState) -> AppStateRef<'_> {
    match state.app_handle.inner.as_ref() {
        Some(app) => AppStateRef::Managed(app.state::<AppState>()),
        None => AppStateRef::Headless(headless_app_state()),
    }
}

/// 取 Copilot OAuth 状态，headless 下退回到进程内自持实例。
///
/// 与 [`app_state`] 同一条接缝，只是换了个状态类型。GUI 分支克隆托管状态里的
/// `Arc`（与 GUI 共享同一份账号表），headless 分支按 [`RemoteState::config_dir`]
/// 新建一个 —— 那是夹具的临时目录，headless 用例因此仍不碰用户真实的
/// `~/.cc-switch`。
fn copilot_state(state: &RemoteState) -> CopilotAuthState {
    match state.app_handle.inner.as_ref() {
        Some(app) => CopilotAuthState(app.state::<CopilotAuthState>().0.clone()),
        None => CopilotAuthState(Arc::new(tauri::async_runtime::RwLock::new(
            CopilotAuthManager::new(state.config_dir.clone()),
        ))),
    }
}

/// 取 xAI OAuth 状态，headless 下退回到进程内自持实例。见 [`copilot_state`]。
fn xai_state(state: &RemoteState) -> XaiOAuthState {
    match state.app_handle.inner.as_ref() {
        Some(app) => XaiOAuthState(app.state::<XaiOAuthState>().0.clone()),
        None => XaiOAuthState(Arc::new(tauri::async_runtime::RwLock::new(
            XaiOAuthManager::new(state.config_dir.clone()),
        ))),
    }
}

/// 远程错误响应的**唯一**出口。
///
/// 签名只收 `Display` 与状态码，**不收已经成型的 `serde_json::Value`**：收
/// `Value` 就还能写出 `remote_error(OK, json!({"error": e.to_string()}))` 这种
/// 「先把上游文本拼进 JSON、再假装过了闸门」的调用。顺序错误会重新变得可表达，
/// 而它正是这道出口要消灭的东西 —— 上一轮在单个分支补 `redact_error` 之所以
/// 失效，就是同一函数里还有十几处各自拼的 `json!` 分支。
///
/// 需要在错误体上追加额外字段（如 `proxy_control` 的 `action`）时用
/// [`redact::error_json_body`]：同一个出口的 `Value` 形态，脱敏同样在里面强制
/// 发生，追加只发生在脱敏之后。
fn remote_error(
    status: axum::http::StatusCode,
    detail: impl std::fmt::Display,
) -> axum::response::Response {
    redact::error_json(status, detail)
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
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    let app_state = app_state(&state);

    match ProviderService::list(app_state.inner(), REMOTE_APP) {
        Ok(providers) => {
            // 获取当前 provider ID
            let current_id =
                ProviderService::current(app_state.inner(), REMOTE_APP).unwrap_or_default();

            // IndexMap 的迭代序是插入序，对「优先级」这一语义毫无意义：Web UI
            // 若不按 sort_index 排，`POST /api/reorder` 改完顺序后页面看起来没变。
            // 排序口径与 `reorder_providers` 一致（None 排最后）；sort_index 相同时
            // 用名字兜底，避免同值时顺序随 IndexMap 的插入序漂移。
            let mut ordered: Vec<(&String, &crate::provider::Provider)> =
                providers.iter().collect();
            ordered.sort_by(|(_, a), (_, b)| match (a.sort_index, b.sort_index) {
                (Some(ai), Some(bi)) => ai.cmp(&bi).then_with(|| a.name.cmp(&b.name)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.name.cmp(&b.name),
            });

            let provider_list: Vec<serde_json::Value> = ordered
                .into_iter()
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
        Err(e) => remote_error(axum::http::StatusCode::OK, e),
    }
}

/// GET /api/current
pub async fn get_current(AxumState(state): AxumState<Arc<RemoteState>>) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
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
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    if body.provider_id.is_empty() {
        return remote_error(axum::http::StatusCode::OK, "Missing provider_id");
    }

    let app_state = app_state(&state);

    // 故障转移开启时供应商由队列推进，手动切换会把队列的 P1 顶掉。拒绝必须
    // **零副作用**：不广播、不 notify、不刷新托盘 —— 否则 UI 会以为切换发生过。
    // 判据与命令层共用同一个纯函数：failover 只在路由模式下由队列推进，直连
    // 模式下即使标志残留也允许手动切换。
    let (_, auto_failover) = app_state
        .inner()
        .db
        .get_proxy_flags_sync(REMOTE_APP.as_str());
    if crate::commands::manual_switch_blocked(
        crate::mode::current::is_proxy(&REMOTE_APP),
        auto_failover,
    ) {
        return remote_error(
            axum::http::StatusCode::OK,
            "故障转移已开启，无法手动切换供应商；请先关闭故障转移",
        );
    }

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
            state.sink.notify_frontend(
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
            remote_error(axum::http::StatusCode::OK, e)
        }
    }
}

/// POST /api/reorder
///
/// 模拟拖拽排序：将 moved_id 移动到 before_id 之前。
/// 如果 before_id 为空，则移动到列表末尾。
pub async fn reorder_providers(
    AxumState(state): AxumState<Arc<RemoteState>>,
    Json(body): Json<ReorderRequest>,
) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    if body.moved_id.is_empty() {
        return remote_error(axum::http::StatusCode::OK, "Missing moved_id");
    }

    let app_state = app_state(&state);

    // 1. 获取当前完整 provider 列表
    let providers = match ProviderService::list(app_state.inner(), REMOTE_APP) {
        Ok(p) => p,
        Err(e) => {
            return remote_error(axum::http::StatusCode::OK, e);
        }
    };

    // 2. 按 sort_index 排序（与前端 sortedProviders 一致）
    let mut sorted: Vec<(&String, &crate::provider::Provider)> = providers.iter().collect();
    sorted.sort_by(|(_, a), (_, b)| match (a.sort_index, b.sort_index) {
        (Some(ai), Some(bi)) => ai.cmp(&bi),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    // 3. 找到 moved_id 和 before_id 的位置
    let moved_idx = sorted
        .iter()
        .position(|(id, _)| id.as_str() == body.moved_id);
    let before_idx = if body.before_id.is_empty() {
        None // 移动到末尾
    } else {
        sorted
            .iter()
            .position(|(id, _)| id.as_str() == body.before_id)
    };

    let moved_idx = match moved_idx {
        Some(idx) => idx,
        None => {
            return remote_error(
                axum::http::StatusCode::OK,
                format!("Provider '{}' not found", body.moved_id),
            );
        }
    };

    // 4. 模拟 arrayMove：移除 moved，插入到 before 之前
    let item = sorted.remove(moved_idx);
    let new_idx = match before_idx {
        Some(idx) => {
            // 移除 moved 后，若 moved 在 before 前面，before 的位置前移了一位
            if idx > moved_idx {
                idx - 1
            } else {
                idx
            }
        }
        None => sorted.len(), // 末尾
    };
    sorted.insert(new_idx, item);

    // 5. 重新分配 sort_index
    let updates: Vec<ProviderSortUpdate> = sorted
        .into_iter()
        .enumerate()
        .map(|(idx, (id, _))| ProviderSortUpdate {
            id: id.clone(),
            sort_index: idx,
        })
        .collect();

    // 6. 保存
    match ProviderService::update_sort_order(app_state.inner(), REMOTE_APP, updates) {
        Ok(_) => {
            log::info!(
                "[Remote] Moved provider '{}' to position {} in {}",
                body.moved_id,
                new_idx,
                REMOTE_APP.as_str()
            );

            // 数据库已经改了，但桌面端与内嵌 Web UI 都靠 SSE 才知道顺序变了：
            // 不发这一帧，两边会一直显示旧的 P 序号，且没有任何报错提示它们过期了。
            let sse_data = json!({
                "type": "reorder",
                "app_type": REMOTE_APP.as_str(),
                "moved_id": &body.moved_id,
                "position": new_idx,
            });
            let _ = state.sse_tx.send(sse_data.to_string());
            log::info!(
                "[Remote] Broadcast reorder event: '{}' -> position {} in {}",
                body.moved_id,
                new_idx,
                REMOTE_APP.as_str()
            );

            Json(json!({"success": true})).into_response()
        }
        Err(e) => {
            log::error!("[Remote] Reorder failed: {e}");
            remote_error(axum::http::StatusCode::OK, e)
        }
    }
}

/// GET /api/events — SSE endpoint
///
/// 视图由 [`SessionAudience`] 选择：本机无凭据调用方拿到**降级流**（仍 200、仍能
/// 收到帧，但切换帧只剩 `type` 与降级标记），持有效凭据的连接逐字拿到全量帧。
/// 判定格第 7 格要求 loopback 无凭据时可达，所以这里绝不能改成 401。
pub async fn sse_events(
    AxumState(state): AxumState<Arc<RemoteState>>,
    audience: Option<Extension<SessionAudience>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.sse_tx.subscribe();
    // `Option` 兜住没经过鉴权层的调用（单元直调等）。缺档位时按**匿名**处理：
    // 未知即最小暴露，不能因为少一个 extension 就默认全量透传。
    let audience = audience.map_or(SessionAudience::AnonymousLoopback, |Extension(a)| a);

    let stream = async_stream::stream! {
        loop {
            if !state.running.load(Ordering::SeqCst) {
                // Server stopped: send shutdown event and close stream
                yield Ok(Event::default().data(r#"{"type":"shutdown"}"#));
                break;
            }
            match rx.recv().await {
                Ok(data) => {
                    // `None` = 这一帧不下发（非法 JSON），跳过它但**不**断流。
                    if let Some(frame) = project_frame(&data, audience) {
                        yield Ok(Event::default().data(frame));
                    }
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

/// 广播帧里**携带 provider 细节**、因而必须向匿名调用方降级的帧类型白名单。
///
/// `switch` 带 `provider_id` / `name`，`reorder` 带 `moved_id`，
/// `failover_changed` 带 `app_type` / `enabled` —— 三者都是供应商拓扑，
/// 匿名流只保留「发生过这件事」。
const DEGRADED_FRAME_TYPES: &[&str] = &["switch", "reorder", "failover_changed"];

/// 把一帧广播载荷投影成 SSE 事件文本；`None` = 这一帧不下发（**不得**因此断流）。
///
/// 纯函数：广播扇出、`Lagged`、keep-alive 与断连语义全在调用方，这里只回答
/// 「这一帧发给这个档位的调用方时长什么样」。
///
/// - [`SessionAudience::Authenticated`]：逐字透传，一个字节都不动。
/// - [`SessionAudience::AnonymousLoopback`]：命中 [`DEGRADED_FRAME_TYPES`] 的帧
///   （`switch` / `reorder` / `failover_changed`）按**白名单**重建为
///   `{"type":<type>,"degraded":true}` —— 只留类型与降级标记，`provider_id` /
///   `name` / `moved_id` / `position` / `app_type` / `enabled` 等一切供应商细节
///   都不得出现；其余帧类型（`shutdown` 等）原样下发。
///
/// 为什么按白名单重建而不是逐个 `remove`：移除清单是**枚举**的，新加一个
/// `provider_xxx` 字段就会从清单漏出去；白名单是**封闭**的，漏不出去。
pub fn project_frame(raw: &str, audience: SessionAudience) -> Option<String> {
    match audience {
        SessionAudience::Authenticated => Some(raw.to_string()),
        SessionAudience::AnonymousLoopback => {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
                return None;
            };
            let Some(frame_type) = value.get("type").and_then(|t| t.as_str()) else {
                return Some(raw.to_string());
            };
            if !DEGRADED_FRAME_TYPES.contains(&frame_type) {
                return Some(raw.to_string());
            }
            serde_json::to_string(&json!({
                "type": frame_type,
                "degraded": true,
            }))
            .ok()
        }
    }
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

/// `color` 查询参数是否是可安全替换进 SVG 的颜色字面量。
///
/// 唯一放行的形态是 `#` + 3/4/6/8 位 ASCII 十六进制（CSS 短写、带 alpha 的短写、
/// 常规写、带 alpha 的常规写）。返回 `Some` 时交回**原串**而不是裁剪后的 `hex`
/// —— 替换点需要的是带 `#` 的完整写法。
///
/// 为什么必须白名单而不是黑名单：替换目标是 `fill="currentColor"` 的属性值，
/// 一旦取值能带上 `"`，就能闭合属性再写任意新属性（`onload=` 之类），
/// 而这在 `<img src>` 下也会被当作 SVG 文档执行。十六进制字符集之外的任何取值
/// （含 `red`、`transparent`、`currentColor` 变体）一律判否。
fn safe_svg_color(color: &str) -> Option<&str> {
    let hex = color.strip_prefix('#')?;
    if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(color)
    } else {
        None
    }
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
        "cubence" => include_str!("../../../src/icons/extracted/cubence.svg"),
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

    // 用实际颜色替换 currentColor，使 <img> 标签也能正确着色。
    // 颜色必须先过 [`safe_svg_color`]：query 参数直接拼进 SVG 文本，
    // `color=red" onload="alert(1)` 这种取值会**闭掉 fill 属性再注入新属性**。
    let svg_content = match &query.color {
        Some(color) if !color.is_empty() && color != "currentColor" => {
            match safe_svg_color(color) {
                Some(color) => svg_template.replace("currentColor", color),
                None => svg_template.to_string(),
            }
        }
        _ => svg_template.to_string(),
    };

    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            // SVG 是**可执行文档**而不是图片：内联它就等于交出脚本执行权。
            // `default-src 'none'` 关掉一切来源，只留 `style-src 'unsafe-inline'`
            // 给图标内部可能存在的内联样式。
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
            // 不加这条时浏览器可能把 SVG 按文本嗅探，或干脆猜成别的类型。
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        svg_content,
    )
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxyControlRequest {
    /// true  = 启动代理服务（不接管任何 app 的 Live 配置，接管见 /api/proxy/takeover）
    /// false = 停止代理并让所有 app 退回直连 — 默认
    pub start: bool,
}

/// POST /api/proxy/control — 统一控制代理的停止与启动
///
/// Body（必填 JSON，必须带 `Content-Type: application/json`）：
///   - `{}`                          → 停止代理并让所有 app 退回直连
///   - `{"start": true}`             → 启动代理服务
///   - `{"start": false}`            → 停止代理（与 `{}` 等价）
///
/// 注意：Body 必填（axum `Json` extractor）。不带 `Content-Type: application/json`
/// 的请求（跨站表单提交、不带 Content-Type 的 `fetch`）会被 415 拒绝，因此这些
/// CORS 简单请求无法触发停止分支（它会停代理、让各 app 退回直连并删除 live
/// 备份）——这正是把 extractor 设为必填的原因。
///
/// 远程调用前必须停止代理，否则开发版本启动后会因 single-instance 冲突
/// 导致生产进程被抢占，进而使正在运行的 Agent 无法调用模型。
pub async fn proxy_control(
    AxumState(state): AxumState<Arc<RemoteState>>,
    Json(body): Json<ProxyControlRequest>,
) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    let app_state = app_state(&state);
    let start = body.start;

    if start {
        // 启动代理服务。上游 4.0.14 把 `start_with_takeover` 折进 `mode::controller`：
        // 「拉起服务」与「接管某个 app 的 Live 配置」现在是两件事，后者走
        // `set_proxy_takeover_for_app`（对应本文件的 `/api/proxy/takeover`）。
        // 这里与桌面命令 `commands::proxy::start_proxy_server` 停在同一层：
        // 只把服务拉起来，不替用户改写任何 app 的 Live 配置。
        // 幂等：`ProxyService::start()` 内部（`start_server`）会先看 is_running()，
        // 已在跑时直接回当前地址/端口。
        match app_state.proxy_service.start().await {
            Ok(info) => {
                // 与 `commands::proxy::start_proxy_server` 对称，让桌面 UI 通过 SSE
                // invalidate（远程 API 改状态后 UI 不刷新是历史上出过的回归）。
                app_state.proxy_service.broadcast_event(json!({
                    "type": "proxy_started",
                    "address": info.address,
                    "port": info.port,
                }));
                log::info!(
                    "[Remote] Proxy started via API: {}:{}",
                    info.address,
                    info.port
                );
                Json(json!({
                    "success": true,
                    "action": "start",
                    "address": info.address,
                    "port": info.port,
                }))
                .into_response()
            }
            Err(e) => {
                log::error!("[Remote] Failed to start proxy via API: {e}");
                let mut body = redact::error_json_body(e);
                body["action"] = json!("start");
                Json(body).into_response()
            }
        }
    } else {
        // 与桌面命令 `commands::proxy::stop_proxy_with_restore` 同层：所有 app 退回直连，
        // 再停服务（`stop_with_restore` 已被上游折进 `mode::controller`）。
        match crate::mode::controller::exit_all(app_state.inner()).await {
            Ok(()) => {
                app_state
                    .proxy_service
                    .broadcast_event(json!({"type": "proxy_stopped"}));
                log::info!("[Remote] Proxy stopped and live configs restored via API");
                Json(json!({"success": true, "action": "stop"})).into_response()
            }
            Err(e) => {
                log::error!("[Remote] Failed to stop proxy via API: {e}");
                let mut body = redact::error_json_body(e);
                body["action"] = json!("stop");
                Json(body).into_response()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTakeoverRequest {
    pub app_type: String,
    pub enabled: bool,
}

/// POST /api/proxy/takeover — 为指定应用开启/关闭代理接管
///
/// Body: {"appType": "claude", "enabled": true}
///
/// 与桌面命令 `commands::proxy::set_proxy_takeover_for_app` 同层：开启走
/// `mode::controller::enter_with_route`（内部会按需把代理服务拉起来），关闭走
/// `mode::controller::exit`。远程请求没有确认框，所以 `stack` 固定 false（路由
/// 模式）、`route` 传 `None` 沿用上次的路由 —— 与桌面点「开启」不选目标时一致。
pub async fn proxy_takeover(
    AxumState(state): AxumState<Arc<RemoteState>>,
    Json(body): Json<ProxyTakeoverRequest>,
) -> impl IntoResponse {
    use std::str::FromStr;

    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    let app_state = app_state(&state);

    // 与 `commands::proxy::require_mode_app` 同一条校验（错误文案也一致）。
    let app = match AppType::from_str(&body.app_type) {
        Ok(app) => app,
        Err(e) => {
            return remote_error(axum::http::StatusCode::OK, format!("无效的应用类型: {e}"));
        }
    };
    if !app.has_route_mode() {
        return remote_error(
            axum::http::StatusCode::OK,
            format!("{} 不支持本地路由", app.as_str()),
        );
    }

    let result = if body.enabled {
        crate::mode::controller::enter_with_route(app_state.inner(), &app, false, None).await
    } else {
        crate::mode::controller::exit(app_state.inner(), &app).await
    };

    match result {
        Ok(()) => {
            // 与桌面命令 `commands::proxy::set_proxy_takeover_for_app` 发同一条事件。
            // 桌面端的 `useProxyStatus` 用 `useProxyTakeoverStatus(false)` —— 接管状态
            // **不轮询**，只靠这条 SSE 触发 React Query invalidate。远程改了接管却
            // 不发事件，桌面就会停在「未接管」，而流量其实已经在走代理。
            app_state.proxy_service.broadcast_event(json!({
                "type": "takeover_changed",
                "app_type": app.as_str(),
                "enabled": body.enabled,
            }));
            log::info!(
                "[Remote] Proxy takeover for {} set to {} via API",
                body.app_type,
                body.enabled
            );
            Json(json!({"success": true})).into_response()
        }
        Err(e) => {
            log::error!("[Remote] Failed to set proxy takeover via API: {e}");
            remote_error(axum::http::StatusCode::OK, e)
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverToggleRequest {
    pub app_type: String,
    pub enabled: bool,
}

/// POST /api/proxy/failover — 开启/关闭指定应用的自动故障转移
///
/// Body: {"appType": "claude", "enabled": true}
///
/// 开启时会自动将故障转移队列的 P1 设为当前供应商。
pub async fn proxy_failover(
    AxumState(state): AxumState<Arc<RemoteState>>,
    Json(body): Json<FailoverToggleRequest>,
) -> impl IntoResponse {
    use std::str::FromStr;

    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    let app_state = app_state(&state);
    let app = &state.app_handle;

    // 强一致语义：开启故障转移后立即切到队列 P1（并确保队列非空）
    let p1_provider_id = if body.enabled {
        let mut queue = match app_state.db.get_failover_queue(&body.app_type) {
            Ok(q) => q,
            Err(e) => {
                return remote_error(axum::http::StatusCode::OK, e);
            }
        };

        if queue.is_empty() {
            let app_enum = match crate::app_config::AppType::from_str(&body.app_type) {
                Ok(a) => a,
                Err(_) => {
                    return remote_error(
                        axum::http::StatusCode::OK,
                        format!("无效的应用类型: {}", body.app_type),
                    );
                }
            };

            // 与桌面命令 `commands::failover::set_auto_failover_enabled` 同层：上游
            // 4.0.14 要求「读当前供应商」一律经 `mode::current::provider_for` 并说明
            // 用途（`mode::current` 那条源码门禁会拦下裸读）。故障转移队列的 P1 要
            // 排的是**正在用的那家**，所以是 `Purpose::InUse`，与桌面命令逐字一致。
            let current_id = match crate::mode::current::provider_for(
                &app_state.db,
                &app_enum,
                crate::mode::current::Purpose::InUse,
            ) {
                Ok(id) => id,
                Err(e) => {
                    return remote_error(axum::http::StatusCode::OK, e.to_string());
                }
            };

            let Some(current_id) = current_id else {
                return remote_error(
                    axum::http::StatusCode::OK,
                    "故障转移队列为空，且未设置当前供应商，无法开启故障转移",
                );
            };

            if let Err(e) = app_state
                .db
                .add_to_failover_queue(&body.app_type, &current_id)
            {
                return remote_error(axum::http::StatusCode::OK, e);
            }

            queue = match app_state.db.get_failover_queue(&body.app_type) {
                Ok(q) => q,
                Err(e) => {
                    return remote_error(axum::http::StatusCode::OK, e);
                }
            };
        }

        match queue.first() {
            Some(item) => item.provider_id.clone(),
            None => {
                return remote_error(
                    axum::http::StatusCode::OK,
                    "故障转移队列为空，无法开启故障转移",
                );
            }
        }
    } else {
        String::new()
    };

    // 读取并更新 proxy_config
    let mut config = match app_state.db.get_proxy_config_for_app(&body.app_type).await {
        Ok(c) => c,
        Err(e) => {
            return remote_error(axum::http::StatusCode::OK, e);
        }
    };
    config.auto_failover_enabled = body.enabled;

    if let Err(e) = app_state.db.update_proxy_config_for_app(config).await {
        return remote_error(axum::http::StatusCode::OK, e);
    }

    // 开启后立即切到 P1
    if body.enabled {
        let app_enum = match crate::app_config::AppType::from_str(&body.app_type) {
            Ok(a) => a,
            Err(e) => {
                return remote_error(axum::http::StatusCode::OK, format!("无效的应用类型: {e}"));
            }
        };

        // 与桌面命令 `commands::failover::set_auto_failover_enabled` 同层：上游 4.0.14
        // 用 `mode::controller::switch_route_for_failover` 取代了
        // `ProxyService::switch_proxy_target`（后者已删）。故障转移不走聚合模式，
        // 所以用带这个拒绝的入口，而不是通用的 `switch_route`。
        if let Err(e) = crate::mode::controller::switch_route_for_failover(
            app_state.inner(),
            &app_enum,
            &p1_provider_id,
        )
        .await
        {
            return remote_error(axum::http::StatusCode::OK, e);
        }

        // 发射 provider-switched 事件
        let event_data = serde_json::json!({
            "appType": body.app_type,
            "providerId": p1_provider_id,
            "source": "failoverEnabled"
        });
        state.sink.notify_frontend("provider-switched", event_data);

        // 广播给远程浏览器（SSE）
        let app_clone = app.clone();
        let p1_id = p1_provider_id.clone();
        let app_type = body.app_type.clone();
        tauri::async_runtime::spawn(async move {
            crate::remote::broadcast_provider_switch(&app_clone, &app_type, &p1_id).await;
        });
    }

    // 无论开启还是关闭都广播：UI 上这个开关决定「路由目标按队列还是按手动」，
    // 不发这一帧，前端就会一直沿用旧的目标列表（切走再回来才刷新）。
    // 放在 P1 切换之后，确保订阅方读到的已经是最终状态。
    let failover_frame = json!({
        "type": "failover_changed",
        "app_type": &body.app_type,
        "enabled": body.enabled,
    });
    let _ = state.sse_tx.send(failover_frame.to_string());
    log::info!(
        "[Remote] Broadcast failover_changed: {} enabled={}",
        body.app_type,
        body.enabled
    );

    // 刷新托盘菜单
    if let Ok(new_menu) = crate::tray::create_tray_menu(app, app_state.inner()) {
        if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
            let _ = tray.set_menu(Some(new_menu));
        }
    }

    log::info!(
        "[Remote] Auto failover for {} set to {} via API",
        body.app_type,
        body.enabled
    );

    Json(json!({"success": true})).into_response()
}

/// 单个 tier 的用量 DTO
#[derive(Debug, Clone, Serialize)]
pub struct TierUsageDto {
    pub name: String,
    pub used_percent: f64,
    pub remaining: f64,
    pub total: f64,
    pub unit: String,
    pub resets_at: Option<String>,
}

/// Provider 用量 DTO
#[derive(Debug, Clone, Serialize)]
pub struct ProviderUsageDto {
    pub provider: String,
    pub app_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<TierUsageDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// true = 查询成功, false = 查询失败
    pub success: bool,
    /// 查询失败时的错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// GET /api/usage — 实时查询所有 enabled provider 的官方用量
///
/// 与 /api/providers 不同：本接口不返回状态/配置，而是触发一次实时查询
/// 拉取每个 provider 官方的 quota 数据（5h/周限额/余额等）并合并为统一 DTO。
pub async fn get_usage(AxumState(state): AxumState<Arc<RemoteState>>) -> impl IntoResponse {
    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    // 三份状态都走 headless-safe 接缝：直接 `app_handle.state::<T>()` 在无
    // Tauri 运行时下是 `expect` panic，请求会在连接里炸掉 —— 那样这条路由
    // 在 E2E 里根本无法被验到（也就无法证明它的错误文本过了脱敏闸门）。
    let app_state = app_state(&state);
    let copilot_state = copilot_state(&state);
    let xai_state = xai_state(&state);

    let mut results: Vec<ProviderUsageDto> = Vec::new();

    for app_type in AppType::all() {
        let providers = match app_state.db.get_all_providers(app_type.as_str()) {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "[Remote] Failed to get providers for {}: {e}",
                    app_type.as_str()
                );
                continue;
            }
        };

        for (provider_id, provider) in providers {
            // 只查询启用了 usage_script 的 provider
            if !provider.has_usage_script_enabled() {
                continue;
            }

            let template_type = provider
                .meta
                .as_ref()
                .and_then(|m| m.usage_script.as_ref())
                .and_then(|s| s.template_type.as_deref())
                .unwrap_or("");

            // Copilot provider 必须先有 OAuth 登录；未登录时静默跳过
            if template_type == "github_copilot" {
                let auth = copilot_state.0.read().await;
                if !auth.is_authenticated().await {
                    log::debug!(
                        "[Remote] Skipping Copilot provider {}: not authenticated",
                        provider_id
                    );
                    continue;
                }
            }

            let query_result = commands::query_provider_usage_inner(
                app_state.inner(),
                &copilot_state,
                &xai_state,
                app_type.clone(),
                &provider_id,
            )
            .await;

            let usage_result = match query_result {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[Remote] Usage query failed for {}: {e}", provider_id);
                    results.push(ProviderUsageDto {
                        provider: provider.name.clone(),
                        app_type: app_type.as_str().to_string(),
                        tiers: vec![],
                        updated_at: None,
                        success: false,
                        // usage 脚本的上下文里注入了明文 api_key，脚本一旦回显
                        // 变量就会逐字带到这里 —— 下发前必须过脱敏闸门。
                        error: Some(redact::redact_error(&e.to_string())),
                    });
                    continue;
                }
            };

            if !usage_result.success {
                log::warn!(
                    "[Remote] Usage query returned failure for {}: {:?}",
                    provider_id,
                    usage_result.error
                );
                results.push(ProviderUsageDto {
                    provider: provider.name.clone(),
                    app_type: app_type.as_str().to_string(),
                    tiers: vec![],
                    updated_at: None,
                    success: false,
                    // 同上：脚本返回的错误文本可能含 key，不能原样下发
                    error: usage_result.error.as_deref().map(redact::redact_error),
                });
                continue;
            }

            let data = match usage_result.data {
                Some(d) if !d.is_empty() => d,
                _ => continue,
            };

            let tiers: Vec<TierUsageDto> = data
                .iter()
                .map(|d| TierUsageDto {
                    name: d.plan_name.clone().unwrap_or_default(),
                    used_percent: d.used.unwrap_or(0.0),
                    remaining: d.remaining.unwrap_or(0.0),
                    total: d.total.unwrap_or(0.0),
                    unit: d.unit.clone().unwrap_or_default(),
                    resets_at: d.extra.clone(),
                })
                .collect();

            results.push(ProviderUsageDto {
                provider: provider.name.clone(),
                app_type: app_type.as_str().to_string(),
                tiers,
                updated_at: Some(chrono::Local::now().to_rfc3339()),
                success: true,
                error: None,
            });
        }
    }

    Json(json!({ "providers": results })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderRequest {
    /// Provider ID
    pub provider_id: String,
    /// 应用类型，如 "claude" / "codex" / "gemini"
    pub app_type: String,
}

/// POST /api/provider/test — 对单个供应商执行模型连通性测试
///
/// 与 UI 点击 "测试模型" 按钮调用 `stream_check_provider` Tauri 命令完全一致：
/// 复用同一组 helper（resolve_copilot_auth_override / base_url / claude_api_format）
/// 和 `StreamCheckService::check_with_retry`。返回值也是同一个 `StreamCheckResult`
/// 结构，便于自动化运维脚本（例如先测一下 P1 可用性再决定是否排到队列头部）。
///
/// Body: {"providerId": "p1", "appType": "claude"}
///
/// Response: 成功时直接返回 `StreamCheckResult` JSON 字段；
///           失败时返回 {"success": false, "error": "..."}。
pub async fn test_provider(
    AxumState(state): AxumState<Arc<RemoteState>>,
    Json(body): Json<TestProviderRequest>,
) -> impl IntoResponse {
    use std::str::FromStr;

    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }

    if body.provider_id.is_empty() {
        return remote_error(axum::http::StatusCode::OK, "Missing provider_id");
    }

    let app_type = match AppType::from_str(&body.app_type) {
        Ok(t) => t,
        Err(_) => {
            return remote_error(
                axum::http::StatusCode::OK,
                format!("无效的应用类型: {}", body.app_type),
            );
        }
    };

    let app_state = app_state(&state);
    let copilot_state = copilot_state(&state);

    // 与 Tauri 命令 stream_check_provider 完全相同的逻辑路径：上游 4.0.14 删掉了
    // `stream_check_config` 的持久化（连带 `get_stream_check_config` /
    // `save_stream_check_config` / `save_stream_check_log` 三个 Dao 方法），
    // 命令层因此改用 `StreamCheckConfig::default()`，这里照搬。
    let config = crate::services::stream_check::StreamCheckConfig::default();

    let providers = match app_state.db.get_all_providers(app_type.as_str()) {
        Ok(p) => p,
        Err(e) => {
            return remote_error(axum::http::StatusCode::OK, e);
        }
    };

    let provider = match providers.get(&body.provider_id) {
        Some(p) => p,
        None => {
            return remote_error(
                axum::http::StatusCode::OK,
                format!("供应商 {} 不存在", body.provider_id),
            );
        }
    };

    let base_url_override =
        match commands::resolve_copilot_base_url_override(provider, &copilot_state).await {
            Ok(b) => b,
            Err(e) => {
                return remote_error(axum::http::StatusCode::OK, e);
            }
        };

    let result = match crate::services::stream_check::StreamCheckService::check_with_retry(
        &app_type,
        provider,
        &config,
        base_url_override,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            // 上游响应体逐字透传：鉴权失败时它会把请求里的 Authorization /
            // api_key 片段一起回显出来。远程可见度契约要求这条路径先过脱敏。
            return remote_error(axum::http::StatusCode::OK, e);
        }
    };

    // 上游 4.0.14 一并删掉了 `stream_check_log` 的写入（`save_stream_check_log`
    // 这个 Dao 方法已不存在，只剩 `cleanup_old_stream_check_logs`）：桌面命令
    // `stream_check_provider` 现在也不再落库，这里与它保持一致。

    let status_str = serde_json::to_string(&result.status)
        .unwrap_or_else(|_| "\"unknown\"".to_string())
        .trim_matches('"')
        .to_string();

    log::info!(
        "[Remote] /api/provider/test {} {} → {} ({}ms)",
        app_type.as_str(),
        body.provider_id,
        status_str,
        result.response_time_ms.unwrap_or(0)
    );

    Json(json!({
        "success": true,
        "status": status_str,
        "operational": result.success,
        // 失败时 message 装的是上游错误原文（stream_check 里 `e.to_string()`），
        // 同样逐字下发 —— 与上面的 error 字段走同一道脱敏闸门。
        "message": redact::redact_error(&result.message),
        "response_time_ms": result.response_time_ms,
        "http_status": result.http_status,
        "tested_at": result.tested_at,
        // `model_used` / `error_category` 随上游 4.0.14 一起没了：`StreamCheckResult`
        // 已删掉这两个字段（持久化一并删除）。已核对 `src/remote/html.rs` 内嵌 UI、
        // `src/` 前端与 `src-tauri/src/remote/e2e.rs`，没有任何一处读它们。
        "retry_count": result.retry_count,
    }))
    .into_response()
}

/// `POST /api/proxy/control` 的 CSRF 回归测试。
///
/// 覆盖 handler 依赖的 extractor 契约：非 JSON / 缺 Content-Type 的请求（CORS
/// 简单请求：跨站表单、不带 Content-Type 的 fetch）一律 415，且合法 JSON 调用方
/// 行为不变（`{}` → 停止，`{"start":true}` → 启动）。
///
/// 上面这些用例直接调 `Json` extractor，不经过 `proxy_control` 本体（extractor
/// rejection 在进入函数体前发生），因此单靠它们锁不住 handler 的签名。缺口由
/// `proxy_control_takes_required_json_body` 用编译期断言补上：把 body 参数改回
/// `Option<Json<ProxyControlRequest>>` 会让断言无法编译，整套测试直接构建失败。
// ===========================================================================
// 两段式绑定范围（REST）
// ===========================================================================
//
// 浏览器不是「特等公民」的例外：这三条与桌面端的 `request_bind_scope_change`
// 走**同一个** `BindScopeGate`，只是第二段的载体不同（HTTP 确认 vs 原生弹窗）。
//
// 网络可达入口的安全边界，再说一遍：
//
// - 三条路由都是 `AuthScope::Protected`：非回环来源必须持有配对凭证。
//   本功能不改既有鉴权模型。
// - 这里**没有**「直接设一个档位」的端点。宽窄判定在 gate 里：收紧立即生效，
//   放宽只能返回一个待确认 id。
// - 对「已被完全攻陷、且已配对的客户端」，两段式是提高门槛 + 留痕，不是硬边界。
//   硬边界只有「必须回到那台机器上点原生弹窗」，本设计选择允许浏览器内完成，
//   因此不能声称自己防得住一切。

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindScopeRequestBody {
    /// 目标档位。**注意**：这不是一个直接 setter —— 是否生效由 gate 的宽窄
    /// 判定决定。
    pub scope: BindScope,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindScopeConfirmationBody {
    pub confirmation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindScopeResponse {
    /// 这次调用结束时目标档位是否已经生效。
    pub applied: bool,
    /// 调用结束时**生效中**的档位（待确认时仍是旧值）。
    pub bind_scope: BindScope,
    /// 仅放宽申请返回：第二段的凭据。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation_id: Option<String>,
    /// 仅放宽申请返回：Unix 秒。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl BindScopeResponse {
    fn applied_now(bind_scope: BindScope) -> Self {
        Self {
            applied: true,
            bind_scope,
            confirmation_id: None,
            expires_at: None,
        }
    }
}

/// 取中间件注入的会话身份。
///
/// `Protected` 路由的 `Allow` 出口一定会注入；拿不到说明装配层被改坏了 ——
/// 此时**不能**退回一个默认身份（那会让任意请求都能充当某个会话），必须拒绝。
///
/// `Err` 直接是 axum 的 `Response`（中间件出口），由 axum 本身决定其尺寸；
/// 装箱只会把同一个值挪到堆上并给调用方多加一层解包。
#[allow(clippy::result_large_err)]
fn session_identity(
    identity: Option<Extension<SessionIdentity>>,
) -> Result<SessionIdentity, axum::response::Response> {
    identity.map(|Extension(identity)| identity).ok_or_else(|| {
        remote_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "missing session identity on a protected route",
        )
    })
}

/// 落盘之后、返回之前：让正在跑的 listener 立刻跟上新档位。
///
/// 失败只可能是 bind 失败（比如 Tailscale IP 没了）。此时文件已经是新档位，
/// 下次启动仍会按它来；本次返回 500 让调用方知道「现在还没生效」。
async fn rebind_or_500(
    state: &Arc<RemoteState>,
    scope: BindScope,
) -> Result<(), axum::response::Response> {
    state.rebind_bind_scope(scope).await.map_err(|e| {
        remote_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("bind scope persisted but listeners could not follow: {e}"),
        )
    })
}

/// POST /api/remote/config/bind-scope/request
///
/// 收紧 → `{ applied: true, bindScope: … }`；
/// 放宽 → `{ applied: false, bindScope: <当前>, confirmationId, expiresAt }`。
pub async fn request_bind_scope(
    AxumState(state): AxumState<Arc<RemoteState>>,
    identity: Option<Extension<SessionIdentity>>,
    Json(body): Json<BindScopeRequestBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }
    let identity = match session_identity(identity) {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    match state
        .exposure
        .request_bind_scope_change(body.scope, identity)
    {
        Ok(super::config::BindScopeRequest::Applied(scope)) => {
            if let Err(response) = rebind_or_500(&state, scope).await {
                return response;
            }
            Json(BindScopeResponse::applied_now(scope)).into_response()
        }
        Ok(super::config::BindScopeRequest::Pending {
            confirmation_id,
            expires_at_unix,
        }) => {
            let current = state
                .exposure
                .overlay(&state.config_snapshot().await)
                .bind_scope;
            Json(BindScopeResponse {
                applied: false,
                bind_scope: current,
                confirmation_id: Some(confirmation_id),
                expires_at: Some(expires_at_unix),
            })
            .into_response()
        }
        Err(e) => remote_error(axum::http::StatusCode::BAD_REQUEST, e),
    }
}

/// POST /api/remote/config/bind-scope/confirm
pub async fn confirm_bind_scope(
    AxumState(state): AxumState<Arc<RemoteState>>,
    identity: Option<Extension<SessionIdentity>>,
    Json(body): Json<BindScopeConfirmationBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }
    let identity = match session_identity(identity) {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    match state.exposure.confirm_bind_scope_change(
        &body.confirmation_id,
        identity,
        std::time::Instant::now(),
    ) {
        Ok(scope) => {
            if let Err(response) = rebind_or_500(&state, scope).await {
                return response;
            }
            log::info!("[Remote] bind scope confirmed over REST: {scope:?}");
            Json(BindScopeResponse::applied_now(scope)).into_response()
        }
        Err(e) => remote_error(axum::http::StatusCode::BAD_REQUEST, e),
    }
}

/// POST /api/remote/config/bind-scope/reject
pub async fn reject_bind_scope(
    AxumState(state): AxumState<Arc<RemoteState>>,
    identity: Option<Extension<SessionIdentity>>,
    Json(body): Json<BindScopeConfirmationBody>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !state.running.load(Ordering::SeqCst) {
        return remote_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is stopped",
        );
    }
    let identity = match session_identity(identity) {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    match state.exposure.reject_bind_scope_change(
        &body.confirmation_id,
        identity,
        std::time::Instant::now(),
    ) {
        Ok(()) => Json(json!({ "rejected": true })).into_response(),
        Err(e) => remote_error(axum::http::StatusCode::BAD_REQUEST, e),
    }
}

#[cfg(test)]
mod tests {
    use super::{project_frame, proxy_control, safe_svg_color, ProxyControlRequest, RemoteState};
    use crate::remote::auth::SessionAudience;
    use axum::body::Body;
    use axum::extract::{FromRequest, State as AxumState};
    use axum::http::header::CONTENT_TYPE;
    use axum::http::{Request, StatusCode};
    use axum::response::{IntoResponse, Json, Response};
    use std::sync::Arc;

    /// 一条真实的 provider 切换广播帧（`switch_provider` / `broadcast_provider_switch` 发的同款）。
    const SWITCH_FRAME: &str =
        r#"{"type":"switch","provider_id":"p-secret","name":"Secret Provider"}"#;

    /// `color` 只能是十六进制颜色字面量：白名单两侧都必须钉死。
    ///
    /// 接受的一侧是 CSS 的四种十六进制写法（3/4/6/8 位）；拒绝的一侧里，
    /// `red" onload="alert(1)` 就是这条路由真实的注入形态 —— 它能闭合
    /// `fill="currentColor"` 再挂上新属性，而 `<img src>` 会照样执行。
    #[test]
    fn safe_svg_color_accepts_only_hex_literals() {
        for accepted in ["#fff", "#ffff", "#ffffff", "#ffffffff"] {
            assert_eq!(
                safe_svg_color(accepted),
                Some(accepted),
                "{accepted:?} 是合法的十六进制颜色，必须原样放行"
            );
        }
        for rejected in ["red", "#ff", "#fffff", "#ggg", r#"red" onload="alert(1)"#] {
            assert_eq!(
                safe_svg_color(rejected),
                None,
                "{rejected:?} 不是十六进制颜色字面量，必须判否"
            );
        }
    }

    /// 已鉴权连接：帧必须**逐字**透传。
    ///
    /// 钉的是「不损失任何东西」——降级投影一旦误伤已鉴权连接，远程 Web UI 会
    /// 收不到 provider 名，而这类退化在 UI 上只表现为「名字空着」，测试照样可能
    /// 全绿。所以这里断言的是**字节相等**，不是「含有某些字段」。
    #[test]
    fn project_frame_passes_authenticated_frames_through_verbatim() {
        assert_eq!(
            project_frame(SWITCH_FRAME, SessionAudience::Authenticated),
            Some(SWITCH_FRAME.to_string()),
            "已鉴权连接的 switch 帧必须逐字透传"
        );
    }

    /// 匿名（本机无会话）连接：switch 帧必须被剥成只剩 type 与降级标记。
    ///
    /// 本机任何无凭据调用方拿到的是**降级流**而不是
    /// 401，所以它确实能看到「发生了切换」，但看不到**切到哪一个、叫什么** ——
    /// 后两者正是判定格第 7 格（Loopback / 无 Origin / 无代理信号 + `Cred::None`）
    /// 放行时绝不该外泄的东西。
    #[test]
    fn project_frame_strips_provider_details_for_anonymous() {
        let projected = project_frame(SWITCH_FRAME, SessionAudience::AnonymousLoopback)
            .expect("switch 帧必须仍被下发 —— 匿名降级不是静默丢帧");
        let parsed: serde_json::Value = serde_json::from_str(&projected)
            .unwrap_or_else(|e| panic!("降级帧必须是合法 JSON（{projected:?}）：{e}"));

        assert_eq!(parsed["type"], "switch", "type 必须保留，前端据此刷新");
        assert_eq!(
            parsed["degraded"], true,
            "降级标记必须显式带上，否则前端无法区分「切了但看不到细节」与「没切」"
        );
        assert!(
            parsed.get("provider_id").is_none(),
            "匿名流里不得出现 provider_id（实际 {parsed}）"
        );
        assert!(
            parsed.get("name").is_none(),
            "匿名流里不得出现 provider 名（实际 {parsed}）"
        );
        assert!(
            !projected.contains("Secret Provider"),
            "provider 名字连文本都不得残留：{projected}"
        );
    }

    /// 匿名连接上**非** switch 的帧类型原样下发：降级只针对 provider 细节。
    ///
    /// 例如 `{"type":"shutdown"}` —— 漏掉它等于让匿名客户端在服务停止时永远
    /// 挂着一条死连接。
    #[test]
    fn project_frame_keeps_other_frame_types_for_anonymous() {
        let raw = r#"{"type":"shutdown"}"#;
        assert_eq!(
            project_frame(raw, SessionAudience::AnonymousLoopback),
            Some(raw.to_string()),
            "非 switch 帧不得被降级逻辑波及"
        );
    }

    /// 匿名档：`reorder` 帧同样必须被降级。
    ///
    /// reorder 帧带 `moved_id`（provider id）。漏掉这一条，同机无凭据调用方就能
    /// 从「谁被挪了」读到供应商拓扑 —— 那正是 switch 降级要挡住的东西；
    /// 换个帧类型就能绕过去，等于分级形同虚设。
    #[test]
    fn project_frame_strips_reorder_details_for_anonymous() {
        let raw = r#"{"type":"reorder","app_type":"claude","moved_id":"secret-provider-id","position":2}"#;
        let projected = project_frame(raw, SessionAudience::AnonymousLoopback)
            .expect("reorder 帧必须仍被下发 —— 降级不是静默丢帧");
        let parsed: serde_json::Value = serde_json::from_str(&projected)
            .unwrap_or_else(|e| panic!("降级帧必须是合法 JSON（{projected:?}）：{e}"));

        assert_eq!(parsed["type"], "reorder", "type 必须保留，前端据此刷新顺序");
        assert_eq!(
            parsed["degraded"], true,
            "降级标记必须显式带上，否则前端无法区分「降级」与「无事件」"
        );
        assert!(
            parsed.get("moved_id").is_none(),
            "匿名流里不得出现 moved_id（实际 {parsed}）"
        );
        assert!(
            parsed.get("app_type").is_none(),
            "匿名流里不得出现 app_type（实际 {parsed}）"
        );
        assert!(
            parsed.get("position").is_none(),
            "匿名流里不得出现 position（实际 {parsed}）"
        );
        assert!(
            !projected.contains("secret-provider-id"),
            "provider id 连文本都不得残留：{projected}"
        );
    }

    /// 已鉴权档：`reorder` 帧同样逐字透传。
    ///
    /// 前端需要 `moved_id` / `position` 才能按精确变化刷新；被降级成
    /// `{"type":"reorder","degraded":true}` 会让已鉴权调用方也丢掉这些字段。
    #[test]
    fn project_frame_passes_authenticated_reorder_frames_through_verbatim() {
        let raw = r#"{"type":"reorder","app_type":"claude","moved_id":"p-1","position":2}"#;
        assert_eq!(
            project_frame(raw, SessionAudience::Authenticated),
            Some(raw.to_string()),
            "已鉴权连接的 reorder 帧必须逐字透传"
        );
    }

    /// 匿名档：`failover_changed` 帧同样必须被降级。
    ///
    /// 这一帧带 `app_type` / `enabled` —— 前者是供应商拓扑所在的维度，
    /// 后者是这个应用当前的调度语义。降级后只保留「开关变过」这件事本身，
    /// 前端据此失效缓存并不需要这两个字段。
    #[test]
    fn project_frame_strips_failover_details_for_anonymous() {
        let raw = r#"{"type":"failover_changed","app_type":"claude","enabled":true}"#;
        let projected = project_frame(raw, SessionAudience::AnonymousLoopback)
            .expect("failover_changed 帧必须仍被下发 —— 降级不是静默丢帧");
        let parsed: serde_json::Value = serde_json::from_str(&projected)
            .unwrap_or_else(|e| panic!("降级帧必须是合法 JSON（{projected:?}）：{e}"));

        assert_eq!(
            parsed["type"], "failover_changed",
            "type 必须保留，前端据此失效缓存"
        );
        assert_eq!(
            parsed["degraded"], true,
            "降级标记必须显式带上，否则前端无法区分「降级」与「无事件」"
        );
        assert!(
            parsed.get("app_type").is_none(),
            "匿名流里不得出现 app_type（实际 {parsed}）"
        );
        assert!(
            parsed.get("enabled").is_none(),
            "匿名流里不得出现 enabled（实际 {parsed}）"
        );
        assert!(
            !projected.contains("claude"),
            "app_type 的字面量连文本都不得残留：{projected}"
        );
    }

    /// 已鉴权档：`failover_changed` 帧必须逐字透传。
    #[test]
    fn project_frame_passes_authenticated_failover_frames_through_verbatim() {
        let raw = r#"{"type":"failover_changed","app_type":"claude","enabled":false}"#;
        assert_eq!(
            project_frame(raw, SessionAudience::Authenticated),
            Some(raw.to_string()),
            "已鉴权连接的 failover_changed 帧必须逐字透传"
        );
    }

    /// 非法 JSON：跳过这一帧（`None`），但绝不因此断流。
    ///
    /// 广播端不止 handler 一处（tray / failover_switch / provider 切换命令），
    /// 任何一处写出非 JSON 的载荷都不该把匿名客户端的连接掐掉。
    #[test]
    fn project_frame_skips_invalid_json_instead_of_breaking_the_stream() {
        assert_eq!(
            project_frame("not json at all", SessionAudience::AnonymousLoopback),
            None,
            "匿名档遇到非法 JSON 必须返回 None（跳过该帧）"
        );
        assert_eq!(
            project_frame("not json at all", SessionAudience::Authenticated),
            Some("not json at all".to_string()),
            "已鉴权连接逐字透传，不做解析"
        );
    }

    /// 编译期签名锁：只有当 `handler` 的第二个参数**恰好**是必填的
    /// `Json<ProxyControlRequest>` 时，这个 where 约束才成立。
    ///
    /// 为什么要单独锁签名：`Option<T>` 在 axum 里实现了 `FromRequest`，
    /// 它把所有 extractor rejection（含 415）吞成 `None`
    /// （axum-core `extract/mod.rs`）。所以 `Option<Json<..>>` 和 `Json<..>`
    /// 对上面那些直接测 extractor 的用例是等价的，但前者会让 CORS 简单请求
    /// （跨站 `<form method="POST">`、不带 Content-Type 的 `fetch`）拿到
    /// `None` → 落到 `unwrap_or(false)` → 触发 `stop_with_restore()`，
    /// 即 CSRF 洞重新打开，而测试照样全绿。函数项的 `Fn` 实现对参数类型是精确的，
    /// 没有隐式转换，所以这里能在编译期把签名钉死。
    ///
    /// 之所以能做到这一点而不构造 `RemoteState`：`RemoteState` 持有
    /// `tauri::AppHandle`（默认 `Wry`），而 `tauri::test::mock_app()` 给的是
    /// `AppHandle<MockRuntime>`，两者不是同一个类型，构造 router 级测试会先卡在
    /// 这里。而本断言只用到类型名，不构造任何值。
    fn assert_takes_required_json_body<F, Fut>(_handler: F)
    where
        F: Fn(AxumState<Arc<RemoteState>>, Json<ProxyControlRequest>) -> Fut,
    {
    }

    /// CSRF 回归（签名级）：`proxy_control` 的 body 参数必须是必填的
    /// `Json<ProxyControlRequest>`，不能退回 `Option<Json<..>>`。
    ///
    /// 改回 `Option<Json<..>>`（CSRF 修复被静默撤销）时本测试无法编译 ——
    /// `cargo test` 会在这里直接失败，而不是让上面 6 个用例误报通过。
    #[test]
    fn proxy_control_takes_required_json_body() {
        assert_takes_required_json_body(proxy_control);
    }

    /// 构造一个 POST /api/proxy/control 请求（Content-Type / body 均可指定，
    /// 用于复现跨站表单提交与不带 Content-Type 的 fetch 这两种 CORS 简单请求）。
    fn req(content_type: Option<&'static str>, body: &'static str) -> Request<Body> {
        let mut builder = Request::builder().method("POST").uri("/api/proxy/control");
        if let Some(ct) = content_type {
            builder = builder.header(CONTENT_TYPE, ct);
        }
        builder.body(Body::from(body)).unwrap()
    }

    /// 模拟 handler 的 `Json<ProxyControlRequest>` extractor（不构造 RemoteState）。
    async fn extract_body(req: Request<Body>) -> Result<ProxyControlRequest, Response> {
        Json::<ProxyControlRequest>::from_request(req, &())
            .await
            .map(|Json(b)| b)
            .map_err(|e| e.into_response())
    }

    /// 断言该请求被 extractor 拒绝，并返回拒绝时的 HTTP 状态码。
    async fn rejection_status(
        content_type: Option<&'static str>,
        body: &'static str,
    ) -> StatusCode {
        match extract_body(req(content_type, body)).await {
            Ok(_) => panic!("request must be rejected, but body was accepted"),
            Err(resp) => resp.status(),
        }
    }

    /// CSRF 回归：跨站表单提交是 CORS 简单请求，不会带 JSON Content-Type，
    /// 必须被 415 挡下，否则会触发 stop_with_restore()（停代理 + 清 enabled + 删备份）。
    #[tokio::test]
    async fn form_post_is_rejected_with_415() {
        let status = rejection_status(Some("application/x-www-form-urlencoded"), "").await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    /// CSRF 回归：不带 Content-Type 的 `fetch(url, {method:'POST'})` 同样是简单请求。
    #[tokio::test]
    async fn missing_content_type_is_rejected_with_415() {
        let status = rejection_status(None, "").await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    /// text/plain 也必须被拒。
    #[tokio::test]
    async fn text_plain_is_rejected_with_415() {
        let status = rejection_status(Some("text/plain"), "{}").await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    /// 合法调用方行为不变：`{}` 仍然解析为 start = false（停止代理）。
    #[tokio::test]
    async fn empty_json_object_still_parses_as_stop() {
        let parsed = extract_body(req(Some("application/json"), "{}"))
            .await
            .expect("`{}` must still parse");
        assert!(!parsed.start, "`{{}}` must keep meaning 'stop proxy'");
    }

    /// 合法调用方行为不变：`{"start": true}` 仍然解析为启动代理。
    #[tokio::test]
    async fn start_true_still_parses() {
        let parsed = extract_body(req(Some("application/json"), r#"{"start":true}"#))
            .await
            .expect("start:true must parse");
        assert!(parsed.start);
    }

    /// 合法调用方行为不变：`{"start": false}` 仍然解析为停止代理。
    #[tokio::test]
    async fn start_false_still_parses_as_stop() {
        let parsed = extract_body(req(Some("application/json"), r#"{"start":false}"#))
            .await
            .expect("start:false must parse");
        assert!(!parsed.start);
    }

    /// 本文件自身的源码，用来在测试期做静态扫描（见下面两个脱敏回归用例）。
    const HANDLERS_SRC: &str = include_str!("handlers.rs");

    /// 从 `sig` 起按大括号配平截出一个函数体，返回其源码文本。
    ///
    /// 故意不做词法分析：这两个 handler 里出现的花括号都来自 `{}` 格式化占位符
    /// 或真正的代码块，本身配平，所以朴素计数就够。真正的风险是「截歪了却
    /// 悄悄通过」，由两个用例各自的尾部哨兵（sentinel）断言挡住。
    fn fn_body(src: &str, sig: &str) -> String {
        let start = src
            .find(sig)
            .unwrap_or_else(|| panic!("signature not found in handlers.rs: {sig}"));
        let rest = &src[start..];
        let open = rest
            .find('{')
            .unwrap_or_else(|| panic!("no body brace after: {sig}"));
        let mut depth = 0usize;
        // `char_indices()` 的偏移是相对 `body` 这段切片自身的，所以拼回
        // `rest` 时必须加上 `open` —— 少加会把函数体截掉前 `open` 个字节，
        // 症状是「少了函数尾部那一小截」，很容易被当成正常。
        let body = &rest[open..];
        for (i, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return body[..=i].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced braces in body of: {sig}");
    }

    /// 判断一行是不是「把某个值写进错误载荷」的字段初始化。
    ///
    /// 必须在**整行**里找，不能只认行首：真正漏检过的那一行长得是
    /// `return Json(json!({"success": false, "error": e.to_string()}))...`，
    /// `"error":` 夹在行中间。只匹配行首的写法会把这个洞原样放过。
    ///
    /// 匹配 `"error":` / `"message":`（`json!` 的字符串键）与 `error:` /
    /// `message:`（DTO 字段名）。匹配的是带引号的完整键，不是子串，所以键名里
    /// 含 `error` 但另起一个词的（例如 `"error_counts":`）不会被误伤。
    fn is_error_payload_line(line: &str) -> bool {
        line.contains("\"error\":")
            || line.contains("\"message\":")
            || line.contains("error:")
            || line.contains("message:")
    }

    /// 不需要脱敏的合法行：`None`（没有错误可报）。
    fn is_allowed_without_redaction(line: &str) -> bool {
        let t = line.trim();
        t == "error: None," || t == "error: None" || t == "message: None,"
    }

    /// 脱敏回归（源码级）：`/api/usage` 的 `get_usage` 里，每一个把值写进
    /// 错误载荷的字段，都必须经过 `redact_error`。
    ///
    /// 这条锁的是真正出过的那个洞：错误分支在同一个函数里各自拼
    /// `json!`，漏掉一处就把内部错误原文（可能夹带 base_url / 凭据片段）
    /// 逐字交给了远程调用方，而同文件其他分支全都在脱敏，测试照样全绿。
    #[test]
    fn get_usage_error_fields_go_through_redaction() {
        let body = fn_body(HANDLERS_SRC, "pub async fn get_usage");
        // 尾部哨兵：确认没有把别的函数截进来。
        assert!(
            body.contains("\"providers\": results"),
            "get_usage body extraction looks truncated: {body}"
        );

        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("//") || !is_error_payload_line(line) {
                continue;
            }
            if is_allowed_without_redaction(line) {
                continue;
            }
            assert!(
                line.contains("redact_error"),
                "get_usage writes an error payload without going through redact_error \
                 — the upstream text would reach the caller verbatim:\n  {}\n(body: {})",
                line.trim(),
                body,
            );
        }
    }

    /// 脱敏回归（源码级）：`/api/provider/test` 的 `test_provider` 同上。
    ///
    /// 历史上这条路径上的 `resolve_copilot_base_url_override` 失败分支就是把
    /// `e.to_string()` 裸塞进 `json!` 的那一处。
    #[test]
    fn test_provider_error_fields_go_through_redaction() {
        let body = fn_body(HANDLERS_SRC, "pub async fn test_provider");
        // 尾部哨兵：确认没有把后面的 handler 截进来。取响应载荷里**最后一个**键
        // （`model_used` / `error_category` 随上游 4.0.14 删除后，末键是 retry_count）。
        assert!(
            body.contains("\"retry_count\""),
            "test_provider body extraction looks truncated: {body}"
        );

        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("//") || !is_error_payload_line(line) {
                continue;
            }
            if is_allowed_without_redaction(line) {
                continue;
            }
            assert!(
                line.contains("redact_error"),
                "test_provider writes an error payload without going through redact_error \
                 — the upstream text would reach the caller verbatim:\n  {}\n(body: {})",
                line.trim(),
                body,
            );
        }
    }
}
