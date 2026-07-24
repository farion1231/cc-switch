//! Grok (xAI) 官方订阅额度查询
//!
//! 读取 Grok CLI 的 OAuth 凭据（~/.grok/auth.json），调用 grok.com 的
//! gRPC-web billing 端点查询 SuperGrok 订阅的 credit 用量。
//!
//! 实现移植自 CodexBar（steipete/CodexBar）的 Grok provider：
//! - 凭据：`GrokAuth.swift` —— auth.json 是以 OIDC scope URL 为 key 的 map，
//!   优先 SuperGrok 的 `https://auth.x.ai::<client-id>` 条目，回退 legacy
//!   session 条目；`key` 字段即 Bearer token。
//! - 查询：`GrokWebBillingFetcher.swift` —— POST 空 gRPC-web 帧到
//!   `GetGrokCreditsConfig`，响应无公开 .proto，用通用 protobuf 扫描按
//!   字段路径启发式提取已用百分比与重置时间。
//! - token 刷新由 Grok CLI 自己负责（约 7 天过期），本模块只读不刷新，
//!   过期时引导用户重新 `grok login`。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::services::subscription::{
    CredentialStatus, QuotaTier, SubscriptionQuota, TIER_CREDITS, TIER_MONTHLY, TIER_WEEKLY_LIMIT,
};

const GROK_BILLING_ENDPOINT: &str =
    "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";

/// SuperGrok（OIDC）条目的 scope 前缀
const OIDC_SCOPE_PREFIX: &str = "https://auth.x.ai::";
/// 旧版 `grok login` 的 session scope
const LEGACY_SESSION_SCOPE: &str = "https://accounts.x.ai/sign-in";

const RELOGIN_HINT: &str = "Please re-login with `grok login`.";

// ── 凭据读取 ──────────────────────────────────────────────

/// (access_token, status, message)
type GrokCredentials = (Option<String>, CredentialStatus, Option<String>);

/// 读取 Grok CLI 的 OAuth 凭据（~/.grok/auth.json，目录可被设置覆盖）
fn read_grok_credentials() -> GrokCredentials {
    let auth_path = crate::grok_config::get_grok_config_dir().join("auth.json");

    if !auth_path.exists() {
        return (None, CredentialStatus::NotFound, None);
    }

    let content = match std::fs::read_to_string(&auth_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to read Grok auth file: {e}")),
            );
        }
    };

    parse_grok_auth_json(&content)
}

/// 解析 auth.json：顶层是 scope → 条目的 map，选出首选条目并检查过期
fn parse_grok_auth_json(content: &str) -> GrokCredentials {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            return (
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to parse Grok auth JSON: {e}")),
            );
        }
    };

    let root = match parsed.as_object() {
        Some(o) => o,
        None => {
            return (
                None,
                CredentialStatus::ParseError,
                Some("Grok auth.json root is not an object".to_string()),
            );
        }
    };

    let entry = match select_preferred_entry(root) {
        Some(e) => e,
        None => {
            return (
                None,
                CredentialStatus::ParseError,
                Some("Grok auth.json contains no usable access token".to_string()),
            );
        }
    };

    // select_preferred_entry 已保证 key 非空
    let access_token = entry
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if let Some(expires_at) = entry.get("expires_at").and_then(|v| v.as_str()) {
        if is_iso_expired(expires_at) {
            return (
                Some(access_token),
                CredentialStatus::Expired,
                Some("Grok OAuth token has expired".to_string()),
            );
        }
    }

    (Some(access_token), CredentialStatus::Valid, None)
}

/// 选择首选凭据条目：OIDC（SuperGrok）优先，legacy session 兜底。
///
/// 只接受 `key` 非空的条目——残缺的 OIDC 记录不能遮蔽健康的 legacy 条目
/// （与 CodexBar `selectPreferredEntry` 一致）。
fn select_preferred_entry(
    root: &serde_json::Map<String, serde_json::Value>,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let mut oidc_candidate = None;
    let mut legacy_candidate = None;

    for (scope, value) in root {
        let entry = match value.as_object() {
            Some(e) => e,
            None => continue,
        };
        let has_key = entry
            .get("key")
            .and_then(|v| v.as_str())
            .is_some_and(|k| !k.is_empty());
        if !has_key {
            continue;
        }
        if scope.starts_with(OIDC_SCOPE_PREFIX) {
            oidc_candidate = Some(entry);
        } else if scope == LEGACY_SESSION_SCOPE || scope.contains("/sign-in") {
            legacy_candidate = Some(entry);
        }
    }

    oidc_candidate.or(legacy_candidate)
}

/// 判断 ISO 8601 时间串是否已过期；无法解析时不视为过期
fn is_iso_expired(iso: &str) -> bool {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) {
        dt.timestamp() < now_secs
    } else if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.f") {
        dt.and_utc().timestamp() < now_secs
    } else {
        false
    }
}

// ── gRPC-web 帧与 protobuf 解析 ──────────────────────────

/// protobuf 扫描收集到的字段（路径 = 从根到该字段的 field number 链）
#[derive(Default)]
struct ProtobufScan {
    /// (path, float 值, 出现顺序)
    fixed32_fields: Vec<(Vec<u64>, f32, usize)>,
    /// (path, varint 值)
    varint_fields: Vec<(Vec<u64>, u64)>,
}

fn read_varint(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    while *index < bytes.len() && shift < 64 {
        let byte = bytes[*index];
        *index += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

/// 递归扫描 protobuf 消息，收集 varint 与 fixed32 字段。
///
/// 无 .proto 定义，length-delimited 字段一律当嵌套消息试扫（深度 ≤4）；
/// 无法解析的字节从字段起点 +1 重新同步。返回下一个 fixed32 序号。
fn scan_protobuf(
    bytes: &[u8],
    depth: usize,
    path: &[u64],
    order: usize,
    scan: &mut ProtobufScan,
) -> usize {
    let mut index = 0;
    let mut next_order = order;

    while index < bytes.len() {
        let field_start = index;
        let key = match read_varint(bytes, &mut index) {
            Some(k) if k != 0 => k,
            _ => {
                index = field_start + 1;
                continue;
            }
        };
        let field_number = key >> 3;
        let wire_type = key & 0x07;
        let mut field_path = path.to_vec();
        field_path.push(field_number);

        match wire_type {
            0 => match read_varint(bytes, &mut index) {
                Some(value) => scan.varint_fields.push((field_path, value)),
                None => index = field_start + 1,
            },
            1 => {
                if index + 8 > bytes.len() {
                    return next_order;
                }
                index += 8;
            }
            2 => {
                let length = match read_varint(bytes, &mut index) {
                    Some(l) if l <= (bytes.len() - index) as u64 => l as usize,
                    _ => {
                        index = field_start + 1;
                        continue;
                    }
                };
                let end = index + length;
                if depth < 4 {
                    next_order =
                        scan_protobuf(&bytes[index..end], depth + 1, &field_path, next_order, scan);
                }
                index = end;
            }
            5 => {
                if index + 4 > bytes.len() {
                    return next_order;
                }
                let bits = u32::from_le_bytes([
                    bytes[index],
                    bytes[index + 1],
                    bytes[index + 2],
                    bytes[index + 3],
                ]);
                scan.fixed32_fields
                    .push((field_path, f32::from_bits(bits), next_order));
                next_order += 1;
                index += 4;
            }
            _ => index = field_start + 1,
        }
    }

    next_order
}

/// 拆出 gRPC-web data 帧（flags 高位 0x80 的 trailer 帧跳过）。
/// 任一帧长度非法时返回空——调用方再按裸 protobuf 兜底。
fn grpc_web_data_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut index = 0;
    while index < data.len() {
        if index + 5 > data.len() {
            return Vec::new();
        }
        let flags = data[index];
        let length = u32::from_be_bytes([
            data[index + 1],
            data[index + 2],
            data[index + 3],
            data[index + 4],
        ]) as usize;
        let start = index + 5;
        let end = start + length;
        if end > data.len() {
            return Vec::new();
        }
        if flags & 0x80 == 0 {
            frames.push(&data[start..end]);
        }
        index = end;
    }
    frames
}

/// 响应体没有帧头时，看首字节是否像合法 protobuf tag（某些成功请求直接返回裸 protobuf）
fn looks_like_protobuf_payload(data: &[u8]) -> bool {
    match data.first() {
        Some(&first) => {
            let field_number = first >> 3;
            let wire_type = first & 0x07;
            field_number > 0 && matches!(wire_type, 0 | 1 | 2 | 5)
        }
        None => false,
    }
}

/// 从 trailer 帧（flags & 0x80）解析 `grpc-status` / `grpc-message` 等字段
fn grpc_web_trailer_fields(data: &[u8]) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let mut index = 0;
    while index + 5 <= data.len() {
        let flags = data[index];
        let length = u32::from_be_bytes([
            data[index + 1],
            data[index + 2],
            data[index + 3],
            data[index + 4],
        ]) as usize;
        let start = index + 5;
        let end = start + length;
        if end > data.len() {
            break;
        }
        if flags & 0x80 != 0 {
            if let Ok(text) = std::str::from_utf8(&data[start..end]) {
                for line in text.lines().filter(|l| !l.is_empty()) {
                    if let Some((key, value)) = line.split_once(':') {
                        fields.insert(key.trim().to_lowercase(), percent_decode(value.trim()));
                    }
                }
            }
        }
        index = end;
    }
    fields
}

/// gRPC message 使用 percent-encoding；解码失败的序列原样保留
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // 只切字节切片再校验 UTF-8：对 &str 按字节切片会在多字节字符
            // 边界内 panic（trailer 内容由服务端控制，可含任意 UTF-8）
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 解析出的账单快照
struct GrokBillingSnapshot {
    used_percent: f64,
    /// Unix 秒
    resets_at: Option<i64>,
}

/// 从响应体提取已用百分比与重置时间（CodexBar `parseGRPCWebResponse` 的移植）。
///
/// 启发式：
/// - 百分比：wire-type 5 (float) 中路径末段为 1、值域 [0,100] 的字段，
///   取路径最浅、出现最早的那个；
/// - 重置时间：varint 中值落在合理 Unix 秒区间且晚于当前时刻的字段，
///   优先精确路径 [1,5,1]，否则取最近的未来时间；
/// - 零用量特判：proto3 会省略值为 0 的 percent 字段，此时若存在重置时间
///   和用量周期标记（路径 [1,6,*] 或 [1,8,1]=1/2），按 0% 处理。
fn parse_billing_payload(data: &[u8], now_secs: i64) -> Result<GrokBillingSnapshot, String> {
    let mut payloads = grpc_web_data_frames(data);
    if payloads.is_empty() && looks_like_protobuf_payload(data) {
        payloads = vec![data];
    }
    if payloads.is_empty() {
        return Err("Grok billing response contained no protobuf payload".to_string());
    }

    let mut scan = ProtobufScan::default();
    for payload in payloads {
        // 与 CodexBar 一致：fixed32 序号在每个顶层 data 帧内独立从 0 计数
        scan_protobuf(payload, 0, &[], 0, &mut scan);
    }

    let parsed_percent = scan
        .fixed32_fields
        .iter()
        .filter(|(path, value, _)| {
            path.last() == Some(&1) && value.is_finite() && *value >= 0.0 && *value <= 100.0
        })
        .min_by_key(|(path, _, order)| (path.len(), *order))
        .map(|(_, value, _)| f64::from(*value));

    let reset_candidates: Vec<(&[u64], i64)> = scan
        .varint_fields
        .iter()
        .filter(|(_, value)| (1_700_000_000..=2_100_000_000).contains(value))
        .map(|(path, value)| (path.as_slice(), *value as i64))
        .filter(|(_, ts)| *ts > now_secs)
        .collect();
    let reset = reset_candidates
        .iter()
        .filter(|(path, _)| *path == [1, 5, 1])
        .map(|(_, ts)| *ts)
        .min()
        .or_else(|| reset_candidates.iter().map(|(_, ts)| *ts).min());

    let has_usage_period = scan.varint_fields.iter().any(|(path, value)| {
        path.starts_with(&[1, 6]) || (path.as_slice() == [1, 8, 1] && (*value == 1 || *value == 2))
    });
    let no_usage_yet = parsed_percent.is_none()
        && scan.fixed32_fields.is_empty()
        && reset.is_some()
        && has_usage_period;

    let used_percent = match parsed_percent.or(if no_usage_yet { Some(0.0) } else { None }) {
        Some(p) => p,
        None => return Err("Could not locate usage percent in Grok billing response".to_string()),
    };

    Ok(GrokBillingSnapshot {
        used_percent,
        resets_at: reset,
    })
}

// ── API 查询 ──────────────────────────────────────────────

/// 认证类失败（token 无效/过期）的 gRPC 状态判定，
/// 移植自 CodexBar `GrokWebBillingError.isAuthenticationFailure`
fn is_grpc_auth_failure(status: i64, message: &str) -> bool {
    if status == 16 {
        return true;
    }
    if status != 7 {
        return false;
    }
    let lower = message.to_lowercase();
    lower.contains("bad-credentials")
        || lower.contains("unauthenticated")
        || (lower.contains("oauth2") && lower.contains("could not be validated"))
        || (lower.contains("access token")
            && (lower.contains("invalid")
                || lower.contains("expired")
                || lower.contains("could not be validated")))
}

/// xAI 尚未提供团队主体的用量接口，识别其专属失败以给出可读提示
fn is_team_billing_unavailable(status: i64, message: &str) -> bool {
    status == 9
        && matches!(
            message.trim().to_lowercase().as_str(),
            "no personal team" | "no personal team."
        )
}

/// 瞬时性 gRPC 状态：DEADLINE_EXCEEDED(4) / UNAVAILABLE(14)，以及带超时
/// 文案的 CANCELLED(1)。语义上等价 HTTP 504/503，对齐 CodexBar `shouldRetry`
/// 的 rpcFailed 分支
fn is_transient_grpc_status(status: i64, message: &str) -> bool {
    match status {
        4 | 14 => true,
        1 => {
            let lower = message.to_lowercase();
            lower.contains("timeout") || lower.contains("deadline") || lower.contains("expired")
        }
        _ => false,
    }
}

/// 将非 0 的 gRPC 状态映射为失败。
///
/// 瞬时状态（超时/不可用）→ `Err`：前端 react-query retry + keep-last-good
/// 保留上次成功值，托盘保留旧快照；折叠成 `Ok(success:false)` 会因错误文案
/// 匹配不到前端 `isTransientUsageError` 的任何瞬时模式而被当确定性失败，
/// 一次服务端抖动就清掉展示值与 lastGood 快照。其余状态 → 确定性失败快照。
/// header（trailers-only 响应）与 body trailer 两条路径都必须走这里。
fn grpc_status_failure(
    status: i64,
    message: &str,
    tool_label: &str,
    relogin_hint: &str,
) -> Result<SubscriptionQuota, String> {
    if is_transient_grpc_status(status, message) {
        return Err(format!(
            "Transient gRPC failure (grpc-status {status}): {message}"
        ));
    }
    Ok(grpc_status_error(status, message, tool_label, relogin_hint))
}

/// 将非 0 的 gRPC 状态映射为确定性失败快照
fn grpc_status_error(
    status: i64,
    message: &str,
    tool_label: &str,
    relogin_hint: &str,
) -> SubscriptionQuota {
    if is_grpc_auth_failure(status, message) {
        return SubscriptionQuota::error(
            tool_label,
            CredentialStatus::Expired,
            format!("Grok credentials were rejected (grpc-status {status}). {relogin_hint}"),
        );
    }
    if is_team_billing_unavailable(status, message) {
        return SubscriptionQuota::error(
            tool_label,
            CredentialStatus::Valid,
            "Grok team usage is not available from the billing API yet".to_string(),
        );
    }
    SubscriptionQuota::error(
        tool_label,
        CredentialStatus::Valid,
        format!("Grok billing RPC failed (grpc-status {status}): {message}"),
    )
}

/// 按重置时间距今的天数推断窗口 tier 名（CodexBar `primaryLabel` 的阈值）：
/// 4–12 天 → 周窗口，20–45 天 → 月窗口，其余 → 通用 credit 额度
fn tier_name_for_reset(resets_at: Option<i64>, now_secs: i64) -> &'static str {
    if let Some(ts) = resets_at {
        let days = ((ts - now_secs) as f64 / 86400.0).round() as i64;
        if (4..=12).contains(&days) {
            return TIER_WEEKLY_LIMIT;
        }
        if (20..=45).contains(&days) {
            return TIER_MONTHLY;
        }
    }
    TIER_CREDITS
}

/// 查询 Grok 官方订阅额度
///
/// 与 claude/codex/gemini 同一约定：瞬时传输失败返回 `Err`（前端 retry +
/// 保留上次成功值），确定性失败返回 `Ok(success:false)`。
///
/// 参数化 `tool_label` / `relogin_hint` 让该函数可被两个调用点共用（与
/// `query_codex_quota` 的双调用点设计一致）：
/// - `"grokbuild"` + "grok login"（Grok CLI 凭据路径）
/// - `"xai_oauth"` + "re-login via cc-switch"（cc-switch 自管 xAI OAuth 路径，
///   见 `commands::xai_oauth::get_xai_oauth_quota`；两者是同一个 OAuth client，
///   token 对 grok.com 账单端点等效）
pub(crate) async fn query_grok_quota(
    access_token: &str,
    tool_label: &str,
    relogin_hint: &str,
) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    // 空 gRPC-web 帧：1 字节 flags + 4 字节大端长度 0
    let resp = client
        .post(GROK_BILLING_ENDPOINT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Origin", "https://grok.com")
        .header("Referer", "https://grok.com/?_s=usage")
        .header("Accept", "*/*")
        .header("Content-Type", "application/grpc-web+proto")
        .header("x-grpc-web", "1")
        .header("x-user-agent", "connect-es/2.1.1")
        .header("User-Agent", "cc-switch")
        .body(vec![0u8; 5])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota::error(
            tool_label,
            CredentialStatus::Expired,
            format!("Authentication failed (HTTP {status}). {relogin_hint}"),
        ));
    }

    // HTTP 408 与 grpc-status 4 同为服务端超时，以 Err 传播（前端 retry +
    // keep-last-good）；折叠进下方通用分支会因前端 isTransientUsageError 只认
    // 5xx/429 为瞬时而清掉 lastGood。CodexBar 的 shouldRetry 同样重试 408，
    // 其余的 502/503/504 前端已按 5xx 识别为瞬时，维持 Ok(success:false)。
    if status == reqwest::StatusCode::REQUEST_TIMEOUT {
        return Err(format!("Transient HTTP failure (HTTP {status})"));
    }

    // gRPC 错误可能在 HTTP 头里携带（trailers-only 响应），先于响应体检查
    let header_status = resp
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok());
    let header_message = resp
        .headers()
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .unwrap_or_default();

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let body: String = body.chars().take(400).collect();
        return Ok(SubscriptionQuota::error(
            tool_label,
            CredentialStatus::Valid,
            format!("API error (HTTP {status}): {body}"),
        ));
    }

    if let Some(code) = header_status {
        if code != 0 {
            return grpc_status_failure(code, &header_message, tool_label, relogin_hint);
        }
    }

    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read API response: {e}")),
    };

    let trailers = grpc_web_trailer_fields(&raw);
    if let Some(code) = trailers
        .get("grpc-status")
        .and_then(|v| v.parse::<i64>().ok())
    {
        if code != 0 {
            let message = trailers
                .get("grpc-message")
                .map(String::as_str)
                .unwrap_or("");
            return grpc_status_failure(code, message, tool_label, relogin_hint);
        }
    }

    let now_secs = now_secs();
    let snapshot = match parse_billing_payload(&raw, now_secs) {
        Ok(s) => s,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                tool_label,
                CredentialStatus::Valid,
                format!("Failed to parse API response: {e}"),
            ));
        }
    };

    let tier = QuotaTier {
        name: tier_name_for_reset(snapshot.resets_at, now_secs).to_string(),
        utilization: snapshot.used_percent.clamp(0.0, 100.0),
        resets_at: snapshot
            .resets_at
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| dt.to_rfc3339()),
        used_value_usd: None,
        max_value_usd: None,
    };

    Ok(SubscriptionQuota {
        tool: tool_label.to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers: vec![tier],
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

/// grokbuild 的订阅额度入口（由 `subscription::get_subscription_quota` 分发）
pub(crate) async fn get_grok_subscription_quota() -> Result<SubscriptionQuota, String> {
    let (token, status, message) = read_grok_credentials();

    match status {
        CredentialStatus::NotFound => Ok(SubscriptionQuota::not_found("grokbuild")),
        CredentialStatus::ParseError => Ok(SubscriptionQuota::error(
            "grokbuild",
            CredentialStatus::ParseError,
            message.unwrap_or_else(|| "Failed to parse Grok credentials".to_string()),
        )),
        CredentialStatus::Expired => {
            // 即使过期也尝试调用 API（时钟偏差时 token 可能仍有效）
            if let Some(ref token) = token {
                let result = query_grok_quota(token, "grokbuild", RELOGIN_HINT).await?;
                if result.success {
                    return Ok(result);
                }
            }
            Ok(SubscriptionQuota::error(
                "grokbuild",
                CredentialStatus::Expired,
                format!(
                    "{} {RELOGIN_HINT}",
                    message.unwrap_or_else(|| "Grok OAuth token has expired.".to_string())
                ),
            ))
        }
        CredentialStatus::Valid => {
            let token = token.expect("token must be Some when status is Valid");
            query_grok_quota(&token, "grokbuild", RELOGIN_HINT).await
        }
    }
}

// ── 辅助函数 ──────────────────────────────────────────────

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── protobuf 构造辅助 ──

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out
    }

    fn field_varint(number: u64, value: u64) -> Vec<u8> {
        let mut out = varint(number << 3);
        out.extend(varint(value));
        out
    }

    fn field_float(number: u64, value: f32) -> Vec<u8> {
        let mut out = varint((number << 3) | 5);
        out.extend(value.to_bits().to_le_bytes());
        out
    }

    fn field_message(number: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = varint((number << 3) | 2);
        out.extend(varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    fn grpc_web_frame(flags: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![flags];
        out.extend((payload.len() as u32).to_be_bytes());
        out.extend(payload);
        out
    }

    const NOW: i64 = 1_750_000_000;

    #[test]
    fn parses_percent_and_reset_from_framed_payload() {
        // message { 1: { 1: 37.5f, 5: { 1: reset_ts } } }
        let reset_ts = (NOW + 30 * 86400) as u64;
        let inner = [
            field_float(1, 37.5),
            field_message(5, &field_varint(1, reset_ts)),
        ]
        .concat();
        let payload = field_message(1, &inner);
        let data = grpc_web_frame(0, &payload);

        let snapshot = parse_billing_payload(&data, NOW).expect("parse ok");
        assert_eq!(snapshot.used_percent, 37.5);
        assert_eq!(snapshot.resets_at, Some(reset_ts as i64));
    }

    #[test]
    fn parses_bare_protobuf_without_frame_header() {
        let payload = field_message(1, &field_float(1, 12.0));
        let snapshot = parse_billing_payload(&payload, NOW).expect("parse ok");
        assert_eq!(snapshot.used_percent, 12.0);
        assert_eq!(snapshot.resets_at, None);
    }

    #[test]
    fn prefers_shallowest_percent_candidate() {
        // 深层 [1,2,1]=99.0 不应盖过浅层 [1,1]=25.0
        let inner = [
            field_message(2, &field_float(1, 99.0)),
            field_float(1, 25.0),
        ]
        .concat();
        let payload = field_message(1, &inner);
        let data = grpc_web_frame(0, &payload);

        let snapshot = parse_billing_payload(&data, NOW).expect("parse ok");
        assert_eq!(snapshot.used_percent, 25.0);
    }

    #[test]
    fn zero_usage_period_without_percent_field_reads_as_zero() {
        // proto3 省略 0 值 percent：仅有 [1,5,1] 重置时间 + [1,6,1] 周期标记
        let reset_ts = (NOW + 7 * 86400) as u64;
        let inner = [
            field_message(5, &field_varint(1, reset_ts)),
            field_message(6, &field_varint(1, 3)),
        ]
        .concat();
        let payload = field_message(1, &inner);
        let data = grpc_web_frame(0, &payload);

        let snapshot = parse_billing_payload(&data, NOW).expect("parse ok");
        assert_eq!(snapshot.used_percent, 0.0);
        assert_eq!(snapshot.resets_at, Some(reset_ts as i64));
    }

    #[test]
    fn missing_percent_without_period_marker_is_parse_error() {
        let payload = field_message(1, &field_varint(7, 42));
        let data = grpc_web_frame(0, &payload);
        assert!(parse_billing_payload(&data, NOW).is_err());
    }

    #[test]
    fn trailer_frames_are_excluded_from_payload_and_expose_status() {
        let payload = field_message(1, &field_float(1, 50.0));
        let mut data = grpc_web_frame(0, &payload);
        data.extend(grpc_web_frame(0x80, b"grpc-status: 0\r\ngrpc-message: ok"));

        let snapshot = parse_billing_payload(&data, NOW).expect("parse ok");
        assert_eq!(snapshot.used_percent, 50.0);

        let trailers = grpc_web_trailer_fields(&data);
        assert_eq!(trailers.get("grpc-status").map(String::as_str), Some("0"));
        assert_eq!(trailers.get("grpc-message").map(String::as_str), Some("ok"));
    }

    #[test]
    fn percent_decode_unescapes_grpc_message() {
        assert_eq!(percent_decode("no%20personal%20team"), "no personal team");
        assert_eq!(percent_decode("plain"), "plain");
        // 非法序列原样保留
        assert_eq!(percent_decode("50%ZZ"), "50%ZZ");
        // '%' + ASCII + 多字节字符：不得在字符边界内切片 panic
        assert_eq!(percent_decode("bad%1é"), "bad%1é");
        assert_eq!(percent_decode("%1é"), "%1é");
    }

    #[test]
    fn auth_json_prefers_oidc_entry_over_legacy() {
        let content = r#"{
            "https://accounts.x.ai/sign-in": {"key": "legacy-token"},
            "https://auth.x.ai::client-id": {"key": "oidc-token"}
        }"#;
        let (token, status, _) = parse_grok_auth_json(content);
        assert_eq!(token.as_deref(), Some("oidc-token"));
        assert!(matches!(status, CredentialStatus::Valid));
    }

    #[test]
    fn auth_json_empty_oidc_key_falls_back_to_legacy() {
        // 残缺 OIDC 记录不遮蔽健康的 legacy 条目
        let content = r#"{
            "https://auth.x.ai::client-id": {"key": ""},
            "https://accounts.x.ai/sign-in": {"key": "legacy-token"}
        }"#;
        let (token, status, _) = parse_grok_auth_json(content);
        assert_eq!(token.as_deref(), Some("legacy-token"));
        assert!(matches!(status, CredentialStatus::Valid));
    }

    #[test]
    fn auth_json_expired_entry_reports_expired() {
        let content = r#"{
            "https://auth.x.ai::client-id": {
                "key": "token",
                "expires_at": "2020-01-01T00:00:00.000Z"
            }
        }"#;
        let (token, status, message) = parse_grok_auth_json(content);
        assert_eq!(token.as_deref(), Some("token"));
        assert!(matches!(status, CredentialStatus::Expired));
        assert!(message.is_some());
    }

    #[test]
    fn auth_json_without_usable_entry_is_parse_error() {
        let (token, status, _) = parse_grok_auth_json(r#"{"other-scope": {"key": "x"}}"#);
        assert!(token.is_none());
        assert!(matches!(status, CredentialStatus::ParseError));
    }

    #[test]
    fn tier_name_follows_reset_distance() {
        assert_eq!(
            tier_name_for_reset(Some(NOW + 7 * 86400), NOW),
            TIER_WEEKLY_LIMIT
        );
        assert_eq!(
            tier_name_for_reset(Some(NOW + 30 * 86400), NOW),
            TIER_MONTHLY
        );
        assert_eq!(tier_name_for_reset(Some(NOW + 86400), NOW), TIER_CREDITS);
        assert_eq!(tier_name_for_reset(None, NOW), TIER_CREDITS);
    }

    #[test]
    fn grpc_auth_and_team_failures_classify_correctly() {
        assert!(is_grpc_auth_failure(16, ""));
        assert!(is_grpc_auth_failure(7, "Bad-Credentials: token rejected"));
        assert!(!is_grpc_auth_failure(7, "quota exceeded"));
        assert!(is_team_billing_unavailable(9, " No Personal Team "));
        assert!(!is_team_billing_unavailable(9, "other precondition"));
    }

    #[test]
    fn transient_grpc_statuses_propagate_as_err() {
        // DEADLINE_EXCEEDED / UNAVAILABLE 无条件瞬时
        assert!(is_transient_grpc_status(4, ""));
        assert!(is_transient_grpc_status(14, ""));
        // CANCELLED 仅在带超时文案时瞬时
        assert!(is_transient_grpc_status(1, "context deadline exceeded"));
        assert!(!is_transient_grpc_status(1, "cancelled by user"));
        // 鉴权/团队/其他状态不属瞬时
        assert!(!is_transient_grpc_status(16, ""));
        assert!(!is_transient_grpc_status(9, "no personal team"));
        assert!(!is_transient_grpc_status(13, "internal"));

        // 瞬时 → Err（前端 retry + keep-last-good），确定性 → Ok(success:false)
        assert!(grpc_status_failure(4, "deadline exceeded", "grokbuild", RELOGIN_HINT).is_err());
        assert!(grpc_status_failure(14, "unavailable", "grokbuild", RELOGIN_HINT).is_err());
        let determinate = grpc_status_failure(13, "internal", "grokbuild", RELOGIN_HINT)
            .expect("determinate is Ok");
        assert!(!determinate.success);
        // tool_label 参数化：两条链路（CLI / cc-switch 自管 OAuth）标签正确落到快照
        let auth =
            grpc_status_failure(16, "", "xai_oauth", "re-login").expect("auth failure is Ok");
        assert!(matches!(auth.credential_status, CredentialStatus::Expired));
        assert_eq!(auth.tool, "xai_oauth");
    }
}
