use super::model::CopilotByokGroup;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// Keep new upstream device state separate from the older all-in-one preview.
const STORE_FILE: &str = "vscode-copilot-byok.json";
const STORE_VERSION: u32 = 1;
const MAX_TARGETS: usize = 64;
const MAX_GROUPS: usize = 256;
const MAX_MODELS: usize = 256;
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;

fn default_store_version() -> u32 {
    STORE_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokCustomTarget {
    pub id: String,
    pub name: String,
    pub language_models_path: String,
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
                // An empty name represents the UI's localized default label.
                // Existing stored names remain user data and are not renamed.
                .unwrap_or_default(),
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
}

impl Default for CopilotByokStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            targets_initialized: false,
            selected_target_ids: Vec::new(),
            custom_targets: Vec::new(),
            groups: Vec::new(),
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

pub(crate) fn device_store_contents(store: &CopilotByokStore) -> Result<Vec<u8>, AppError> {
    let mut local = store.clone();
    local.groups.clear();
    normalize_store(&mut local)?;
    crate::config::serialize_json_file_contents(&local)
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

pub(crate) fn parse_store_value(value: Value) -> Result<CopilotByokStore, AppError> {
    let mut store: CopilotByokStore = serde_json::from_value(value).map_err(|error| {
        AppError::Config(format!(
            "Failed to parse Copilot BYOK device state: {error}"
        ))
    })?;
    normalize_store(&mut store)?;
    Ok(store)
}

pub fn store_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join(STORE_FILE)
}

pub fn load_store() -> Result<CopilotByokStore, AppError> {
    let path = store_path();
    if !path.exists() {
        return Ok(CopilotByokStore::default());
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| AppError::io(&path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_STORE_BYTES
    {
        return Err(AppError::InvalidInput(
            "Copilot BYOK device state must be a regular file of at most 8 MiB".into(),
        ));
    }
    let text = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    let value = serde_json::from_str(&text).map_err(|error| AppError::json(&path, error))?;
    parse_store_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn rejects_custom_non_language_model_file() {
        let result = CopilotByokCustomTarget::from_path("/tmp/settings.json", None);
        assert!(result.is_err());
    }

    #[test]
    fn unnamed_custom_target_keeps_the_default_label_out_of_stored_data() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("chatLanguageModels.json");
        for name in [None, Some("   ".to_string())] {
            let target = CopilotByokCustomTarget::from_path(&path, name).unwrap();
            assert_eq!(target.name, "");
            let serialized = serde_json::to_value(&target).unwrap();
            assert_eq!(serialized["name"], "");
        }
    }

    #[test]
    fn explicitly_named_custom_target_is_preserved_even_if_it_matches_a_legacy_default() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("chatLanguageModels.json");
        let target =
            CopilotByokCustomTarget::from_path(&path, Some("Custom VS Code profile".to_string()))
                .unwrap();
        assert_eq!(target.name, "Custom VS Code profile");
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
    fn selected_target_ids_are_deduplicated() {
        let store = parse_store_value(json!({
            "version": STORE_VERSION,
            "targetsInitialized": true,
            "selectedTargetIds": ["stable:default", "stable:default"],
            "customTargets": [],
            "groups": []
        }))
        .expect("parse store");
        assert!(store.targets_initialized);
        assert_eq!(store.selected_target_ids, vec!["stable:default"]);
    }

    #[test]
    fn explicit_empty_target_selection_is_preserved() {
        let store = parse_store_value(json!({
            "version": STORE_VERSION,
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
