use rquickjs::{Context, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use url::{Host, Url};

use crate::error::AppError;

/// 用量脚本沙箱里宿主进程的本地时区偏移（分钟，遵循 JS `getTimezoneOffset()`
/// 的语义：UTC − local，正数代表 UTC 西侧）。
///
/// 见 #7751：rquickjs-sys 0.8.1 在 Windows 上的 `quickjs.c` 把
/// `TIME_ZONE_INFORMATION.Bias`（单位本身就是分钟）又除以 60，于是
/// `getTimezoneOffset()` 在 UTC+8 上返回 `-8` 而非 `-480`，本地时间字段随之
/// 整体偏离一个数量级。这里把 chrono（自带时区数据库、DST 感知）算出的真实偏移
/// 注入脚本运行时，覆盖引擎默认实现。
fn local_timezone_offset_minutes() -> i32 {
    // 直接从 chrono 的偏移取 `local_minus_utc`（秒），避免跨时区相减带来的类型
    // 推导问题；JS 期望的是「UTC − local」分钟数，所以取负。
    let minutes = -chrono::Local::now().offset().local_minus_utc() / 60;
    // 限制到 JS 规范允许的 [-720, 840] 区间，避免上游时间戳异常导致巨值。
    minutes.clamp(-720, 840)
}

/// 在用量脚本之前注入的时区垫片，把 `Date.prototype.getTimezoneOffset` 与
/// 本地读取类方法重写到宿主时间上。构造（`new Date(y, m, ...)`）与 setter 不在
/// 本次范围（见 #7751）。
///
/// `offset_minutes` 是 JS 语义的偏移：UTC − local，单位分钟。
/// 本地毫秒数 = UTC 毫秒数 − `offset_minutes` * 60_000。
fn timezone_shim(offset_minutes: i32) -> String {
    format!(
        r#"
        (function() {{
            var __CC_TZ_OFFSET_MIN__ = {offset_minutes};
            var __CC_TZ_OFFSET_MS__ = __CC_TZ_OFFSET_MIN__ * 60000;
            function __cc_local_ms(utc_ms) {{
                return utc_ms - __CC_TZ_OFFSET_MS__;
            }}
            function __cc_local_date(self) {{
                var t = (self && typeof self.getTime === 'function')
                    ? self.getTime()
                    : Date.now();
                return new Date(__cc_local_ms(t));
            }}
            Date.prototype.getTimezoneOffset = function() {{
                return __CC_TZ_OFFSET_MIN__;
            }};
            Date.prototype.getFullYear = function() {{
                return __cc_local_date(this).getUTCFullYear();
            }};
            Date.prototype.getMonth = function() {{
                return __cc_local_date(this).getUTCMonth();
            }};
            Date.prototype.getDate = function() {{
                return __cc_local_date(this).getUTCDate();
            }};
            Date.prototype.getDay = function() {{
                return __cc_local_date(this).getUTCDay();
            }};
            Date.prototype.getHours = function() {{
                return __cc_local_date(this).getUTCHours();
            }};
            Date.prototype.getMinutes = function() {{
                return __cc_local_date(this).getUTCMinutes();
            }};
            Date.prototype.getSeconds = function() {{
                return __cc_local_date(this).getUTCSeconds();
            }};
            Date.prototype.getMilliseconds = function() {{
                return __cc_local_date(this).getUTCMilliseconds();
            }};
        }})();
        "#
    )
}

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

        let timezone_shim = timezone_shim(local_timezone_offset_minutes());

        context.with(|ctx| {
            // 先注入时区垫片覆盖 rquickjs-sys 0.8.1 在 Windows 上把偏移单位算错的
            // bug（见 #7751）；垫片对其它平台也是无害的覆盖。
            ctx.eval::<(), _>(timezone_shim.as_str()).map_err(|e| {
                AppError::localized(
                    "usage_script.timezone_shim_failed",
                    format!("注入时区垫片失败: {e}"),
                    format!("Failed to install timezone shim: {e}"),
                )
            })?;

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

    // 6. 发送 HTTP 请求
    let response_data = send_http_request(&request, timeout_secs).await?;

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

        let timezone_shim = timezone_shim(local_timezone_offset_minutes());

        context.with(|ctx| {
            // 与 request 阶段相同：先注入时区垫片，再 eval 用户脚本。
            ctx.eval::<(), _>(timezone_shim.as_str()).map_err(|e| {
                AppError::localized(
                    "usage_script.timezone_shim_failed",
                    format!("注入时区垫片失败: {e}"),
                    format!("Failed to install timezone shim: {e}"),
                )
            })?;

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

    /// `#7751`：rquickjs-sys 0.8.1 在 Windows 上的 `quickjs.c` 把
    /// `TIME_ZONE_INFORMATION.Bias`（单位本身就是分钟）又除以 60，
    /// `getTimezoneOffset()` 在 UTC+8 上返回 `-8` 而非 `-480`，本地时间
    /// 字段因此整体偏离一个数量级。垫片按宿主 chrono 的真实偏移覆盖引擎
    /// 默认实现，本测试断言垫片在已知偏移下能把一个 UTC 时间戳映射到期望
    /// 的本地字段。
    #[test]
    fn timezone_shim_aligns_local_fields_with_host_offset() {
        // 1790680967498 = 2026-09-29T11:22:47.498Z（与 #7751 探针同时间戳）
        let utc_ms: i64 = 1_790_680_967_498;
        for (offset_minutes, expected) in [
            // UTC+8：本地应该是 19:22:47.498
            (-480i32, ["2026", "8", "29", "19", "22", "47", "498"]),
            // UTC：本地就是 UTC 本身
            (0, ["2026", "8", "29", "11", "22", "47", "498"]),
            // UTC-5（EST，无 DST）：本地应该是 06:22:47.498
            (300, ["2026", "8", "29", "6", "22", "47", "498"]),
        ] {
            let program = format!(
                r#"
                {shim}
                (function() {{
                    var d = new Date({utc});
                    return [
                        String(d.getFullYear()),
                        String(d.getMonth()),
                        String(d.getDate()),
                        String(d.getHours()),
                        String(d.getMinutes()),
                        String(d.getSeconds()),
                        String(d.getMilliseconds())
                    ].join(',');
                }})()
                "#,
                shim = timezone_shim(offset_minutes),
                utc = utc_ms
            );

            let runtime = Runtime::new().expect("runtime");
            runtime.set_memory_limit(16 * 1024 * 1024);
            runtime.set_max_stack_size(256 * 1024);
            let context = Context::full(&runtime).expect("context");
            let raw: String =
                context.with(|ctx| ctx.eval::<String, _>(program.as_str()).expect("eval"));

            assert_eq!(
                raw,
                expected.join(","),
                "offset = {offset_minutes}",
                offset_minutes = offset_minutes,
            );
        }
    }

    /// `#7751`：垫片要原样返回 JS 规范要求的 `getTimezoneOffset()` 偏移
    /// （单位分钟），不能像坏掉的引擎那样返回一个数量级错误的 `-8`。
    #[test]
    fn timezone_shim_reports_offset_in_minutes() {
        for offset_minutes in [-720i32, -480, -60, 0, 60, 300, 840] {
            let program = format!(
                r#"
                {shim}
                (function() {{
                    return String(new Date(0).getTimezoneOffset());
                }})()
                "#,
                shim = timezone_shim(offset_minutes),
            );

            let runtime = Runtime::new().expect("runtime");
            runtime.set_memory_limit(16 * 1024 * 1024);
            runtime.set_max_stack_size(256 * 1024);
            let context = Context::full(&runtime).expect("context");
            let raw: i32 = context.with(|ctx| {
                ctx.eval::<String, _>(program.as_str())
                    .expect("eval")
                    .parse()
                    .expect("parse")
            });

            assert_eq!(
                raw,
                offset_minutes,
                "offset_minutes = {offset_minutes}",
                offset_minutes = offset_minutes,
            );
        }
    }

    /// 宿主偏移必须落在 JS 规范允许的 [-720, 840] 区间，否则垫片会把本地
    /// 字段算出错误结果。
    #[test]
    fn local_timezone_offset_minutes_is_within_safe_range() {
        let offset = local_timezone_offset_minutes();
        assert!(
            (-720..=840).contains(&offset),
            "offset = {offset} out of JS-allowed range [-720, 840]",
            offset = offset,
        );
    }
}
