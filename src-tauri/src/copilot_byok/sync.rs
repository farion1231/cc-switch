use super::model::{is_managed_group, CopilotByokGroup};
use super::store::{self, path_identity_key, CopilotByokCustomTarget, CopilotByokStore};
use super::vscode::{discover_vscode_targets, VsCodeProfileTarget};
use crate::error::AppError;
use crate::file_transaction::{commit_file_updates_with_hook, FileSnapshot, FileUpdate};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotByokSyncResult {
    pub target_ids: Vec<String>,
    pub managed_model_count: usize,
    pub changed_target_count: usize,
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.cc-switch.bak")
}

fn ensure_regular_target(path: &Path) -> Result<(), AppError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(AppError::InvalidInput(format!(
                "Refusing to modify symlinked Copilot BYOK config: {}",
                path.display()
            )));
        }
        if !metadata.is_file() {
            return Err(AppError::InvalidInput(format!(
                "Copilot BYOK target is not a regular file: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn ensure_file_size(path: &Path) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Copilot BYOK config exceeds {} MiB: {}",
            MAX_CONFIG_BYTES / 1024 / 1024,
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn read_language_model_groups(path: &Path) -> Result<Vec<Value>, AppError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    ensure_regular_target(path)?;
    ensure_file_size(path)?;

    let text = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = json5::from_str(&text).map_err(|error| {
        AppError::Config(format!(
            "Failed to parse VS Code language model config {}: {error}",
            path.display()
        ))
    })?;
    match value {
        Value::Array(groups) => Ok(groups),
        // VS Code's language-model editor normalizes this to an array when it
        // next writes the file, but its reader also accepts one root provider
        // object. Preserve that provider and normalize only when CC Switch
        // needs to write the merged configuration.
        Value::Object(_) => Ok(vec![value]),
        _ => Err(AppError::Config(format!(
            "VS Code language model config must be a JSON array or provider object: {}",
            path.display()
        ))),
    }
}

pub(crate) fn merge_managed_groups(
    existing: Vec<Value>,
    groups: &[CopilotByokGroup],
) -> Vec<Value> {
    let mut merged: Vec<Value> = existing
        .into_iter()
        .filter(|group| !is_managed_group(group))
        .collect();
    merged.extend(
        groups
            .iter()
            .filter(|group| group.enabled_model_count() > 0)
            .map(CopilotByokGroup::to_language_model_group),
    );
    merged
}

fn custom_target_path(target: &CopilotByokCustomTarget) -> PathBuf {
    PathBuf::from(&target.language_models_path)
}

fn target_path_map(
    store: &CopilotByokStore,
    discovered: &[VsCodeProfileTarget],
) -> HashMap<String, PathBuf> {
    let mut paths = HashMap::new();
    for target in discovered {
        paths.insert(target.id.clone(), target.path());
    }
    for target in &store.custom_targets {
        paths.insert(target.id.clone(), custom_target_path(target));
    }
    paths
}

pub fn effective_selected_target_ids(
    store: &CopilotByokStore,
    discovered: &[VsCodeProfileTarget],
) -> Vec<String> {
    if store.targets_initialized {
        // Tolerate stale ids left behind by deleted VS Code profiles or removed
        // custom targets: they no longer resolve to a path, so skip them here
        // instead of failing every sync/import with "Unknown target". The store
        // keeps the ids, so the selection revives if the profile reappears.
        let available: HashSet<&str> = discovered
            .iter()
            .map(|target| target.id.as_str())
            .chain(store.custom_targets.iter().map(|target| target.id.as_str()))
            .collect();
        return store
            .selected_target_ids
            .iter()
            .filter(|id| available.contains(id.as_str()))
            .cloned()
            .collect();
    }

    discovered
        .iter()
        .find(|target| target.id == "stable:default")
        .or_else(|| discovered.iter().find(|target| target.is_default))
        .map(|target| vec![target.id.clone()])
        .unwrap_or_default()
}

pub(crate) fn resolve_target_paths(
    store: &CopilotByokStore,
    requested_ids: &[String],
) -> Result<Vec<(String, PathBuf)>, AppError> {
    let discovered = discover_vscode_targets()?;
    let paths = target_path_map(store, &discovered);
    let mut seen = HashSet::new();
    requested_ids
        .iter()
        .map(|id| {
            paths
                .get(id)
                .cloned()
                .map(|path| (id.clone(), path))
                .ok_or_else(|| {
                    AppError::InvalidInput(format!(
                        "Unknown or unavailable Copilot BYOK target: {id}"
                    ))
                })
        })
        .filter(|result| {
            result
                .as_ref()
                .map_or(true, |(_, path)| seen.insert(path_identity_key(path)))
        })
        .collect()
}

#[derive(Debug, Default)]
pub(crate) struct TransactionOverrides {
    pub base_groups: HashMap<String, Vec<Value>>,
    pub restore_targets: HashSet<String>,
}

fn snapshot_file(path: &Path) -> Result<FileSnapshot, AppError> {
    crate::file_transaction::snapshot_file(path, Some(MAX_CONFIG_BYTES), "Copilot BYOK config")
}

fn all_target_paths(
    before: &CopilotByokStore,
    after: &CopilotByokStore,
    discovered: &[VsCodeProfileTarget],
) -> HashMap<String, PathBuf> {
    let mut paths = target_path_map(before, discovered);
    paths.extend(target_path_map(after, discovered));
    paths
}

pub(crate) fn commit_store_update(
    before: &CopilotByokStore,
    after: &CopilotByokStore,
    overrides: TransactionOverrides,
    persist_store: bool,
) -> Result<CopilotByokSyncResult, AppError> {
    commit_store_update_with_hook(
        before,
        after,
        overrides,
        persist_store,
        &store::store_path(),
        |_, _| Ok(()),
    )
}

fn commit_store_update_with_hook<F>(
    before: &CopilotByokStore,
    after: &CopilotByokStore,
    overrides: TransactionOverrides,
    persist_store: bool,
    store_path: &Path,
    before_apply: F,
) -> Result<CopilotByokSyncResult, AppError>
where
    F: FnMut(usize, &FileUpdate) -> Result<(), AppError>,
{
    let mut normalized_after = after.clone();
    store::normalize_store(&mut normalized_after)?;
    let after = &normalized_after;
    let discovered = discover_vscode_targets()?;
    let previous_ids: HashSet<String> = effective_selected_target_ids(before, &discovered)
        .into_iter()
        .collect();
    let next_target_ids = effective_selected_target_ids(after, &discovered);
    let next_ids: HashSet<String> = next_target_ids.iter().cloned().collect();
    let paths = all_target_paths(before, after, &discovered);
    let next_path_identities: HashSet<String> = next_ids
        .iter()
        .filter_map(|id| paths.get(id))
        .map(|path| path_identity_key(path))
        .collect();

    let override_identities = |ids: &HashSet<String>| -> Result<HashSet<String>, AppError> {
        ids.iter()
            .map(|id| {
                paths
                    .get(id)
                    .map(|path| path_identity_key(path))
                    .ok_or_else(|| {
                        AppError::InvalidInput(format!(
                            "Unknown or unavailable Copilot BYOK target: {id}"
                        ))
                    })
            })
            .collect()
    };
    let restore_identities = override_identities(&overrides.restore_targets)?;
    let mut base_groups_by_identity: HashMap<String, Vec<Value>> = HashMap::new();
    for (id, groups) in &overrides.base_groups {
        let path = paths.get(id).ok_or_else(|| {
            AppError::InvalidInput(format!("Unknown or unavailable Copilot BYOK target: {id}"))
        })?;
        let identity = path_identity_key(path);
        if base_groups_by_identity
            .insert(identity, groups.clone())
            .is_some_and(|existing| existing != *groups)
        {
            return Err(AppError::InvalidInput(format!(
                "Conflicting Copilot import bases resolve to the same file: {}",
                path.display()
            )));
        }
    }
    let mut action_ids: HashSet<String> = previous_ids.union(&next_ids).cloned().collect();
    action_ids.extend(overrides.base_groups.keys().cloned());
    action_ids.extend(overrides.restore_targets.iter().cloned());

    let mut action_ids: Vec<String> = action_ids.into_iter().collect();
    action_ids.sort();
    let mut updates = Vec::new();
    let mut changed_target_count = 0;
    let mut physical_paths: HashMap<String, Option<Vec<u8>>> = HashMap::new();

    for target_id in action_ids {
        let path = paths.get(&target_id).cloned().ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown or unavailable Copilot BYOK target: {target_id}"
            ))
        })?;
        let before_file = snapshot_file(&path)?;
        let backup = backup_path(&path);
        let backup_before = snapshot_file(&backup)?;
        let identity = path_identity_key(&path);

        let after_contents = if restore_identities.contains(&identity) {
            match &backup_before.contents {
                Some(contents) => Some(contents.clone()),
                None => {
                    let existing = read_language_model_groups(&path)?;
                    let unmanaged: Vec<Value> = existing
                        .iter()
                        .filter(|group| !is_managed_group(group))
                        .cloned()
                        .collect();
                    if unmanaged == existing {
                        before_file.contents.clone()
                    } else if unmanaged.is_empty() {
                        None
                    } else {
                        Some(crate::config::serialize_json_file_contents(&Value::Array(
                            unmanaged,
                        ))?)
                    }
                }
            }
        } else if !next_path_identities.contains(&identity) {
            if before_file.contents.is_none() {
                None
            } else {
                let existing = read_language_model_groups(&path)?;
                let unmanaged: Vec<Value> = existing
                    .iter()
                    .filter(|group| !is_managed_group(group))
                    .cloned()
                    .collect();
                if unmanaged == existing {
                    before_file.contents.clone()
                } else if unmanaged.is_empty() && backup_before.contents.is_none() {
                    None
                } else {
                    Some(crate::config::serialize_json_file_contents(&Value::Array(
                        unmanaged,
                    ))?)
                }
            }
        } else {
            let base_groups = base_groups_by_identity.get(&identity);
            let existing = match base_groups {
                Some(groups) => groups.clone(),
                None => read_language_model_groups(&path)?,
            };
            let merged = merge_managed_groups(existing.clone(), &after.groups);
            if base_groups.is_none() && merged == existing {
                before_file.contents.clone()
            } else {
                Some(crate::config::serialize_json_file_contents(&Value::Array(
                    merged,
                ))?)
            }
        };

        if before_file.contents == after_contents {
            continue;
        }
        if let Some(existing_after) = physical_paths.get(&identity) {
            if existing_after != &after_contents {
                return Err(AppError::InvalidInput(format!(
                    "Conflicting Copilot sync targets resolve to the same file: {}",
                    path.display()
                )));
            }
            continue;
        }
        physical_paths.insert(identity, after_contents.clone());
        // Models and recovery backups can both contain provider credentials.
        if backup_before.contents.is_none() {
            if let Some(contents) = before_file.contents {
                updates.push(FileUpdate::write_private(backup, contents));
            }
        }
        updates.push(match after_contents {
            Some(contents) => FileUpdate::write_private(path, contents),
            None => FileUpdate::delete_private(path),
        });
        changed_target_count += 1;
    }

    if persist_store {
        snapshot_file(store_path)?;
        updates.push(FileUpdate::write_private(
            store_path.to_path_buf(),
            store::device_store_contents(after)?,
        ));
    }
    // Preflight every resource before any write. A later failure restores
    // models, backups and target selection as one transaction.
    commit_file_updates_with_hook(updates, None, "Copilot target config", before_apply)?;

    Ok(CopilotByokSyncResult {
        target_ids: next_target_ids,
        managed_model_count: after
            .groups
            .iter()
            .map(CopilotByokGroup::enabled_model_count)
            .sum(),
        changed_target_count,
    })
}

pub fn sync_store(store: &CopilotByokStore) -> Result<CopilotByokSyncResult, AppError> {
    let discovered = discover_vscode_targets()?;
    if effective_selected_target_ids(store, &discovered).is_empty() {
        return Err(AppError::InvalidInput(
            "No VS Code profile is selected for Copilot BYOK sync".to_string(),
        ));
    }
    commit_store_update(store, store, TransactionOverrides::default(), false)
}

#[cfg(test)]
mod tests {
    use super::super::model::CopilotByokModel;
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn managed_group() -> CopilotByokGroup {
        serde_json::from_value(json!({
            "id": "managed",
            "name": "Managed",
            "url": "https://api.example.com/v1/chat/completions",
            "apiKey": "",
            "apiType": "chat-completions",
            "models": [{
                "id": "managed-model",
                "modelId": "model-a",
                "name": "Model A"
            }]
        }))
        .expect("managed group")
    }

    fn custom_store(paths: &[PathBuf]) -> CopilotByokStore {
        let targets: Vec<CopilotByokCustomTarget> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                CopilotByokCustomTarget::from_path(path, Some(format!("Target {index}")))
                    .expect("custom target")
            })
            .collect();
        CopilotByokStore {
            targets_initialized: true,
            selected_target_ids: targets.iter().map(|target| target.id.clone()).collect(),
            custom_targets: targets,
            ..CopilotByokStore::default()
        }
    }

    #[test]
    fn merge_preserves_unmanaged_groups() {
        let existing = vec![
            json!({"name": "User model", "vendor": "customendpoint"}),
            json!({"name": "CC Switch: Old", "vendor": "customendpoint"}),
        ];
        let merged = merge_managed_groups(existing, &[]);
        assert_eq!(
            merged,
            vec![json!({
                "name": "User model",
                "vendor": "customendpoint"
            })]
        );
    }

    #[test]
    fn reads_single_provider_object_used_by_vscode() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("chatLanguageModels.json");
        fs::write(
            &path,
            r#"{"name":"Copilot","vendor":"copilot","settings":{"gpt-5-mini":{}}}"#,
        )
        .expect("write language model config");

        let groups = read_language_model_groups(&path).expect("read provider object");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["name"], "Copilot");
        assert_eq!(groups[0]["vendor"], "copilot");
    }

    #[test]
    fn merge_writes_one_provider_group_with_multiple_models() {
        let group = CopilotByokGroup {
            id: "moonshot".to_string(),
            name: "Moonshot".to_string(),
            url: "https://api.example.com/v1/responses".to_string(),
            api_key: "secret".to_string(),
            api_type: "responses".to_string(),
            website_url: None,
            notes: None,
            icon: None,
            icon_color: None,
            category: None,
            enabled: true,
            request_headers: BTreeMap::new(),
            models: ["kimi-k2", "kimi-k3"]
                .into_iter()
                .map(|id| CopilotByokModel {
                    id: id.to_string(),
                    model_id: id.to_string(),
                    name: id.to_string(),
                    enabled: true,
                    tool_calling: Some(true),
                    vision: Some(false),
                    thinking: Some(true),
                    streaming: Some(true),
                    context_window: Some(262_144),
                    max_input_tokens: None,
                    max_output_tokens: Some(32_768),
                    edit_tools: Vec::new(),
                    zero_data_retention_enabled: false,
                    supports_reasoning_effort: Vec::new(),
                    reasoning_effort_format: None,
                    model_options: json!({}),
                    extra: BTreeMap::new(),
                })
                .collect(),
            extra: BTreeMap::new(),
        };

        let merged = merge_managed_groups(Vec::new(), &[group]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0]["name"], "Moonshot");
        assert_eq!(merged[0]["apiType"], "responses");
        assert_eq!(merged[0]["models"].as_array().map(Vec::len), Some(2));
        assert!(merged[0].get("url").is_none());
        assert_eq!(merged[0]["models"][0]["url"], merged[0]["models"][1]["url"]);
    }

    fn target(id: &str, edition: super::super::vscode::VsCodeEdition) -> VsCodeProfileTarget {
        VsCodeProfileTarget {
            id: id.to_string(),
            edition,
            edition_name: id.to_string(),
            profile_id: None,
            profile_name: "Default".to_string(),
            is_default: true,
            user_dir: format!("/{id}"),
            resources: super::super::vscode::VsCodeProfileResources {
                language_models_path: format!("/{id}/chatLanguageModels.json"),
            },
            config_exists: false,
            backup_exists: false,
        }
    }

    #[test]
    fn effective_selection_prefers_stable_default_before_initialization() {
        let store = CopilotByokStore::default();
        let targets = vec![
            target(
                "insiders:default",
                super::super::vscode::VsCodeEdition::Insiders,
            ),
            target(
                "stable:default",
                super::super::vscode::VsCodeEdition::Stable,
            ),
        ];
        assert_eq!(
            effective_selected_target_ids(&store, &targets),
            vec!["stable:default"]
        );
    }

    #[test]
    fn effective_selection_preserves_explicit_empty_selection() {
        let store = CopilotByokStore {
            targets_initialized: true,
            ..CopilotByokStore::default()
        };
        let targets = vec![target(
            "stable:default",
            super::super::vscode::VsCodeEdition::Stable,
        )];
        assert!(effective_selected_target_ids(&store, &targets).is_empty());
    }

    #[test]
    fn effective_selection_skips_targets_that_no_longer_exist() {
        let temp = tempfile::tempdir().expect("temp directory");
        let mut store = custom_store(&[temp.path().join("chatLanguageModels.json")]);
        store.selected_target_ids = vec![
            "stable:profile:deleted".to_string(),
            "stable:default".to_string(),
            store.custom_targets[0].id.clone(),
        ];
        let custom_id = store.custom_targets[0].id.clone();
        let targets = vec![target(
            "stable:default",
            super::super::vscode::VsCodeEdition::Stable,
        )];

        assert_eq!(
            effective_selected_target_ids(&store, &targets),
            vec!["stable:default".to_string(), custom_id]
        );
    }

    #[cfg(unix)]
    #[test]
    fn sync_writes_credential_files_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("chatLanguageModels.json");
        fs::write(&path, "[]").expect("initial config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("initial mode");
        let before = custom_store(std::slice::from_ref(&path));
        let mut after = before.clone();
        after.groups.push(managed_group());
        commit_store_update(&before, &after, TransactionOverrides::default(), false).expect("sync");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let mode = fs::metadata(backup_path(&path))
            .expect("backup metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn effective_selection_uses_only_explicit_target_ids() {
        let store = CopilotByokStore {
            targets_initialized: true,
            selected_target_ids: vec!["insiders:default".to_string()],
            ..CopilotByokStore::default()
        };
        let targets = vec![
            target(
                "stable:default",
                super::super::vscode::VsCodeEdition::Stable,
            ),
            target(
                "insiders:default",
                super::super::vscode::VsCodeEdition::Insiders,
            ),
        ];
        assert_eq!(
            effective_selected_target_ids(&store, &targets),
            vec!["insiders:default"]
        );
    }

    #[test]
    fn semantic_noop_preserves_json5_bytes_and_does_not_create_backup() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("chatLanguageModels.json");
        let original = br#"[
            // User-owned provider comment must survive an idempotent sync.
            {name: 'User', vendor: 'customendpoint', models: [],},
        ]"#;
        fs::write(&path, original).expect("user config");
        let store = custom_store(std::slice::from_ref(&path));

        let result = commit_store_update(&store, &store, TransactionOverrides::default(), false)
            .expect("idempotent sync");

        assert_eq!(result.changed_target_count, 0);
        assert_eq!(fs::read(&path).expect("preserved config"), original);
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn multi_target_preflight_leaves_every_file_unchanged_on_parse_error() {
        let temp = tempfile::tempdir().expect("temp directory");
        let first = temp.path().join("first").join("chatLanguageModels.json");
        let second = temp.path().join("second").join("chatLanguageModels.json");
        fs::create_dir_all(first.parent().unwrap()).expect("first directory");
        fs::create_dir_all(second.parent().unwrap()).expect("second directory");
        let original = br#"[{"name":"User","vendor":"customendpoint"}]"#;
        fs::write(&first, original).expect("first config");
        fs::write(&second, "not valid json5").expect("invalid second config");

        let before = custom_store(&[first.clone(), second]);
        let mut after = before.clone();
        after.groups.push(managed_group());
        assert!(
            commit_store_update(&before, &after, TransactionOverrides::default(), false,).is_err()
        );

        assert_eq!(fs::read(&first).expect("read first"), original);
        assert!(!backup_path(&first).exists());
    }

    #[test]
    fn multi_target_commit_updates_all_files_and_creates_recovery_backups() {
        let temp = tempfile::tempdir().expect("temp directory");
        let paths = [
            temp.path().join("first").join("chatLanguageModels.json"),
            temp.path().join("second").join("chatLanguageModels.json"),
        ];
        for path in &paths {
            fs::create_dir_all(path.parent().unwrap()).expect("target directory");
            fs::write(path, "[]").expect("target config");
        }

        let before = custom_store(&paths);
        let mut after = before.clone();
        after.groups.push(managed_group());
        let result = commit_store_update(&before, &after, TransactionOverrides::default(), false)
            .expect("transactional sync");

        assert_eq!(result.changed_target_count, 2);
        for path in &paths {
            let groups = read_language_model_groups(path).expect("synced groups");
            assert_eq!(groups.len(), 1);
            assert!(is_managed_group(&groups[0]));
            assert_eq!(fs::read_to_string(backup_path(path)).unwrap(), "[]");
        }
    }
}
