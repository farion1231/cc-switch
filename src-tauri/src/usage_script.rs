use reqwest::Client;
use rquickjs::{Context, Runtime, Function};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// 执行用量查询脚本
pub async fn execute_usage_script(
    script_code: &str,
    api_key: &str,
    base_url: &str,
    timeout_secs: u64,
) -> Result<Value, String> {
    // 1. 替换变量
    let replaced = script_code
        .replace("{{apiKey}}", api_key)
        .replace("{{baseUrl}}", base_url);

    // 2. 在独立作用域中提取 request 配置（确保 Runtime/Context 在 await 前释放）
    let request_config = {
        let runtime = Runtime::new().map_err(|e| format!("创建 JS 运行时失败: {}", e))?;
        let context = Context::full(&runtime).map_err(|e| format!("创建 JS 上下文失败: {}", e))?;

        context.with(|ctx| {
            // 执行用户代码，获取配置对象
            let config: rquickjs::Object = ctx
                .eval(replaced.clone())
                .map_err(|e| format!("解析配置失败: {}", e))?;

            // 提取 request 配置
            let request: rquickjs::Object = config
                .get("request")
                .map_err(|e| format!("缺少 request 配置: {}", e))?;

            // 将 request 转换为 JSON 字符串
            let request_json: String = ctx
                .json_stringify(request)
                .map_err(|e| format!("序列化 request 失败: {}", e))?
                .ok_or("序列化返回 None")?
                .get()
                .map_err(|e| format!("获取字符串失败: {}", e))?;

            Ok::<_, String>(request_json)
        })?
    }; // Runtime 和 Context 在这里被 drop

    // 3. 解析 request 配置
    let request: RequestConfig = serde_json::from_str(&request_config)
        .map_err(|e| format!("request 配置格式错误: {}", e))?;

    // 4. 发送 HTTP 请求
    let response_data = send_http_request(&request, timeout_secs).await?;

    // 5. 在独立作用域中执行 extractor（确保 Runtime/Context 在函数结束前释放）
    let result: Value = {
        let runtime = Runtime::new().map_err(|e| format!("创建 JS 运行时失败: {}", e))?;
        let context = Context::full(&runtime).map_err(|e| format!("创建 JS 上下文失败: {}", e))?;

        context.with(|ctx| {
            // 重新 eval 获取配置对象
            let config: rquickjs::Object = ctx
                .eval(replaced.clone())
                .map_err(|e| format!("重新解析配置失败: {}", e))?;

            // 提取 extractor 函数
            let extractor: Function = config
                .get("extractor")
                .map_err(|e| format!("缺少 extractor 函数: {}", e))?;

            // 将响应数据转换为 JS 值
            let response_js: rquickjs::Value = ctx
                .json_parse(response_data.as_str())
                .map_err(|e| format!("解析响应 JSON 失败: {}", e))?;

            // 调用 extractor(response)
            let result_js: rquickjs::Value = extractor
                .call((response_js,))
                .map_err(|e| format!("执行 extractor 失败: {}", e))?;

            // 转换为 JSON 字符串
            let result_json: String = ctx
                .json_stringify(result_js)
                .map_err(|e| format!("序列化结果失败: {}", e))?
                .ok_or("序列化返回 None")?
                .get()
                .map_err(|e| format!("获取字符串失败: {}", e))?;

            // 解析为 serde_json::Value
            serde_json::from_str(&result_json).map_err(|e| format!("JSON 解析失败: {}", e))
        })?
    }; // Runtime 和 Context 在这里被 drop

    // 6. 验证返回值格式
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
async fn send_http_request(config: &RequestConfig, timeout_secs: u64) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("创建客户端失败: {}", e))?;

    let method = config
        .method
        .parse()
        .unwrap_or(reqwest::Method::GET);

    let mut req = client.request(method.clone(), &config.url);

    // 添加请求头
    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    // 添加请求体
    if let Some(body) = &config.body {
        req = req.body(body.clone());
    }

    // 发送请求
    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))?;

    if !status.is_success() {
        let preview = if text.len() > 200 {
            format!("{}...", &text[..200])
        } else {
            text.clone()
        };
        return Err(format!("HTTP {} : {}", status, preview));
    }

    Ok(text)
}

/// 验证脚本返回值（支持单对象或数组）
fn validate_result(result: &Value) -> Result<(), String> {
    // 如果是数组，验证每个元素
    if let Some(arr) = result.as_array() {
        if arr.is_empty() {
            return Err("脚本返回的数组不能为空".to_string());
        }
        for (idx, item) in arr.iter().enumerate() {
            validate_single_usage(item)
                .map_err(|e| format!("数组索引[{}]验证失败: {}", idx, e))?;
        }
        return Ok(());
    }

    // 如果是单对象，直接验证（向后兼容）
    validate_single_usage(result)
}

/// 验证单个用量数据对象
fn validate_single_usage(result: &Value) -> Result<(), String> {
    let obj = result.as_object().ok_or("脚本必须返回对象或对象数组")?;

    // 所有字段均为可选，只进行类型检查
    if obj.contains_key("isValid") && !result["isValid"].is_null() && !result["isValid"].is_boolean() {
        return Err("isValid 必须是布尔值或 null".to_string());
    }
    if obj.contains_key("invalidMessage") && !result["invalidMessage"].is_null() && !result["invalidMessage"].is_string() {
        return Err("invalidMessage 必须是字符串或 null".to_string());
    }
    if obj.contains_key("remaining") && !result["remaining"].is_null() && !result["remaining"].is_number() {
        return Err("remaining 必须是数字或 null".to_string());
    }
    if obj.contains_key("unit") && !result["unit"].is_null() && !result["unit"].is_string() {
        return Err("unit 必须是字符串或 null".to_string());
    }
    if obj.contains_key("total") && !result["total"].is_null() && !result["total"].is_number() {
        return Err("total 必须是数字或 null".to_string());
    }
    if obj.contains_key("used") && !result["used"].is_null() && !result["used"].is_number() {
        return Err("used 必须是数字或 null".to_string());
    }
    if obj.contains_key("planName") && !result["planName"].is_null() && !result["planName"].is_string() {
        return Err("planName 必须是字符串或 null".to_string());
    }
    if obj.contains_key("extra") && !result["extra"].is_null() && !result["extra"].is_string() {
        return Err("extra 必须是字符串或 null".to_string());
    }

    Ok(())
}
