use crate::config::{atomic_write, get_home_dir, write_json_file_with_contents};
use crate::error::AppError;
use crate::opencode_config::get_opencode_dir;
use crate::provider::Provider;
use crate::store::AppState;
use json_five::rt::parser::{
    from_str as rt_from_str, ArrayValueContext as RtArrayValueContext,
    JSONArrayContext as RtJSONArrayContext, JSONArrayValue as RtJSONArrayValue,
    JSONKeyValuePair as RtJSONKeyValuePair, JSONObjectContext as RtJSONObjectContext,
    JSONText as RtJSONText, JSONValue as RtJSONValue, KeyValuePairContext as RtKeyValuePairContext,
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
type OptionalConfigFileVersions = Option<(Vec<u8>, Vec<u8>)>;

const UNIFIED_CONFIG_FILENAMES: [&str; 2] = ["omo.jsonc", "omo.json"];
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
    semantic: Value,
    text: RtJSONText,
}

impl UnifiedConfigDocument {
    fn load(path: &Path) -> Result<Self, AppError> {
        let original_source = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
        let semantic: Value = json5::from_str(&original_source)
            .map_err(|e| AppError::Config(format!("Failed to parse OMO config: {e}")))?;
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
            semantic,
            text,
        })
    }

    fn set_opencode_section(&mut self, value: &Value) -> Result<bool, AppError> {
        if !value.is_object() {
            return Err(AppError::Config(
                "OMO [opencode] section must be an object".to_string(),
            ));
        }
        let path_display = self.path.display().to_string();
        let line_ending = if self.original_source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let current_section = self
            .semantic
            .as_object()
            .ok_or_else(|| AppError::Config("OMO config root must be a JSON object".to_string()))?
            .get(OPENCODE_SECTION_KEY)
            .cloned();
        let RtJSONValue::JSONObject {
            key_value_pairs,
            context,
        } = &mut self.text.value
        else {
            return Err(AppError::Config(
                "OMO config root must be a JSON object".to_string(),
            ));
        };

        let existing_index = key_value_pairs
            .iter()
            .position(|pair| json5_key_name(&pair.key) == Some(OPENCODE_SECTION_KEY));
        let (_, child_indent) = object_layout(key_value_pairs, context, "");

        if let Some(current_section) = current_section {
            if !current_section.is_object() {
                return Err(AppError::Config(format!(
                    "OMO [opencode] section must be an object: {path_display}"
                )));
            }
            let Some(index) = existing_index else {
                return Err(AppError::Config(format!(
                    "OMO [opencode] section could not be located in round-trip document: {path_display}"
                )));
            };
            if !matches!(
                &key_value_pairs[index].value,
                RtJSONValue::JSONObject { .. }
            ) {
                return Err(AppError::Config(format!(
                    "OMO [opencode] section must be an object: {path_display}"
                )));
            }
            if current_section == *value {
                return Ok(false);
            }

            let changed = merge_rt_value(
                &mut key_value_pairs[index].value,
                &current_section,
                value,
                &child_indent,
                line_ending,
            )?;
            self.semantic
                .as_object_mut()
                .expect("validated object root")
                .insert(OPENCODE_SECTION_KEY.to_string(), value.clone());
            Ok(changed)
        } else {
            if existing_index.is_some() {
                return Err(AppError::Config(format!(
                    "OMO [opencode] section is inconsistent in round-trip document: {path_display}"
                )));
            }
            append_object_pair(
                key_value_pairs,
                context,
                OPENCODE_SECTION_KEY,
                value,
                "",
                line_ending,
            )?;
            self.semantic
                .as_object_mut()
                .expect("validated object root")
                .insert(OPENCODE_SECTION_KEY.to_string(), value.clone());
            Ok(true)
        }
    }

    fn remove_opencode_section(&mut self) -> Result<bool, AppError> {
        let RtJSONValue::JSONObject {
            key_value_pairs,
            context,
        } = &mut self.text.value
        else {
            return Err(AppError::Config(
                "OMO config root must be a JSON object".to_string(),
            ));
        };

        let Some(index) = key_value_pairs
            .iter()
            .position(|pair| json5_key_name(&pair.key) == Some(OPENCODE_SECTION_KEY))
        else {
            return Ok(false);
        };
        remove_object_pair_at(key_value_pairs, context, index);
        self.semantic
            .as_object_mut()
            .expect("validated object root")
            .remove(OPENCODE_SECTION_KEY);
        Ok(true)
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

        let next_source = self.text.to_string();
        rt_from_str(&next_source).map_err(|e| {
            AppError::Config(format!(
                "Refusing to write invalid OMO config after round-trip serialization: {}",
                e.message
            ))
        })?;
        let reparsed: Value = json5::from_str(&next_source).map_err(|e| {
            AppError::Config(format!(
                "Refusing to write invalid OMO config after round-trip serialization: {e}"
            ))
        })?;
        if reparsed != self.semantic {
            return Err(AppError::Config(
                "Refusing to write OMO config: serialized output does not match the intended state"
                    .to_string(),
            ));
        }

        let next_contents = next_source.into_bytes();
        atomic_write(&self.path, &next_contents)?;
        Ok(next_contents)
    }
}

fn omo_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn omo_operation_lock() -> &'static Mutex<()> {
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

fn ensure_array_context(context: &mut Option<RtJSONArrayContext>) -> &mut RtJSONArrayContext {
    context.get_or_insert_with(|| RtJSONArrayContext {
        wsc: (String::new(),),
    })
}

fn ensure_array_value_context(value: &mut RtJSONArrayValue) -> &mut RtArrayValueContext {
    value.context.get_or_insert_with(|| RtArrayValueContext {
        wsc: (String::new(), None),
    })
}

fn object_layout(
    pairs: &[RtJSONKeyValuePair],
    context: &Option<RtJSONObjectContext>,
    parent_indent: &str,
) -> (bool, String) {
    let mut is_multiline = false;
    let mut child_indent = None;

    if let Some(context) = context {
        if context.wsc.0.contains('\n') {
            is_multiline = true;
            child_indent = Some(extract_trailing_indent(&context.wsc.0));
        }
    }
    for pair in pairs {
        if let Some(context) = &pair.context {
            is_multiline |= context.wsc.0.contains('\n')
                || context.wsc.1.contains('\n')
                || context.wsc.2.contains('\n')
                || context
                    .wsc
                    .3
                    .as_ref()
                    .is_some_and(|whitespace| whitespace.contains('\n'));
            if child_indent.is_none() {
                child_indent = context
                    .wsc
                    .3
                    .as_ref()
                    .filter(|whitespace| whitespace.contains('\n'))
                    .map(|whitespace| extract_trailing_indent(whitespace));
            }
        }
    }

    let child_indent = child_indent
        .filter(|indent| !indent.is_empty())
        .unwrap_or_else(|| format!("{parent_indent}  "));
    (is_multiline, child_indent)
}

fn array_layout(
    values: &[RtJSONArrayValue],
    context: &Option<RtJSONArrayContext>,
    parent_indent: &str,
) -> (bool, String) {
    let mut is_multiline = false;
    let mut child_indent = None;

    if let Some(context) = context {
        if context.wsc.0.contains('\n') {
            is_multiline = true;
            child_indent = Some(extract_trailing_indent(&context.wsc.0));
        }
    }
    for value in values {
        if let Some(context) = &value.context {
            is_multiline |= context.wsc.0.contains('\n')
                || context
                    .wsc
                    .1
                    .as_ref()
                    .is_some_and(|whitespace| whitespace.contains('\n'));
            if child_indent.is_none() {
                child_indent = context
                    .wsc
                    .1
                    .as_ref()
                    .filter(|whitespace| whitespace.contains('\n'))
                    .map(|whitespace| extract_trailing_indent(whitespace));
            }
        }
    }

    let child_indent = child_indent
        .filter(|indent| !indent.is_empty())
        .unwrap_or_else(|| format!("{parent_indent}  "));
    (is_multiline, child_indent)
}

fn remove_object_pair_at(
    pairs: &mut Vec<RtJSONKeyValuePair>,
    context: &mut Option<RtJSONObjectContext>,
    index: usize,
) {
    let removed = pairs.remove(index);
    let Some(removed_context) = removed.context else {
        return;
    };

    if index < pairs.len() {
        let after_comma = removed_context.wsc.3.unwrap_or_default();
        if index == 0 {
            ensure_object_context(context).wsc.0.push_str(&after_comma);
        } else {
            let previous = ensure_kvp_context(&mut pairs[index - 1]);
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
        let previous = ensure_kvp_context(&mut pairs[index - 1]);
        let separator = previous.wsc.3.take().unwrap_or_default();
        if let Some(after_comma) = removed_context.wsc.3 {
            previous.wsc.3 = Some(format!("{separator}{after_comma}"));
        } else {
            previous.wsc.2.push_str(&separator);
            previous.wsc.2.push_str(&removed_context.wsc.2);
        }
    }
}

fn remove_array_value_at(
    values: &mut Vec<RtJSONArrayValue>,
    context: &mut Option<RtJSONArrayContext>,
    index: usize,
) {
    let removed = values.remove(index);
    let Some(removed_context) = removed.context else {
        return;
    };

    if index < values.len() {
        let after_comma = removed_context.wsc.1.unwrap_or_default();
        if index == 0 {
            ensure_array_context(context).wsc.0.push_str(&after_comma);
        } else {
            let previous = ensure_array_value_context(&mut values[index - 1]);
            let separator = previous.wsc.1.take().unwrap_or_default();
            previous.wsc.1 = Some(format!("{separator}{after_comma}"));
        }
    } else if index == 0 {
        let array_context = ensure_array_context(context);
        array_context.wsc.0.push_str(&removed_context.wsc.0);
        if let Some(after_comma) = removed_context.wsc.1 {
            array_context.wsc.0.push_str(&after_comma);
        }
    } else {
        let previous = ensure_array_value_context(&mut values[index - 1]);
        let separator = previous.wsc.1.take().unwrap_or_default();
        if let Some(after_comma) = removed_context.wsc.1 {
            previous.wsc.1 = Some(format!("{separator}{after_comma}"));
        } else {
            previous.wsc.0.push_str(&separator);
            previous.wsc.0.push_str(&removed_context.wsc.0);
        }
    }
}

fn append_object_pair(
    pairs: &mut Vec<RtJSONKeyValuePair>,
    context: &mut Option<RtJSONObjectContext>,
    key: &str,
    value: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<(), AppError> {
    let (is_multiline, child_indent) = object_layout(pairs, context, parent_indent);
    let separator = if is_multiline {
        format!("{line_ending}{child_indent}")
    } else {
        String::new()
    };
    let mut pair = RtJSONKeyValuePair {
        key: RtJSONValue::DoubleQuotedString(key.to_string()),
        value: value_to_rt_value(value, &child_indent, line_ending)?,
        context: Some(RtKeyValuePairContext {
            wsc: (String::new(), " ".to_string(), String::new(), None),
        }),
    };

    if let Some(previous) = pairs.last_mut() {
        let previous_context = ensure_kvp_context(previous);
        let closing_ws = previous_context
            .wsc
            .3
            .take()
            .unwrap_or_else(|| std::mem::take(&mut previous_context.wsc.2));
        previous_context.wsc.3 = Some(separator);
        ensure_kvp_context(&mut pair).wsc.2 = closing_ws;
    } else {
        let object_context = ensure_object_context(context);
        let closing_ws = std::mem::take(&mut object_context.wsc.0);
        object_context.wsc.0 = separator;
        ensure_kvp_context(&mut pair).wsc.2 = closing_ws;
    }

    pairs.push(pair);
    Ok(())
}

fn append_array_value(
    values: &mut Vec<RtJSONArrayValue>,
    context: &mut Option<RtJSONArrayContext>,
    value: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<(), AppError> {
    let (is_multiline, child_indent) = array_layout(values, context, parent_indent);
    let separator = if is_multiline {
        format!("{line_ending}{child_indent}")
    } else {
        String::new()
    };
    let mut array_value = RtJSONArrayValue {
        value: value_to_rt_value(value, &child_indent, line_ending)?,
        context: Some(RtArrayValueContext {
            wsc: (String::new(), None),
        }),
    };

    if let Some(previous) = values.last_mut() {
        let previous_context = ensure_array_value_context(previous);
        let closing_ws = previous_context
            .wsc
            .1
            .take()
            .unwrap_or_else(|| std::mem::take(&mut previous_context.wsc.0));
        previous_context.wsc.1 = Some(separator);
        ensure_array_value_context(&mut array_value).wsc.0 = closing_ws;
    } else {
        let array_context = ensure_array_context(context);
        let closing_ws = std::mem::take(&mut array_context.wsc.0);
        array_context.wsc.0 = separator;
        ensure_array_value_context(&mut array_value).wsc.0 = closing_ws;
    }

    values.push(array_value);
    Ok(())
}

fn merge_rt_value(
    round_trip: &mut RtJSONValue,
    current: &Value,
    desired: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<bool, AppError> {
    if current == desired {
        return Ok(false);
    }

    match (round_trip, current, desired) {
        (
            RtJSONValue::JSONObject {
                key_value_pairs,
                context,
            },
            Value::Object(current),
            Value::Object(desired),
        ) => {
            let (_, child_indent) = object_layout(key_value_pairs, context, parent_indent);
            let mut changed = false;
            let mut seen = std::collections::HashSet::new();
            let mut index = 0;
            while index < key_value_pairs.len() {
                let key = json5_key_name(&key_value_pairs[index].key).map(str::to_string);
                let Some(key) = key else {
                    remove_object_pair_at(key_value_pairs, context, index);
                    changed = true;
                    continue;
                };
                let Some(desired_value) = desired.get(&key) else {
                    remove_object_pair_at(key_value_pairs, context, index);
                    changed = true;
                    continue;
                };
                if !seen.insert(key.clone()) {
                    remove_object_pair_at(key_value_pairs, context, index);
                    changed = true;
                    continue;
                }

                let current_value = current.get(&key).unwrap_or(&Value::Null);
                changed |= merge_rt_value(
                    &mut key_value_pairs[index].value,
                    current_value,
                    desired_value,
                    &child_indent,
                    line_ending,
                )?;
                index += 1;
            }

            for (key, desired_value) in desired {
                if seen.contains(key) {
                    continue;
                }
                append_object_pair(
                    key_value_pairs,
                    context,
                    key,
                    desired_value,
                    parent_indent,
                    line_ending,
                )?;
                changed = true;
            }
            Ok(changed)
        }
        (
            RtJSONValue::JSONArray { values, context },
            Value::Array(current),
            Value::Array(desired),
        ) => {
            let (_, child_indent) = array_layout(values, context, parent_indent);
            let common_len = values.len().min(current.len()).min(desired.len());
            let mut changed = false;
            for index in 0..common_len {
                changed |= merge_rt_value(
                    &mut values[index].value,
                    &current[index],
                    &desired[index],
                    &child_indent,
                    line_ending,
                )?;
            }
            while values.len() > desired.len() {
                let index = values.len() - 1;
                remove_array_value_at(values, context, index);
                changed = true;
            }
            while values.len() < desired.len() {
                let index = values.len();
                append_array_value(values, context, &desired[index], parent_indent, line_ending)?;
                changed = true;
            }
            Ok(changed)
        }
        (round_trip, _, desired) => {
            *round_trip = value_to_rt_value(desired, parent_indent, line_ending)?;
            Ok(true)
        }
    }
}

fn value_to_rt_value(
    value: &Value,
    parent_indent: &str,
    line_ending: &str,
) -> Result<RtJSONValue, AppError> {
    let source = serde_json::to_string_pretty(value)
        .map_err(|e| AppError::Config(format!("Failed to serialize OMO section: {e}")))?;
    let adjusted = reindent_json5_block(&source, parent_indent, line_ending);
    let text = rt_from_str(&adjusted).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse generated OMO section: {}",
            e.message
        ))
    })?;
    Ok(text.value)
}

fn reindent_json5_block(source: &str, parent_indent: &str, line_ending: &str) -> String {
    if parent_indent.is_empty() || !source.contains('\n') {
        return source.to_string();
    }

    let mut lines = source.lines();
    let Some(first_line) = lines.next() else {
        return String::new();
    };

    let mut result = String::from(first_line);
    for line in lines {
        result.push_str(line_ending);
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

        let config_dir = home_dir.join(".omo");
        for filename in UNIFIED_CONFIG_FILENAMES {
            let path = config_dir.join(filename);
            if path.try_exists().map_err(|e| AppError::io(&path, e))? {
                UnifiedConfigDocument::load(&path)?;
                return Ok(Some(path));
            }
        }

        Ok(None)
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

    fn config_location(
        v: &OmoVariant,
        home_dir: &Path,
        legacy_dir: &Path,
    ) -> Result<OmoConfigLocation, AppError> {
        Ok(Self::find_config_location(v, home_dir, legacy_dir)?
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
            OmoConfigLocation::Unified(_) => match root.remove(OPENCODE_SECTION_KEY) {
                None => Err(AppError::OmoConfigNotFound),
                Some(Value::Object(section)) => Ok(section),
                Some(_) => Err(AppError::Config(format!(
                    "OMO [opencode] section must be an object: {}",
                    location.path().display()
                ))),
            },
            OmoConfigLocation::Legacy(_) => Ok(root),
        }
    }

    fn remove_unified_config_section(path: &Path) -> Result<OptionalConfigFileVersions, AppError> {
        let mut document = UnifiedConfigDocument::load(path)?;
        let previous_contents = document.original_source.as_bytes().to_vec();
        if !document.remove_opencode_section()? {
            return Ok(None);
        }
        let expected_contents = document.save()?;
        Ok(Some((previous_contents, expected_contents)))
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
        match std::fs::read(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(AppError::io(path, err)),
        }
    }

    fn restore_config_file(path: &Path, snapshot: Option<&[u8]>) -> Result<(), AppError> {
        match snapshot {
            Some(bytes) => atomic_write(path, bytes),
            None => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(AppError::io(path, err)),
            },
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
        let _operation_guard = omo_operation_lock().lock()?;
        let plugin_config_path = crate::opencode_config::get_opencode_config_path();
        let legacy_dir = plugin_config_path.parent().ok_or_else(|| {
            AppError::Config("OpenCode config path has no parent directory".to_string())
        })?;
        let home_dir = get_home_dir();
        let merged = Self::build_config(v, profile_data);
        let location = Self::config_location(v, &home_dir, legacy_dir)?;
        let config_path = location.path().to_path_buf();

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let (previous_contents, expected_contents) = match &location {
            OmoConfigLocation::Unified(path) => {
                let mut document = UnifiedConfigDocument::load(path)?;
                if document.set_opencode_section(&merged)? {
                    let previous_contents = Some(document.original_source.as_bytes().to_vec());
                    let expected_contents = Some(document.save()?);
                    (previous_contents, expected_contents)
                } else {
                    (None, None)
                }
            }
            OmoConfigLocation::Legacy(path) => {
                let _guard = omo_write_lock().lock()?;
                let previous_contents = Self::snapshot_config_file(path)?;
                let expected_contents = Some(write_json_file_with_contents(path, &merged)?);
                (previous_contents, expected_contents)
            }
        };
        if let Err(err) = crate::opencode_config::add_plugin(&plugin_config_path, v.plugin_name) {
            if expected_contents.is_some() {
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
            }
            return Err(err);
        }
        if expected_contents.is_some() {
            log::info!("{} config written to {config_path:?}", v.label);
        }
        Ok(())
    }

    // ── Public API (variant-parameterized) ─────────────────

    pub fn delete_config_file(v: &OmoVariant) -> Result<(), AppError> {
        let _operation_guard = omo_operation_lock().lock()?;
        let plugin_config_path = crate::opencode_config::get_opencode_config_path();
        let base_dir = plugin_config_path
            .parent()
            .ok_or_else(|| {
                AppError::Config("OpenCode config path has no parent directory".to_string())
            })?
            .to_path_buf();
        let unified_path = Self::find_unified_config_path(v, &get_home_dir())?;
        let mut legacy_paths = Vec::new();
        for path in Self::config_candidates(v, &base_dir) {
            if path.try_exists().map_err(|e| AppError::io(&path, e))? {
                legacy_paths.push(path);
            }
        }

        let mut applied_changes = Vec::new();

        let result = (|| -> Result<(), AppError> {
            if let Some(path) = &unified_path {
                if let Some((snapshot, expected_contents)) =
                    Self::remove_unified_config_section(path)?
                {
                    applied_changes.push((path.clone(), Some(snapshot), Some(expected_contents)));
                }
            }
            for path in &legacy_paths {
                let _guard = omo_write_lock().lock()?;
                let snapshot = Self::snapshot_config_file(path)?;
                if snapshot.is_none() {
                    continue;
                }
                std::fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
                applied_changes.push((path.clone(), snapshot, None));
            }
            crate::opencode_config::remove_plugins_by_prefixes(
                &plugin_config_path,
                v.plugin_prefixes,
            )?;
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

        let changed_paths: Vec<_> = applied_changes
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect();
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
    fn test_find_config_location_uses_unified_when_opencode_block_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(&unified_path, r#"{"[codex]":{"agents":{}}}"#).unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let found = OmoService::find_config_location(&STANDARD, home.path(), &legacy_dir).unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
    }

    #[test]
    fn test_find_config_location_uses_unified_json_fallback() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.json");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(&unified_path, r#"{"[opencode]":{"agents":{}}}"#).unwrap();
        std::fs::write(
            legacy_dir.join(STANDARD.preferred_filename),
            r#"{"agents":{}}"#,
        )
        .unwrap();

        let found = OmoService::find_config_location(&STANDARD, home.path(), &legacy_dir).unwrap();

        assert_eq!(found, Some(OmoConfigLocation::Unified(unified_path)));
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
    fn test_unified_config_rejects_non_object_opencode_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{"[opencode]":[],"[codex]":{"agents":{}}}"#;
        std::fs::write(&path, original).unwrap();

        let read_result = OmoService::read_config_object(&OmoConfigLocation::Unified(path.clone()));
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        let write_result = document.set_opencode_section(&serde_json::json!({"agents": {}}));

        assert!(read_result.is_err());
        assert!(write_result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
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
    fn test_unified_config_semantic_noop_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{
  "[opencode]": {
    // Keep this agent explanation and the original quoting.
    agents: {'sisyphus': {model: 'openai/gpt-5.3'}},
  },
}"#;
        std::fs::write(&path, original).unwrap();
        let root: Value = json5::from_str(original).unwrap();
        let desired = root[OPENCODE_SECTION_KEY].clone();

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        let changed = document.set_opencode_section(&desired).unwrap();

        assert!(!changed);
        assert_eq!(document.text.to_string(), original);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unified_config_merge_preserves_nested_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{
  "[opencode]": {
    // Keep the agents explanation.
    "agents": {
      "sisyphus": {
        // Keep the model explanation.
        "model": "old/model",
        "temperature": 0.2
      }
    },
    "disabled_agents": [
      "one", // Keep the first array item explanation.
      "two"
    ]
  }
}"#;
        std::fs::write(&path, original).unwrap();
        let desired = serde_json::json!({
            "agents": {
                "sisyphus": {
                    "model": "new/model",
                    "temperature": 0.2
                }
            },
            "disabled_agents": ["one", "two", "three"]
        });

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        assert!(document.set_opencode_section(&desired).unwrap());
        document.save().unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert_eq!(root[OPENCODE_SECTION_KEY], desired);
        assert!(source.contains("// Keep the agents explanation."));
        assert!(source.contains("// Keep the model explanation."));
        assert!(source.contains("// Keep the first array item explanation."));
    }

    #[test]
    fn test_unified_config_write_inserts_missing_opencode_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(
            &path,
            r#"{
  "_migrations": ["2026-07-opencode-config-unification"],
  // Keep the Codex section in place.
  "[codex]": {"agents": {"reviewer": {"model": "openai/gpt-5.4"}}}
}"#,
        )
        .unwrap();

        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({
                "agents": {"sisyphus": {"model": "openai/gpt-5.3"}}
            }))
            .unwrap();
        document.save().unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert_eq!(
            root["[opencode]"]["agents"]["sisyphus"]["model"],
            "openai/gpt-5.3"
        );
        assert_eq!(
            root["[codex]"]["agents"]["reviewer"]["model"],
            "openai/gpt-5.4"
        );
        assert!(source.contains("// Keep the Codex section in place."));
    }

    #[test]
    fn test_disable_then_reenable_stays_on_unified_path() {
        let home = tempfile::tempdir().unwrap();
        let unified_path = home.path().join(".omo").join("omo.jsonc");
        let legacy_dir = home.path().join(".config").join("opencode");
        std::fs::create_dir_all(unified_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            &unified_path,
            r#"{
  "_migrations": ["2026-07-opencode-config-unification"],
  "[opencode]": {"agents": {"old": {}}},
  "[codex]": {"agents": {"reviewer": {}}}
}"#,
        )
        .unwrap();

        OmoService::remove_unified_config_section(&unified_path).unwrap();
        let location =
            OmoService::find_config_location(&STANDARD, home.path(), &legacy_dir).unwrap();
        assert_eq!(
            location,
            Some(OmoConfigLocation::Unified(unified_path.clone()))
        );

        let mut document = UnifiedConfigDocument::load(&unified_path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();
        document.save().unwrap();

        let root = OmoService::read_jsonc_object(&unified_path).unwrap();
        assert!(root["[opencode]"]["agents"].get("new").is_some());
        assert!(root.contains_key("[codex]"));
        assert_eq!(
            root["_migrations"],
            serde_json::json!(["2026-07-opencode-config-unification"])
        );
    }

    #[test]
    fn test_unified_config_write_rejects_block_comment_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{ /* keep */ "[opencode]": {"agents": {}} }"#;
        std::fs::write(&path, original).unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();

        let result = document.save();

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unified_config_write_rejects_parseable_semantic_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        let original = r#"{
  /* block */
  "[codex]": {"agents": {"reviewer": {}}},
  // tail */
  "[opencode]": {"agents": {}}
}"#;
        std::fs::write(&path, original).unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({"agents": {"new": {}}}))
            .unwrap();

        let result = document.save();

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_unified_config_write_preserves_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omo.jsonc");
        std::fs::write(&path, "{\r\n  \"[codex]\": {\"agents\": {}}\r\n}\r\n").unwrap();
        let mut document = UnifiedConfigDocument::load(&path).unwrap();
        document
            .set_opencode_section(&serde_json::json!({
                "agents": {"sisyphus": {"model": "openai/gpt-5.3"}}
            }))
            .unwrap();

        document.save().unwrap();

        let source = std::fs::read_to_string(&path).unwrap();
        assert!(!source.replace("\r\n", "").contains('\n'));
        assert_eq!(
            OmoService::read_jsonc_object(&path).unwrap()["[opencode]"]["agents"]["sisyphus"]
                ["model"],
            "openai/gpt-5.3"
        );
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

        assert!(OmoService::remove_unified_config_section(&path)
            .unwrap()
            .is_some());

        let source = std::fs::read_to_string(&path).unwrap();
        let root = OmoService::read_jsonc_object(&path).unwrap();
        assert!(!root.contains_key(OPENCODE_SECTION_KEY));
        assert!(root.contains_key("[codex]"));
        assert_eq!(root["_migrations"], serde_json::json!(["done"]));
        assert!(source.contains("// Keep this Codex explanation when disabling OpenCode OMO."));

        assert!(OmoService::remove_unified_config_section(&path)
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
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
