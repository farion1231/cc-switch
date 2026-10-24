//! 配对端点：`POST /pairing/request`、`/pairing/poll`、`/pairing/approve`、
//! `/pairing/revoke`。
//!
//! 前两条属于「尚未持有凭据的一方」—— 它们是拿到凭据的唯一入口，因此凭据
//! 只能从这两条路上产生，且 `pollToken` **只**认 `Authorization: Pairing <token>`
//! 一个来源。查询参数不参与鉴权：URL 会进浏览器历史、进代理日志、进 Referer，
//! 一次性凭据一旦出现在 URL 里就已经泄了。
//!
//! 后两条是桌面端那侧的动作（批准、撤销），因此**只接受 loopback 来源**：
//! 批准就是 human-in-the-loop 的那一环，对局域网开放等于让任何机器给自己批。
//!
//! 凭据的闭环也在这一层：`poll` 交付那一刻把摘要写进 `clients.json`（与
//! `auth_layer` 读同一个目录、同一把 secret），`revoke` 再把它摘掉 —— 两端
//! 共享一份库，撤销才可能**立即**生效。
//!
//! 时间由调用方经 `Instant::now()` 注入；状态判据全在 [`super::state`]，
//! 这里不重算 TTL，只把结果翻译成状态码与响应体。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::state::{PairingError, PairingStore, PollOutcome, PAIRING_TTL, POLL_INTERVAL_SECS};
use crate::remote::auth::{
    build_session_cookie, generate_token, load_clients, save_clients, token_hash, ClientToken,
    REMOTE_LISTENER_IS_TLS, SESSION_COOKIE_NAME,
};
use crate::remote::events::EventSink;

/// 承载凭据的 `Authorization` 方案名。
pub const PAIRING_SCHEME: &str = "Pairing";

/// 会话 cookie 的 `Max-Age`，单位秒（10 年）。
///
/// 与凭据库同寿而不是配对码的 10 分钟 TTL：cookie 的判据是**库里那条记录还在不在**，
/// 记录在，浏览器重启就不需要重新配对。真正决定它什么时候失效的是 `POST /pairing/revoke`
/// —— 摘掉记录的那一刻，两个摘要一起失效（见 [`super::handlers::issue_credential`]）。
const SESSION_MAX_AGE_SECS: i64 = 10 * 365 * 24 * 60 * 60;

/// 两条配对路由的共享状态。
///
/// `config_dir` / `secret` 让配对子系统**自己**能把签发的凭据写进与鉴权
/// 中间件同一个目录下的 `clients.json`：两端共用一把 secret、同一个库，中间
/// 件才认配对交付出来的 token。缺了这两个字段，配对交付出去的东西对
/// `Protected` 路由是不存在的 —— 那不叫「另一条认证通道」，叫一条死路。
#[derive(Clone)]
pub struct PairingApiState {
    pub store: PairingStore,
    pub sink: Arc<dyn EventSink>,
    /// 凭据落盘目录，与 `auth_layer` 读的是同一个。
    pub config_dir: PathBuf,
    /// `config_dir/remote-auth/secret`，签发时算摘要、撤销时反查摘要都用它。
    pub secret: [u8; 32],
}

/// `POST /pairing/request` 的请求体。
///
/// `client_name` 必填；`requestedScopes` 缺省为空。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingRequestBody {
    pub client_name: String,
    #[serde(default)]
    pub requested_scopes: Vec<String>,
}

/// `POST /pairing/poll` 的请求体。
///
/// **故意是空的**，并且 `deny_unknown_fields`：pollToken 只从 header 读，
/// 请求体里出现任何字段都是契约错误，直接 422。把这一点写进类型而不是写进
/// 注释 —— 改成「顺手也支持 body」会先在编译期红，而不是在运行时红。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingPollBody {}

/// `POST /pairing/approve` 的请求体：桌面端把用户敲的配对码送回来。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingApproveBody {
    pub user_code: String,
    /// 用户在界面上勾过「我已知悉这不是本机账号」吗。
    ///
    /// 缺省 `false`：这条路由本来就不该被当成生产批准路径（真正的批准走
    /// `approve_pairing` 命令，它会把证据**重算**一遍），而缺省成 `true` 等于给一条
    /// 裸 HTTP 接口开了后门 —— 绕过命令层就把外来账号批了。
    #[serde(default)]
    pub confirmed_foreign_account: bool,
}

/// `POST /pairing/revoke` 的请求体：按 clientId 撤销一条已交付凭据。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingRevokeBody {
    pub client_id: String,
}

/// `POST /pairing/request` 的成功响应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequestResponse {
    pub request_id: String,
    /// 6 位人读配对码，桌面端拿它把请求对应到人。
    pub user_code: String,
    /// 一次性轮询凭据，只能放进 `Authorization: Pairing <token>`。
    pub poll_token: String,
    /// RFC3339 时刻，仅供 CLI 显示倒计时。
    pub expires_at: String,
    pub poll_interval_seconds: u64,
}

/// `POST /pairing/poll` 的响应。
///
/// `client_token` / `error` 都是 `skip_serializing_if`：失效与过期的响应体里
/// **不得**出现任何 token 形态的字段名，否则等于把「曾经有过凭据」泄漏出去。
/// `client_id` 同理 —— 它只在交付那一刻出现。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPollResponse {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
    /// 凭据的稳定标识，后续 `POST /pairing/revoke` 按它撤销。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

/// 取来源 IP。
///
/// `ConnectInfo` 由 `into_make_service_with_connect_info` 装配。在它装上之前
/// 这里拿到的是 `None`：**不能**因此跳过限流，否则等于没有限流。所以退回一个
/// 固定哨兵地址，让所有来源挤在同一个桶里 —— 宁可一起被限，也不放过任何一个。
fn peer_ip(peer: Option<&ConnectInfo<SocketAddr>>) -> IpAddr {
    peer.map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

/// 取来源地址，供审批证据使用。
///
/// 拿不到 `ConnectInfo` 时退回哨兵 `0.0.0.0:0`：这个地址查身份必然查不到，
/// 于是证据落到 `Unavailable` —— 与 [`peer_ip`] 的 fail-closed 同一条规则。
/// 绝不因为「取不到来源」就跳过取证据，那等于凭空造一条可信身份出来。
fn peer_addr(peer: Option<&ConnectInfo<SocketAddr>>) -> SocketAddr {
    peer.map(|ci| ci.0)
        .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
}

/// 从 `Authorization: Pairing <pollToken>` 里取一次性凭据。
///
/// **唯一**入口。签名只收 `HeaderMap`，因此结构上就够不到查询参数 ——
/// `?pollToken=` 走不到这里，也就不会被当成凭据。
pub fn poll_token_from_headers(headers: &HeaderMap) -> Option<&str> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case(PAIRING_SCHEME) {
        return None;
    }
    let token = token.trim();
    // 拒绝空值，也拒绝 "Pairing a b" 这种夹带 —— 只认恰好一个凭据。
    if token.is_empty() || token.split_whitespace().count() != 1 {
        return None;
    }
    Some(token)
}

/// 把一个墙钟时刻渲染成 RFC3339。
///
/// 这只是给 CLI 显示倒计时的字段；配对码是否过期由 [`super::state`] 用
/// `Instant` 单调时钟判定，两者不共享判据。
fn iso8601(at: SystemTime) -> String {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

/// `POST /pairing/request`：发起一次配对，并通知桌面端有请求待批。
pub async fn pairing_request(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    Json(body): Json<PairingRequestBody>,
) -> Response {
    let now = Instant::now();
    let scopes: Vec<&str> = body.requested_scopes.iter().map(String::as_str).collect();

    let ticket =
        match state
            .store
            .request_from_peer(peer_ip(peer.as_ref()), &body.client_name, &scopes, now)
        {
            Ok(ticket) => ticket,
            Err(PairingError::RateLimited) => {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({ "error": "rate_limited" })),
                )
                    .into_response();
            }
            // request 路径当前不会走到 Denied，但枚举必须穷尽：真发生了也不许
            // 悄悄放行一条没签发凭据的响应。
            Err(PairingError::Denied) => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "denied" })),
                )
                    .into_response();
            }
            // 同理：`Unconfirmed` 只由批准闸门产出，request 路径走到这里也是
            // 折叠成同一个出口，不给发起方多一个可区分的分支。
            Err(PairingError::Unconfirmed) => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "denied" })),
                )
                    .into_response();
            }
        };

    // 桌面端靠这条事件把「有人要配对」推给人看，字段与审批 UI 一一对应。
    //
    // 证据是**真查出来的**：`tailscale whois <对端 IP>` 的 `Node.User.ID` 与
    // `tailscale status --json` 的 `Self.UserID` 比对得出 `is_foreign_account`
    // （见 [`super::build_evidence`]）。写死一条回环证据会把「外来账号默认拒绝」
    // 这条分支变成死代码 —— 对端是不是 tailnet 节点根本不看。
    let evidence = super::evidence_for(peer_addr(peer.as_ref())).await;
    // 判定记回库里，让「非本人默认拒绝」这道防线属于状态机自己 —— 只存在事件里的话，
    // 任何一次不带证据的 `approve` 都能把它洗掉。
    state
        .store
        .note_foreign_account(&ticket.user_code, evidence.is_foreign_account);
    super::emit_approval(
        state.sink.as_ref(),
        &ticket.request_id,
        &ticket.client_name,
        &ticket.user_code,
        evidence,
    );

    (
        StatusCode::OK,
        Json(PairingRequestResponse {
            request_id: ticket.request_id.clone(),
            user_code: ticket.user_code.clone(),
            poll_token: ticket.poll_token.clone(),
            expires_at: iso8601(SystemTime::now() + PAIRING_TTL),
            poll_interval_seconds: POLL_INTERVAL_SECS,
        }),
    )
        .into_response()
}

/// `POST /pairing/poll`：凭 `Authorization: Pairing <pollToken>` 换长期凭据。
pub async fn pairing_poll(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(_body): Json<PairingPollBody>,
) -> Response {
    // 先认凭据，再认限流：没有凭据的请求没有资格消耗限流配额，也不该靠
    // 429/401 的差异来区分「凭据对不对」。
    let Some(token) = poll_token_from_headers(&headers) else {
        return unauthorized();
    };

    let outcome = state
        .store
        .poll_from_peer(token, peer_ip(peer.as_ref()), Instant::now());

    // 凭据一交付就必须立刻进库：客户端拿到 token 的下一发请求往往紧跟着来，
    // 「先返回、后落盘」中间那一瞬会被判 401，看起来就是配对偶发失败。
    let mut session_cookie: Option<String> = None;
    let outcome = match outcome {
        PollOutcome::Approved {
            token,
            client_id,
            client_name,
        } => match issue_credential(&state, &client_name, &token) {
            Ok(value) => {
                session_cookie = Some(build_session_cookie(
                    &value,
                    SESSION_MAX_AGE_SECS,
                    REMOTE_LISTENER_IS_TLS,
                ));
                PollOutcome::Approved {
                    token,
                    client_id,
                    client_name,
                }
            }
            // 落库失败就不能把凭据交出去：交出去的就是一把中间件永远不认的
            // 死 token，客户端拿着它一直 401 且毫无线索。
            Err(e) => {
                log::error!("[Remote] pairing: 凭据落库失败，本次交付作废: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "credential_persist_failed" })),
                )
                    .into_response();
            }
        },
        other => other,
    };

    let (status, payload) = poll_response(outcome);
    let mut response = (status, Json(payload)).into_response();
    // 只有交付凭据的那一次响应才带 cookie：pending / 过期 / 被拒的响应带上它
    // 等于凭空发一把谁都没批准的凭据。
    if let Some(cookie) = session_cookie {
        if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
            response
                .headers_mut()
                .insert(axum::http::header::SET_COOKIE, value);
        } else {
            log::error!(
                "[Remote] pairing: 会话 cookie 无法写入响应头，cookie 名 {SESSION_COOKIE_NAME} 不可用"
            );
        }
    }
    response
}

/// 把刚交付的凭据登记进凭据库：**只落摘要**，明文不出这一层。
///
/// 返回会话 cookie 的**明文**（只这一次，调用方拿它拼 `Set-Cookie` 后即丢弃）：
/// cookie 值与 header token 一同签发但取值不同，两个摘要存在**同一条**记录上 ——
/// 撤销摘掉这一条记录，两个摘要同时消失，不需要「cookie 撤销表」这种第二份状态。
fn issue_credential(
    state: &PairingApiState,
    client_name: &str,
    token: &str,
) -> Result<String, String> {
    let mut clients = load_clients(&state.config_dir).map_err(|e| e.to_string())?;
    let session_value = generate_token();
    let mut entry = ClientToken::new(client_name, &state.secret, token);
    entry.cookie_hash = Some(token_hash(&state.secret, &session_value));
    // 同一 token 重复交付时只留最新一条，免得库里堆着早已作废的摘要。
    // 比摘要经 `ClientDigest::ct_eq`（恒时）—— 裸 `[u8; 32]` 在这一层拿不到。
    let issued = entry.token_hash;
    clients.retain(|c| !c.token_hash.ct_eq(&issued));
    clients.push(entry);
    save_clients(&state.config_dir, &clients).map_err(|e| e.to_string())?;
    Ok(session_value)
}

/// 把 `client_id` 对应的凭据从库里摘掉。返回是否摘到了。
fn revoke_credential(state: &PairingApiState, client_id: &str) -> Result<bool, String> {
    // 状态机交出的是那条凭据的**摘要**，不是明文：交付那一刻的明文是随机生成的，
    // store 里没有任何字段能把它重算出来，也不该重算。凭据库里存的本来就是这个
    // 摘要，摘要对摘要逐条经 `ct_eq` 比。
    let Some(digest) = state.store.revoke_by_client_id(client_id, Instant::now()) else {
        return Ok(false);
    };
    let mut clients = load_clients(&state.config_dir).map_err(|e| e.to_string())?;
    let before = clients.len();
    clients.retain(|c| !c.token_hash.ct_eq(&digest));
    if clients.len() == before {
        return Ok(false);
    }
    save_clients(&state.config_dir, &clients).map_err(|e| e.to_string())?;
    Ok(true)
}

/// `POST /pairing/approve`：桌面端批准一条待审请求。
///
/// **只接受 loopback 来源**。批准是 human-in-the-loop 的那一环，一旦这条路由
/// 对局域网可达，任何一台机器都能给自己批掉配对请求 —— 那配对就只剩了个形式。
/// 拿不到 `ConnectInfo` 一律按非本机处理（与 [`peer_ip`] 同一条 fail-closed 规则）。
pub async fn pairing_approve(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(body): Json<PairingApproveBody>,
) -> Response {
    if let Some(denied) = require_local_desktop(peer.as_ref(), &headers) {
        return denied;
    }
    match state.store.approve(
        &body.user_code,
        body.confirmed_foreign_account,
        Instant::now(),
    ) {
        // 明文在这一刻存在，随后即被状态机丢弃，响应体里也不带 —— 批准方
        // 要的只是「批了没有」，凭据由发起方自己 poll 走。
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "approved": true })),
        )
            .into_response(),
        // 错码 / 已过期 / 非待审折叠成同一个出口，不给发起方 oracle。
        Err(PairingError::Denied) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "denied" })),
        )
            .into_response(),
        // `PairingStore::approve` 只产出 `Denied` / `Unconfirmed`，approve 路径
        // 走不到限流这一支；枚举必须穷尽，真发生了也不许把它降级成一个 2xx。
        Err(PairingError::RateLimited) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "rate_limited" })),
        )
            .into_response(),
        // 外来账号且没交回确认时状态机会走到这一条。与上面折叠成同一个出口：
        // 这条路由只接受 loopback 来源，调用方是桌面端自己而不是配对请求的发起
        // 方，可区分也不会变成 oracle；真要区分提示，命令层已经把两种情况分开报。
        Err(PairingError::Unconfirmed) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "denied" })),
        )
            .into_response(),
    }
}

/// `POST /pairing/revoke`：撤销一条已交付的长期凭据。
///
/// 与 [`pairing_approve`] 同一条 loopback 限制：撤销是桌面端在设备管理界面
/// 上做的动作，不是一条对局域网开放的接口。
pub async fn pairing_revoke(
    state: State<PairingApiState>,
    peer: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(body): Json<PairingRevokeBody>,
) -> Response {
    if let Some(denied) = require_local_desktop(peer.as_ref(), &headers) {
        return denied;
    }
    match revoke_credential(&state, &body.client_id) {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({ "revoked": true }))).into_response(),
        // 「本来就没有可撤销的凭据」不是错误：撤销是幂等的，重复点一次同样
        // 该成功，否则界面上的「撤销」按钮会在重试时变成故障。
        Ok(false) => (
            StatusCode::OK,
            Json(serde_json::json!({ "revoked": false })),
        )
            .into_response(),
        Err(e) => {
            log::error!("[Remote] pairing: 撤销落库失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "credential_revoke_failed" })),
            )
                .into_response()
        }
    }
}

/// 批准 / 撤销只许本机。返回 `Some` 表示已被拒绝。
///
/// **只看 `ConnectInfo` 的对端地址是不够的**：本机 HTTP 代理（Clash 之类）转发
/// 进来的请求，对端地址同样是 `127.0.0.1` —— 那一格一旦只看 peer，任何人只要让
/// 请求经过本机代理就能从「局域网」变成「本机」，human-in-the-loop 的那一环
/// 直接失效。因此这里与判定表第三列取同一个判据：**携带任一代理信号头就不是
/// 桌面端自己发的那一发**。
fn require_local_desktop(
    peer: Option<&ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
) -> Option<Response> {
    match peer {
        Some(ci) if ci.0.ip().is_loopback() && !crate::remote::auth::has_proxy_signal(headers) => {
            None
        }
        _ => Some(
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "local_desktop_only" })),
            )
                .into_response(),
        ),
    }
}

/// 无凭据的响应。字段名里刻意不带 token 形态。
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(PairingPollResponse {
            status: "unauthorized",
            client_token: None,
            client_id: None,
            error: Some("unauthorized"),
        }),
    )
        .into_response()
}

/// `PollOutcome` → (状态码, 响应体)。
///
/// 只有 `Approved` 会带 `client_token` / `client_id`；其余一律不带。
fn poll_response(outcome: PollOutcome) -> (StatusCode, PairingPollResponse) {
    let status = match &outcome {
        PollOutcome::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        PollOutcome::Invalid => StatusCode::UNAUTHORIZED,
        _ => StatusCode::OK,
    };

    let payload = match outcome {
        PollOutcome::Pending => PairingPollResponse {
            status: "pending",
            client_token: None,
            client_id: None,
            error: None,
        },
        PollOutcome::Approved {
            token, client_id, ..
        } => PairingPollResponse {
            status: "approved",
            client_token: Some(token),
            client_id: Some(client_id),
            error: None,
        },
        PollOutcome::Expired => PairingPollResponse {
            status: "expired",
            client_token: None,
            client_id: None,
            error: None,
        },
        PollOutcome::Rejected => PairingPollResponse {
            status: "rejected",
            client_token: None,
            client_id: None,
            error: None,
        },
        PollOutcome::RateLimited => PairingPollResponse {
            status: "rate_limited",
            client_token: None,
            client_id: None,
            error: Some("rate_limited"),
        },
        PollOutcome::Invalid => PairingPollResponse {
            status: "unauthorized",
            client_token: None,
            client_id: None,
            error: Some("unauthorized"),
        },
    };

    (status, payload)
}

/// 四条配对路由。
pub fn router(state: PairingApiState) -> Router<PairingApiState> {
    Router::new()
        .route("/pairing/request", post(pairing_request))
        .route("/pairing/poll", post(pairing_poll))
        .route("/pairing/approve", post(pairing_approve))
        .route("/pairing/revoke", post(pairing_revoke))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::events::RecordingSink;
    use serde_json::{json, Value};

    /// 每条用例一个独立临时配置目录：凭据库与 secret 落在这里，绝不碰真实
    /// `~/.cc-switch`。
    fn api() -> (PairingApiState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("isolated config dir");
        let secret = crate::remote::auth::load_or_create(dir.path())
            .expect("secret must be created in the isolated dir");
        let state = PairingApiState {
            // 状态机与凭据库必须共用同一把 secret —— 撤销要跨的那条边界靠的就是
            // 两边算出同一个摘要。这里用 `PairingStore::new()`（惰性随机）的话，
            // 跨模块的撤销用例会红，而状态机自己的用例照样全绿。
            store: PairingStore::with_secret(secret),
            sink: Arc::new(RecordingSink::new()),
            config_dir: dir.path().to_path_buf(),
            secret,
        };
        (state, dir)
    }

    /// 一个线程内的 Tokio 运行时。
    ///
    /// 不起 TCP listener：本 crate 的 `tower` 没开 `util` feature，
    /// `ServiceExt::oneshot` 用不上，而为了测试去改 `Cargo.toml` 会越过本任务的
    /// 依赖边界。直接调用 handler 走的仍是同一条 extractor → 业务 → 响应体
    /// 序列化路径。
    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime")
    }

    fn call<F, Fut, T>(f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        rt().block_on(f())
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("response body must be readable");
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    fn read_body(resp: Response) -> (StatusCode, Value) {
        let status = resp.status();
        let body = call(|| body_json(resp));
        (status, body)
    }

    /// 走一遍 `POST /pairing/request` 的 handler，返回响应体。
    async fn request_once(state: &PairingApiState) -> Value {
        let resp = pairing_request(
            State(state.clone()),
            None,
            Json(PairingRequestBody {
                client_name: "cli".into(),
                requested_scopes: vec!["read".into()],
            }),
        )
        .await;
        let status = resp.status();
        let body = body_json(resp).await;
        assert_eq!(status, StatusCode::OK, "配对请求必须受理：{body}");
        body
    }

    /// 发起配对并批掉它，返回 (pollToken, 配对码)。
    ///
    /// **不**顺手把凭据 poll 走：批准侧压根拿不到明文（`approve` 只返回 `()`），
    /// 凭据只存在于交付那一次的响应里；要测响应就得由用例自己去 poll。
    fn approved_pair(state: &PairingApiState) -> (String, String) {
        let body = call(|| request_once(state));
        let poll_token = body["pollToken"].as_str().unwrap().to_string();
        let user_code = body["userCode"].as_str().unwrap().to_string();
        state
            .store
            .approve(&user_code, false, Instant::now())
            .expect("批准必须成功");
        (poll_token, user_code)
    }

    fn auth_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("{PAIRING_SCHEME} {token}").parse().unwrap(),
        );
        headers
    }

    /// 找任意层级里名字含 token 的字段（只看名字，不看值）。
    fn token_shaped_field(v: &Value) -> Option<String> {
        match v {
            Value::Object(m) => {
                for (k, val) in m {
                    if k.to_ascii_lowercase().contains("token") && !val.is_null() {
                        return Some(k.clone());
                    }
                    if let Some(found) = token_shaped_field(val) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Array(a) => {
                for item in a {
                    if let Some(found) = token_shaped_field(item) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 正常请求 → 200，且 `userCode` 与 `pollToken` 两个字段都在且非空。
    #[test]
    fn pairing_request_returns_user_code_and_poll_token() {
        let (state, _dir) = api();
        let body = call(|| request_once(&state));

        let user_code = body["userCode"].as_str().unwrap_or_default();
        let poll_token = body["pollToken"].as_str().unwrap_or_default();

        assert_eq!(
            user_code.len(),
            6,
            "userCode 必须是 6 位，实际 {user_code:?}"
        );
        assert!(!poll_token.is_empty(), "pollToken 不得为空，响应体：{body}");
        assert!(!body["requestId"].as_str().unwrap_or_default().is_empty());
        assert_eq!(
            body["pollIntervalSeconds"].as_u64(),
            Some(POLL_INTERVAL_SECS)
        );
        assert!(
            body["expiresAt"]
                .as_str()
                .unwrap_or_default()
                .ends_with('Z'),
            "expiresAt 必须是 RFC3339 UTC，实际 {:?}",
            body["expiresAt"]
        );
    }

    /// 没有 `Authorization: Pairing <token>` → 401。
    #[test]
    fn poll_requires_pairing_authorization_header() {
        let (state, _dir) = api();

        // 无 header
        let (status, body) = read_body(call(|| {
            pairing_poll(
                State(state.clone()),
                None,
                HeaderMap::new(),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(token_shaped_field(&body), None, "401 体不得带凭据字段");

        // 方案名不对
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer whatever".parse().unwrap(),
        );
        let (status, _) = read_body(call(|| {
            pairing_poll(State(state), None, headers, Json(PairingPollBody {}))
        }));
        assert_eq!(status, StatusCode::UNAUTHORIZED, "只有 Pairing 方案算数");
    }

    /// `?pollToken=` 不算凭据：query 参数走不到鉴权，仍按无凭据处理。
    #[test]
    fn poll_query_parameter_is_not_a_credential() {
        let (state, _dir) = api();
        let body = call(|| request_once(&state));
        let poll_token = body["pollToken"].as_str().unwrap().to_string();

        let (status, _) = read_body(call(|| {
            pairing_poll(
                State(state.clone()),
                None,
                auth_headers(&poll_token),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(status, StatusCode::OK, "带 header 时必须能进入业务");

        // 同样的 token 改从查询参数带进来 —— 没有任何 header，必须 401。
        let (status, _) = read_body(call(|| {
            pairing_poll(
                State(state),
                None,
                HeaderMap::new(),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "只带 ?pollToken= 必须按无凭据处理"
        );
    }

    /// 批准后：第一次 poll 拿到凭据，第二次失效且响应体里没有 token 形态字段。
    #[test]
    fn poll_is_consumed_once_and_failure_body_has_no_token_field() {
        let (state, _dir) = api();
        let (poll_token, _user_code) = approved_pair(&state);

        let (status, first_body) = read_body(call(|| {
            pairing_poll(
                State(state.clone()),
                None,
                auth_headers(&poll_token),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first_body["status"], json!("approved"));
        let client_token = first_body["clientToken"]
            .as_str()
            .expect("成功响应必须带凭据")
            .to_string();
        assert_eq!(
            token_shaped_field(&first_body),
            Some("clientToken".to_string()),
            "成功响应确实带凭据字段（否则本用例后面的否定断言是空的）"
        );
        // 交付出去的必须正是 store 签发的那一条：批准侧没有别的途径拿到明文，
        // 因此「拿它去撤销得掉」就是「服务端与客户端持有同一份凭据」的唯一证明。
        let client_id = first_body["clientId"]
            .as_str()
            .expect("成功响应必须带 clientId");
        assert!(
            !client_id.is_empty() && state.store.user_code_for_request(client_id).is_some(),
            "clientId 必须指向一条真实存在的配对请求：{client_id}"
        );
        assert!(
            state.store.revoke(&client_token, Instant::now()),
            "交付出去的凭据必须与 store 签发的那一条逐字节相同"
        );

        let (status, second_body) = read_body(call(|| {
            pairing_poll(
                State(state),
                None,
                auth_headers(&poll_token),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "同一 pollToken 第二次 poll 必须失效"
        );
        assert_eq!(
            token_shaped_field(&second_body),
            None,
            "失效响应体里不得出现任何 token 形态字段，实际：{second_body}"
        );
    }

    /// 一个 loopback 的 `ConnectInfo`：撤销 / 批准都只许本机（fail-closed），
    /// 拿不到 `ConnectInfo` 一律按非本机拒，所以必须显式造一个。
    fn loopback_peer() -> Option<ConnectInfo<SocketAddr>> {
        Some(ConnectInfo(
            "127.0.0.1:40000".parse().expect("valid loopback addr"),
        ))
    }

    fn nonloopback_peer() -> Option<ConnectInfo<SocketAddr>> {
        Some(ConnectInfo(
            "203.0.113.7:40000"
                .parse()
                .expect("valid non-loopback addr"),
        ))
    }

    fn headers_with(signal: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_bytes(signal.as_bytes())
                .unwrap_or_else(|e| panic!("{signal:?} 必须是合法 header 名: {e}")),
            axum::http::HeaderValue::from_static("203.0.113.1"),
        );
        headers
    }

    /// 批准 / 撤销的来源判据：回环**且**不带代理信号头才算桌面端自己。
    ///
    /// 这一格原本只看 `ConnectInfo`，于是本机 HTTP 代理转发进来的请求
    /// （对端同样落在 `127.0.0.1`）与桌面端自己发出的那一发完全无法区分 ——
    /// 任何能让请求经过本机代理的人都能自己批掉配对请求。
    #[test]
    fn require_local_desktop_rejects_proxy_signalled_loopback() {
        // 无信号头的回环：放行（桌面端自己的那一发）。
        assert!(
            require_local_desktop(loopback_peer().as_ref(), &HeaderMap::new()).is_none(),
            "回环且不带任何代理信号头 = 桌面端自己，必须放行"
        );

        // 三个信号头逐个试：只看其中一个就够，名单与判定表第三列同源。
        for signal in ["x-forwarded-for", "forwarded", "tailscale-user-login"] {
            assert!(
                require_local_desktop(loopback_peer().as_ref(), &headers_with(signal)).is_some(),
                "回环 + `{signal}` = 请求经过本机代理，不是桌面端自己，必须 403"
            );
        }

        // 非回环：与改动前一致，照旧 403。
        assert!(
            require_local_desktop(nonloopback_peer().as_ref(), &HeaderMap::new()).is_some(),
            "非回环来源必须 403"
        );
        // 拿不到 ConnectInfo 一律按非本机处理（fail-closed），改动后仍如此。
        assert!(
            require_local_desktop(None, &HeaderMap::new()).is_some(),
            "拿不到 ConnectInfo 必须按非本机拒（fail-closed）"
        );
    }

    /// 承重的一条：信号头必须真的**穿过 handler 传到判据里**。
    ///
    /// 上面那条验的是纯函数。判据改成收 `&HeaderMap` 之后，最容易出的错是
    /// handler 忘了把头传进去（或者传了个空表）—— 此时判据永远只看回环，
    /// 而上面那条纯函数用例照样全绿。所以这里从 `pairing_approve` /
    /// `pairing_revoke` 的**生产入口**进去，要求真的拿到 403 与
    /// `local_desktop_only`。
    #[test]
    fn pairing_handlers_reject_proxy_signalled_loopback_from_the_entry_point() {
        let (state, _dir) = api();
        let forwarded = headers_with("x-forwarded-for");

        let (approve_status, approve_body) = read_body(call(|| {
            pairing_approve(
                State(state.clone()),
                loopback_peer(),
                forwarded.clone(),
                Json(PairingApproveBody {
                    user_code: "ABCDEF".into(),
                    confirmed_foreign_account: false,
                }),
            )
        }));
        assert_eq!(approve_status, StatusCode::FORBIDDEN);
        assert_eq!(approve_body["error"], "local_desktop_only");

        let (revoke_status, revoke_body) = read_body(call(|| {
            pairing_revoke(
                State(state.clone()),
                loopback_peer(),
                forwarded,
                Json(PairingRevokeBody {
                    client_id: "does-not-matter".into(),
                }),
            )
        }));
        assert_eq!(revoke_status, StatusCode::FORBIDDEN);
        assert_eq!(revoke_body["error"], "local_desktop_only");

        // 正向对照：不带信号头的同一格必须**不是** `local_desktop_only` ——
        // 否则上面两条 403 可能只是「这两个 userCode / clientId 本来就不存在」。
        let (ok_status, ok_body) = read_body(call(|| {
            pairing_revoke(
                State(state.clone()),
                loopback_peer(),
                HeaderMap::new(),
                Json(PairingRevokeBody {
                    client_id: "does-not-matter".into(),
                }),
            )
        }));
        assert_eq!(ok_status, StatusCode::OK);
        assert_ne!(ok_body["error"], "local_desktop_only");
    }

    /// 回归：按 clientId 撤销必须**真的把凭据摘出凭据库**。
    ///
    /// 上面那条用例走的是 `store.revoke(明文)` —— 比摘要的两端都是 store 自己
    /// 那把 secret，于是无论这把 secret 对不对都恒等，它压根测不到跨模块的
    /// 摘要同源性。
    ///
    /// 这里走的是生产路径：状态机只交出**摘要**（明文谁也重算不出来），
    /// `revoke_credential` 拿它去 `clients.json` 里逐条 `ct_eq` 比对。两份摘要
    /// 只有同源才相等 —— 一旦状态机用的是自己另生成的那把 secret，撤销就会变成
    /// 「受理成功、实际一条没摘」，凭据继续畅通，而界面上撤销按钮已经变灰。
    #[test]
    fn revoke_by_client_id_actually_removes_the_credential_from_the_library() {
        let (state, _dir) = api();
        let (poll_token, _user_code) = approved_pair(&state);

        let (status, body) = read_body(call(|| {
            pairing_poll(
                State(state.clone()),
                None,
                auth_headers(&poll_token),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(status, StatusCode::OK, "poll 必须交付凭据：{body}");
        let client_token = body["clientToken"]
            .as_str()
            .expect("成功响应必须带 clientToken")
            .to_string();
        let client_id = body["clientId"]
            .as_str()
            .expect("成功响应必须带 clientId")
            .to_string();

        // 交付的那条凭据确实进了库 —— 否则后面的否定断言就是空的。
        let stored = token_hash(&state.secret, &client_token);
        assert!(
            load_clients(&state.config_dir)
                .expect("凭据库必须可读")
                .iter()
                .any(|c| c.token_hash.ct_eq(&stored)),
            "交付出去的凭据必须确实落在凭据库里"
        );

        let (status, body) = read_body(call(|| {
            pairing_revoke(
                State(state.clone()),
                loopback_peer(),
                HeaderMap::new(),
                Json(PairingRevokeBody {
                    client_id: client_id.clone(),
                }),
            )
        }));
        assert_eq!(status, StatusCode::OK, "撤销请求必须受理：{body}");
        assert_eq!(
            body["revoked"], true,
            "按 clientId 撤销一条刚交付的凭据必须报已撤销：{body}"
        );

        let still_there = load_clients(&state.config_dir)
            .expect("凭据库必须可读")
            .iter()
            .any(|c| c.token_hash.ct_eq(&stored));
        assert!(
            !still_there,
            "撤销后凭据库里不得还留着这条摘要 —— 留着就等于撤销没生效，\
             持有者可以继续拿着旧 token 畅通到天荒地老"
        );
    }

    /// 编译期签名锁：body 参数必须是必填的 `Json<T>`，不是 `Option<Json<T>>`。
    ///
    /// 改成 `Option<..>` 会让 `Option<Json<T>>: FromRequest` 的宽松语义回来 ——
    /// CORS 简单请求（跨站表单、不带 Content-Type 的 fetch）会被吞成 `None`，
    /// 而行为上的用例照样可能全绿。这里把形状钉在编译期：改回 `Option` 时整个
    /// 测试 crate 直接构建失败。
    fn assert_takes_required_json_body<F, Fut>(_handler: F)
    where
        F: Fn(
            State<PairingApiState>,
            Option<ConnectInfo<SocketAddr>>,
            Json<PairingRequestBody>,
        ) -> Fut,
    {
    }

    fn assert_poll_takes_required_json_body<F, Fut>(_handler: F)
    where
        F: Fn(
            State<PairingApiState>,
            Option<ConnectInfo<SocketAddr>>,
            HeaderMap,
            Json<PairingPollBody>,
        ) -> Fut,
    {
    }

    /// CSRF 回归（签名级）：两条配对 handler 的 body 参数都必须是必填 `Json<T>`。
    #[test]
    fn assert_pairing_handlers_take_required_json_body() {
        assert_takes_required_json_body(pairing_request);
        assert_poll_takes_required_json_body(pairing_poll);
    }

    /// poll 的 body 不接受任何字段 —— 凭据只能走 header。
    #[test]
    fn poll_body_rejects_a_poll_token_field() {
        use axum::extract::FromRequest;

        let req = axum::http::Request::builder()
            .method("POST")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(r#"{"pollToken":"x"}"#))
            .unwrap();

        let outcome = call(|| Json::<PairingPollBody>::from_request(req, &()));

        assert!(
            outcome.is_err(),
            "请求体里带 pollToken 必须被拒：凭据只认 Authorization 头"
        );
    }

    /// `poll_token_from_headers` 的取值规则。
    #[test]
    fn poll_token_from_headers_accepts_only_the_pairing_scheme() {
        let mk = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert(axum::http::header::AUTHORIZATION, v.parse().unwrap());
            h
        };

        assert_eq!(poll_token_from_headers(&mk("Pairing abc")), Some("abc"));
        assert_eq!(poll_token_from_headers(&mk("pairing abc")), Some("abc"));
        assert_eq!(poll_token_from_headers(&mk("Bearer abc")), None);
        assert_eq!(poll_token_from_headers(&mk("Pairing")), None);
        assert_eq!(poll_token_from_headers(&mk("Pairing ")), None);
        assert_eq!(
            poll_token_from_headers(&mk("Pairing a b")),
            None,
            "只认恰好一个凭据"
        );
        assert_eq!(poll_token_from_headers(&HeaderMap::new()), None);
    }

    /// 结构锁：实现里不许出现任何查询参数提取器。
    ///
    /// 这条与 `poll_token_from_headers` 的签名互为犄角 —— 签名挡住了不传
    /// `Query`，这条挡住「绕开签名另外去 `req.uri()` 上扒」。
    #[test]
    fn pairing_poll_does_not_read_query_parameters() {
        let source = include_str!("handlers.rs");
        let body = source
            .split("#[cfg(test)]")
            .next()
            .expect("must have an impl region before the tests");

        for line in body.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            for banned in ["Query<", "QueryParams", "RawQuery", ".uri()"] {
                assert!(
                    !line.contains(banned),
                    "配对端点不得读取查询参数（`{banned}`），凭据只走 Authorization 头：{trimmed}"
                );
            }
        }
    }

    /// 限流：同一来源 20 次放行、第 21 次 429；另一个来源不受牵连。
    #[test]
    fn pairing_request_is_rate_limited_per_source() {
        let (state, _dir) = api();

        for i in 0..20 {
            let (status, _) = read_body(call(|| {
                pairing_request(
                    State(state.clone()),
                    Some(ConnectInfo("10.1.2.3:5000".parse().unwrap())),
                    Json(PairingRequestBody {
                        client_name: "cli".into(),
                        requested_scopes: vec![],
                    }),
                )
            }));
            assert_eq!(status, StatusCode::OK, "第 {i} 次必须放行");
        }

        let (status, _) = read_body(call(|| {
            pairing_request(
                State(state.clone()),
                Some(ConnectInfo("10.1.2.3:5000".parse().unwrap())),
                Json(PairingRequestBody {
                    client_name: "cli".into(),
                    requested_scopes: vec![],
                }),
            )
        }));
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "第 21 次必须 429");

        let (status, _) = read_body(call(|| {
            pairing_request(
                State(state),
                Some(ConnectInfo("10.1.2.4:5000".parse().unwrap())),
                Json(PairingRequestBody {
                    client_name: "cli".into(),
                    requested_scopes: vec![],
                }),
            )
        }));
        assert_eq!(
            status,
            StatusCode::OK,
            "限流按来源 IP 计，另一个 peer 不该被牵连"
        );
    }

    /// 尚未批准时 poll 返回 pending，响应体不含任何 token 形态字段。
    #[test]
    fn poll_pending_response_has_no_token_field() {
        let (state, _dir) = api();
        let body = call(|| request_once(&state));
        let poll_token = body["pollToken"].as_str().unwrap().to_string();

        let (status, pending) = read_body(call(|| {
            pairing_poll(
                State(state),
                None,
                auth_headers(&poll_token),
                Json(PairingPollBody {}),
            )
        }));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(pending["status"], json!("pending"));
        assert_eq!(token_shaped_field(&pending), None, "{pending}");
    }

    /// 过期 / 被拒的响应都不带凭据字段；只有 `Approved` 带。
    #[test]
    fn only_approved_poll_response_carries_a_credential() {
        for outcome in [
            PollOutcome::Pending,
            PollOutcome::Expired,
            PollOutcome::Rejected,
            PollOutcome::RateLimited,
            PollOutcome::Invalid,
        ] {
            let (status, payload) = poll_response(outcome.clone());
            let json = serde_json::to_value(&payload).unwrap();
            assert_eq!(
                token_shaped_field(&json),
                None,
                "{outcome:?} 的响应体不得带凭据字段：{json}"
            );
            assert!(
                status.is_success()
                    || status == StatusCode::TOO_MANY_REQUESTS
                    || status == StatusCode::UNAUTHORIZED
            );
        }

        let (status, payload) = poll_response(PollOutcome::Approved {
            token: "the-credential".into(),
            client_id: "client-1".into(),
            client_name: "cli".into(),
        });
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload.client_token.as_deref(), Some("the-credential"));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(token_shaped_field(&json), Some("clientToken".to_string()));
    }

    /// 配对请求会真的发出审批事件，字段与响应一一对应。
    #[test]
    fn pairing_request_emits_approval_event_with_matching_codes() {
        let (mut state, _dir) = api();
        let sink = RecordingSink::new();
        state.sink = Arc::new(sink.clone());

        let (_, body) = read_body(call(|| {
            pairing_request(
                State(state),
                Some(ConnectInfo("10.9.9.9:4242".parse().unwrap())),
                Json(PairingRequestBody {
                    client_name: "cli".into(),
                    requested_scopes: vec!["read".into()],
                }),
            )
        }));

        let recorded = sink.recorded();
        assert_eq!(recorded.len(), 1, "桌面端必须收到一条待批事件");
        assert_eq!(recorded[0].user_code, body["userCode"].as_str().unwrap());
        assert_eq!(recorded[0].request_id, body["requestId"].as_str().unwrap());
        assert_eq!(recorded[0].evidence.peer_addr, "10.9.9.9:4242");
    }

    /// 唯一的生产推送点必须发**真实证据**，不许硬编码成回环。
    ///
    /// `ApprovalEvidence::loopback(..)` 无条件把 `source_kind` 设成 `Loopback`、
    /// `is_foreign_account` 设成 `false` —— 哪怕对端就是个别的账号 share 进本
    /// tailnet 的节点。界面上的「外来账号默认拒绝」分支因此永远走不到。
    #[test]
    fn pairing_request_emits_probed_evidence_not_a_hardcoded_loopback() {
        let (mut state, _dir) = api();
        let sink = RecordingSink::new();
        state.sink = Arc::new(sink.clone());

        call(|| {
            pairing_request(
                State(state),
                Some(ConnectInfo("10.9.9.9:4242".parse().unwrap())),
                Json(PairingRequestBody {
                    client_name: "cli".into(),
                    requested_scopes: vec![],
                }),
            )
        });

        let recorded = sink.recorded();
        assert_eq!(recorded.len(), 1);
        let evidence = &recorded[0].evidence;
        assert_ne!(
            evidence.source_kind,
            crate::remote::events::SourceKind::Loopback,
            "证据必须来自 whois 的 UserProfile.ID / Node.User 比对，不能写死成 Loopback：{evidence:?}"
        );
        assert_eq!(evidence.peer_addr, "10.9.9.9:4242", "对端地址必须原样带上");
    }

    /// 结构锁：`pairing_request` 的函数体里不许再出现 `ApprovalEvidence::loopback`。
    ///
    /// 上一条是行为锁（看推出去的东西对不对），这条锁住**写法** —— 有人日后
    /// 「顺手简化一下」把真实证据换回写死的构造器时，会先在这里红。
    #[test]
    fn pairing_request_does_not_construct_loopback_evidence() {
        let source = include_str!("handlers.rs");
        let impl_region = source
            .split("#[cfg(test)]")
            .next()
            .expect("must have an impl region before the tests");
        let body = impl_region
            .split("pub async fn pairing_request")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn pairing_poll").next())
            .expect("pairing_request body must be delimited by the two handlers");

        assert!(
            !body.contains("ApprovalEvidence::loopback"),
            "pairing_request 不得构造写死的回环证据：\n{body}"
        );
        assert!(
            body.contains("evidence_for"),
            "pairing_request 必须走 evidence_for 取真实证据：\n{body}"
        );
    }
}
