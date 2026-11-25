use rquickjs::{Context, Function, Persistent, Runtime};
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
    if should_validate_base_url(base_url, is_custom_template) {
        validate_base_url(base_url)?;
    }

    // QuickJS values never leave this blocking worker. Reuse its runtime and evaluated
    // config across HTTP, which is still driven by the Tokio executor. The worker is
    // reserved for this query (HTTP is bounded to 30s); async workers remain available.
    let base_url = base_url.to_owned();
    let executor = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let runtime = create_script_runtime()?;
        let context = Context::full(&runtime).map_err(|e| {
            AppError::localized(
                "usage_script.context_create_failed",
                format!("创建 JS 上下文失败: {e}"),
                format!("Failed to create JS context: {e}"),
            )
        })?;

        let (request_json, config) = context.with(|ctx| {
            // 执行用户代码，获取配置对象
            let config: rquickjs::Object = ctx.eval(script_with_vars).map_err(|e| {
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

            Ok::<_, AppError>((request_json, Persistent::save(&ctx, config)))
        })?;

        let request: RequestConfig = serde_json::from_str(&request_json).map_err(|e| {
            AppError::localized(
                "usage_script.request_format_invalid",
                format!("request 配置格式错误: {e}"),
                format!("Invalid request config format: {e}"),
            )
        })?;
        validate_request_url(&request.url, &base_url, is_custom_template)?;
        let response_data = executor.block_on(send_http_request(&request, timeout_secs))?;

        // Network latency does not consume the extractor's CPU time budget.
        reset_script_deadline(&runtime)?;
        let result = context.with(|ctx| {
            let config = config
                .restore(&ctx)
                .map_err(|e| AppError::Message(e.to_string()))?;
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
        })?;
        validate_result(&result)?;
        Ok(result)
    })
    .await
    .map_err(|e| {
        AppError::localized(
            "usage_script.worker_failed",
            format!("用量脚本工作线程失败: {e}"),
            format!("Usage script worker failed: {e}"),
        )
    })?
}

// 用量脚本允许的最长执行时间（秒）。脚本来自不可信来源（deeplink、同步导入），
// 必须限制其 CPU / 内存 / 栈占用，防止一个恶意/ buggy 脚本挂死整个后端。
const USAGE_SCRIPT_TIMEOUT_SECS: u64 = 5;
// 16 MiB 对仅构造 request 配置 / extractor 的脚本已经足够。
const USAGE_SCRIPT_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// 创建一个受控的 QuickJS Runtime：限制内存与栈，并安装执行时间中断器。
fn create_script_runtime() -> Result<Runtime, AppError> {
    let runtime = Runtime::new().map_err(|e| {
        AppError::localized(
            "usage_script.runtime_create_failed",
            format!("创建 JS 运行时失败: {e}"),
            format!("Failed to create JS runtime: {e}"),
        )
    })?;

    // 内存和栈限制必须在 eval 前设置。
    runtime.set_memory_limit(USAGE_SCRIPT_MEMORY_LIMIT_BYTES);
    // set_max_stack_size 默认 256 KiB 够用，这里显式重申请求它保持一致。
    runtime.set_max_stack_size(256 * 1024);

    reset_script_deadline(&runtime)?;

    Ok(runtime)
}

fn reset_script_deadline(runtime: &Runtime) -> Result<(), AppError> {
    // 时间片中断器：每轮解释器循环检查是否超时，超时则抛出不可捕获的异常。
    let deadline = std::time::Instant::now()
        .checked_add(std::time::Duration::from_secs(USAGE_SCRIPT_TIMEOUT_SECS))
        .ok_or_else(|| {
            AppError::localized(
                "usage_script.invalid_timeout",
                "无法计算脚本执行截止时间",
                "Unable to compute script execution deadline",
            )
        })?;
    runtime.set_interrupt_handler(Some(Box::new(move || std::time::Instant::now() > deadline)));

    Ok(())
}

/// 用量查询响应体最大字节数（2 MiB），防止恶意或异常端点导致 OOM
const MAX_USAGE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

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

/// 流式读取响应文本并强制执行字节数上限
async fn read_response_text_limited(
    mut resp: reqwest::Response,
    max_bytes: usize,
) -> Result<String, AppError> {
    if resp
        .content_length()
        .is_some_and(|len| len > max_bytes as u64)
    {
        return Err(AppError::localized(
            "usage_script.response_too_large",
            format!("响应体过大，超过 {max_bytes} 字节上限"),
            format!("Response body exceeds limit of {max_bytes} bytes"),
        ));
    }

    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| {
        AppError::localized(
            "usage_script.read_response_failed",
            format!("读取响应失败: {e}"),
            format!("Failed to read response: {e}"),
        )
    })? {
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(AppError::localized(
                "usage_script.response_too_large",
                format!("响应体过大，超过 {max_bytes} 字节上限"),
                format!("Response body exceeds limit of {max_bytes} bytes"),
            ));
        }
        buf.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// 发送 HTTP 请求
async fn send_http_request(config: &RequestConfig, timeout_secs: u64) -> Result<String, AppError> {
    // 禁止用量脚本访问 CC Switch 自身的本地代理监听端口，防止回打消耗其他供应商额度
    for (listen_address, listen_port) in crate::proxy::http_client::active_proxy_listeners() {
        crate::proxy::upstream_guard::reject_self_forwarding(
            &config.url,
            &listen_address,
            listen_port,
        )
        .await
        .map_err(|_| {
            AppError::localized(
                "usage_script.self_forwarding_blocked",
                "请求地址不能指向 CC Switch 自身代理端口",
                "Request URL cannot target CC Switch's own proxy listener",
            )
        })?;
    }

    // 使用不跟随重定向的客户端（本机地址直连，其余继承全局代理配置）：
    // 1. 防止 301/302/307/308 重定向绕过 validate_request_url 的同源与 HTTPS 限制（SSRF）；
    // 2. 防止跨主机或降级重定向泄露自定义认证头（如 x-api-key）与 POST 请求体。
    let client = crate::proxy::http_client::no_redirect_for_url(&config.url).map_err(|e| {
        AppError::localized(
            "usage_script.http_client_failed",
            format!("创建 HTTP 客户端失败: {e}"),
            format!("Failed to build HTTP client: {e}"),
        )
    })?;
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
    let text = read_response_text_limited(resp, MAX_USAGE_RESPONSE_BYTES).await?;

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

/// 转义插入到 JS 字符串字面量（双引号、单引号或模板字符串）中的变量内容，
/// 防止凭据或 URL 中包含引号、反斜杠、换行或 `${...}` 时跳出字符串造成 JS 注入。
fn escape_js_string_content(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str(r"\\"),
            '"' => escaped.push_str(r#"\""#),
            '\'' => escaped.push_str(r"\'"),
            '`' => escaped.push_str(r"\`"),
            '$' => escaped.push_str(r"\$"),
            '\n' => escaped.push_str(r"\n"),
            '\r' => escaped.push_str(r"\r"),
            '\0' => escaped.push_str(r"\u0000"),
            '\u{2028}' => escaped.push_str(r"\u2028"),
            '\u{2029}' => escaped.push_str(r"\u2029"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// 构建替换变量后的脚本（单遍扫描 + JS 字符串转义），保持与旧版脚本的兼容性，
/// 同时防止字符串逃逸与跨变量占位符二次展开。
fn build_script_with_vars(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    access_token: Option<&str>,
    user_id: Option<&str>,
) -> String {
    let escaped_api_key = escape_js_string_content(api_key);
    let escaped_base_url = escape_js_string_content(base_url);
    let escaped_access_token = access_token.map(escape_js_string_content);
    let escaped_user_id = user_id.map(escape_js_string_content);

    let mut out = String::with_capacity(script_code.len());
    let mut rest = script_code;
    while let Some(idx) = rest.find("{{") {
        out.push_str(&rest[..idx]);
        rest = &rest[idx..];
        if let Some(after) = rest.strip_prefix("{{apiKey}}") {
            out.push_str(&escaped_api_key);
            rest = after;
        } else if let Some(after) = rest.strip_prefix("{{baseUrl}}") {
            out.push_str(&escaped_base_url);
            rest = after;
        } else if let (Some(token), Some(after)) = (
            escaped_access_token.as_deref(),
            rest.strip_prefix("{{accessToken}}"),
        ) {
            out.push_str(token);
            rest = after;
        } else if let (Some(uid), Some(after)) =
            (escaped_user_id.as_deref(), rest.strip_prefix("{{userId}}"))
        {
            out.push_str(uid);
            rest = after;
        } else {
            out.push_str("{{");
            rest = &rest[2..];
        }
    }
    out.push_str(rest);
    out
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

    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(AppError::localized(
            "usage_script.base_url_scheme_invalid",
            format!("base_url 仅支持 HTTP/HTTPS 协议: {}", parsed_url.scheme()),
            format!("base_url must use HTTP or HTTPS: {}", parsed_url.scheme()),
        ));
    }

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

    if !matches!(parsed_request.scheme(), "http" | "https") {
        return Err(AppError::localized(
            "usage_script.request_scheme_invalid",
            format!(
                "请求 URL 仅支持 HTTP/HTTPS 协议: {}",
                parsed_request.scheme()
            ),
            format!(
                "Request URL must use HTTP or HTTPS: {}",
                parsed_request.scheme()
            ),
        ));
    }

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

        // 核心安全检查：必须与 base_url 同源（相同协议、域名和端口）
        if parsed_request.scheme() != parsed_base.scheme() {
            return Err(AppError::localized(
                "usage_script.request_scheme_mismatch",
                format!(
                    "请求协议 {} 与 base_url 协议 {} 不匹配（必须是同源请求）",
                    parsed_request.scheme(),
                    parsed_base.scheme()
                ),
                format!(
                    "Request scheme {} must match base_url scheme {} (same-origin required)",
                    parsed_request.scheme(),
                    parsed_base.scheme()
                ),
            ));
        }

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

/// 判断 URL 是否指向本机（localhost / loopback，含 IPv4 映射的 IPv6 回环地址）
fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => {
            let trimmed = d.trim_end_matches('.');
            trimmed.eq_ignore_ascii_case("localhost")
                || trimmed.to_ascii_lowercase().ends_with(".localhost")
        }
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn infinite_loop_usage_script_is_interrupted_before_blocking_the_backend() {
        // 用量脚本来自不可信输入（deeplink / 同步导入的 DB 行），必须限制 CPU 时间，
        // 否则 `while(true)` 会挂死执行线程（DoS）。
        let script = r#"
            (function(){
                while (true) { Math.sqrt(Math.random()); }
            })();
            ({ request: { url: "https://example.com", method: "GET" } })
        "#;

        let start = std::time::Instant::now();
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("tokio runtime for test")
            .block_on(async {
                let start = std::time::Instant::now();
                let heartbeat = async {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    assert!(
                        start.elapsed() < std::time::Duration::from_secs(2),
                        "QuickJS blocked the async executor"
                    );
                };
                let (_, result) = tokio::join!(
                    heartbeat,
                    execute_usage_script(
                        script,
                        "sk-test",
                        "https://api.example.com",
                        30,
                        None,
                        None,
                        None,
                    )
                );
                result
            });
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "infinite loop script must be rejected, got: {result:?}"
        );
        // 必须明显短于无限等待；留足余量避免 CI 抖动，但应远小于 30 秒网络超时。
        assert!(
            elapsed < std::time::Duration::from_secs(15),
            "interruption took too long: {elapsed:?}"
        );
    }

    async fn serve_usage_response(
        delay: std::time::Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(move || async move {
                tokio::time::sleep(delay).await;
                axum::Json(serde_json::json!({}))
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, server)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn script_is_evaluated_once_and_http_does_not_use_extractor_budget() {
        let (url, server) = serve_usage_response(std::time::Duration::from_millis(5200)).await;
        // Reading request mutates closure state. Re-evaluating the script loses it.
        let script = format!(
            r#"(() => {{
            let reads = 0;
            return {{
                get request() {{ reads++; return {{ url: "{url}", method: "GET" }}; }},
                extractor: () => {{
                    let sum = 0;
                    for (let i = 0; i < 100000; i++) sum += i;
                    return {{ remaining: reads, isValid: sum > 0 }};
                }}
            }};
        }})()"#
        );
        let result = execute_usage_script(&script, "", &url, 15, None, None, None).await;
        server.abort();
        assert_eq!(
            result.unwrap(),
            serde_json::json!({"remaining": 1, "isValid": true})
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn extractor_timeout_does_not_block_async_timers() {
        let (url, server) = serve_usage_response(std::time::Duration::ZERO).await;
        let script = format!(
            r#"({{
            request: {{ url: "{url}", method: "GET" }},
            extractor: () => {{ while (true) {{ Math.sqrt(Math.random()); }} }}
        }})"#
        );
        let start = std::time::Instant::now();
        let heartbeat = async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            assert!(start.elapsed() < std::time::Duration::from_secs(2));
        };
        let (_, result) = tokio::join!(
            heartbeat,
            execute_usage_script(&script, "", &url, 15, None, None, None)
        );
        server.abort();
        assert!(result.unwrap_err().to_string().contains("extractor"));
        assert!(start.elapsed() < std::time::Duration::from_secs(15));
    }

    #[tokio::test]
    async fn redirect_response_is_not_followed_and_does_not_leak_headers_or_body() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        let victim_hits = Arc::new(AtomicUsize::new(0));
        let victim_hits_clone = Arc::clone(&victim_hits);
        let victim_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let victim_addr = victim_listener.local_addr().unwrap();
        let victim_app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(move || {
                let hits = Arc::clone(&victim_hits_clone);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({ "remaining": 999 }))
                }
            }),
        );
        let victim_server = tokio::spawn(async move {
            let _ = axum::serve(victim_listener, victim_app).await;
        });

        let redirect_target = format!("http://{victim_addr}/v1/messages");
        let attacker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let attacker_addr = attacker_listener.local_addr().unwrap();
        let attacker_url = format!("http://{attacker_addr}");
        let attacker_app = axum::Router::new().route(
            "/usage",
            axum::routing::post(move || {
                let target = redirect_target.clone();
                async move {
                    axum::http::Response::builder()
                        .status(axum::http::StatusCode::TEMPORARY_REDIRECT)
                        .header(axum::http::header::LOCATION, target)
                        .body(axum::body::Body::empty())
                        .unwrap()
                }
            }),
        );
        let attacker_server = tokio::spawn(async move {
            let _ = axum::serve(attacker_listener, attacker_app).await;
        });

        let script = r#"({
            request: {
                url: "{{baseUrl}}/usage",
                method: "POST",
                headers: {
                    "x-api-key": "{{apiKey}}",
                    "Content-Type": "application/json"
                },
                body: "{\"prompt\":\"hi\"}"
            },
            extractor: (response) => ({ isValid: true, remaining: response.remaining })
        })"#;

        let result = execute_usage_script(
            script,
            "sk-secret-token",
            &attacker_url,
            5,
            None,
            None,
            None,
        )
        .await;

        attacker_server.abort();
        victim_server.abort();

        assert!(
            result.is_err(),
            "307 redirect must be rejected instead of followed, got: {result:?}"
        );
        assert_eq!(
            victim_hits.load(Ordering::SeqCst),
            0,
            "redirect target must never receive the forwarded POST request or credentials"
        );
    }

    #[tokio::test]
    async fn oversized_response_body_is_rejected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");
        let app = axum::Router::new().route(
            "/usage",
            axum::routing::get(|| async {
                let padding = "a".repeat(2 * 1024 * 1024 + 4096);
                let payload = format!(r#"{{"pad":"{padding}"}}"#);
                axum::http::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(payload))
                    .unwrap()
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/usage", method: "GET" },
            extractor: () => ({ isValid: true, remaining: 1 })
        })"#;

        let result = execute_usage_script(script, "sk-test", &base_url, 5, None, None, None).await;
        server.abort();

        assert!(
            result.is_err(),
            "oversized response body (>2 MiB) must be rejected, got: {result:?}"
        );
    }

    #[test]
    fn build_script_with_vars_prevents_js_string_breakout_and_cross_expansion() {
        let evil_key = r#"sk-1", url: "https://evil.example/steal", _x: ""#;
        let evil_token = r#"tok' + (1+1) + '"#;
        let evil_uid = "u1\n`${1+1}`{{apiKey}}";
        let script = r#"({
            request: {
                url: "{{baseUrl}}/v1/usage",
                method: "GET",
                headers: {
                    "X-Double": "{{apiKey}}",
                    "X-Single": '{{accessToken}}',
                    "X-Template": `{{userId}}`
                }
            }
        })"#;

        let built = build_script_with_vars(
            script,
            evil_key,
            "https://api.example.com",
            Some(evil_token),
            Some(evil_uid),
        );

        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        let request_json: String = context
            .with(|ctx| {
                let obj: rquickjs::Object = ctx.eval(built).unwrap();
                let req: rquickjs::Object = obj.get("request").unwrap();
                ctx.json_stringify(req).unwrap().unwrap().get()
            })
            .unwrap();
        let parsed: RequestConfig = serde_json::from_str(&request_json).unwrap();
        assert_eq!(parsed.url, "https://api.example.com/v1/usage");
        assert_eq!(parsed.headers.get("X-Double").unwrap(), evil_key);
        assert_eq!(parsed.headers.get("X-Single").unwrap(), evil_token);
        assert_eq!(parsed.headers.get("X-Template").unwrap(), evil_uid);
    }

    #[test]
    fn validate_urls_reject_non_http_schemes_and_loopback_downgrade() {
        assert!(validate_base_url("file://localhost/etc/passwd").is_err());
        assert!(validate_request_url(
            "file://localhost/etc/passwd",
            "https://localhost:8443",
            false
        )
        .is_err());
        assert!(validate_request_url("file:///etc/passwd", "", true).is_err());
        assert!(
            validate_request_url("http://localhost:8443/v1", "https://localhost:8443", false)
                .is_err()
        );
        assert!(is_loopback_host(
            &Url::parse("http://[::ffff:127.0.0.1]:8080/v1").unwrap()
        ));
    }

    #[tokio::test]
    async fn oversized_chunked_response_body_without_content_length_is_rejected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");
        let app = axum::Router::new().route(
            "/usage",
            axum::routing::get(|| async {
                let chunks = (0..65).map(|_| Ok::<_, std::io::Error>(vec![b'a'; 64 * 1024]));
                axum::http::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .body(axum::body::Body::from_stream(futures::stream::iter(chunks)))
                    .unwrap()
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/usage", method: "GET" },
            extractor: () => ({ isValid: true, remaining: 1 })
        })"#;

        let result = execute_usage_script(script, "sk-test", &base_url, 5, None, None, None).await;
        server.abort();

        assert!(
            result.is_err(),
            "chunked response exceeding limit must be rejected, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn request_targeting_active_proxy_listener_is_blocked() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };

        let hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = Arc::clone(&hits);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(move || {
                let hits = Arc::clone(&hits_clone);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({ "remaining": 100 }))
                }
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let _guard =
            crate::proxy::http_client::register_active_proxy_listener("127.0.0.1", addr.port());

        let script = r#"({
            request: { url: "{{baseUrl}}/v1/messages", method: "POST" },
            extractor: (r) => ({ isValid: true, remaining: r.remaining })
        })"#;

        for template_type in [None, Some("custom")] {
            let result =
                execute_usage_script(script, "sk-test", &base_url, 5, None, None, template_type)
                    .await;
            assert!(
                result.is_err(),
                "self-forwarding to active proxy listener must be blocked (template={template_type:?}), got: {result:?}"
            );
        }

        server.abort();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "active proxy listener must never receive usage_script requests"
        );
    }
}
