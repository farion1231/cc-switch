use crate::proxy::json_canonical::canonical_json_string;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ToolCompatContext {
    custom_tools: BTreeMap<String, CustomToolSpec>,
    function_tools: BTreeMap<String, FunctionToolSpec>,
    has_custom_tools: bool,
    has_namespace_tools: bool,
}

#[derive(Debug, Clone)]
struct CustomToolSpec {
    upstream_name: String,
    kind: CustomToolKind,
    proxy_action: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CustomToolKind {
    Raw,
    ApplyPatch,
    BuiltIn,
}

#[derive(Debug, Clone)]
struct FunctionToolSpec {
    namespace: Option<String>,
    name: String,
}

#[derive(Debug, Clone, Default)]
struct PatchOperation {
    op_type: String,
    path: String,
    move_to: String,
    content: String,
    hunks: Vec<PatchHunk>,
}

#[derive(Debug, Clone, Default)]
struct PatchHunk {
    context: String,
    lines: Vec<PatchLine>,
}

#[derive(Debug, Clone, Default)]
struct PatchLine {
    op: String,
    text: String,
}

impl ToolCompatContext {
    pub fn from_request(body: &Value) -> Self {
        body.get("tools")
            .and_then(|value| value.as_array())
            .map(|tools| Self::from_tools(tools))
            .unwrap_or_default()
    }

    pub fn from_tools(tools: &[Value]) -> Self {
        let mut ctx = Self::default();

        for raw_tool in tools {
            if let Some(name) = raw_tool.as_str().filter(|name| !name.is_empty()) {
                if let Some(action) = proxy_action_from_name(name) {
                    if name.starts_with("apply_patch_") {
                        ctx.custom_tools.insert(
                            name.to_string(),
                            CustomToolSpec {
                                upstream_name: "apply_patch".to_string(),
                                kind: CustomToolKind::ApplyPatch,
                                proxy_action: Some(action),
                            },
                        );
                    } else {
                        ctx.custom_tools.insert(
                            name.to_string(),
                            CustomToolSpec {
                                upstream_name: name.to_string(),
                                kind: CustomToolKind::Raw,
                                proxy_action: None,
                            },
                        );
                    }
                } else {
                    ctx.custom_tools.insert(
                        name.to_string(),
                        CustomToolSpec {
                            upstream_name: name.to_string(),
                            kind: CustomToolKind::Raw,
                            proxy_action: None,
                        },
                    );
                }
                ctx.has_custom_tools = true;
                continue;
            }

            let Some(tool) = raw_tool.as_object() else {
                continue;
            };
            let tool_type = tool
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match tool_type {
                "custom" => ctx.add_custom_tool(tool),
                "function" => {
                    if let Some(name) = extract_tool_name(tool) {
                        ctx.function_tools.insert(
                            name.clone(),
                            FunctionToolSpec {
                                namespace: None,
                                name,
                            },
                        );
                    }
                }
                "namespace" => ctx.add_namespace_tool(tool),
                "web_search" | "local_shell" | "computer_use" => {
                    let name = extract_tool_name(tool).unwrap_or_else(|| tool_type.to_string());
                    ctx.custom_tools.insert(
                        name.clone(),
                        CustomToolSpec {
                            upstream_name: name,
                            kind: CustomToolKind::BuiltIn,
                            proxy_action: None,
                        },
                    );
                    ctx.has_custom_tools = true;
                }
                _ => {}
            }
        }

        ctx
    }

    pub fn convert_tools_to_chat(&self, tools: &[Value]) -> Vec<Value> {
        if !self.has_custom_tools && !self.has_namespace_tools {
            return tools
                .iter()
                .filter_map(simple_responses_tool_to_chat_tool)
                .collect();
        }

        let mut result = Vec::new();
        let mut seen_patch_roots = BTreeMap::<String, bool>::new();

        for raw_tool in tools {
            if let Some(name) = raw_tool.as_str().filter(|name| !name.is_empty()) {
                result.push(generic_custom_proxy_tool(name, None));
                continue;
            }

            let Some(tool) = raw_tool.as_object() else {
                continue;
            };
            let tool_type = tool
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match tool_type {
                "function" => {
                    if let Some(value) = simple_responses_tool_to_chat_tool(raw_tool) {
                        result.push(value);
                    }
                }
                "namespace" => result.extend(self.namespace_tools_to_chat(tool)),
                "custom" | "web_search" | "local_shell" | "computer_use" => {
                    let name = extract_tool_name(tool).unwrap_or_else(|| tool_type.to_string());
                    let description = extract_tool_description(tool);
                    let spec = self.custom_tools.get(&name);
                    if matches!(spec.map(|spec| spec.kind), Some(CustomToolKind::ApplyPatch)) {
                        if seen_patch_roots.insert(name.clone(), true).is_none() {
                            result.extend(apply_patch_proxy_tools(&name, description.as_deref()));
                        }
                    } else {
                        result.push(generic_custom_proxy_tool(&name, description.as_deref()));
                    }
                }
                _ => {}
            }
        }

        result
    }

    pub fn convert_tool_choice(&self, tool_choice: &Value) -> Option<Value> {
        let Some(obj) = tool_choice.as_object() else {
            return Some(tool_choice.clone());
        };
        let choice_type = obj
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match choice_type {
            "function" => {
                if let Some(namespace) = obj.get("namespace").and_then(|value| value.as_str()) {
                    let name = obj
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    return Some(chat_function_choice(&flatten_namespace_tool_name(
                        namespace, name,
                    )));
                }
                if let Some(function) = obj.get("function").and_then(|value| value.as_object()) {
                    if let Some(namespace) = function.get("namespace").and_then(|v| v.as_str()) {
                        let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        return Some(chat_function_choice(&flatten_namespace_tool_name(
                            namespace, name,
                        )));
                    }
                    if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                        return Some(chat_function_choice(name));
                    }
                }
                let name = obj
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                Some(chat_function_choice(name))
            }
            "custom" => {
                let name = obj
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let spec = self.custom_tools.get(name)?;
                let upstream = if spec.kind == CustomToolKind::ApplyPatch {
                    format!("{}_batch", spec.upstream_name)
                } else {
                    spec.upstream_name.clone()
                };
                Some(chat_function_choice(&upstream))
            }
            _ => Some(tool_choice.clone()),
        }
    }

    pub fn custom_history_arguments(&self, original_name: &str, input: &str) -> (String, String) {
        let Some(spec) = self.custom_tools.get(original_name) else {
            return (
                original_name.to_string(),
                serde_json::to_string(&json!({ "input": input })).unwrap_or_else(|_| "{}".into()),
            );
        };

        if spec.kind != CustomToolKind::ApplyPatch {
            return (
                spec.upstream_name.clone(),
                serde_json::to_string(&json!({ "input": input })).unwrap_or_else(|_| "{}".into()),
            );
        }

        let Some(ops) = parse_patch_operations(input).filter(|ops| !ops.is_empty()) else {
            return (
                format!("{}_batch", spec.upstream_name),
                serde_json::to_string(&json!({
                    "operations": [],
                    "raw_patch": input
                }))
                .unwrap_or_else(|_| "{}".into()),
            );
        };

        if ops.len() == 1 {
            let action = choose_single_proxy_action(&ops[0].op_type);
            return (
                format!("{}_{}", spec.upstream_name, action),
                single_patch_operation_args(&ops[0]),
            );
        }

        (
            format!("{}_batch", spec.upstream_name),
            batch_patch_operations_args(&ops),
        )
    }

    pub fn flatten_response_function_name(&self, item: &Value) -> String {
        let namespace = item.get("namespace").and_then(|value| value.as_str());
        let name = item
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        namespace
            .map(|namespace| flatten_namespace_tool_name(namespace, name))
            .unwrap_or_else(|| name.to_string())
    }

    pub fn custom_response_item(
        &self,
        call_id: &str,
        upstream_name: &str,
        raw_arguments: &str,
    ) -> Option<Value> {
        let spec = self.custom_tools.get(upstream_name)?;
        let input = self.reconstruct_custom_input(upstream_name, raw_arguments);
        Some(json!({
            "id": format!("ctc_{call_id}"),
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": call_id,
            "name": spec.upstream_name,
            "input": input
        }))
    }

    pub fn apply_namespace_to_response_item(&self, item: &mut Value) {
        let Some(name) = item.get("name").and_then(|value| value.as_str()) else {
            return;
        };
        let Some(spec) = self.function_tools.get(name) else {
            return;
        };
        let Some(namespace) = spec.namespace.as_deref() else {
            return;
        };
        item["name"] = json!(spec.name);
        item["namespace"] = json!(namespace);
    }

    pub fn is_custom_proxy(&self, upstream_name: &str) -> bool {
        self.custom_tools.contains_key(upstream_name)
    }

    pub fn original_custom_name(&self, upstream_name: &str) -> String {
        self.custom_tools
            .get(upstream_name)
            .map(|spec| spec.upstream_name.clone())
            .unwrap_or_else(|| upstream_name.to_string())
    }

    pub fn reconstruct_custom_input(&self, upstream_name: &str, raw_arguments: &str) -> String {
        let Some(spec) = self.custom_tools.get(upstream_name) else {
            return raw_arguments.to_string();
        };

        if spec.kind == CustomToolKind::ApplyPatch {
            let action = spec
                .proxy_action
                .or_else(|| proxy_action_from_name(upstream_name))
                .unwrap_or("");
            return apply_patch_input_from_proxy_arguments(raw_arguments, action);
        }

        serde_json::from_str::<Value>(raw_arguments)
            .ok()
            .and_then(|value| {
                value
                    .get("input")
                    .and_then(|input| input.as_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| raw_arguments.to_string())
    }

    fn add_custom_tool(&mut self, tool: &Map<String, Value>) {
        let Some(name) = extract_tool_name(tool) else {
            return;
        };
        let kind = detect_custom_tool_kind(tool);
        let spec = CustomToolSpec {
            upstream_name: name.clone(),
            kind,
            proxy_action: None,
        };
        if kind == CustomToolKind::ApplyPatch {
            self.custom_tools.insert(name.clone(), spec.clone());
            for action in [
                "add_file",
                "delete_file",
                "update_file",
                "replace_file",
                "batch",
            ] {
                let mut proxy_spec = spec.clone();
                proxy_spec.proxy_action = Some(action);
                self.custom_tools
                    .insert(format!("{name}_{action}"), proxy_spec);
            }
        } else {
            self.custom_tools.insert(name, spec);
        }
        self.has_custom_tools = true;
    }

    fn add_namespace_tool(&mut self, tool: &Map<String, Value>) {
        let namespace = extract_tool_name(tool).unwrap_or_default();
        let Some(children) = tool.get("tools").and_then(|value| value.as_array()) else {
            return;
        };

        for child in children {
            let Some(child_obj) = child.as_object() else {
                continue;
            };
            if child_obj.get("type").and_then(|value| value.as_str()) != Some("function") {
                continue;
            }
            let Some(name) = extract_tool_name(child_obj) else {
                continue;
            };
            let flat = flatten_namespace_tool_name(&namespace, &name);
            if self
                .function_tools
                .get(&flat)
                .and_then(|spec| spec.namespace.as_deref())
                .is_none()
                && self.function_tools.contains_key(&flat)
            {
                continue;
            }
            self.function_tools.insert(
                flat,
                FunctionToolSpec {
                    namespace: (!namespace.is_empty()).then_some(namespace.clone()),
                    name,
                },
            );
            self.has_namespace_tools = true;
        }
    }

    fn namespace_tools_to_chat(&self, tool: &Map<String, Value>) -> Vec<Value> {
        let namespace = extract_tool_name(tool).unwrap_or_default();
        let namespace_description = extract_tool_description(tool);
        let Some(children) = tool.get("tools").and_then(|value| value.as_array()) else {
            return Vec::new();
        };

        children
            .iter()
            .filter_map(|child| {
                let child_obj = child.as_object()?;
                if child_obj.get("type").and_then(|value| value.as_str()) != Some("function") {
                    return None;
                }
                let name = extract_tool_name(child_obj)?;
                let flat = flatten_namespace_tool_name(&namespace, &name);
                if !namespace.is_empty()
                    && self
                        .function_tools
                        .get(&flat)
                        .and_then(|spec| spec.namespace.as_deref())
                        .is_none()
                {
                    return None;
                }

                let (_, child_description, parameters, strict) =
                    extract_responses_tool_fields(child_obj);
                let combined = combine_namespace_description(
                    namespace_description.as_deref(),
                    child_description.as_deref(),
                );
                Some(function_tool(
                    &flat,
                    combined.as_deref(),
                    parameters,
                    strict,
                ))
            })
            .collect()
    }
}

fn simple_responses_tool_to_chat_tool(tool: &Value) -> Option<Value> {
    let obj = tool.as_object()?;
    if obj.get("type").and_then(|value| value.as_str()) != Some("function") {
        return None;
    }

    if obj.get("function").is_some() {
        let mut chat_tool = tool.clone();
        if let Some(strict) = obj.get("strict").cloned() {
            if let Some(function) = chat_tool
                .get_mut("function")
                .and_then(|value| value.as_object_mut())
            {
                function.entry("strict".to_string()).or_insert(strict);
            }
            if let Some(obj) = chat_tool.as_object_mut() {
                obj.remove("strict");
            }
        }
        return Some(chat_tool);
    }

    let (name, description, parameters, strict) = extract_responses_tool_fields(obj);
    Some(function_tool(
        &name,
        description.as_deref(),
        parameters,
        strict,
    ))
}

fn extract_responses_tool_fields(
    obj: &Map<String, Value>,
) -> (String, Option<String>, Value, Option<Value>) {
    if let Some(function) = obj.get("function").and_then(|value| value.as_object()) {
        return (
            function
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
            function
                .get("description")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({})),
            function
                .get("strict")
                .cloned()
                .or_else(|| obj.get("strict").cloned()),
        );
    }

    (
        obj.get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        obj.get("description")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        obj.get("parameters").cloned().unwrap_or_else(|| json!({})),
        obj.get("strict").cloned(),
    )
}

fn extract_tool_name(obj: &Map<String, Value>) -> Option<String> {
    let name = extract_responses_tool_fields(obj).0;
    (!name.trim().is_empty()).then_some(name)
}

fn extract_tool_description(obj: &Map<String, Value>) -> Option<String> {
    extract_responses_tool_fields(obj).1
}

fn detect_custom_tool_kind(tool: &Map<String, Value>) -> CustomToolKind {
    let name = extract_tool_name(tool).unwrap_or_default();
    let grammar = tool
        .get("format")
        .and_then(|value| value.get("definition"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if name == "apply_patch"
        || (grammar.contains("begin_patch")
            && grammar.contains("end_patch")
            && grammar.contains("add_hunk"))
    {
        CustomToolKind::ApplyPatch
    } else {
        CustomToolKind::Raw
    }
}

fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        return name.to_string();
    }
    if name.is_empty() {
        return namespace.to_string();
    }
    if namespace.ends_with("__") || name.starts_with("__") {
        format!("{namespace}{name}")
    } else {
        format!("{namespace}__{name}")
    }
}

fn combine_namespace_description(
    namespace_description: Option<&str>,
    child_description: Option<&str>,
) -> Option<String> {
    let namespace_description = namespace_description.unwrap_or("").trim();
    let child_description = child_description.unwrap_or("").trim();
    match (
        namespace_description.is_empty(),
        child_description.is_empty(),
    ) {
        (true, true) => None,
        (true, false) => Some(child_description.to_string()),
        (false, true) => Some(namespace_description.to_string()),
        (false, false) => Some(format!("{namespace_description}\n\n{child_description}")),
    }
}

fn generic_custom_proxy_tool(name: &str, description: Option<&str>) -> Value {
    let description = match description.filter(|value| !value.trim().is_empty()) {
        Some(description) => {
            format!("{description}\n\nThis is a FREEFORM tool. Do not wrap the input in JSON or markdown.")
        }
        None => format!("FREEFORM custom tool: {name}. Put only the tool input text here."),
    };

    function_tool(
        name,
        Some(&description),
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "input": {
                    "type": "string",
                    "description": "Raw freeform input for this custom tool."
                }
            },
            "required": ["input"]
        }),
        None,
    )
}

fn function_tool(
    name: &str,
    description: Option<&str>,
    parameters: Value,
    strict: Option<Value>,
) -> Value {
    let mut function = json!({
        "name": name,
        "parameters": parameters
    });
    if let Some(description) = description.filter(|value| !value.is_empty()) {
        function["description"] = json!(description);
    }
    if let Some(strict) = strict {
        function["strict"] = strict;
    }
    json!({
        "type": "function",
        "function": function
    })
}

fn apply_patch_proxy_tools(name: &str, description: Option<&str>) -> Vec<Value> {
    vec![
        function_tool(
            &format!("{name}_add_file"),
            Some(&patch_proxy_description(
                description,
                "add_file",
                "Create one new file by providing a target path and full file content.",
            )),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {"type": "string", "description": "Target file path."},
                    "content": {"type": "string", "description": "Full file content without patch '+' prefixes."}
                },
                "required": ["path", "content"]
            }),
            None,
        ),
        function_tool(
            &format!("{name}_delete_file"),
            Some(&patch_proxy_description(
                description,
                "delete_file",
                "Delete one file by providing a target path.",
            )),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {"type": "string", "description": "Target file path."}
                },
                "required": ["path"]
            }),
            None,
        ),
        function_tool(
            &format!("{name}_update_file"),
            Some(&patch_proxy_description(
                description,
                "update_file",
                "Edit one existing file with structured hunks.",
            )),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {"type": "string", "description": "Target file path."},
                    "move_to": {"type": "string", "description": "Optional destination path for move operations."},
                    "hunks": patch_hunks_schema()
                },
                "required": ["path", "hunks"]
            }),
            None,
        ),
        function_tool(
            &format!("{name}_replace_file"),
            Some(&patch_proxy_description(
                description,
                "replace_file",
                "Replace one existing file by providing a target path and full new file content.",
            )),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {"type": "string", "description": "Target file path."},
                    "content": {"type": "string", "description": "Full replacement content."}
                },
                "required": ["path", "content"]
            }),
            None,
        ),
        function_tool(
            &format!("{name}_batch"),
            Some(&patch_proxy_description(
                description,
                "batch",
                "Edit files by providing structured JSON patch operations.",
            )),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "operations": {
                        "type": "array",
                        "description": "Ordered list of file patch operations.",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "type": {"type": "string", "enum": ["add_file", "delete_file", "update_file", "replace_file"]},
                                "path": {"type": "string"},
                                "move_to": {"type": "string", "description": "Optional destination path for move operations."},
                                "content": {"type": "string", "description": "Full file content for add_file / replace_file."},
                                "hunks": patch_hunks_schema()
                            },
                            "required": ["type", "path"]
                        }
                    }
                },
                "required": ["operations"]
            }),
            None,
        ),
    ]
}

fn patch_proxy_description(description: Option<&str>, action: &str, fallback: &str) -> String {
    match description.filter(|value| !value.trim().is_empty()) {
        Some(description) => format!("{description} (proxy action: {action})"),
        None => fallback.to_string(),
    }
}

fn patch_hunks_schema() -> Value {
    json!({
        "type": "array",
        "description": "Structured update hunks.",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "context": {"type": "string", "description": "Optional @@ context header text."},
                "lines": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "op": {"type": "string", "enum": ["context", "add", "remove"]},
                            "text": {"type": "string"}
                        },
                        "required": ["op", "text"]
                    }
                }
            },
            "required": ["lines"]
        }
    })
}

fn chat_function_choice(name: &str) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name
        }
    })
}

fn proxy_action_from_name(name: &str) -> Option<&'static str> {
    if name.ends_with("_add_file") {
        Some("add_file")
    } else if name.ends_with("_delete_file") {
        Some("delete_file")
    } else if name.ends_with("_update_file") {
        Some("update_file")
    } else if name.ends_with("_replace_file") {
        Some("replace_file")
    } else if name.ends_with("_batch") {
        Some("batch")
    } else {
        None
    }
}

fn apply_patch_input_from_proxy_arguments(raw_arguments: &str, action: &str) -> String {
    let Ok(mut parsed) = serde_json::from_str::<Value>(raw_arguments) else {
        return raw_arguments.to_string();
    };

    if !action.is_empty() {
        if let Some(input) = parsed.get("input").and_then(|value| value.as_str()) {
            if let Ok(nested) = serde_json::from_str::<Value>(input) {
                if let (Some(parsed_obj), Some(nested_obj)) =
                    (parsed.as_object_mut(), nested.as_object())
                {
                    for (key, value) in nested_obj {
                        parsed_obj
                            .entry(key.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
        }
    }

    let Some(obj) = parsed.as_object() else {
        return raw_arguments.to_string();
    };

    let mut ops = Vec::new();
    match action {
        "add_file" => ops.push(PatchOperation {
            op_type: "add_file".to_string(),
            path: string_field(obj, "path"),
            content: string_field(obj, "content"),
            ..Default::default()
        }),
        "delete_file" => ops.push(PatchOperation {
            op_type: "delete_file".to_string(),
            path: string_field(obj, "path"),
            ..Default::default()
        }),
        "update_file" => ops.push(PatchOperation {
            op_type: "update_file".to_string(),
            path: string_field(obj, "path"),
            move_to: string_field(obj, "move_to"),
            hunks: parse_hunks_value(obj.get("hunks")),
            ..Default::default()
        }),
        "replace_file" => ops.push(PatchOperation {
            op_type: "replace_file".to_string(),
            path: string_field(obj, "path"),
            content: string_field(obj, "content"),
            ..Default::default()
        }),
        "batch" => {
            if let Some(items) = obj.get("operations").and_then(|value| value.as_array()) {
                for item in items {
                    let Some(item_obj) = item.as_object() else {
                        continue;
                    };
                    ops.push(PatchOperation {
                        op_type: string_field(item_obj, "type"),
                        path: string_field(item_obj, "path"),
                        move_to: string_field(item_obj, "move_to"),
                        content: string_field(item_obj, "content"),
                        hunks: parse_hunks_value(item_obj.get("hunks")),
                    });
                }
            }
        }
        _ => {
            return obj
                .get("input")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| raw_arguments.to_string());
        }
    }

    if ops.is_empty() {
        raw_arguments.to_string()
    } else {
        build_patch_input(&ops)
    }
}

fn parse_patch_operations(input: &str) -> Option<Vec<PatchOperation>> {
    if input.is_empty() || !input.starts_with("*** Begin Patch") {
        return None;
    }

    let mut ops = Vec::<PatchOperation>::new();
    let mut current: Option<PatchOperation> = None;

    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line == "*** Begin Patch" || line == "*** End Patch" {
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            if let Some(op) = current.take() {
                ops.push(op);
            }
            current = Some(PatchOperation {
                op_type: "add_file".to_string(),
                path: path.to_string(),
                ..Default::default()
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            if let Some(op) = current.take() {
                ops.push(op);
            }
            current = Some(PatchOperation {
                op_type: "delete_file".to_string(),
                path: path.to_string(),
                ..Default::default()
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Update File: ") {
            if let Some(op) = current.take() {
                ops.push(op);
            }
            current = Some(PatchOperation {
                op_type: "update_file".to_string(),
                path: path.to_string(),
                ..Default::default()
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Move to: ") {
            if let Some(op) = current.as_mut().filter(|op| op.op_type == "update_file") {
                op.move_to = path.to_string();
            }
            continue;
        }

        if line.starts_with("@@") {
            if let Some(op) = current.as_mut().filter(|op| op.op_type == "update_file") {
                op.hunks.push(PatchHunk {
                    context: line.trim_start_matches("@@").trim().to_string(),
                    lines: Vec::new(),
                });
            }
            continue;
        }

        if let Some(op) = current.as_mut() {
            match op.op_type.as_str() {
                "add_file" => {
                    if let Some(text) = line.strip_prefix('+') {
                        op.content.push_str(text);
                        op.content.push('\n');
                    }
                }
                "update_file" => {
                    if let Some(hunk) = op.hunks.last_mut() {
                        if let Some((prefix, op_name)) = line_op_from_prefix(line) {
                            hunk.lines.push(PatchLine {
                                op: op_name.to_string(),
                                text: line.strip_prefix(prefix).unwrap_or(line).to_string(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(op) = current {
        ops.push(op);
    }

    (!ops.is_empty()).then_some(ops)
}

fn build_patch_input(ops: &[PatchOperation]) -> String {
    let mut out = String::from("*** Begin Patch\n");
    for op in ops {
        match op.op_type.as_str() {
            "add_file" => {
                out.push_str("*** Add File: ");
                out.push_str(&op.path);
                out.push('\n');
                write_added_content(&mut out, &op.content);
            }
            "delete_file" => {
                out.push_str("*** Delete File: ");
                out.push_str(&op.path);
                out.push('\n');
            }
            "update_file" => {
                out.push_str("*** Update File: ");
                out.push_str(&op.path);
                out.push('\n');
                if !op.move_to.is_empty() {
                    out.push_str("*** Move to: ");
                    out.push_str(&op.move_to);
                    out.push('\n');
                }
                for hunk in &op.hunks {
                    if hunk.context.is_empty() {
                        out.push_str("@@\n");
                    } else {
                        out.push_str("@@ ");
                        out.push_str(&hunk.context);
                        out.push('\n');
                    }
                    for line in &hunk.lines {
                        out.push_str(line_op_prefix(&line.op));
                        out.push_str(&line.text);
                        out.push('\n');
                    }
                }
            }
            "replace_file" => {
                out.push_str("*** Delete File: ");
                out.push_str(&op.path);
                out.push('\n');
                out.push_str("*** Add File: ");
                out.push_str(&op.path);
                out.push('\n');
                write_added_content(&mut out, &op.content);
            }
            _ => {}
        }
    }
    out.push_str("*** End Patch");
    out
}

fn write_added_content(out: &mut String, content: &str) {
    let content = content.trim_end_matches('\n');
    if content.is_empty() {
        return;
    }
    for line in content.split('\n') {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
}

fn single_patch_operation_args(op: &PatchOperation) -> String {
    let value = match op.op_type.as_str() {
        "add_file" | "replace_file" => json!({
            "path": op.path,
            "content": op.content
        }),
        "delete_file" => json!({
            "path": op.path
        }),
        "update_file" => {
            let mut value = json!({
                "path": op.path,
                "hunks": hunks_to_value(&op.hunks)
            });
            if !op.move_to.is_empty() {
                value["move_to"] = json!(op.move_to);
            }
            value
        }
        _ => json!({
            "path": op.path
        }),
    };
    canonical_json_string(&value)
}

fn batch_patch_operations_args(ops: &[PatchOperation]) -> String {
    let operations = ops
        .iter()
        .map(|op| {
            let mut value = json!({
                "type": op.op_type,
                "path": op.path
            });
            if !op.move_to.is_empty() {
                value["move_to"] = json!(op.move_to);
            }
            if !op.content.is_empty() {
                value["content"] = json!(op.content);
            }
            if !op.hunks.is_empty() {
                value["hunks"] = hunks_to_value(&op.hunks);
            }
            value
        })
        .collect::<Vec<_>>();
    canonical_json_string(&json!({ "operations": operations }))
}

fn hunks_to_value(hunks: &[PatchHunk]) -> Value {
    Value::Array(
        hunks
            .iter()
            .map(|hunk| {
                json!({
                    "context": hunk.context,
                    "lines": hunk.lines.iter().map(|line| {
                        json!({
                            "op": line.op,
                            "text": line.text
                        })
                    }).collect::<Vec<_>>()
                })
            })
            .collect(),
    )
}

fn parse_hunks_value(raw: Option<&Value>) -> Vec<PatchHunk> {
    raw.and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    Some(PatchHunk {
                        context: string_field(obj, "context"),
                        lines: obj
                            .get("lines")
                            .and_then(|value| value.as_array())
                            .map(|lines| {
                                lines
                                    .iter()
                                    .filter_map(|line| {
                                        let obj = line.as_object()?;
                                        Some(PatchLine {
                                            op: string_field(obj, "op"),
                                            text: string_field(obj, "text"),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn choose_single_proxy_action(op_type: &str) -> &'static str {
    match op_type {
        "add_file" => "add_file",
        "delete_file" => "delete_file",
        "update_file" => "update_file",
        "replace_file" => "replace_file",
        _ => "batch",
    }
}

fn string_field(obj: &Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

fn line_op_prefix(op: &str) -> &'static str {
    match op {
        "add" => "+",
        "remove" | "delete" => "-",
        _ => " ",
    }
}

fn line_op_from_prefix(line: &str) -> Option<(&str, &'static str)> {
    if line.starts_with(' ') {
        Some((" ", "context"))
    } else if line.starts_with('+') {
        Some(("+", "add"))
    } else if line.starts_with('-') {
        Some(("-", "remove"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_custom_and_namespace_tools_to_chat_functions() {
        let tools = vec![
            json!({
                "type": "custom",
                "name": "apply_patch",
                "description": "Patch files"
            }),
            json!({
                "type": "namespace",
                "name": "mcp__editor__",
                "description": "Editor actions",
                "tools": [{
                    "type": "function",
                    "name": "save_all",
                    "description": "Save files",
                    "parameters": {"type": "object"}
                }]
            }),
            json!("exec_command"),
        ];

        let ctx = ToolCompatContext::from_tools(&tools);
        let converted = ctx.convert_tools_to_chat(&tools);
        let names = converted
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();

        assert!(names.contains(&"apply_patch_add_file".to_string()));
        assert!(names.contains(&"apply_patch_batch".to_string()));
        assert!(names.contains(&"mcp__editor__save_all".to_string()));
        assert!(names.contains(&"exec_command".to_string()));
    }

    #[test]
    fn rebuilds_patch_input_from_proxy_arguments() {
        let ctx = ToolCompatContext::from_tools(&[json!({
            "type": "custom",
            "name": "apply_patch"
        })]);

        let item = ctx
            .custom_response_item(
                "call_1",
                "apply_patch_add_file",
                r##"{"path":"docs/test.md","content":"# Test\n"}"##,
            )
            .unwrap();

        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["name"], "apply_patch");
        assert_eq!(
            item["input"],
            "*** Begin Patch\n*** Add File: docs/test.md\n+# Test\n*** End Patch"
        );
        assert!(item.get("arguments").is_none());
    }

    #[test]
    fn restores_string_patch_proxy_tool_calls() {
        let ctx = ToolCompatContext::from_tools(&[
            json!("apply_patch_add_file"),
            json!("apply_patch_batch"),
            json!("exec_command"),
        ]);

        let patch_item = ctx
            .custom_response_item(
                "call_patch",
                "apply_patch_add_file",
                r##"{"path":"docs/test.md","content":"# Test\n"}"##,
            )
            .unwrap();
        let exec_item = ctx
            .custom_response_item(
                "call_exec",
                "exec_command",
                r##"{"input":"Get-ChildItem"}"##,
            )
            .unwrap();

        assert_eq!(patch_item["name"], "apply_patch");
        assert_eq!(
            patch_item["input"],
            "*** Begin Patch\n*** Add File: docs/test.md\n+# Test\n*** End Patch"
        );
        assert_eq!(exec_item["name"], "exec_command");
        assert_eq!(exec_item["input"], "Get-ChildItem");
    }

    #[test]
    fn converts_builtin_tools_to_generic_function_proxies() {
        let tools = vec![
            json!({"type": "web_search"}),
            json!({"type": "local_shell", "name": "shell"}),
            json!({"type": "computer_use", "name": "desktop"}),
        ];
        let ctx = ToolCompatContext::from_tools(&tools);
        let converted = ctx.convert_tools_to_chat(&tools);
        let names = converted
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();

        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"shell".to_string()));
        assert!(names.contains(&"desktop".to_string()));
    }

    #[test]
    fn maps_history_custom_call_to_proxy_arguments() {
        let ctx = ToolCompatContext::from_tools(&[json!({
            "type": "custom",
            "name": "apply_patch"
        })]);

        let (name, args) = ctx.custom_history_arguments(
            "apply_patch",
            "*** Begin Patch\n*** Add File: docs/test.md\n+# Test\n*** End Patch",
        );
        let args_value: Value = serde_json::from_str(&args).unwrap();

        assert_eq!(name, "apply_patch_add_file");
        assert_eq!(args_value["path"], "docs/test.md");
        assert_eq!(args_value["content"], "# Test\n");
    }

    #[test]
    fn restores_namespace_on_response_items() {
        let ctx = ToolCompatContext::from_tools(&[json!({
            "type": "namespace",
            "name": "mcp__editor__",
            "tools": [{
                "type": "function",
                "name": "save_all",
                "parameters": {"type": "object"}
            }]
        })]);
        let mut item = json!({
            "type": "function_call",
            "name": "mcp__editor__save_all",
            "arguments": "{}"
        });

        ctx.apply_namespace_to_response_item(&mut item);

        assert_eq!(item["name"], "save_all");
        assert_eq!(item["namespace"], "mcp__editor__");
    }

    #[test]
    fn converts_custom_tool_choice_to_proxy_function() {
        let ctx = ToolCompatContext::from_tools(&[json!({
            "type": "custom",
            "name": "apply_patch"
        })]);

        let choice = ctx
            .convert_tool_choice(&json!({"type": "custom", "name": "apply_patch"}))
            .unwrap();

        assert_eq!(choice["function"]["name"], "apply_patch_batch");
    }
}
