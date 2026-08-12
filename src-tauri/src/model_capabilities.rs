use serde_json::Value;

/// Image-input capability shared by Codex catalog generation and proxy request
/// rectification.
///
/// `Unknown` is intentionally distinct from `Supported`: callers may choose
/// different execution policies without duplicating the model-name registry.
/// The Codex catalog treats unknown models as image-capable (fail open), while
/// the media rectifier leaves their request bodies untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageInputCapability {
    Supported,
    Unsupported,
    Unknown,
}

/// Resolve image-input capability from an explicit declaration first, then the
/// confirmed text-only model registry when the caller enables registry lookup.
pub(crate) fn resolve_image_input_capability(
    model: &str,
    declared_support: Option<bool>,
    use_confirmed_registry: bool,
) -> ImageInputCapability {
    match declared_support {
        Some(true) => ImageInputCapability::Supported,
        Some(false) => ImageInputCapability::Unsupported,
        None if use_confirmed_registry && is_confirmed_text_only_model(model) => {
            ImageInputCapability::Unsupported
        }
        None => ImageInputCapability::Unknown,
    }
}

/// Resolve a model's image-input capability from the provider settings shapes
/// accepted by the proxy (`modelCatalog.models`, `modelCatalog`, or `models`).
pub(crate) fn image_input_capability_from_settings(
    settings: &Value,
    model: &str,
    use_confirmed_registry: bool,
) -> ImageInputCapability {
    resolve_image_input_capability(
        model,
        declared_model_image_support(settings, model),
        use_confirmed_registry,
    )
}

/// Convert a catalog row's explicit modality list into the shared capability
/// representation, falling back to the text-only registry when omitted.
pub(crate) fn image_input_capability_from_modalities(
    model: &str,
    modalities: Option<&[String]>,
) -> ImageInputCapability {
    let declared_support = modalities.map(|items| {
        items
            .iter()
            .any(|item| item.trim().eq_ignore_ascii_case("image"))
    });
    resolve_image_input_capability(model, declared_support, true)
}

/// Models that CC Switch is willing to advertise to clients as text-only.
///
/// This registry is deliberately exact and fail-open. A new suffix is not
/// inherited automatically: it remains image-capable until its capability is
/// confirmed, preventing a future `-vision`/`-vl` variant from being blocked by
/// the Codex client before a request can reach the proxy.
pub(crate) fn is_confirmed_text_only_model(model: &str) -> bool {
    let normalized = normalize_model_id(model);
    let tail = normalized.rsplit('/').next().unwrap_or(normalized.as_str());

    const CONFIRMED_TAILS: &[&str] = &[
        "ark-code-latest",
        "deepseek-chat",
        "deepseek-reasoner",
        "deepseek-v4-flash",
        "deepseek-v4-pro",
        "glm-5.1",
        // Exact rather than prefix matching: GLM visual models use a `v`
        // suffix (for example glm-5.2v), which must remain image-capable.
        "glm-5.2",
        "kat-coder",
        "kat-coder-pro",
        "kat-coder-pro v1",
        "kat-coder-pro v2",
        "kat-coder-pro-v1",
        "kat-coder-pro-v2",
        "ling-2.5-1t",
        "longcat-2.0",
        "longcat-flash-chat",
        "minimax-m2.7",
        "minimax-m2.7-highspeed",
        "mimo-v2.5-pro",
        "qwen3-coder-480b",
        "qwen3-coder-480b-a35b-instruct",
        "qwen3-coder-flash",
        "qwen3-coder-next",
        "qwen3-coder-plus",
        "step-3.5-flash",
        "step-3.5-flash-2603",
        "us.deepseek.r1-v1",
    ];

    CONFIRMED_TAILS.contains(&tail)
}

fn declared_model_image_support(settings: &Value, model: &str) -> Option<bool> {
    [
        settings
            .get("modelCatalog")
            .and_then(|catalog| catalog.get("models")),
        settings.get("modelCatalog"),
        settings.get("models"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| declared_model_image_support_in_value(value, model))
}

fn declared_model_image_support_in_value(value: &Value, model: &str) -> Option<bool> {
    if let Some(models) = value.as_array() {
        return models.iter().find_map(|entry| {
            model_entry_matches(entry, None, model).then(|| explicit_image_support(entry))?
        });
    }

    let object = value.as_object()?;
    object.iter().find_map(|(key, entry)| {
        model_entry_matches(entry, Some(key), model).then(|| explicit_image_support(entry))?
    })
}

fn explicit_image_support(entry: &Value) -> Option<bool> {
    if let Some(value) = entry
        .get("supportsImage")
        .or_else(|| entry.get("supports_image"))
        .or_else(|| entry.get("vision"))
        .and_then(Value::as_bool)
    {
        return Some(value);
    }

    [
        entry.get("input"),
        entry.pointer("/modalities/input"),
        entry.get("input_modalities"),
        entry.get("inputModalities"),
    ]
    .into_iter()
    .flatten()
    .find_map(input_modalities_support_image)
}

fn input_modalities_support_image(value: &Value) -> Option<bool> {
    let modalities = value.as_array()?;
    Some(modalities.iter().any(|item| {
        item.as_str()
            .map(str::trim)
            .is_some_and(|item| item.eq_ignore_ascii_case("image"))
    }))
}

fn model_entry_matches(entry: &Value, key: Option<&str>, model: &str) -> bool {
    key.is_some_and(|key| model_ids_match(key, model))
        || ["model", "id", "name"]
            .into_iter()
            .filter_map(|field| entry.get(field).and_then(Value::as_str))
            .any(|candidate| model_ids_match(candidate, model))
}

fn model_ids_match(candidate: &str, model: &str) -> bool {
    let candidate = normalize_model_id(candidate);
    let model = normalize_model_id(model);
    if candidate.is_empty() || model.is_empty() {
        return false;
    }
    if candidate == model {
        return true;
    }

    let candidate_tail = candidate.rsplit('/').next().unwrap_or(candidate.as_str());
    let model_tail = model.rsplit('/').next().unwrap_or(model.as_str());
    candidate_tail == model_tail || candidate == model_tail || candidate_tail == model
}

fn normalize_model_id(value: &str) -> String {
    let mut normalized = value
        .trim()
        .trim_start_matches("models/")
        .trim()
        .to_ascii_lowercase();
    if let Some(stripped) =
        normalized.strip_suffix(crate::claude_desktop_config::ONE_M_CONTEXT_MARKER)
    {
        normalized = stripped.trim().to_string();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gpt_and_unknown_models_remain_unknown_without_declarations() {
        for model in ["gpt-5.4", "gpt-5.5", "gpt-5.6-sol", "custom-alias"] {
            assert_eq!(
                resolve_image_input_capability(model, None, true),
                ImageInputCapability::Unknown,
                "{model} must fail open"
            );
        }
    }

    #[test]
    fn confirmed_text_only_registry_normalizes_namespaces_and_context_markers() {
        assert!(is_confirmed_text_only_model("deepseek/deepseek-v4-pro"));
        assert!(is_confirmed_text_only_model("GLM-5.2[1M]"));
        assert!(is_confirmed_text_only_model("qwen/qwen3-coder-plus"));
        assert!(is_confirmed_text_only_model(
            "Qwen/Qwen3-Coder-480B-A35B-Instruct"
        ));
        assert!(is_confirmed_text_only_model("MiniMax-M2.7-Highspeed"));
        assert!(is_confirmed_text_only_model("step-3.5-flash-2603"));
        assert!(!is_confirmed_text_only_model("glm-5.2v"));
    }

    #[test]
    fn unconfirmed_family_suffixes_fail_open() {
        for model in [
            "minimax-m2.7-vision",
            "qwen3-coder-ultra",
            "qwen3-coder-vl",
            "step-3.5-flash-vision",
        ] {
            assert!(
                !is_confirmed_text_only_model(model),
                "unconfirmed variant {model} must not be hard-gated"
            );
        }
    }

    #[test]
    fn explicit_capability_overrides_the_registry() {
        assert_eq!(
            resolve_image_input_capability("deepseek-v4-pro", Some(true), true),
            ImageInputCapability::Supported
        );
        assert_eq!(
            resolve_image_input_capability("gpt-5.4", Some(false), true),
            ImageInputCapability::Unsupported
        );
    }

    #[test]
    fn provider_settings_support_multiple_capability_shapes() {
        let settings = json!({
            "modelCatalog": {
                "models": [
                    { "model": "vision", "modalities": { "input": ["text", "image"] } },
                    { "model": "text", "inputModalities": ["text"] }
                ]
            }
        });

        assert_eq!(
            image_input_capability_from_settings(&settings, "vision", true),
            ImageInputCapability::Supported
        );
        assert_eq!(
            image_input_capability_from_settings(&settings, "text", true),
            ImageInputCapability::Unsupported
        );
    }
}
