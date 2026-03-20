use super::ProxyError;
use axum::http::HeaderMap;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

static DATA_LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);
static DATA_LOG_FILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-goog-api-key",
];

fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

pub fn format_headers(headers: &HeaderMap) -> String {
    headers
        .iter()
        .map(|(key, value)| {
            let value_str = if is_sensitive_header(key.as_str()) {
                "<redacted>".to_string()
            } else {
                value.to_str().unwrap_or("<non-utf8>").to_string()
            };
            format!("{key}={value_str}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn log_json_payload(tag: &str, trace_id: &str, label: &str, body: &Value) {
    match serde_json::to_string(body) {
        Ok(body_str) => {
            log::debug!(
                "[{tag}] [trace={trace_id}] {label} ({} bytes): {}",
                body_str.len(),
                body_str
            );
        }
        Err(err) => {
            log::warn!("[{tag}] [trace={trace_id}] Failed to serialize {label}: {err}");
        }
    }
}

fn data_log_lock() -> &'static Mutex<()> {
    DATA_LOG_FILE_LOCK.get_or_init(|| Mutex::new(()))
}

fn data_session_dir() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("logs")
        .join("cc-switch-data")
}

fn data_session_path(session_id: &str) -> PathBuf {
    let safe_session_id = session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    data_session_dir().join(format!("{safe_session_id}.json"))
}

pub fn set_data_logging_enabled(enabled: bool) {
    DATA_LOGGING_ENABLED.store(enabled, Ordering::Relaxed);
}

fn is_data_logging_enabled() -> bool {
    DATA_LOGGING_ENABLED.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataSessionSummary {
    turn_count: usize,
    internal_request_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataTurnParticipant {
    text: String,
    timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataInternalResponse {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataInternalUsage {
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataInternalRequest {
    trace_id: String,
    kind: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    request_messages: Vec<Value>,
    response: DataInternalResponse,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<DataInternalUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataTurn {
    turn_index: usize,
    user: DataTurnParticipant,
    assistant: DataTurnParticipant,
    internal_requests: Vec<DataInternalRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataSessionRecord {
    session_id: String,
    app: String,
    started_at: String,
    updated_at: String,
    summary: DataSessionSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pending_internal_requests: Vec<DataInternalRequest>,
    turns: Vec<DataTurn>,
}

#[derive(Debug, Clone)]
pub struct DataRequestLogInput<'a> {
    pub session_id: &'a str,
    pub app: &'a str,
    pub trace_id: &'a str,
    pub kind: &'a str,
    pub source: &'a str,
    pub model: Option<&'a str>,
    pub request_messages: &'a [Value],
    pub response_text: &'a str,
    pub latency_ms: Option<u64>,
    pub usage: Option<&'a crate::proxy::usage::parser::TokenUsage>,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub timestamp: DateTime<Utc>,
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn strip_system_reminders(text: &str) -> String {
    let mut remaining = text;
    let mut cleaned = String::new();

    loop {
        let Some(start) = remaining.find("<system-reminder>") else {
            cleaned.push_str(remaining);
            break;
        };
        cleaned.push_str(&remaining[..start]);
        let after_start = &remaining[start + "<system-reminder>".len()..];
        let Some(end) = after_start.find("</system-reminder>") else {
            break;
        };
        remaining = &after_start[end + "</system-reminder>".len()..];
    }

    cleaned.trim().to_string()
}

fn latest_user_text(request_messages: &[Value]) -> String {
    request_messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .map(strip_system_reminders)
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

fn first_system_text(request_messages: &[Value]) -> String {
    request_messages
        .iter()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn infer_effective_kind(input: &DataRequestLogInput<'_>) -> String {
    if input.kind != "main_response" {
        return input.kind.to_string();
    }

    let system_text = first_system_text(input.request_messages);
    let latest_user = latest_user_text(input.request_messages);

    if latest_user.eq_ignore_ascii_case("warmup") {
        return "warmup".to_string();
    }

    if system_text.contains("Analyze if this message indicates a new conversation topic") {
        return "topic_detection".to_string();
    }

    if system_text.contains("Please write a 5-10 word title")
        || system_text.contains("Summarize this coding conversation in under 50 characters")
    {
        return "title_generation".to_string();
    }

    "main_response".to_string()
}

fn refresh_summary(record: &mut DataSessionRecord) {
    record.summary.turn_count = record.turns.len();
    record.summary.internal_request_count = record
        .pending_internal_requests
        .len()
        + record
        .turns
        .iter()
        .map(|turn| turn.internal_requests.len())
        .sum::<usize>();
}

fn should_merge_into_turn(
    turn: &DataTurn,
    user_text: &str,
    response_text: &str,
    now: DateTime<Utc>,
) -> bool {
    if turn.user.text != user_text || turn.assistant.text != response_text {
        return false;
    }

    let Some(last_ts) = turn
        .internal_requests
        .last()
        .and_then(|request| parse_timestamp(&request.timestamp))
    else {
        return false;
    };

    now.signed_duration_since(last_ts) <= Duration::seconds(30)
}

fn should_attach_internal_request_to_turn(
    turn: &DataTurn,
    kind: &str,
    user_text: &str,
    now: DateTime<Utc>,
) -> bool {
    if kind == "warmup" {
        return false;
    }

    if user_text.is_empty() || turn.user.text != user_text {
        return false;
    }

    let Some(turn_ts) = parse_timestamp(&turn.assistant.timestamp) else {
        return false;
    };

    now.signed_duration_since(turn_ts) <= Duration::seconds(30)
}

fn merge_data_request(record: &mut DataSessionRecord, input: &DataRequestLogInput<'_>) {
    let effective_kind = infer_effective_kind(input);
    let response_text = input.response_text.trim();
    let user_text = latest_user_text(input.request_messages);

    if user_text.is_empty() || response_text.is_empty() {
        return;
    }

    let timestamp = input.timestamp.to_rfc3339();
    let internal_request = DataInternalRequest {
        trace_id: input.trace_id.to_string(),
        kind: effective_kind.clone(),
        source: input.source.to_string(),
        model: input.model.filter(|value| !value.trim().is_empty()).map(str::to_string),
        request_messages: input.request_messages.to_vec(),
        response: DataInternalResponse {
            role: "assistant".to_string(),
            content: response_text.to_string(),
        },
        status: input.status.to_string(),
        latency_ms: input.latency_ms,
        usage: input.usage.map(|usage| DataInternalUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
        }),
        error: input
            .error
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string),
        timestamp: timestamp.clone(),
    };

    if effective_kind != "main_response" {
        let should_attach = record.turns.last().is_some_and(|turn| {
            should_attach_internal_request_to_turn(turn, &effective_kind, &user_text, input.timestamp)
        });

        if should_attach {
            let turn = record.turns.last_mut().expect("turn exists after should_attach");
            let duplicated = turn
                .internal_requests
                .iter()
                .any(|request| request.trace_id == input.trace_id);
            if !duplicated {
                turn.internal_requests.push(internal_request);
                record.updated_at = timestamp;
                refresh_summary(record);
            }
        } else {
            let duplicated = record
                .pending_internal_requests
                .iter()
                .any(|request| request.trace_id == input.trace_id);
            if !duplicated {
                record.pending_internal_requests.push(internal_request);
                record.updated_at = timestamp;
                refresh_summary(record);
            }
        }
        return;
    }

    let merge_target = record.turns.last().is_some_and(|turn| {
        should_merge_into_turn(turn, &user_text, response_text, input.timestamp)
    });

    if merge_target {
        if let Some(turn) = record.turns.last_mut() {
            let duplicated = turn
                .internal_requests
                .iter()
                .any(|request| request.trace_id == input.trace_id);
            if !duplicated {
                turn.internal_requests.push(internal_request);
                turn.assistant.timestamp = timestamp.clone();
            }
        }
    } else {
        let mut internal_requests = Vec::new();
        internal_requests.append(&mut record.pending_internal_requests);
        internal_requests.push(internal_request);
        record.turns.push(DataTurn {
            turn_index: record.turns.len() + 1,
            user: DataTurnParticipant {
                text: user_text,
                timestamp: timestamp.clone(),
            },
            assistant: DataTurnParticipant {
                text: response_text.to_string(),
                timestamp: timestamp.clone(),
            },
            internal_requests,
        });
    }

    record.updated_at = timestamp;
    refresh_summary(record);
}

fn load_data_session(path: &PathBuf) -> Option<DataSessionRecord> {
    let mut file = OpenOptions::new().read(true).open(path).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

fn store_data_session(record: &DataSessionRecord) {
    let session_path = data_session_path(&record.session_id);
    let Some(parent) = session_path.parent() else {
        eprintln!(
            "Failed to resolve parent dir for data session {}",
            session_path.display()
        );
        return;
    };

    if let Err(err) = std::fs::create_dir_all(parent) {
        eprintln!("Failed to create data session dir {}: {err}", parent.display());
        return;
    }

    let tmp_path = session_path.with_extension("json.tmp");
    let payload = match serde_json::to_vec_pretty(record) {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!(
                "Failed to serialize data session {}: {err}",
                session_path.display()
            );
            return;
        }
    };

    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
    {
        Ok(mut file) => {
            if let Err(err) = file.write_all(&payload) {
                eprintln!("Failed to write temp data session {}: {err}", tmp_path.display());
                let _ = std::fs::remove_file(&tmp_path);
                return;
            }
        }
        Err(err) => {
            eprintln!("Failed to open temp data session {}: {err}", tmp_path.display());
            return;
        }
    }

    if let Err(err) = std::fs::rename(&tmp_path, &session_path) {
        eprintln!(
            "Failed to replace data session {} with {}: {err}",
            session_path.display(),
            tmp_path.display()
        );
        let _ = std::fs::remove_file(&tmp_path);
    }
}

fn write_data_session(input: &DataRequestLogInput<'_>) {
    let _guard = match data_log_lock().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let session_path = data_session_path(input.session_id);
    let mut record = load_data_session(&session_path).unwrap_or_else(|| DataSessionRecord {
        session_id: input.session_id.to_string(),
        app: input.app.to_string(),
        started_at: input.timestamp.to_rfc3339(),
        updated_at: input.timestamp.to_rfc3339(),
        summary: DataSessionSummary {
            turn_count: 0,
            internal_request_count: 0,
        },
        pending_internal_requests: Vec::new(),
        turns: Vec::new(),
    });

    record.app = input.app.to_string();
    merge_data_request(&mut record, input);
    store_data_session(&record);
}

fn log_text_payload(tag: &str, trace_id: &str, label: &str, body: &str) {
    log::debug!(
        "[{tag}] [trace={trace_id}] {label} ({} bytes): {}",
        body.len(),
        body
    );
}

pub fn log_client_request(
    tag: &str,
    trace_id: &str,
    endpoint: &str,
    headers: &HeaderMap,
    body: &Value,
) {
    log::info!(
        "[{tag}] [trace={trace_id}] Client Request endpoint={endpoint}, headers={}",
        format_headers(headers)
    );
    log_json_payload(tag, trace_id, "Client Request Body", body);
}

pub fn log_upstream_request(
    tag: &str,
    trace_id: &str,
    url: &str,
    request_model: &str,
    body: &Value,
) {
    log::info!(
        "[{tag}] [trace={trace_id}] Upstream Request url={url}, model={request_model}"
    );
    log_json_payload(tag, trace_id, "Upstream Request Body", body);
}

pub fn log_passthrough_response_bytes(
    tag: &str,
    trace_id: &str,
    status: u16,
    headers: &HeaderMap,
    body: &[u8],
) {
    log::info!(
        "[{tag}] [trace={trace_id}] Passthrough Response status={status}, headers={}",
        format_headers(headers)
    );
    log_text_payload(
        tag,
        trace_id,
        "Passthrough Response Body",
        &String::from_utf8_lossy(body),
    );
}

pub fn log_client_response_bytes(
    tag: &str,
    trace_id: &str,
    status: u16,
    headers: &HeaderMap,
    body: &[u8],
) {
    log::info!(
        "[{tag}] [trace={trace_id}] Client Response status={status}, headers={}",
        format_headers(headers)
    );
    log_text_payload(
        tag,
        trace_id,
        "Client Response Body",
        &String::from_utf8_lossy(body),
    );
}

pub fn log_stream_response_start(tag: &str, trace_id: &str, status: u16, headers: &HeaderMap) {
    log::info!(
        "[{tag}] [trace={trace_id}] Client Stream Response status={status}, headers={}",
        format_headers(headers)
    );
}

pub fn log_stream_payload(tag: &str, trace_id: &str, payload: &str) {
    log::debug!("[{tag}] [trace={trace_id}] Client Stream Payload: {payload}");
}

fn content_block_to_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    let block_type = value.get("type").and_then(|v| v.as_str()).unwrap_or_default();

    match block_type {
        "text" | "input_text" | "output_text" => value
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "thinking" => value
            .get("thinking")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "tool_result" => value
            .get("content")
            .map(content_value_to_text)
            .filter(|s| !s.is_empty()),
        _ => {
            if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                Some(text.to_string())
            } else if let Some(input_text) = value.get("input_text").and_then(|v| v.as_str()) {
                Some(input_text.to_string())
            } else if let Some(output_text) = value.get("output_text").and_then(|v| v.as_str()) {
                Some(output_text.to_string())
            } else {
                None
            }
        }
    }
}

fn content_value_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(content_block_to_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(map) => {
            if let Some(parts) = map.get("parts").and_then(|v| v.as_array()) {
                return parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
            }
            content_block_to_text(value).unwrap_or_default()
        }
        _ => String::new(),
    }
}

fn push_message(messages: &mut Vec<Value>, role: &str, content: String) {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return;
    }

    messages.push(json!({
        "role": role,
        "content": trimmed,
    }));
}

pub fn extract_request_messages(body: &Value) -> Vec<Value> {
    let mut messages = Vec::new();

    match body.get("system") {
        Some(Value::String(text)) => push_message(&mut messages, "system", text.clone()),
        Some(Value::Array(items)) => {
            let system_text = items
                .iter()
                .map(content_value_to_text)
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            push_message(&mut messages, "system", system_text);
        }
        _ => {}
    }

    if let Some(items) = body.get("messages").and_then(|v| v.as_array()) {
        for item in items {
            let role = item
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user");
            let content = item
                .get("content")
                .map(content_value_to_text)
                .unwrap_or_default();
            push_message(&mut messages, role, content);
        }
    }

    match body.get("input") {
        Some(Value::String(text)) => push_message(&mut messages, "user", text.clone()),
        Some(Value::Array(items)) => {
            for item in items {
                let role = item
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user");
                let content = item
                    .get("content")
                    .or_else(|| item.get("input"))
                    .map(content_value_to_text)
                    .unwrap_or_default();
                push_message(&mut messages, role, content);
            }
        }
        _ => {}
    }

    if let Some(contents) = body.get("contents").and_then(|v| v.as_array()) {
        for item in contents {
            let role = item
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user");
            let content = item
                .get("parts")
                .map(content_value_to_text)
                .unwrap_or_default();
            push_message(&mut messages, role, content);
        }
    }

    messages
}

fn extract_non_stream_response_text(body: &Value) -> String {
    if let Some(content) = body.get("content") {
        let text = content_value_to_text(content);
        if !text.trim().is_empty() {
            return text;
        }
    }

    if let Some(output) = body.get("output") {
        let text = content_value_to_text(output);
        if !text.trim().is_empty() {
            return text;
        }
    }

    if let Some(choices) = body.get("choices").and_then(|v| v.as_array()) {
        let text = choices
            .iter()
            .filter_map(|choice| choice.get("message").or_else(|| choice.get("delta")))
            .map(content_value_to_text)
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return text;
        }
    }

    if let Some(candidates) = body.get("candidates").and_then(|v| v.as_array()) {
        let text = candidates
            .iter()
            .filter_map(|candidate| candidate.get("content"))
            .map(content_value_to_text)
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return text;
        }
    }

    String::new()
}

pub fn collect_stream_response_text(event: &Value, response_text: &mut String) {
    if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
        match event_type {
            "content_block_delta" => {
                if let Some(text) = event
                    .get("delta")
                    .and_then(|delta| delta.get("text"))
                    .and_then(|v| v.as_str())
                {
                    response_text.push_str(text);
                }
                if let Some(text) = event
                    .get("delta")
                    .and_then(|delta| delta.get("thinking"))
                    .and_then(|v| v.as_str())
                {
                    response_text.push_str(text);
                }
            }
            "response.output_text.delta" => {
                if let Some(text) = event.get("delta").and_then(|v| v.as_str()) {
                    response_text.push_str(text);
                }
            }
            _ => {}
        }
    }

    if let Some(choices) = event.get("choices").and_then(|v| v.as_array()) {
        for choice in choices {
            if let Some(content) = choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(|v| v.as_str())
            {
                response_text.push_str(content);
            }
        }
    }

    if let Some(candidates) = event.get("candidates").and_then(|v| v.as_array()) {
        for candidate in candidates {
            if let Some(content) = candidate.get("content") {
                let text = content_value_to_text(content);
                if !text.is_empty() {
                    response_text.push_str(&text);
                }
            }
        }
    }
}

pub fn log_training_sample_with_response(
    tag: &str,
    session_id: &str,
    trace_id: &str,
    kind: &str,
    model: Option<&str>,
    request_messages: &[Value],
    response_body: &Value,
    latency_ms: Option<u64>,
    status: &str,
) {
    let response_text = extract_non_stream_response_text(response_body);
    let usage = crate::proxy::usage::parser::TokenUsage::from_claude_response(response_body)
        .or_else(|| crate::proxy::usage::parser::TokenUsage::from_codex_response_auto(response_body))
        .or_else(|| crate::proxy::usage::parser::TokenUsage::from_openai_response(response_body))
        .or_else(|| crate::proxy::usage::parser::TokenUsage::from_gemini_response(response_body));
    let resolved_model = usage
        .as_ref()
        .and_then(|item| item.model.as_deref())
        .or(model);
    log_training_sample_with_text(
        tag,
        session_id,
        trace_id,
        kind,
        resolved_model,
        request_messages,
        &response_text,
        latency_ms,
        usage.as_ref(),
        status,
        None,
    );
}

pub fn log_training_sample_with_text(
    tag: &str,
    session_id: &str,
    trace_id: &str,
    kind: &str,
    model: Option<&str>,
    request_messages: &[Value],
    response_text: &str,
    latency_ms: Option<u64>,
    usage: Option<&crate::proxy::usage::parser::TokenUsage>,
    status: &str,
    error: Option<&str>,
) {
    let trimmed_response = response_text.trim();
    if request_messages.is_empty() || trimmed_response.is_empty() {
        return;
    }

    let mut messages = request_messages.to_vec();
    messages.push(json!({
        "role": "assistant",
        "content": trimmed_response,
    }));

    let record = json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "trace_id": trace_id,
        "app": tag,
        "messages": messages,
    });

    if is_data_logging_enabled() {
        let input = DataRequestLogInput {
            session_id,
            app: tag,
            trace_id,
            kind,
            source: "proxy_data",
            model,
            request_messages,
            response_text: trimmed_response,
            latency_ms,
            usage,
            status,
            error,
            timestamp: Utc::now(),
        };
        write_data_session(&input);
        return;
    }

    log::info!("[{tag}] [trace={trace_id}] Training Sample: {record}");
}

pub fn log_proxy_error_response(tag: &str, trace_id: &str, error: &ProxyError) {
    let (status, body) = error.response_snapshot();
    log::warn!(
        "[{tag}] [trace={trace_id}] Client Error Response status={}",
        status.as_u16()
    );
    log_json_payload(tag, trace_id, "Client Error Response Body", &body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_data_request_appends_internal_requests_to_same_turn() {
        let now = Utc::now();
        let first_request_messages = vec![json!({
            "role": "user",
            "content": "hello",
        })];
        let second_request_messages = vec![json!({
            "role": "user",
            "content": "hello",
        })];
        let first_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-1",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &first_request_messages,
            response_text: "world",
            latency_ms: Some(10),
            usage: None,
            status: "success",
            error: None,
            timestamp: now,
        };
        let second_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-2",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &second_request_messages,
            response_text: "world",
            latency_ms: Some(11),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(5),
        };

        let mut record = DataSessionRecord {
            session_id: "sess-1".to_string(),
            app: "Claude".to_string(),
            started_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            summary: DataSessionSummary {
                turn_count: 0,
                internal_request_count: 0,
            },
            pending_internal_requests: Vec::new(),
            turns: Vec::new(),
        };

        merge_data_request(&mut record, &first_input);
        merge_data_request(&mut record, &second_input);

        assert_eq!(record.turns.len(), 1);
        assert_eq!(record.turns[0].internal_requests.len(), 2);
        assert_eq!(record.summary.turn_count, 1);
        assert_eq!(record.summary.internal_request_count, 2);
    }

    #[test]
    fn merge_data_request_creates_new_turn_for_new_prompt() {
        let now = Utc::now();
        let first_request_messages = vec![json!({
            "role": "user",
            "content": "hello",
        })];
        let second_request_messages = vec![json!({
            "role": "user",
            "content": "next prompt",
        })];
        let first_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-1",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &first_request_messages,
            response_text: "world",
            latency_ms: Some(10),
            usage: None,
            status: "success",
            error: None,
            timestamp: now,
        };
        let second_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-2",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &second_request_messages,
            response_text: "second answer",
            latency_ms: Some(12),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(5),
        };

        let mut record = DataSessionRecord {
            session_id: "sess-1".to_string(),
            app: "Claude".to_string(),
            started_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            summary: DataSessionSummary {
                turn_count: 0,
                internal_request_count: 0,
            },
            pending_internal_requests: Vec::new(),
            turns: Vec::new(),
        };

        merge_data_request(&mut record, &first_input);
        merge_data_request(&mut record, &second_input);

        assert_eq!(record.turns.len(), 2);
        assert_eq!(record.turns[0].turn_index, 1);
        assert_eq!(record.turns[1].turn_index, 2);
        assert_eq!(record.summary.turn_count, 2);
    }

    #[test]
    fn merge_data_request_does_not_create_turn_for_warmup() {
        let now = Utc::now();
        let warmup_messages = vec![
            json!({
                "role": "system",
                "content": "You are Claude Code",
            }),
            json!({
                "role": "user",
                "content": "Warmup",
            }),
        ];
        let main_messages = vec![json!({
            "role": "user",
            "content": "real question",
        })];
        let warmup_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-warmup",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &warmup_messages,
            response_text: "ready",
            latency_ms: Some(5),
            usage: None,
            status: "success",
            error: None,
            timestamp: now,
        };
        let main_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-main",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &main_messages,
            response_text: "answer",
            latency_ms: Some(8),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(1),
        };

        let mut record = DataSessionRecord {
            session_id: "sess-1".to_string(),
            app: "Claude".to_string(),
            started_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            summary: DataSessionSummary {
                turn_count: 0,
                internal_request_count: 0,
            },
            pending_internal_requests: Vec::new(),
            turns: Vec::new(),
        };

        merge_data_request(&mut record, &warmup_input);
        assert_eq!(record.turns.len(), 0);

        merge_data_request(&mut record, &main_input);
        assert_eq!(record.turns.len(), 1);
        assert_eq!(record.turns[0].user.text, "real question");
    }

    #[test]
    fn merge_data_request_attaches_topic_detection_to_existing_turn() {
        let now = Utc::now();
        let main_messages = vec![json!({
            "role": "user",
            "content": "hello",
        })];
        let topic_messages = vec![
            json!({
                "role": "system",
                "content": "Analyze if this message indicates a new conversation topic",
            }),
            json!({
                "role": "user",
                "content": "hello",
            }),
        ];
        let main_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-main",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &main_messages,
            response_text: "hi there",
            latency_ms: Some(10),
            usage: None,
            status: "success",
            error: None,
            timestamp: now,
        };
        let topic_input = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-topic",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &topic_messages,
            response_text: "{\"isNewTopic\":true}",
            latency_ms: Some(11),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(1),
        };

        let mut record = DataSessionRecord {
            session_id: "sess-1".to_string(),
            app: "Claude".to_string(),
            started_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            summary: DataSessionSummary {
                turn_count: 0,
                internal_request_count: 0,
            },
            pending_internal_requests: Vec::new(),
            turns: Vec::new(),
        };

        merge_data_request(&mut record, &main_input);
        merge_data_request(&mut record, &topic_input);

        assert_eq!(record.turns.len(), 1);
        assert_eq!(record.turns[0].internal_requests.len(), 2);
        assert_eq!(record.turns[0].internal_requests[1].kind, "topic_detection");
        assert_eq!(record.summary.turn_count, 1);
    }

    #[test]
    fn merge_data_request_keeps_next_turn_internal_requests_pending() {
        let now = Utc::now();
        let first_main_messages = vec![json!({
            "role": "user",
            "content": "first",
        })];
        let warmup_messages = vec![
            json!({
                "role": "system",
                "content": "You are Claude Code",
            }),
            json!({
                "role": "user",
                "content": "Warmup",
            }),
        ];
        let second_topic_messages = vec![
            json!({
                "role": "system",
                "content": "Analyze if this message indicates a new conversation topic",
            }),
            json!({
                "role": "user",
                "content": "second",
            }),
        ];
        let second_main_messages = vec![json!({
            "role": "user",
            "content": "second",
        })];

        let first_main = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-main-1",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &first_main_messages,
            response_text: "answer 1",
            latency_ms: Some(10),
            usage: None,
            status: "success",
            error: None,
            timestamp: now,
        };
        let warmup = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-warmup-2",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &warmup_messages,
            response_text: "ready",
            latency_ms: Some(11),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(5),
        };
        let second_topic = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-topic-2",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &second_topic_messages,
            response_text: "{\"isNewTopic\":true}",
            latency_ms: Some(12),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(6),
        };
        let second_main = DataRequestLogInput {
            session_id: "sess-1",
            app: "Claude",
            trace_id: "trace-main-2",
            kind: "main_response",
            source: "proxy_data",
            model: Some("claude-test"),
            request_messages: &second_main_messages,
            response_text: "answer 2",
            latency_ms: Some(13),
            usage: None,
            status: "success",
            error: None,
            timestamp: now + Duration::seconds(7),
        };

        let mut record = DataSessionRecord {
            session_id: "sess-1".to_string(),
            app: "Claude".to_string(),
            started_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            summary: DataSessionSummary {
                turn_count: 0,
                internal_request_count: 0,
            },
            pending_internal_requests: Vec::new(),
            turns: Vec::new(),
        };

        merge_data_request(&mut record, &first_main);
        merge_data_request(&mut record, &warmup);
        merge_data_request(&mut record, &second_topic);

        assert_eq!(record.turns.len(), 1);
        assert_eq!(record.turns[0].internal_requests.len(), 1);
        assert_eq!(record.pending_internal_requests.len(), 2);

        merge_data_request(&mut record, &second_main);

        assert_eq!(record.turns.len(), 2);
        assert_eq!(record.turns[0].internal_requests.len(), 1);
        assert_eq!(record.turns[1].internal_requests.len(), 3);
        assert!(record.pending_internal_requests.is_empty());
        assert_eq!(record.turns[1].internal_requests[0].kind, "warmup");
        assert_eq!(record.turns[1].internal_requests[1].kind, "topic_detection");
        assert_eq!(record.turns[1].internal_requests[2].kind, "main_response");
    }
}
