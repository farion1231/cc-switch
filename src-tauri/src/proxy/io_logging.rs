use super::ProxyError;
use axum::http::HeaderMap;
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::Write,
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

fn data_log_path() -> std::path::PathBuf {
    crate::config::get_app_config_dir()
        .join("logs")
        .join("cc-switch-data.jsonl")
}

pub fn set_data_logging_enabled(enabled: bool) {
    DATA_LOGGING_ENABLED.store(enabled, Ordering::Relaxed);
}

fn is_data_logging_enabled() -> bool {
    DATA_LOGGING_ENABLED.load(Ordering::Relaxed)
}

fn append_data_record(record: &Value) {
    let log_path = data_log_path();
    if let Some(parent) = log_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            eprintln!("Failed to create data log dir {}: {err}", parent.display());
            return;
        }
    }

    let _guard = match data_log_lock().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let mut file = match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("Failed to open data log {}: {err}", log_path.display());
            return;
        }
    };

    if let Err(err) = serde_json::to_writer(&mut file, record) {
        eprintln!("Failed to write data record {}: {err}", log_path.display());
        return;
    }

    if let Err(err) = file.write_all(b"\n") {
        eprintln!("Failed to terminate data record {}: {err}", log_path.display());
    }
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
    trace_id: &str,
    request_messages: &[Value],
    response_body: &Value,
) {
    let response_text = extract_non_stream_response_text(response_body);
    log_training_sample_with_text(tag, trace_id, request_messages, &response_text);
}

pub fn log_training_sample_with_text(
    tag: &str,
    trace_id: &str,
    request_messages: &[Value],
    response_text: &str,
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
        append_data_record(&record);
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
