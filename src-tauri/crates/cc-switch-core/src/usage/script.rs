use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use cc_switch_protocol::protocol::MAX_FRAME_BYTES;
use reqwest::blocking::Client;
use rquickjs::{Context, Function, Runtime};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;
use url::{Host, Url};

use crate::CoreError;

use super::model::{ProviderUsageInput, ProviderUsageTestInput, UsageData, UsageResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageScriptConfig {
    enabled: bool,
    code: String,
    timeout: Option<u64>,
    api_key: Option<String>,
    base_url: Option<String>,
    access_token: Option<String>,
    user_id: Option<String>,
    template_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RequestConfig {
    url: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

struct ScriptExecution<'a> {
    code: &'a str,
    api_key: &'a str,
    base_url: &'a str,
    timeout: u64,
    access_token: Option<&'a str>,
    user_id: Option<&'a str>,
    template_type: Option<&'a str>,
}

pub(super) fn provider_query(
    connection: &Connection,
    input: ProviderUsageInput,
) -> Result<UsageResult, CoreError> {
    let provider = provider(connection, &input.app_type, &input.provider_id)?;
    let script = provider
        .meta
        .get("usageScript")
        .cloned()
        .ok_or_else(|| CoreError::UsageScript("未配置用量查询脚本".to_string()))?;
    let script: UsageScriptConfig = serde_json::from_value(script)?;
    if !script.enabled {
        return Err(CoreError::UsageScript("用量查询未启用".to_string()));
    }
    let (fallback_url, fallback_key) = resolve_provider_credentials(&input.app_type, &provider);
    let api_key = explicit_or_fallback(script.api_key.as_deref(), &fallback_key, false);
    let base_url = explicit_or_fallback(script.base_url.as_deref(), &fallback_url, true);
    execute_and_format(ScriptExecution {
        code: &script.code,
        api_key: &api_key,
        base_url: &base_url,
        timeout: script.timeout.unwrap_or(10),
        access_token: script.access_token.as_deref(),
        user_id: script.user_id.as_deref(),
        template_type: script.template_type.as_deref(),
    })
}

pub(super) fn provider_test(
    connection: &Connection,
    input: ProviderUsageTestInput,
) -> Result<UsageResult, CoreError> {
    let provider = provider(connection, &input.app_type, &input.provider_id)?;
    let (fallback_url, fallback_key) = resolve_provider_credentials(&input.app_type, &provider);
    let api_key = explicit_or_fallback(input.api_key.as_deref(), &fallback_key, false);
    let base_url = explicit_or_fallback(input.base_url.as_deref(), &fallback_url, true);
    execute_and_format(ScriptExecution {
        code: &input.script_code,
        api_key: &api_key,
        base_url: &base_url,
        timeout: input.timeout,
        access_token: input.access_token.as_deref(),
        user_id: input.user_id.as_deref(),
        template_type: input.template_type.as_deref(),
    })
}

fn provider(
    connection: &Connection,
    app_type: &str,
    provider_id: &str,
) -> Result<ScriptProvider, CoreError> {
    connection
        .query_row(
            "SELECT settings_config, meta FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
            |row| {
                let settings: String = row.get(0)?;
                let meta: String = row.get(1)?;
                Ok((settings, meta))
            },
        )
        .optional()?
        .map(|(settings, meta)| -> Result<ScriptProvider, CoreError> {
            Ok(ScriptProvider {
                settings_config: serde_json::from_str(&settings)?,
                meta: serde_json::from_str(&meta)?,
            })
        })
        .transpose()?
        .ok_or_else(|| CoreError::ProviderNotFound(provider_id.to_string()))
}

struct ScriptProvider {
    settings_config: Value,
    meta: Value,
}

fn execute_and_format(input: ScriptExecution<'_>) -> Result<UsageResult, CoreError> {
    let value = match execute_script(&input) {
        Ok(value) => value,
        // 脚本配置、解析与 HTTP 状态错误是可展示的确定性失败；传输超时和帧上限仍向上抛出。
        Err(CoreError::UsageScript(message)) => {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some(message),
            });
        }
        Err(error) => return Err(error),
    };
    let data = if value.is_array() {
        serde_json::from_value::<Vec<UsageData>>(value)?
    } else {
        vec![serde_json::from_value::<UsageData>(value)?]
    };
    if data.is_empty() {
        return Err(CoreError::UsageScript("脚本返回的数组不能为空".to_string()));
    }
    Ok(UsageResult {
        success: true,
        data: Some(data),
        error: None,
    })
}

fn execute_script(input: &ScriptExecution<'_>) -> Result<Value, CoreError> {
    let custom = input.template_type == Some("custom");
    let code = build_script_with_vars(
        input.code,
        input.api_key,
        input.base_url,
        input.access_token,
        input.user_id,
    );
    if !input.base_url.is_empty() && !custom {
        validate_base_url(input.base_url)?;
    }
    let request_json = evaluate_request(&code)?;
    let request: RequestConfig = serde_json::from_str(&request_json)?;
    validate_request_url(&request.url, input.base_url, custom)?;
    let response = send_http_request(&request, input.timeout)?;
    let result = evaluate_extractor(&code, &response)?;
    validate_result(&result)?;
    Ok(result)
}

fn evaluate_request(code: &str) -> Result<String, CoreError> {
    let runtime = Runtime::new().map_err(script_error)?;
    let context = Context::full(&runtime).map_err(script_error)?;
    context.with(|ctx| {
        let config: rquickjs::Object = ctx.eval(code).map_err(script_error)?;
        let request: rquickjs::Object = config.get("request").map_err(script_error)?;
        ctx.json_stringify(request)
            .map_err(script_error)?
            .ok_or_else(|| CoreError::UsageScript("request 无法序列化".to_string()))?
            .get()
            .map_err(script_error)
    })
}

fn evaluate_extractor(code: &str, response: &str) -> Result<Value, CoreError> {
    let runtime = Runtime::new().map_err(script_error)?;
    let context = Context::full(&runtime).map_err(script_error)?;
    context.with(|ctx| {
        let config: rquickjs::Object = ctx.eval(code).map_err(script_error)?;
        let extractor: Function = config.get("extractor").map_err(script_error)?;
        let response = ctx.json_parse(response).map_err(script_error)?;
        let result: rquickjs::Value = extractor.call((response,)).map_err(script_error)?;
        let json: String = ctx
            .json_stringify(result)
            .map_err(script_error)?
            .ok_or_else(|| CoreError::UsageScript("extractor 结果无法序列化".to_string()))?
            .get()
            .map_err(script_error)?;
        serde_json::from_str(&json).map_err(CoreError::from)
    })
}

/// Agent worker 使用阻塞 rustls 客户端；响应读取有硬上限，防止脚本端点耗尽进程内存。
fn send_http_request(config: &RequestConfig, timeout: u64) -> Result<String, CoreError> {
    let timeout = Duration::from_secs(timeout.clamp(2, 30));
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| CoreError::UsageScript(error.to_string()))?;
    let method = config
        .method
        .parse()
        .map_err(|_| CoreError::UsageScript(format!("不支持的 HTTP 方法: {}", config.method)))?;
    let mut request = client.request(method, &config.url);
    for (name, value) in &config.headers {
        request = request.header(name, value);
    }
    if let Some(body) = &config.body {
        request = request.body(body.clone());
    }
    let mut response = request.send().map_err(|error| {
        if error.is_timeout() {
            CoreError::RemoteOperationTimeout("Provider Usage 请求超时".to_string())
        } else {
            CoreError::UsageScript(format!("Provider Usage 请求失败: {error}"))
        }
    })?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| CoreError::UsageScript(format!("读取 Usage 响应失败: {error}")))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CoreError::PayloadTooLarge {
            actual: bytes.len(),
            limit: MAX_FRAME_BYTES,
        });
    }
    let body = String::from_utf8(bytes)
        .map_err(|error| CoreError::UsageScript(format!("Usage 响应不是 UTF-8: {error}")))?;
    if !status.is_success() {
        let preview: String = body.chars().take(200).collect();
        return Err(CoreError::UsageScript(format!("HTTP {status}: {preview}")));
    }
    Ok(body)
}

fn validate_result(result: &Value) -> Result<(), CoreError> {
    let items = result
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(result));
    if items.is_empty() {
        return Err(CoreError::UsageScript("脚本返回的数组不能为空".to_string()));
    }
    for item in items {
        let object = item
            .as_object()
            .ok_or_else(|| CoreError::UsageScript("脚本必须返回对象或对象数组".to_string()))?;
        for (key, kind) in [
            ("isValid", "boolean"),
            ("invalidMessage", "string"),
            ("remaining", "number"),
            ("unit", "string"),
            ("total", "number"),
            ("used", "number"),
            ("planName", "string"),
            ("extra", "string"),
        ] {
            if let Some(value) = object.get(key) {
                let valid = value.is_null()
                    || match kind {
                        "boolean" => value.is_boolean(),
                        "string" => value.is_string(),
                        "number" => value.is_number(),
                        _ => false,
                    };
                if !valid {
                    return Err(CoreError::UsageScript(format!("{key} 类型必须为 {kind}")));
                }
            }
        }
    }
    Ok(())
}

fn build_script_with_vars(
    code: &str,
    api_key: &str,
    base_url: &str,
    access_token: Option<&str>,
    user_id: Option<&str>,
) -> String {
    let mut code = code
        .replace("{{apiKey}}", api_key)
        .replace("{{baseUrl}}", base_url);
    if let Some(value) = access_token {
        code = code.replace("{{accessToken}}", value);
    }
    if let Some(value) = user_id {
        code = code.replace("{{userId}}", value);
    }
    code
}

fn validate_base_url(value: &str) -> Result<(), CoreError> {
    let url = Url::parse(value)
        .map_err(|error| CoreError::UsageScript(format!("无效的 base_url: {error}")))?;
    if url.scheme() != "https" && !is_loopback(&url) {
        return Err(CoreError::UsageScript(
            "base_url 必须使用 HTTPS（localhost 除外）".to_string(),
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(CoreError::UsageScript("base_url 缺少主机名".to_string()));
    }
    Ok(())
}

fn validate_request_url(value: &str, base_url: &str, custom: bool) -> Result<(), CoreError> {
    let request = Url::parse(value)
        .map_err(|error| CoreError::UsageScript(format!("无效的请求 URL: {error}")))?;
    if !custom && request.scheme() != "https" && !is_loopback(&request) {
        return Err(CoreError::UsageScript(
            "请求 URL 必须使用 HTTPS（localhost 除外）".to_string(),
        ));
    }
    if !custom && !base_url.is_empty() {
        let base = Url::parse(base_url)
            .map_err(|error| CoreError::UsageScript(format!("无效的 base_url: {error}")))?;
        if request.host_str() != base.host_str()
            || request.port_or_known_default() != base.port_or_known_default()
        {
            return Err(CoreError::UsageScript(
                "Usage 请求必须与 base_url 同源".to_string(),
            ));
        }
    }
    Ok(())
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(value)) => value.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(value)) => value.is_loopback(),
        Some(Host::Ipv6(value)) => value.is_loopback(),
        None => false,
    }
}

fn explicit_or_fallback(explicit: Option<&str>, fallback: &str, trim_slash: bool) -> String {
    let value = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if trim_slash {
        value.trim_end_matches('/').to_string()
    } else {
        value.to_string()
    }
}

fn resolve_provider_credentials(app_type: &str, provider: &ScriptProvider) -> (String, String) {
    let settings = &provider.settings_config;
    let string = |value: Option<&Value>| value.and_then(Value::as_str).unwrap_or("").to_string();
    let first = |container: Option<&Value>, keys: &[&str]| {
        keys.iter()
            .find_map(|key| {
                container
                    .and_then(|value| value.get(*key))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or("")
            .to_string()
    };
    let (base_url, api_key) = match app_type {
        "claude" | "claude-desktop" => {
            let env = settings.get("env");
            (
                string(env.and_then(|value| value.get("ANTHROPIC_BASE_URL"))),
                first(
                    env,
                    &[
                        "ANTHROPIC_AUTH_TOKEN",
                        "ANTHROPIC_API_KEY",
                        "OPENROUTER_API_KEY",
                        "GOOGLE_API_KEY",
                    ],
                ),
            )
        }
        "gemini" => {
            let env = settings.get("env");
            (
                string(env.and_then(|value| value.get("GOOGLE_GEMINI_BASE_URL"))),
                first(env, &["GEMINI_API_KEY", "GOOGLE_API_KEY"]),
            )
        }
        "opencode" => {
            let options = settings.get("options");
            (
                string(options.and_then(|value| value.get("baseURL"))),
                string(options.and_then(|value| value.get("apiKey"))),
            )
        }
        "openclaw" => (
            string(settings.get("baseUrl")),
            string(settings.get("apiKey")),
        ),
        "hermes" => (
            string(settings.get("base_url")),
            string(settings.get("api_key")),
        ),
        "codex" => resolve_codex_credentials(settings),
        "grokbuild" => resolve_flat_config_credentials(settings),
        _ => (String::new(), String::new()),
    };
    (base_url.trim_end_matches('/').to_string(), api_key)
}

fn resolve_codex_credentials(settings: &Value) -> (String, String) {
    let api_key = settings
        .get("auth")
        .and_then(|value| value.get("OPENAI_API_KEY"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let Some(config) = settings.get("config").and_then(Value::as_str) else {
        return (String::new(), api_key);
    };
    let parsed = config.parse::<toml::Value>().ok();
    let selected = parsed
        .as_ref()
        .and_then(|value| value.get("model_provider"))
        .and_then(toml::Value::as_str);
    let base_url = selected
        .and_then(|name| {
            parsed
                .as_ref()?
                .get("model_providers")?
                .get(name)?
                .get("base_url")?
                .as_str()
        })
        .unwrap_or("")
        .to_string();
    (base_url, api_key)
}

fn resolve_flat_config_credentials(settings: &Value) -> (String, String) {
    let Some(config) = settings.get("config").and_then(Value::as_str) else {
        return (String::new(), String::new());
    };
    let parsed = config.parse::<toml::Value>().ok();
    let base_url = parsed
        .as_ref()
        .and_then(|value| value.get("base_url"))
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string();
    let api_key = parsed
        .as_ref()
        .and_then(|value| value.get("api_key"))
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string();
    (base_url, api_key)
}

fn script_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::UsageScript(error.to_string())
}
