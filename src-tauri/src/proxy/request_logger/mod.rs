//! 代理完整请求/响应日志记录
//!
//! 把客户端发往代理的请求体与上游的响应体（流式聚合后/非流式原样）一起落到
//! 本地 JSONL（`~/.cc-switch/proxy_request_logs/<app_type>/<session-id>.jsonl`），
//! 一个会话一个文件，方便事后排查 prompt / tool_use / reasoning 的完整内容。
//!
//! - 文件名用 session-id，与各 CLI 自己写的 `<session-id>.jsonl` 日志对应，
//!   Codex 去掉内部 `codex_` 前缀，session 缺失落 `unknown-session.jsonl`。
//! - 与 `proxy_request_logs` 表的元数据互补：那张表只存 token/cost/latency，
//!   这里存完整 body。共用同一个 `request_id` 字段（UUIDv4）做关联。
//! - 写入用每文件路径一把短时锁；失败仅 warn，不影响转发。
//! - 每个应用目录只保留最新的 `request_log_max_sessions` 个会话文件，
//!   超出的最旧文件在写入后清理；`request_log_max_sessions = 0` 表示关闭记录。

pub mod aggregators;

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};
use tokio::sync::Mutex as AsyncMutex;

pub use aggregators::{aggregator_for_api_format, select_aggregator, SseAggregator};

/// 流式聚合的累计输入字节软上限。与非流式路径的 `MAX_RESPONSE_BODY_BYTES` 对齐：
/// 超过则停止聚合并标记 truncated，流照常透传，仅 request-log 不再记录后续内容。
/// 聚合状态会完整驻留内存，故用同一上限防止超长/恶意流式响应导致 OOM。
const MAX_AGGREGATED_BYTES: usize = 128 * 1024 * 1024;

/// 一条完整日志记录
#[derive(Debug, Clone, Serialize)]
pub struct RequestLogRecord {
    /// 请求进入代理的时刻（UTC RFC3339，由 RequestContext 捕获）
    #[serde(rename = "startTime")]
    pub start_time: String,
    /// 请求结束（记录构造）的时刻（UTC RFC3339）
    #[serde(rename = "endTime")]
    pub end_time: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "providerId")]
    pub provider_id: String,
    #[serde(rename = "appType")]
    pub app_type: String,
    pub method: String,
    pub endpoint: String,
    pub model: String,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    #[serde(rename = "isStreaming")]
    pub is_streaming: bool,
    #[serde(rename = "requestHeaders")]
    pub request_headers: Value,
    #[serde(rename = "requestBody")]
    pub request_body: Value,
    #[serde(rename = "responseHeaders")]
    pub response_headers: Value,
    #[serde(rename = "responseBody")]
    pub response_body: Value,
    #[serde(rename = "statusCode")]
    pub status_code: u16,
    pub error: Option<String>,
    /// session_id 是否由客户端真实提供。兜底生成的 UUID 不能用来命名会话文件
    /// （它和 CLI 写的日志文件名对不上），这类记录落到 `<app>/unknown-session.jsonl`。
    #[serde(skip)]
    pub session_client_provided: bool,
}

impl RequestLogRecord {
    /// 构建一条记录。`end_time` 自动填当前 UTC，`start_time` 由调用方传入
    /// （RequestContext 捕获的请求进入时刻）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start_time: String,
        request_id: String,
        session_id: String,
        session_client_provided: bool,
        provider_id: String,
        app_type: String,
        method: String,
        endpoint: String,
        model: String,
        duration_ms: u64,
        is_streaming: bool,
        request_headers: Value,
        request_body: Value,
        response_headers: Value,
        response_body: Value,
        status_code: u16,
        error: Option<String>,
    ) -> Self {
        Self {
            start_time,
            end_time: Utc::now().to_rfc3339(),
            request_id,
            session_id,
            session_client_provided,
            provider_id,
            app_type,
            method,
            endpoint,
            model,
            duration_ms,
            is_streaming,
            request_headers,
            request_body,
            response_headers,
            response_body,
            status_code,
            error,
        }
    }
}

/// 记录原始 headers（不脱敏）：请求日志用于本地排障与 cURL 重放，
/// 保留真实凭据才能直接复制执行。
pub fn sanitize_request_headers(headers: &http::HeaderMap) -> Value {
    headers_to_value(headers)
}

pub fn sanitize_response_headers(headers: &http::HeaderMap) -> Value {
    headers_to_value(headers)
}

fn headers_to_value(headers: &http::HeaderMap) -> Value {
    Value::Array(
        headers
            .iter()
            .map(|(name, value)| {
                serde_json::json!({
                    "name": name.as_str(),
                    "value": value.to_str().unwrap_or("[NON_UTF8]"),
                })
            })
            .collect(),
    )
}

/// 全局每文件路径锁注册表。避免同一天文件被多个写入并发追加导致行被打散。
static FILE_LOCKS: once_cell::sync::Lazy<StdMutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>> =
    once_cell::sync::Lazy::new(|| StdMutex::new(HashMap::new()));

fn lock_for(path: &Path) -> Arc<AsyncMutex<()>> {
    let mut guard = FILE_LOCKS.lock().expect("file locks registry poisoned");
    guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

/// 把记录追加到对应会话文件。失败仅 warn。
///
/// 路径布局：`<request_log_dir>/<app_type>/<session-id>.jsonl`，一个会话一个文件，
/// 与各 CLI 自己写的 `<session-id>.jsonl` 日志文件名对应，方便对照查看。
/// session_id 缺失（兜底生成的 UUID）时落到 `<app_type>/unknown-session.jsonl`。
///
/// 写入后按 `max_sessions` 清理该 app 目录下多余的旧会话文件；
/// 只在本次写入创建了新会话文件时才扫描目录（追加不改变文件数）。
/// 该函数在 tokio 异步上下文里调用，但落盘用 `spawn_blocking` 移到阻塞线程池，
/// 避免阻塞 reactor 线程。
pub async fn append_record(record: RequestLogRecord, max_sessions: u64) {
    let dir = match crate::config::get_proxy_request_log_dir() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("[request_logger] 获取日志目录失败: {e}");
            return;
        }
    };

    let app_dir = dir.join(sanitize_path_component(&record.app_type));
    let file_stem = session_file_stem(&record);
    let path = app_dir.join(format!("{file_stem}.jsonl"));

    let mut line = match serde_json::to_string(&record) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[request_logger] 序列化记录失败: {e}");
            return;
        }
    };
    line.push('\n');

    let path_for_blocking = path.clone();
    let lock = lock_for(&path);
    let _held = lock.lock().await;
    let write_result =
        tokio::task::spawn_blocking(move || write_line_with_freshness(&path_for_blocking, &line))
            .await;
    let created_new_file = match write_result {
        Ok(Ok(created)) => created,
        Ok(Err(e)) => {
            log::warn!("[request_logger] 写入失败 {}: {e}", path.display());
            return;
        }
        Err(e) => {
            log::warn!("[request_logger] 写入任务 join 失败: {e}");
            return;
        }
    };
    drop(_held);

    if max_sessions > 0 && created_new_file {
        let prune_result =
            tokio::task::spawn_blocking(move || prune_old_session_files(&app_dir, max_sessions))
                .await;
        match prune_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("[request_logger] 清理旧会话文件失败: {e}"),
            Err(e) => log::warn!("[request_logger] 清理任务 join 失败: {e}"),
        }
    }
}

/// 清理 app 目录下超出 `max_sessions` 的最旧会话文件（按修改时间降序保留前 N 个）。
fn prune_old_session_files(app_dir: &Path, max_sessions: u64) -> std::io::Result<()> {
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(app_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((entry.path(), modified))
        })
        .collect();
    if entries.len() as u64 <= max_sessions {
        return Ok(());
    }
    entries.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified)); // 新→旧
    for (path, _) in entries.into_iter().skip(max_sessions as usize) {
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!(
                "[request_logger] 删除旧会话文件失败 {}: {e}",
                path.display()
            );
        }
    }
    Ok(())
}

/// 计算会话文件名（不含扩展名）。
///
/// - 客户端未提供稳定 session_id（兜底 UUID）→ `unknown-session`
/// - 去掉 Codex 内部加的 `codex_` 前缀，让文件名与 Codex CLI 的 `<uuid>.jsonl` 对应
/// - 做文件名安全清洗（防止 `/`、`..` 等穿越目录）
fn session_file_stem(record: &RequestLogRecord) -> String {
    if !record.session_client_provided {
        return "unknown-session".to_string();
    }
    let raw = record
        .session_id
        .strip_prefix("codex_")
        .unwrap_or(&record.session_id);
    let cleaned = sanitize_path_component(raw);
    if cleaned.is_empty() {
        "unknown-session".to_string()
    } else {
        cleaned
    }
}

/// 文件名/目录名安全清洗：只保留字母数字与 `-` `_` `.`，其余替换为 `_`。
/// 同时拒绝纯 `.` / `..`，避免目录穿越。
pub fn sanitize_path_component(input: &str) -> String {
    let mut out: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // 截断过长文件名（部分文件系统 255 字节上限），留出扩展名空间
    if out.len() > 200 {
        out.truncate(200);
    }
    if out.is_empty() || out == "." || out == ".." || out.chars().all(|c| c == '.') {
        return String::new();
    }
    out
}

/// 每个请求写一行后都要 `spawn_blocking` 剪枝，会频繁读同一个 app 目录。
/// 只在「新建会话文件」时才真正扫描——追加到已存在的文件不会改变目录内
/// 文件数量，剪枝结果不变。文件存在性检查与写线合并在同一个 blocking 任务里。
fn write_line_with_freshness(path: &Path, line: &str) -> std::io::Result<bool> {
    use std::io::Write;
    // 确保 per-app 子目录存在（顶层目录已由 get_proxy_request_log_dir 创建）
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(line.as_bytes())?;
    file.flush()?;
    Ok(is_new)
}

// ============================================================================
// SSE 流式聚合 collector + finish guard（与 SseUsageCollector 同款形态）
// ============================================================================

type AggregatorSlot = Arc<AsyncMutex<Option<Box<dyn SseAggregator + Send>>>>;

#[derive(Clone)]
pub struct SseRequestLogCollector {
    inner: Arc<SseRequestLogCollectorInner>,
}

struct SseRequestLogCollectorInner {
    aggregator: AggregatorSlot,
    finished: AtomicBool,
    /// 累计喂入的 SSE event 字节数，超 `MAX_AGGREGATED_BYTES` 后停止 ingest。
    accumulated_bytes: AtomicUsize,
    /// 是否因超限停止了聚合。finalize 时据此在产物里标记 truncated。
    truncated: AtomicBool,
    on_complete: Box<dyn Fn(Value) + Send + Sync + 'static>,
}

impl SseRequestLogCollector {
    /// 创建一个流式 request-log 收集器。
    ///
    /// `aggregator` 由 `select_aggregator(app_type, endpoint)` 提供。
    /// `on_complete` 在流结束（或被 Drop guard 触发）时拿到聚合后的完整 response JSON。
    pub fn new(
        aggregator: Box<dyn SseAggregator + Send>,
        on_complete: impl Fn(Value) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(SseRequestLogCollectorInner {
                aggregator: Arc::new(AsyncMutex::new(Some(aggregator))),
                finished: AtomicBool::new(false),
                accumulated_bytes: AtomicUsize::new(0),
                truncated: AtomicBool::new(false),
                on_complete: Box::new(on_complete),
            }),
        }
    }

    /// 推入一个解析过的 SSE event JSON。
    pub async fn push(&self, value: Value) {
        let mut guard = self.inner.aggregator.lock().await;
        let Some(agg) = guard.as_mut() else {
            return;
        };
        // 超限后停止 ingest：流照常透传，仅不再记录后续 event。
        if self.inner.truncated.load(Ordering::Relaxed) {
            return;
        }
        let chunk_bytes = value.to_string().len();
        let prev = self
            .inner
            .accumulated_bytes
            .fetch_add(chunk_bytes, Ordering::Relaxed);
        if prev + chunk_bytes > MAX_AGGREGATED_BYTES {
            self.inner.truncated.store(true, Ordering::Relaxed);
            return;
        }
        agg.ingest_event(&value);
    }

    /// 结束聚合并触发回调。幂等。
    pub async fn finish(&self) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        let aggregator = {
            let mut guard = self.inner.aggregator.lock().await;
            guard.take()
        };
        if let Some(agg) = aggregator {
            let mut value = agg.finalize();
            if self.inner.truncated.load(Ordering::Relaxed) {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("requestLogTruncated".to_string(), Value::Bool(true));
                }
            }
            (self.inner.on_complete)(value);
        }
    }
}

/// RAII guard，确保 stream 在中途被丢弃（客户端断开等）时也会触发 finish。
/// 与 `SseUsageFinishGuard` 同款形态。
pub struct SseRequestLogFinishGuard {
    collector: Option<SseRequestLogCollector>,
}

impl SseRequestLogFinishGuard {
    pub fn new(collector: SseRequestLogCollector) -> Self {
        Self {
            collector: Some(collector),
        }
    }

    pub fn disarm(&mut self) {
        self.collector = None;
    }
}

impl Drop for SseRequestLogFinishGuard {
    fn drop(&mut self) {
        if let Some(collector) = self.collector.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    collector.finish().await;
                });
            } else {
                log::warn!("Request-log 收尾保护触发时 Tokio runtime 不可用，跳过异步 finish");
            }
        }
    }
}

/// 旁路（tee）一条上游原始 SSE 字节流：原样透传每个 chunk，同时把解析出的
/// SSE event JSON 喂给 `collector`，流结束时触发其 finalize 落盘。
///
/// 用于 Claude transform 路径——上游是 OpenAI/Responses SSE，会被转换器改写成
/// Anthropic SSE 后再返回客户端。request-log 需要的是**转换前**的上游原始报文，
/// 所以在转换器消费之前先在这里旁路一份。
///
/// 与 `response_processor::create_logged_passthrough_stream` 的区别：那个挂在
/// 转换**后**的流上（记 Anthropic 形态、走 usage 收集器）；这个挂在转换**前**
/// 的上游流上（记上游原始形态）。两者互补，互不干扰。
pub fn tee_raw_sse_for_request_log<S>(
    stream: S,
    collector: SseRequestLogCollector,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send
where
    S: futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static,
{
    use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
    use futures::StreamExt;

    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut finish_guard = SseRequestLogFinishGuard::new(collector.clone());

        tokio::pin!(stream);

        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        for line in block.lines() {
                            if let Some(data) = strip_sse_field(line, "data") {
                                let data = data.trim();
                                if data.is_empty() || data == "[DONE]" {
                                    continue;
                                }
                                if let Ok(value) = serde_json::from_str::<Value>(data) {
                                    collector.push(value).await;
                                }
                            }
                        }
                    }
                    yield Ok(bytes);
                }
                Err(e) => {
                    yield Err(e);
                }
            }
        }

        collector.finish().await;
        finish_guard.disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record_with(session_id: &str, client_provided: bool, app_type: &str) -> RequestLogRecord {
        RequestLogRecord::new(
            "2026-09-17T08:38:05.123Z".to_string(),
            "req-1".to_string(),
            session_id.to_string(),
            client_provided,
            "provider-1".to_string(),
            app_type.to_string(),
            "POST".to_string(),
            "/v1/messages".to_string(),
            "claude-opus-4-7".to_string(),
            0,
            true,
            json!([]),
            json!({"model": "claude-opus-4-7"}),
            json!([]),
            json!({"id": "msg_1"}),
            200,
            None,
        )
    }

    #[test]
    fn session_file_stem_uses_session_id_when_client_provided() {
        let r = record_with("abc-123-def", true, "claude");
        assert_eq!(session_file_stem(&r), "abc-123-def");
    }

    #[test]
    fn session_file_stem_strips_codex_prefix() {
        let r = record_with("codex_9f8e7d6c-uuid", true, "codex");
        assert_eq!(session_file_stem(&r), "9f8e7d6c-uuid");
    }

    #[test]
    fn session_file_stem_falls_back_when_not_client_provided() {
        // 兜底生成的 UUID（client_provided=false）不能当会话名
        let r = record_with("random-generated-uuid", false, "claude");
        assert_eq!(session_file_stem(&r), "unknown-session");
    }

    #[test]
    fn session_file_stem_falls_back_when_empty_after_clean() {
        let r = record_with("///", true, "claude");
        // 清洗后是 "___"，非空，所以保留清洗结果而非兜底
        assert_eq!(session_file_stem(&r), "___");
    }

    #[test]
    fn sanitize_rejects_path_traversal() {
        assert_eq!(sanitize_path_component(".."), "");
        assert_eq!(sanitize_path_component("."), "");
        assert_eq!(sanitize_path_component("a/../b"), "a_.._b");
        assert_eq!(sanitize_path_component("sess/../../etc"), "sess_.._.._etc");
    }

    #[test]
    fn sanitize_keeps_safe_chars() {
        assert_eq!(
            sanitize_path_component("01HX9-abc_DEF.123"),
            "01HX9-abc_DEF.123"
        );
    }

    #[test]
    fn sanitize_request_headers_keeps_original_values() {
        let mut headers = http::HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert("anthropic-beta", "claude-code-20250219".parse().unwrap());
        headers.append("anthropic-beta", "context-1m-2025-08-07".parse().unwrap());
        headers.insert(
            "user-agent",
            "claude-cli/1.0.119 (external, cli)".parse().unwrap(),
        );
        headers.insert("x-app", "cli".parse().unwrap());
        headers.insert("x-client-token", "secret-token".parse().unwrap());
        headers.insert("host", "api.anthropic.com".parse().unwrap());
        headers.insert("x-api-key", "sk-ant-abc123".parse().unwrap());

        let entries = sanitize_request_headers(&headers);
        let entries = entries.as_array().unwrap();
        let values_for = |name: &str| -> Vec<&str> {
            entries
                .iter()
                .filter(|entry| entry["name"] == name)
                .filter_map(|entry| entry["value"].as_str())
                .collect()
        };

        // 原始值完整保留：请求日志用于本地排障与 cURL 重放
        assert_eq!(values_for("authorization"), vec!["Bearer secret"]);
        assert_eq!(values_for("x-client-token"), vec!["secret-token"]);
        assert_eq!(values_for("host"), vec!["api.anthropic.com"]);
        assert_eq!(values_for("x-api-key"), vec!["sk-ant-abc123"]);
        assert_eq!(values_for("anthropic-version"), vec!["2023-06-01"]);
        assert_eq!(
            values_for("anthropic-beta"),
            vec!["claude-code-20250219", "context-1m-2025-08-07"]
        );
        assert_eq!(
            values_for("user-agent"),
            vec!["claude-cli/1.0.119 (external, cli)"]
        );
        assert_eq!(values_for("x-app"), vec!["cli"]);
    }

    #[test]
    fn sanitize_response_headers_keeps_original_values() {
        let mut headers = http::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("set-cookie", "sid=secret".parse().unwrap());
        headers.insert("www-authenticate", "Bearer token=secret".parse().unwrap());
        headers.insert("x-response-token", "secret-token".parse().unwrap());
        headers.insert("host", "api.example.com:8443".parse().unwrap());

        let entries = sanitize_response_headers(&headers);
        let entries = entries.as_array().unwrap();
        let values_for = |name: &str| -> Vec<&str> {
            entries
                .iter()
                .filter(|entry| entry["name"] == name)
                .filter_map(|entry| entry["value"].as_str())
                .collect()
        };

        assert_eq!(values_for("content-type"), vec!["application/json"]);
        assert_eq!(values_for("set-cookie"), vec!["sid=secret"]);
        assert_eq!(values_for("www-authenticate"), vec!["Bearer token=secret"]);
        assert_eq!(values_for("x-response-token"), vec!["secret-token"]);
        assert_eq!(values_for("host"), vec!["api.example.com:8443"]);
    }

    #[test]
    fn session_id_is_serialized_in_record() {
        let r = record_with("sess-42", true, "claude");
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["sessionId"], "sess-42");
        assert_eq!(v["startTime"], "2026-09-17T08:38:05.123Z");
        // endTime 由 new() 自动填当前 UTC，只要存在且非空即可
        assert!(v["endTime"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(v.get("ts").is_none());
        assert_eq!(v["method"], "POST");
        assert_eq!(v["endpoint"], "/v1/messages");
        assert!(v.get("url").is_none());
        assert_eq!(v["requestHeaders"], json!([]));
        assert_eq!(v["requestBody"], json!({"model": "claude-opus-4-7"}));
        assert_eq!(v["responseHeaders"], json!([]));
        assert_eq!(v["responseBody"], json!({"id": "msg_1"}));
        assert!(v.get("response").is_none());
        assert!(v.get("request").is_none());
        assert!(v.get("requestBodyRaw").is_none());
        // session_client_provided 标记不应出现在落盘 JSON 里
        assert!(v.get("session_client_provided").is_none());
        assert!(v.get("sessionClientProvided").is_none());
        assert!(v.get("perspective").is_none());
    }
}
