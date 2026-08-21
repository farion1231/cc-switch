use super::model::{is_managed_group, CopilotByokGroup, CopilotByokModel};
use super::store::CopilotByokStore;
use super::sync;
use crate::error::AppError;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokImportResult {
    pub target_id: String,
    pub imported_group_count: usize,
    pub imported_model_count: usize,
    pub reused_model_count: usize,
    pub skipped_group_count: usize,
    pub changed_target_count: usize,
    pub warnings: Vec<String>,
}

pub(crate) struct PreparedImport {
    pub result: CopilotByokImportResult,
    pub original_store: CopilotByokStore,
    pub updated_store: CopilotByokStore,
    pub overrides: sync::TransactionOverrides,
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn value_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

fn value_bool(value: Option<&Value>, default: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(default)
}

fn string_array(value: Option<&Value>, field: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("{field} must be an array of strings"))?;
    if values.iter().any(|value| !value.is_string()) {
        return Err(format!("{field} must be an array of strings"));
    }
    Ok(values
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect())
}

fn string_map(value: Option<&Value>, field: &str) -> Result<BTreeMap<String, String>, String> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object with string values"))?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_string()))
                .ok_or_else(|| format!("{field}.{key} must be a string"))
        })
        .collect()
}

fn require_optional_type(
    object: &Map<String, Value>,
    field: &str,
    expected: &str,
    predicate: impl FnOnce(&Value) -> bool,
) -> Result<(), String> {
    if object.get(field).is_some_and(|value| !predicate(value)) {
        return Err(format!("{field} must be {expected}"));
    }
    Ok(())
}

fn validate_model_shapes(object: &Map<String, Value>) -> Result<(), String> {
    for field in [
        "id",
        "name",
        "url",
        "apiKey",
        "apiType",
        "reasoningEffortFormat",
    ] {
        require_optional_type(object, field, "a string", Value::is_string)?;
    }
    for field in [
        "toolCalling",
        "vision",
        "thinking",
        "streaming",
        "zeroDataRetentionEnabled",
    ] {
        require_optional_type(object, field, "a boolean", Value::is_boolean)?;
    }
    for field in ["contextWindow", "maxInputTokens", "maxOutputTokens"] {
        require_optional_type(object, field, "a positive safe integer", |value| {
            value
                .as_u64()
                .is_some_and(|number| number > 0 && number <= MAX_SAFE_JSON_INTEGER)
        })?;
    }
    require_optional_type(object, "modelOptions", "an object", Value::is_object)?;
    Ok(())
}

fn unknown_fields(object: &Map<String, Value>, known: &[&str]) -> BTreeMap<String, Value> {
    object
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn infer_api_type(url: &str) -> String {
    if url.contains("/messages") {
        "messages".to_string()
    } else if url.contains("/responses") {
        "responses".to_string()
    } else {
        "chat-completions".to_string()
    }
}

fn deterministic_id(parts: &[&str]) -> String {
    let digest = Sha256::digest(parts.join("\0").as_bytes());
    let encoded = format!("{digest:x}");
    format!("vscode-import:{}", &encoded[..24])
}

fn parse_model(
    target_id: &str,
    group_name: &str,
    url: &str,
    object: &Map<String, Value>,
) -> Result<CopilotByokModel, String> {
    validate_model_shapes(object)?;
    let model_id =
        value_string(object.get("id")).ok_or_else(|| "model entry is missing id".to_string())?;
    let name = value_string(object.get("name")).unwrap_or_else(|| model_id.clone());
    let mut model = CopilotByokModel {
        id: deterministic_id(&[target_id, group_name, &model_id, url]),
        model_id,
        name,
        enabled: true,
        tool_calling: object.get("toolCalling").and_then(Value::as_bool),
        vision: object.get("vision").and_then(Value::as_bool),
        thinking: object.get("thinking").and_then(Value::as_bool),
        streaming: object.get("streaming").and_then(Value::as_bool),
        context_window: value_u64(object.get("contextWindow")),
        max_input_tokens: value_u64(object.get("maxInputTokens")),
        max_output_tokens: value_u64(object.get("maxOutputTokens")),
        edit_tools: string_array(object.get("editTools"), "editTools")?,
        zero_data_retention_enabled: value_bool(object.get("zeroDataRetentionEnabled"), false),
        supports_reasoning_effort: string_array(
            object.get("supportsReasoningEffort"),
            "supportsReasoningEffort",
        )?,
        reasoning_effort_format: value_string(object.get("reasoningEffortFormat")),
        model_options: object
            .get("modelOptions")
            .cloned()
            .unwrap_or_else(|| json!({})),
        extra: unknown_fields(
            object,
            &[
                "id",
                "name",
                "url",
                "apiKey",
                "apiType",
                "requestHeaders",
                "toolCalling",
                "vision",
                "thinking",
                "streaming",
                "contextWindow",
                "maxInputTokens",
                "maxOutputTokens",
                "editTools",
                "zeroDataRetentionEnabled",
                "supportsReasoningEffort",
                "reasoningEffortFormat",
                "modelOptions",
            ],
        ),
    };
    model.normalize();
    model.validate().map_err(|error| error.to_string())?;
    Ok(model)
}

fn shared_value(
    group: &Map<String, Value>,
    models: &[Map<String, Value>],
    field: &str,
    fallback: Option<String>,
) -> Result<String, String> {
    let group_value = value_string(group.get(field));
    let mut values = HashSet::new();
    for model in models {
        if let Some(value) = value_string(model.get(field)).or_else(|| group_value.clone()) {
            values.insert(value);
        }
    }
    if values.len() > 1 {
        return Err(format!(
            "models use different {field} values; a BYOK provider group must share one connection"
        ));
    }
    values
        .into_iter()
        .next()
        .or(group_value)
        .or(fallback)
        .ok_or_else(|| format!("provider group is missing {field}"))
}

fn parse_group(target_id: &str, value: &Value) -> Result<CopilotByokGroup, String> {
    let group = value
        .as_object()
        .ok_or_else(|| "provider group is not a JSON object".to_string())?;
    for field in [
        "name",
        "vendor",
        "url",
        "apiKey",
        "apiType",
        "ccSwitchGroupId",
    ] {
        require_optional_type(group, field, "a string", Value::is_string)?;
    }
    require_optional_type(group, "ccSwitchManaged", "a boolean", Value::is_boolean)?;
    let name = value_string(group.get("name")).unwrap_or_else(|| "Custom Endpoint".to_string());
    let values = group
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{name} has no static models array"))?;
    if values.is_empty() {
        return Err(format!("{name} has an empty models array"));
    }
    let model_objects = values
        .iter()
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or_else(|| format!("{name} contains a model that is not a JSON object"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let url = shared_value(group, &model_objects, "url", None)?;
    let api_key = shared_value(group, &model_objects, "apiKey", Some(String::new()))?;
    let api_type = shared_value(group, &model_objects, "apiType", Some(infer_api_type(&url)))?;

    let group_headers = string_map(group.get("requestHeaders"), "requestHeaders")?;
    let mut shared_headers: Option<BTreeMap<String, String>> = None;
    for model in &model_objects {
        validate_model_shapes(model)?;
        let mut headers = group_headers.clone();
        headers.extend(string_map(
            model.get("requestHeaders"),
            "model.requestHeaders",
        )?);
        if let Some(existing) = &shared_headers {
            if existing != &headers {
                return Err(format!(
                    "{name} models use different requestHeaders; a BYOK provider group must share one connection"
                ));
            }
        } else {
            shared_headers = Some(headers);
        }
    }

    let models = model_objects
        .iter()
        .map(|model| parse_model(target_id, &name, &url, model))
        .collect::<Result<Vec<_>, _>>()?;
    let mut parsed = CopilotByokGroup {
        id: deterministic_id(&[target_id, &name, &url]),
        name,
        url,
        api_key,
        api_type,
        website_url: None,
        notes: None,
        icon: None,
        icon_color: None,
        category: None,
        usage_script: None,
        enabled: true,
        request_headers: shared_headers.unwrap_or_default(),
        models,
        extra: unknown_fields(
            group,
            &[
                "name",
                "vendor",
                "url",
                "apiKey",
                "apiType",
                "requestHeaders",
                "models",
                "ccSwitchManaged",
                "ccSwitchGroupId",
            ],
        ),
    };
    parsed.normalize();
    parsed.validate().map_err(|error| error.to_string())?;
    Ok(parsed)
}

fn equivalent_group(left: &CopilotByokGroup, right: &CopilotByokGroup) -> bool {
    let comparable = |group: &CopilotByokGroup| {
        let mut group = group.clone();
        group.id.clear();
        group.website_url = None;
        group.notes = None;
        group.icon = None;
        group.icon_color = None;
        group.usage_script = None;
        for model in &mut group.models {
            model.id.clear();
        }
        group
    };
    comparable(left) == comparable(right)
}

fn add_group(
    store: &mut CopilotByokStore,
    mut group: CopilotByokGroup,
) -> Result<(usize, usize), String> {
    if let Some(existing) = store
        .groups
        .iter()
        .find(|existing| equivalent_group(existing, &group))
    {
        return Ok((0, existing.models.len()));
    }

    let existing_ids: HashSet<String> = store.groups.iter().map(|item| item.id.clone()).collect();
    if existing_ids.contains(&group.id) {
        group.id = uuid::Uuid::new_v4().to_string();
    }

    let names: HashSet<String> = store
        .groups
        .iter()
        .map(|item| item.name.to_lowercase())
        .collect();
    if names.contains(&group.name.to_lowercase()) {
        let base = format!("{} · imported", group.name);
        group.name = base.clone();
        let mut suffix = 2;
        while names.contains(&group.name.to_lowercase()) {
            group.name = format!("{base} {suffix}");
            suffix += 1;
        }
    }

    let imported = group.models.len();
    group.validate().map_err(|error| error.to_string())?;
    store.groups.push(group);
    Ok((imported, 0))
}

pub fn prepare_import_from_target(
    mut store: CopilotByokStore,
    target_id: &str,
) -> Result<PreparedImport, AppError> {
    let original_store = store.clone();
    let resolved = sync::resolve_target_paths(&store, &[target_id.to_string()])?;
    let (_, path) = resolved
        .into_iter()
        .next()
        .ok_or_else(|| AppError::InvalidInput("Copilot BYOK target is required".to_string()))?;
    let groups = sync::read_language_model_groups(&path)?;

    let mut accepted_indexes = HashSet::new();
    let mut imported_group_count = 0;
    let mut imported_model_count = 0;
    let mut reused_model_count = 0;
    let mut skipped_group_count = 0;
    let mut warnings = Vec::new();

    for (index, group) in groups.iter().enumerate() {
        if is_managed_group(group)
            || group.get("vendor").and_then(Value::as_str) != Some("customendpoint")
        {
            continue;
        }

        let parsed = parse_group(target_id, group);
        let secret_reference_group = parsed.as_ref().ok().and_then(|parsed| {
            parsed
                .api_key
                .starts_with("${input:")
                .then(|| parsed.name.clone())
        });

        match parsed.and_then(|parsed| add_group(&mut store, parsed)) {
            Ok((imported, reused)) => {
                if let Some(group_name) = secret_reference_group {
                    warnings.push(format!(
                        "{group_name} keeps a VS Code SecretStorage reference; other profiles may need the secret to be entered again"
                    ));
                }
                accepted_indexes.insert(index);
                imported_group_count += 1;
                imported_model_count += imported;
                reused_model_count += reused;
            }
            Err(reason) => {
                skipped_group_count += 1;
                warnings.push(reason);
            }
        }
    }

    if accepted_indexes.is_empty() {
        return Ok(PreparedImport {
            result: CopilotByokImportResult {
                target_id: target_id.to_string(),
                imported_group_count,
                imported_model_count,
                reused_model_count,
                skipped_group_count,
                changed_target_count: 0,
                warnings,
            },
            updated_store: store,
            original_store,
            overrides: sync::TransactionOverrides::default(),
        });
    }

    store.targets_initialized = true;
    if !store.selected_target_ids.iter().any(|id| id == target_id) {
        store.selected_target_ids.push(target_id.to_string());
    }
    let unmanaged: Vec<Value> = groups
        .into_iter()
        .enumerate()
        .filter_map(|(index, group)| (!accepted_indexes.contains(&index)).then_some(group))
        .collect();
    let mut overrides = sync::TransactionOverrides::default();
    overrides
        .base_groups
        .insert(target_id.to_string(), unmanaged);
    Ok(PreparedImport {
        result: CopilotByokImportResult {
            target_id: target_id.to_string(),
            imported_group_count,
            imported_model_count,
            reused_model_count,
            skipped_group_count,
            changed_target_count: 0,
            warnings,
        },
        original_store,
        updated_store: store,
        overrides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_shared_connection_with_multiple_models() {
        let group = json!({
            "name": "Existing Kimi",
            "vendor": "customendpoint",
            "apiKey": "${input:chat.lm.secret.test}",
            "apiType": "responses",
            "models": [{
                "id": "kimi-k3",
                "name": "Kimi K3",
                "url": "https://api.example.com/v1/responses",
                "contextWindow": 262144,
                "maxOutputTokens": 32768,
                "toolCalling": true,
                "thinking": true,
                "editTools": ["apply-patch", "unsupported"],
                "supportsReasoningEffort": ["low", "high"],
                "reasoningEffortFormat": "responses",
                "zeroDataRetentionEnabled": true
            }, {
                "id": "kimi-k2",
                "name": "Kimi K2",
                "url": "https://api.example.com/v1/responses"
            }]
        });

        let parsed = parse_group("stable:default", &group).expect("parse group");
        assert_eq!(parsed.name, "Existing Kimi");
        assert_eq!(parsed.api_key, "${input:chat.lm.secret.test}");
        assert_eq!(parsed.api_type, "responses");
        assert_eq!(parsed.models.len(), 2);
        assert_eq!(
            parsed.models[0].edit_tools,
            vec!["apply-patch", "unsupported"]
        );
        assert_eq!(
            parsed.models[0].supports_reasoning_effort,
            vec!["low", "high"]
        );
        assert!(parsed.models[0].zero_data_retention_enabled);
        assert_eq!(parsed.models[0].tool_calling, Some(true));
        assert_eq!(parsed.models[1].tool_calling, Some(true));
        assert_eq!(parsed.models[1].context_window, None);

        let rendered = parsed.to_language_model_group();
        assert_eq!(rendered["apiKey"], "${input:chat.lm.secret.test}");
        assert!(rendered["models"][0].get("requestHeaders").is_none());
        assert_eq!(rendered["models"][1]["toolCalling"], true);
    }

    #[test]
    fn rejects_group_with_different_model_urls() {
        let group = json!({
            "name": "Mixed",
            "vendor": "customendpoint",
            "apiKey": "secret",
            "models": [
                {"id": "model-a", "url": "https://a.example.com/v1/responses"},
                {"id": "model-b", "url": "https://b.example.com/v1/responses"}
            ]
        });
        assert!(parse_group("stable:default", &group).is_err());
    }

    #[test]
    fn rejects_model_options_that_cannot_be_edited_as_an_object() {
        let group = json!({
            "name": "Invalid options",
            "vendor": "customendpoint",
            "models": [{
                "id": "model-a",
                "url": "https://api.example.com/v1/responses",
                "modelOptions": ["not", "an", "object"]
            }]
        });

        let error = parse_group("stable:default", &group).expect_err("reject array options");
        assert!(error.contains("modelOptions must be an object"));
    }

    #[test]
    fn equivalent_existing_groups_are_reused() {
        let group = parse_group(
            "stable:default",
            &json!({
                "name": "Existing",
                "vendor": "customendpoint",
                "apiKey": "secret",
                "models": [{
                    "id": "model-a",
                    "name": "Model A",
                    "url": "https://api.example.com/v1/chat/completions"
                }]
            }),
        )
        .expect("parse group");
        let mut store = CopilotByokStore::default();
        store.groups.push(group.clone());

        let (imported, reused) = add_group(&mut store, group).expect("reuse group");
        assert_eq!(imported, 0);
        assert_eq!(reused, 1);
        assert_eq!(store.groups.len(), 1);
    }

    #[test]
    fn groups_with_different_shared_credentials_are_not_reused() {
        let group = parse_group(
            "stable:default",
            &json!({
                "name": "Existing",
                "vendor": "customendpoint",
                "apiKey": "first-secret",
                "models": [{
                    "id": "model-a",
                    "name": "Model A",
                    "url": "https://api.example.com/v1/chat/completions"
                }]
            }),
        )
        .expect("parse group");
        let mut imported = group.clone();
        imported.api_key = "second-secret".to_string();
        let mut store = CopilotByokStore::default();
        store.groups.push(group);

        let (imported_count, reused_count) =
            add_group(&mut store, imported).expect("keep distinct group");
        assert_eq!(imported_count, 1);
        assert_eq!(reused_count, 0);
        assert_eq!(store.groups.len(), 2);
        assert_eq!(store.groups[1].name, "Existing · imported");
    }

    #[test]
    fn preserves_unknown_provider_and_model_fields() {
        let parsed = parse_group(
            "stable:default",
            &json!({
                "name": "Future provider",
                "vendor": "customendpoint",
                "apiKey": "",
                "futureProviderOption": {"enabled": true},
                "models": [{
                    "id": "future-model",
                    "name": "Future Model",
                    "url": "https://api.example.com/v1/responses",
                    "futureModelOption": [1, 2, 3],
                    "editTools": ["future-edit-tool"]
                }]
            }),
        )
        .expect("parse future fields");

        let rendered = parsed.to_language_model_group();
        assert_eq!(rendered["futureProviderOption"]["enabled"], true);
        assert_eq!(rendered["models"][0]["futureModelOption"], json!([1, 2, 3]));
        assert_eq!(rendered["models"][0]["editTools"][0], "future-edit-tool");
    }

    #[test]
    fn rejects_known_fields_with_incompatible_shapes() {
        for invalid in [
            json!({
                "name": "Bad headers",
                "vendor": "customendpoint",
                "requestHeaders": {"Authorization": true},
                "models": [{
                    "id": "model-a",
                    "url": "https://api.example.com/v1/responses"
                }]
            }),
            json!({
                "name": "Bad tools",
                "vendor": "customendpoint",
                "models": [{
                    "id": "model-a",
                    "url": "https://api.example.com/v1/responses",
                    "editTools": ["apply-patch", {"future": true}]
                }]
            }),
            json!({
                "name": "Bad capability",
                "vendor": "customendpoint",
                "models": [{
                    "id": "model-a",
                    "url": "https://api.example.com/v1/responses",
                    "toolCalling": "yes"
                }]
            }),
            json!({
                "name": "Zero token limit",
                "vendor": "customendpoint",
                "models": [{
                    "id": "model-a",
                    "url": "https://api.example.com/v1/responses",
                    "contextWindow": 0
                }]
            }),
            json!({
                "name": "Unsafe token limit",
                "vendor": "customendpoint",
                "models": [{
                    "id": "model-a",
                    "url": "https://api.example.com/v1/responses",
                    "maxOutputTokens": 9_007_199_254_740_992_u64
                }]
            }),
        ] {
            assert!(parse_group("stable:default", &invalid).is_err());
        }
    }
}
