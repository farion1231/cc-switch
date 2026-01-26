//! Configuration merge utilities for the common config redesign.
//!
//! This module provides functions for:
//! - `compute_final_config`: Merges common config (base) with custom config (override)
//! - `extract_difference`: Extracts custom parts from live config by comparing with common config
//!
//! Supports JSON (Claude, Gemini) and TOML (Codex) formats.

// Allow dead code as this is a utility module with functions available for future use
#![allow(dead_code)]

use serde_json::{Map, Value as JsonValue};
use toml::Value as TomlValue;

// ============================================================================
// JSON Configuration Merge Functions
// ============================================================================

/// Deep merge two JSON objects where `source` overrides `target`.
///
/// Merge rules:
/// - Nested objects: Recursive merge
/// - Arrays: Source completely replaces target (no element-level merge)
/// - Primitives: Source overrides target
/// - Null values: Do not override
fn deep_merge_json(target: &mut JsonValue, source: &JsonValue) {
    // First check if both are objects without destructuring
    let both_objects =
        matches!(target, JsonValue::Object(_)) && matches!(source, JsonValue::Object(_));

    if both_objects {
        // Safe to destructure now since we know both are objects
        if let (JsonValue::Object(target_map), JsonValue::Object(source_map)) = (target, source) {
            for (key, source_value) in source_map {
                if source_value.is_null() {
                    // Null doesn't override
                    continue;
                }
                match target_map.get_mut(key) {
                    Some(target_value) if target_value.is_object() && source_value.is_object() => {
                        // Nested object: recursive merge
                        deep_merge_json(target_value, source_value);
                    }
                    _ => {
                        // Other cases: source overrides
                        target_map.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
    } else {
        // Non-object: source overrides
        *target = source.clone();
    }
}

/// Compute final JSON config.
///
/// Common config as base, custom config overrides (custom takes priority).
///
/// # Arguments
/// * `custom_config` - Provider's custom configuration
/// * `common_config` - Common configuration snippet
/// * `enabled` - Whether common config is enabled
///
/// # Returns
/// The merged final configuration as JSON value
pub fn compute_final_json_config(
    custom_config: &JsonValue,
    common_config: &JsonValue,
    enabled: bool,
) -> JsonValue {
    if !enabled {
        return custom_config.clone();
    }

    // Validate both are objects
    let common_obj = match common_config {
        JsonValue::Object(m) if !m.is_empty() => m,
        _ => return custom_config.clone(),
    };

    let custom_obj = match custom_config {
        JsonValue::Object(_) => custom_config,
        _ => return custom_config.clone(),
    };

    // Start with common config as base
    let mut result = JsonValue::Object(common_obj.clone());

    // Merge custom config on top (custom overrides common)
    deep_merge_json(&mut result, custom_obj);

    result
}

/// Compute final JSON config from strings.
///
/// # Arguments
/// * `custom_config_json` - Custom configuration as JSON string
/// * `common_config_json` - Common configuration as JSON string
/// * `enabled` - Whether common config is enabled
///
/// # Returns
/// Tuple of (final_config_json, error_message)
pub fn compute_final_json_config_str(
    custom_config_json: &str,
    common_config_json: &str,
    enabled: bool,
) -> (String, Option<String>) {
    let custom_config: JsonValue = match serde_json::from_str(custom_config_json) {
        Ok(v) => v,
        Err(_) => {
            return (
                custom_config_json.to_string(),
                Some("Failed to parse custom config JSON".to_string()),
            )
        }
    };

    let common_config: JsonValue = if common_config_json.trim().is_empty() {
        JsonValue::Object(Map::new())
    } else {
        match serde_json::from_str(common_config_json) {
            Ok(v) => v,
            Err(_) => {
                return (
                    custom_config_json.to_string(),
                    Some("Failed to parse common config JSON".to_string()),
                )
            }
        }
    };

    let final_config = compute_final_json_config(&custom_config, &common_config, enabled);

    match serde_json::to_string_pretty(&final_config) {
        Ok(s) => (s, None),
        Err(e) => (
            custom_config_json.to_string(),
            Some(format!("Failed to serialize: {e}")),
        ),
    }
}

/// Check if two JSON values are deeply equal.
fn json_deep_equal(a: &JsonValue, b: &JsonValue) -> bool {
    match (a, b) {
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
        (JsonValue::Number(a), JsonValue::Number(b)) => a == b,
        (JsonValue::String(a), JsonValue::String(b)) => a == b,
        (JsonValue::Array(a), JsonValue::Array(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(x, y)| json_deep_equal(x, y))
        }
        (JsonValue::Object(a), JsonValue::Object(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .all(|(k, v)| b.get(k).is_some_and(|bv| json_deep_equal(v, bv)))
        }
        _ => false,
    }
}

/// Extract difference between live config and common config.
///
/// Extraction rules:
/// - Keys not in common config → include in custom config
/// - Keys in common config but with different values → include in custom config (user override)
/// - Keys in common config with same values → skip (avoid redundant storage)
///
/// # Arguments
/// * `live_config` - Configuration read from live file
/// * `common_config` - Common configuration snippet
///
/// # Returns
/// Tuple of (custom_config, has_common_keys)
pub fn extract_json_difference(
    live_config: &JsonValue,
    common_config: &JsonValue,
) -> (JsonValue, bool) {
    let live_obj = match live_config {
        JsonValue::Object(m) => m,
        _ => return (live_config.clone(), false),
    };

    let common_obj = match common_config {
        JsonValue::Object(m) => m,
        _ => return (live_config.clone(), false),
    };

    let mut custom_config = Map::new();
    let mut has_common_keys = false;

    fn extract_recursive(
        live: &Map<String, JsonValue>,
        common: &Map<String, JsonValue>,
        target: &mut Map<String, JsonValue>,
        has_common: &mut bool,
    ) {
        for (key, live_value) in live {
            match common.get(key) {
                None => {
                    // Case 1: Key not in common config, keep it
                    target.insert(key.clone(), live_value.clone());
                }
                Some(common_value) => {
                    // Check if both are objects for nested handling
                    match (live_value, common_value) {
                        (JsonValue::Object(live_map), JsonValue::Object(common_map)) => {
                            // Case 2: Nested object, recurse
                            let mut nested = Map::new();
                            extract_recursive(live_map, common_map, &mut nested, has_common);
                            if !nested.is_empty() {
                                target.insert(key.clone(), JsonValue::Object(nested));
                            } else {
                                // Nested object matches common config
                                *has_common = true;
                            }
                        }
                        _ if !json_deep_equal(live_value, common_value) => {
                            // Case 3: Value different, keep it (user override)
                            target.insert(key.clone(), live_value.clone());
                        }
                        _ => {
                            // Case 4: Value same, skip (avoid redundancy)
                            *has_common = true;
                        }
                    }
                }
            }
        }
    }

    extract_recursive(
        live_obj,
        common_obj,
        &mut custom_config,
        &mut has_common_keys,
    );

    (JsonValue::Object(custom_config), has_common_keys)
}

/// Extract difference from JSON strings.
///
/// # Returns
/// Tuple of (custom_config_json, has_common_keys, error_message)
pub fn extract_json_difference_str(
    live_config_json: &str,
    common_config_json: &str,
) -> (String, bool, Option<String>) {
    let live_config: JsonValue = match serde_json::from_str(live_config_json) {
        Ok(v) => v,
        Err(_) => {
            return (
                live_config_json.to_string(),
                false,
                Some("Failed to parse live config JSON".to_string()),
            )
        }
    };

    let common_config: JsonValue = if common_config_json.trim().is_empty() {
        JsonValue::Object(Map::new())
    } else {
        match serde_json::from_str(common_config_json) {
            Ok(v) => v,
            Err(_) => {
                return (
                    live_config_json.to_string(),
                    false,
                    Some("Failed to parse common config JSON".to_string()),
                )
            }
        }
    };

    let (custom_config, has_common_keys) = extract_json_difference(&live_config, &common_config);

    match serde_json::to_string_pretty(&custom_config) {
        Ok(s) => (s, has_common_keys, None),
        Err(e) => (
            live_config_json.to_string(),
            false,
            Some(format!("Failed to serialize: {e}")),
        ),
    }
}

// ============================================================================
// TOML Configuration Merge Functions
// ============================================================================

/// Deep merge two TOML tables where `source` overrides `target`.
fn deep_merge_toml(target: &mut TomlValue, source: &TomlValue) {
    // First check if both are tables without destructuring
    let both_tables =
        matches!(target, TomlValue::Table(_)) && matches!(source, TomlValue::Table(_));

    if both_tables {
        // Safe to destructure now since we know both are tables
        if let (TomlValue::Table(target_map), TomlValue::Table(source_map)) = (target, source) {
            for (key, source_value) in source_map {
                match target_map.get_mut(key) {
                    Some(target_value) if target_value.is_table() && source_value.is_table() => {
                        // Nested table: recursive merge
                        deep_merge_toml(target_value, source_value);
                    }
                    _ => {
                        // Other cases: source overrides
                        target_map.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
    } else {
        // Non-table: source overrides
        *target = source.clone();
    }
}

/// Compute final TOML config.
///
/// # Arguments
/// * `custom_config` - Provider's custom TOML configuration
/// * `common_config` - Common TOML configuration snippet
/// * `enabled` - Whether common config is enabled
///
/// # Returns
/// Tuple of (final_config_toml, error_message)
pub fn compute_final_toml_config_str(
    custom_toml: &str,
    common_toml: &str,
    enabled: bool,
) -> (String, Option<String>) {
    if !enabled || common_toml.trim().is_empty() {
        return (custom_toml.to_string(), None);
    }

    // Check if common TOML has actual content (not just comments)
    let common_has_content = common_toml.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    });

    if !common_has_content {
        return (custom_toml.to_string(), None);
    }

    // Parse custom TOML
    let custom_config: TomlValue = match custom_toml.parse() {
        Ok(v) => v,
        Err(_) if custom_toml.trim().is_empty() => TomlValue::Table(toml::map::Map::new()),
        Err(e) => {
            return (
                custom_toml.to_string(),
                Some(format!("Failed to parse custom TOML: {e}")),
            )
        }
    };

    // Parse common TOML
    let common_config: TomlValue = match common_toml.parse() {
        Ok(v) => v,
        Err(e) => {
            return (
                custom_toml.to_string(),
                Some(format!("Failed to parse common TOML: {e}")),
            )
        }
    };

    // Start with common config as base
    let mut result = common_config;

    // Merge custom config on top
    deep_merge_toml(&mut result, &custom_config);

    // Serialize back to TOML string
    match toml::to_string_pretty(&result) {
        Ok(s) => (s, None),
        Err(e) => (
            custom_toml.to_string(),
            Some(format!("Failed to serialize TOML: {e}")),
        ),
    }
}

/// Check if two TOML values are deeply equal.
fn toml_deep_equal(a: &TomlValue, b: &TomlValue) -> bool {
    match (a, b) {
        (TomlValue::String(a), TomlValue::String(b)) => a == b,
        (TomlValue::Integer(a), TomlValue::Integer(b)) => a == b,
        (TomlValue::Float(a), TomlValue::Float(b)) => (a - b).abs() < f64::EPSILON,
        (TomlValue::Boolean(a), TomlValue::Boolean(b)) => a == b,
        (TomlValue::Datetime(a), TomlValue::Datetime(b)) => a == b,
        (TomlValue::Array(a), TomlValue::Array(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(x, y)| toml_deep_equal(x, y))
        }
        (TomlValue::Table(a), TomlValue::Table(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .all(|(k, v)| b.get(k).is_some_and(|bv| toml_deep_equal(v, bv)))
        }
        _ => false,
    }
}

/// Extract difference between live TOML config and common config.
///
/// # Returns
/// Tuple of (custom_toml, has_common_keys, error_message)
pub fn extract_toml_difference_str(
    live_toml: &str,
    common_toml: &str,
) -> (String, bool, Option<String>) {
    if common_toml.trim().is_empty() {
        return (live_toml.to_string(), false, None);
    }

    // Check if common TOML has actual content
    let common_has_content = common_toml.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    });

    if !common_has_content {
        return (live_toml.to_string(), false, None);
    }

    // Parse live TOML
    let live_config: TomlValue = match live_toml.parse() {
        Ok(v) => v,
        Err(_) if live_toml.trim().is_empty() => TomlValue::Table(toml::map::Map::new()),
        Err(e) => {
            return (
                live_toml.to_string(),
                false,
                Some(format!("Failed to parse live TOML: {e}")),
            )
        }
    };

    // Parse common TOML
    let common_config: TomlValue = match common_toml.parse() {
        Ok(v) => v,
        Err(e) => {
            return (
                live_toml.to_string(),
                false,
                Some(format!("Failed to parse common TOML: {e}")),
            )
        }
    };

    let live_table = match &live_config {
        TomlValue::Table(m) => m,
        _ => return (live_toml.to_string(), false, None),
    };

    let common_table = match &common_config {
        TomlValue::Table(m) => m,
        _ => return (live_toml.to_string(), false, None),
    };

    let mut custom_table = toml::map::Map::new();
    let mut has_common_keys = false;

    fn extract_recursive_toml(
        live: &toml::map::Map<String, TomlValue>,
        common: &toml::map::Map<String, TomlValue>,
        target: &mut toml::map::Map<String, TomlValue>,
        has_common: &mut bool,
    ) {
        for (key, live_value) in live {
            match common.get(key) {
                None => {
                    target.insert(key.clone(), live_value.clone());
                }
                Some(common_value) => match (live_value, common_value) {
                    (TomlValue::Table(live_map), TomlValue::Table(common_map)) => {
                        let mut nested = toml::map::Map::new();
                        extract_recursive_toml(live_map, common_map, &mut nested, has_common);
                        if !nested.is_empty() {
                            target.insert(key.clone(), TomlValue::Table(nested));
                        } else {
                            *has_common = true;
                        }
                    }
                    _ if !toml_deep_equal(live_value, common_value) => {
                        target.insert(key.clone(), live_value.clone());
                    }
                    _ => {
                        *has_common = true;
                    }
                },
            }
        }
    }

    extract_recursive_toml(
        live_table,
        common_table,
        &mut custom_table,
        &mut has_common_keys,
    );

    let custom_config = TomlValue::Table(custom_table);

    match toml::to_string_pretty(&custom_config) {
        Ok(s) if s.trim().is_empty() => (String::new(), has_common_keys, None),
        Ok(s) => (s, has_common_keys, None),
        Err(e) => (
            live_toml.to_string(),
            false,
            Some(format!("Failed to serialize TOML: {e}")),
        ),
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_compute_final_json_config_disabled() {
        let custom = json!({"a": 1, "b": 2});
        let common = json!({"c": 3});

        let result = compute_final_json_config(&custom, &common, false);
        assert_eq!(result, custom);
    }

    #[test]
    fn test_compute_final_json_config_enabled() {
        let custom = json!({"a": 1, "b": 2});
        let common = json!({"b": 99, "c": 3});

        let result = compute_final_json_config(&custom, &common, true);

        // custom overrides common, so b should be 2
        assert_eq!(result["a"], 1);
        assert_eq!(result["b"], 2); // custom wins
        assert_eq!(result["c"], 3); // from common
    }

    #[test]
    fn test_compute_final_json_config_nested() {
        let custom = json!({
            "env": {
                "API_KEY": "custom-key",
                "CUSTOM_VAR": "value"
            }
        });
        let common = json!({
            "env": {
                "API_KEY": "common-key",
                "SHARED_VAR": "shared"
            },
            "includeCoAuthoredBy": false
        });

        let result = compute_final_json_config(&custom, &common, true);

        assert_eq!(result["env"]["API_KEY"], "custom-key"); // custom wins
        assert_eq!(result["env"]["CUSTOM_VAR"], "value"); // from custom
        assert_eq!(result["env"]["SHARED_VAR"], "shared"); // from common
        assert_eq!(result["includeCoAuthoredBy"], false); // from common
    }

    #[test]
    fn test_extract_json_difference() {
        let live = json!({
            "env": {
                "API_KEY": "my-key",
                "SHARED_VAR": "shared"
            },
            "includeCoAuthoredBy": false,
            "custom_field": true
        });
        let common = json!({
            "env": {
                "SHARED_VAR": "shared"
            },
            "includeCoAuthoredBy": false
        });

        let (custom, has_common) = extract_json_difference(&live, &common);

        // Should keep API_KEY (not in common) and custom_field
        assert_eq!(custom["env"]["API_KEY"], "my-key");
        assert_eq!(custom["custom_field"], true);
        // Should NOT have SHARED_VAR or includeCoAuthoredBy (same as common)
        assert!(custom["env"].get("SHARED_VAR").is_none());
        assert!(custom.get("includeCoAuthoredBy").is_none());
        assert!(has_common);
    }

    #[test]
    fn test_extract_json_difference_with_override() {
        let live = json!({
            "includeCoAuthoredBy": true, // Different from common!
            "shared": "value"
        });
        let common = json!({
            "includeCoAuthoredBy": false,
            "shared": "value"
        });

        let (custom, has_common) = extract_json_difference(&live, &common);

        // Should keep includeCoAuthoredBy because value is different
        assert_eq!(custom["includeCoAuthoredBy"], true);
        // Should NOT have shared (same as common)
        assert!(custom.get("shared").is_none());
        assert!(has_common);
    }

    #[test]
    fn test_compute_final_toml_config() {
        let custom = r#"
model = "custom-model"
[custom_section]
key = "value"
"#;
        let common = r#"
model = "common-model"
shared_key = "shared"
"#;

        let (result, error) = compute_final_toml_config_str(custom, common, true);

        assert!(error.is_none());
        assert!(result.contains("custom-model")); // custom wins
        assert!(result.contains("shared_key")); // from common
    }

    #[test]
    fn test_extract_toml_difference() {
        let live = r#"
model = "my-model"
shared_key = "shared"
[custom_section]
key = "value"
"#;
        let common = r#"
shared_key = "shared"
"#;

        let (custom, has_common, error) = extract_toml_difference_str(live, common);

        assert!(error.is_none());
        assert!(custom.contains("model")); // not in common
        assert!(custom.contains("custom_section")); // not in common
        assert!(!custom.contains("shared_key")); // same as common
        assert!(has_common);
    }
}
