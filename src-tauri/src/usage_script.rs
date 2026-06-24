use rquickjs::{Context, Function, Object, Runtime};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use url::{Host, Url};

use crate::error::AppError;

const MAX_USAGE_SCRIPT_REQUESTS: usize = 5;

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
    if should_validate_base_url(base_url, is_custom_template) {
        validate_base_url(base_url)?;
    }

    // 3. 在独立作用域中提取 request/requests 配置（确保 Runtime/Context 在 await 前释放）
    let request_spec = extract_request_spec_json(&script_with_vars)?;

    // 4. 解析 request/requests 配置
    let request_spec = parse_request_spec(&request_spec)?;

    // 5. 验证请求 URL（HTTPS 强制 + 同源检查）
    validate_request_spec(&request_spec, base_url, is_custom_template)?;

    // 6. 发送 HTTP 请求
    let response_data = send_request_spec(&request_spec, timeout_secs).await?;

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
            let config: rquickjs::Object = ctx.eval(script_with_vars).map_err(|e| {
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

            // 调用 extractor(response)。旧版 request 路径传单个响应对象；
            // 新版 requests 路径传 { key: response } 对象。
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

fn extract_request_spec_json(script_with_vars: &str) -> Result<String, AppError> {
    let request_spec = {
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
            let config: rquickjs::Object = ctx.eval(script_with_vars).map_err(|e| {
                AppError::localized(
                    "usage_script.config_parse_failed",
                    format!("解析配置失败: {e}"),
                    format!("Failed to parse config: {e}"),
                )
            })?;

            // 先选择有效契约，再只序列化对应字段，避免无关字段影响旧版 request。
            let request_contract: Object = Object::new(ctx.clone()).map_err(|e| {
                AppError::localized(
                    "usage_script.config_serialize_failed",
                    format!("创建请求配置对象失败: {e}"),
                    format!("Failed to create request config object: {e}"),
                )
            })?;

            if config.contains_key("request").map_err(|e| {
                AppError::localized(
                    "usage_script.config_serialize_failed",
                    format!("检查 request 配置失败: {e}"),
                    format!("Failed to check request config: {e}"),
                )
            })? {
                let request_value: rquickjs::Value = config.get("request").map_err(|e| {
                    AppError::localized(
                        "usage_script.config_serialize_failed",
                        format!("提取 request 配置失败: {e}"),
                        format!("Failed to extract request config: {e}"),
                    )
                })?;
                request_contract
                    .set("request", request_value)
                    .map_err(|e| {
                        AppError::localized(
                            "usage_script.config_serialize_failed",
                            format!("设置 request 配置失败: {e}"),
                            format!("Failed to set request config: {e}"),
                        )
                    })?;
            } else {
                let requests_value: rquickjs::Value = config.get("requests").map_err(|e| {
                    AppError::localized(
                        "usage_script.config_serialize_failed",
                        format!("提取 requests 配置失败: {e}"),
                        format!("Failed to extract requests config: {e}"),
                    )
                })?;
                request_contract
                    .set("requests", requests_value)
                    .map_err(|e| {
                        AppError::localized(
                            "usage_script.config_serialize_failed",
                            format!("设置 requests 配置失败: {e}"),
                            format!("Failed to set requests config: {e}"),
                        )
                    })?;
            }

            let config_json: String = ctx
                .json_stringify(request_contract)
                .map_err(|e| {
                    AppError::localized(
                        "usage_script.config_serialize_failed",
                        format!("序列化配置失败: {e}"),
                        format!("Failed to serialize config: {e}"),
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

            Ok::<_, AppError>(config_json)
        })?
    }; // Runtime 和 Context 在这里被 drop
    Ok(request_spec)
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

#[derive(Debug, serde::Deserialize)]
struct UsageScriptConfig {
    #[serde(default)]
    request: Option<RequestConfig>,
    #[serde(default)]
    requests: Option<BTreeMap<String, RequestConfig>>,
}

#[derive(Debug)]
enum RequestSpec {
    Single(RequestConfig),
    Multi(BTreeMap<String, RequestConfig>),
}

fn parse_request_spec(config_json: &str) -> Result<RequestSpec, AppError> {
    let config: UsageScriptConfig = serde_json::from_str(config_json).map_err(|e| {
        AppError::localized(
            "usage_script.config_format_invalid",
            format!("配置格式错误: {e}"),
            format!("Invalid config format: {e}"),
        )
    })?;

    if let Some(request) = config.request {
        return Ok(RequestSpec::Single(request));
    }

    if let Some(requests) = config.requests {
        if requests.is_empty() {
            return Err(AppError::localized(
                "usage_script.requests_empty",
                "requests 配置不能为空",
                "requests config cannot be empty",
            ));
        }
        if requests.len() > MAX_USAGE_SCRIPT_REQUESTS {
            return Err(AppError::localized(
                "usage_script.requests_too_many",
                format!("requests 最多支持 {MAX_USAGE_SCRIPT_REQUESTS} 个请求"),
                format!("requests supports at most {MAX_USAGE_SCRIPT_REQUESTS} entries"),
            ));
        }
        return Ok(RequestSpec::Multi(requests));
    }

    Err(AppError::localized(
        "usage_script.request_missing",
        "缺少 request 或 requests 配置",
        "Missing request or requests config",
    ))
}

fn validate_request_spec(
    spec: &RequestSpec,
    base_url: &str,
    is_custom_template: bool,
) -> Result<(), AppError> {
    match spec {
        RequestSpec::Single(request) => {
            validate_request_url(&request.url, base_url, is_custom_template)
        }
        RequestSpec::Multi(requests) => {
            for (name, request) in requests {
                validate_request_url(&request.url, base_url, is_custom_template).map_err(|e| {
                    AppError::localized(
                        "usage_script.request_validation_failed",
                        format!("requests.{name} 验证失败: {e}"),
                        format!("requests.{name} validation failed: {e}"),
                    )
                })?;
            }
            Ok(())
        }
    }
}

async fn send_request_spec(spec: &RequestSpec, timeout_secs: u64) -> Result<String, AppError> {
    match spec {
        RequestSpec::Single(request) => send_http_request(request, timeout_secs).await,
        RequestSpec::Multi(requests) => {
            let mut responses = serde_json::Map::new();
            for (name, request) in requests {
                let response_text =
                    send_http_request(request, timeout_secs)
                        .await
                        .map_err(|e| {
                            AppError::localized(
                                "usage_script.request_failed_named",
                                format!("requests.{name} 请求失败: {e}"),
                                format!("requests.{name} request failed: {e}"),
                            )
                        })?;
                let response_value: Value = serde_json::from_str(&response_text).map_err(|e| {
                    AppError::localized(
                        "usage_script.response_parse_failed",
                        format!("requests.{name} 响应不是有效 JSON: {e}"),
                        format!("requests.{name} response is not valid JSON: {e}"),
                    )
                })?;
                responses.insert(name.clone(), response_value);
            }
            serde_json::to_string(&Value::Object(responses)).map_err(|e| {
                AppError::localized(
                    "usage_script.response_serialize_failed",
                    format!("序列化多请求响应失败: {e}"),
                    format!("Failed to serialize multi-request response: {e}"),
                )
            })
        }
    }
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

    Ok(())
}

fn should_validate_base_url(base_url: &str, is_custom_template: bool) -> bool {
    !base_url.is_empty() && !is_custom_template
}

/// 验证请求 URL 是否安全（HTTPS 强制 + 同源检查）
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
    }

    Ok(())
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
    fn parse_request_spec_prefers_legacy_request_when_both_exist() {
        let config = r#"{
            "request": {
                "url": "https://api.example.com/legacy",
                "method": "GET"
            },
            "requests": {
                "quota": {
                    "url": "https://api.example.com/quota",
                    "method": "GET"
                }
            }
        }"#;

        let spec = parse_request_spec(config).expect("config should parse");
        match spec {
            RequestSpec::Single(request) => {
                assert_eq!(request.url, "https://api.example.com/legacy");
            }
            RequestSpec::Multi(_) => panic!("legacy request must keep old behavior"),
        }
    }

    #[test]
    fn parse_request_spec_accepts_named_requests() {
        let config = r#"{
            "requests": {
                "quota": {
                    "url": "https://api.example.com/quota",
                    "method": "GET"
                },
                "usage": {
                    "url": "https://api.example.com/usage",
                    "method": "POST",
                    "headers": {
                        "Authorization": "Bearer sk-test"
                    },
                    "body": "{}"
                }
            }
        }"#;

        let spec = parse_request_spec(config).expect("config should parse");
        match spec {
            RequestSpec::Multi(requests) => {
                assert_eq!(requests.len(), 2);
                assert_eq!(
                    requests.get("quota").expect("quota request").url,
                    "https://api.example.com/quota"
                );
                assert_eq!(requests.get("usage").expect("usage request").method, "POST");
            }
            RequestSpec::Single(_) => panic!("requests-only config should use multi mode"),
        }
    }

    #[test]
    fn parse_request_spec_accepts_max_named_requests() {
        let config = r#"{
            "requests": {
                "one": { "url": "https://api.example.com/one", "method": "GET" },
                "two": { "url": "https://api.example.com/two", "method": "GET" },
                "three": { "url": "https://api.example.com/three", "method": "GET" },
                "four": { "url": "https://api.example.com/four", "method": "GET" },
                "five": { "url": "https://api.example.com/five", "method": "GET" }
            }
        }"#;

        let spec = parse_request_spec(config).expect("max requests should parse");
        match spec {
            RequestSpec::Multi(requests) => {
                assert_eq!(requests.len(), MAX_USAGE_SCRIPT_REQUESTS);
            }
            RequestSpec::Single(_) => panic!("requests-only config should use multi mode"),
        }
    }

    #[test]
    fn parse_request_spec_rejects_too_many_named_requests() {
        let config = r#"{
            "requests": {
                "one": { "url": "https://api.example.com/one", "method": "GET" },
                "two": { "url": "https://api.example.com/two", "method": "GET" },
                "three": { "url": "https://api.example.com/three", "method": "GET" },
                "four": { "url": "https://api.example.com/four", "method": "GET" },
                "five": { "url": "https://api.example.com/five", "method": "GET" },
                "six": { "url": "https://api.example.com/six", "method": "GET" }
            }
        }"#;

        let err = parse_request_spec(config).expect_err("too many requests fail");
        assert!(err
            .to_string()
            .contains(&format!("最多支持 {MAX_USAGE_SCRIPT_REQUESTS} 个请求")));
    }

    #[test]
    fn parse_request_spec_rejects_empty_named_requests() {
        let err = parse_request_spec(r#"{"requests": {}}"#).expect_err("empty requests fail");
        assert!(err.to_string().contains("requests 配置不能为空"));
    }

    #[test]
    fn extract_request_spec_ignores_unrelated_non_serializable_config_fields() {
        let script = r#"({
            request: {
                url: "https://api.example.com/usage",
                method: "GET"
            },
            unrelated: BigInt(1),
            extractor: function(response) {
                return response;
            }
        })"#;

        let config_json =
            extract_request_spec_json(script).expect("unrelated fields should not be serialized");
        let spec = parse_request_spec(&config_json).expect("request spec should parse");
        match spec {
            RequestSpec::Single(request) => {
                assert_eq!(request.url, "https://api.example.com/usage");
            }
            RequestSpec::Multi(_) => panic!("request-only config should use single mode"),
        }
    }

    #[test]
    fn extract_request_spec_ignores_requests_when_legacy_request_exists() {
        let script = r#"({
            request: {
                url: "https://api.example.com/legacy",
                method: "GET"
            },
            requests: {
                quota: {
                    url: "https://api.example.com/quota",
                    method: "GET",
                    body: BigInt(1)
                }
            },
            extractor: function(response) {
                return response;
            }
        })"#;

        let config_json =
            extract_request_spec_json(script).expect("legacy request should ignore requests");
        let spec = parse_request_spec(&config_json).expect("request spec should parse");
        match spec {
            RequestSpec::Single(request) => {
                assert_eq!(request.url, "https://api.example.com/legacy");
            }
            RequestSpec::Multi(_) => panic!("legacy request must keep old behavior"),
        }
    }

    #[test]
    fn validate_request_spec_checks_all_named_requests() {
        let spec = RequestSpec::Multi(BTreeMap::from([
            (
                "quota".to_string(),
                RequestConfig {
                    url: "https://api.example.com/quota".to_string(),
                    method: "GET".to_string(),
                    headers: HashMap::new(),
                    body: None,
                },
            ),
            (
                "usage".to_string(),
                RequestConfig {
                    url: "https://other.example.com/usage".to_string(),
                    method: "GET".to_string(),
                    headers: HashMap::new(),
                    body: None,
                },
            ),
        ]));

        let err = validate_request_spec(&spec, "https://api.example.com", false)
            .expect_err("host mismatch should fail");
        assert!(err.to_string().contains("requests.usage"));
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
    fn test_custom_template_allows_http_lan_request_with_different_base_url() {
        assert!(
            !should_validate_base_url("http://10.37.192.156:8090/anthropic", true),
            "Custom scripts should not validate an unused provider base_url fallback"
        );

        let result = validate_request_url(
            "http://10.37.192.156:18344/user/balance",
            "http://10.37.192.156:8090/anthropic",
            true,
        );
        assert!(
            result.is_ok(),
            "Custom usage scripts should be able to call an explicit HTTP quota endpoint"
        );
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
