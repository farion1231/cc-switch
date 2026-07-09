//! 请求体过滤模块
//!
//! 过滤不应透传到上游的私有参数，防止内部信息泄露。
//!
//! ## 过滤规则
//! - 以 `_` 开头的字段被视为私有参数，会被递归过滤
//! - 支持白名单机制，允许透传特定的 `_` 前缀字段
//! - 支持嵌套对象和数组的深度过滤
//! - JSON Schema 的 properties / patternProperties / definitions / $defs 名称
//!   是用户定义的字段名，不按私有参数过滤
//!
//! ## 使用场景
//! - `_internal_id`: 内部追踪 ID
//! - `_debug_mode`: 调试标记
//! - `_session_token`: 会话令牌
//! - `_client_version`: 客户端版本

use serde_json::Value;
use std::collections::HashSet;

const IMAGE_GENERATION_TOOL_TYPES: &[&str] = &[
    "image_generation",
    "image_generation_call",
    "image_generation_preview",
];

/// 过滤私有参数（以 `_` 开头的字段）
///
/// 递归遍历 JSON 结构，移除所有以下划线开头的字段。
///
/// # Arguments
/// * `body` - 原始请求体
///
/// # Returns
/// 过滤后的请求体
///
/// # Example
/// ```ignore
/// let input = json!({
///     "model": "claude-3",
///     "_internal_id": "abc123",
///     "messages": [{"role": "user", "content": "hello", "_token": "secret"}]
/// });
/// let output = filter_private_params(input);
/// // output 中不包含 _internal_id 和 _token
/// ```
#[cfg(test)]
pub fn filter_private_params(body: Value) -> Value {
    filter_private_params_with_whitelist(body, &[])
}

/// 过滤私有参数（支持白名单）
///
/// 递归遍历 JSON 结构，移除所有以下划线开头的字段，
/// 但保留白名单中指定的字段。
///
/// # Arguments
/// * `body` - 原始请求体
/// * `whitelist` - 白名单字段列表（不过滤这些字段）
///
/// # Returns
/// 过滤后的请求体
///
/// # Example
/// ```ignore
/// let input = json!({
///     "model": "claude-3",
///     "_metadata": {"key": "value"},  // 白名单中，保留
///     "_internal_id": "abc123"        // 不在白名单中，过滤
/// });
/// let output = filter_private_params_with_whitelist(input, &["_metadata"]);
/// // output 包含 _metadata，不包含 _internal_id
/// ```
pub fn filter_private_params_with_whitelist(body: Value, whitelist: &[String]) -> Value {
    let whitelist_set: HashSet<&str> = whitelist.iter().map(|s| s.as_str()).collect();
    filter_recursive_with_whitelist(body, &mut Vec::new(), &mut Vec::new(), &whitelist_set)
}

/// 移除 Codex Responses 请求中会触发部分上游生图权限校验的工具声明。
///
/// - 移除 `type` 为 `image_generation` / `image_generation_call` /
///   `image_generation_preview` 的对象
/// - 移除 `name` 中包含 `image_generation`（大小写不敏感）的对象
pub fn strip_image_generation_tools(body: Value) -> (Value, usize) {
    let mut removed = 0;
    let filtered = strip_image_generation_recursive(body, &mut removed);
    (filtered, removed)
}

/// 递归过滤实现（支持白名单）
fn filter_recursive_with_whitelist(
    value: Value,
    path: &mut Vec<String>,
    removed_keys: &mut Vec<String>,
    whitelist: &HashSet<&str>,
) -> Value {
    match value {
        Value::Object(map) => {
            let is_schema_name_map = path.last().is_some_and(|key| matches_schema_name_map(key));
            let filtered: serde_json::Map<String, Value> = map
                .into_iter()
                .filter_map(|(key, val)| {
                    // 以 _ 开头且不在白名单中的字段被过滤
                    if key.starts_with('_')
                        && !whitelist.contains(key.as_str())
                        && !is_schema_name_map
                    {
                        removed_keys.push(key);
                        None
                    } else {
                        path.push(key.clone());
                        let filtered_value =
                            filter_recursive_with_whitelist(val, path, removed_keys, whitelist);
                        path.pop();
                        Some((key, filtered_value))
                    }
                })
                .collect();

            // 仅在有过滤时记录日志（避免每次请求都打印）
            if !removed_keys.is_empty() {
                log::debug!("[BodyFilter] 过滤私有参数: {removed_keys:?}");
                removed_keys.clear();
            }

            Value::Object(filtered)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| filter_recursive_with_whitelist(v, path, removed_keys, whitelist))
                .collect(),
        ),
        other => other,
    }
}

fn strip_image_generation_recursive(value: Value, removed: &mut usize) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .filter_map(|item| {
                    if is_image_generation_tool_object(&item) {
                        *removed += 1;
                        None
                    } else {
                        Some(strip_image_generation_recursive(item, removed))
                    }
                })
                .collect(),
        ),
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .into_iter()
                .filter_map(|(key, child)| {
                    if is_image_generation_tool_object(&child) {
                        *removed += 1;
                        None
                    } else {
                        Some((key, strip_image_generation_recursive(child, removed)))
                    }
                })
                .collect();
            Value::Object(filtered)
        }
        other => other,
    }
}

fn is_image_generation_tool_object(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    if object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value_type| IMAGE_GENERATION_TOOL_TYPES.contains(&value_type))
    {
        return true;
    }

    object
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name.to_ascii_lowercase().contains("image_generation"))
}

fn matches_schema_name_map(key: &str) -> bool {
    matches!(
        key,
        "properties" | "patternProperties" | "definitions" | "$defs"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_filter_top_level_private_params() {
        let input = json!({
            "model": "claude-3",
            "_internal_id": "abc123",
            "_debug": true,
            "max_tokens": 1024
        });

        let output = filter_private_params(input);

        assert!(output.get("model").is_some());
        assert!(output.get("max_tokens").is_some());
        assert!(output.get("_internal_id").is_none());
        assert!(output.get("_debug").is_none());
    }

    #[test]
    fn test_filter_nested_private_params() {
        let input = json!({
            "model": "claude-3",
            "messages": [
                {
                    "role": "user",
                    "content": "hello",
                    "_session_token": "secret"
                }
            ],
            "metadata": {
                "user_id": "user-1",
                "_tracking_id": "track-1"
            }
        });

        let output = filter_private_params(input);

        // 顶级字段保留
        assert!(output.get("model").is_some());
        assert!(output.get("messages").is_some());
        assert!(output.get("metadata").is_some());

        // messages 数组中的私有参数被过滤
        let messages = output.get("messages").unwrap().as_array().unwrap();
        assert!(messages[0].get("role").is_some());
        assert!(messages[0].get("content").is_some());
        assert!(messages[0].get("_session_token").is_none());

        // metadata 对象中的私有参数被过滤
        let metadata = output.get("metadata").unwrap();
        assert!(metadata.get("user_id").is_some());
        assert!(metadata.get("_tracking_id").is_none());
    }

    #[test]
    fn test_filter_deeply_nested() {
        let input = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "keep": "value",
                        "_remove": "secret"
                    }
                }
            }
        });

        let output = filter_private_params(input);

        let level3 = output
            .get("level1")
            .unwrap()
            .get("level2")
            .unwrap()
            .get("level3")
            .unwrap();

        assert!(level3.get("keep").is_some());
        assert!(level3.get("_remove").is_none());
    }

    #[test]
    fn test_filter_array_of_objects() {
        let input = json!({
            "items": [
                {"id": 1, "_secret": "a"},
                {"id": 2, "_secret": "b"},
                {"id": 3, "_secret": "c"}
            ]
        });

        let output = filter_private_params(input);
        let items = output.get("items").unwrap().as_array().unwrap();

        for item in items {
            assert!(item.get("id").is_some());
            assert!(item.get("_secret").is_none());
        }
    }

    #[test]
    fn test_no_private_params() {
        let input = json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hello"}]
        });

        let output = filter_private_params(input.clone());

        // 无私有参数时，输出应与输入相同
        assert_eq!(input, output);
    }

    #[test]
    fn test_empty_object() {
        let input = json!({});
        let output = filter_private_params(input);
        assert_eq!(output, json!({}));
    }

    #[test]
    fn test_primitive_values() {
        // 原始值不应被修改
        assert_eq!(filter_private_params(json!(42)), json!(42));
        assert_eq!(filter_private_params(json!("string")), json!("string"));
        assert_eq!(filter_private_params(json!(true)), json!(true));
        assert_eq!(filter_private_params(json!(null)), json!(null));
    }

    #[test]
    fn test_whitelist_preserves_private_params() {
        let input = json!({
            "model": "claude-3",
            "_metadata": {"key": "value"},
            "_internal_id": "abc123",
            "_stream_options": {"include_usage": true}
        });

        let whitelist = vec!["_metadata".to_string(), "_stream_options".to_string()];
        let output = filter_private_params_with_whitelist(input, &whitelist);

        // 白名单中的字段保留
        assert!(output.get("_metadata").is_some());
        assert!(output.get("_stream_options").is_some());
        // 不在白名单中的私有字段被过滤
        assert!(output.get("_internal_id").is_none());
        // 普通字段保留
        assert!(output.get("model").is_some());
    }

    #[test]
    fn test_whitelist_nested() {
        let input = json!({
            "data": {
                "_allowed": "keep",
                "_forbidden": "remove",
                "normal": "value"
            }
        });

        let whitelist = vec!["_allowed".to_string()];
        let output = filter_private_params_with_whitelist(input, &whitelist);

        let data = output.get("data").unwrap();
        assert!(data.get("_allowed").is_some());
        assert!(data.get("_forbidden").is_none());
        assert!(data.get("normal").is_some());
    }

    #[test]
    fn test_preserves_json_schema_property_names_with_underscore() {
        let input = json!({
            "tools": [
                {
                    "name": "lookup",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "_id": {"type": "string", "_internal_note": "remove"},
                            "_meta": {"type": "object"}
                        },
                        "_private_schema_note": "remove"
                    }
                }
            ]
        });

        let output = filter_private_params(input);
        let schema = &output["tools"][0]["input_schema"];

        assert!(schema["properties"].get("_id").is_some());
        assert!(schema["properties"].get("_meta").is_some());
        assert!(schema["properties"]["_id"].get("_internal_note").is_none());
        assert!(schema.get("_private_schema_note").is_none());
    }

    #[test]
    fn test_empty_whitelist_same_as_default() {
        let input = json!({
            "model": "claude-3",
            "_internal_id": "abc123"
        });

        let output1 = filter_private_params(input.clone());
        let output2 = filter_private_params_with_whitelist(input, &[]);

        assert_eq!(output1, output2);
    }

    #[test]
    fn test_strip_image_generation_tools_removes_codex_proxy_tool_types() {
        let input = json!({
            "model": "gpt-5-codex",
            "tools": [
                {"type": "function", "name": "shell"},
                {"type": "image_generation"},
                {"type": "image_generation_call"},
                {"type": "image_generation_preview"}
            ],
            "tool_choice": {"type": "image_generation"}
        });

        let (output, removed) = strip_image_generation_tools(input);

        assert_eq!(removed, 4);
        assert_eq!(
            output["tools"],
            json!([{"type": "function", "name": "shell"}])
        );
        assert!(output.get("tool_choice").is_none());
    }

    #[test]
    fn test_strip_image_generation_tools_removes_nested_name_matches() {
        let input = json!({
            "metadata": {
                "keep": true,
                "nested": {
                    "name": "preview_image_generation_tool",
                    "description": "drop me"
                }
            },
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "hello"},
                        {"name": "IMAGE_GENERATION_helper", "enabled": true}
                    ]
                }
            ]
        });

        let (output, removed) = strip_image_generation_tools(input);

        assert_eq!(removed, 2);
        assert_eq!(output["metadata"], json!({"keep": true}));
        assert_eq!(
            output["input"][0]["content"],
            json!([{"type": "input_text", "text": "hello"}])
        );
    }
}
