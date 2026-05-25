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

/// 截断过长的 body（8KB）
fn truncate_body(body: &str) -> &str {
    const MAX_LEN: usize = 8192;
    if body.len() <= MAX_LEN {
        body
    } else {
        &body[..MAX_LEN]
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
    let body = truncate_body(body);
    let headers = sanitize_headers(headers);
    append_line(&format!("{ts} [{tag}] ← INCOMING {method} {uri}"));
    append_line(&format!("  headers: {headers} | body: {body}"));
}

/// ④ cc-switch → 上游提供商请求
pub fn log_upstream_req(tag: &str, method: &str, url: &str, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let body = truncate_body(body);
    append_line(&format!("{ts} [{tag}] → UPSTREAM {method} {url}"));
    append_line(&format!("  body: {body}"));
}

/// ⑤ 上游 → cc-switch 响应
pub fn log_upstream_resp(tag: &str, status: u16, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let body = truncate_body(body);
    append_line(&format!("{ts} [{tag}] ← UPSTREAM RESPONSE {status}"));
    append_line(&format!("  body: {body}"));
}

/// ⑥ cc-switch → Codex CLI 最终返回
pub fn log_return(tag: &str, status: u16, extra: &str, body: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let body = truncate_body(body);
    if extra.is_empty() {
        append_line(&format!("{ts} [{tag}] → RETURN {status}"));
    } else {
        append_line(&format!("{ts} [{tag}] → RETURN {status} ({extra})"));
    }
    append_line(&format!("  body: {body}"));
}
