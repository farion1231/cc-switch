use crate::config::{atomic_write, get_home_dir, write_json_file};
use crate::error::AppError;
use crate::opencode_config::get_opencode_dir;
use crate::provider::Provider;
use crate::store::AppState;
use json_five::rt::parser::{
    from_str as rt_from_str, JSONObjectContext as RtJSONObjectContext, JSONText as RtJSONText,
    JSONValue as RtJSONValue, KeyValuePairContext as RtKeyValuePairContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmoLocalFileData {
    pub agents: Option<Value>,
    pub categories: Option<Value>,
    pub other_fields: Option<Value>,
    pub file_path: String,
    pub last_modified: Option<String>,
}

type OmoProfileData = (Option<Value>, Option<Value>, Option<Value>);

const UNIFIED_CONFIG_FILENAME: &str = "omo.jsonc";
const OPENCODE_SECTION_KEY: &str = "[opencode]";

#[derive(Debug, PartialEq)]
enum OmoConfigLocation {
    Unified(PathBuf),
    Legacy(PathBuf),
}

impl OmoConfigLocation {
    fn path(&self) -> &Path {
        match self {
            Self::Unified(path) | Self::Legacy(path) => path,
        }
    }
}

struct UnifiedConfigDocument {
    path: PathBuf,
    original_source: String,
    text: RtJSONText,
}

impl UnifiedConfigDocument {
    fn load(path: &Path) -> Result<Self, AppError> {
        let original_source = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        let text = rt_from_str(&original_source).map_err(|e| {
            AppError::Config(format!(
                "Failed to parse OMO config as round-trip JSON5: {}",
                e.message
            ))
        })?;
        let RtJSONValue::JSONObject {
            key_value_pairs, ..
        } = &text.value
        else {
            return Err(AppError::Config(
                "OMO config root must be a JSON object".to_string(),
            ));
        };
        if key_value_pairs
            .iter()
            .filter(|pair| json5_key_name(&pair.key) == Some(OPENCODE_SECTION_KEY))
            .count()
            > 1
        {
            return Err(AppError::Config(
                "OMO config contains duplicate [opencode] sections".to_string(),
            ));
        }

        Ok(Self {
            path: path.to_path_buf(),
            original_source,
            text,
        })
    }

    fn set_opencode_section(&mut self, value: &Value) -> Result<(), AppError> {
        let RtJSONValue::JSONObject {
            key_value_pairs,
            context,
        } = &mut self.text.value
        else {
            return Err(AppError::Config(
                "OMO config root must be a JSON object".to_string(),
            ));
        };

        let child_indent = context
            .as_ref()
            .map(|ctx| extract_trailing_indent(&ctx.wsc.0))
            .unwrap_or_default();
        let pair = key_value_pairs
            .iter_mut()
            .find(|pair| json5_key_name(&pair.key) == Some(OPENCODE_SECTION_KEY))
            .ok_or(AppError::OmoConfigNotFound)?;
        pair.value = value_to_rt_value(value, &child_indent)?;
        Ok(())
    }

    fn remove_opencode_section(&mut self) -> Result<(), AppError> {
        let RtJSONValue::JSONObject {
            key_value_pairs,
            context,
        } = &mut self.text.value
        else {
            return Err(AppError::Config(
                "OMO config root must be a JSON object".to_string(),
            ));
        };

        let index = key_value_pairs
            .iter()
            .position(|pair| json5_key_name(&pair.key) == Some(OPENCODE_SECTION_KEY))
            .ok_or(AppError::OmoConfigNotFound)?;
        let removed = key_value_pairs.remove(index);
        let Some(removed_context) = removed.context else {
            return Ok(());
        };

        if index < key_value_pairs.len() {
            let after_comma = removed_context.wsc.3.unwrap_or_default();
            if index == 0 {
                ensure_object_context(context).wsc.0.push_str(&after_comma);
            } else {
                let previous = ensure_kvp_context(&mut key_value_pairs[index - 1]);
                let separator = previous.wsc.3.take().unwrap_or_default();
                previous.wsc.3 = Some(format!("{separator}{after_comma}"));
            }
        } else if index == 0 {
            let object_context = ensure_object_context(context);
            object_context.wsc.0.push_str(&removed_context.wsc.2);
            if let Some(after_comma) = removed_context.wsc.3 {
                object_context.wsc.0.push_str(&after_comma);
            }
        } else {
            let previous = ensure_kvp_context(&mut key_value_pairs[index - 1]);
            let separator = previous.wsc.3.take().unwrap_or_default();
            if let Some(after_comma) = removed_context.wsc.3 {
                previous.wsc.3 = Some(format!("{separator}{after_comma}"));
            } else {
                previous.wsc.2.push_str(&separator);
                previous.wsc.2.push_str(&removed_context.wsc.2);
            }
        }
        Ok(())
    }

    fn save(self) -> Result<Vec<u8>, AppError> {
        let _guard = omo_write_lock().lock()?;
        let current_source =
            std::fs::read_to_string(&self.path).map_err(|e| AppError::io(&self.path, e))?;
        if current_source != self.original_source {
            return Err(AppError::Config(
                "OMO config changed on disk. Please reload and try again.".to_string(),
            ));
        }

        let next_contents = self.text.to_string().into_bytes();
        atomic_write(&self.path, &next_contents)?;
        Ok(next_contents)
    }
}

fn omo_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn extract_trailing_indent(separator_ws: &str) -> String {
    separator_ws
        .rsplit_once('\n')
        .map(|(_, tail)| tail.to_string())
        .unwrap_or_default()
}

fn ensure_object_context(context: &mut Option<RtJSONObjectContext>) -> &mut RtJSONObjectContext {
    context.get_or_insert_with(|| RtJSONObjectContext {
        wsc: (String::new(),),
    })
}

fn ensure_kvp_context(
    pair: &mut json_five::rt::parser::JSONKeyValuePair,
) -> &mut RtKeyValuePairContext {
    pair.context.get_or_insert_with(|| RtKeyValuePairContext {
        wsc: (String::new(), String::new(), String::new(), None),
    })
}

fn value_to_rt_value(value: &Value, parent_indent: &str) -> Result<RtJSONValue, AppError> {
    let source = serde_json::to_string_pretty(value)
        .map_err(|e| AppError::Config(format!("Failed to serialize OMO section: {e}")))?;
    let adjusted = reindent_json5_block(&source, parent_indent);
    let text = rt_from_str(&adjusted).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse generated OMO section: {}",
            e.message
        ))
    })?;
    Ok(text.value)
}

fn reindent_json5_block(source: &str, parent_indent: &str) -> String {
    if parent_indent.is_empty() || !source.contains('\n') {
        return source.to_string();
    }

    let mut lines = source.lines();
    let Some(first_line) = lines.next() else {
        return String::new();
    };

    let mut result = String::from(first_line);
    for line in lines {
        result.push('\n');
        result.push_str(parent_indent);
        result.push_str(line);
    }
    result
}

fn json5_key_name(key: &RtJSONValue) -> Option<&str> {
    match key {
        RtJSONValue::Identifier(name)
        | RtJSONValue::DoubleQuotedString(name)
        | RtJSONValue::SingleQuotedString(name) => Some(name),
        _ => None,
    }
}

// ── Variant descriptor ─────────────────────────────────────────

pub struct OmoVariant {
    pub preferred_filename: &'static str,
    pub config_candidates: &'static [&'static str],
    pub category: &'static str,
    pub provider_prefix: &'static str,
    pub plugin_name: &'static str,
    pub plugin_prefixes: &'static [&'static str],
    pub has_categories: bool,
    pub label: &'static str,
    pub import_label: &'static str,
}

pub const STANDARD: OmoVariant = OmoVariant {
    preferred_filename: "oh-my-openagent.jsonc",
    config_candidates: &[
        "oh-my-openagent.jsonc",
        "oh-my-openagent.json",
        "oh-my-opencode.jsonc",
        "oh-my-opencode.json",
    ],
    category: "omo",
    provider_prefix: "omo-",
    plugin_name: "oh-my-openagent@latest",
    plugin_prefixes: &["oh-my-openagent", "oh-my-opencode"],
    has_categories: true,
    label: "OMO",
    import_label: "Imported",
};

pub const SLIM: OmoVariant = OmoVariant {
    preferred_filename: "oh-my-opencode-slim.jsonc",
    config_candidates: &["oh-my-opencode-slim.jsonc", "oh-my-opencode-slim.json"],
    category: "omo-slim",
    provider_prefix: "omo-slim-",
    plugin_name: "oh-my-opencode-slim@latest",
    plugin_prefixes: &["oh-my-opencode-slim"],
    has_categories: false,
    label: "OMO Slim",
    import_label: "Imported Slim",
};

// ── Service ────────────────────────────────────────────────────

pub struct OmoService;

impl OmoService {
    // ── Path helpers ────────────────────────────────────────

    fn config_candidates(v: &OmoVariant, base_dir: &Path) -> Vec<PathBuf> {
        v.config_candidates
            .iter()
            .map(|name| base_dir.join(name))
            .collect()
    }

    fn find_existing_config_path(v: &OmoVariant, base_dir: &Path) -> Option<PathBuf> {
        Self::config_candidates(v, base_dir)
            .into_iter()
            .find(|path| path.exists())
    }

    fn find_unified_config_path(
        v: &OmoVariant,
        home_dir: &Path,
    ) -> Result<Option<PathBuf>, AppError> {
        if v.category != STANDARD.category {
            return Ok(None);
        }

        let path = home_dir.join(".omo").join(UNIFIED_CONFIG_FILENAME);
        if !path.exists() {
            return Ok(None);
        }

        UnifiedConfigDocument::load(&path)?;
        let root = Self::read_jsonc_object(&path)?;
        Ok(root
            .get(OPENCODE_SECTION_KEY)
            .and_then(Value::as_object)
            .map(|_| path))
    }

    fn find_config_location(
        v: &OmoVariant,
        home_dir: &Path,
        legacy_dir: &Path,
    ) -> Result<Option<OmoConfigLocation>, AppError> {
        if let Some(path) = Self::find_unified_config_path(v, home_dir)? {
            return Ok(Some(OmoConfigLocation::Unified(path)));
        }

        Ok(Self::find_existing_config_path(v, legacy_dir).map(OmoConfigLocation::Legacy))
    }

    fn config_location(v: &OmoVariant) -> Result<OmoConfigLocation, AppError> {
        let legacy_dir = get_opencode_dir();
        Ok(Self::find_config_location(v, &get_home_dir(), &legacy_dir)?
            .unwrap_or_else(|| OmoConfigLocation::Legacy(legacy_dir.join(v.preferred_filename))))
    }

    fn resolve_local_config_location(v: &OmoVariant) -> Result<OmoConfigLocation, AppError> {
        Self::find_config_location(v, &get_home_dir(), &get_opencode_dir())?
            .ok_or(AppError::OmoConfigNotFound)
    }

    fn read_jsonc_object(path: &Path) -> Result<Map<String, Value>, AppError> {
        let content = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        let parsed: Value = json5::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse OMO config: {e}")))?;
        parsed
            .as_object()
            .cloned()
            .ok_or_else(|| AppError::Config("Expected JSON object".to_string()))
    }

    fn read_config_object(location: &OmoConfigLocation) -> Result<Map<String, Value>, AppError> {
        let mut root = Self::read_jsonc_object(location.path())?;
        match location {
            OmoConfigLocation::Unified(_) => root
                .remove(OPENCODE_SECTION_KEY)
                .and_then(|value| value.as_object().cloned())
                .ok_or(AppError::OmoConfigNotFound),
            OmoConfigLocation::Legacy(_) => Ok(root),
        }
    }

    fn remove_unified_config_section(path: &Path) -> Result<Vec<u8>, AppError> {
        let mut document = UnifiedConfigDocument::load(path)?;
        document.remove_opencode_section()?;
        document.save()
    }

    // ── Field extraction ───────────────────────────────────

    fn extract_other_fields_with_keys(
        obj: &Map<String, Value>,
        known: &[&str],
    ) -> Map<String, Value> {
        let mut other = Map::new();
        for (k, v) in obj {
            if !known.contains(&k.as_str()) {
                other.insert(k.clone(), v.clone());
            }
        }
        other
    }

    // ── Merge helpers ──────────────────────────────────────

    fn insert_opt_value(result: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
        if let Some(v) = value {
            result.insert(key.to_string(), v.clone());
        }
    }

    fn insert_object_entries(result: &mut Map<String, Value>, value: Option<&Value>) {
        if let Some(Value::Object(map)) = value {
            for (k, v) in map {
                result.insert(k.clone(), v.clone());
            }
        }
    }

    fn profile_data_from_provider(provider: &Provider, v: &OmoVariant) -> OmoProfileData {
        let agents = provider.settings_config.get("agents").cloned();
        let categories = if v.has_categories {
            provider.settings_config.get("categories").cloned()
        } else {
            None
        };
        let other_fields = provider.settings_config.get("otherFields").cloned();
        (agents, categories, other_fields)
    }

    fn snapshot_config_file(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
        if !path.exists() {
            return Ok(None);
        }

        std::fs::read(path)
            .map(Some)
            .map_err(|e| AppError::io(path, e))
    }

    fn restore_config_file(path: &Path, snapshot: Option<&[u8]>) -> Result<(), AppError> {
        match snapshot {
            Some(bytes) => atomic_write(path, bytes),
            None => {
                if path.exists() {
                    std::fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
                }
                Ok(())
            }
        }
    }

    fn restore_config_file_if_unchanged(
        path: &Path,
        expected_contents: Option<&[u8]>,
        snapshot: Option<&[u8]>,
    ) -> Result<(), AppError> {
        let _guard = omo_write_lock().lock()?;
        let current_contents = Self::snapshot_config_file(path)?;
        if current_contents.as_deref() != expected_contents {
            return Err(AppError::Config(format!(
                "Config changed after CC Switch wrote it; refusing to roll back {}",
                path.display()
            )));
        }
        Self::restore_config_file(path, snapshot)
    }

    fn write_profile_config(
        v: &OmoVariant,
        profile_data: Option<&OmoProfileData>,
    ) -> Result<(), AppError> {
        let merged = Self::build_config(v, profile_data);
        let location = Self::config_location(v)?;
        let config_path = location.path().to_path_buf();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let (previous_contents, expected_contents) = match &location {
            OmoConfigLocation::Unified(path) => {
                let mut document = UnifiedConfigDocument::load(path)?;
                let previous_contents = Some(document.original_source.as_bytes().to_vec());
                document.set_opencode_section(&merged)?;
                let expected_contents = Some(document.save()?);
                (previous_contents, expected_contents)
            }
            OmoConfigLocation::Legacy(path) => {
                let previous_contents = Self::snapshot_config_file(path)?;
                write_json_file(path, &merged)?;
                let expected_contents = Self::snapshot_config_file(path)?;
                (previous_contents, expected_contents)
            }
        };
        if let Err(err) = crate::opencode_config::add_plugin(v.plugin_name) {
            if let Err(rollback_err) = Self::restore_config_file_if_unchanged(
                &config_path,
                expected_contents.as_deref(),
                previous_contents.as_deref(),
            ) {
                log::warn!(
                    "Failed to roll back {} config after plugin sync error: {}",
                    v.label,
                    rollback_err
                );
            }
            return Err(err);
        }
        log::info!("{} config written to {config_path:?}", v.label);
        Ok(())
    }

    // ── Public API (variant-parameterized) ─────────────────

    pub fn delete_config_file(v: &OmoVariant) -> Result<(), AppError> {
        let base_dir = get_opencode_dir();
        let unified_path = Self::find_unified_config_path(v, &get_home_dir())?;
        let legacy_paths: Vec<_> = Self::config_candidates(v, &base_dir)
            .into_iter()
            .filter(|path| path.exists())
            .collect();
        let plugin_config_path = crate::opencode_config::get_opencode_config_path();

        let plugin_config_snapshot = Self::snapshot_config_file(&plugin_config_path)?;
        let unified_snapshot = unified_path
            .as_ref()
            .map(|path| Self::snapshot_config_file(path))
            .transpose()?
            .flatten();
        let legacy_snapshots = legacy_paths
            .iter()
            .map(|path| Self::snapshot_config_file(path))
            .collect::<Result<Vec<_>, _>>()?;
        let mut applied_changes = Vec::new();

        let result = (|| -> Result<(), AppError> {
            crate::opencode_config::remove_plugins_by_prefixes(v.plugin_prefixes)?;
            let plugin_config_expected = Self::snapshot_config_file(&plugin_config_path)?;
            applied_changes.push((
                plugin_config_path,
                plugin_config_snapshot,
                plugin_config_expected,
            ));
            if let Some(path) = &unified_path {
                let expected_contents = Some(Self::remove_unified_config_section(path)?);
                applied_changes.push((path.clone(), unified_snapshot, expected_contents));
            }
            for (path, snapshot) in legacy_paths.iter().zip(legacy_snapshots) {
                std::fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
                applied_changes.push((path.clone(), snapshot, None));
            }
            Ok(())
        })();

        if let Err(err) = result {
            for (path, snapshot, expected_contents) in applied_changes.iter().rev() {
                if let Err(rollback_err) = Self::restore_config_file_if_unchanged(
                    path,
                    expected_contents.as_deref(),
                    snapshot.as_deref(),
                ) {
                    log::warn!(
                        "Failed to roll back OMO disable change at {path:?}: {rollback_err}"
                    );
                }
            }
            return Err(err);
        }

        let mut changed_paths = legacy_paths;
        if let Some(path) = unified_path {
            changed_paths.push(path);
        }
        if !changed_paths.is_empty() {
            log::info!(
                "{} config files updated or deleted: {changed_paths:?}",
                v.label
            );
        }
        Ok(())
    }

    pub fn write_config_to_file(state: &AppState, v: &OmoVariant) -> Result<(), AppError> {
        let current_omo = state.db.get_current_omo_provider("opencode", v.category)?;
        let profile_data = current_omo
            .as_ref()
            .map(|provider| Self::profile_data_from_provider(provider, v));
        Self::write_profile_config(v, profile_data.as_ref())
    }

    pub fn write_provider_config_to_file(
        provider: &Provider,
        v: &OmoVariant,
    ) -> Result<(), AppError> {
        let profile_data = Self::profile_data_from_provider(provider, v);
        Self::write_profile_config(v, Some(&profile_data))
    }

    fn build_config(v: &OmoVariant, profile_data: Option<&OmoProfileData>) -> Value {
        let mut result = Map::new();
        if let Some((agents, categories, other_fields)) = profile_data {
            Self::insert_object_entries(&mut result, other_fields.as_ref());
            Self::insert_opt_value(&mut result, "agents", agents);
            if v.has_categories {
                Self::insert_opt_value(&mut result, "categories", categories);
            }
        }
        Value::Object(result)
    }

    pub fn import_from_local(
        state: &AppState,
        v: &OmoVariant,
    ) -> Result<crate::provider::Provider, AppError> {
        let location = Self::resolve_local_config_location(v)?;
        let obj = Self::read_config_object(&location)?;

        let mut settings = Map::new();
        if let Some(agents) = obj.get("agents") {
            settings.insert("agents".to_string(), agents.clone());
        }
        if v.has_categories {
            if let Some(categories) = obj.get("categories") {
                settings.insert("categories".to_string(), categories.clone());
            }
        }

        let other = Self::extract_other_fields_with_keys(&obj, &["agents", "categories"]);
        if !other.is_empty() {
            settings.insert("otherFields".to_string(), Value::Object(other));
        }

        let provider_id = format!("{}{}", v.provider_prefix, uuid::Uuid::new_v4());
        let name = format!(
            "{} {}",
            v.import_label,
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );
        let settings_config =
            serde_json::to_value(&settings).unwrap_or_else(|_| serde_json::json!({}));

        let provider = crate::provider::Provider {
            id: provider_id,
            name,
            settings_config,
            website_url: None,
            category: Some(v.category.to_string()),
            created_at: Some(chrono::Utc::now().timestamp_millis()),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        state.db.save_provider("opencode", &provider)?;
        state
            .db
            .set_omo_provider_current("opencode", &provider.id, v.category)?;
        Self::write_config_to_file(state, v)?;
        Ok(provider)
    }

    pub fn read_local_file(v: &OmoVariant) -> Result<OmoLocalFileData, AppError> {
        let location = Self::resolve_local_config_location(v)?;
        let actual_path = location.path().to_path_buf();
        let metadata = std::fs::metadata(&actual_path).ok();
        let last_modified = metadata
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

        let obj = Self::read_config_object(&location)?;

        Ok(Self::build_local_file_data(
            v,
            &obj,
            actual_path.to_string_lossy().to_string(),
            last_modified,
        ))
    }

    fn build_local_file_data(
        v: &OmoVariant,
        obj: &Map<String, Value>,
        file_path: String,
        last_modified: Option<String>,
    ) -> OmoLocalFileData {
        let agents = obj.get("agents").cloned();
        let categories = if v.has_categories {
            obj.get("categories").cloned()
        } else {
            None
        };

        let other = Self::extract_other_fields_with_keys(obj, &["agents", "categories"]);
        let other_fields = if other.is_empty() {
            None
        } else {
            Some(Value::Object(other))
        };

        OmoLocalFileData {
            agents,
            categories,
            other_fields,
            file_path,
            last_modified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_jsonc_object_supports_comments_and_trailing_commas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  // This is a comment
  "key": "value",
  "key2": "val//ue",
}"#,
        )
        .unwrap();

        let parsed = OmoService::read_jsonc_object(&path).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["key2"], "val//ue");
    }

    #[test]
    fn test_find_config_location_prefers_unified_opencode_block() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            &unified_path,
            r#"{"[opencode]":{"agents":{}},"[codex]":{"agents":{}}}"#,
        )
        .unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let found = OmoService::find_config_location(&STANDARD, home.path(), &legacy_dir).unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
    }

    #[test]
    fn test_find_config_location_falls_back_when_unified_has_no_opencode_block() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        let legacy_path = legacy_dir.join(STANDARD.preferred_filename);
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(&unified_path, r#"{"[codex]":{"agents":{}}}"#).unwrap();
        std::fs::write(&legacy_path, r#"{"agents":{}}"#).unwrap();

        let found = OmoService::find_config_location(&STANDARD, home.path(), &legacy_dir).unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Legacy(legacy_path)));
    }

    #[test]
    fn test_unified_config_reads_only_opencode_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  "models": {"shared": {"model": "shared/model"}},
  "[opencode]": {"agents": {"sisyphus": {"model": "openai/gpt-5.3"}}},
  "[codex]": {"agents": {"reviewer": {"model": "openai/gpt-5.4"}}}
}"#,
        )
        .unwrap();

        let obj = OmoService::read_config_object(&OmoConfigLocation::Unified(path)).unwrap();

        assert_eq!(obj["agents"]["sisyphus"]["model"], "openai/gpt-5.3");
        assert!(!obj.contains_key("models"));
        assert!(!obj.contains_key("[codex]"));
    }

    #[test]
    fn test_unified_config_write_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  // Migration state belongs to the shared document.
  "_migrations": ["2026-07-opencode-config-unification"],
  "[opencode]": {"agents": {"old": {"model": "old/model"}}},
  // Codex settings must survive OpenCode profile switches.
  "[codex]": {"agents": {"reviewer": {"model": "openai/gpt-5.4"}}}
}"#,
        )
        .unwrap();
        let profile_data = (
            Some(serde_json::json!({"sisyphus": {"model": "openai/gpt-5.3"}})),
            None,
            None,
        );
        let config = OmoService::build_config(&STANDARD, Some(&profile_data));

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document.set_opencode_section(&config).unwrap();
        let _written_contents = document.save().unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        let document: Value = json5::from_str(&source).unwrap();

        assert_eq!(
            document["_migrations"],
            serde_json::json!(["2026-07-opencode-config-unification"])
        );
        assert_eq!(
            document["[codex]"]["agents"]["reviewer"]["model"],
            "openai/gpt-5.4"
        );
        assert_eq!(
            document["[opencode]"]["agents"]["sisyphus"]["model"],
            "openai/gpt-5.3"
        );
        assert!(source.contains("// Migration state belongs to the shared document."));
        assert!(source.contains("// Codex settings must survive OpenCode profile switches."));
    }

    #[test]
    fn test_remove_unified_config_section_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  "[opencode]": {"agents": {}},
  // Keep this Codex explanation when disabling OpenCode OMO.
  "[codex]": {"agents": {}},
  "_migrations": ["done"]
}"#,
        )
        .unwrap();

        OmoService::remove_unified_config_section(&path).unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert!(!root.contains_key(OPENCODE_SECTION_KEY));
        assert!(root.contains_key("[codex]"));
        assert_eq!(root["_migrations"], serde_json::json!(["done"]));
        assert!(source.contains("// Keep this Codex explanation when disabling OpenCode OMO."));
    }

    #[test]
    fn test_unified_config_write_rejects_concurrent_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(&path, r#"{"[opencode]":{"agents":{}}}"#).unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();
        let concurrent_source = r#"{"[opencode]":{"agents":{}},"[codex]":{"changed":true}}"#;
        std::fs::write(&path, concurrent_source).unwrap();

        let result = document.save();

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), concurrent_source);
    }

    #[test]
    fn test_unified_config_rejects_duplicate_opencode_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{"[opencode]":{"agents":{"first":{}}},"[opencode]":{"agents":{"second":{}}}}"#,
        )
        .unwrap();

        let result = UnifiedConfigDocument::load(&path);

        assert!(result.is_err());
    }

    #[test]
    fn test_rollback_refuses_to_overwrite_newer_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = br#"{"[opencode]":{"agents":{"old":{}}}}"#;
        let written = br#"{"[opencode]":{"agents":{"ours":{}}}}"#;
        let concurrent = br#"{"[opencode]":{"agents":{"theirs":{}}}}"#;
        std::fs::write(&path, concurrent).unwrap();

        let result =
            OmoService::restore_config_file_if_unchanged(&path, Some(written), Some(original));

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), concurrent);
    }

    #[test]
    fn test_build_config_empty() {
        let merged = OmoService::build_config(&STANDARD, None);
        assert!(merged.is_object());
        assert!(merged.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_build_config_with_profile() {
        let agents = Some(serde_json::json!({
            "sisyphus": { "model": "claude-opus-4-5" }
        }));
        let categories = None;
        let other_fields = Some(serde_json::json!({
            "$schema": "https://example.com/schema.json",
            "disabled_agents": ["explore"]
        }));
        let profile_data = (agents, categories, other_fields);
        let merged = OmoService::build_config(&STANDARD, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert_eq!(obj["$schema"], "https://example.com/schema.json");
        assert_eq!(obj["disabled_agents"], serde_json::json!(["explore"]));
        assert!(obj.contains_key("agents"));
        assert_eq!(obj["agents"]["sisyphus"]["model"], "claude-opus-4-5");
    }

    #[test]
    fn test_build_local_file_data_keeps_all_non_agent_category_fields_in_other() {
        let obj = serde_json::json!({
            "$schema": "https://example.com/schema.json",
            "disabled_agents": ["oracle"],
            "agents": {
                "sisyphus": { "model": "claude-opus-4-6" }
            },
            "categories": {
                "code": { "model": "gpt-5.3" }
            },
            "custom_top_level": {
                "enabled": true
            }
        });
        let obj_map = obj.as_object().unwrap().clone();

        let data = OmoService::build_local_file_data(
            &STANDARD,
            &obj_map,
            "/tmp/oh-my-opencode.jsonc".to_string(),
            None,
        );

        // All non-agents/categories fields should be in other_fields
        let other = data.other_fields.unwrap();
        let other_obj = other.as_object().unwrap();
        assert_eq!(
            other_obj.get("$schema").unwrap(),
            "https://example.com/schema.json"
        );
        assert_eq!(
            other_obj.get("disabled_agents").unwrap(),
            &serde_json::json!(["oracle"])
        );
        assert_eq!(
            other_obj.get("custom_top_level").unwrap(),
            &serde_json::json!({"enabled": true})
        );
        // agents and categories should NOT be in other_fields
        assert!(!other_obj.contains_key("agents"));
        assert!(!other_obj.contains_key("categories"));
    }

    #[test]
    fn test_build_config_ignores_non_object_other_fields() {
        let agents = None;
        let categories = None;
        let other_fields = Some(serde_json::json!("profile_non_object"));
        let profile_data = (agents, categories, other_fields);

        let merged = OmoService::build_config(&STANDARD, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        assert!(!obj.contains_key("profile_non_object"));
    }

    #[test]
    fn test_build_config_slim_excludes_categories() {
        let agents = Some(serde_json::json!({"orchestrator": {"model": "k2"}}));
        let categories = Some(serde_json::json!({"code": {"model": "gpt"}}));
        let other_fields = Some(serde_json::json!({
            "$schema": "https://slim.schema",
            "disabled_agents": ["oracle"]
        }));
        let profile_data = (agents, categories, other_fields);

        let merged = OmoService::build_config(&SLIM, Some(&profile_data));
        let obj = merged.as_object().unwrap();

        // Slim should NOT include categories
        assert!(!obj.contains_key("categories"));

        // Slim SHOULD include these
        assert_eq!(obj["$schema"], "https://slim.schema");
        assert!(obj.contains_key("agents"));
        assert!(obj.contains_key("disabled_agents"));
    }

    #[test]
    fn test_find_existing_config_prefers_new_name_over_old() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("oh-my-opencode.jsonc");
        let new_path = dir.path().join("oh-my-openagent.jsonc");

        // Create both old and new files
        std::fs::write(&old_path, r#"{"agents":{}}"#).unwrap();
        std::fs::write(&new_path, r#"{"agents":{}}"#).unwrap();

        let found = OmoService::find_existing_config_path(&STANDARD, dir.path());
        assert_eq!(
            found.unwrap(),
            new_path,
            "When both old and new config files exist, the new name (oh-my-openagent) must be preferred"
        );
    }

    #[test]
    fn test_find_existing_config_falls_back_to_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("oh-my-opencode.jsonc");

        // Only old file exists
        std::fs::write(&old_path, r#"{"agents":{}}"#).unwrap();

        let found = OmoService::find_existing_config_path(&STANDARD, dir.path());
        assert_eq!(
            found.unwrap(),
            old_path,
            "When only the old config file exists, it should still be found"
        );
    }
}
