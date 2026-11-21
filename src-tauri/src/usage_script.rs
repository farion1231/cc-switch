use rquickjs::{Context, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use url::{Host, Url};

use crate::error::AppError;

/// 用量脚本 HTTP 成功响应默认最大字节数（2 MiB，防止超大响应绕过 QuickJS 16 MiB 内存上限）
const DEFAULT_MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// 非 2xx 错误响应最多展示的预览字节数（避免为构造错误摘要而读取超大错误页）
const ERROR_PREVIEW_CHAR_LIMIT: usize = 200;

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
    execute_usage_script_with_max_bytes(
        script_code,
        api_key,
        base_url,
        timeout_secs,
        access_token,
        user_id,
        template_type,
        DEFAULT_MAX_RESPONSE_BYTES,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_usage_script_with_max_bytes(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout_secs: u64,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
    max_response_bytes: usize,
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

    // 3. 在独立作用域中提取 request 配置（确保 Runtime/Context 在 await 前释放）
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

        Ok(runtime)
    }

    let request_config = {
        let runtime = create_script_runtime()?;
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

    // 5. 验证请求 URL（HTTPS 强制 + 同源检查）
    validate_request_url(&request.url, base_url, is_custom_template)?;

    // 6. 发送 HTTP 请求（对响应体流式限额，防止超大响应耗尽宿主内存）
    let response_data = send_http_request(&request, timeout_secs, max_response_bytes).await?;

    // 7. 在独立作用域中执行 extractor（确保 Runtime/Context 在函数结束前释放）
    let result: Value = {
        let runtime = create_script_runtime()?;
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

/// 发送 HTTP 请求（带总超时与响应体大小上限）
async fn send_http_request(
    config: &RequestConfig,
    timeout_secs: u64,
    max_response_bytes: usize,
) -> Result<String, AppError> {
    // 使用全局 HTTP 客户端（本机回环地址直连，其余跟随全局代理配置）
    let client = crate::proxy::http_client::get_for_url(&config.url);
    // 约束超时范围，防止异常配置导致长时间阻塞（最小 2 秒，最大 30 秒）
    let request_timeout = std::time::Duration::from_secs(timeout_secs.clamp(2, 30));
    let deadline = std::time::Instant::now()
        .checked_add(request_timeout)
        .ok_or_else(|| {
            AppError::localized(
                "usage_script.invalid_timeout",
                "无法计算 HTTP 请求超时截止时间",
                "Unable to compute HTTP request timeout deadline",
            )
        })?;

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
    if !status.is_success() {
        let preview = read_bounded_error_preview(resp, max_response_bytes, deadline).await?;
        return Err(AppError::localized(
            "usage_script.http_error",
            format!("HTTP {status} : {preview}"),
            format!("HTTP {status} : {preview}"),
        ));
    }

    read_bounded_response_text(resp, max_response_bytes, deadline).await
}

fn remaining_read_timeout(deadline: std::time::Instant) -> Result<std::time::Duration, AppError> {
    let now = std::time::Instant::now();
    if now >= deadline {
        return Err(AppError::localized(
            "usage_script.read_response_failed",
            "读取响应超时".to_string(),
            "Timed out while reading response".to_string(),
        ));
    }
    Ok(deadline.saturating_duration_since(now))
}

async fn next_chunk_with_deadline(
    resp: &mut reqwest::Response,
    deadline: std::time::Instant,
) -> Result<Option<bytes::Bytes>, AppError> {
    let read_timeout = remaining_read_timeout(deadline)?;
    tokio::time::timeout(read_timeout, resp.chunk())
        .await
        .map_err(|_| {
            AppError::localized(
                "usage_script.read_response_failed",
                "读取响应超时".to_string(),
                "Timed out while reading response".to_string(),
            )
        })?
        .map_err(|e| {
            AppError::localized(
                "usage_script.read_response_failed",
                format!("读取响应失败: {e}"),
                format!("Failed to read response: {e}"),
            )
        })
}

fn response_too_large_error(max_bytes: usize, declared_len: Option<u64>) -> AppError {
    match declared_len {
        Some(len) => AppError::localized(
            "usage_script.response_too_large",
            format!("用量接口响应体过大（声明 {len} 字节，上限 {max_bytes} 字节）"),
            format!(
                "Usage response body exceeds limit (declared {len} bytes, max {max_bytes} bytes)"
            ),
        ),
        None => AppError::localized(
            "usage_script.response_too_large",
            format!("用量接口响应体超过大小上限（{max_bytes} 字节）"),
            format!("Usage response body exceeds size limit ({max_bytes} bytes)"),
        ),
    }
}

async fn read_bounded_response_text(
    mut resp: reqwest::Response,
    max_response_bytes: usize,
    deadline: std::time::Instant,
) -> Result<String, AppError> {
    if let Some(declared_len) = resp.content_length() {
        if declared_len > max_response_bytes as u64 {
            return Err(response_too_large_error(
                max_response_bytes,
                Some(declared_len),
            ));
        }
    }
    for header_val in resp.headers().get_all(reqwest::header::CONTENT_LENGTH) {
        if let Ok(raw) = header_val.to_str() {
            for part in raw.split(',') {
                if let Ok(parsed_len) = part.trim().parse::<u128>() {
                    if parsed_len > max_response_bytes as u128 {
                        let reported = u64::try_from(parsed_len).ok();
                        return Err(response_too_large_error(max_response_bytes, reported));
                    }
                }
            }
        }
    }

    let initial_cap = resp
        .content_length()
        .and_then(|len| usize::try_from(len).ok())
        .unwrap_or(0)
        .min(max_response_bytes)
        .min(64 * 1024);
    let mut buf = Vec::with_capacity(initial_cap);

    while let Some(chunk) = next_chunk_with_deadline(&mut resp, deadline).await? {
        if buf.len().saturating_add(chunk.len()) > max_response_bytes {
            return Err(response_too_large_error(max_response_bytes, None));
        }
        buf.extend_from_slice(&chunk);
    }

    let body = buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&buf);
    Ok(String::from_utf8_lossy(body).into_owned())
}

async fn read_bounded_error_preview(
    mut resp: reqwest::Response,
    max_response_bytes: usize,
    deadline: std::time::Instant,
) -> Result<String, AppError> {
    let preview_byte_budget = max_response_bytes.min(ERROR_PREVIEW_CHAR_LIMIT + 4);
    let declared_exceeds_budget = resp
        .content_length()
        .is_some_and(|len| len > preview_byte_budget as u64);
    let mut buf = Vec::with_capacity(preview_byte_budget);
    let mut truncated = declared_exceeds_budget;

    if preview_byte_budget > 0 {
        while buf.len() < preview_byte_budget {
            match next_chunk_with_deadline(&mut resp, deadline).await {
                Ok(Some(chunk)) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    let remaining = preview_byte_budget - buf.len();
                    let take = chunk.len().min(remaining);
                    buf.extend_from_slice(&chunk[..take]);
                    if take < chunk.len() {
                        truncated = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    if buf.is_empty() {
                        return Err(err);
                    }
                    truncated = true;
                    break;
                }
            }
        }
    }
    drop(resp);

    let body = buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&buf);
    let text = match std::str::from_utf8(body) {
        Ok(valid) => valid.to_string(),
        Err(err) if err.error_len().is_none() => {
            String::from_utf8_lossy(&body[..err.valid_up_to()]).into_owned()
        }
        Err(_) => String::from_utf8_lossy(body).into_owned(),
    };

    if text.len() > ERROR_PREVIEW_CHAR_LIMIT {
        let mut safe_cut = ERROR_PREVIEW_CHAR_LIMIT;
        while !text.is_char_boundary(safe_cut) {
            safe_cut = safe_cut.saturating_sub(1);
        }
        Ok(format!("{}...", &text[..safe_cut]))
    } else if truncated && !text.is_empty() {
        Ok(format!("{text}..."))
    } else {
        Ok(text)
    }
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
            .block_on(execute_usage_script(
                script,
                "sk-test",
                "https://api.example.com",
                30,
                None,
                None,
                None,
            ));
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

    async fn read_raw_http_head(stream: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt;

        let mut buf = Vec::with_capacity(1024);
        let mut tmp = [0u8; 512];
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut tmp))
                .await
            {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                Ok(Ok(n)) => {
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= 8192 {
                        break;
                    }
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn assert_localized_key(err: &AppError, expected_key: &str) {
        match err {
            AppError::Localized { key, .. } => assert_eq!(
                *key, expected_key,
                "expected localized error key {expected_key}, got {err:?}"
            ),
            other => panic!("expected AppError::Localized({expected_key}), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn response_content_length_exceeding_budget_fails_fast() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        use tokio::io::AsyncWriteExt;

        const SMALL_BUDGET: usize = 512;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let body_chunks_sent = Arc::new(AtomicUsize::new(0));
        let sent_clone = body_chunks_sent.clone();

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_raw_http_head(&mut stream).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = stream.flush().await;
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                if stream.write_all(&[b'x'; 1024]).await.is_ok() {
                    sent_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/v1/usage", method: "GET" },
            extractor: function(data) { return { remaining: data.remaining }; }
        })"#;
        let base_url = format!("http://{addr}");
        let err = execute_usage_script_with_max_bytes(
            script,
            "sk-test",
            &base_url,
            5,
            None,
            None,
            None,
            SMALL_BUDGET,
        )
        .await
        .expect_err("Content-Length > budget must fail immediately");
        assert_localized_key(&err, "usage_script.response_too_large");
        assert_eq!(body_chunks_sent.load(Ordering::SeqCst), 0);

        server_task.abort();
    }

    #[tokio::test]
    async fn chunked_response_exceeding_budget_stops_reading_and_errors() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        use tokio::io::AsyncWriteExt;

        const SMALL_BUDGET: usize = 512;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let late_chunks_sent = Arc::new(AtomicUsize::new(0));
        let late_sent_clone = late_chunks_sent.clone();

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_raw_http_head(&mut stream).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let chunk_payload = "a".repeat(200);
                let wire_chunk = format!("{:x}\r\n{}\r\n", chunk_payload.len(), chunk_payload);
                // 先发 4 个分块（共 800 字节 > 512 字节预算），故意不发结束块 0\r\n\r\n：
                // 若客户端未按分块立即截停，而是等整段流结束，这里就会一直挂起到超时。
                for _ in 0..4 {
                    if stream.write_all(wire_chunk.as_bytes()).await.is_err() {
                        return;
                    }
                    let _ = stream.flush().await;
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                if stream.write_all(wire_chunk.as_bytes()).await.is_ok() {
                    late_sent_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/v1/usage", method: "GET" },
            extractor: function(data) { return { remaining: data.remaining }; }
        })"#;
        let base_url = format!("http://{addr}");
        let err = execute_usage_script_with_max_bytes(
            script,
            "sk-test",
            &base_url,
            5,
            None,
            None,
            None,
            SMALL_BUDGET,
        )
        .await
        .expect_err("chunked response exceeding budget must abort immediately");
        assert_localized_key(&err, "usage_script.response_too_large");
        assert_eq!(
            late_chunks_sent.load(Ordering::SeqCst),
            0,
            "reader must stop immediately upon exceeding budget without waiting for stream EOF"
        );

        server_task.abort();
    }

    #[tokio::test]
    async fn non_200_error_response_with_huge_body_is_safely_bounded() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        use tokio::io::AsyncWriteExt;

        const SMALL_BUDGET: usize = 512;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        let late_chunks_sent = Arc::new(AtomicUsize::new(0));
        let late_sent_clone = late_chunks_sent.clone();

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_raw_http_head(&mut stream).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let chunk_payload = "E".repeat(1024);
                let wire_chunk = format!("{:x}\r\n{}\r\n", chunk_payload.len(), chunk_payload);
                // 先发 2 个 1 KiB 错误分块（已超过预览上限），随后保持连接不发结束块：
                // 受限错误读取器应在拿到前 ~200 字节预览后立即断开并返回 http_error，不等待后续海量错误体。
                for _ in 0..2 {
                    if stream.write_all(wire_chunk.as_bytes()).await.is_err() {
                        return;
                    }
                    let _ = stream.flush().await;
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                if stream.write_all(wire_chunk.as_bytes()).await.is_ok() {
                    late_sent_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/v1/usage", method: "GET" },
            extractor: function(data) { return { remaining: data.remaining }; }
        })"#;
        let base_url = format!("http://{addr}");
        let err = execute_usage_script_with_max_bytes(
            script,
            "sk-test",
            &base_url,
            5,
            None,
            None,
            None,
            SMALL_BUDGET,
        )
        .await
        .expect_err("non-200 response must return http_error");
        assert_localized_key(&err, "usage_script.http_error");
        match &err {
            AppError::Localized { zh, en, .. } => {
                assert!(zh.contains("HTTP 502"), "zh={zh}");
                assert!(en.contains("HTTP 502"), "en={en}");
                assert!(zh.ends_with("..."), "zh={zh}");
                assert!(en.ends_with("..."), "en={en}");
                assert!(
                    zh.len() < 260,
                    "zh preview must be bounded: len={}",
                    zh.len()
                );
                assert!(
                    en.len() < 260,
                    "en preview must be bounded: len={}",
                    en.len()
                );
            }
            other => panic!("expected Localized error, got {other:?}"),
        }
        assert_eq!(
            late_chunks_sent.load(Ordering::SeqCst),
            0,
            "non-200 reader must stop after reading bounded preview without waiting for stream EOF"
        );

        server_task.abort();
    }

    #[tokio::test]
    async fn normal_small_json_response_within_budget_succeeds() {
        use tokio::io::AsyncWriteExt;

        const SMALL_BUDGET: usize = 512;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_raw_http_head(&mut stream).await;
                let body = r#"{"remaining":19.25,"total":50,"used":30.75,"unit":"USD"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/v1/usage", method: "GET" },
            extractor: function(data) {
                return {
                    remaining: data.remaining,
                    total: data.total,
                    used: data.used,
                    unit: data.unit
                };
            }
        })"#;
        let base_url = format!("http://{addr}");
        let value = execute_usage_script_with_max_bytes(
            script,
            "sk-test",
            &base_url,
            5,
            None,
            None,
            None,
            SMALL_BUDGET,
        )
        .await
        .expect("small JSON within budget should succeed");

        assert_eq!(value["remaining"].as_f64(), Some(19.25));
        assert_eq!(value["total"].as_f64(), Some(50.0));
        assert_eq!(value["used"].as_f64(), Some(30.75));
        assert_eq!(value["unit"].as_str(), Some("USD"));

        server_task.abort();
    }

    #[tokio::test]
    async fn utf8_bom_prefixed_json_response_parses_successfully() {
        use tokio::io::AsyncWriteExt;

        const SMALL_BUDGET: usize = 512;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("addr");

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_raw_http_head(&mut stream).await;
                let body = b"\xEF\xBB\xBF{\"remaining\":7}";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body).await;
            }
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = read_raw_http_head(&mut stream).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let part1 = b"\xEF";
                let part2 = b"\xBB\xBF{\"remaining\":9.5}";
                let chunk1 = format!("{:x}\r\n", part1.len());
                let chunk2 = format!("{:x}\r\n", part2.len());
                let _ = stream.write_all(chunk1.as_bytes()).await;
                let _ = stream.write_all(part1).await;
                let _ = stream.write_all(b"\r\n").await;
                let _ = stream.flush().await;
                let _ = stream.write_all(chunk2.as_bytes()).await;
                let _ = stream.write_all(part2).await;
                let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
            }
        });

        let script = r#"({
            request: { url: "{{baseUrl}}/v1/usage", method: "GET" },
            extractor: function(data) {
                return { remaining: data.remaining };
            }
        })"#;
        let base_url = format!("http://{addr}");

        let first = execute_usage_script_with_max_bytes(
            script,
            "sk-test",
            &base_url,
            5,
            None,
            None,
            None,
            SMALL_BUDGET,
        )
        .await
        .expect("UTF-8 BOM JSON response should parse");
        assert_eq!(first["remaining"].as_f64(), Some(7.0));

        let second = execute_usage_script_with_max_bytes(
            script,
            "sk-test",
            &base_url,
            5,
            None,
            None,
            None,
            SMALL_BUDGET,
        )
        .await
        .expect("chunk-split UTF-8 BOM JSON response should parse");
        assert_eq!(second["remaining"].as_f64(), Some(9.5));

        server_task.abort();
    }
}
