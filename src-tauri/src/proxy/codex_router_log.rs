//! Codex router diagnostic log.
//!
//! 普通 usage log 记录计费用的请求摘要；router 排障还需要一条紧凑时间线，
//! 用来定位模型解析、认证、上游发送和首包等待分别花了多久。

use std::fs::OpenOptions;
use std::io::Write;

/// 向 `codex-router.log` 追加一条清洗后的 Codex router 诊断事件。
///
/// 只记录阶段、模型、provider、耗时等摘要，不写 prompt、header 原文或 SSE
/// 内容。该日志只用于排障，所以写入失败会被忽略，不能影响真实代理请求。
pub(crate) fn append_event(event: &str, fields: &[(&str, String)]) {
    let log_dir = crate::config::get_app_config_dir().join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }

    let path = log_dir.join("codex-router.log");
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let mut line = format!(
        "{} event={}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        sanitize_value(event)
    );
    for (key, value) in fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(&sanitize_value(value));
    }
    line.push('\n');

    let _ = file.write_all(line.as_bytes());
}

/// 将日志字段清洗成单行安全文本。
///
/// 明显的 API key/token 会被遮盖，空白字符会折叠为下划线，避免异常值污染
/// 下一行日志。
fn sanitize_value(value: &str) -> String {
    let value = value.replace(['\r', '\n', '\t'], " ");
    let mut parts = Vec::new();
    for part in value.split_whitespace() {
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("sk-")
            || lower.contains("bearer ")
            || lower.contains("api_key")
            || lower.contains("apikey")
            || lower.contains("token")
        {
            parts.push("<redacted>".to_string());
        } else {
            parts.push(part.to_string());
        }
    }
    parts.join("_")
}
