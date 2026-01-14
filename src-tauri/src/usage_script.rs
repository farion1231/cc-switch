use rquickjs::{Context, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use url::{Host, Url};

use crate::error::AppError;

/// 执行用量查询脚本
pub async fn execute_usage_script(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout_secs: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<Value, AppError> {
    // 检测是否为自定义模板模式
    // 优先使用前端传递的 template_type
    let is_custom_template = template_type.map(|t| t == "custom").unwrap_or(false);

    // 1. 替换模板变量，避免泄露敏感信息
    let script_with_vars =
        build_script_with_vars(script_code, api_key, base_url, access_token, user_id);

    // 2. 验证 base_url 的安全性（仅当提供了 base_url 时）
    // 自定义模板模式下，用户可能不使用模板变量，而是直接在脚本中写完整 URL
    if !base_url.is_empty() {
        validate_base_url(base_url)?;
    }

    // 3. 在独立作用域中提取 request 配置（确保 Runtime/Context 在 await 前释放）
    let request_config = {
        let runtime = Runtime::new().map_err(|e| {
            AppError::localized(
                "usage_script.runtime_create_failed",
                format!("创建 JS 运行时失败: {e}"),
                format!("Failed to create JS runtime: {e}"),
            )
        })?;
        let context = Context::full(&runtime).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!("创建 JS 上下文失败: {e}"),
                format!("Failed to create JS context: {e}"),
            )
        })?;

        context.with(|ctx| {
            // 执行用户代码，获取配置对象
            let config: rquickjs::Object = ctx.eval(script_with_vars.clone()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_parse_failed",
                    format!("解析配置失败: {e}"),
                    format!("Failed to parse config: {e}"),
                )
            })?;

            // 提取 request 配置
            let request: rquickjs::Object = config.get("request").map_err(|e| {
                AppError::localized(
                    "usage_script.request_missing",
                    format!("缺少 request 配置: {e}"),
                    format!("Missing request config: {e}"),
                )
            })?;

            // 将 request 转换为 JSON 字符串
            let request_json: String = ctx
                .json_stringify(request)
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.request_serialize_failed",
                        format!("序列化 request 失败: {e}"),
                        format!("Failed to serialize request: {e}"),
                    )
                })?
                .ok_or_else(|| {
                    AppError::localized(
                        "usage_script.serialize_none",
                        "序列化返回 None",
                        "Serialization returned None",
                    )
                })?
                .get()
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.get_string_failed",
                        format!("获取字符串失败: {e}"),
                        format!("Failed to get string: {e}"),
                    )
                })?;

            Ok::<_, AppError>(request_json)
        })?
    }; // Runtime 和 Context 在这里被 drop

    // 4. 解析 request 配置
    let request: RequestConfig = serde_json::from_str(&request_config).map_err(|e| {
        AppError::localized(
            "usage_script.request_format_invalid",
            format!("request 配置格式错误: {e}"),
            format!("Invalid request config format: {e}"),
        )
    })?;

    // 5. 验证请求 URL 是否安全（防止 SSRF）
    // 如果提供了 base_url，则验证同源；否则只做基本安全检查
    validate_request_url(&request.url, base_url, is_custom_template)?;

    // 6. 发送 HTTP 请求
    let response_data = send_http_request(&request, timeout_secs).await?;

    // 7. 在独立作用域中执行 extractor（确保 Runtime/Context 在函数结束前释放）
    let result: Value = {
        let runtime = Runtime::new().map_err(|e| {
            AppError::localized(
                "usage_script.runtime_create_failed",
                format!("创建 JS 运行时失败: {e}"),
                format!("Failed to create JS runtime: {e}"),
            )
        })?;
        let context = Context::full(&runtime).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!("创建 JS 上下文失败: {e}"),
                format!("Failed to create JS context: {e}"),
            )
        })?;

        context.with(|ctx| {
            // 重新 eval 获取配置对象
            let config: rquickjs::Object = ctx.eval(script_with_vars.clone()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_reparse_failed",
                    format!("重新解析配置失败: {e}"),
                    format!("Failed to re-parse config: {e}"),
                )
            })?;

            // 提取 extractor 函数
            let extractor: Function = config.get("extractor").map_err(|e| {
                AppError::localized(
                    "usage_script.extractor_missing",
                    format!("缺少 extractor 函数: {e}"),
                    format!("Missing extractor function: {e}"),
                )
            })?;

            // 将响应数据转换为 JS 值
            let response_js: rquickjs::Value =
                ctx.json_parse(response_data.as_str()).map_err(|e| {
                    AppError::localized(
                        "usage_script.response_parse_failed",
                        format!("解析响应 JSON 失败: {e}"),
                        format!("Failed to parse response JSON: {e}"),
                    )
                })?;

            // 调用 extractor(response)
            let result_js: rquickjs::Value = extractor.call((response_js,)).map_err(|e| {
                AppError::localized(
                    "usage_script.extractor_exec_failed",
                    format!("执行 extractor 失败: {e}"),
                    format!("Failed to execute extractor: {e}"),
                )
            })?;

            // 转换为 JSON 字符串
            let result_json: String = ctx
                .json_stringify(result_js)
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.result_serialize_failed",
                        format!("序列化结果失败: {e}"),
                        format!("Failed to serialize result: {e}"),
                    )
                })?
                .ok_or_else(|| {
                    AppError::localized(
                        "usage_script.serialize_none",
                        "序列化返回 None",
                        "Serialization returned None",
                    )
                })?
                .get()
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.get_string_failed",
                        format!("获取字符串失败: {e}"),
                        format!("Failed to get string: {e}"),
                    )
                })?;

            // 解析为 serde_json::Value
            serde_json::from_str(&result_json).map_err(|e| {
                AppError::localized(
                    "usage_script.json_parse_failed",
                    format!("JSON 解析失败: {e}"),
                    format!("JSON parse failed: {e}"),
                )
            })
        })?
    }; // Runtime 和 Context 在这里被 drop

    // 8. 验证返回值格式
    validate_result(&result)?;

    Ok(result)
}

/// 请求配置结构
#[derive(Debug, serde::Deserialize)]
struct RequestConfig {
    url: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

/// 发送 HTTP 请求
async fn send_http_request(config: &RequestConfig, timeout_secs: u64) -> Result<String, AppError> {
    // 使用全局 HTTP 客户端（已包含代理配置）
    let client = crate::proxy::http_client::get();
    // 约束超时范围，防止异常配置导致长时间阻塞（最小 2 秒，最大 30 秒）
    let request_timeout = std::time::Duration::from_secs(timeout_secs.clamp(2, 30));

    // 严格校验 HTTP 方法，非法值不回退为 GET
    let method: reqwest::Method = config.method.parse().map_err(|_| {
        AppError::localized(
            "usage_script.invalid_http_method",
            format!("不支持的 HTTP 方法: {}", config.method),
            format!("Unsupported HTTP method: {}", config.method),
        )
    })?;

    let mut req = client
        .request(method.clone(), &config.url)
        .timeout(request_timeout);

    // 添加请求头
    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    // 添加请求体
    if let Some(body) = &config.body {
        req = req.body(body.clone());
    }

    // 发送请求
    let resp = req.send().await.map_err(|e| {
        AppError::localized(
            "usage_script.request_failed",
            format!("请求失败: {e}"),
            format!("Request failed: {e}"),
        )
    })?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| {
        AppError::localized(
            "usage_script.read_response_failed",
            format!("读取响应失败: {e}"),
            format!("Failed to read response: {e}"),
        )
    })?;

    if !status.is_success() {
        let preview = if text.len() > 200 {
            let mut safe_cut = 200usize;
            while !text.is_char_boundary(safe_cut) {
                safe_cut = safe_cut.saturating_sub(1);
            }
            format!("{}...", &text[..safe_cut])
        } else {
            text.clone()
        };
        return Err(AppError::localized(
            "usage_script.http_error",
            format!("HTTP {status} : {preview}"),
            format!("HTTP {status} : {preview}"),
        ));
    }

    Ok(text)
}

/// 验证脚本返回值（支持单对象或数组）
fn validate_result(result: &Value) -> Result<(), AppError> {
    // 如果是数组，验证每个元素
    if let Some(arr) = result.as_array() {
        if arr.is_empty() {
            return Err(AppError::localized(
                "usage_script.empty_array",
                "脚本返回的数组不能为空",
                "Script returned empty array",
            ));
        }
        for (idx, item) in arr.iter().enumerate() {
            validate_single_usage(item).map_err(|e| {
                AppError::localized(
                    "usage_script.array_validation_failed",
                    format!("数组索引[{idx}]验证失败: {e}"),
                    format!("Validation failed at index [{idx}]: {e}"),
                )
            })?;
        }
        return Ok(());
    }

    // 如果是单对象，直接验证（向后兼容）
    validate_single_usage(result)
}

/// 验证单个用量数据对象
fn validate_single_usage(result: &Value) -> Result<(), AppError> {
    let obj = result.as_object().ok_or_else(|| {
        AppError::localized(
            "usage_script.must_return_object",
            "脚本必须返回对象或对象数组",
            "Script must return object or array of objects",
        )
    })?;

    // 所有字段均为可选，只进行类型检查
    if obj.contains_key("isValid")
        && !result["isValid"].is_null()
        && !result["isValid"].is_boolean()
    {
        return Err(AppError::localized(
            "usage_script.isvalid_type_error",
            "isValid 必须是布尔值或 null",
            "isValid must be boolean or null",
        ));
    }
    if obj.contains_key("invalidMessage")
        && !result["invalidMessage"].is_null()
        && !result["invalidMessage"].is_string()
    {
        return Err(AppError::localized(
            "usage_script.invalidmessage_type_error",
            "invalidMessage 必须是字符串或 null",
            "invalidMessage must be string or null",
        ));
    }
    if obj.contains_key("remaining")
        && !result["remaining"].is_null()
        && !result["remaining"].is_number()
    {
        return Err(AppError::localized(
            "usage_script.remaining_type_error",
            "remaining 必须是数字或 null",
            "remaining must be number or null",
        ));
    }
    if obj.contains_key("unit") && !result["unit"].is_null() && !result["unit"].is_string() {
        return Err(AppError::localized(
            "usage_script.unit_type_error",
            "unit 必须是字符串或 null",
            "unit must be string or null",
        ));
    }
    if obj.contains_key("total") && !result["total"].is_null() && !result["total"].is_number() {
        return Err(AppError::localized(
            "usage_script.total_type_error",
            "total 必须是数字或 null",
            "total must be number or null",
        ));
    }
    if obj.contains_key("used") && !result["used"].is_null() && !result["used"].is_number() {
        return Err(AppError::localized(
            "usage_script.used_type_error",
            "used 必须是数字或 null",
            "used must be number or null",
        ));
    }
    if obj.contains_key("planName")
        && !result["planName"].is_null()
        && !result["planName"].is_string()
    {
        return Err(AppError::localized(
            "usage_script.planname_type_error",
            "planName 必须是字符串或 null",
            "planName must be string or null",
        ));
    }
    if obj.contains_key("extra") && !result["extra"].is_null() && !result["extra"].is_string() {
        return Err(AppError::localized(
            "usage_script.extra_type_error",
            "extra 必须是字符串或 null",
            "extra must be string or null",
        ));
    }

    Ok(())
}

/// 构建替换变量后的脚本，保持与旧版脚本的兼容性
fn build_script_with_vars(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    access_token: Option<&str>,
    user_id: Option<&str>,
) -> String {
    let mut replaced = script_code
        .replace("{{apiKey}}", api_key)
        .replace("{{baseUrl}}", base_url);

    if let Some(token) = access_token {
        replaced = replaced.replace("{{accessToken}}", token);
    }
    if let Some(uid) = user_id {
        replaced = replaced.replace("{{userId}}", uid);
    }

    replaced
}

/// 验证 base_url 的基本安全性
fn validate_base_url(base_url: &str) -> Result<(), AppError> {
    if base_url.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_empty",
            "base_url 不能为空",
            "base_url cannot be empty",
        ));
    }

    // 解析 URL
    let parsed_url = Url::parse(base_url).map_err(|e| {
        AppError::localized(
            "usage_script.base_url_invalid",
            format!("无效的 base_url: {e}"),
            format!("Invalid base_url: {e}"),
        )
    })?;

    let is_loopback = is_loopback_host(&parsed_url);

    // 必须是 HTTPS（允许 localhost 用于开发）
    if parsed_url.scheme() != "https" && !is_loopback {
        return Err(AppError::localized(
            "usage_script.base_url_https_required",
            "base_url 必须使用 HTTPS 协议（localhost 除外）",
            "base_url must use HTTPS (localhost allowed)",
        ));
    }

    // 检查主机名格式有效性
    let hostname = parsed_url.host_str().ok_or_else(|| {
        AppError::localized(
            "usage_script.base_url_hostname_missing",
            "base_url 必须包含有效的主机名",
            "base_url must include a valid hostname",
        )
    })?;

    // 基本的主机名格式检查
    if hostname.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_hostname_empty",
            "base_url 主机名不能为空",
            "base_url hostname cannot be empty",
        ));
    }

    // 检查是否为明显的私有IP（但在 base_url 阶段不过于严格，主要在 request_url 阶段检查）
    if is_suspicious_hostname(hostname) {
        return Err(AppError::localized(
            "usage_script.base_url_suspicious",
            "base_url 包含可疑的主机名",
            "base_url contains a suspicious hostname",
        ));
    }

    Ok(())
}

/// 验证请求 URL 是否安全（防止 SSRF）
fn validate_request_url(
    request_url: &str,
    base_url: &str,
    is_custom_template: bool,
) -> Result<(), AppError> {
    // 解析请求 URL
    let parsed_request = Url::parse(request_url).map_err(|e| {
        AppError::localized(
            "usage_script.request_url_invalid",
            format!("无效的请求 URL: {e}"),
            format!("Invalid request URL: {e}"),
        )
    })?;

    let is_request_loopback = is_loopback_host(&parsed_request);

    // 必须使用 HTTPS（允许 localhost 用于开发）
    // 自定义模板模式下，允许用户自行决定是否使用 HTTP（用户需自行承担安全风险）
    if !is_custom_template && parsed_request.scheme() != "https" && !is_request_loopback {
        return Err(AppError::localized(
            "usage_script.request_https_required",
            "请求 URL 必须使用 HTTPS 协议（localhost 除外）",
            "Request URL must use HTTPS (localhost allowed)",
        ));
    }

    // 如果提供了 base_url（非空），则进行同源检查
    // 🔧 自定义模板模式下，用户可以自由访问任意 HTTPS 域名，跳过同源检查
    if !base_url.is_empty() && !is_custom_template {
        // 解析 base URL
        let parsed_base = Url::parse(base_url).map_err(|e| {
            AppError::localized(
                "usage_script.base_url_invalid",
                format!("无效的 base_url: {e}"),
                format!("Invalid base_url: {e}"),
            )
        })?;

        // 核心安全检查：必须与 base_url 同源（相同域名和端口）
        if parsed_request.host_str() != parsed_base.host_str() {
            return Err(AppError::localized(
                "usage_script.request_host_mismatch",
                format!(
                    "请求域名 {} 与 base_url 域名 {} 不匹配（必须是同源请求）",
                    parsed_request.host_str().unwrap_or("unknown"),
                    parsed_base.host_str().unwrap_or("unknown")
                ),
                format!(
                    "Request host {} must match base_url host {} (same-origin required)",
                    parsed_request.host_str().unwrap_or("unknown"),
                    parsed_base.host_str().unwrap_or("unknown")
                ),
            ));
        }

        // 检查端口是否匹配（考虑默认端口）
        // 使用 port_or_known_default() 会自动处理默认端口（http->80, https->443）
        match (
            parsed_request.port_or_known_default(),
            parsed_base.port_or_known_default(),
        ) {
            (Some(request_port), Some(base_port)) if request_port == base_port => {
                // 端口匹配，继续执行
            }
            (Some(request_port), Some(base_port)) => {
                return Err(AppError::localized(
                    "usage_script.request_port_mismatch",
                    format!("请求端口 {request_port} 必须与 base_url 端口 {base_port} 匹配"),
                    format!("Request port {request_port} must match base_url port {base_port}"),
                ));
            }
            _ => {
                // 理论上不会发生，因为 port_or_known_default() 应该总是返回 Some
                return Err(AppError::localized(
                    "usage_script.request_port_unknown",
                    "无法确定端口号",
                    "Unable to determine port number",
                ));
            }
        }

        // 禁止私有 IP 地址访问（除非 base_url 本身就是私有地址，用于开发环境）
        if let Some(host) = parsed_request.host_str() {
            let base_host = parsed_base.host_str().unwrap_or("");

            // 如果 base_url 不是私有地址，则禁止访问私有IP
            if !is_private_ip(base_host) && is_private_ip(host) {
                return Err(AppError::localized(
                    "usage_script.private_ip_blocked",
                    "禁止访问私有 IP 地址",
                    "Access to private IP addresses is blocked",
                ));
            }
        }
    } else {
        // 自定义模板模式：没有 base_url，需要额外的安全检查
        // 禁止访问私有 IP 地址（SSRF 防护）
        if let Some(host) = parsed_request.host_str() {
            if is_private_ip(host) && !is_request_loopback {
                return Err(AppError::localized(
                    "usage_script.private_ip_blocked",
                    "禁止访问私有 IP 地址（localhost 除外）",
                    "Access to private IP addresses is blocked (localhost allowed)",
                ));
            }
        }
    }

    Ok(())
}

/// 检查是否为私有 IP 地址
fn is_private_ip(host: &str) -> bool {
    // localhost 检查
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    // 尝试解析为IP地址
    if let Ok(ip_addr) = host.parse::<std::net::IpAddr>() {
        return is_private_ip_addr(ip_addr);
    }

    // 如果不是IP地址，不是私有IP
    false
}

/// 使用标准库API检查IP地址是否为私有地址
fn is_private_ip_addr(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();

            // 0.0.0.0/8 (包括未指定地址)
            if octets[0] == 0 {
                return true;
            }

            // RFC1918 私有地址范围
            // 10.0.0.0/8
            if octets[0] == 10 {
                return true;
            }

            // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
            if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                return true;
            }

            // 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }

            // 其他特殊地址
            // 169.254.0.0/16 (链路本地地址)
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }

            // 127.0.0.0/8 (环回地址)
            if octets[0] == 127 {
                return true;
            }

            false
        }
        std::net::IpAddr::V6(ipv6) => {
            // IPv6 私有地址检查 - 使用标准库方法

            // ::1 (环回地址)
            if ipv6.is_loopback() {
                return true;
            }

            // 唯一本地地址 (fc00::/7)
            // Rust 1.70+ 可以使用 ipv6.is_unique_local()
            // 但为了兼容性，我们手动检查
            let first_segment = ipv6.segments()[0];
            if (first_segment & 0xfe00) == 0xfc00 {
                return true;
            }

            // 链路本地地址 (fe80::/10)
            if (first_segment & 0xffc0) == 0xfe80 {
                return true;
            }

            // 未指定地址 ::
            if ipv6.is_unspecified() {
                return true;
            }

            false
        }
    }
}

/// 检查是否为可疑的主机名（只检查明显不安全的模式）
fn is_suspicious_hostname(hostname: &str) -> bool {
    // 空主机名
    if hostname.is_empty() {
        return true;
    }

    // 检查明显的主机名格式问题
    if hostname.contains("..") || hostname.starts_with(".") || hostname.ends_with(".") {
        return true;
    }

    // 检查是否为纯IP地址但没有合理格式（过于宽松的检查在这里可能不够，但主要依赖后续的同源检查）
    if hostname.parse::<std::net::IpAddr>().is_ok() {
        // IP地址格式的，在这里不直接拒绝，让同源检查来处理
        return false;
    }

    // 检查是否包含明显不当的字符
    let suspicious_chars = ['<', '>', '"', '\'', '\n', '\r', '\t', '\0'];
    if hostname.chars().any(|c| suspicious_chars.contains(&c)) {
        return true;
    }

    false
}

/// 判断 URL 是否指向本机（localhost / loopback）
fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_ip_validation() {
        // 测试IPv4私网地址

        // RFC1918私网地址 - 应该返回true
        assert!(is_private_ip("10.0.0.1"));
        assert!(is_private_ip("10.255.255.254"));
        assert!(is_private_ip("172.16.0.1"));
        assert!(is_private_ip("172.31.255.255"));
        assert!(is_private_ip("192.168.0.1"));
        assert!(is_private_ip("192.168.255.255"));

        // 链路本地地址 - 应该返回true
        assert!(is_private_ip("169.254.0.1"));
        assert!(is_private_ip("169.254.255.255"));

        // 环回地址 - 应该返回true
        assert!(is_private_ip("127.0.0.1"));
        assert!(is_private_ip("localhost"));

        // 公网172.x.x.x地址 - 应该返回false（这是修复的重点）
        assert!(!is_private_ip("172.0.0.1"));
        assert!(!is_private_ip("172.15.255.255"));
        assert!(!is_private_ip("172.32.0.1"));
        assert!(!is_private_ip("172.64.0.1"));
        assert!(!is_private_ip("172.67.0.1")); // Cloudflare CDN
        assert!(!is_private_ip("172.68.0.1"));
        assert!(!is_private_ip("172.100.50.25"));
        assert!(!is_private_ip("172.255.255.255"));

        // 其他公网地址 - 应该返回false
        assert!(!is_private_ip("8.8.8.8")); // Google DNS
        assert!(!is_private_ip("1.1.1.1")); // Cloudflare DNS
        assert!(!is_private_ip("208.67.222.222")); // OpenDNS
        assert!(!is_private_ip("180.76.76.76")); // Baidu DNS

        // 域名 - 应该返回false
        assert!(!is_private_ip("api.example.com"));
        assert!(!is_private_ip("www.google.com"));
    }

    #[test]
    fn test_ipv6_private_validation() {
        // IPv6私网地址
        assert!(is_private_ip("::1")); // 环回地址
        assert!(is_private_ip("fc00::1")); // 唯一本地地址
        assert!(is_private_ip("fd00::1")); // 唯一本地地址
        assert!(is_private_ip("fe80::1")); // 链路本地地址
        assert!(is_private_ip("::")); // 未指定地址

        // IPv6公网地址 - 应该返回false（修复的重点）
        assert!(!is_private_ip("2001:4860:4860::8888")); // Google DNS IPv6
        assert!(!is_private_ip("2606:4700:4700::1111")); // Cloudflare DNS IPv6
        assert!(!is_private_ip("2404:6800:4001:c01::67")); // Google DNS IPv6 (其他格式)
        assert!(!is_private_ip("2001:db8::1")); // 文档地址（非私网）

        // 测试包含 ::1 子串但不是环回地址的公网地址
        assert!(!is_private_ip("2001:db8::1abc")); // 包含 ::1abc 但不是环回
        assert!(!is_private_ip("2606:4700::1")); // 包含 ::1 但不是环回
    }

    #[test]
    fn test_hostname_bypass_prevention() {
        // 看起来像本地，但实际是域名
        assert!(!is_private_ip("127.0.0.1.evil.com"));
        assert!(!is_private_ip("localhost.evil.com"));

        // 0.0.0.0 应该被视为本地/阻断
        assert!(is_private_ip("0.0.0.0"));
    }

    #[test]
    fn test_https_bypass_prevention() {
        // 非本地域名的 HTTP 应该被拒绝
        let result = validate_base_url("http://127.0.0.1.evil.com/api");
        assert!(
            result.is_err(),
            "Should reject HTTP for non-localhost domains"
        );
    }

    #[test]
    fn test_edge_cases() {
        // 边界情况测试
        assert!(is_private_ip("172.16.0.0")); // RFC1918起始
        assert!(is_private_ip("172.31.255.255")); // RFC1918结束
        assert!(is_private_ip("10.0.0.0")); // 10.0.0.0/8起始
        assert!(is_private_ip("10.255.255.255")); // 10.0.0.0/8结束
        assert!(is_private_ip("192.168.0.0")); // 192.168.0.0/16起始
        assert!(is_private_ip("192.168.255.255")); // 192.168.0.0/16结束

        // 紧邻RFC1918的公网地址 - 应该返回false
        assert!(!is_private_ip("172.15.255.255")); // 172.16.0.0的前一个
        assert!(!is_private_ip("172.32.0.0")); // 172.31.255.255的后一个
    }

    #[test]
    fn test_ip_addr_parsing() {
        // 测试IP地址解析功能
        let ipv4_private = "10.0.0.1".parse::<std::net::IpAddr>().unwrap();
        assert!(is_private_ip_addr(ipv4_private));

        let ipv4_public = "172.67.0.1".parse::<std::net::IpAddr>().unwrap();
        assert!(!is_private_ip_addr(ipv4_public));

        let ipv6_private = "fc00::1".parse::<std::net::IpAddr>().unwrap();
        assert!(is_private_ip_addr(ipv6_private));

        let ipv6_public = "2001:4860:4860::8888".parse::<std::net::IpAddr>().unwrap();
        assert!(!is_private_ip_addr(ipv6_public));
    }

    #[test]
    fn test_port_comparison() {
        // 测试端口比较逻辑是否正确处理默认端口和显式端口

        // 测试用例：(base_url, request_url, should_match)
        let test_cases = vec![
            // HTTPS默认端口测试
            (
                "https://api.example.com",
                "https://api.example.com/v1/test",
                true,
            ),
            (
                "https://api.example.com",
                "https://api.example.com:443/v1/test",
                true,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com/v1/test",
                true,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com:443/v1/test",
                true,
            ),
            // 端口不匹配测试
            (
                "https://api.example.com",
                "https://api.example.com:8443/v1/test",
                false,
            ),
            (
                "https://api.example.com:443",
                "https://api.example.com:8443/v1/test",
                false,
            ),
        ];

        for (base_url, request_url, should_match) in test_cases {
            let result = validate_request_url(request_url, base_url, false);

            if should_match {
                assert!(
                    result.is_ok(),
                    "应该匹配的URL被拒绝: base_url={}, request_url={}, error={}",
                    base_url,
                    request_url,
                    result.unwrap_err()
                );
            } else {
                assert!(
                    result.is_err(),
                    "应该不匹配的URL被允许: base_url={}, request_url={}",
                    base_url,
                    request_url
                );
            }
        }
    }
}
