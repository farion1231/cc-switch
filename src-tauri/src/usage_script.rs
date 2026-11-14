use rquickjs::prelude::Func;
use rquickjs::{Context, Function, Runtime};
use serde_json::Value;
use std::collections::HashMap;
use url::{Host, Url};

use crate::error::AppError;

// 用量脚本允许的最长执行时间（秒）。脚本来自不可信来源（deeplink、同步导入），
// 必须限制其 CPU / 内存 / 栈占用，防止一个恶意/ buggy 脚本挂死整个后端。
const USAGE_SCRIPT_TIMEOUT_SECS: u64 = 5;
// 16 MiB 对仅构造 request 配置 / extractor 的脚本已经足够。
const USAGE_SCRIPT_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

// JS prelude that repairs Date local-time reading AND writing on top of the
// host-provided offset. Needed because the QuickJS build vendored by
// rquickjs-sys 0.8.1 computes the Windows offset in the wrong unit:
// TIME_ZONE_INFORMATION.Bias is already in minutes, but quickjs divides it by
// 60, so getTimezoneOffset() returns hours (e.g. -8 for UTC+8 instead of
// -480) and every local-time getter and setter shifts the timestamp by
// minutes instead of hours. See issue #7751.
//
// Reads and writes must share the same conversion, otherwise previously
// self-consistent combinations (e.g. d.setDate(d.getDate() + 1)) would break
// once only the getters were corrected. The prelude therefore rewrites
// getTimezoneOffset(), the local getters, the local setters, the multi-arg
// constructor and spec-local ISO parsing on top of one host callback, then
// hides the callback from user scripts.
const TIMEZONE_SHIM_PRELUDE: &str = r#"
(() => {
  const hostOffsetMinutes = globalThis.__hostUtcOffsetMinutes;
  delete globalThis.__hostUtcOffsetMinutes;
  if (typeof hostOffsetMinutes !== "function") return;
  const proto = Date.prototype;
  // Local wall-clock epoch value: local = UTC + offsetMinutes * 60000.
  const localEpochMs = function (utcMs) { return utcMs + hostOffsetMinutes(utcMs) * 60000; };
  // Inverse map: the UTC instant whose local wall clock equals localMs.
  // Iterated to a fixed point of t = localMs - offset(t): exact for fixed
  // offsets, deterministic for DST-ambiguous local times.
  const utcEpochMs = function (localMs) {
    let t = localMs - hostOffsetMinutes(localMs) * 60000;
    for (let i = 0; i < 2; i++) {
      const next = localMs - hostOffsetMinutes(t) * 60000;
      if (next === t) break;
      t = next;
    }
    return t;
  };
  // Spec: getTimezoneOffset() = UTC - local, in minutes.
  proto.getTimezoneOffset = function () {
    const utcMs = this.getTime();
    return isFinite(utcMs) ? -hostOffsetMinutes(utcMs) : NaN;
  };
  const utcGetterNames = {
    getFullYear: "getUTCFullYear",
    getMonth: "getUTCMonth",
    getDate: "getUTCDate",
    getDay: "getUTCDay",
    getHours: "getUTCHours",
    getMinutes: "getUTCMinutes",
    getSeconds: "getUTCSeconds",
    getMilliseconds: "getUTCMilliseconds",
  };
  for (const localName of Object.keys(utcGetterNames)) {
    const utcName = utcGetterNames[localName];
    proto[localName] = function () {
      return new Date(localEpochMs(this.getTime()))[utcName]();
    };
  }
  // Annex B: getYear() = getFullYear() - 1900.
  proto.getYear = function () {
    const y = this.getFullYear();
    return isNaN(y) ? NaN : y - 1900;
  };
  // Unique sentinel marking "argument absent, keep the current local field";
  // an explicitly passed undefined is applied and poisons the result like
  // ToNumber(undefined) would in a spec engine.
  const KEEP = {};
  // Local-field writes. Fields are ordered [year, month, date, hours,
  // minutes, seconds, milliseconds]; KEEP entries keep the current local
  // value, while explicitly passed arguments (even undefined or NaN) are
  // applied, poisoning the result exactly like a spec engine.
  const setLocalFields = function (self, fields) {
    const t = self.getTime();
    if (!isFinite(t)) { self.setTime(NaN); return NaN; }
    const local = new Date(localEpochMs(t));
    const parts = [
      local.getUTCFullYear(), local.getUTCMonth(), local.getUTCDate(),
      local.getUTCHours(), local.getUTCMinutes(), local.getUTCSeconds(),
      local.getUTCMilliseconds(),
    ];
    for (let i = 0; i < 7; i++) {
      if (fields[i] !== KEEP) parts[i] = fields[i];
    }
    // UTC setters normalize overflow (day 0, month 13, hour 25 ...) without
    // the 0-99 year remapping that Date.UTC would apply.
    const scratch = new Date(0);
    scratch.setUTCFullYear(parts[0], parts[1], parts[2]);
    scratch.setUTCHours(parts[3], parts[4], parts[5], parts[6]);
    return self.setTime(utcEpochMs(scratch.getTime()));
  };
  proto.setDate = function (v) {
    return setLocalFields(this, [KEEP, KEEP, v, KEEP, KEEP, KEEP, KEEP]);
  };
  proto.setFullYear = function (y, m, d) {
    const fields = [y, KEEP, KEEP, KEEP, KEEP, KEEP, KEEP];
    if (arguments.length > 1) fields[1] = m;
    if (arguments.length > 2) fields[2] = d;
    return setLocalFields(this, fields);
  };
  proto.setHours = function (h, m, s, ms) {
    const fields = [KEEP, KEEP, KEEP, h, KEEP, KEEP, KEEP];
    if (arguments.length > 1) fields[4] = m;
    if (arguments.length > 2) fields[5] = s;
    if (arguments.length > 3) fields[6] = ms;
    return setLocalFields(this, fields);
  };
  proto.setMilliseconds = function (ms) {
    return setLocalFields(this, [KEEP, KEEP, KEEP, KEEP, KEEP, KEEP, ms]);
  };
  proto.setMinutes = function (m, s, ms) {
    const fields = [KEEP, KEEP, KEEP, KEEP, m, KEEP, KEEP];
    if (arguments.length > 1) fields[5] = s;
    if (arguments.length > 2) fields[6] = ms;
    return setLocalFields(this, fields);
  };
  proto.setMonth = function (m, d) {
    const fields = [KEEP, m, KEEP, KEEP, KEEP, KEEP, KEEP];
    if (arguments.length > 1) fields[2] = d;
    return setLocalFields(this, fields);
  };
  proto.setSeconds = function (s, ms) {
    const fields = [KEEP, KEEP, KEEP, KEEP, KEEP, s, KEEP];
    if (arguments.length > 1) fields[6] = ms;
    return setLocalFields(this, fields);
  };
  // Annex B: setYear remaps 0-99 to 1900 + y, unlike setFullYear.
  proto.setYear = function (y) {
    let yr = y;
    if (!isNaN(yr) && yr >= 0 && yr <= 99) yr += 1900;
    return this.setFullYear(yr);
  };
  // Replace the global constructor so multi-argument calls and spec-local
  // ISO strings share the same conversion as getters and setters.
  const NativeDate = Date;
  // Spec date-time strings without an offset are interpreted as local time;
  // date-only forms and explicit offsets are UTC and stay native.
  const isoLocalRe = /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2})(?::(\d{1,2})(?:\.(\d+))?)?$/;
  const parseIsoLocal = function (s) {
    const m = isoLocalRe.exec(s);
    if (!m) return undefined;
    const month = +m[2], day = +m[3], hour = +m[4], minute = +m[5];
    const second = m[6] === undefined ? 0 : +m[6];
    if (month < 1 || month > 12 || day < 1 || day > 31) return NaN;
    if (hour > 23 || minute > 59 || second > 59) return NaN;
    let ms = 0;
    if (m[7] !== undefined) ms = +(m[7] + "000").slice(0, 3);
    return utcEpochMs(Date.UTC(+m[1], month - 1, day, hour, minute, second, ms));
  };
  const ShimDate = function (a, b, c, d, e, f, g) {
    if (new.target) {
      const n = arguments.length;
      if (n === 0) return new NativeDate();
      if (n === 1) {
        if (typeof a === "string") {
          const local = parseIsoLocal(a);
          if (local !== undefined) return new NativeDate(local);
        }
        return new NativeDate(a);
      }
      // Two or more arguments are local fields; Date.UTC supplies the 0-99
      // year remap and overflow normalization, and apply() keeps absent
      // trailing fields absent so Date.UTC applies its own defaults.
      return new NativeDate(utcEpochMs(Date.UTC.apply(null, arguments)));
    }
    return NativeDate();
  };
  ShimDate.prototype = NativeDate.prototype;
  Object.setPrototypeOf(ShimDate, NativeDate);
  ShimDate.parse = function (s) {
    if (typeof s === "string") {
      const local = parseIsoLocal(s);
      if (local !== undefined) return local;
    }
    return NativeDate.parse(s);
  };
  NativeDate.prototype.constructor = ShimDate;
  globalThis.Date = ShimDate;
})();
"#;

// Local UTC offset in minutes (local minus UTC) for a timestamp in
// milliseconds. DST-aware via the platform timezone database through chrono.
fn local_utc_offset_minutes(timestamp_ms: f64) -> f64 {
    use chrono::TimeZone;
    if !timestamp_ms.is_finite() {
        return f64::NAN;
    }
    let timestamp_ms = timestamp_ms as i64; // saturates on overflow
    let Some(utc) = chrono::Utc.timestamp_millis_opt(timestamp_ms).single() else {
        return 0.0;
    };
    let offset_secs = chrono::Local
        .offset_from_utc_datetime(&utc.naive_utc())
        .local_minus_utc();
    offset_secs as f64 / 60.0
}

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

/// Create the script context and install the Date timezone shim so that user
/// scripts observe spec-correct local time on every platform.
fn create_script_context(runtime: &Runtime) -> Result<Context, AppError> {
    create_script_context_with_offset(runtime, local_utc_offset_minutes)
}

/// Same as [`create_script_context`] but with an injectable offset callback,
/// used by tests to pin a fixed offset instead of the machine time zone.
fn create_script_context_with_offset(
    runtime: &Runtime,
    utc_offset_minutes: fn(f64) -> f64,
) -> Result<Context, AppError> {
    let context = Context::full(runtime).map_err(|e| {
        AppError::localized(
            "usage_script.context_create_failed",
            format!("创建 JS 上下文失败: {e}"),
            format!("Failed to create JS context: {e}"),
        )
    })?;

    context.with(|ctx| -> Result<(), AppError> {
        ctx.globals()
            .set("__hostUtcOffsetMinutes", Func::from(utc_offset_minutes))
            .map_err(|e| {
                AppError::localized(
                    "usage_script.timezone_shim_failed",
                    format!("安装时区垫片失败: {e}"),
                    format!("Failed to install timezone shim: {e}"),
                )
            })?;
        let _: rquickjs::Value = ctx.eval(TIMEZONE_SHIM_PRELUDE).map_err(|e| {
            AppError::localized(
                "usage_script.timezone_shim_failed",
                format!("安装时区垫片失败: {e}"),
                format!("Failed to install timezone shim: {e}"),
            )
        })?;
        Ok(())
    })?;

    Ok(context)
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
    let request_config = {
        let runtime = create_script_runtime()?;
        let context = create_script_context(&runtime)?;

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

    // 6. 发送 HTTP 请求
    let response_data = send_http_request(&request, timeout_secs).await?;

    // 7. 在独立作用域中执行 extractor（确保 Runtime/Context 在函数结束前释放）
    let result: Value = {
        let runtime = create_script_runtime()?;
        let context = create_script_context(&runtime)?;

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
    fn date_local_time_apis_use_correct_units() {
        // Repro instant from issue #7751: 2026-09-29T11:22:47.498Z.
        const FIXED_MS: i64 = 1_790_680_967_498;

        let script = format!(
            r#"
            (function () {{
                var d = new Date({FIXED_MS});
                var asUtc = Date.UTC(
                    d.getFullYear(), d.getMonth(), d.getDate(),
                    d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()
                );
                return JSON.stringify({{
                    tzOffset: d.getTimezoneOffset(),
                    localShiftMinutes: Math.round((asUtc - {FIXED_MS}) / 60000),
                    hours: d.getHours()
                }});
            }})()
            "#
        );

        let runtime = create_script_runtime().expect("runtime");
        let context = create_script_context(&runtime).expect("context");
        let json: String = context
            .with(|ctx| -> Result<String, rquickjs::Error> { ctx.eval(script.as_str()) })
            .expect("eval probe failed");
        let probe: Value = serde_json::from_str(&json).expect("probe json");

        // Host-side ground truth: chrono is DST-aware and unit-correct on every platform.
        use chrono::{TimeZone, Timelike};
        let utc = chrono::Utc
            .timestamp_millis_opt(FIXED_MS)
            .single()
            .expect("valid instant");
        let offset_secs = chrono::Local
            .offset_from_utc_datetime(&utc.naive_utc())
            .local_minus_utc();
        let offset_minutes = offset_secs / 60;
        let expected_hours =
            (utc.naive_utc() + chrono::Duration::seconds(offset_secs as i64)).hour();

        assert_eq!(
            probe["tzOffset"],
            serde_json::json!(-offset_minutes),
            "getTimezoneOffset() must return minutes (UTC minus local)"
        );
        assert_eq!(
            probe["localShiftMinutes"],
            serde_json::json!(offset_minutes),
            "local-time fields must shift by the full offset in minutes"
        );
        assert_eq!(
            probe["hours"],
            serde_json::json!(expected_hours),
            "getHours() must return the local hour"
        );
    }

    // Fixed-offset host callback (+480 min, i.e. UTC+8) so the sandboxed Date
    // is fully deterministic and independent of the machine time zone.
    fn fixed_offset_480(_timestamp_ms: f64) -> f64 {
        480.0
    }

    #[test]
    fn local_read_write_apis_share_the_correct_offset() {
        // Differential battery: with the host offset pinned to +480 minutes,
        // the sandboxed Date must behave exactly like a spec-compliant Date in
        // a UTC+8 zone. Reference values come from pure UTC arithmetic (shift
        // by the offset, mutate UTC fields, shift back), which is exact for
        // fixed-offset zones, so this test never depends on the machine TZ.
        let script = r#"
            (function () {
                var OFF = 480 * 60000;
                var out = [];
                function ok(name, cond) { out.push(name + "=" + (cond ? "1" : "0")); }
                function ref(t0, mutate) {
                    var r = new Date(t0 + OFF);
                    mutate(r);
                    return r.getTime() - OFF;
                }
                var instants = [
                    Date.UTC(2026, 8, 29, 20, 22, 47, 498),
                    Date.UTC(2026, 0, 1, 0, 0, 0, 0),
                    Date.UTC(2026, 11, 31, 23, 59, 59, 999),
                    Date.UTC(2024, 1, 28, 12, 0, 0, 0),
                    Date.UTC(2000, 5, 15, 6, 30, 0, 250),
                    Date.UTC(2026, 8, 30, 15, 59, 59, 1),
                ];
                for (var i = 0; i < instants.length; i++) {
                    var t0 = instants[i];
                    var d = new Date(t0);
                    ok("get_date_" + i, d.getDate() === new Date(t0 + OFF).getUTCDate());
                    var rb = new Date(d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds());
                    ok("roundtrip_" + i, rb.getTime() === t0);
                    var d2 = new Date(t0); d2.setHours(d2.getHours());
                    ok("set_hours_id_" + i, d2.getTime() === t0);
                    var d3 = new Date(t0);
                    d3.setDate(d3.getDate() + 1);
                    ok("next_day_" + i, d3.getTime() === ref(t0, function (r) { r.setUTCDate(r.getUTCDate() + 1); }));
                    var d4 = new Date(t0); d4.setDate(d4.getDate());
                    ok("set_date_id_" + i, d4.getTime() === t0);
                    var d5 = new Date(t0); d5.setMonth(d5.getMonth());
                    ok("set_month_id_" + i, d5.getTime() === t0);
                    var d6 = new Date(t0); d6.setFullYear(d6.getFullYear());
                    ok("set_full_year_id_" + i, d6.getTime() === t0);
                    var d7 = new Date(t0); d7.setMinutes(7, 8, 9);
                    ok("set_minutes_" + i, d7.getTime() === ref(t0, function (r) { r.setUTCMinutes(7, 8, 9); }));
                    var d8 = new Date(t0); d8.setSeconds(3, 4);
                    ok("set_seconds_" + i, d8.getTime() === ref(t0, function (r) { r.setUTCSeconds(3, 4); }));
                    var d9 = new Date(t0); d9.setMilliseconds(42);
                    ok("set_ms_" + i, d9.getTime() === ref(t0, function (r) { r.setUTCMilliseconds(42); }));
                    var d10 = new Date(t0); d10.setHours(1, 2, 3, 4);
                    ok("set_hours_multi_" + i, d10.getTime() === ref(t0, function (r) { r.setUTCHours(1, 2, 3, 4); }));
                    var d11 = new Date(t0); d11.setMonth(0, 13);
                    ok("set_month_day_overflow_" + i, d11.getTime() === ref(t0, function (r) { r.setUTCMonth(0, 13); }));
                    var d12 = new Date(t0); d12.setDate(0);
                    ok("set_date_zero_" + i, d12.getTime() === ref(t0, function (r) { r.setUTCDate(0); }));
                    var d14 = new Date(d.getFullYear(), d.getMonth(), d.getDate());
                    ok("ctor_ymd_midnight_" + i, d14.getTime() === ref(t0, function (r) { r.setUTCHours(0, 0, 0, 0); }));
                }
                ok("ctor_two_args", new Date(2026, 8).getTime() === Date.UTC(2026, 8, 1) - OFF);
                ok("ctor_year_map", new Date(49, 8, 30).getTime() === Date.UTC(1949, 8, 30) - OFF);
                var d17 = new Date(Date.UTC(2026, 8, 29, 20, 22, 47, 498));
                d17.setFullYear(26);
                ok("set_full_year_no_remap", d17.getFullYear() === 26);
                var d18 = new Date(Date.UTC(2026, 8, 29, 20, 22, 47, 498));
                d18.setYear(49);
                ok("set_year_remap", d18.getFullYear() === 1949);
                ok("get_year_annex_b", new Date(Date.UTC(2026, 8, 29)).getYear() === 126);
                var d19 = new Date(NaN);
                ok("invalid_set_date", isNaN(d19.setDate(1)) && isNaN(d19.getTime()));
                var d20 = new Date(Date.UTC(2026, 8, 29, 20, 22, 47, 498));
                d20.setDate(undefined);
                ok("undefined_arg_poisons", isNaN(d20.getTime()));
                var d21 = new Date(Date.UTC(2026, 8, 29, 20, 22, 47, 498));
                d21.setHours(4, undefined);
                ok("present_undefined_poisons", isNaN(d21.getTime()));
                ok("iso_local_full", new Date("2026-09-30T04:22:47").getTime() === Date.UTC(2026, 8, 30, 4, 22, 47) - OFF);
                ok("iso_local_no_seconds", new Date("2026-09-30T04:22").getTime() === Date.UTC(2026, 8, 30, 4, 22) - OFF);
                ok("iso_local_millis", new Date("2026-09-30T04:22:47.5").getTime() === Date.UTC(2026, 8, 30, 4, 22, 47, 500) - OFF);
                ok("parse_iso_local", Date.parse("2026-09-30T04:22:47") === Date.UTC(2026, 8, 30, 4, 22, 47) - OFF);
                ok("iso_date_only_utc", new Date("2026-09-30").getTime() === Date.UTC(2026, 8, 30));
                ok("iso_z_utc", new Date("2026-09-30T04:22:47Z").getTime() === Date.UTC(2026, 8, 30, 4, 22, 47));
                ok("iso_explicit_offset", new Date("2026-09-30T04:22:47+08:00").getTime() === Date.UTC(2026, 8, 29, 20, 22, 47));
                ok("garbage_string_nan", isNaN(new Date("not a date").getTime()));
                ok("instanceof_kept", new Date(0) instanceof Date);
                ok("statics_kept", typeof Date.now === "function" && typeof Date.UTC === "function" && typeof Date.parse === "function");
                ok("call_as_function_string", typeof Date(0) === "string");
                ok("ctor_length_kept", Date.length === 7);
                ok("set_time_utc_kept", (function () { var d = new Date(123456); d.setTime(654321); return d.getTime() === 654321; })());
                ok("utc_setters_kept", (function () { var d = new Date(0); d.setUTCFullYear(2026, 8, 30); d.setUTCHours(4, 0, 0, 0); return d.getTime() === Date.UTC(2026, 8, 30, 4); })());
                return out.join("|");
            })()
        "#;

        let runtime = create_script_runtime().expect("runtime");
        let context =
            create_script_context_with_offset(&runtime, fixed_offset_480).expect("context");
        let joined: String = context
            .with(|ctx| -> Result<String, rquickjs::Error> { ctx.eval(script) })
            .expect("eval failed");
        let failures: Vec<&str> = joined.split('|').filter(|s| s.ends_with("=0")).collect();
        assert!(
            failures.is_empty(),
            "read/write consistency broken under fixed +480 offset: {failures:?}"
        );
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
}
