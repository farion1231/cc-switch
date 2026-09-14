use super::model::{CopilotByokGroup, CopilotByokModel};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const STORE_FILE: &str = "copilot-byok.json";
const STORE_VERSION: u32 = 10;
const GROUPED_STORE_VERSION: u64 = 3;
const MAX_TARGETS: usize = 64;
const MAX_GROUPS: usize = 256;
const MAX_MODELS: usize = 256;
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;

fn default_store_version() -> u32 {
    STORE_VERSION
}

fn default_true() -> bool {
    true
}

fn default_api_type() -> String {
    "chat-completions".to_string()
}

fn default_context_window() -> u64 {
    128_000
}

fn default_max_output_tokens() -> u64 {
    8_192
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokCustomTarget {
    pub id: String,
    pub name: String,
    pub language_models_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliManagedEnvironment {
    /// Legacy pre-management values retained only until the v10 provider
    /// import migration has converted them into a normal catalog entry.
    #[serde(default)]
    pub original: BTreeMap<String, Option<String>>,
    /// Last values written by CC Switch. These are compared before a switch so
    /// an external edit is never overwritten silently.
    #[serde(default)]
    pub last_written: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Legacy v9 marker. v10 imports the retained environment as a normal
    /// provider and clears this flag.
    #[serde(default)]
    pub official_override_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model_id: Option<String>,
    #[serde(default)]
    pub managed_environment: CopilotCliManagedEnvironment,
}

impl CopilotByokCustomTarget {
    pub fn from_path(path: impl AsRef<Path>, name: Option<String>) -> Result<Self, AppError> {
        let normalized_path = normalize_language_models_path(path.as_ref())?;
        let normalized = normalized_path.to_string_lossy().to_string();
        Ok(Self {
            id: custom_target_id(&normalized),
            name: name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "Custom VS Code profile".to_string()),
            language_models_path: normalized,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokStore {
    #[serde(default = "default_store_version")]
    pub version: u32,
    #[serde(default)]
    pub targets_initialized: bool,
    #[serde(default)]
    pub selected_target_ids: Vec<String>,
    #[serde(default)]
    pub custom_targets: Vec<CopilotByokCustomTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<CopilotByokGroup>,
    #[serde(default)]
    pub cli: CopilotCliConfig,
    /// One-time migration marker for splitting the historical shared provider
    /// catalog into independent VS Code Copilot and Copilot CLI catalogs.
    #[serde(default)]
    pub cli_catalog_initialized: bool,
    /// One-time migration marker for collapsing the historical VS Code-style
    /// multi-model CLI catalog to one default model per provider.
    #[serde(default)]
    pub cli_single_model_catalog_initialized: bool,
    /// One-time migration marker for importing an environment that predates
    /// CC Switch management as a normal Copilot CLI provider.
    #[serde(default)]
    pub cli_environment_import_initialized: bool,
}

impl Default for CopilotByokStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            targets_initialized: false,
            selected_target_ids: Vec::new(),
            custom_targets: Vec::new(),
            groups: Vec::new(),
            cli: CopilotCliConfig::default(),
            cli_catalog_initialized: false,
            cli_single_model_catalog_initialized: false,
            cli_environment_import_initialized: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyStore {
    #[serde(default)]
    targets_initialized: bool,
    #[serde(default)]
    selected_target_ids: Vec<String>,
    #[serde(default)]
    custom_targets: Vec<CopilotByokCustomTarget>,
    #[serde(default)]
    config_path: Option<String>,
    #[serde(default)]
    models: Vec<LegacyModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyModel {
    #[serde(default)]
    id: String,
    model_id: String,
    name: String,
    url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default = "default_api_type")]
    api_type: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_true")]
    tool_calling: bool,
    #[serde(default)]
    vision: bool,
    #[serde(default = "default_true")]
    thinking: bool,
    #[serde(default = "default_true")]
    streaming: bool,
    #[serde(default = "default_context_window")]
    context_window: u64,
    #[serde(default)]
    max_input_tokens: Option<u64>,
    #[serde(default = "default_max_output_tokens")]
    max_output_tokens: u64,
    #[serde(default)]
    edit_tools: Vec<String>,
    #[serde(default)]
    zero_data_retention_enabled: bool,
    #[serde(default)]
    supports_reasoning_effort: Vec<String>,
    #[serde(default)]
    reasoning_effort_format: Option<String>,
    #[serde(default)]
    request_headers: BTreeMap<String, String>,
    #[serde(default)]
    model_options: Value,
}

impl LegacyModel {
    fn into_group(self) -> CopilotByokGroup {
        let id = if self.id.trim().is_empty() {
            let digest = Sha256::digest(
                format!("{}\0{}\0{}", self.name, self.model_id, self.url).as_bytes(),
            );
            let encoded = format!("{digest:x}");
            format!("legacy:{}", &encoded[..24])
        } else {
            self.id.trim().to_string()
        };
        CopilotByokGroup {
            id: id.clone(),
            name: self.name.clone(),
            url: self.url,
            api_key: self.api_key,
            api_type: self.api_type,
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            category: None,
            usage_script: None,
            enabled: self.enabled,
            request_headers: self.request_headers,
            models: vec![CopilotByokModel {
                id: format!("{id}:model"),
                model_id: self.model_id,
                name: self.name,
                enabled: true,
                tool_calling: Some(self.tool_calling),
                vision: Some(self.vision),
                thinking: Some(self.thinking),
                streaming: Some(self.streaming),
                context_window: Some(self.context_window),
                max_input_tokens: self.max_input_tokens,
                max_output_tokens: Some(self.max_output_tokens),
                edit_tools: self.edit_tools,
                zero_data_retention_enabled: self.zero_data_retention_enabled,
                supports_reasoning_effort: self.supports_reasoning_effort,
                reasoning_effort_format: self.reasoning_effort_format,
                model_options: if self.model_options.is_null() {
                    json!({})
                } else {
                    self.model_options
                },
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }
    }
}

fn custom_target_id(path: &str) -> String {
    let digest = Sha256::digest(path_identity_key(Path::new(path)).as_bytes());
    format!("custom:{:x}", digest)[..19].to_string()
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(path) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(path);
        }
    }
    path
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn normalize_path(path: &Path) -> Result<PathBuf, AppError> {
    let lexical = lexical_normalize(path);
    let mut ancestor = lexical.as_path();
    let mut tail = Vec::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            break;
        };
        tail.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            break;
        };
        ancestor = parent;
    }

    let mut normalized = if ancestor.exists() {
        fs::canonicalize(ancestor).map_err(|error| AppError::io(ancestor, error))?
    } else {
        ancestor.to_path_buf()
    };
    for component in tail.into_iter().rev() {
        normalized.push(component);
    }
    Ok(strip_verbatim_prefix(lexical_normalize(&normalized)))
}

pub(crate) fn normalize_language_models_path(path: &Path) -> Result<PathBuf, AppError> {
    validate_language_models_path(path)?;
    normalize_path(path)
}

pub(crate) fn path_identity_key(path: &Path) -> String {
    let normalized = normalize_path(path)
        .unwrap_or_else(|_| lexical_normalize(path))
        .to_string_lossy()
        .replace('\\', "/");
    #[cfg(windows)]
    {
        normalized.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

pub fn validate_language_models_path(path: &Path) -> Result<(), AppError> {
    if path.file_name().and_then(|value| value.to_str()) != Some("chatLanguageModels.json") {
        return Err(AppError::InvalidInput(
            "Custom Copilot BYOK target must end with chatLanguageModels.json".to_string(),
        ));
    }
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(
            "Custom Copilot BYOK target must be an absolute path".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn normalize_store(store: &mut CopilotByokStore) -> Result<(), AppError> {
    if store.version > STORE_VERSION {
        return Err(AppError::Config(format!(
            "Copilot BYOK store version {} is newer than supported version {STORE_VERSION}",
            store.version
        )));
    }
    if !store.selected_target_ids.is_empty() || !store.custom_targets.is_empty() {
        store.targets_initialized = true;
    }
    store.version = STORE_VERSION;

    store.cli.selected_group_id = store
        .cli
        .selected_group_id
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    store.cli.selected_model_id = store
        .cli
        .selected_model_id
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if !store.cli.enabled {
        store.cli.selected_group_id = None;
        store.cli.selected_model_id = None;
        if !store.cli.official_override_active {
            store.cli.managed_environment = CopilotCliManagedEnvironment::default();
        }
    } else {
        // A custom provider and the official-clear state are mutually
        // exclusive. Older or partially written stores prefer the concrete
        // custom selection when `enabled` is true.
        store.cli.official_override_active = false;
    }

    let mut selected = HashSet::new();
    store.selected_target_ids.retain(|id| {
        let trimmed = id.trim();
        !trimmed.is_empty() && selected.insert(trimmed.to_string())
    });
    for id in &mut store.selected_target_ids {
        *id = id.trim().to_string();
    }

    let mut target_paths = HashSet::new();
    let mut migrated_target_ids = std::collections::HashMap::new();
    let mut custom_targets = Vec::with_capacity(store.custom_targets.len());
    for mut target in std::mem::take(&mut store.custom_targets) {
        let old_id = target.id.trim().to_string();
        target.name = target.name.trim().to_string();
        let normalized =
            normalize_language_models_path(Path::new(target.language_models_path.trim()))?;
        target.language_models_path = normalized.to_string_lossy().to_string();
        target.id = custom_target_id(&target.language_models_path);
        migrated_target_ids.insert(old_id, target.id.clone());
        if target_paths.insert(path_identity_key(&normalized)) {
            custom_targets.push(target);
        }
    }
    store.custom_targets = custom_targets;
    for selected in &mut store.selected_target_ids {
        if let Some(migrated) = migrated_target_ids.get(selected) {
            *selected = migrated.clone();
        }
    }
    let mut selected = HashSet::new();
    store
        .selected_target_ids
        .retain(|id| selected.insert(id.clone()));

    if store.selected_target_ids.len() > MAX_TARGETS || store.custom_targets.len() > MAX_TARGETS {
        return Err(AppError::InvalidInput(format!(
            "Copilot BYOK supports at most {MAX_TARGETS} selected or custom targets"
        )));
    }
    normalize_groups(&mut store.groups)?;
    Ok(())
}

pub(crate) fn normalize_groups(groups: &mut Vec<CopilotByokGroup>) -> Result<(), AppError> {
    if groups.len() > MAX_GROUPS {
        return Err(AppError::InvalidInput(format!(
            "Copilot BYOK supports at most {MAX_GROUPS} provider groups"
        )));
    }
    if groups.iter().map(|group| group.models.len()).sum::<usize>() > MAX_MODELS {
        return Err(AppError::InvalidInput(format!(
            "Copilot BYOK supports at most {MAX_MODELS} models"
        )));
    }

    let mut group_ids = HashSet::new();
    let mut group_names = HashSet::new();
    for group in groups {
        group.normalize();
        group.validate()?;
        if !group_ids.insert(group.id.clone()) {
            return Err(AppError::InvalidInput(format!(
                "Duplicate Copilot BYOK provider id: {}",
                group.id
            )));
        }
        if !group_names.insert(group.name.to_lowercase()) {
            return Err(AppError::InvalidInput(format!(
                "Duplicate Copilot BYOK provider name: {}",
                group.name
            )));
        }
    }
    Ok(())
}

fn move_flattened_extra(object: &mut Map<String, Value>, known_fields: &[&str]) {
    let previous_extra = object.remove("extra");
    let mut extra = match previous_extra {
        Some(Value::Object(extra)) => extra,
        Some(value) => Map::from_iter([("extra".to_string(), value)]),
        None => Map::new(),
    };
    let extra_keys: Vec<String> = object
        .keys()
        .filter(|key| !known_fields.contains(&key.as_str()))
        .cloned()
        .collect();
    for key in extra_keys {
        if let Some(value) = object.remove(&key) {
            extra.entry(key).or_insert(value);
        }
    }
    object.insert("extra".to_string(), Value::Object(extra));
}

fn migrate_v4_flattened_extras(value: &mut Value) {
    let Some(groups) = value.get_mut("groups").and_then(Value::as_array_mut) else {
        return;
    };
    for group in groups {
        let Some(group) = group.as_object_mut() else {
            continue;
        };
        if let Some(models) = group.get_mut("models").and_then(Value::as_array_mut) {
            for model in models {
                let Some(model) = model.as_object_mut() else {
                    continue;
                };
                move_flattened_extra(
                    model,
                    &[
                        "id",
                        "modelId",
                        "name",
                        "enabled",
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
                );
            }
        }
        move_flattened_extra(
            group,
            &[
                "id",
                "name",
                "url",
                "apiKey",
                "apiType",
                "websiteUrl",
                "notes",
                "icon",
                "iconColor",
                "category",
                "enabled",
                "requestHeaders",
                "models",
            ],
        );
    }
}

pub(crate) fn parse_store_value(mut value: Value) -> Result<CopilotByokStore, AppError> {
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version < u64::from(STORE_VERSION) && value.get("groups").is_some() {
        migrate_v4_flattened_extras(&mut value);
    }
    let mut store = if value.get("groups").is_some() || version >= GROUPED_STORE_VERSION {
        serde_json::from_value(value).map_err(|error| {
            AppError::Config(format!("Failed to parse Copilot BYOK store: {error}"))
        })?
    } else {
        let legacy: LegacyStore = serde_json::from_value(value).map_err(|error| {
            AppError::Config(format!(
                "Failed to parse legacy Copilot BYOK store: {error}"
            ))
        })?;
        let mut migrated = CopilotByokStore {
            targets_initialized: legacy.targets_initialized,
            selected_target_ids: legacy.selected_target_ids,
            custom_targets: legacy.custom_targets,
            groups: legacy
                .models
                .into_iter()
                .map(LegacyModel::into_group)
                .collect(),
            ..CopilotByokStore::default()
        };
        if let Some(path) = legacy
            .config_path
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            let custom = CopilotByokCustomTarget::from_path(&path, Some("Migrated target".into()))?;
            migrated.targets_initialized = true;
            if !migrated.selected_target_ids.contains(&custom.id) {
                migrated.selected_target_ids.push(custom.id.clone());
            }
            if !migrated
                .custom_targets
                .iter()
                .any(|target| target.id == custom.id)
            {
                migrated.custom_targets.push(custom);
            }
        }
        migrated
    };

    normalize_store(&mut store)?;
    Ok(store)
}

pub fn store_path() -> PathBuf {
    // This file contains device-specific absolute paths and the last applied
    // Copilot CLI environment. Keep it outside the portable app-config
    // override, which users may intentionally place in Dropbox/OneDrive.
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join(STORE_FILE)
}

fn legacy_portable_store_path() -> Option<PathBuf> {
    let local = store_path();
    let legacy = crate::config::get_app_config_dir().join(STORE_FILE);
    (path_identity_key(&local) != path_identity_key(&legacy)).then_some(legacy)
}

fn load_store_at(path: &Path) -> Result<CopilotByokStore, AppError> {
    if !path.exists() {
        return Ok(CopilotByokStore::default());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "Copilot BYOK store is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_STORE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Copilot BYOK store exceeds {} MiB: {}",
            MAX_STORE_BYTES / 1024 / 1024,
            path.display()
        )));
    }
    let text = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    let value = serde_json::from_str(&text).map_err(|error| AppError::json(path, error))?;
    parse_store_value(value)
}

fn save_store_at(path: &Path, store: &CopilotByokStore) -> Result<CopilotByokStore, AppError> {
    let mut normalized = store.clone();
    normalize_store(&mut normalized)?;
    crate::config::write_json_file_private(path, &normalized)?;
    Ok(normalized)
}

fn remove_legacy_store_best_effort(path: &Path) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            log::warn!(
                "Could not inspect legacy portable Copilot BYOK store {}: {error}",
                path.display()
            );
            return;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        log::warn!(
            "Refusing to remove non-regular legacy Copilot BYOK store: {}",
            path.display()
        );
        return;
    }
    if let Err(error) = fs::remove_file(path) {
        log::warn!(
            "Could not remove legacy portable Copilot BYOK store {}: {error}",
            path.display()
        );
    }
}

fn remove_equivalent_legacy_store_best_effort(path: &Path, local: &CopilotByokStore) {
    if !path.exists() {
        return;
    }
    match load_store_at(path) {
        Ok(legacy) if &legacy == local => remove_legacy_store_best_effort(path),
        Ok(_) => log::warn!(
            "Preserving legacy portable Copilot BYOK store because it differs from device-local state: {}",
            path.display()
        ),
        Err(error) => log::warn!(
            "Preserving unreadable legacy portable Copilot BYOK store {}: {error}",
            path.display()
        ),
    }
}

fn load_store_with_legacy(
    local_path: &Path,
    legacy_path: Option<&Path>,
) -> Result<CopilotByokStore, AppError> {
    if local_path.exists() {
        let store = load_store_at(local_path)?;
        if let Some(legacy_path) = legacy_path {
            remove_equivalent_legacy_store_best_effort(legacy_path, &store);
        }
        return Ok(store);
    }

    let Some(legacy_path) = legacy_path.filter(|path| path.exists()) else {
        return Ok(CopilotByokStore::default());
    };
    let store = load_store_at(legacy_path)?;
    let local_store = save_store_at(local_path, &store)?;
    remove_equivalent_legacy_store_best_effort(legacy_path, &local_store);
    log::info!(
        "Migrated device-local Copilot BYOK state from {} to {}",
        legacy_path.display(),
        local_path.display()
    );
    Ok(local_store)
}

pub fn load_store() -> Result<CopilotByokStore, AppError> {
    let local_path = store_path();
    let legacy_path = legacy_portable_store_path();
    load_store_with_legacy(&local_path, legacy_path.as_deref())
}

pub fn save_store(store: &CopilotByokStore) -> Result<(), AppError> {
    let normalized = save_store_at(&store_path(), store)?;
    if let Some(legacy_path) = legacy_portable_store_path() {
        remove_equivalent_legacy_store_best_effort(&legacy_path, &normalized);
    }
    Ok(())
}

/// Persist only device-local VS Code targets. The provider catalog lives in
/// the main database so normal export and cloud synchronization include it.
pub fn save_device_store(store: &CopilotByokStore) -> Result<(), AppError> {
    let mut local = store.clone();
    local.groups.clear();
    save_store(&local)
}

pub(crate) fn apply_group_order(
    groups: &mut Vec<CopilotByokGroup>,
    group_ids: &[String],
) -> Result<(), AppError> {
    if group_ids.len() != groups.len() {
        return Err(AppError::InvalidInput(
            "Copilot BYOK provider order must include every provider exactly once".to_string(),
        ));
    }

    let requested: HashSet<&str> = group_ids.iter().map(String::as_str).collect();
    let existing: HashSet<&str> = groups.iter().map(|group| group.id.as_str()).collect();
    if requested.len() != group_ids.len() || requested != existing {
        return Err(AppError::InvalidInput(
            "Copilot BYOK provider order contains duplicate or unknown providers".to_string(),
        ));
    }

    let mut by_id: std::collections::HashMap<String, CopilotByokGroup> = std::mem::take(groups)
        .into_iter()
        .map(|group| (group.id.clone(), group))
        .collect();
    *groups = group_ids
        .iter()
        .map(|id| {
            by_id.remove(id).ok_or_else(|| {
                AppError::InvalidInput(format!("Unknown Copilot BYOK provider: {id}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_portable_store_to_private_device_path() {
        let temp = tempfile::tempdir().expect("temp directory");
        let local = temp.path().join("local").join(STORE_FILE);
        let legacy = temp.path().join("portable").join(STORE_FILE);
        let mut store = CopilotByokStore {
            targets_initialized: true,
            selected_target_ids: vec!["stable:default".to_string()],
            cli: CopilotCliConfig {
                official_override_active: true,
                ..CopilotCliConfig::default()
            },
            ..CopilotByokStore::default()
        };
        store.cli.managed_environment.last_written.insert(
            "COPILOT_PROVIDER_API_KEY".to_string(),
            Some("secret".to_string()),
        );
        save_store_at(&legacy, &store).expect("write legacy store");

        let migrated = load_store_with_legacy(&local, Some(&legacy)).expect("migrate store");

        assert_eq!(migrated, store);
        assert!(local.is_file());
        assert!(!legacy.exists());
        assert_eq!(
            load_store_at(&local)
                .expect("read migrated store")
                .cli
                .managed_environment
                .last_written["COPILOT_PROVIDER_API_KEY"]
                .as_deref(),
            Some("secret")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&local)
                    .expect("migrated metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn preserves_different_portable_store_when_device_store_exists() {
        let temp = tempfile::tempdir().expect("temp directory");
        let local = temp.path().join("local").join(STORE_FILE);
        let legacy = temp.path().join("portable").join(STORE_FILE);
        let mut local_store = CopilotByokStore {
            targets_initialized: true,
            selected_target_ids: vec!["stable:default".to_string()],
            ..CopilotByokStore::default()
        };
        let legacy_store = CopilotByokStore {
            targets_initialized: true,
            selected_target_ids: vec!["stable:insiders".to_string()],
            ..CopilotByokStore::default()
        };
        local_store = save_store_at(&local, &local_store).expect("write local store");
        save_store_at(&legacy, &legacy_store).expect("write legacy store");

        let loaded = load_store_with_legacy(&local, Some(&legacy)).expect("load local store");

        assert_eq!(loaded, local_store);
        assert!(legacy.is_file());
        assert_eq!(load_store_at(&legacy).expect("read legacy"), legacy_store);
    }

    #[test]
    fn migrates_legacy_single_path_store() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("chatLanguageModels.json");
        let store = parse_store_value(json!({
            "configPath": path,
            "models": []
        }))
        .expect("migrate store");

        assert_eq!(store.version, STORE_VERSION);
        assert!(store.targets_initialized);
        assert_eq!(store.custom_targets.len(), 1);
        assert_eq!(
            store.selected_target_ids,
            vec![store.custom_targets[0].id.clone()]
        );
    }

    #[test]
    fn migrates_v2_models_into_single_model_provider_groups() {
        let store = parse_store_value(json!({
            "version": 2,
            "targetsInitialized": true,
            "selectedTargetIds": [],
            "customTargets": [],
            "models": [{
                "id": "old-kimi",
                "modelId": "kimi-k3",
                "name": "Moonshot",
                "url": "https://api.example.com/v1/responses",
                "apiKey": "secret",
                "apiType": "responses"
            }]
        }))
        .expect("migrate v2 store");

        assert_eq!(store.version, STORE_VERSION);
        assert_eq!(store.groups.len(), 1);
        assert_eq!(store.groups[0].name, "Moonshot");
        assert_eq!(store.groups[0].api_type, "responses");
        assert_eq!(store.groups[0].models[0].model_id, "kimi-k3");
    }

    #[test]
    fn rejects_custom_non_language_model_file() {
        let result = CopilotByokCustomTarget::from_path("/tmp/settings.json", None);
        assert!(result.is_err());
    }

    #[test]
    fn custom_target_identity_normalizes_lexical_path_aliases() {
        let temp = tempfile::tempdir().expect("temp directory");
        let profile = temp.path().join("profile");
        fs::create_dir_all(&profile).expect("profile directory");
        let direct = profile.join("chatLanguageModels.json");
        let aliased = profile
            .join("nested")
            .join("..")
            .join("chatLanguageModels.json");

        let direct = CopilotByokCustomTarget::from_path(direct, None).unwrap();
        let aliased = CopilotByokCustomTarget::from_path(aliased, None).unwrap();
        assert_eq!(direct.id, aliased.id);
        assert_eq!(direct.language_models_path, aliased.language_models_path);
    }

    #[test]
    fn resource_identity_accepts_and_normalizes_non_model_paths() {
        let temp = tempfile::tempdir().expect("temp directory");
        let profile = temp.path().join("profile");
        fs::create_dir_all(profile.join("nested")).expect("profile directory");

        let direct = profile.join("prompts");
        let aliased = profile.join("nested").join("..").join("prompts");

        assert_eq!(path_identity_key(&direct), path_identity_key(&aliased));
    }

    #[test]
    fn selected_target_ids_are_deduplicated() {
        let store = parse_store_value(json!({
            "version": 2,
            "selectedTargetIds": ["stable:default", "stable:default"],
            "customTargets": [],
            "models": []
        }))
        .expect("parse store");
        assert!(store.targets_initialized);
        assert_eq!(store.selected_target_ids, vec!["stable:default"]);
    }

    #[test]
    fn explicit_empty_target_selection_is_preserved() {
        let store = parse_store_value(json!({
            "version": 3,
            "targetsInitialized": true,
            "selectedTargetIds": [],
            "customTargets": [],
            "groups": []
        }))
        .expect("parse store");
        assert!(store.targets_initialized);
        assert!(store.selected_target_ids.is_empty());
    }

    #[test]
    fn v9_store_without_serialized_groups_retains_cli_environment_for_v10_import() {
        let store = parse_store_value(json!({
            "version": 9,
            "targetsInitialized": false,
            "selectedTargetIds": [],
            "customTargets": [],
            "cli": {
                "enabled": false,
                "officialOverrideActive": true,
                "managedEnvironment": {
                    "original": {
                        "COPILOT_PROVIDER_BASE_URL": "https://api.example.com/v1",
                        "COPILOT_MODEL": "model-1"
                    },
                    "lastWritten": {}
                }
            },
            "cliCatalogInitialized": true,
            "cliSingleModelCatalogInitialized": true
        }))
        .expect("parse v9 store without groups field");

        assert_eq!(store.version, STORE_VERSION);
        assert!(store.cli.official_override_active);
        assert_eq!(
            store.cli.managed_environment.original["COPILOT_MODEL"].as_deref(),
            Some("model-1")
        );
        assert!(!store.cli_environment_import_initialized);
    }

    #[test]
    fn default_store_does_not_manage_any_target() {
        let store = CopilotByokStore::default();
        assert!(!store.targets_initialized);
        assert!(store.selected_target_ids.is_empty());
    }

    #[test]
    fn rejects_future_store_versions() {
        let result = parse_store_value(json!({
            "version": STORE_VERSION + 1,
            "targetsInitialized": true,
            "selectedTargetIds": [],
            "customTargets": [],
            "groups": []
        }));
        assert!(result.is_err());
    }

    #[test]
    fn migrates_v4_flattened_extension_fields_without_losing_them() {
        let store = parse_store_value(json!({
            "version": 4,
            "groups": [{
                "id": "future",
                "name": "Future",
                "url": "https://api.example.com/v1/responses",
                "apiType": "responses",
                "futureProviderOption": {"enabled": true},
                "models": [{
                    "id": "internal-model",
                    "modelId": "future-model",
                    "name": "Future Model",
                    "futureModelOption": [1, 2, 3]
                }]
            }]
        }))
        .expect("migrate v4 extras");

        assert_eq!(store.version, STORE_VERSION);
        assert_eq!(
            store.groups[0].extra["futureProviderOption"]["enabled"],
            true
        );
        assert_eq!(
            store.groups[0].models[0].extra["futureModelOption"],
            json!([1, 2, 3])
        );
        let rendered = store.groups[0].to_language_model_group();
        assert_eq!(rendered["futureProviderOption"]["enabled"], true);
        assert_eq!(rendered["models"][0]["futureModelOption"], json!([1, 2, 3]));
    }

    #[test]
    fn provider_order_requires_and_reorders_the_complete_id_set() {
        let mut groups = vec![
            CopilotByokGroup {
                id: "first".to_string(),
                name: "First".to_string(),
                url: "https://first.example.com/v1".to_string(),
                api_key: "one".to_string(),
                api_type: "chat-completions".to_string(),
                website_url: None,
                notes: None,
                icon: None,
                icon_color: None,
                category: None,
                usage_script: None,
                enabled: true,
                request_headers: BTreeMap::new(),
                models: Vec::new(),
                extra: BTreeMap::new(),
            },
            CopilotByokGroup {
                id: "second".to_string(),
                name: "Second".to_string(),
                url: "https://second.example.com/v1".to_string(),
                api_key: "two".to_string(),
                api_type: "chat-completions".to_string(),
                website_url: None,
                notes: None,
                icon: None,
                icon_color: None,
                category: None,
                usage_script: None,
                enabled: true,
                request_headers: BTreeMap::new(),
                models: Vec::new(),
                extra: BTreeMap::new(),
            },
        ];

        apply_group_order(&mut groups, &["second".to_string(), "first".to_string()])
            .expect("reorder providers");
        assert_eq!(groups[0].id, "second");
        assert!(
            apply_group_order(&mut groups, &["second".to_string(), "second".to_string()]).is_err()
        );
    }
}
