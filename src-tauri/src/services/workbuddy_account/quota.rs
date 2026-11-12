//! WorkBuddy 账号的套餐与积分（只读）。
//!
//! WorkBuddy 主进程给本机脚本（它的内置 skill 等）开了一条本地 IPC 通道 WBIPC
//! （`main/server.js` 的 `createWbipcService`，客户端 SDK 是 app 里的 `workbuddy_ipc` Python 包）：
//! - 发现文件 `~/.workbuddy/wbipc/endpoint.json`（0600，明文 `{endpoint, ticket}`），每次启动重写；
//! - `endpoint` 是 unix socket（Windows 上是命名管道 `\\.\pipe\wbipc-<id>`），按行分隔的 JSON 帧；
//! - 握手是双向 HMAC-SHA256 证明、服务端先自证（`ticket` 本身从不上线，只发它的 sha256 前缀）；
//! - `wb.request` pipe 的 `http.fetch`：WorkBuddy 用它当前登录的身份代为访问自己的后端。
//!
//! 这里只发 WorkBuddy 自己刷新额度时就会发的只读查询：`POST /billing/meter/get-user-resource-summary`
//! （空 body，总量），再按汇总里有哪类套餐查 free / paid 两个套餐明细（各个资源包的到期时间，
//! 请求体照抄 WorkBuddy 的 `fetchUserResources`）。规矩：
//! - 只在 WorkBuddy 正在运行、且要查的正是它当前登着的号时查；别的号只给上次缓存的结果；
//! - 不读 token、keyblob，不调用任何写操作 / 登出 / 续期接口；`ticket` 不进日志、不出这个模块；
//! - 结果（套餐名、积分数和到期时间，不敏感）缓存进 CC Switch 自己的账号表，WorkBuddy 没运行时显示上次结果。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::WorkBuddyAccountManager;
use crate::error::AppError;

/// 当前账号查过之后这么久内直接用缓存（手动刷新除外）。
pub(super) const QUOTA_CACHE_TTL_MS: i64 = 5 * 60 * 1000;
/// 整个会话（连接 + 握手 + 一次后端请求）的超时；WorkBuddy 自己对后端的超时是 50 秒。
const SESSION_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_FRAME: usize = 1024 * 1024;
const PROTOCOL: u32 = 1;
const PIPE: &str = "wb.request";
const METHOD: &str = "http.fetch";
pub(super) const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
pub(super) const FREE_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-free-packages";
pub(super) const PAID_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-paid-packages";
/// WorkBuddy 按套餐代码把资源分两个接口查，各自只认自己那组（它的 `Is` / `Fs` 两个列表）。
pub(super) const FREE_PACKAGE_CODES: &[&str] = &[
    "TCACA_code_001_PqouKr6QWV", // free
    "TCACA_code_008_cfWoLwvjU4", // freeMon（体验版）
    "TCACA_code_035_ArVxJcGDsm", // freeMonIntl
    "TCACA_code_006_DbXS0lrypC", // gift
    "TCACA_code_039_KRcQj7wUat", // proTrialMon
    "TCACA_code_040_mi9rCYg46x", // proTrialYear
    "TCACA_code_007_nzdH5h4Nl0", // activity
    "TCACA_code_028_NtpWi0jzXs", // bonus28
    "TCACA_code_037_WxOD3MpI2o", // bonusIntl
    "TCACA_code_029_6wCGEWquYy", // bonus29
    "TCACA_code_030_BjSt89qTvr", // bonus30（拉新权益包）
];
pub(super) const PAID_PACKAGE_CODES: &[&str] = &[
    "TCACA_code_002_AkiJS3ZHF5", // proMon
    "TCACA_code_005_maRGyrHhw1", // proMonPlus
    "TCACA_code_003_FAnt7lcmRT", // proYear
    "TCACA_code_023_4xbGhMrE6q", // youth
    "TCACA_code_026_BaESVICNoi", // advanced
    "TCACA_code_027_0FCGVA6vSa", // flagship
    "TCACA_code_043_6oIkxT7Z0S", // buddyOneVip
    "TCACA_code_009_0XmEQc2xOf", // extra（积分包）
    "TCACA_code_038_OhvqZtiPKr", // extra38
    "TCACA_code_036_lupO5WgNdG", // extraIntl
];
/// 一页取多少个资源包（同 WorkBuddy）。只取第一页：一个号两百个以上资源包时到期列表不全，总量不受影响
const PACKAGES_PAGE_SIZE: u32 = 200;
/// 资源状态：有效、已用完（同 WorkBuddy）
const PACKAGE_STATUSES: [u32; 2] = [0, 3];
/// 「切片」资源（按天切的额度，见 WorkBuddy `isSliceCapacityResource`）：当天的切片今晚就到期，
/// 天天提醒没有意义，不进到期列表
const SLICE_CAPACITY_TYPE: i64 = 4;
/// 计费后端的日期时间串（`2026-11-30 23:59:59`）是北京时间
const BILLING_UTC_OFFSET_SECS: i32 = 8 * 3600;
/// 到期时间远于这么久（官方用 2035、2049 这类日期占位）当作不会过期
const NEVER_EXPIRES_AFTER_MS: i64 = 10 * 365 * 24 * 3600 * 1000;

/// 一次查询的结果（存进账号表，也原样给前端）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyQuotaSnapshot {
    /// 套餐名（`SubscriptionPackageName`，后端给的中文名）；没有时为 `None`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_name: Option<String>,
    #[serde(default)]
    pub is_paid: bool,
    /// 本周期积分：总量 / 剩余 / 已用（各资源包相加，和 WorkBuddy 自己的算法一致）
    pub total: f64,
    pub remaining: f64,
    pub used: f64,
    /// 查询时刻（毫秒）
    pub queried_at: i64,
    /// 还有剩余的各个资源包各自什么时候到期，按到期先后排（不会过期的在最后）；
    /// `None` 表示套餐明细这次没查到（只有总量）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiries: Option<Vec<WorkBuddyCreditExpiry>>,
}

/// 一个资源包的剩余积分和到期时间。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyCreditExpiry {
    pub credits: f64,
    /// 到期时刻（毫秒）；不会过期时为 `None`
    #[serde(default)]
    pub expires_at: Option<i64>,
}

/// 这次没能拿到新结果的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnavailable {
    /// WorkBuddy 没在运行（额度只能经它查）
    NotRunning,
    /// 不是 WorkBuddy 当前登着的号
    NotCurrent,
    /// WorkBuddy 在运行但没登录
    NotSignedIn,
    /// 其他失败（见 `error`）
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyQuota {
    /// 最近一次成功的结果（可能是这次刚查的，也可能是上次缓存的）
    pub snapshot: Option<WorkBuddyQuotaSnapshot>,
    /// 是 WorkBuddy 当前登着的号
    pub current: bool,
    /// 这次没刷新成的原因；`None` 表示 `snapshot` 是新鲜的
    pub unavailable: Option<QuotaUnavailable>,
    pub error: Option<String>,
}

/// 查额度前在阻塞线程里收集的上下文。
pub(super) struct QuotaContext {
    pub cached: Option<WorkBuddyQuotaSnapshot>,
    pub current: bool,
    pub running: bool,
    pub discovery: PathBuf,
}

#[derive(Debug, PartialEq)]
pub(super) enum FetchError {
    NotRunning,
    NotSignedIn,
    Failed(String),
}

/// 这个账号的套餐与积分。别的号直接给缓存；当前号在缓存过期（或 `force`）且 WorkBuddy 在运行时
/// 经 WBIPC 查一次并写回账号表。
pub async fn account_quota(
    manager: Arc<WorkBuddyAccountManager>,
    account_id: String,
    force: bool,
) -> Result<WorkBuddyQuota, AppError> {
    // 同一时间只开一条会话：几个界面同时要额度时，后来的直接吃前一个写好的缓存
    let _gate = manager.quota_gate.lock().await;
    let ctx = blocking(&manager, {
        let id = account_id.clone();
        move |manager| manager.quota_context(&id)
    })
    .await?;
    let reply = |snapshot, unavailable, error| WorkBuddyQuota {
        snapshot,
        current: ctx.current,
        unavailable,
        error,
    };
    if !ctx.current {
        return Ok(reply(
            ctx.cached.clone(),
            Some(QuotaUnavailable::NotCurrent),
            None,
        ));
    }
    let now = chrono::Utc::now().timestamp_millis();
    if !force {
        if let Some(cached) = &ctx.cached {
            if now - cached.queried_at < QUOTA_CACHE_TTL_MS {
                return Ok(reply(Some(cached.clone()), None, None));
            }
        }
    }
    if !ctx.running {
        return Ok(reply(
            ctx.cached.clone(),
            Some(QuotaUnavailable::NotRunning),
            None,
        ));
    }

    match fetch_summary(&ctx.discovery).await {
        Ok(snapshot) => {
            let saved = blocking(&manager, {
                let id = account_id.clone();
                let snapshot = snapshot.clone();
                move |manager| manager.save_quota(&id, snapshot)
            })
            .await?;
            if saved {
                Ok(reply(Some(snapshot), None, None))
            } else {
                // 查询期间 WorkBuddy 换了号：这份结果不属于它，丢掉
                Ok(reply(
                    ctx.cached.clone(),
                    Some(QuotaUnavailable::NotCurrent),
                    None,
                ))
            }
        }
        Err(FetchError::NotRunning) => Ok(reply(
            ctx.cached.clone(),
            Some(QuotaUnavailable::NotRunning),
            None,
        )),
        Err(FetchError::NotSignedIn) => Ok(reply(
            ctx.cached.clone(),
            Some(QuotaUnavailable::NotSignedIn),
            None,
        )),
        Err(FetchError::Failed(message)) => {
            log::warn!("[WorkBuddyAccount] 经 WorkBuddy 查询额度失败: {message}");
            Ok(reply(
                ctx.cached.clone(),
                Some(QuotaUnavailable::Failed),
                Some(message),
            ))
        }
    }
}

async fn blocking<T: Send + 'static>(
    manager: &Arc<WorkBuddyAccountManager>,
    job: impl FnOnce(&WorkBuddyAccountManager) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let manager = Arc::clone(manager);
    tokio::task::spawn_blocking(move || job(&manager))
        .await
        .map_err(|error| AppError::Message(error.to_string()))?
}

/// 发现文件里的连接信息。`ticket` 只用来算握手证明，不打印。
struct Discovery {
    endpoint: String,
    ticket: String,
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

fn read_discovery(path: &Path) -> Result<Discovery, FetchError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(FetchError::NotRunning)
        }
        Err(error) => {
            return Err(FetchError::Failed(format!(
                "读不了 WBIPC 发现文件: {error}"
            )))
        }
    };
    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|_| FetchError::Failed("WBIPC 发现文件格式不对".into()))?;
    let text = |key: &str| {
        doc.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match (text("endpoint"), text("ticket")) {
        (Some(endpoint), Some(ticket)) => Ok(Discovery { endpoint, ticket }),
        _ => Err(FetchError::NotRunning),
    }
}

/// 经 WBIPC 查一次额度汇总。
pub(super) async fn fetch_summary(discovery: &Path) -> Result<WorkBuddyQuotaSnapshot, FetchError> {
    let discovery = read_discovery(discovery)?;
    tokio::time::timeout(SESSION_TIMEOUT, async {
        let stream = connect(&discovery.endpoint).await?;
        session(stream, &discovery).await
    })
    .await
    .map_err(|_| FetchError::Failed("WorkBuddy 本地接口超时".into()))?
}

#[cfg(unix)]
async fn connect(endpoint: &str) -> Result<tokio::net::UnixStream, FetchError> {
    if !Path::new(endpoint).is_absolute() {
        return Err(FetchError::Failed("WBIPC 端点不是绝对路径".into()));
    }
    tokio::net::UnixStream::connect(endpoint)
        .await
        .map_err(|error| match error.kind() {
            // socket 没了 / 没人在听：WorkBuddy 已退出，发现文件是上次留下的
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                FetchError::NotRunning
            }
            _ => FetchError::Failed(format!("连不上 WorkBuddy 本地接口: {error}")),
        })
}

/// 推断（未实测）：Windows 上端点是命名管道 `\\.\pipe\wbipc-<id>`。
#[cfg(windows)]
async fn connect(
    endpoint: &str,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, FetchError> {
    if !endpoint.starts_with(r"\\.\pipe\") {
        return Err(FetchError::Failed("WBIPC 端点不是命名管道".into()));
    }
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => FetchError::NotRunning,
            _ => FetchError::Failed(format!("连不上 WorkBuddy 本地接口: {error}")),
        })
}

pub(super) fn ticket_id(ticket: &str) -> String {
    Sha256::digest(ticket.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 握手证明：HMAC-SHA256(ticket, 长度前缀拼接的 [域, 协议版本, 端点, 客户端 nonce, 服务端 nonce])。
pub(super) fn proof_mac(
    ticket: &str,
    server: bool,
    endpoint: &str,
    client_nonce: &str,
    server_nonce: &str,
) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(ticket.as_bytes()).expect("HMAC accepts any key length");
    let protocol = PROTOCOL.to_string();
    let domain = if server { "wbipc-s" } else { "wbipc-c" };
    for part in [
        domain,
        protocol.as_str(),
        endpoint,
        client_nonce,
        server_nonce,
    ] {
        mac.update(&(part.len() as u32).to_be_bytes());
        mac.update(part.as_bytes());
    }
    mac
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, frame: &Value) -> Result<(), FetchError> {
    let mut line =
        serde_json::to_vec(frame).map_err(|error| FetchError::Failed(error.to_string()))?;
    line.push(b'\n');
    writer
        .write_all(&line)
        .await
        .map_err(|error| FetchError::Failed(format!("写 WorkBuddy 本地接口失败: {error}")))?;
    writer
        .flush()
        .await
        .map_err(|error| FetchError::Failed(format!("写 WorkBuddy 本地接口失败: {error}")))
}

async fn recv<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<Value, FetchError> {
    loop {
        let mut line = Vec::new();
        let read = (&mut *reader)
            .take(MAX_FRAME as u64 + 1)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|error| FetchError::Failed(format!("读 WorkBuddy 本地接口失败: {error}")))?;
        if read == 0 {
            return Err(FetchError::Failed("WorkBuddy 本地接口断开了".into()));
        }
        if line.last() != Some(&b'\n') {
            return Err(FetchError::Failed(if line.len() > MAX_FRAME {
                "WorkBuddy 本地接口的回包太大".into()
            } else {
                "WorkBuddy 本地接口断开了".into()
            }));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        return serde_json::from_slice(&line)
            .map_err(|_| FetchError::Failed("WorkBuddy 本地接口回了看不懂的数据".into()));
    }
}

/// 等某个请求 id 的回包（中间的通知帧跳过）。
async fn response<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    id: u64,
) -> Result<Value, FetchError> {
    loop {
        let frame = recv(reader).await?;
        if frame.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = frame.get("error") {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("");
            return Err(match code {
                "E_NOT_CONNECTED" => FetchError::NotSignedIn,
                _ => FetchError::Failed(format!(
                    "WorkBuddy 本地接口拒绝了请求（{}）",
                    if code.is_empty() { "unknown" } else { code }
                )),
            });
        }
        return Ok(frame.get("result").cloned().unwrap_or(Value::Null));
    }
}

async fn session<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    discovery: &Discovery,
) -> Result<WorkBuddyQuotaSnapshot, FetchError> {
    let (read_half, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let endpoint = discovery.endpoint.as_str();
    let ticket = discovery.ticket.as_str();

    let client_nonce = URL_SAFE_NO_PAD.encode(uuid::Uuid::new_v4().as_bytes());
    send(
        &mut writer,
        &json!({
            "type": "session_hello",
            "protocol_min": PROTOCOL,
            "protocol_max": PROTOCOL,
            "client_nonce": client_nonce,
            "ticket_id": ticket_id(ticket),
            "client": {
                "kind": "external",
                "id": "cc-switch",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }),
    )
    .await?;

    let challenge = recv(&mut reader).await?;
    match challenge.get("type").and_then(Value::as_str) {
        Some("session_challenge") => {}
        Some("session_hello_error") => {
            let code = challenge
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("rejected");
            return Err(FetchError::Failed(format!(
                "WorkBuddy 本地接口拒绝了握手（{code}）"
            )));
        }
        _ => return Err(FetchError::Failed("WorkBuddy 本地接口握手异常".into())),
    }
    let server_nonce = challenge
        .get("server_nonce")
        .and_then(Value::as_str)
        .filter(|nonce| !nonce.is_empty())
        .ok_or_else(|| FetchError::Failed("WorkBuddy 本地接口握手异常".into()))?;
    // 对面先证明自己也持有 ticket；证明不了就可能是被抢占的端点，什么都不再发
    let server_proof = challenge
        .get("server_proof")
        .and_then(Value::as_str)
        .and_then(|proof| URL_SAFE_NO_PAD.decode(proof).ok())
        .unwrap_or_default();
    if proof_mac(ticket, true, endpoint, &client_nonce, server_nonce)
        .verify_slice(&server_proof)
        .is_err()
    {
        return Err(FetchError::Failed(
            "WorkBuddy 本地接口没能证明自己，已停止".into(),
        ));
    }
    let client_proof = URL_SAFE_NO_PAD.encode(
        proof_mac(ticket, false, endpoint, &client_nonce, server_nonce)
            .finalize()
            .into_bytes(),
    );
    send(
        &mut writer,
        &json!({ "type": "session_prove", "client_proof": client_proof }),
    )
    .await?;
    let ack = recv(&mut reader).await?;
    if ack.get("type").and_then(Value::as_str) != Some("session_hello_ack") {
        return Err(FetchError::Failed("WorkBuddy 本地接口握手被拒绝".into()));
    }

    send(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "broker/GetPipe",
            "params": { "pipe": PIPE },
        }),
    )
    .await?;
    let pipe = response(&mut reader, 1).await?;
    let channel = pipe
        .get("channel")
        .and_then(Value::as_str)
        .ok_or_else(|| FetchError::Failed("WorkBuddy 本地接口没有可用的请求通道".into()))?
        .to_string();
    let has_method = pipe
        .get("methods")
        .and_then(Value::as_array)
        .is_some_and(|methods| methods.iter().any(|method| method == METHOD));
    if !has_method {
        return Err(FetchError::NotSignedIn);
    }

    let now = chrono::Utc::now().timestamp_millis();
    let summary = fetch(
        &mut writer,
        &mut reader,
        &channel,
        2,
        SUMMARY_PATH,
        json!({}),
    )
    .await?;
    let mut snapshot = parse_summary(&summary, now)?;
    // 套餐明细只用来补到期时间：查不到就只显示总量，不算这次失败
    snapshot.expiries = match fetch_expiries(&mut writer, &mut reader, &channel, &summary).await {
        Ok(expiries) => Some(expiries),
        Err(error) => {
            log::warn!("[WorkBuddyAccount] 查积分到期时间失败: {error:?}");
            None
        }
    };
    let _ = writer.shutdown().await;
    Ok(snapshot)
}

/// 经 `http.fetch` 让 WorkBuddy 用它当前的身份 POST 一次，返回响应体。
async fn fetch<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    writer: &mut W,
    reader: &mut BufReader<R>,
    channel: &str,
    id: u64,
    path: &str,
    body: Value,
) -> Result<Vec<u8>, FetchError> {
    let body = serde_json::to_vec(&body).map_err(|error| FetchError::Failed(error.to_string()))?;
    send(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": format!("{channel}/{METHOD}"),
            "mode": "call",
            "params": {
                "method": "POST",
                "path": path,
                "headers": {
                    "content-type": "application/json",
                    "accept": "application/json",
                },
                "body_b64": STANDARD.encode(body),
            },
        }),
    )
    .await?;
    let result = response(reader, id).await?;
    let status = result.get("status").and_then(Value::as_u64).unwrap_or(0);
    if status == 401 || status == 403 {
        return Err(FetchError::NotSignedIn);
    }
    if !(200..300).contains(&status) {
        return Err(FetchError::Failed(format!("额度接口返回 HTTP {status}")));
    }
    result
        .get("body_b64")
        .and_then(Value::as_str)
        .and_then(|body| STANDARD.decode(body).ok())
        .ok_or_else(|| FetchError::Failed("额度接口回包为空".into()))
}

/// 汇总里出现了哪类套餐，就查哪个明细接口（WorkBuddy 对只有免费套餐的号也不查 paid）。
async fn fetch_expiries<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    writer: &mut W,
    reader: &mut BufReader<R>,
    channel: &str,
    summary: &[u8],
) -> Result<Vec<WorkBuddyCreditExpiry>, FetchError> {
    let codes = summary_package_codes(summary);
    let has_any = |group: &[&str]| codes.iter().any(|code| group.contains(&code.as_str()));
    let mut bodies = Vec::new();
    if has_any(FREE_PACKAGE_CODES) {
        let (start, end) = slice_period(chrono::Local::now().naive_local().date());
        let body = json!({
            "PageNumber": 1,
            "PageSize": PACKAGES_PAGE_SIZE,
            "PackageCodes": FREE_PACKAGE_CODES,
            "Status": PACKAGE_STATUSES,
            "SlicePeriodStartTime": start,
            "SlicePeriodEndTime": end,
        });
        bodies.push(fetch(writer, reader, channel, 3, FREE_PACKAGES_PATH, body).await?);
    }
    if has_any(PAID_PACKAGE_CODES) {
        let body = json!({
            "PageNumber": 1,
            "PageSize": PACKAGES_PAGE_SIZE,
            "PackageCodes": PAID_PACKAGE_CODES,
            "Status": PACKAGE_STATUSES,
            "NeedRenewInfo": true,
        });
        bodies.push(fetch(writer, reader, channel, 4, PAID_PACKAGES_PATH, body).await?);
    }
    parse_packages(&bodies, chrono::Utc::now().timestamp_millis())
}

/// 当天 0 点到 23:59:59（本地时间，同 WorkBuddy 的 `buildSlicePeriodRange`）。
pub(super) fn slice_period(day: chrono::NaiveDate) -> (String, String) {
    (
        format!("{} 00:00:00", day.format("%Y-%m-%d")),
        format!("{} 23:59:59", day.format("%Y-%m-%d")),
    )
}

/// 积分数：字符串或数字，非有限 / 非正一律当 0（同 WorkBuddy 的 `toCount` + `sumSummaryCapacity`）。
fn count(value: Option<&Value>) -> f64 {
    let number = match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    number.filter(|n| n.is_finite() && *n > 0.0).unwrap_or(0.0)
}

pub(super) fn parse_summary(
    body: &[u8],
    queried_at: i64,
) -> Result<WorkBuddyQuotaSnapshot, FetchError> {
    let doc: Value = serde_json::from_slice(body)
        .map_err(|_| FetchError::Failed("额度接口回了看不懂的数据".into()))?;
    let data = business_data(&doc)?;
    let (mut total, mut remaining, mut used) = (0.0, 0.0, 0.0);
    for package in data
        .get("Packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        total += count(package.get("CycleTotalCapacity"));
        remaining += count(package.get("CycleRemainCapacity"));
        used += count(package.get("CycleUsedCapacity"));
    }
    Ok(WorkBuddyQuotaSnapshot {
        plan_name: data
            .get("SubscriptionPackageName")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        is_paid: data
            .get("IsPaidUser")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        total,
        remaining,
        used,
        queried_at,
        expiries: None,
    })
}

/// 业务码为 0 时取 `data`，否则报错。
fn business_data(doc: &Value) -> Result<&Value, FetchError> {
    let code = doc.get("code").and_then(|code| match code {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    });
    match (code, doc.get("data").filter(|data| data.is_object())) {
        (Some(0), Some(data)) => Ok(data),
        _ => Err(FetchError::Failed(format!(
            "额度接口返回错误（code={}）",
            code.map_or_else(|| "?".to_string(), |code| code.to_string())
        ))),
    }
}

fn summary_package_codes(body: &[u8]) -> Vec<String> {
    let Ok(doc) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    doc.pointer("/data/Packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| package.get("PackageCode").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// 计费时间：毫秒数（数字或数字串），或北京时间的 `YYYY-MM-DD HH:MM:SS`；空串、0 和认不出的为 `None`。
fn billing_time(value: Option<&Value>) -> Option<i64> {
    let ms = match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => {
            let text = text.trim();
            if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
                text.parse().ok()
            } else {
                let beijing = chrono::FixedOffset::east_opt(BILLING_UTC_OFFSET_SECS)?;
                chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                    .ok()?
                    .and_local_timezone(beijing)
                    .single()
                    .map(|time| time.timestamp_millis())
            }
        }
        _ => None,
    }?;
    (ms > 0).then_some(ms)
}

/// 套餐明细 → 各资源包的剩余和到期时间。
///
/// 剩余照 WorkBuddy `resolvePackageCapacity`：有总容量（`CapacitySizePrecise`，一次性额度）就用
/// `CapacityRemainPrecise`，否则用本周期的剩余。到期取「资源到期」（`DeductionEndTime`，没有时
/// `ExpiredTime`）和「本周期结束」（`CycleEndTime`）里早的那个：按月发放的套餐资源要到 2035 年才
/// 到期，但这个月没用完的积分月底就作废了。
pub(super) fn parse_packages(
    bodies: &[Vec<u8>],
    now: i64,
) -> Result<Vec<WorkBuddyCreditExpiry>, FetchError> {
    let mut seen = std::collections::HashSet::new();
    let mut expiries = Vec::new();
    for body in bodies {
        let doc: Value = serde_json::from_slice(body)
            .map_err(|_| FetchError::Failed("套餐明细接口回了看不懂的数据".into()))?;
        let data = business_data(&doc)?;
        for package in data
            .get("Accounts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // 两个接口可能都回同一个资源（同 WorkBuddy 按 ResourceId 去重）
            if let Some(id) = package.get("ResourceId").filter(|id| !id.is_null()) {
                if !seen.insert(id.to_string()) {
                    continue;
                }
            }
            let slice = package.get("CapacityType").and_then(Value::as_i64)
                == Some(SLICE_CAPACITY_TYPE)
                && package
                    .get("SlicePeriodUsageDetails")
                    .and_then(Value::as_array)
                    .is_some_and(|details| !details.is_empty());
            if slice {
                continue;
            }
            let credits = if count(package.get("CapacitySizePrecise")) > 0.0 {
                count(package.get("CapacityRemainPrecise"))
            } else {
                let precise = count(package.get("CycleCapacityRemainPrecise"));
                if precise > 0.0 {
                    precise
                } else {
                    count(package.get("CycleCapacityRemain"))
                }
            };
            if credits <= 0.0 {
                continue;
            }
            let resource_end = billing_time(package.get("DeductionEndTime"))
                .or_else(|| billing_time(package.get("ExpiredTime")));
            let cycle_end = billing_time(package.get("CycleEndTime"));
            let expires_at = [resource_end, cycle_end]
                .into_iter()
                .flatten()
                .min()
                .filter(|at| at - now < NEVER_EXPIRES_AFTER_MS);
            expiries.push(WorkBuddyCreditExpiry {
                credits,
                expires_at,
            });
        }
    }
    expiries.sort_by_key(|expiry| expiry.expires_at.unwrap_or(i64::MAX));
    Ok(expiries)
}
