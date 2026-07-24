//! GitHub Copilot 代理诊断日志。
//!
//! 仅在应用以 `--copilot-diagnostic` 启动时启用。诊断事件使用独立 JSONL 文件，
//! 不受应用日志级别影响，也不会记录请求正文、认证信息或工具参数。

use chrono::Local;
use once_cell::sync::OnceCell;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const SCHEMA_VERSION: u8 = 1;
const FILE_NAME: &str = "copilot-diagnostic.jsonl";
const MAX_FILE_SIZE: u64 = 20 * 1024 * 1024;
const ARCHIVES_TO_KEEP: usize = 4;
const QUEUE_CAPACITY: usize = 4096;
const FAILURE_SNIPPET_LIMIT: usize = 8 * 1024;
const DROP_WARNING_INTERVAL_SECS: u64 = 30;
const REDACTED: &str = "[REDACTED]";

static LOGGER: OnceCell<Arc<CopilotDiagnosticLogger>> = OnceCell::new();

#[derive(Debug)]
struct CopilotDiagnosticLogger {
    sender: mpsc::SyncSender<Value>,
    last_drop_warning: AtomicU64,
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StreamDiagnosticContext {
    request_id: String,
    protocol: &'static str,
}

impl StreamDiagnosticContext {
    pub fn new(request_id: impl Into<String>, protocol: &'static str) -> Self {
        Self {
            request_id: request_id.into(),
            protocol,
        }
    }

    pub fn emit(&self, event: &str, fields: Value) {
        let mut fields = match fields {
            Value::Object(map) => map,
            other => {
                let mut map = Map::new();
                map.insert("value".to_string(), other);
                map
            }
        };
        fields.insert("request_id".to_string(), json!(self.request_id));
        fields.insert("protocol".to_string(), json!(self.protocol));
        emit(event, Value::Object(fields));
    }
}

pub fn is_copilot_provider(provider: &crate::provider::Provider) -> bool {
    if provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("github_copilot")
    {
        return true;
    }
    [
        provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(Value::as_str),
        provider
            .settings_config
            .get("base_url")
            .and_then(Value::as_str),
        provider
            .settings_config
            .get("baseURL")
            .and_then(Value::as_str),
        provider
            .settings_config
            .get("apiEndpoint")
            .and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .any(|url| url.contains("githubcopilot.com"))
}

pub fn requested_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == "--copilot-diagnostic")
}

pub fn initialize(log_dir: PathBuf) -> std::io::Result<Option<PathBuf>> {
    if !requested_from_args(std::env::args()) {
        return Ok(None);
    }
    if let Some(logger) = LOGGER.get() {
        return Ok(Some(logger.path.clone()));
    }

    fs::create_dir_all(&log_dir)?;
    let path = log_dir.join(FILE_NAME);
    let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
    let logger = Arc::new(CopilotDiagnosticLogger {
        sender,
        last_drop_warning: AtomicU64::new(0),
        path: path.clone(),
    });

    let writer_path = path.clone();
    std::thread::Builder::new()
        .name("copilot-diagnostic-writer".to_string())
        .spawn(move || writer_loop(receiver, writer_path))?;

    let _ = LOGGER.set(logger);
    emit(
        "diagnostic_started",
        json!({
            "path": path.to_string_lossy(),
            "max_file_size": MAX_FILE_SIZE,
            "archives": ARCHIVES_TO_KEEP + 1,
        }),
    );
    Ok(Some(path))
}

pub fn enabled() -> bool {
    LOGGER.get().is_some()
}

pub fn new_request_id() -> Option<String> {
    enabled().then(|| Uuid::new_v4().to_string())
}

pub fn emit(event: &str, fields: Value) {
    let Some(logger) = LOGGER.get() else {
        return;
    };

    let mut record = match fields {
        Value::Object(map) => map,
        other => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
    };
    record.insert("schema_version".to_string(), json!(SCHEMA_VERSION));
    record.insert("timestamp".to_string(), json!(timestamp()));
    record.insert("event".to_string(), json!(event));

    if let Err(error) = logger.sender.try_send(Value::Object(record)) {
        let now = unix_seconds();
        let previous = logger.last_drop_warning.load(Ordering::Relaxed);
        if now.saturating_sub(previous) >= DROP_WARNING_INTERVAL_SECS
            && logger
                .last_drop_warning
                .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            log::warn!("[CopilotDiagnostic] 诊断队列已满或关闭，事件被丢弃: {error}");
        }
    }
}

pub fn hash_identifier(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let digest = Sha256::digest(value.as_bytes());
    hex_prefix(&digest, 16)
}

pub fn sanitize_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => raw.split(['?', '#']).next().unwrap_or(raw).to_string(),
    }
}

pub fn summarize_body(body: &Value) -> Value {
    let mut block_types: BTreeMap<String, u64> = BTreeMap::new();
    let mut roles: BTreeMap<String, u64> = BTreeMap::new();
    let mut message_count = 0_u64;

    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        message_count = messages.len() as u64;
        for message in messages {
            if let Some(role) = message.get("role").and_then(Value::as_str) {
                *roles.entry(role.to_string()).or_default() += 1;
            }
            summarize_content_types(message.get("content"), &mut block_types);
        }
    }
    if let Some(input) = body.get("input").and_then(Value::as_array) {
        message_count = message_count.max(input.len() as u64);
        for item in input {
            if let Some(role) = item.get("role").and_then(Value::as_str) {
                *roles.entry(role.to_string()).or_default() += 1;
            }
            summarize_content_types(item.get("content"), &mut block_types);
            if let Some(item_type) = item.get("type").and_then(Value::as_str) {
                *block_types.entry(item_type.to_string()).or_default() += 1;
            }
        }
    }

    let tools = body
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    let name = tool
                        .get("name")
                        .or_else(|| tool.pointer("/function/name"))
                        .and_then(Value::as_str)
                        .unwrap_or("<unknown>");
                    json!({
                        "name": name,
                        "has_schema": tool.get("input_schema").is_some()
                            || tool.get("parameters").is_some()
                            || tool.pointer("/function/parameters").is_some(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let top_level_keys = body
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let choice_summaries = body
        .get("choices")
        .and_then(Value::as_array)
        .map(|choices| {
            choices
                .iter()
                .map(|choice| {
                    let payload = choice.get("delta").or_else(|| choice.get("message"));
                    json!({
                        "finish_reason": choice.get("finish_reason").and_then(Value::as_str),
                        "payload_keys": payload
                            .and_then(Value::as_object)
                            .map(|map| map.keys().cloned().collect::<Vec<_>>())
                            .unwrap_or_default(),
                        "tool_call_count": payload
                            .and_then(|value| value.get("tool_calls"))
                            .and_then(Value::as_array)
                            .map(Vec::len)
                            .unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut output_item_types: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(output) = body.get("output").and_then(Value::as_array) {
        for item in output {
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_else(|| value_type(item));
            *output_item_types.entry(item_type.to_string()).or_default() += 1;
        }
    }

    json!({
        "json_type": value_type(body),
        "serialized_bytes": serde_json::to_vec(body).map(|v| v.len()).unwrap_or_default(),
        "top_level_keys": top_level_keys,
        "model": body.get("model").and_then(Value::as_str),
        "stream": body.get("stream").and_then(Value::as_bool),
        "message_count": message_count,
        "roles": roles,
        "content_block_types": block_types,
        "tool_count": tools.len(),
        "tools": tools,
        "has_system": body.get("system").is_some() || body.get("instructions").is_some(),
        "has_thinking": body.get("thinking").is_some() || body.get("reasoning").is_some(),
        "tool_choice_type": body.get("tool_choice").map(value_type),
        "response_status": body.get("status").and_then(Value::as_str),
        "has_error": body.get("error").is_some(),
        "choice_count": choice_summaries.len(),
        "choices": choice_summaries,
        "output_item_types": output_item_types,
        "usage_keys": body
            .get("usage")
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

pub fn failure_snippet(raw: &str) -> Value {
    let sanitized = match serde_json::from_str::<Value>(raw) {
        Ok(mut value) => {
            redact_value(&mut value);
            serde_json::to_string(&value).unwrap_or_else(|_| "[UNSERIALIZABLE]".to_string())
        }
        Err(_) => redact_text(raw),
    };
    truncate_utf8(&sanitized, FAILURE_SNIPPET_LIMIT)
}

fn writer_loop(receiver: mpsc::Receiver<Value>, path: PathBuf) {
    let mut writer: Option<BufWriter<File>> = None;
    let mut current_size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    while let Ok(record) = receiver.recv() {
        let mut line = match serde_json::to_vec(&record) {
            Ok(line) => line,
            Err(error) => {
                log::warn!("[CopilotDiagnostic] 序列化诊断事件失败: {error}");
                continue;
            }
        };
        line.push(b'\n');

        if current_size.saturating_add(line.len() as u64) > MAX_FILE_SIZE {
            writer.take();
            if let Err(error) = rotate_files(&path, ARCHIVES_TO_KEEP) {
                log::warn!("[CopilotDiagnostic] 轮转诊断日志失败: {error}");
            }
            current_size = 0;
        }

        if writer.is_none() {
            match open_log_file(&path) {
                Ok(file) => writer = Some(BufWriter::new(file)),
                Err(error) => {
                    log::warn!("[CopilotDiagnostic] 打开诊断日志失败: {error}");
                    continue;
                }
            }
        }

        if let Some(output) = writer.as_mut() {
            if let Err(error) = output.write_all(&line).and_then(|_| output.flush()) {
                log::warn!("[CopilotDiagnostic] 写入诊断日志失败: {error}");
                writer = None;
                continue;
            }
            current_size = current_size.saturating_add(line.len() as u64);
        }
    }
}

fn open_log_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let file = options.open(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        return Ok(file);
    }
    #[cfg(not(unix))]
    {
        options.open(path)
    }
}

fn rotate_files(path: &Path, archives_to_keep: usize) -> std::io::Result<()> {
    if archives_to_keep == 0 {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }

    for index in (1..=archives_to_keep).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            rotated_path(path, index - 1)
        };
        if !source.exists() {
            continue;
        }
        let destination = rotated_path(path, index);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(source, destination)?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(format!(".{index}"));
    PathBuf::from(value)
}

fn summarize_content_types(content: Option<&Value>, counts: &mut BTreeMap<String, u64>) {
    match content {
        Some(Value::String(_)) => *counts.entry("text".to_string()).or_default() += 1,
        Some(Value::Array(blocks)) => {
            for block in blocks {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| value_type(block));
                *counts.entry(block_type.to_string()).or_default() += 1;
            }
        }
        Some(other) => *counts.entry(value_type(other).to_string()).or_default() += 1,
        None => {}
    }
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *child = Value::String(REDACTED.to_string());
                } else {
                    redact_value(child);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact_value),
        Value::String(text) => *text = redact_text(text),
        _ => {}
    }
}

fn redact_text(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            let sensitive_header = line
                .split_once(':')
                .map(|(key, _)| is_sensitive_key(key.trim()))
                .unwrap_or(false);
            if sensitive_header
                || lower.contains("bearer ")
                || lower.contains("basic ")
                || lower.contains("api_key=")
                || lower.contains("api-key=")
            {
                REDACTED.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    normalized == "authorization"
        || normalized.contains("token")
        || normalized.contains("api_key")
        || normalized.contains("secret")
        || normalized.contains("cookie")
        || normalized == "password"
}

fn truncate_utf8(value: &str, max_bytes: usize) -> Value {
    if value.len() <= max_bytes {
        return json!({"text": value, "truncated": false, "original_bytes": value.len()});
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    json!({
        "text": &value[..end],
        "truncated": true,
        "original_bytes": value.len(),
    })
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    let mut result = String::with_capacity(chars);
    for byte in bytes {
        result.push_str(&format!("{byte:02x}"));
        if result.len() >= chars {
            result.truncate(chars);
            break;
        }
    }
    result
}

fn timestamp() -> String {
    Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_startup_flag_is_required() {
        assert!(requested_from_args(["cc-switch", "--copilot-diagnostic"]));
        assert!(!requested_from_args(["cc-switch", "--copilot-diagnostics"]));
        assert!(!requested_from_args(["--copilot-diagnostic=true"]));
    }

    #[test]
    fn body_summary_keeps_shape_not_content() {
        let body = json!({
            "model": "claude-sonnet-4",
            "stream": true,
            "system": "private system prompt",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "private source code"},
                    {"type": "tool_result", "tool_use_id": "secret", "content": "private result"}
                ]
            }],
            "tools": [{"name": "Analyzer", "description": "private", "input_schema": {"type": "object"}}]
        });
        let summary = summarize_body(&body);
        let encoded = serde_json::to_string(&summary).unwrap();
        assert_eq!(summary["message_count"], 1);
        assert_eq!(summary["content_block_types"]["tool_result"], 1);
        assert_eq!(summary["tools"][0]["name"], "Analyzer");
        assert!(!encoded.contains("private source code"));
        assert!(!encoded.contains("private system prompt"));
        assert!(!encoded.contains("private result"));
    }

    #[test]
    fn failure_snippet_redacts_secrets_and_truncates() {
        let raw = json!({
            "authorization": "Bearer abc",
            "nested": {"access_token": "token-value"},
            "message": "safe"
        })
        .to_string();
        let snippet = failure_snippet(&raw);
        let text = snippet["text"].as_str().unwrap();
        assert!(text.contains(REDACTED));
        assert!(!text.contains("Bearer abc"));
        assert!(!text.contains("token-value"));

        let text_error =
            failure_snippet("max_tokens must be positive\nAuthorization: Bearer very-secret");
        let text = text_error["text"].as_str().unwrap();
        assert!(text.contains("max_tokens must be positive"));
        assert!(!text.contains("very-secret"));

        let large = "x".repeat(FAILURE_SNIPPET_LIMIT + 100);
        let truncated = failure_snippet(&large);
        assert_eq!(truncated["truncated"], true);
        assert_eq!(
            truncated["text"].as_str().unwrap().len(),
            FAILURE_SNIPPET_LIMIT
        );
    }

    #[test]
    fn url_query_and_fragment_are_removed() {
        assert_eq!(
            sanitize_url("https://example.com/v1/messages?key=secret#fragment"),
            "https://example.com/v1/messages"
        );
    }

    #[test]
    fn writer_produces_one_valid_json_object_per_line() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE_NAME);
        let (sender, receiver) = mpsc::sync_channel(2);
        let writer_path = path.clone();
        let handle = std::thread::spawn(move || writer_loop(receiver, writer_path));
        sender.send(json!({"event": "test", "value": 1})).unwrap();
        sender.send(json!({"event": "test", "value": 2})).unwrap();
        drop(sender);
        handle.join().unwrap();

        let text = fs::read_to_string(path).unwrap();
        let records = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["value"], 1);
        assert_eq!(records[1]["value"], 2);
    }

    #[test]
    fn rotation_keeps_requested_archive_count() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE_NAME);
        fs::write(&path, "current").unwrap();
        fs::write(rotated_path(&path, 1), "one").unwrap();
        fs::write(rotated_path(&path, 2), "two").unwrap();
        rotate_files(&path, 2).unwrap();
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(rotated_path(&path, 1)).unwrap(),
            "current"
        );
        assert_eq!(fs::read_to_string(rotated_path(&path, 2)).unwrap(), "one");
    }
}
