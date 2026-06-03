//! Helpers for adapting Codex Responses custom/freeform tools to Chat function tools.

use crate::proxy::json_canonical::{canonical_json_string, canonicalize_json_string_if_parseable};
use serde_json::{json, Value};

pub(crate) const CUSTOM_TOOL_INPUT_FIELD: &str = "input";

const APPLY_PATCH_TOOL_NAME: &str = "apply_patch";
const EXEC_TOOL_NAME: &str = "exec";
const APPLY_PATCH_BEGIN_MARKER: &str = "*** Begin Patch";
const APPLY_PATCH_END_MARKER: &str = "*** End Patch";

const CUSTOM_CHAT_DESCRIPTION: &str = r#"Codex custom/freeform tool adapted for Chat Completions.

For this bridge, call the function with JSON arguments:
{"input":"<raw tool input>"}

The input string is forwarded as Responses custom_tool_call.input. Put only the
raw string required by the tool in input; do not wrap it in a second JSON object,
Markdown fences, prose, or a shell command unless the original tool description
explicitly requires that."#;

const CUSTOM_INPUT_DESCRIPTION: &str = r#"Raw string input for this Codex custom/freeform tool. This Chat function wrapper requires JSON arguments with an "input" string, and that string is forwarded to Responses custom_tool_call.input."#;

const EXEC_CHAT_DESCRIPTION: &str = r#"Run Codex code-mode JavaScript through the raw exec custom tool.

For this Chat Completions bridge, call the function with JSON arguments:
{"input":"<raw JavaScript source>"}

The input string must contain only JavaScript source text. Do not include
Markdown fences, a JSON string literal, prose, or a shell command wrapper inside
input. You may start with a first-line pragma such as:
// @exec: {"yield_time_ms": 1000, "max_output_tokens": 2000}

Examples:
{"input":"const result = await tools.exec_command({cmd: \"pwd\"});\ntext(result.output);"}

{"input":"// @exec: {\"yield_time_ms\": 1000, \"max_output_tokens\": 2000}\nconst result = await tools.list_mcp_resources({});\ntext(JSON.stringify(result));"}"#;

const EXEC_INPUT_DESCRIPTION: &str = r#"Raw JavaScript source for Codex code-mode exec. Put JS source here as a plain string; do not wrap it in Markdown fences, prose, a shell command, or a second JSON object. An optional first-line // @exec: {...} pragma is allowed."#;

const APPLY_PATCH_CHAT_DESCRIPTION: &str = r#"Apply file edits using Codex's raw apply_patch format.

For this Chat Completions bridge you must call the function with JSON arguments:
{"input":"<raw patch text>"}

The input string itself must be only the raw patch. Do not include Markdown fences,
shell heredocs, JSON, prose, or an apply_patch command wrapper inside input. The
patch must start with "*** Begin Patch" and end with "*** End Patch".

Supported operations:

Add a file:
*** Begin Patch
*** Add File: path/to/new.txt
+first line
+second line
*** End Patch

Update a file:
*** Begin Patch
*** Update File: path/to/file.txt
@@
-old line
+new line
*** End Patch

Delete a file:
*** Begin Patch
*** Delete File: path/to/old.txt
*** End Patch

Move/update a file:
*** Begin Patch
*** Update File: old/path.txt
*** Move to: new/path.txt
@@
-old content
+new content
*** End Patch"#;

const APPLY_PATCH_INPUT_DESCRIPTION: &str = r#"Raw apply_patch patch text. It must begin with "*** Begin Patch" and end with "*** End Patch". Put the patch here as a plain string; do not wrap it in Markdown code fences, prose, a shell command, or a second JSON object. Use "*** Add File:", "*** Update File:", "*** Delete File:", and optionally "*** Move to:" hunks."#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CustomToolInputKind {
    ApplyPatch,
    Exec,
    Generic,
}

impl CustomToolInputKind {
    fn for_tool_name(tool_name: &str) -> Self {
        match tool_name {
            APPLY_PATCH_TOOL_NAME => Self::ApplyPatch,
            EXEC_TOOL_NAME => Self::Exec,
            _ => Self::Generic,
        }
    }

    fn chat_description(self, tool: &Value) -> String {
        match self {
            Self::ApplyPatch => APPLY_PATCH_CHAT_DESCRIPTION.to_string(),
            Self::Exec => with_original_tool_description(EXEC_CHAT_DESCRIPTION, tool),
            Self::Generic => with_original_tool_description(CUSTOM_CHAT_DESCRIPTION, tool),
        }
    }

    fn input_description(self) -> &'static str {
        match self {
            Self::ApplyPatch => APPLY_PATCH_INPUT_DESCRIPTION,
            Self::Exec => EXEC_INPUT_DESCRIPTION,
            Self::Generic => CUSTOM_INPUT_DESCRIPTION,
        }
    }

    fn argument_keys(self) -> &'static [&'static str] {
        match self {
            Self::ApplyPatch => &[CUSTOM_TOOL_INPUT_FIELD, "patch", "command"],
            Self::Exec => &[
                CUSTOM_TOOL_INPUT_FIELD,
                "code",
                "source",
                "script",
                "javascript",
                "js",
                "command",
            ],
            Self::Generic => &[
                CUSTOM_TOOL_INPUT_FIELD,
                "text",
                "content",
                "payload",
                "data",
                "source",
                "code",
            ],
        }
    }

    fn normalize_input(self, input: &str) -> String {
        match self {
            Self::ApplyPatch => normalize_apply_patch_input(input),
            Self::Exec => normalize_exec_input(input),
            Self::Generic => normalize_generic_input(input),
        }
    }
}

pub(crate) fn custom_tool_chat_description(tool_name: &str, tool: &Value) -> String {
    CustomToolInputKind::for_tool_name(tool_name).chat_description(tool)
}

pub(crate) fn custom_tool_input_description(tool_name: &str) -> &'static str {
    CustomToolInputKind::for_tool_name(tool_name).input_description()
}

pub(crate) fn custom_tool_arguments_from_response_input(input: Option<&Value>) -> String {
    let input = response_custom_input_to_string(input);
    canonical_json_string(&json!({ CUSTOM_TOOL_INPUT_FIELD: input }))
}

pub(crate) fn custom_tool_arguments_from_chat_value(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(value)) => custom_tool_arguments_from_chat_str(value),
        Some(value) => canonical_json_string(value),
        None => String::new(),
    }
}

pub(crate) fn custom_tool_arguments_from_chat_str(arguments: &str) -> String {
    canonicalize_json_string_if_parseable(arguments)
}

pub(crate) fn custom_tool_input_from_chat_arguments(tool_name: &str, arguments: &str) -> String {
    let kind = CustomToolInputKind::for_tool_name(tool_name);
    let candidate = custom_tool_input_candidate_from_chat_arguments(kind, arguments)
        .unwrap_or_else(|| arguments.to_string());
    kind.normalize_input(&candidate)
}

fn response_custom_input_to_string(input: Option<&Value>) -> String {
    match input {
        Some(Value::String(value)) => value.clone(),
        Some(value) => canonical_json_string(value),
        None => String::new(),
    }
}

fn custom_tool_input_candidate_from_chat_arguments(
    kind: CustomToolInputKind,
    arguments: &str,
) -> Option<String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Some(String::new());
    }

    let value = serde_json::from_str::<Value>(trimmed).ok()?;
    custom_tool_input_candidate_from_value(kind, &value)
}

fn custom_tool_input_candidate_from_value(
    kind: CustomToolInputKind,
    value: &Value,
) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(obj) => {
            for key in kind.argument_keys() {
                if let Some(value) = obj.get(*key) {
                    if let Some(candidate) =
                        custom_tool_input_candidate_from_argument_value(kind, value)
                    {
                        return Some(candidate);
                    }
                }
            }

            if obj.len() == 1 {
                return obj
                    .values()
                    .next()
                    .map(|value| response_custom_input_to_string(Some(value)));
            }

            (kind == CustomToolInputKind::Generic).then(|| canonical_json_string(value))
        }
        _ => Some(response_custom_input_to_string(Some(value))),
    }
}

fn custom_tool_input_candidate_from_argument_value(
    kind: CustomToolInputKind,
    value: &Value,
) -> Option<String> {
    if kind == CustomToolInputKind::ApplyPatch {
        if let Some(candidate) = apply_patch_candidate_from_command_array(value) {
            return Some(candidate);
        }
    }

    Some(response_custom_input_to_string(Some(value)))
}

fn apply_patch_candidate_from_command_array(value: &Value) -> Option<String> {
    let Value::Array(items) = value else {
        return None;
    };
    if !items
        .first()
        .and_then(Value::as_str)
        .is_some_and(|name| name == APPLY_PATCH_TOOL_NAME)
    {
        return None;
    }

    items
        .iter()
        .skip(1)
        .filter_map(Value::as_str)
        .find(|text| text.contains(APPLY_PATCH_BEGIN_MARKER))
        .map(ToString::to_string)
}

fn normalize_apply_patch_input(input: &str) -> String {
    let mut value = strip_markdown_fence(input)
        .unwrap_or(input)
        .trim()
        .to_string();
    if let Some(extracted) = extract_apply_patch_body(&value) {
        value = extracted;
    }
    strip_markdown_fence(&value)
        .unwrap_or(&value)
        .trim()
        .to_string()
}

fn normalize_exec_input(input: &str) -> String {
    strip_markdown_fence(input)
        .unwrap_or(input)
        .trim()
        .to_string()
}

fn normalize_generic_input(input: &str) -> String {
    strip_markdown_fence(input)
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| input.to_string())
}

fn extract_apply_patch_body(input: &str) -> Option<String> {
    let begin = input.find(APPLY_PATCH_BEGIN_MARKER)?;
    let after_begin = &input[begin..];
    let end_relative = after_begin.find(APPLY_PATCH_END_MARKER)?;
    let end = begin + end_relative + APPLY_PATCH_END_MARKER.len();
    Some(input[begin..end].to_string())
}

fn strip_markdown_fence(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix("```")?;
    let first_newline = rest.find('\n')?;
    let body = rest[first_newline + 1..].trim_end();
    body.strip_suffix("```").map(str::trim_end)
}

fn with_original_tool_description(prefix: &str, tool: &Value) -> String {
    let Some(description) = tool
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return prefix.to_string();
    };

    format!("{prefix}\n\nOriginal Codex tool description:\n{description}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn custom_tool_arguments_from_response_input_always_uses_string_input() {
        assert_eq!(
            custom_tool_arguments_from_response_input(Some(&json!("hello"))),
            r#"{"input":"hello"}"#
        );
        assert_eq!(
            custom_tool_arguments_from_response_input(Some(&json!({"b": 2, "a": 1}))),
            r#"{"input":"{\"a\":1,\"b\":2}"}"#
        );
    }

    #[test]
    fn generic_custom_input_accepts_string_aliases_and_structured_values() {
        assert_eq!(
            custom_tool_input_from_chat_arguments("custom", r#"{"content":"hello"}"#),
            "hello"
        );
        assert_eq!(
            custom_tool_input_from_chat_arguments("custom", r#"{"input":{"b":2,"a":1}}"#),
            r#"{"a":1,"b":2}"#
        );
        assert_eq!(
            custom_tool_input_from_chat_arguments("custom", r#""raw string""#),
            "raw string"
        );
    }

    #[test]
    fn exec_input_accepts_code_alias_and_strips_fences() {
        let input = custom_tool_input_from_chat_arguments(
            "exec",
            r#"{"code":"```js\nconst value = 1;\ntext(value);\n```"}"#,
        );

        assert_eq!(input, "const value = 1;\ntext(value);");
    }

    #[test]
    fn apply_patch_input_extracts_command_array_and_patch_body() {
        let input = custom_tool_input_from_chat_arguments(
            "apply_patch",
            r#"{"command":["apply_patch","prose\n*** Begin Patch\n*** Delete File: old.txt\n*** End Patch\nextra"]}"#,
        );

        assert_eq!(
            input,
            "*** Begin Patch\n*** Delete File: old.txt\n*** End Patch"
        );
    }

    #[test]
    fn custom_tool_chat_description_preserves_original_description() {
        let description =
            custom_tool_chat_description("exec", &json!({"description": "Original exec details."}));

        assert!(description.contains(r#"{"input":"<raw JavaScript source>"}"#));
        assert!(description.contains("Original Codex tool description:"));
        assert!(description.contains("Original exec details."));
    }
}
