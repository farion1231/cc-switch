//! Tool Schema 整流器
//!
//! 修复 OpenAI Responses API 中 tool 定义与第三方 Provider 的兼容性问题。
//!
//! ## 问题背景
//! Codex CLI 通过 OpenAI Responses API 发送的 tool 列表中可能包含：
//! 1. 非标准 tool 类型（如 `web_search`），部分 Provider（如阿里云百炼/DashScope）不支持
//! 2. `parameters` 对象缺少 `required` 字段，部分 Provider 要求该字段必须存在
//!
//! ## 整流策略
//! - 移除 `type` 不为 `"function"` 的 tool（如 `web_search`、`code_interpreter` 等）
//! - 对 `parameters` 存在但缺少 `required` 的 function tool，补充 `"required": []`

use serde_json::Value;

/// 整流结果
#[derive(Debug, Clone, Default)]
pub struct ToolSchemaRectifyResult {
    /// 是否应用了整流（有任何修改即为 true）
    pub applied: bool,
    /// 移除的非 function 类型 tool 数量
    pub removed_non_function_tools: usize,
    /// 补充 `required` 字段的 tool 数量
    pub patched_required_count: usize,
}

/// 整流请求体中的 tool schema
///
/// 就地修改 `body` 中的 `tools` 数组：
/// 1. 移除非 `function` 类型的 tool
/// 2. 为缺少 `required` 字段的 `parameters` 补充空数组
pub fn rectify_tool_schema(body: &mut Value) -> ToolSchemaRectifyResult {
    let mut result = ToolSchemaRectifyResult::default();

    let Some(tools) = body.get_mut("tools").and_then(|v| v.as_array_mut()) else {
        return result;
    };

    // 统计移除的非 function tool
    let original_len = tools.len();
    tools.retain(|tool| {
        let tool_type = tool.get("type").and_then(|v| v.as_str()).unwrap_or("");
        tool_type == "function"
    });
    result.removed_non_function_tools = original_len - tools.len();

    // 补充缺失的 required 字段
    for tool in tools.iter_mut() {
        if let Some(params) = tool.get_mut("parameters") {
            if params.is_object() && params.get("required").is_none() {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("required".to_string(), Value::Array(vec![]));
                result.patched_required_count += 1;
            }
        }
    }

    result.applied = result.removed_non_function_tools > 0 || result.patched_required_count > 0;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_no_tools_field() {
        let mut body = json!({"model": "qwen3-max", "input": "hello"});
        let result = rectify_tool_schema(&mut body);
        assert!(!result.applied);
        assert_eq!(result.removed_non_function_tools, 0);
        assert_eq!(result.patched_required_count, 0);
    }

    #[test]
    fn test_empty_tools_array() {
        let mut body = json!({"model": "qwen3-max", "tools": []});
        let result = rectify_tool_schema(&mut body);
        assert!(!result.applied);
    }

    #[test]
    fn test_remove_web_search_tool() {
        let mut body = json!({
            "model": "qwen3-max",
            "tools": [
                {"type": "function", "name": "exec_command", "parameters": {"type": "object", "properties": {}, "required": []}},
                {"type": "web_search", "external_web_access": true},
                {"type": "function", "name": "write_file", "parameters": {"type": "object", "properties": {}, "required": []}}
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(result.applied);
        assert_eq!(result.removed_non_function_tools, 1);
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
        // Verify remaining tools are both function type
        for tool in body["tools"].as_array().unwrap() {
            assert_eq!(tool["type"].as_str().unwrap(), "function");
        }
    }

    #[test]
    fn test_remove_code_interpreter_tool() {
        let mut body = json!({
            "tools": [
                {"type": "code_interpreter"},
                {"type": "function", "name": "test", "parameters": {"type": "object", "properties": {}, "required": []}}
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(result.applied);
        assert_eq!(result.removed_non_function_tools, 1);
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_patch_missing_required_field() {
        let mut body = json!({
            "tools": [
                {
                    "type": "function",
                    "name": "list_resources",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "uri": {"type": "string"}
                        },
                        "additionalProperties": false
                    }
                }
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(result.applied);
        assert_eq!(result.patched_required_count, 1);
        // Verify required field was added
        let params = &body["tools"][0]["parameters"];
        assert!(params.get("required").is_some());
        assert_eq!(params["required"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_skip_tools_with_existing_required() {
        let mut body = json!({
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": {"cmd": {"type": "string"}},
                        "required": ["cmd"]
                    }
                }
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(!result.applied);
        assert_eq!(result.patched_required_count, 0);
    }

    #[test]
    fn test_combined_rectification() {
        let mut body = json!({
            "model": "qwen3-max",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}, "required": ["cmd"]}
                },
                {"type": "web_search", "external_web_access": true},
                {
                    "type": "function",
                    "name": "list_resources",
                    "parameters": {"type": "object", "properties": {"uri": {"type": "string"}}, "additionalProperties": false}
                },
                {
                    "type": "function",
                    "name": "spawn_agent",
                    "parameters": {"type": "object", "properties": {}, "additionalProperties": false}
                }
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(result.applied);
        assert_eq!(result.removed_non_function_tools, 1); // web_search removed
        assert_eq!(result.patched_required_count, 2); // list_resources + spawn_agent
        assert_eq!(body["tools"].as_array().unwrap().len(), 3); // 4 - 1 = 3
    }

    #[test]
    fn test_tools_without_parameters() {
        // Some function tools might not have parameters at all
        let mut body = json!({
            "tools": [
                {"type": "function", "name": "no_params"}
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(!result.applied);
        assert_eq!(result.patched_required_count, 0);
    }

    #[test]
    fn test_tools_with_non_object_parameters() {
        // Edge case: parameters is not an object
        let mut body = json!({
            "tools": [
                {"type": "function", "name": "weird", "parameters": "not_an_object"}
            ]
        });
        let result = rectify_tool_schema(&mut body);
        assert!(!result.applied);
    }
}
