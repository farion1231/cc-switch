//! Global OMP model configuration and cached catalog; native credentials are never read here.

use super::{manual_edit, parse, read_text, write_and_commit};
use crate::error::AppError;
use indexmap::IndexMap;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
const FIELDS: &[&str] = &[
    "name",
    "contextWindow",
    "maxContextWindow",
    "maxTokens",
    "input",
    "reasoning",
];
const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelManagementState {
    scope: &'static str,
    settings_path: String,
    models_path: String,
    settings_revision: String,
    models_revision: String,
    default_model: Option<String>,
    default_thinking_level: Option<String>,
    model_roles: IndexMap<String, Value>,
    role_names: Vec<String>,
    agent_model_overrides: IndexMap<String, Value>,
    model_overrides: Vec<ModelOverride>,
    catalog_status: &'static str,
    catalog_message: Option<&'static str>,
    models: Vec<CatalogModel>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelOverride {
    provider: String,
    model_id: String,
    parameters: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogModel {
    provider: String,
    id: String,
    kind: Option<String>,
    name: String,
    context_window: Option<u64>,
    max_context_window: Option<u64>,
    max_tokens: Option<u64>,
    input: Vec<String>,
    reasoning: bool,
    thinking: Option<Value>,
    source: &'static str,
    cache_updated_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsPatch {
    expected_revision: String,
    expected_path: String,
    changes: Vec<SettingsEntryPatch>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsEntryPatch {
    section: String,
    key: String,
    expected: Value,
    value: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelOverridePatch {
    expected_revision: String,
    expected_path: String,
    provider: String,
    model_id: String,
    changes: Vec<ModelParameterPatch>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelParameterPatch {
    field: String,
    expected: Value,
    value: Value,
}

fn revision(bytes: Option<&[u8]>) -> String {
    bytes.map_or_else(
        || "missing".into(),
        |bytes| format!("{:x}", Sha256::digest(bytes)),
    )
}

fn invalid_settings() -> AppError {
    AppError::Config("omp.errors.invalidModelSettings".into())
}

fn invalid_override() -> AppError {
    AppError::Config("omp.errors.invalidModelOverride".into())
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn selector(value: &Value, arrays: bool) -> bool {
    value.as_str().is_some_and(nonempty)
        || (arrays
            && value.as_array().is_some_and(|values| {
                !values.is_empty() && values.iter().all(|v| v.as_str().is_some_and(nonempty))
            }))
}

fn at<'a>(document: &'a serde_yaml::Value, path: &[&str]) -> Option<&'a serde_yaml::Value> {
    path.iter().try_fold(document, |node, key| node.get(*key))
}

fn json_at(document: &serde_yaml::Value, path: &[&str]) -> Result<Value, AppError> {
    at(document, path).map_or(Ok(Value::Null), |value| {
        serde_json::to_value(value).map_err(|_| manual_edit())
    })
}

fn set_leaf(
    document: &mut serde_yaml::Value,
    path: &[&str],
    value: Option<&Value>,
) -> Result<(), AppError> {
    let (key, rest) = path.split_first().ok_or_else(manual_edit)?;
    if document.is_null() {
        *document = serde_yaml::Value::Mapping(Default::default());
    }
    let map = document.as_mapping_mut().ok_or_else(manual_edit)?;
    let key = serde_yaml::Value::String((*key).into());
    if rest.is_empty() {
        if let Some(value) = value {
            map.insert(key, serde_yaml::to_value(value).map_err(|_| manual_edit())?);
        } else {
            map.remove(&key);
        }
    } else if value.is_some() || map.contains_key(&key) {
        let child = map
            .entry(key)
            .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
        set_leaf(child, rest, value)?;
    }
    Ok(())
}

/// Only mapping delimiters outside quotes count; an opaque model key can contain `:` or `#`.
fn delimiter(line: &str) -> Result<usize, AppError> {
    let mut quote = None;
    let mut escaped = false;
    let mut chars = line.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && c == '\\' {
            escaped = true;
            continue;
        }
        if quote == Some('\'') && c == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
            chars.next();
            continue;
        }
        if quote == Some(c) {
            quote = None;
            continue;
        }
        if quote.is_some() {
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
        }
        if c == ':' && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) {
            return Ok(offset);
        }
    }
    Err(manual_edit())
}

fn emit(path: &[&str], value: &Value, indent: usize, newline: &str) -> Result<String, AppError> {
    let mut fragment = value.clone();
    for key in path.iter().rev() {
        fragment = serde_json::json!({*key: fragment});
    }
    let yaml = serde_yaml::to_string(&fragment).map_err(|_| manual_edit())?;
    Ok(yaml
        .lines()
        .map(|line| format!("{}{line}{newline}", " ".repeat(indent)))
        .collect())
}

/// Walk only block-mapping ancestors. Off-path values (including scripts/block strings) stay opaque.
fn splice(
    text: &str,
    path: &[&str],
    value: Option<&Value>,
    indent: usize,
) -> Result<String, AppError> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut children = Vec::new();
    let mut offsets = vec![0];
    for (i, line) in lines.iter().enumerate() {
        offsets.push(offsets[i] + line.len());
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces < indent || (spaces == indent && trimmed == "...") {
            return Err(manual_edit());
        }
        if spaces != indent {
            continue;
        }
        if trimmed == "{}" {
            continue;
        }
        if line[spaces..].starts_with('\t') {
            return Err(manual_edit());
        }
        let colon = delimiter(line)?;
        let header: serde_yaml::Mapping =
            serde_yaml::from_str(&format!("{}: null", &line[..colon]))
                .map_err(|_| manual_edit())?;
        let key = header
            .keys()
            .next()
            .and_then(serde_yaml::Value::as_str)
            .ok_or_else(manual_edit)?
            .to_string();
        if key == "<<" {
            return Err(manual_edit());
        }
        children.push((i, key, colon));
    }
    let key = path.first().ok_or_else(manual_edit)?;
    let target = children
        .iter()
        .position(|(_, candidate, _)| candidate == key);
    let Some(index) = target else {
        let Some(value) = value else {
            return Ok(text.into());
        };
        let mut stop = lines.len();
        while stop > 0
            && (lines[stop - 1].trim().is_empty() || lines[stop - 1].trim_start().starts_with('#'))
        {
            stop -= 1;
        }
        let mut output = text.to_string();
        let mut fragment = emit(path, value, indent, newline)?;
        if offsets[stop] > 0 && !text[..offsets[stop]].ends_with('\n') {
            fragment.insert_str(0, newline);
        }
        // An empty mapping's standalone marker is replaced locally, never left beside new entries.
        let empty = lines.iter().position(|line| line.trim() == "{}");
        if let Some(empty) = empty {
            output.replace_range(offsets[empty]..offsets[empty + 1], &fragment);
        } else {
            output.insert_str(offsets[stop], &fragment);
        }
        return Ok(output);
    };
    let (start, _, colon) = children[index];
    let mut stop = children.get(index + 1).map_or(lines.len(), |child| child.0);
    while stop > start + 1
        && (lines[stop - 1].trim().is_empty() || lines[stop - 1].trim_start().starts_with('#'))
    {
        stop -= 1;
    }
    let line = lines[start];
    super::reject_yaml_tokens(line)?;
    let tail = line[colon + 1..].trim();
    if tail.starts_with('|') || tail.starts_with('>') {
        return Err(manual_edit());
    }
    let replacement = if path.len() == 1 {
        for entry in &lines[start..stop] {
            super::reject_yaml_tokens(entry)?;
        }
        match value {
            Some(value) => emit(path, value, indent, newline)?,
            None if children.len() == 1 => format!("{}{{}}{newline}", " ".repeat(indent)),
            None => String::new(),
        }
    } else {
        let token = tail.split('#').next().unwrap_or_default().trim();
        if !matches!(token, "" | "null" | "~" | "{}") {
            return Err(manual_edit());
        }
        let body = &text[offsets[start + 1]..offsets[stop]];
        let child_indent = body
            .lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .map_or(indent + 2, |line| {
                line.len() - line.trim_start_matches(' ').len()
            });
        if child_indent <= indent {
            return Err(manual_edit());
        }
        let body = splice(body, &path[1..], value, child_indent)?;
        let header = if token.is_empty() {
            line.to_string()
        } else {
            let comment = tail.find('#').map(|index| &tail[index..]);
            format!(
                "{}:{}{newline}",
                &line[..colon],
                comment.map_or(String::new(), |comment| format!(" {comment}"))
            )
        };
        format!("{header}{body}")
    };
    let mut output = text.to_string();
    output.replace_range(offsets[start]..offsets[stop], &replacement);
    Ok(output)
}

fn edit_leaf(text: &str, path: &[&str], value: Option<&Value>) -> Result<String, AppError> {
    let mut intended = parse(text)?;
    set_leaf(&mut intended, path, value)?;
    let output = splice(text, path, value, 0)?;
    if parse(&output)? != intended {
        return Err(manual_edit());
    }
    Ok(output)
}

fn references(
    document: &serde_yaml::Value,
    path: &[&str],
) -> Result<IndexMap<String, Value>, AppError> {
    let mut node = document;
    for key in path {
        if node.is_null() {
            return Ok(IndexMap::new());
        }
        let map = node.as_mapping().ok_or_else(invalid_settings)?;
        let Some(child) = map.get(serde_yaml::Value::String((*key).into())) else {
            return Ok(IndexMap::new());
        };
        node = child;
    }
    if node.is_null() {
        return Ok(IndexMap::new());
    }
    let map = node.as_mapping().ok_or_else(invalid_settings)?;
    map.iter()
        .map(|(key, value)| {
            let key = key
                .as_str()
                .filter(|key| nonempty(key))
                .ok_or_else(invalid_settings)?;
            let value = serde_json::to_value(value).map_err(|_| invalid_settings())?;
            if !selector(&value, true) {
                return Err(invalid_settings());
            }
            Ok((key.to_string(), value))
        })
        .collect()
}

fn thinking_level(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| EFFORTS.contains(&value) || value == "auto")
}

fn parameter(field: &str, value: &Value) -> bool {
    match field {
        "name" => value.as_str().is_some_and(nonempty),
        "contextWindow" | "maxContextWindow" | "maxTokens" => value
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 9_007_199_254_740_991),
        "reasoning" => value.is_boolean(),
        "input" => value.as_array().is_some_and(|values| {
            values
                .iter()
                .all(|value| matches!(value.as_str(), Some("text" | "image")))
        }),
        _ => false,
    }
}

fn safe_thinking(value: &Value) -> Option<Value> {
    let mode = value.get("mode")?.as_str()?;
    if ![
        "effort",
        "budget",
        "google-level",
        "anthropic-adaptive",
        "anthropic-budget-effort",
    ]
    .contains(&mode)
    {
        return None;
    }
    let efforts = value
        .get("efforts")
        .or_else(|| value.get("levels"))?
        .as_array()?;
    if efforts.is_empty()
        || efforts
            .iter()
            .any(|level| !level.as_str().is_some_and(|level| EFFORTS.contains(&level)))
    {
        return None;
    }
    let mut output = serde_json::json!({"mode":mode,"efforts":efforts});
    if let Some(level) = value
        .get("defaultLevel")
        .and_then(Value::as_str)
        .filter(|level| EFFORTS.contains(level))
    {
        output["defaultLevel"] = Value::String(level.into());
    }
    if let Some(required) = value.get("requiresEffort").and_then(Value::as_bool) {
        output["requiresEffort"] = Value::Bool(required);
    }
    Some(output)
}

fn overlay(model: &mut CatalogModel, node: &serde_json::Map<String, Value>) {
    if let Some(value) = node.get("name").and_then(Value::as_str) {
        model.name = value.into();
    }
    for (field, target) in [
        ("contextWindow", &mut model.context_window),
        ("maxContextWindow", &mut model.max_context_window),
        ("maxTokens", &mut model.max_tokens),
    ] {
        if let Some(value) = node.get(field).and_then(Value::as_u64) {
            *target = Some(value);
        }
    }
    if let Some(value) = node.get("reasoning").and_then(Value::as_bool) {
        model.reasoning = value;
    }
    if let Some(values) = node.get("input").and_then(Value::as_array) {
        model.input = values
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| matches!(*value, "text" | "image"))
            .map(str::to_owned)
            .collect();
    }
    if let Some(value) = node.get("thinking") {
        model.thinking = safe_thinking(value);
    }
    if let Some(value) = node.get("kind").and_then(Value::as_str) {
        if [
            "chat",
            "tiny",
            "image",
            "tts",
            "stt",
            "search",
            "judge",
            "embedding",
            "rerank",
            "video",
        ]
        .contains(&value)
        {
            model.kind = Some(value.into());
        }
    }
}

type CachedModels = BTreeMap<(String, String), CatalogModel>;

fn cached_catalog() -> Result<(CachedModels, bool), ()> {
    let path = super::get_omp_data_dir().map_err(|_| ())?.join("models.db");
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| ())?;
    db.busy_timeout(std::time::Duration::ZERO).map_err(|_| ())?;
    let unsupported: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM model_cache WHERE version != 14)",
            [],
            |row| row.get(0),
        )
        .map_err(|_| ())?;
    // Never transfer the complete cache payload (which can contain headers/account metadata).
    // A bounded per-model SQL projection is the only data crossing the SQLite boundary.
    let mut statement = db
        .prepare(
            "SELECT m.updated_at, CASE WHEN length(j.value) <= 262144 THEN json_object(
          'provider',json_extract(j.value,'$.provider'), 'id',json_extract(j.value,'$.id'),
          'name',json_extract(j.value,'$.name'), 'kind',json_extract(j.value,'$.kind'),
          'contextWindow',json_extract(j.value,'$.contextWindow'),
          'maxContextWindow',json_extract(j.value,'$.maxContextWindow'),
          'maxTokens',json_extract(j.value,'$.maxTokens'),
          'input',json_extract(j.value,'$.input'), 'reasoning',json_extract(j.value,'$.reasoning'),
          'thinking',json_object('mode',json_extract(j.value,'$.thinking.mode'),
            'efforts',json_extract(j.value,'$.thinking.efforts'),
            'defaultLevel',json_extract(j.value,'$.thinking.defaultLevel'),
            'requiresEffort',json_extract(j.value,'$.thinking.requiresEffort'))
          ) ELSE NULL END
         FROM model_cache m, json_each(m.models) j
         WHERE m.version=14 ORDER BY m.updated_at, m.provider_id, j.key LIMIT 50001",
        )
        .map_err(|_| ())?;
    let mut rows = statement.query([]).map_err(|_| ())?;
    let mut models = BTreeMap::new();
    let mut count = 0;
    while let Some(row) = rows.next().map_err(|_| ())? {
        count += 1;
        if count > 50000 {
            return Err(());
        }
        let updated: i64 = row.get(0).map_err(|_| ())?;
        let projected: String = row.get(1).map_err(|_| ())?;
        let mut metadata: Value = serde_json::from_str(&projected).map_err(|_| ())?;
        // SQLite represents JSON booleans extracted as SQL scalars as 0/1.
        if let Some(reasoning) = metadata.get("reasoning").and_then(Value::as_i64) {
            metadata["reasoning"] = Value::Bool(reasoning != 0);
        }
        if let Some(required) = metadata
            .pointer("/thinking/requiresEffort")
            .and_then(Value::as_i64)
        {
            metadata["thinking"]["requiresEffort"] = Value::Bool(required != 0);
        }
        let provider = metadata
            .get("provider")
            .and_then(Value::as_str)
            .filter(|value| nonempty(value))
            .ok_or(())?;
        let id = metadata
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| nonempty(value))
            .ok_or(())?;
        let mut model = CatalogModel {
            provider: provider.into(),
            id: id.into(),
            kind: None,
            name: id.into(),
            context_window: None,
            max_context_window: None,
            max_tokens: None,
            input: Vec::new(),
            reasoning: false,
            thinking: None,
            source: "nativeCache",
            cache_updated_at: Some(updated),
        };
        overlay(&mut model, metadata.as_object().ok_or(())?);
        models.insert((model.provider.clone(), model.id.clone()), model);
    }
    Ok((models, unsupported))
}

pub(crate) fn get_state() -> Result<ModelManagementState, AppError> {
    let _settings = SETTINGS_LOCK.lock()?;
    let _models = super::MODELS_LOCK.lock()?;
    let settings_path = super::get_omp_settings_path()?;
    let models_path = super::get_omp_models_path()?;
    let (settings_bytes, settings_text) = read_text(&settings_path)?;
    let (models_bytes, models_text) = read_text(&models_path)?;
    snapshot(
        &settings_path,
        &models_path,
        settings_bytes.as_deref(),
        models_bytes.as_deref(),
        &settings_text,
        &models_text,
    )
}

fn snapshot(
    settings_path: &std::path::Path,
    models_path: &std::path::Path,
    settings_bytes: Option<&[u8]>,
    models_bytes: Option<&[u8]>,
    settings_text: &str,
    models_text: &str,
) -> Result<ModelManagementState, AppError> {
    let settings = parse(settings_text)?;
    let native = super::providers(&parse(models_text)?)?;
    let roles = references(&settings, &["modelRoles"])?;
    let workers = references(&settings, &["task", "agentModelOverrides"])?;
    let default_thinking = json_at(&settings, &["defaultThinkingLevel"])?;
    if !default_thinking.is_null() && !thinking_level(&default_thinking) {
        return Err(invalid_settings());
    }
    let default_model = roles.get("default").map(|value| {
        value.as_str().map(str::to_owned).unwrap_or_else(|| {
            value
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
    });
    let mut role_names: Vec<String> = [
        "default",
        "smol",
        "slow",
        "vision",
        "plan",
        "commit",
        "tiny",
        "memory",
        "task",
        "advisor",
        "widget",
        "image",
        "web",
        "speech",
        "dictation",
        "judge",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    role_names.extend(roles.keys().cloned());
    if let Some(values) = settings
        .get("cycleOrder")
        .and_then(serde_yaml::Value::as_sequence)
    {
        role_names.extend(
            values
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_owned),
        );
    }
    if let Some(values) = settings
        .get("modelTags")
        .and_then(serde_yaml::Value::as_mapping)
    {
        role_names.extend(
            values
                .keys()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_owned),
        );
    }
    let mut seen = HashSet::new();
    role_names.retain(|role| seen.insert(role.clone()));
    let (mut models, status, message) = if !super::get_omp_data_dir()?.join("models.db").exists() {
        (
            BTreeMap::new(),
            "unavailable",
            Some("omp.errors.catalogUnavailable"),
        )
    } else {
        match cached_catalog() {
            Ok((models, false)) => (models, "ready", None),
            Ok((models, true)) => (models, "unsupported", Some("omp.errors.catalogUnsupported")),
            Err(()) => (BTreeMap::new(), "error", Some("omp.errors.catalogRead")),
        }
    };
    let mut overrides = Vec::new();
    for (provider, node) in native {
        if let Some(entries) = node.get("models") {
            for entry in entries.as_array().ok_or_else(invalid_override)? {
                let object = entry.as_object().ok_or_else(invalid_override)?;
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| nonempty(id))
                    .ok_or_else(invalid_override)?;
                let model = models
                    .entry((provider.clone(), id.into()))
                    .or_insert_with(|| CatalogModel {
                        provider: provider.clone(),
                        id: id.into(),
                        kind: None,
                        name: id.into(),
                        context_window: None,
                        max_context_window: None,
                        max_tokens: None,
                        input: Vec::new(),
                        reasoning: false,
                        thinking: None,
                        source: "configured",
                        cache_updated_at: None,
                    });
                overlay(model, object);
                model.source = "configured";
            }
        }
        if let Some(entries) = node.get("modelOverrides") {
            for (id, entry) in entries.as_object().ok_or_else(invalid_override)? {
                let entry = entry.as_object().ok_or_else(invalid_override)?;
                let mut parameters = serde_json::Map::new();
                for field in FIELDS {
                    if let Some(value) = entry.get(*field) {
                        if !parameter(field, value) {
                            return Err(invalid_override());
                        }
                        parameters.insert((*field).into(), value.clone());
                    }
                }
                if let Some(model) = models.get_mut(&(provider.clone(), id.clone())) {
                    overlay(model, &parameters);
                }
                overrides.push(ModelOverride {
                    provider: provider.clone(),
                    model_id: id.clone(),
                    parameters,
                });
            }
        }
    }
    Ok(ModelManagementState {
        scope: "global",
        settings_path: settings_path.to_string_lossy().into_owned(),
        models_path: models_path.to_string_lossy().into_owned(),
        settings_revision: revision(settings_bytes),
        models_revision: revision(models_bytes),
        default_model,
        default_thinking_level: default_thinking.as_str().map(str::to_owned),
        model_roles: roles,
        role_names,
        agent_model_overrides: workers,
        model_overrides: overrides,
        catalog_status: status,
        catalog_message: message,
        models: models.into_values().collect(),
    })
}

fn baseline(
    path: &std::path::Path,
    expected_path: &str,
    expected_revision: &str,
) -> Result<(), AppError> {
    if path.to_string_lossy() != expected_path
        || (expected_revision != "missing"
            && (expected_revision.len() != 64
                || !expected_revision.bytes().all(|c| c.is_ascii_hexdigit())))
    {
        return Err(AppError::Conflict("omp.errors.externalChange".into()));
    }
    Ok(())
}

fn expect(document: &serde_yaml::Value, path: &[&str], expected: &Value) -> Result<(), AppError> {
    if json_at(document, path)? != *expected {
        return Err(AppError::Conflict("omp.errors.externalChange".into()));
    }
    Ok(())
}

pub(crate) fn patch_settings(patch: SettingsPatch) -> Result<ModelManagementState, AppError> {
    let result;
    {
        let _guard = SETTINGS_LOCK.lock()?;
        let _models = super::MODELS_LOCK.lock()?;
        let path = super::get_omp_settings_path()?;
        baseline(&path, &patch.expected_path, &patch.expected_revision)?;
        let (before, mut text) = read_text(&path)?;
        if before.is_none() && path.with_file_name("settings.json").exists() {
            return Err(AppError::Config("omp.errors.legacySettingsJson".into()));
        }
        let document = parse(&text)?;
        references(&document, &["modelRoles"])?;
        references(&document, &["task", "agentModelOverrides"])?;
        if patch.changes.is_empty() {
            return Err(invalid_settings());
        }
        let mut seen = HashSet::new();
        for change in &patch.changes {
            if !nonempty(&change.key) || !seen.insert((&change.section, &change.key)) {
                return Err(invalid_settings());
            }
            let path = match change.section.as_str() {
                "modelRoles" => vec!["modelRoles", &change.key],
                "agentModelOverrides" => vec!["task", "agentModelOverrides", &change.key],
                "defaultThinkingLevel" if change.key == "defaultThinkingLevel" => {
                    vec!["defaultThinkingLevel"]
                }
                _ => return Err(invalid_settings()),
            };
            let valid = |value: &Value| {
                value.is_null()
                    || if change.section == "defaultThinkingLevel" {
                        thinking_level(value)
                    } else {
                        selector(value, true)
                    }
            };
            if !valid(&change.expected) || !valid(&change.value) {
                return Err(invalid_settings());
            }
            expect(&document, &path, &change.expected)?;
            text = edit_leaf(
                &text,
                &path,
                (!change.value.is_null()).then_some(&change.value),
            )?;
        }
        let models_path = super::get_omp_models_path()?;
        let (models_bytes, models_text) = read_text(&models_path)?;
        result = snapshot(
            &path,
            &models_path,
            Some(text.as_bytes()),
            models_bytes.as_deref(),
            &text,
            &models_text,
        )?;
        if before.as_deref() != Some(text.as_bytes()) {
            write_and_commit(&path, before.as_deref(), text.as_bytes(), || Ok(()))?;
        }
    }
    Ok(result)
}

pub(crate) fn patch_override(patch: ModelOverridePatch) -> Result<ModelManagementState, AppError> {
    let result;
    {
        let _settings = SETTINGS_LOCK.lock()?;
        let _guard = super::MODELS_LOCK.lock()?;
        let path = super::get_omp_models_path()?;
        baseline(&path, &patch.expected_path, &patch.expected_revision)?;
        let (before, mut text) = read_text(&path)?;
        if before.is_none() && path.with_file_name("models.json").exists() {
            return Err(AppError::Config("omp.errors.legacyModelsJson".into()));
        }
        if !nonempty(&patch.provider) || !nonempty(&patch.model_id) || patch.changes.is_empty() {
            return Err(invalid_override());
        }
        let document = parse(&text)?;
        let mut seen = HashSet::new();
        for change in &patch.changes {
            if !FIELDS.contains(&change.field.as_str())
                || !seen.insert(&change.field)
                || (!change.value.is_null() && !parameter(&change.field, &change.value))
            {
                return Err(invalid_override());
            }
            let path = [
                "providers",
                &patch.provider,
                "modelOverrides",
                &patch.model_id,
                &change.field,
            ];
            expect(&document, &path, &change.expected)?;
            text = edit_leaf(
                &text,
                &path,
                (!change.value.is_null()).then_some(&change.value),
            )?;
        }
        let edited = parse(&text)?;
        let base = [
            "providers",
            &patch.provider,
            "modelOverrides",
            &patch.model_id,
        ];
        if let Some(node) = at(&edited, &base) {
            let node = node.as_mapping().ok_or_else(invalid_override)?;
            if node.is_empty() {
                text = edit_leaf(&text, &base, None)?;
                let edited = parse(&text)?;
                let overrides = ["providers", &patch.provider, "modelOverrides"];
                if at(&edited, &overrides)
                    .and_then(serde_yaml::Value::as_mapping)
                    .is_some_and(|map| map.is_empty())
                {
                    text = edit_leaf(&text, &overrides, None)?;
                    let edited = parse(&text)?;
                    let provider_path = ["providers", &patch.provider];
                    let provider = at(&edited, &provider_path)
                        .and_then(serde_yaml::Value::as_mapping)
                        .ok_or_else(invalid_override)?;
                    if provider.is_empty()
                        || (provider.len() == 1
                            && provider
                                .get(serde_yaml::Value::String("auth".into()))
                                .and_then(serde_yaml::Value::as_str)
                                == Some("oauth"))
                    {
                        text = edit_leaf(&text, &provider_path, None)?;
                    }
                }
            } else {
                let context = node
                    .get(serde_yaml::Value::String("contextWindow".into()))
                    .and_then(serde_yaml::Value::as_u64);
                let max = node
                    .get(serde_yaml::Value::String("maxContextWindow".into()))
                    .and_then(serde_yaml::Value::as_u64);
                if context.zip(max).is_some_and(|(context, max)| max < context) {
                    return Err(invalid_override());
                }
            }
        }
        let settings_path = super::get_omp_settings_path()?;
        let (settings_bytes, settings_text) = read_text(&settings_path)?;
        result = snapshot(
            &settings_path,
            &path,
            settings_bytes.as_deref(),
            Some(text.as_bytes()),
            &settings_text,
            &text,
        )?;
        if result
            .models
            .iter()
            .filter(|model| model.provider == patch.provider && model.id == patch.model_id)
            .any(|model| {
                model
                    .context_window
                    .zip(model.max_context_window)
                    .is_some_and(|(context, max)| max < context)
            })
        {
            return Err(invalid_override());
        }
        if before.as_deref() != Some(text.as_bytes()) {
            write_and_commit(&path, before.as_deref(), text.as_bytes(), || Ok(()))?;
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod behavior_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn management_leaf_edit_preserves_sibling_bytes_and_unknown_fields() {
        let before = "# heading\nmodelRoles: # global\n  default: old/model:xhigh\n  custom: '@task' # keep alias\ntask:\n  limit: 4 # unknown\n  agentModelOverrides:\n    scout: [old/model, '@smol']\nfooter: 'quoted' # keep\n";
        let after = edit_leaf(
            before,
            &["modelRoles", "default"],
            Some(&json!("new/model:high")),
        )
        .unwrap();
        assert_eq!(after, "# heading\nmodelRoles: # global\n  default: new/model:high\n  custom: '@task' # keep alias\ntask:\n  limit: 4 # unknown\n  agentModelOverrides:\n    scout: [old/model, '@smol']\nfooter: 'quoted' # keep\n");
        let after = edit_leaf(
            &after,
            &["task", "agentModelOverrides", "scout"],
            Some(&json!(["@task", "new/model:low"])),
        )
        .unwrap();
        let parsed = super::super::parse(&after).unwrap();
        assert_eq!(
            serde_json::to_value(&parsed["task"]["agentModelOverrides"]["scout"]).unwrap(),
            json!(["@task", "new/model:low"])
        );
        assert!(after.contains("custom: '@task' # keep alias\ntask:\n  limit: 4 # unknown\n"));
        assert!(after.ends_with("footer: 'quoted' # keep\n"));
    }

    #[test]
    fn management_leaf_edit_creates_only_missing_parents_and_handles_last_removal() {
        let after = edit_leaf(
            "# untouched\nother: yes\n",
            &[
                "providers",
                "openai-codex",
                "modelOverrides",
                "model",
                "maxTokens",
            ],
            Some(&json!(4096)),
        )
        .unwrap();
        assert!(after.starts_with("# untouched\nother: yes\n"));
        assert_eq!(
            super::super::parse(&after).unwrap()["providers"]["openai-codex"]["modelOverrides"]
                ["model"]["maxTokens"]
                .as_i64(),
            Some(4096)
        );
        let after = edit_leaf(
            "modelRoles:\n  default: a/b\n# footer\n",
            &["modelRoles", "default"],
            None,
        )
        .unwrap();
        assert!(super::super::parse(&after).unwrap()["modelRoles"]
            .as_mapping()
            .unwrap()
            .is_empty());
        assert!(after.ends_with("# footer\n"));
    }

    #[test]
    fn management_leaf_edit_refuses_unsafe_parents_without_rewriting() {
        for text in [
            "modelRoles: {default: old/model}\n",
            "modelRoles: &roles\n  default: old/model\n",
            "modelRoles:\n  default: |\n    old/model\n",
            "modelRoles:\n  default: old/model\ncopy: *roles\n",
        ] {
            assert!(
                edit_leaf(text, &["modelRoles", "default"], Some(&json!("new/model"))).is_err(),
                "{text}"
            );
        }
    }
}
