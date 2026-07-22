use rquickjs::{Context, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

use crate::error::AppError;

/// 执行用量查询脚本
#[allow(clippy::too_many_arguments)]
pub async fn execute_usage_script(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout_secs: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
    allow_private_network: bool,
) -> Result<Value, AppError> {
    let is_custom_template = template_type.map(|t| t == "custom").unwrap_or(false);

    // Rewrite placeholders to JS identifiers; secrets are injected as globals (not into source).
    let script_with_vars = rewrite_placeholders(script_code);

    if should_validate_base_url(base_url, is_custom_template) {
        validate_base_url(base_url, allow_private_network)?;
    }

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
            inject_script_globals(
                &ctx,
                api_key,
                base_url,
                access_token.unwrap_or(""),
                user_id.unwrap_or(""),
            )?;

            let config: rquickjs::Object = ctx.eval(script_with_vars.clone()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_parse_failed",
                    format!("解析配置失败: {e}"),
                    format!("Failed to parse config: {e}"),
                )
            })?;

            let request: rquickjs::Object = config.get("request").map_err(|e| {
                AppError::localized(
                    "usage_script.request_missing",
                    format!("缺少 request 配置: {e}"),
                    format!("Missing request config: {e}"),
                )
            })?;

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
    };

    let request: RequestConfig = serde_json::from_str(&request_config).map_err(|e| {
        AppError::localized(
            "usage_script.request_format_invalid",
            format!("request 配置格式错误: {e}"),
            format!("Invalid request config format: {e}"),
        )
    })?;

    validate_request_url(
        &request.url,
        base_url,
        is_custom_template,
        allow_private_network,
    )?;

    let response_data = send_http_request(&request, timeout_secs).await?;

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
            inject_script_globals(
                &ctx,
                api_key,
                base_url,
                access_token.unwrap_or(""),
                user_id.unwrap_or(""),
            )?;

            let config: rquickjs::Object = ctx.eval(script_with_vars.clone()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_reparse_failed",
                    format!("重新解析配置失败: {e}"),
                    format!("Failed to re-parse config: {e}"),
                )
            })?;

            let extractor: Function = config.get("extractor").map_err(|e| {
                AppError::localized(
                    "usage_script.extractor_missing",
                    format!("缺少 extractor 函数: {e}"),
                    format!("Missing extractor function: {e}"),
                )
            })?;

            let response_js: rquickjs::Value =
                ctx.json_parse(response_data.as_str()).map_err(|e| {
                    AppError::localized(
                        "usage_script.response_parse_failed",
                        format!("解析响应 JSON 失败: {e}"),
                        format!("Failed to parse response JSON: {e}"),
                    )
                })?;

            let result_js: rquickjs::Value = extractor.call((response_js,)).map_err(|e| {
                AppError::localized(
                    "usage_script.extractor_exec_failed",
                    format!("执行 extractor 失败: {e}"),
                    format!("Failed to execute extractor: {e}"),
                )
            })?;

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

            serde_json::from_str(&result_json).map_err(|e| {
                AppError::localized(
                    "usage_script.json_parse_failed",
                    format!("JSON 解析失败: {e}"),
                    format!("JSON parse failed: {e}"),
                )
            })
        })?
    };

    validate_result(&result)?;
    Ok(result)
}

#[derive(Debug, serde::Deserialize)]
struct RequestConfig {
    url: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

fn inject_script_globals<'js>(
    ctx: &rquickjs::Ctx<'js>,
    api_key: &str,
    base_url: &str,
    access_token: &str,
    user_id: &str,
) -> Result<(), AppError> {
    let globals = ctx.globals();
    globals.set("apiKey", api_key).map_err(|e| {
        AppError::localized(
            "usage_script.inject_global_failed",
            format!("注入 apiKey 失败: {e}"),
            format!("Failed to inject apiKey: {e}"),
        )
    })?;
    globals.set("baseUrl", base_url).map_err(|e| {
        AppError::localized(
            "usage_script.inject_global_failed",
            format!("注入 baseUrl 失败: {e}"),
            format!("Failed to inject baseUrl: {e}"),
        )
    })?;
    globals.set("accessToken", access_token).map_err(|e| {
        AppError::localized(
            "usage_script.inject_global_failed",
            format!("注入 accessToken 失败: {e}"),
            format!("Failed to inject accessToken: {e}"),
        )
    })?;
    globals.set("userId", user_id).map_err(|e| {
        AppError::localized(
            "usage_script.inject_global_failed",
            format!("注入 userId 失败: {e}"),
            format!("Failed to inject userId: {e}"),
        )
    })?;
    Ok(())
}

/// Rewrite `{{apiKey}}` etc. into JS string concatenations referencing globals.
fn rewrite_placeholders(script_code: &str) -> String {
    script_code
        .replace("{{apiKey}}", "\" + apiKey + \"")
        .replace("{{baseUrl}}", "\" + baseUrl + \"")
        .replace("{{accessToken}}", "\" + accessToken + \"")
        .replace("{{userId}}", "\" + userId + \"")
}

async fn send_http_request(config: &RequestConfig, timeout_secs: u64) -> Result<String, AppError> {
    let client = crate::proxy::http_client::get();
    let request_timeout = std::time::Duration::from_secs(timeout_secs.clamp(2, 30));

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

    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    if let Some(body) = &config.body {
        req = req.body(body.clone());
    }

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

fn validate_result(result: &Value) -> Result<(), AppError> {
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
    validate_single_usage(result)
}

fn validate_single_usage(result: &Value) -> Result<(), AppError> {
    let obj = result.as_object().ok_or_else(|| {
        AppError::localized(
            "usage_script.must_return_object",
            "脚本必须返回对象或对象数组",
            "Script must return object or array of objects",
        )
    })?;

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

fn validate_base_url(base_url: &str, allow_private_network: bool) -> Result<(), AppError> {
    if base_url.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_empty",
            "base_url 不能为空",
            "base_url cannot be empty",
        ));
    }

    let parsed_url = Url::parse(base_url).map_err(|e| {
        AppError::localized(
            "usage_script.base_url_invalid",
            format!("无效的 base_url: {e}"),
            format!("Invalid base_url: {e}"),
        )
    })?;

    let is_loopback = is_loopback_host(&parsed_url);

    if parsed_url.scheme() != "https" && !is_loopback {
        return Err(AppError::localized(
            "usage_script.base_url_https_required",
            "base_url 必须使用 HTTPS 协议（localhost 除外）",
            "base_url must use HTTPS (localhost allowed)",
        ));
    }

    let hostname = parsed_url.host_str().ok_or_else(|| {
        AppError::localized(
            "usage_script.base_url_hostname_missing",
            "base_url 必须包含有效的主机名",
            "base_url must include a valid hostname",
        )
    })?;

    if hostname.is_empty() {
        return Err(AppError::localized(
            "usage_script.base_url_hostname_empty",
            "base_url 主机名不能为空",
            "base_url hostname cannot be empty",
        ));
    }

    deny_unsafe_destination(&parsed_url, allow_private_network)?;
    Ok(())
}

fn should_validate_base_url(base_url: &str, is_custom_template: bool) -> bool {
    !base_url.is_empty() && !is_custom_template
}

fn validate_request_url(
    request_url: &str,
    base_url: &str,
    is_custom_template: bool,
    allow_private_network: bool,
) -> Result<(), AppError> {
    let parsed_request = Url::parse(request_url).map_err(|e| {
        AppError::localized(
            "usage_script.request_url_invalid",
            format!("无效的请求 URL: {e}"),
            format!("Invalid request URL: {e}"),
        )
    })?;

    let is_request_loopback = is_loopback_host(&parsed_request);

    // HTTPS required unless loopback, or custom+allow_private_network for LAN HTTP.
    let http_ok = is_request_loopback
        || (is_custom_template
            && allow_private_network
            && parsed_request.scheme() == "http"
            && is_private_or_link_local_host(&parsed_request));

    if parsed_request.scheme() != "https" && !http_ok {
        return Err(AppError::localized(
            "usage_script.request_https_required",
            "请求 URL 必须使用 HTTPS（localhost 除外；自定义脚本需显式允许私有网络才可用 HTTP）",
            "Request URL must use HTTPS (localhost allowed; custom scripts need allowPrivateNetwork for LAN HTTP)",
        ));
    }

    // Always deny metadata / link-local SSRF targets.
    deny_unsafe_destination(&parsed_request, allow_private_network)?;

    if !base_url.is_empty() && !is_custom_template {
        let parsed_base = Url::parse(base_url).map_err(|e| {
            AppError::localized(
                "usage_script.base_url_invalid",
                format!("无效的 base_url: {e}"),
                format!("Invalid base_url: {e}"),
            )
        })?;

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

        match (
            parsed_request.port_or_known_default(),
            parsed_base.port_or_known_default(),
        ) {
            (Some(request_port), Some(base_port)) if request_port == base_port => {}
            (Some(request_port), Some(base_port)) => {
                return Err(AppError::localized(
                    "usage_script.request_port_mismatch",
                    format!("请求端口 {request_port} 必须与 base_url 端口 {base_port} 匹配"),
                    format!("Request port {request_port} must match base_url port {base_port}"),
                ));
            }
            _ => {
                return Err(AppError::localized(
                    "usage_script.request_port_unknown",
                    "无法确定端口号",
                    "Unable to determine port number",
                ));
            }
        }
    }

    Ok(())
}

/// Block cloud metadata and (unless opted in) private LAN destinations.
fn deny_unsafe_destination(url: &Url, allow_private_network: bool) -> Result<(), AppError> {
    if is_loopback_host(url) {
        return Ok(());
    }

    if is_metadata_or_link_local_host(url) {
        return Err(AppError::localized(
            "usage_script.destination_blocked",
            "禁止访问链路本地地址或云元数据服务（SSRF 防护）",
            "Link-local addresses and cloud metadata endpoints are blocked (SSRF protection)",
        ));
    }

    if !allow_private_network && is_private_or_link_local_host(url) {
        return Err(AppError::localized(
            "usage_script.private_network_blocked",
            "禁止访问私有网络地址；自定义脚本请开启「允许私有网络」",
            "Private network addresses are blocked; enable allowPrivateNetwork for custom scripts",
        ));
    }

    Ok(())
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

fn is_metadata_hostname(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "metadata.google.internal"
        || h == "metadata"
        || h == "instance-data"
        || h.ends_with(".metadata.google.internal")
}

fn is_metadata_or_link_local_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => {
            is_metadata_hostname(d) || d == "169.254.169.254" || d.starts_with("169.254.")
        }
        Some(Host::Ipv4(ip)) => is_link_local_v4(ip) || ip == Ipv4Addr::new(169, 254, 169, 254),
        Some(Host::Ipv6(ip)) => is_link_local_v6(ip),
        _ => false,
    }
}

fn is_private_or_link_local_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => {
            // Unresolved hostnames that look like IPs
            if let Ok(ip) = d.parse::<Ipv4Addr>() {
                return is_private_v4(ip) || is_link_local_v4(ip);
            }
            false
        }
        Some(Host::Ipv4(ip)) => is_private_v4(ip) || is_link_local_v4(ip),
        Some(Host::Ipv6(ip)) => is_private_v6(ip) || is_link_local_v6(ip),
        _ => false,
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    // RFC1918 + Carrier-grade NAT 100.64.0.0/10
    ip.is_private() || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64)
}

fn is_link_local_v4(ip: Ipv4Addr) -> bool {
    ip.is_link_local()
}

fn is_link_local_v6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    // Unique local addresses fc00::/7
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_https_bypass_prevention() {
        let result = validate_base_url("http://127.0.0.1.evil.com/api", false);
        assert!(
            result.is_err(),
            "Should reject HTTP for non-localhost domains"
        );
    }

    #[test]
    fn test_custom_lan_http_requires_allow_private_network() {
        assert!(!should_validate_base_url(
            "http://10.37.192.156:8090/anthropic",
            true
        ));

        let denied = validate_request_url(
            "http://10.37.192.156:18344/user/balance",
            "http://10.37.192.156:8090/anthropic",
            true,
            false,
        );
        assert!(
            denied.is_err(),
            "LAN HTTP must be denied without allow_private_network"
        );

        let allowed = validate_request_url(
            "http://10.37.192.156:18344/user/balance",
            "http://10.37.192.156:8090/anthropic",
            true,
            true,
        );
        assert!(
            allowed.is_ok(),
            "LAN HTTP should work when allow_private_network is set"
        );
    }

    #[test]
    fn test_metadata_always_blocked() {
        for allow in [false, true] {
            let result = validate_request_url(
                "http://169.254.169.254/latest/meta-data/",
                "",
                true,
                allow,
            );
            assert!(
                result.is_err(),
                "metadata IP must always be blocked (allow={allow})"
            );
        }
    }

    #[test]
    fn test_rewrite_placeholders_does_not_embed_secret() {
        let rewritten = rewrite_placeholders(
            r#"({ request: { url: "{{baseUrl}}/x", headers: { Authorization: "Bearer {{apiKey}}" } } })"#,
        );
        assert!(!rewritten.contains("sk-secret"));
        assert!(rewritten.contains("apiKey"));
        assert!(rewritten.contains("baseUrl"));
    }

    #[test]
    fn test_port_comparison() {
        let test_cases = vec![
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
                "https://api.example.com",
                "https://api.example.com:8443/v1/test",
                false,
            ),
        ];

        for (base_url, request_url, should_match) in test_cases {
            let result = validate_request_url(request_url, base_url, false, false);
            if should_match {
                assert!(
                    result.is_ok(),
                    "应该匹配的URL被拒绝: base_url={base_url}, request_url={request_url}, error={}",
                    result.unwrap_err()
                );
            } else {
                assert!(
                    result.is_err(),
                    "应该不匹配的URL被允许: base_url={base_url}, request_url={request_url}"
                );
            }
        }
    }
}
