//! WorkBuddy 账号的套餐与积分（只读）。
//!
//! WorkBuddy 主进程给本机脚本（它的内置 skill 等）开了一条本地 IPC 通道 WBIPC
//! （`main/server.js` 的 `createWbipcService`，客户端 SDK 是 app 里的 `workbuddy_ipc` Python 包）：
//! - 发现文件 `~/.workbuddy/wbipc/endpoint.json`（0600，明文 `{endpoint, ticket}`），每次启动重写；
//! - `endpoint` 是 unix socket（Windows 上是命名管道 `\\.\pipe\wbipc-<id>`），按行分隔的 JSON 帧；
//! - 握手是双向 HMAC-SHA256 证明、服务端先自证（`ticket` 本身从不上线，只发它的 sha256 前缀）；
//! - `wb.request` pipe 的 `http.fetch`：WorkBuddy 用它当前登录的身份代为访问自己的后端。
//!
//! 这里只发一条请求：`POST /billing/meter/get-user-resource-summary`，空 body 的查询，和 WorkBuddy
//! 自己定期刷新额度时一模一样。规矩：
//! - 只在 WorkBuddy 正在运行、且要查的正是它当前登着的号时查；别的号只给上次缓存的结果；
//! - 不读 token、keyblob，不调用任何写操作 / 登出 / 续期接口；`ticket` 不进日志、不出这个模块；
//! - 结果（套餐名和积分数，不敏感）缓存进 CC Switch 自己的账号表，WorkBuddy 没运行时显示上次结果。

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

    send(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": format!("{channel}/{METHOD}"),
            "mode": "call",
            "params": {
                "method": "POST",
                "path": SUMMARY_PATH,
                "headers": {
                    "content-type": "application/json",
                    "accept": "application/json",
                },
                "body_b64": STANDARD.encode(b"{}"),
            },
        }),
    )
    .await?;
    let result = response(&mut reader, 2).await?;
    let _ = writer.shutdown().await;

    let status = result.get("status").and_then(Value::as_u64).unwrap_or(0);
    if status == 401 || status == 403 {
        return Err(FetchError::NotSignedIn);
    }
    if !(200..300).contains(&status) {
        return Err(FetchError::Failed(format!("额度接口返回 HTTP {status}")));
    }
    let body = result
        .get("body_b64")
        .and_then(Value::as_str)
        .and_then(|body| STANDARD.decode(body).ok())
        .ok_or_else(|| FetchError::Failed("额度接口回包为空".into()))?;
    parse_summary(&body, chrono::Utc::now().timestamp_millis())
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
    let code = doc.get("code").and_then(|code| match code {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    });
    let data = doc.get("data").filter(|data| data.is_object());
    let (Some(0), Some(data)) = (code, data) else {
        return Err(FetchError::Failed(format!(
            "额度接口返回错误（code={}）",
            code.map_or_else(|| "?".to_string(), |code| code.to_string())
        )));
    };
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
    })
}
