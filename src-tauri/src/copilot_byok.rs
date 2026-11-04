mod model;
mod store;
mod sync;
mod vscode;

pub use model::CopilotByokGroup;
pub use sync::CopilotByokSyncResult;
pub use vscode::{VsCodeEdition, VsCodeProfileTarget};

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use model::is_managed_group;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use store::{CopilotByokCustomTarget, CopilotByokStore};

static OPERATION_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
// A dedicated namespace keeps the portable catalog in the existing provider DB.
const CATALOG_APP_TYPE: &str = "copilot-byok-catalog";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokTargetState {
    pub id: String,
    pub source: String,
    pub edition: Option<VsCodeEdition>,
    pub edition_name: Option<String>,
    pub profile_id: Option<String>,
    pub profile_name: String,
    pub is_default: bool,
    pub language_models_path: String,
    pub config_exists: bool,
    pub backup_exists: bool,
    pub selected: bool,
    pub managed_group_count: usize,
    pub read_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokState {
    pub groups: Vec<CopilotByokGroup>,
    pub targets: Vec<CopilotByokTargetState>,
    pub selected_target_ids: Vec<String>,
    pub managed_model_count: usize,
}

fn load_catalog(db: &Database) -> Result<Vec<CopilotByokGroup>, AppError> {
    load_catalog_from(db, CATALOG_APP_TYPE)
}

fn load_runtime_store(db: &Database) -> Result<CopilotByokStore, AppError> {
    let mut local = store::load_store()?;
    local.groups = load_catalog(db)?;
    store::normalize_store(&mut local)?;
    Ok(local)
}

pub(crate) fn primary_profile_config_dir() -> Result<PathBuf, AppError> {
    let store = store::load_store()?;
    let detected = vscode::discover_vscode_targets()?;
    let ids = sync::effective_selected_target_ids(&store, &detected);
    sync::resolve_target_paths(&store, &ids)?
        .into_iter()
        .find_map(|(_, path)| path.parent().map(Path::to_path_buf))
        .ok_or_else(|| AppError::Config("No VS Code Copilot sync target is selected".into()))
}

pub(crate) fn sync_if_configured(db: &Database) -> Result<(), AppError> {
    let _guard = operation_guard()?;
    sync_if_selected(&load_runtime_store(db)?)
}

fn group_to_provider(group: &CopilotByokGroup, sort_index: usize) -> Result<Provider, AppError> {
    let mut settings_config =
        serde_json::to_value(group).map_err(|error| AppError::JsonSerialize { source: error })?;
    if let Some(settings) = settings_config.as_object_mut() {
        settings.remove("usageScript");
    }
    Ok(Provider {
        id: group.id.clone(),
        name: group.name.clone(),
        settings_config,
        website_url: group.website_url.clone(),
        category: Some("custom".to_string()),
        created_at: None,
        sort_index: Some(sort_index),
        notes: group.notes.clone(),
        meta: None,
        icon: group.icon.clone(),
        icon_color: group.icon_color.clone(),
        in_failover_queue: false,
    })
}

fn provider_to_group(provider: &Provider) -> Result<CopilotByokGroup, AppError> {
    let mut group: CopilotByokGroup = serde_json::from_value(provider.settings_config.clone())
        .map_err(|error| {
            AppError::Config(format!(
                "Failed to parse VS Code Copilot provider '{}': {error}",
                provider.id
            ))
        })?;
    group.id = provider.id.clone();
    group.name = provider.name.clone();
    group.website_url = provider.website_url.clone();
    group.notes = provider.notes.clone();
    group.icon = provider.icon.clone();
    group.icon_color = provider.icon_color.clone();
    group.category =
        (provider.category.as_deref() == Some("official")).then(|| "official".to_string());
    group.normalize();
    group.validate()?;
    Ok(group)
}

fn load_catalog_from(db: &Database, app_type: &str) -> Result<Vec<CopilotByokGroup>, AppError> {
    let mut groups = db
        .get_all_providers(app_type)?
        .values()
        .map(provider_to_group)
        .collect::<Result<Vec<_>, _>>()?;
    store::normalize_groups(&mut groups)?;
    Ok(groups)
}

fn persist_catalog_to(
    db: &Database,
    app_type: &str,
    groups: &[CopilotByokGroup],
) -> Result<(), AppError> {
    let mut normalized = groups.to_vec();
    store::normalize_groups(&mut normalized)?;
    let providers = normalized
        .iter()
        .enumerate()
        .map(|(sort_index, group)| group_to_provider(group, sort_index))
        .collect::<Result<Vec<_>, _>>()?;
    db.replace_provider_catalog(app_type, &providers)
}

fn persist_catalog(db: &Database, groups: &[CopilotByokGroup]) -> Result<(), AppError> {
    persist_catalog_to(db, CATALOG_APP_TYPE, groups)
}

fn operation_guard() -> Result<MutexGuard<'static, ()>, AppError> {
    OPERATION_LOCK
        .lock()
        .map_err(|error| AppError::Lock(error.to_string()))
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.cc-switch.bak")
}

fn inspect_path(path: &Path) -> (usize, Option<String>) {
    match sync::read_language_model_groups(path) {
        Ok(groups) => (
            groups
                .iter()
                .filter(|group| is_managed_group(group))
                .count(),
            None,
        ),
        Err(error) => (0, Some(error.to_string())),
    }
}

fn detected_target_state(
    target: VsCodeProfileTarget,
    selected_ids: &HashSet<String>,
) -> CopilotByokTargetState {
    let path = target.path();
    let (managed_group_count, read_error) = inspect_path(&path);
    CopilotByokTargetState {
        selected: selected_ids.contains(&target.id),
        id: target.id,
        source: "detected".to_string(),
        edition: Some(target.edition),
        edition_name: Some(target.edition_name),
        profile_id: target.profile_id,
        profile_name: target.profile_name,
        is_default: target.is_default,
        language_models_path: target.resources.language_models_path,
        config_exists: target.config_exists,
        backup_exists: target.backup_exists,
        managed_group_count,
        read_error,
    }
}

fn custom_target_state(
    target: &CopilotByokCustomTarget,
    selected_ids: &HashSet<String>,
) -> CopilotByokTargetState {
    let path = PathBuf::from(&target.language_models_path);
    let (managed_group_count, read_error) = inspect_path(&path);
    CopilotByokTargetState {
        id: target.id.clone(),
        source: "custom".to_string(),
        edition: None,
        edition_name: None,
        profile_id: None,
        profile_name: target.name.clone(),
        is_default: false,
        language_models_path: target.language_models_path.clone(),
        config_exists: path.exists(),
        backup_exists: backup_path(&path).exists(),
        selected: selected_ids.contains(&target.id),
        managed_group_count,
        read_error,
    }
}

fn detected_target_aliases(targets: &[VsCodeProfileTarget]) -> HashMap<String, String> {
    let mut representatives: HashMap<String, String> = HashMap::new();
    let mut aliases = HashMap::new();
    for target in targets {
        let identity = store::path_identity_key(&target.path());
        let representative = representatives
            .entry(identity)
            .or_insert_with(|| target.id.clone())
            .clone();
        aliases.insert(target.id.clone(), representative);
    }
    aliases
}

fn canonicalize_detected_target_ids(
    target_ids: &[String],
    aliases: &HashMap<String, String>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    target_ids
        .iter()
        .map(|id| aliases.get(id).cloned().unwrap_or_else(|| id.clone()))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn build_state(_db: &Database, store: CopilotByokStore) -> Result<CopilotByokState, AppError> {
    let detected = vscode::discover_vscode_targets()?;
    let aliases = detected_target_aliases(&detected);
    let available_ids: HashSet<String> = detected
        .iter()
        .filter(|target| aliases.get(&target.id) == Some(&target.id))
        .map(|target| target.id.clone())
        .chain(store.custom_targets.iter().map(|target| target.id.clone()))
        .collect();
    let effective_target_ids = sync::effective_selected_target_ids(&store, &detected);
    let mut selected_target_ids = canonicalize_detected_target_ids(&effective_target_ids, &aliases);
    selected_target_ids.retain(|id| available_ids.contains(id));
    let selected_ids: HashSet<String> = selected_target_ids.iter().cloned().collect();

    let mut targets: Vec<CopilotByokTargetState> = detected
        .into_iter()
        .filter(|target| aliases.get(&target.id) == Some(&target.id))
        .map(|target| detected_target_state(target, &selected_ids))
        .collect();
    targets.extend(
        store
            .custom_targets
            .iter()
            .map(|target| custom_target_state(target, &selected_ids)),
    );
    targets.sort_by(|left, right| {
        right
            .selected
            .cmp(&left.selected)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.profile_name.cmp(&right.profile_name))
    });

    Ok(CopilotByokState {
        managed_model_count: store
            .groups
            .iter()
            .map(CopilotByokGroup::enabled_model_count)
            .sum(),
        groups: store.groups,
        targets,
        selected_target_ids,
    })
}

fn sync_if_selected(store: &CopilotByokStore) -> Result<(), AppError> {
    let detected = vscode::discover_vscode_targets()?;
    let available_ids: HashSet<String> = detected
        .iter()
        .map(|target| target.id.clone())
        .chain(store.custom_targets.iter().map(|target| target.id.clone()))
        .collect();
    let selected_target_ids: Vec<String> = sync::effective_selected_target_ids(store, &detected)
        .into_iter()
        .filter(|id| available_ids.contains(id))
        .collect();
    if selected_target_ids.is_empty() {
        return Ok(());
    }

    let mut available_store = store.clone();
    available_store.targets_initialized = true;
    available_store.selected_target_ids = selected_target_ids;
    sync::sync_store(&available_store)?;
    Ok(())
}

fn commit_and_build(
    db: &Database,
    current: &CopilotByokStore,
    updated: &CopilotByokStore,
    overrides: sync::TransactionOverrides,
) -> Result<CopilotByokState, AppError> {
    sync::commit_store_update(current, updated, overrides, true)?;
    build_state(db, load_runtime_store(db)?)
}

fn commit_target_change(
    db: &Database,
    current: &CopilotByokStore,
    updated: &CopilotByokStore,
) -> Result<CopilotByokState, AppError> {
    let overrides = sync::TransactionOverrides::default();
    // Resolve fallible state reads before committing, so returning an error
    // cannot leave a successfully changed target selection behind.
    let mut next = build_state(db, updated.clone())?;
    sync::commit_store_update(current, updated, overrides, true)?;
    for target in &mut next.targets {
        let path = Path::new(&target.language_models_path);
        target.config_exists = path.exists();
        target.backup_exists = backup_path(path).exists();
        (target.managed_group_count, target.read_error) = inspect_path(path);
    }
    Ok(next)
}

pub fn get_state(db: &Database) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    build_state(db, load_runtime_store(db)?)
}

pub fn set_targets(db: &Database, target_ids: Vec<String>) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let detected = vscode::discover_vscode_targets()?;
    let valid_ids: HashSet<String> = detected
        .iter()
        .map(|target| target.id.clone())
        .chain(
            current
                .custom_targets
                .iter()
                .map(|target| target.id.clone()),
        )
        .collect();
    if let Some(invalid) = target_ids.iter().find(|id| !valid_ids.contains(*id)) {
        return Err(AppError::InvalidInput(format!(
            "Unknown or unavailable Copilot BYOK target: {invalid}"
        )));
    }

    let mut updated = current.clone();
    updated.targets_initialized = true;
    let aliases = detected_target_aliases(&detected);
    updated.selected_target_ids = canonicalize_detected_target_ids(&target_ids, &aliases);
    commit_target_change(db, &current, &updated)
}

pub fn add_custom_target(
    db: &Database,
    path: String,
    name: Option<String>,
) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let custom = CopilotByokCustomTarget::from_path(path, name)?;
    let custom_key = store::path_identity_key(Path::new(&custom.language_models_path));
    if vscode::discover_vscode_targets()?
        .iter()
        .any(|target| store::path_identity_key(&target.path()) == custom_key)
    {
        return Err(AppError::InvalidInput(
            "This VS Code profile is already available as a detected sync target".to_string(),
        ));
    }
    let mut updated = current.clone();
    updated.targets_initialized = true;
    if let Some(existing) = updated
        .custom_targets
        .iter_mut()
        .find(|target| target.id == custom.id)
    {
        *existing = custom.clone();
    } else {
        updated.custom_targets.push(custom.clone());
    }
    if !updated.selected_target_ids.contains(&custom.id) {
        updated.selected_target_ids.push(custom.id);
    }
    commit_target_change(db, &current, &updated)
}

pub fn remove_custom_target(db: &Database, target_id: &str) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let mut updated = current.clone();
    updated
        .custom_targets
        .retain(|target| target.id != target_id);
    updated.selected_target_ids.retain(|id| id != target_id);
    commit_target_change(db, &current, &updated)
}

pub fn upsert_group(
    db: &Database,
    mut group: CopilotByokGroup,
) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    group.normalize();
    group.validate()?;
    let current = load_runtime_store(db)?;
    let previous_groups = current.groups.clone();
    let mut updated = current.clone();
    if let Some(existing) = updated.groups.iter_mut().find(|item| item.id == group.id) {
        *existing = group;
    } else {
        updated.groups.push(group);
    }
    persist_catalog(db, &updated.groups)?;
    match commit_and_build(
        db,
        &current,
        &updated,
        sync::TransactionOverrides::default(),
    ) {
        Ok(state) => Ok(state),
        Err(error) => {
            persist_catalog(db, &previous_groups).map_err(|rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back VS Code Copilot catalog: {rollback_error}"
                ))
            })?;
            Err(error)
        }
    }
}

pub fn delete_group(db: &Database, group_id: &str) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let previous_groups = current.groups.clone();
    let mut updated = current.clone();
    updated.groups.retain(|group| group.id != group_id);
    persist_catalog(db, &updated.groups)?;
    match commit_and_build(
        db,
        &current,
        &updated,
        sync::TransactionOverrides::default(),
    ) {
        Ok(state) => Ok(state),
        Err(error) => {
            persist_catalog(db, &previous_groups).map_err(|rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back VS Code Copilot catalog: {rollback_error}"
                ))
            })?;
            Err(error)
        }
    }
}

pub fn reorder_groups(db: &Database, group_ids: Vec<String>) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let previous_groups = current.groups.clone();
    let mut updated = current.clone();
    store::apply_group_order(&mut updated.groups, &group_ids)?;
    persist_catalog(db, &updated.groups)?;
    match commit_and_build(
        db,
        &current,
        &updated,
        sync::TransactionOverrides::default(),
    ) {
        Ok(state) => Ok(state),
        Err(error) => {
            persist_catalog(db, &previous_groups).map_err(|rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back VS Code Copilot catalog: {rollback_error}"
                ))
            })?;
            Err(error)
        }
    }
}

pub fn sync(db: &Database) -> Result<CopilotByokSyncResult, AppError> {
    let _guard = operation_guard()?;
    let store = load_runtime_store(db)?;
    sync::sync_store(&store)
}

pub fn restore_backup(db: &Database, target_id: &str) -> Result<bool, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let detected = vscode::discover_vscode_targets()?;
    let (_, target_path) = sync::resolve_target_paths(&current, &[target_id.to_string()])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown or unavailable Copilot BYOK target: {target_id}"
            ))
        })?;
    let target_identity = store::path_identity_key(&target_path);
    let alias_ids: HashSet<String> = detected
        .iter()
        .filter(|target| store::path_identity_key(&target.path()) == target_identity)
        .map(|target| target.id.clone())
        .chain(
            current
                .custom_targets
                .iter()
                .filter(|target| {
                    store::path_identity_key(Path::new(&target.language_models_path))
                        == target_identity
                })
                .map(|target| target.id.clone()),
        )
        .collect();
    let selected = sync::effective_selected_target_ids(&current, &detected);
    let was_selected = selected.iter().any(|id| alias_ids.contains(id));
    let mut updated = current.clone();
    updated.targets_initialized = true;
    updated
        .selected_target_ids
        .retain(|id| !alias_ids.contains(id));
    let overrides = sync::TransactionOverrides {
        restore_targets: [target_id.to_string()].into_iter().collect(),
        ..sync::TransactionOverrides::default()
    };
    let result = sync::commit_store_update(&current, &updated, overrides, true)?;
    Ok(was_selected || result.changed_target_count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestHome {
        _directory: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", directory.path());
            Self {
                _directory: directory,
                previous,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn group() -> CopilotByokGroup {
        serde_json::from_value(json!({
            "id": "provider", "name": "Provider", "url": "https://example.com/v1",
            "apiKey": "test-secret", "apiType": "chat-completions", "enabled": true,
            "models": [{ "id": "model", "modelId": "custom-model", "name": "Model", "enabled": true }]
        })).unwrap()
    }

    fn isolated_vscode_config_dir() -> PathBuf {
        let home = crate::config::get_home_dir();
        if cfg!(target_os = "windows") {
            home.join("AppData").join("Roaming")
        } else if cfg!(target_os = "macos") {
            home.join("Library").join("Application Support")
        } else {
            home.join(".config")
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_home_limits_discovery_to_isolated_profiles() {
        let _home = TestHome::new();
        assert!(vscode::discover_vscode_targets().unwrap().is_empty());
        let config = isolated_vscode_config_dir();
        for edition in ["Code", "Code - Insiders"] {
            let user = config.join(edition).join("User");
            std::fs::create_dir_all(user.join("profiles/work")).unwrap();
            crate::config::write_json_file_private(
                &user.join("globalStorage/storage.json"),
                &json!({ "userDataProfiles": [{ "location": "work", "name": "Work" }] }),
            )
            .unwrap();
        }

        let targets = vscode::discover_vscode_targets().unwrap();
        assert_eq!(targets.len(), 4);
        for target in targets {
            assert!(target.path().starts_with(&config));
        }
    }

    #[test]
    #[serial_test::serial]
    fn changing_targets_only_cleans_the_isolated_default_profile() {
        let _home = TestHome::new();
        let home = crate::config::get_home_dir();
        let default_path = isolated_vscode_config_dir().join("Code/User/chatLanguageModels.json");
        let original = json!([
            { "name": "Unmanaged", "vendor": "Other", "models": [] },
            group().to_language_model_group()
        ]);
        crate::config::write_json_file_private(&default_path, &original).unwrap();

        // Check the discovery boundary BEFORE invoking any mutating operation.
        // This regression must fail safely even if OS discovery is reintroduced.
        let targets = vscode::discover_vscode_targets().unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path(), default_path);
        assert!(targets
            .iter()
            .all(|target| target.path().starts_with(&home)));

        let db = Database::memory().unwrap();
        let custom = home.join("custom/chatLanguageModels.json");
        crate::config::write_json_file_private(&custom, &json!([])).unwrap();
        add_custom_target(&db, custom.to_string_lossy().into_owned(), None).unwrap();
        assert_eq!(
            sync::read_language_model_groups(&default_path).unwrap(),
            vec![original[0].clone()]
        );
        let state = get_state(&db).unwrap();
        assert_eq!(state.targets.len(), 2);
        // Custom targets are canonicalized by the store. Compare real paths on
        // both sides: macOS temp roots can be symlink aliases, and Windows
        // canonicalization can change the path prefix/casing. Both fixture files
        // exist, so canonicalize must succeed rather than falling back lexically.
        let canonical_home = std::fs::canonicalize(&home).unwrap();
        for target in &state.targets {
            let canonical_target = std::fs::canonicalize(&target.language_models_path).unwrap();
            assert!(
                canonical_target.starts_with(&canonical_home),
                "target {} resolves outside test home {}: {}",
                target.language_models_path,
                canonical_home.display(),
                canonical_target.display()
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn catalog_crud_sync_and_restore_form_a_complete_loop() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        let target = crate::config::get_home_dir().join("profile/chatLanguageModels.json");
        let original = json!([{ "name": "Unmanaged", "vendor": "Other", "models": [] }]);
        crate::config::write_json_file_private(&target, &original).unwrap();
        add_custom_target(&db, target.to_string_lossy().into_owned(), None).unwrap();
        let state = upsert_group(&db, group()).unwrap();
        assert_eq!(state.managed_model_count, 1);
        let written = sync::read_language_model_groups(&target).unwrap();
        assert_eq!(written[0], original[0]);
        assert_eq!(written.len(), 2);
        let device: serde_json::Value =
            serde_json::from_slice(&std::fs::read(store::store_path()).unwrap()).unwrap();
        assert!(device.get("groups").is_none());
        assert!(device.get("cli").is_none());
        assert!(db
            .get_all_providers("copilot-cli-catalog")
            .unwrap()
            .is_empty());
        let mut edited = group();
        edited.models[0].name = "Updated".into();
        upsert_group(&db, edited).unwrap();
        assert_eq!(
            sync::read_language_model_groups(&target).unwrap()[1]["models"][0]["name"],
            "Updated"
        );
        delete_group(&db, "provider").unwrap();
        assert_eq!(
            sync::read_language_model_groups(&target).unwrap(),
            vec![original[0].clone()]
        );
        upsert_group(&db, group()).unwrap();
        assert!(restore_backup(&db, &state.selected_target_ids[0]).unwrap());
        assert!(get_state(&db).unwrap().selected_target_ids.is_empty());
        assert_eq!(
            sync::read_language_model_groups(&target).unwrap(),
            vec![original[0].clone()]
        );
    }

    #[test]
    #[serial_test::serial]
    fn malformed_target_rolls_back_catalog_and_every_file() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        let target = crate::config::get_home_dir().join("profile/chatLanguageModels.json");
        add_custom_target(&db, target.to_string_lossy().into_owned(), None).unwrap();
        upsert_group(&db, group()).unwrap();
        std::fs::write(&target, b"malformed config").unwrap();
        let device_before = std::fs::read(store::store_path()).unwrap();
        let mut edited = group();
        edited.name = "Should not persist".into();
        assert!(upsert_group(&db, edited).is_err());
        assert_eq!(load_catalog(&db).unwrap()[0].name, "Provider");
        assert_eq!(std::fs::read(&target).unwrap(), b"malformed config");
        assert_eq!(std::fs::read(store::store_path()).unwrap(), device_before);
    }

    #[test]
    #[serial_test::serial]
    fn selecting_an_unreadable_target_does_not_change_selection() {
        let _home = TestHome::new();
        let db = Database::memory().unwrap();
        let target = crate::config::get_home_dir().join("profile/chatLanguageModels.json");
        crate::config::write_json_file_private(&target, &json!([])).unwrap();
        let state = add_custom_target(&db, target.to_string_lossy().into_owned(), None).unwrap();
        set_targets(&db, Vec::new()).unwrap();
        upsert_group(&db, group()).unwrap();
        std::fs::write(&target, b"invalid json").unwrap();
        assert!(set_targets(&db, state.selected_target_ids).is_err());
        assert!(get_state(&db).unwrap().selected_target_ids.is_empty());
        assert_eq!(std::fs::read(&target).unwrap(), b"invalid json");
    }
}
