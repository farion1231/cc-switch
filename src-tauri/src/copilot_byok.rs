mod cli;
mod import;
mod model;
mod store;
mod sync;
mod vscode;

pub use cli::CopilotCliState;
pub use import::CopilotByokImportResult;
pub use model::CopilotByokGroup;
pub use sync::CopilotByokSyncResult;
pub use vscode::{VsCodeEdition, VsCodeProfileTarget};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta, UsageScript};
use model::is_managed_group;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use store::{CopilotByokCustomTarget, CopilotByokStore};

static OPERATION_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
// Usage statistics intentionally use `copilot-byok` with one normalized
// provider row. Keep the portable BYOK catalog in its own provider namespace
// so statistics and configuration can never overwrite or parse each other.
const CATALOG_APP_TYPE: &str = "copilot-byok-catalog";
const CLI_CATALOG_APP_TYPE: &str = "copilot-cli-catalog";
const LEGACY_CATALOG_APP_TYPE: &str = "copilot-byok";
pub const COPILOT_CLI_OFFICIAL_PROVIDER_ID: &str = "copilot-cli-official";
const COPILOT_CLI_OFFICIAL_PROVIDER_NAME: &str = "GitHub Copilot Official";
const COPILOT_CLI_OFFICIAL_WEBSITE: &str = "https://github.com/features/copilot";
const COPILOT_CLI_MIGRATED_PROVIDER_ID: &str = "copilot-cli-official-custom";

/// Resolve GitHub Copilot CLI's home directory. `COPILOT_HOME` is honored so
/// portable/test installations use the same location as the CLI itself.
pub(crate) fn copilot_cli_home() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os("COPILOT_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Ok(path);
    }
    Ok(crate::config::get_home_dir().join(".copilot"))
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
        meta: group.usage_script.clone().map(|usage_script| ProviderMeta {
            usage_script: Some(usage_script),
            ..Default::default()
        }),
        icon: group.icon.clone(),
        icon_color: group.icon_color.clone(),
        in_failover_queue: false,
    })
}

fn copilot_cli_official_provider() -> Provider {
    Provider {
        id: COPILOT_CLI_OFFICIAL_PROVIDER_ID.to_string(),
        name: COPILOT_CLI_OFFICIAL_PROVIDER_NAME.to_string(),
        // Like Codex's OpenAI Official seed, this intentionally contains no
        // third-party endpoint or credential. Selecting it is handled by the
        // CLI environment adapter and returns routing to GitHub Copilot.
        settings_config: serde_json::json!({ "official": true }),
        website_url: Some(COPILOT_CLI_OFFICIAL_WEBSITE.to_string()),
        category: Some("official".to_string()),
        created_at: None,
        sort_index: Some(0),
        notes: None,
        meta: None,
        icon: Some("githubcopilot".to_string()),
        icon_color: None,
        in_failover_queue: false,
    }
}

fn copilot_cli_official_group(provider: Option<&Provider>) -> CopilotByokGroup {
    CopilotByokGroup {
        id: COPILOT_CLI_OFFICIAL_PROVIDER_ID.to_string(),
        name: COPILOT_CLI_OFFICIAL_PROVIDER_NAME.to_string(),
        url: String::new(),
        api_key: String::new(),
        api_type: "chat-completions".to_string(),
        website_url: Some(COPILOT_CLI_OFFICIAL_WEBSITE.to_string()),
        notes: None,
        icon: Some("githubcopilot".to_string()),
        icon_color: None,
        category: Some("official".to_string()),
        usage_script: provider
            .and_then(|provider| provider.meta.as_ref())
            .and_then(|meta| meta.usage_script.clone()),
        enabled: true,
        request_headers: Default::default(),
        models: Vec::new(),
        extra: Default::default(),
    }
}

fn remap_cli_official_collision_selection(store: &mut CopilotByokStore, migrated_id: &str) -> bool {
    if store.cli.selected_group_id.as_deref() != Some(COPILOT_CLI_OFFICIAL_PROVIDER_ID) {
        return false;
    }
    store.cli.selected_group_id = Some(migrated_id.to_string());
    true
}

fn ensure_cli_official_provider(
    db: &Database,
    local_store: Option<&mut CopilotByokStore>,
) -> Result<(), AppError> {
    let providers = db.get_all_providers(CLI_CATALOG_APP_TYPE)?;
    let Some(mut collision) = providers.get(COPILOT_CLI_OFFICIAL_PROVIDER_ID).cloned() else {
        return db.save_provider(CLI_CATALOG_APP_TYPE, &copilot_cli_official_provider());
    };
    if collision.category.as_deref() == Some("official") {
        return Ok(());
    }

    let mut migrated_id = COPILOT_CLI_MIGRATED_PROVIDER_ID.to_string();
    let mut suffix = 2;
    while providers.contains_key(&migrated_id) {
        migrated_id = format!("{COPILOT_CLI_MIGRATED_PROVIDER_ID}-{suffix}");
        suffix += 1;
    }

    // Persist the device-local selection first. If the database replacement
    // fails, the next startup deterministically retries the same migration;
    // it can never mistake the newly seeded Official row for the old custom
    // selection after a partial cross-store update.
    if let Some(store) = local_store {
        if remap_cli_official_collision_selection(store, &migrated_id) {
            store::save_device_store(store)?;
        }
    }

    collision.id = migrated_id.clone();
    collision.category = Some("custom".to_string());
    if let Some(settings) = collision.settings_config.as_object_mut() {
        settings.insert("id".to_string(), serde_json::Value::String(migrated_id));
    }

    let mut custom_providers = providers
        .into_values()
        .filter(|provider| provider.id != COPILOT_CLI_OFFICIAL_PROVIDER_ID)
        .collect::<Vec<_>>();
    custom_providers.push(collision);
    custom_providers.sort_by(|left, right| {
        left.sort_index
            .unwrap_or(usize::MAX)
            .cmp(&right.sort_index.unwrap_or(usize::MAX))
            .then_with(|| left.id.cmp(&right.id))
    });
    for (index, provider) in custom_providers.iter_mut().enumerate() {
        provider.sort_index = Some(index + 1);
    }

    let mut replacement = Vec::with_capacity(custom_providers.len() + 1);
    replacement.push(copilot_cli_official_provider());
    replacement.extend(custom_providers);
    db.replace_provider_catalog(CLI_CATALOG_APP_TYPE, &replacement)
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
    if let Some(usage_script) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.clone())
    {
        group.usage_script = Some(usage_script);
    }
    group.normalize();
    group.validate()?;
    Ok(group)
}

fn migrate_legacy_database_catalog(db: &Database) -> Result<(), AppError> {
    let legacy = db.get_all_providers(LEGACY_CATALOG_APP_TYPE)?;
    let existing = db.get_all_providers(CATALOG_APP_TYPE)?;
    let mut next_sort = existing.len();

    for provider in legacy.values() {
        let Ok(group) = provider_to_group(provider) else {
            // The normalized statistics provider has a deliberately different
            // settings shape and remains in the live application namespace.
            continue;
        };
        if !existing.contains_key(&group.id) {
            db.save_provider(CATALOG_APP_TYPE, &group_to_provider(&group, next_sort)?)?;
            next_sort += 1;
        }
        db.delete_provider(LEGACY_CATALOG_APP_TYPE, &group.id)?;
    }
    Ok(())
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

fn load_catalog(db: &Database) -> Result<Vec<CopilotByokGroup>, AppError> {
    migrate_legacy_database_catalog(db)?;
    load_catalog_from(db, CATALOG_APP_TYPE)
}

fn load_cli_catalog(db: &Database) -> Result<Vec<CopilotByokGroup>, AppError> {
    ensure_cli_official_provider(db, None)?;
    let mut groups = db
        .get_all_providers(CLI_CATALOG_APP_TYPE)?
        .values()
        .filter(|provider| provider.id != COPILOT_CLI_OFFICIAL_PROVIDER_ID)
        .map(provider_to_group)
        .collect::<Result<Vec<_>, _>>()?;
    store::normalize_groups(&mut groups)?;
    Ok(groups)
}

fn build_cli_provider_catalog(
    db: &Database,
    custom_groups: &[CopilotByokGroup],
) -> Result<Vec<CopilotByokGroup>, AppError> {
    let providers = db.get_all_providers(CLI_CATALOG_APP_TYPE)?;
    let custom_by_id: HashMap<&str, &CopilotByokGroup> = custom_groups
        .iter()
        .map(|group| (group.id.as_str(), group))
        .collect();
    let mut groups = Vec::with_capacity(custom_groups.len() + 1);
    for provider in providers.values() {
        if provider.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID {
            groups.push(copilot_cli_official_group(Some(provider)));
        } else if let Some(group) = custom_by_id.get(provider.id.as_str()) {
            groups.push((*group).clone());
        }
    }
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

fn persist_cli_catalog(db: &Database, groups: &[CopilotByokGroup]) -> Result<(), AppError> {
    let mut normalized = groups.to_vec();
    if normalized.iter().any(|group| {
        group.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID
            || group.category.as_deref() == Some("official")
    }) {
        return Err(AppError::InvalidInput(
            "The built-in GitHub Copilot Official provider cannot be replaced".to_string(),
        ));
    }
    for group in &mut normalized {
        group.category = None;
    }
    store::normalize_groups(&mut normalized)?;
    let existing = db.get_all_providers(CLI_CATALOG_APP_TYPE)?;
    let official_position = existing
        .values()
        .position(|provider| provider.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID)
        .unwrap_or(0)
        .min(normalized.len());
    let mut official = copilot_cli_official_provider();
    if let Some(existing_official) = existing.get(COPILOT_CLI_OFFICIAL_PROVIDER_ID) {
        official.meta = existing_official.meta.clone();
    }
    let mut providers = Vec::with_capacity(normalized.len() + 1);
    providers.extend(
        normalized
            .iter()
            .enumerate()
            .map(|(sort_index, group)| group_to_provider(group, sort_index))
            .collect::<Result<Vec<_>, _>>()?,
    );
    providers.insert(official_position, official);
    for (sort_index, provider) in providers.iter_mut().enumerate() {
        provider.sort_index = Some(sort_index);
    }
    db.replace_provider_catalog(CLI_CATALOG_APP_TYPE, &providers)
}

fn persist_ordered_cli_catalog(db: &Database, groups: &[CopilotByokGroup]) -> Result<(), AppError> {
    let official_count = groups
        .iter()
        .filter(|group| group.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID)
        .count();
    if official_count != 1
        || groups.iter().any(|group| {
            group.category.as_deref() == Some("official")
                && group.id != COPILOT_CLI_OFFICIAL_PROVIDER_ID
        })
    {
        return Err(AppError::InvalidInput(
            "Copilot CLI provider order must include the built-in Official provider exactly once"
                .to_string(),
        ));
    }

    let mut custom_groups = groups
        .iter()
        .filter(|group| group.id != COPILOT_CLI_OFFICIAL_PROVIDER_ID)
        .cloned()
        .collect::<Vec<_>>();
    for group in &mut custom_groups {
        group.category = None;
    }
    store::normalize_groups(&mut custom_groups)?;
    let custom_by_id: HashMap<&str, &CopilotByokGroup> = custom_groups
        .iter()
        .map(|group| (group.id.as_str(), group))
        .collect();

    let existing = db.get_all_providers(CLI_CATALOG_APP_TYPE)?;
    let mut official = copilot_cli_official_provider();
    if let Some(existing_official) = existing.get(COPILOT_CLI_OFFICIAL_PROVIDER_ID) {
        official.meta = existing_official.meta.clone();
    }
    let providers = groups
        .iter()
        .enumerate()
        .map(|(sort_index, group)| {
            if group.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID {
                let mut provider = official.clone();
                provider.sort_index = Some(sort_index);
                Ok(provider)
            } else {
                let normalized = custom_by_id.get(group.id.as_str()).ok_or_else(|| {
                    AppError::InvalidInput(format!(
                        "Unknown Copilot CLI provider in order: {}",
                        group.id
                    ))
                })?;
                group_to_provider(normalized, sort_index)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    db.replace_provider_catalog(CLI_CATALOG_APP_TYPE, &providers)
}

fn collapse_cli_catalog_to_default_models(
    groups: &mut [CopilotByokGroup],
    selected_group_id: Option<&str>,
    selected_model_id: Option<&str>,
) -> bool {
    let mut changed = false;
    for group in groups {
        let preferred_model_id = (selected_group_id == Some(group.id.as_str()))
            .then_some(selected_model_id)
            .flatten();
        let preferred_index = preferred_model_id
            .and_then(|model_id| group.models.iter().position(|model| model.id == model_id))
            .or_else(|| group.models.iter().position(|model| model.enabled))
            .unwrap_or(0);
        let mut default_model = group.models[preferred_index].clone();
        if group.models.len() != 1
            || preferred_index != 0
            || !group.enabled
            || !default_model.enabled
        {
            changed = true;
        }
        group.enabled = true;
        default_model.enabled = true;
        group.models = vec![default_model];
    }
    changed
}

fn normalize_cli_group(group: &mut CopilotByokGroup) -> Result<(), AppError> {
    if group.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID || group.category.as_deref() == Some("official")
    {
        return Err(AppError::InvalidInput(
            "The built-in GitHub Copilot Official provider cannot be edited".to_string(),
        ));
    }
    group.category = None;
    group.normalize();
    if group.models.len() != 1 {
        return Err(AppError::InvalidInput(
            "Copilot CLI providers must define exactly one default model".to_string(),
        ));
    }
    group.enabled = true;
    group.models[0].enabled = true;
    group.validate()
}

fn imported_cli_provider_name(groups: &[CopilotByokGroup]) -> String {
    const BASE_NAME: &str = "Imported Copilot CLI Environment";
    if !groups
        .iter()
        .any(|group| group.name.eq_ignore_ascii_case(BASE_NAME))
    {
        return BASE_NAME.to_string();
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{BASE_NAME} ({suffix})");
        if !groups
            .iter()
            .any(|group| group.name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        suffix += 1;
    }
}

fn migrate_cli_environment_provider(
    db: &Database,
    store: &mut CopilotByokStore,
    cli_groups: &mut Vec<CopilotByokGroup>,
) -> Result<(), AppError> {
    if store.cli_environment_import_initialized {
        return Ok(());
    }

    let legacy_values = store.cli.managed_environment.original.clone();
    let has_legacy_values = legacy_values.values().any(Option::is_some);
    let import_current_environment =
        !has_legacy_values && !store.cli.enabled && !store.cli.official_override_active;
    let values = if has_legacy_values {
        legacy_values
    } else if import_current_environment {
        cli::current_environment()?
    } else {
        cli::official_environment()
    };

    let import_name = imported_cli_provider_name(cli_groups);
    let imported = match cli::imported_group_from_environment(&values, &import_name) {
        Ok(group) => group,
        Err(error) => {
            // Keep the legacy snapshot and retry on a later startup. Hiding the
            // obsolete restore action must never silently discard credentials
            // from an environment shape that a newer build may understand.
            log::warn!(
                "Could not import the existing Copilot CLI environment as a provider: {error}"
            );
            return Ok(());
        }
    };

    if let Some(imported) = imported {
        let imported_id = imported.id.clone();
        let imported_model_id = imported.models[0].id.clone();
        if !cli_groups.iter().any(|group| group.id == imported_id) {
            cli_groups.push(imported);
            persist_cli_catalog(db, cli_groups)?;
        }

        if import_current_environment {
            store.cli = store::CopilotCliConfig {
                enabled: true,
                official_override_active: false,
                selected_group_id: Some(imported_id),
                selected_model_id: Some(imported_model_id),
                managed_environment: store::CopilotCliManagedEnvironment {
                    original: cli::official_environment(),
                    last_written: values,
                },
            };
        } else if store.cli.official_override_active {
            // The live environment is already Official; the old snapshot now
            // lives in the provider catalog and no longer needs a restore flag.
            store.cli = store::CopilotCliConfig::default();
        } else if store.cli.enabled {
            // Preserve the current selected provider, but future Official
            // activation clears overrides instead of restoring hidden state.
            store.cli.official_override_active = false;
            store.cli.managed_environment.original = cli::official_environment();
        }
    } else if store.cli.official_override_active {
        store.cli = store::CopilotCliConfig::default();
    }

    store.cli_environment_import_initialized = true;
    store::save_device_store(store)
}

fn load_runtime_store(db: &Database) -> Result<CopilotByokStore, AppError> {
    let mut local = store::load_store()?;
    ensure_cli_official_provider(db, Some(&mut local))?;
    if !local.groups.is_empty() {
        let existing = db.get_all_providers(CATALOG_APP_TYPE)?;
        let mut next_sort = existing.len();
        for group in &local.groups {
            if !existing.contains_key(&group.id) {
                db.save_provider(CATALOG_APP_TYPE, &group_to_provider(group, next_sort)?)?;
                next_sort += 1;
            }
        }
        local.groups.clear();
        store::save_device_store(&local)?;
    }
    local.groups = load_catalog(db)?;
    if !local.cli_catalog_initialized {
        if load_cli_catalog(db)?.is_empty() && !local.groups.is_empty() {
            // The pre-split implementation exposed the VS Code catalog to the
            // CLI. Copy it once so existing selections survive, then let both
            // catalogs evolve independently.
            let mut cli_groups = local.groups.clone();
            for group in &mut cli_groups {
                // The historical shared implementation ignored VS Code's
                // enabled flags when applying a CLI selection. Preserve that
                // availability during the one-time catalog split.
                group.enabled = true;
                for model in &mut group.models {
                    model.enabled = true;
                }
            }
            persist_cli_catalog(db, &cli_groups)?;
        }
        local.cli_catalog_initialized = true;
        store::save_device_store(&local)?;
    }
    let mut cli_groups = load_cli_catalog(db)?;
    let catalog_changed = collapse_cli_catalog_to_default_models(
        &mut cli_groups,
        local.cli.selected_group_id.as_deref(),
        local.cli.selected_model_id.as_deref(),
    );
    if catalog_changed {
        // Keep enforcing the invariant after the one-time migration marker is
        // set as an older cloud/database snapshot may reintroduce a multi-model
        // CLI record on another device.
        persist_cli_catalog(db, &cli_groups)?;
    }
    migrate_cli_environment_provider(db, &mut local, &mut cli_groups)?;
    let mut device_state_changed = !local.cli_single_model_catalog_initialized;
    if let Some(selected_group_id) = local.cli.selected_group_id.as_deref() {
        if let Some(default_model) = cli_groups
            .iter()
            .find(|group| group.id == selected_group_id)
            .and_then(|group| group.models.first())
        {
            if local.cli.selected_model_id.as_deref() != Some(default_model.id.as_str()) {
                local.cli.selected_model_id = Some(default_model.id.clone());
                device_state_changed = true;
            }
        }
    }
    local.cli_single_model_catalog_initialized = true;
    if device_state_changed {
        store::save_device_store(&local)?;
    }
    store::normalize_store(&mut local)?;
    Ok(local)
}

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
    pub cli: CopilotCliState,
}

fn operation_guard() -> Result<MutexGuard<'static, ()>, AppError> {
    OPERATION_LOCK
        .lock()
        .map_err(|error| AppError::Lock(error.to_string()))
}

/// Resolve the device-local VS Code profile targets currently selected for
/// Copilot management. Provider catalog data is portable, while these paths
/// intentionally remain local to this device.
pub(crate) fn selected_language_model_paths() -> Result<Vec<PathBuf>, AppError> {
    selected_resource_paths(sync::TargetResource::LanguageModels)
}

pub(crate) fn selected_prompt_homes() -> Result<Vec<PathBuf>, AppError> {
    selected_resource_paths(sync::TargetResource::PromptsHome)
}

pub(crate) fn selected_mcp_paths() -> Result<Vec<PathBuf>, AppError> {
    selected_resource_paths(sync::TargetResource::Mcp)
}

fn selected_resource_paths(resource: sync::TargetResource) -> Result<Vec<PathBuf>, AppError> {
    let store = store::load_store()?;
    let discovered = vscode::discover_vscode_targets()?;
    let selected_ids = sync::effective_selected_target_ids(&store, &discovered);
    sync::resolve_resource_paths_from_discovered(&store, &selected_ids, resource, &discovered)
        .map(|targets| targets.into_iter().map(|(_, path)| path).collect())
}

pub(crate) fn primary_profile_config_dir() -> Result<PathBuf, AppError> {
    selected_language_model_paths()?
        .into_iter()
        .find_map(|path| path.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            AppError::Config(
                "No VS Code Copilot sync target is selected on this device".to_string(),
            )
        })
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
    let mut representatives: HashMap<(String, String, String), String> = HashMap::new();
    let mut aliases = HashMap::new();
    for target in targets {
        let identity = (
            store::path_identity_key(&target.path()),
            store::path_identity_key(&target.prompts_home()),
            store::path_identity_key(&target.mcp_path()),
        );
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

fn build_state(db: &Database, store: CopilotByokStore) -> Result<CopilotByokState, AppError> {
    let cli_groups = load_cli_catalog(db)?;
    let cli = cli::get_state(&store, &cli_groups)?;
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
        cli,
    })
}

fn build_cli_state(db: &Database, store: CopilotByokStore) -> Result<CopilotByokState, AppError> {
    let custom_groups = load_cli_catalog(db)?;
    let cli = cli::get_state(&store, &custom_groups)?;
    let groups = build_cli_provider_catalog(db, &custom_groups)?;
    Ok(CopilotByokState {
        managed_model_count: custom_groups
            .iter()
            .map(CopilotByokGroup::enabled_model_count)
            .sum(),
        groups,
        targets: Vec::new(),
        selected_target_ids: Vec::new(),
        cli,
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

pub fn get_state(db: &Database) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    build_state(db, load_runtime_store(db)?)
}

pub fn get_cli_state(db: &Database) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    build_cli_state(db, load_runtime_store(db)?)
}

pub fn set_cli_selection(
    db: &Database,
    group_id: &str,
    model_id: &str,
) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let mut store = load_runtime_store(db)?;
    let groups = load_cli_catalog(db)?;
    cli::apply(&mut store, &groups, group_id, model_id)?;
    build_cli_state(db, store)
}

fn resolve_cli_provider<'a>(
    groups: &'a [CopilotByokGroup],
    group_id: &str,
    group_name: Option<&str>,
) -> Result<&'a CopilotByokGroup, AppError> {
    if let Some(group) = groups
        .iter()
        .find(|group| group.id == group_id && group.enabled)
    {
        return Ok(group);
    }

    let requested_name = group_name.map(str::trim).filter(|name| !name.is_empty());
    if let Some(group) = requested_name.and_then(|name| {
        groups
            .iter()
            .find(|group| group.enabled && group.name.eq_ignore_ascii_case(name))
    }) {
        log::warn!(
            "Copilot CLI provider id changed from '{}' to '{}'; resolved by unique provider name '{}'",
            group_id,
            group.id,
            group.name
        );
        return Ok(group);
    }

    Err(AppError::InvalidInput(format!(
        "Unknown Copilot CLI provider: {group_id}"
    )))
}

pub fn set_cli_provider(
    db: &Database,
    group_id: &str,
    group_name: Option<&str>,
    confirm_unmanaged_clear: bool,
) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let mut store = load_runtime_store(db)?;
    let groups = load_cli_catalog(db)?;
    if group_id == COPILOT_CLI_OFFICIAL_PROVIDER_ID {
        cli::use_official(&mut store, &groups, confirm_unmanaged_clear)?;
        return build_cli_state(db, store);
    }
    let group = resolve_cli_provider(&groups, group_id, group_name)?;
    let resolved_group_id = group.id.clone();
    let model = group.models.first().ok_or_else(|| {
        AppError::Config(format!(
            "Copilot CLI provider {resolved_group_id} has no default model"
        ))
    })?;
    if group.models.len() != 1 || !model.enabled {
        return Err(AppError::Config(format!(
            "Copilot CLI provider {resolved_group_id} must have exactly one enabled default model"
        )));
    }
    let model_id = model.id.clone();
    cli::apply(&mut store, &groups, &resolved_group_id, &model_id)?;
    build_cli_state(db, store)
}

pub fn disable_cli(db: &Database) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let mut store = load_runtime_store(db)?;
    let groups = load_cli_catalog(db)?;
    cli::disable(&mut store, &groups)?;
    build_cli_state(db, store)
}

pub(crate) fn cli_launch_environment(
    db: &Database,
    group_id: &str,
) -> Result<std::collections::BTreeMap<String, Option<String>>, AppError> {
    let _guard = operation_guard()?;
    load_runtime_store(db)?;
    if group_id == COPILOT_CLI_OFFICIAL_PROVIDER_ID {
        return cli::launch_environment(None);
    }
    let groups = load_cli_catalog(db)?;
    let group = groups
        .iter()
        .find(|group| group.id == group_id)
        .ok_or_else(|| {
            AppError::InvalidInput(format!("Unknown Copilot CLI provider: {group_id}"))
        })?;
    cli::launch_environment(Some(group))
}

pub fn sync_selected_on_startup(db: &Database) -> Result<(), AppError> {
    sync_if_configured(db)
}

/// Sync Copilot BYOK when this device has at least one available selected
/// target. Global synchronization calls this tolerant variant so users who do
/// not use VS Code Copilot do not block unrelated provider, MCP, or Skill
/// projections. Once a target is configured, real synchronization failures
/// are still propagated to the caller.
pub(crate) fn sync_if_configured(db: &Database) -> Result<(), AppError> {
    let _guard = operation_guard()?;
    let store = load_runtime_store(db)?;
    sync_if_selected(&store)
}

/// 返回会话用量导入所需的 BYOK 目录。调用方只使用供应商与模型元数据，
/// 不会复制或记录 API Key。
pub(crate) fn usage_catalog(db: &Database) -> Result<Vec<CopilotByokGroup>, AppError> {
    let _guard = operation_guard()?;
    load_catalog(db)
}

pub(crate) fn cli_usage_catalog(db: &Database) -> Result<Vec<CopilotByokGroup>, AppError> {
    let _guard = operation_guard()?;
    load_runtime_store(db)?;
    load_cli_catalog(db)
}

pub(crate) fn provider_database_app_type(app_type: &AppType) -> &str {
    match app_type {
        AppType::CopilotByok => CATALOG_APP_TYPE,
        AppType::CopilotCli => CLI_CATALOG_APP_TYPE,
        _ => app_type.as_str(),
    }
}

fn update_usage_script_for_catalog(
    db: &Database,
    app_type: AppType,
    group_id: &str,
    usage_script: UsageScript,
) -> Result<CopilotByokState, AppError> {
    crate::services::ProviderService::validate_usage_script_config(&usage_script)?;
    let _guard = operation_guard()?;
    let store = load_runtime_store(db)?;
    if matches!(app_type, AppType::CopilotCli) {
        ensure_cli_official_provider(db, None)?;
    }
    let database_app_type = provider_database_app_type(&app_type);
    let mut provider = db
        .get_provider_by_id(group_id, database_app_type)?
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown {} provider: {group_id}",
                app_type.as_str()
            ))
        })?;
    let previous = provider.clone();
    provider
        .meta
        .get_or_insert_with(Default::default)
        .usage_script = Some(usage_script);
    db.save_provider(database_app_type, &provider)?;

    let result = match app_type {
        AppType::CopilotByok => build_state(db, store),
        AppType::CopilotCli => build_cli_state(db, store),
        _ => unreachable!("special catalog usage updates only support Copilot apps"),
    };
    result.map_err(|error| {
        if let Err(rollback_error) = db.save_provider(database_app_type, &previous) {
            return AppError::Config(format!(
                "{error}; failed to roll back usage query configuration: {rollback_error}"
            ));
        }
        error
    })
}

pub fn update_usage_script(
    db: &Database,
    group_id: &str,
    usage_script: UsageScript,
) -> Result<CopilotByokState, AppError> {
    update_usage_script_for_catalog(db, AppType::CopilotByok, group_id, usage_script)
}

pub fn update_cli_usage_script(
    db: &Database,
    group_id: &str,
    usage_script: UsageScript,
) -> Result<CopilotByokState, AppError> {
    update_usage_script_for_catalog(db, AppType::CopilotCli, group_id, usage_script)
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
    commit_and_build(
        db,
        &current,
        &updated,
        sync::TransactionOverrides::default(),
    )
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
    commit_and_build(
        db,
        &current,
        &updated,
        sync::TransactionOverrides::default(),
    )
}

pub fn remove_custom_target(db: &Database, target_id: &str) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let mut updated = current.clone();
    updated
        .custom_targets
        .retain(|target| target.id != target_id);
    updated.selected_target_ids.retain(|id| id != target_id);
    commit_and_build(
        db,
        &current,
        &updated,
        sync::TransactionOverrides::default(),
    )
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

pub fn upsert_cli_group(
    db: &Database,
    mut group: CopilotByokGroup,
) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    normalize_cli_group(&mut group)?;
    let store = load_runtime_store(db)?;
    let previous_groups = load_cli_catalog(db)?;
    let mut groups = previous_groups.clone();
    if let Some(existing) = groups.iter_mut().find(|item| item.id == group.id) {
        *existing = group;
    } else {
        groups.push(group);
    }
    store::normalize_groups(&mut groups)?;
    cli::validate_selection(&store, &groups)?;
    persist_cli_catalog(db, &groups)?;
    match build_cli_state(db, store) {
        Ok(state) => Ok(state),
        Err(error) => {
            persist_cli_catalog(db, &previous_groups).map_err(|rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back Copilot CLI catalog: {rollback_error}"
                ))
            })?;
            Err(error)
        }
    }
}

pub fn delete_cli_group(db: &Database, group_id: &str) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    if group_id == COPILOT_CLI_OFFICIAL_PROVIDER_ID {
        return Err(AppError::InvalidInput(
            "The built-in GitHub Copilot Official provider cannot be deleted".to_string(),
        ));
    }
    let store = load_runtime_store(db)?;
    let previous_groups = load_cli_catalog(db)?;
    let mut groups = previous_groups.clone();
    groups.retain(|group| group.id != group_id);
    cli::validate_selection(&store, &groups)?;
    persist_cli_catalog(db, &groups)?;
    match build_cli_state(db, store) {
        Ok(state) => Ok(state),
        Err(error) => {
            persist_cli_catalog(db, &previous_groups).map_err(|rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back Copilot CLI catalog: {rollback_error}"
                ))
            })?;
            Err(error)
        }
    }
}

pub fn reorder_cli_groups(
    db: &Database,
    group_ids: Vec<String>,
) -> Result<CopilotByokState, AppError> {
    let _guard = operation_guard()?;
    let store = load_runtime_store(db)?;
    let custom_groups = load_cli_catalog(db)?;
    let previous_groups = build_cli_provider_catalog(db, &custom_groups)?;
    let mut groups = previous_groups.clone();
    store::apply_group_order(&mut groups, &group_ids)?;
    persist_ordered_cli_catalog(db, &groups)?;
    match build_cli_state(db, store) {
        Ok(state) => Ok(state),
        Err(error) => {
            persist_ordered_cli_catalog(db, &previous_groups).map_err(|rollback_error| {
                AppError::Config(format!(
                    "{error}; failed to roll back Copilot CLI catalog: {rollback_error}"
                ))
            })?;
            Err(error)
        }
    }
}

pub fn import_models(db: &Database, target_id: &str) -> Result<CopilotByokImportResult, AppError> {
    let _guard = operation_guard()?;
    let current = load_runtime_store(db)?;
    let prepared = import::prepare_import_from_target(current.clone(), target_id)?;
    if prepared.result.imported_group_count == 0 {
        return Ok(prepared.result);
    }

    persist_catalog(db, &prepared.updated_store.groups)?;
    match sync::commit_store_update(
        &prepared.original_store,
        &prepared.updated_store,
        prepared.overrides,
        true,
    ) {
        Ok(sync_result) => {
            let mut result = prepared.result;
            result.changed_target_count = sync_result.changed_target_count;
            Ok(result)
        }
        Err(error) => {
            persist_catalog(db, &current.groups).map_err(|rollback_error| {
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
    use crate::copilot_byok::model::CopilotByokModel;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn group(id: &str, name: &str) -> CopilotByokGroup {
        CopilotByokGroup {
            id: id.to_string(),
            name: name.to_string(),
            url: "https://api.example.com/v1".to_string(),
            api_key: "secret".to_string(),
            api_type: "chat-completions".to_string(),
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            category: None,
            usage_script: None,
            enabled: true,
            request_headers: BTreeMap::new(),
            models: vec![CopilotByokModel {
                id: format!("{id}:model"),
                model_id: "model-1".to_string(),
                name: "Model 1".to_string(),
                enabled: true,
                tool_calling: Some(true),
                vision: None,
                thinking: None,
                streaming: None,
                context_window: None,
                max_input_tokens: None,
                max_output_tokens: None,
                edit_tools: Vec::new(),
                zero_data_retention_enabled: false,
                supports_reasoning_effort: Vec::new(),
                reasoning_effort_format: None,
                model_options: json!({}),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }
    }

    fn usage_script(label: &str) -> UsageScript {
        serde_json::from_value(json!({
            "enabled": true,
            "language": "javascript",
            "code": format!("return {{ planName: '{label}', remaining: 1 }};"),
            "timeout": 10
        }))
        .expect("usage script fixture")
    }

    #[test]
    fn cli_provider_selection_recovers_from_a_stale_catalog_id_by_unique_name() {
        let groups = vec![group("current-id", "Minimax")];

        let resolved = resolve_cli_provider(&groups, "stale-id", Some("minimax"))
            .expect("provider name should recover a stale catalog id");

        assert_eq!(resolved.id, "current-id");
    }

    fn detected_target(
        id: &str,
        profile_name: &str,
        is_default: bool,
        language_models_path: &str,
        prompts_home: &str,
        mcp_path: &str,
    ) -> VsCodeProfileTarget {
        VsCodeProfileTarget {
            id: id.to_string(),
            edition: VsCodeEdition::Stable,
            edition_name: "Visual Studio Code".to_string(),
            profile_id: (!is_default).then(|| profile_name.to_string()),
            profile_name: profile_name.to_string(),
            is_default,
            user_dir: "C:/Code/User".to_string(),
            resources: vscode::VsCodeProfileResources {
                language_models_path: language_models_path.to_string(),
                prompts_home: prompts_home.to_string(),
                mcp_path: mcp_path.to_string(),
            },
            config_exists: false,
            backup_exists: false,
        }
    }

    #[test]
    fn exact_profile_resource_aliases_collapse_to_the_default_target() {
        let default = detected_target(
            "stable:default",
            "Default",
            true,
            "C:/Code/User/chatLanguageModels.json",
            "C:/Code/User/prompts",
            "C:/Code/User/mcp.json",
        );
        let agents = detected_target(
            "stable:profile:builtin/agents",
            "Agents",
            false,
            "C:/Code/User/chatLanguageModels.json",
            "C:/Code/User/prompts",
            "C:/Code/User/mcp.json",
        );
        let shared_models_only = detected_target(
            "stable:profile:work",
            "Work",
            false,
            "C:/Code/User/chatLanguageModels.json",
            "C:/Code/User/profiles/work/prompts",
            "C:/Code/User/profiles/work/mcp.json",
        );
        let targets = vec![default, agents, shared_models_only];

        let aliases = detected_target_aliases(&targets);
        assert_eq!(
            aliases.get("stable:profile:builtin/agents"),
            Some(&"stable:default".to_string())
        );
        assert_eq!(
            aliases.get("stable:profile:work"),
            Some(&"stable:profile:work".to_string())
        );
        assert_eq!(
            canonicalize_detected_target_ids(
                &[
                    "stable:profile:builtin/agents".to_string(),
                    "stable:default".to_string(),
                    "stable:profile:work".to_string(),
                ],
                &aliases,
            ),
            vec!["stable:default", "stable:profile:work"]
        );
        assert_eq!(
            targets
                .iter()
                .filter(|target| aliases.get(&target.id) == Some(&target.id))
                .count(),
            2
        );
    }

    #[test]
    fn portable_catalog_round_trips_through_provider_database() -> Result<(), AppError> {
        let db = Database::memory()?;
        let first = group("first", "First");
        let second = group("second", "Second");

        let usage_provider = Provider::with_id(
            "vscode-copilot".to_string(),
            "VSCode Copilot".to_string(),
            json!({ "source": "vscode_session" }),
            None,
        );
        db.save_provider(LEGACY_CATALOG_APP_TYPE, &usage_provider)?;

        persist_catalog(&db, &[first.clone(), second.clone()])?;
        assert_eq!(load_catalog(&db)?, vec![first, second.clone()]);
        assert!(db
            .get_provider_by_id("vscode-copilot", LEGACY_CATALOG_APP_TYPE)?
            .is_some());

        persist_catalog(&db, std::slice::from_ref(&second))?;
        assert_eq!(load_catalog(&db)?, vec![second]);
        assert!(db.get_provider_by_id("first", CATALOG_APP_TYPE)?.is_none());
        Ok(())
    }

    #[test]
    fn usage_scripts_round_trip_and_official_metadata_survives_cli_reorder() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let mut vscode = group("vscode", "VS Code Provider");
        vscode.usage_script = Some(usage_script("vscode"));
        persist_catalog(&db, std::slice::from_ref(&vscode))?;

        let loaded = load_catalog(&db)?;
        assert_eq!(loaded[0].usage_script, vscode.usage_script);
        let stored = db
            .get_provider_by_id("vscode", CATALOG_APP_TYPE)?
            .expect("stored VS Code provider");
        assert_eq!(
            stored
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref()),
            vscode.usage_script.as_ref()
        );
        assert!(stored.settings_config.get("usageScript").is_none());

        ensure_cli_official_provider(&db, None)?;
        let mut official = db
            .get_provider_by_id(COPILOT_CLI_OFFICIAL_PROVIDER_ID, CLI_CATALOG_APP_TYPE)?
            .expect("official provider seed");
        official
            .meta
            .get_or_insert_with(Default::default)
            .usage_script = Some(usage_script("official"));
        db.save_provider(CLI_CATALOG_APP_TYPE, &official)?;

        let cli = group("cli", "CLI Provider");
        let second = group("second", "Second CLI Provider");
        persist_cli_catalog(&db, &[cli.clone(), second.clone()])?;
        let visible = build_cli_provider_catalog(&db, &load_cli_catalog(&db)?)?;
        let reordered = vec![
            second,
            visible
                .iter()
                .find(|group| group.id == COPILOT_CLI_OFFICIAL_PROVIDER_ID)
                .expect("official visible group")
                .clone(),
            cli,
        ];
        persist_ordered_cli_catalog(&db, &reordered)?;

        let visible = build_cli_provider_catalog(&db, &load_cli_catalog(&db)?)?;
        assert_eq!(
            visible
                .iter()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>(),
            vec!["second", COPILOT_CLI_OFFICIAL_PROVIDER_ID, "cli"]
        );
        assert_eq!(visible[1].usage_script, Some(usage_script("official")));
        Ok(())
    }

    #[test]
    fn usage_queries_resolve_first_class_catalog_namespaces() {
        assert_eq!(
            provider_database_app_type(&AppType::CopilotByok),
            CATALOG_APP_TYPE
        );
        assert_eq!(
            provider_database_app_type(&AppType::CopilotCli),
            CLI_CATALOG_APP_TYPE
        );
        assert_eq!(
            provider_database_app_type(&AppType::Claude),
            AppType::Claude.as_str()
        );
    }

    #[test]
    fn reserved_cli_official_id_collision_is_migrated_without_data_loss() -> Result<(), AppError> {
        let db = Database::memory()?;
        let collision = group(COPILOT_CLI_OFFICIAL_PROVIDER_ID, "Old Custom Provider");
        let occupied = group(COPILOT_CLI_MIGRATED_PROVIDER_ID, "Existing Provider");
        db.save_provider(CLI_CATALOG_APP_TYPE, &group_to_provider(&collision, 0)?)?;
        db.save_provider(CLI_CATALOG_APP_TYPE, &group_to_provider(&occupied, 1)?)?;

        ensure_cli_official_provider(&db, None)?;
        let providers = db.get_all_providers(CLI_CATALOG_APP_TYPE)?;
        let official = providers
            .get(COPILOT_CLI_OFFICIAL_PROVIDER_ID)
            .expect("official seed");
        assert_eq!(official.category.as_deref(), Some("official"));
        let migrated = providers
            .get("copilot-cli-official-custom-2")
            .expect("migrated custom provider");
        assert_eq!(migrated.name, "Old Custom Provider");
        assert_eq!(
            migrated
                .settings_config
                .get("id")
                .and_then(|value| value.as_str()),
            Some("copilot-cli-official-custom-2")
        );

        let mut store = CopilotByokStore::default();
        store.cli.selected_group_id = Some(COPILOT_CLI_OFFICIAL_PROVIDER_ID.to_string());
        assert!(remap_cli_official_collision_selection(
            &mut store,
            "copilot-cli-official-custom-2"
        ));
        assert_eq!(
            store.cli.selected_group_id.as_deref(),
            Some("copilot-cli-official-custom-2")
        );
        Ok(())
    }

    #[test]
    fn vscode_and_cli_provider_catalogs_are_independent() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut vscode = group("vscode", "VS Code Provider");
        let mut vscode_second_model = vscode.models[0].clone();
        vscode_second_model.id = "vscode:second".to_string();
        vscode_second_model.model_id = "vscode-second-model".to_string();
        vscode_second_model.name = "VS Code Second Model".to_string();
        vscode.models.push(vscode_second_model);

        let mut cli = group("cli", "CLI Provider");
        let mut cli_second_model = cli.models[0].clone();
        cli_second_model.id = "cli:second".to_string();
        cli_second_model.model_id = "cli-second-model".to_string();
        cli_second_model.name = "CLI Second Model".to_string();
        cli.models.push(cli_second_model);

        persist_catalog(&db, std::slice::from_ref(&vscode))?;
        persist_cli_catalog(&db, std::slice::from_ref(&cli))?;

        assert_eq!(load_catalog(&db)?, vec![vscode.clone()]);
        assert_eq!(load_cli_catalog(&db)?, vec![cli.clone()]);
        let official = db
            .get_provider_by_id(COPILOT_CLI_OFFICIAL_PROVIDER_ID, CLI_CATALOG_APP_TYPE)?
            .expect("Copilot CLI official provider seed");
        assert_eq!(official.category.as_deref(), Some("official"));
        let visible_cli_catalog = build_cli_provider_catalog(&db, &load_cli_catalog(&db)?)?;
        assert_eq!(visible_cli_catalog[0].id, COPILOT_CLI_OFFICIAL_PROVIDER_ID);
        assert_eq!(visible_cli_catalog[0].category.as_deref(), Some("official"));
        assert!(visible_cli_catalog[0].models.is_empty());

        let mut migrated_cli = load_cli_catalog(&db)?;
        assert!(collapse_cli_catalog_to_default_models(
            &mut migrated_cli,
            None,
            None,
        ));
        persist_cli_catalog(&db, &migrated_cli)?;

        let persisted_vscode = load_catalog(&db)?;
        assert_eq!(persisted_vscode, vec![vscode.clone()]);
        assert_eq!(persisted_vscode[0].models.len(), 2);
        let persisted_cli = load_cli_catalog(&db)?;
        assert_eq!(persisted_cli[0].models.len(), 1);
        assert_eq!(persisted_cli[0].models[0].id, cli.models[0].id);

        persist_cli_catalog(&db, &[])?;
        assert_eq!(load_catalog(&db)?, vec![vscode]);
        assert!(load_cli_catalog(&db)?.is_empty());
        assert!(db
            .get_provider_by_id("cli", CLI_CATALOG_APP_TYPE)?
            .is_none());
        assert!(db
            .get_provider_by_id(COPILOT_CLI_OFFICIAL_PROVIDER_ID, CLI_CATALOG_APP_TYPE)?
            .is_some());
        Ok(())
    }

    #[test]
    fn cli_catalog_migration_keeps_one_default_model_per_provider() {
        let mut selected = group("selected", "Selected");
        let mut active_model = selected.models[0].clone();
        active_model.id = "selected:active".to_string();
        active_model.model_id = "active-model".to_string();
        active_model.name = "Active Model".to_string();
        selected.models.push(active_model);

        let mut other = group("other", "Other");
        other.enabled = false;
        other.models[0].enabled = false;
        let mut enabled_model = other.models[0].clone();
        enabled_model.id = "other:enabled".to_string();
        enabled_model.model_id = "enabled-model".to_string();
        enabled_model.name = "Enabled Model".to_string();
        enabled_model.enabled = true;
        other.models.push(enabled_model);

        let mut groups = vec![selected, other];
        assert!(collapse_cli_catalog_to_default_models(
            &mut groups,
            Some("selected"),
            Some("selected:active"),
        ));
        assert_eq!(groups[0].models.len(), 1);
        assert_eq!(groups[0].models[0].id, "selected:active");
        assert_eq!(groups[1].models.len(), 1);
        assert_eq!(groups[1].models[0].id, "other:enabled");
        assert!(groups.iter().all(|group| group.enabled));
        assert!(groups.iter().all(|group| group.models[0].enabled));
        assert!(!collapse_cli_catalog_to_default_models(
            &mut groups,
            Some("selected"),
            Some("selected:active"),
        ));
    }

    #[test]
    fn cli_provider_requires_exactly_one_enabled_default_model() {
        let mut multiple = group("multiple", "Multiple");
        multiple.models.push(multiple.models[0].clone());
        let error = normalize_cli_group(&mut multiple).expect_err("multiple models must fail");
        assert!(error.to_string().contains("exactly one default model"));

        let mut single = group("single", "Single");
        single.enabled = false;
        single.models[0].enabled = false;
        normalize_cli_group(&mut single).expect("single model should normalize");
        assert!(single.enabled);
        assert!(single.models[0].enabled);
        assert!(single.category.is_none());

        let mut official = group(COPILOT_CLI_OFFICIAL_PROVIDER_ID, "Replacement");
        let error = normalize_cli_group(&mut official).expect_err("official provider is fixed");
        assert!(error.to_string().contains("cannot be edited"));
    }

    #[test]
    fn legacy_catalog_rows_move_without_consuming_usage_provider() -> Result<(), AppError> {
        let db = Database::memory()?;
        let legacy_group = group("legacy", "Legacy");
        db.save_provider(
            LEGACY_CATALOG_APP_TYPE,
            &group_to_provider(&legacy_group, 0)?,
        )?;
        db.save_provider(
            LEGACY_CATALOG_APP_TYPE,
            &Provider::with_id(
                "vscode-copilot".to_string(),
                "VSCode Copilot".to_string(),
                json!({ "source": "vscode_session" }),
                None,
            ),
        )?;

        assert_eq!(load_catalog(&db)?, vec![legacy_group]);
        assert!(db
            .get_provider_by_id("legacy", LEGACY_CATALOG_APP_TYPE)?
            .is_none());
        assert!(db
            .get_provider_by_id("vscode-copilot", LEGACY_CATALOG_APP_TYPE)?
            .is_some());
        Ok(())
    }
}
