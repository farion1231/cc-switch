//! 请求/响应日志模块
//!
//! 默认写入 `~/.cc-switch/req.log`，无需开启 debug 级别。
//! 每条事件 2 行：时间 + 方向 + 摘要，header/body 详情。

use std::io::Write;

/// 追加一行到 req.log，忽略写入失败
fn append_line(line: &str) {
    let path = crate::config::get_app_config_dir().join("req.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// 美化 JSON 输出为带缩进的多行字符串，仅截断长文本值
fn indented_field(label: &str, raw: &str) -> String {
    let content = if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        let trimmed = trim_long_strings(v);
        serde_json::to_string_pretty(&trimmed).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    };
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() == 1 {
        format!("  {label}: {}", lines[0])
    } else {
        let mut s = format!("  {label}:\n");
        for line in &lines {
            s.push_str(&format!("    {line}\n"));
        }
        s
    }
}

/// 递归裁剪 JSON 中的长字符串 value，保持结构和 key 完整
fn trim_long_strings(v: serde_json::Value) -> serde_json::Value {
    const MAX_VAL_LEN: usize = 500;
    match v {
        serde_json::Value::String(s) if s.len() > MAX_VAL_LEN => {
            let head: String = s.chars().take(MAX_VAL_LEN).collect();
            serde_json::Value::String(format!("{head}...(truncated)"))
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(trim_long_strings).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, val)| (k, trim_long_strings(val)))
                .collect(),
        ),
        other => other,
    }
}

/// 脱敏 headers：authorization / api-key 类替换为 `***`
fn sanitize_headers(headers: &str) -> String {
    let mut result = String::with_capacity(headers.len());
    let mut remaining = headers;
    while let Some(line_end) = remaining.find('\n') {
        let line = &remaining[..line_end];
        remaining = &remaining[line_end + 1..];
        let lower = line.to_lowercase();
        if lower.contains("authorization")
            || lower.contains("api-key")
            || lower.contains("x-api-key")
        {
            if let Some(colon) = line.find(':') {
                result.push_str(&line[..=colon]);
                result.push_str(" ***");
            } else {
                result.push_str(line);
            }
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    if result.ends_with('\n') {
        result.pop();
    }
    result
}

/// ① Codex CLI → cc-switch 入口请求
pub fn log_incoming(tag: &str, method: &str, uri: &str, headers: &str, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let headers = sanitize_headers(headers);
    append_line(&format!("{ts} [{tag}] ← INCOMING {method} {uri}"));
    append_line(&indented_field("headers", &headers));
    append_line(&indented_field("body", body));
}

/// ④ cc-switch → 上游提供商请求
pub fn log_upstream_req(tag: &str, method: &str, url: &str, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    append_line(&format!("{ts} [{tag}] → UPSTREAM {method} {url}"));
    append_line(&indented_field("body", body));
}

/// ⑤ 上游 → cc-switch 响应
pub fn log_upstream_resp(tag: &str, status: u16, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    append_line(&format!("{ts} [{tag}] ← UPSTREAM RESPONSE {status}"));
    append_line(&indented_field("body", body));
}

/// ⑥ cc-switch → Codex CLI 最终返回
pub fn log_return(tag: &str, status: u16, extra: &str, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if extra.is_empty() {
        append_line(&format!("{ts} [{tag}] → RETURN {status}"));
    } else {
        append_line(&format!("{ts} [{tag}] → RETURN {status} ({extra})"));
    }
    append_line(&indented_field("body", body));
}
