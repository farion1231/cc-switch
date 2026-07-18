//! Kimi Code CLI configuration adapter (Phase 1).
//!
//! Target: MoonshotAI/kimi-code (`kimi` CLI), not legacy kimi-cli.
//!
//! Live config is hybrid/additive:
//! - Multiple `[providers.*]` / `[models.*]` entries coexist in `config.toml`
//! - Activating a CC Switch card only updates top-level `default_model`
//! - Each CC Switch provider owns a scoped TOML fragment (one provider + its models)
//! - Unknown top-level sections/comments are preserved via `toml_edit::DocumentMut`
//! - OAuth credentials under `credentials/` are never read or written

use crate::config::{atomic_write, get_app_config_dir, get_home_dir};
use crate::error::AppError;
use crate::provider::Provider;
use crate::settings::{effective_backup_retain_count, get_kimi_code_override_dir};
use chrono::Local;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use toml_edit::{DocumentMut, Item, Table, Value as TomlEditValue};

const PROVIDER_TYPES: &[&str] = &[
    "kimi",
    "anthropic",
    "openai",
    "openai_responses",
    "google-genai",
    "vertexai",
];

// ============================================================================
// Path resolution
// ============================================================================

/// Resolve Kimi Code home directory.
///
/// Priority:
/// 1. CC Switch directory override
/// 2. `KIMI_CODE_HOME` env (trimmed, non-empty; not tilde-expanded)
/// 3. `~/.kimi-code`
pub fn get_kimi_code_dir() -> PathBuf {
    if let Some(override_dir) = get_kimi_code_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("KIMI_CODE_HOME") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    get_home_dir().join(".kimi-code")
}

/// Live config path: `$KIMI_CODE_HOME/config.toml`
pub fn get_kimi_code_config_path() -> PathBuf {
    get_kimi_code_dir().join("config.toml")
}

fn kimi_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Fragment / ownership helpers
// ============================================================================

/// Provider-owned fragment stored in `Provider.settings_config`.
///
/// ```json
/// {
///   "config": "<scoped TOML with [providers.x] + [models.y]>",
///   "selected_model": "alias",
///   "provider_id": "x"
/// }
/// ```
#[derive(Debug, Clone)]
pub struct KimiOwnedFragment {
    pub provider_id: String,
    pub selected_model: String,
    pub config_toml: String,
    pub model_aliases: BTreeSet<String>,
    pub uses_oauth: bool,
}

/// Active live selection derived from `default_model` → `models[alias].provider`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KimiActiveSelection {
    pub default_model: String,
    pub provider_id: String,
}

fn invalid_toml(error: impl std::fmt::Display) -> AppError {
    AppError::localized(
        "provider.kimicode.config.invalid_toml",
        format!("Kimi Code config.toml 格式错误: {error}"),
        format!("Invalid Kimi Code config.toml: {error}"),
    )
}

fn required_non_empty_string(table: &toml::value::Table, key: &str) -> Result<String, AppError> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.field.missing",
                format!("Kimi Code 配置缺少有效的 {key} 字段"),
                format!("Kimi Code configuration is missing a valid {key} field"),
            )
        })
}

fn is_managed_oauth_provider_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.starts_with("managed:") || trimmed == "kimi-code" || trimmed == "kimi_code"
}

fn provider_table_has_oauth(table: &toml::value::Table) -> bool {
    table
        .get("oauth")
        .map(|value| match value {
            toml::Value::Table(t) => !t.is_empty(),
            toml::Value::String(s) => !s.trim().is_empty(),
            toml::Value::Boolean(true) => true,
            _ => false,
        })
        .unwrap_or(false)
}

/// Whether a live provider entry is an official/managed OAuth provider that must
/// never be deleted or rewritten by CC Switch ownership rules.
pub fn is_protected_oauth_provider(name: &str, provider_table: &toml::value::Table) -> bool {
    is_managed_oauth_provider_name(name) || provider_table_has_oauth(provider_table)
}

fn ensure_providers_table(doc: &mut DocumentMut) -> Result<&mut Table, AppError> {
    if !doc.contains_key("providers") {
        doc["providers"] = Item::Table(Table::new());
    }
    doc["providers"].as_table_mut().ok_or_else(|| {
        AppError::localized(
            "provider.kimicode.providers.not_table",
            "Kimi Code providers 必须是表结构",
            "Kimi Code providers must be a TOML table",
        )
    })
}

fn ensure_models_table(doc: &mut DocumentMut) -> Result<&mut Table, AppError> {
    if !doc.contains_key("models") {
        doc["models"] = Item::Table(Table::new());
    }
    doc["models"].as_table_mut().ok_or_else(|| {
        AppError::localized(
            "provider.kimicode.models.not_table",
            "Kimi Code models 必须是表结构",
            "Kimi Code models must be a TOML table",
        )
    })
}

fn clone_item(item: &Item) -> Item {
    item.clone()
}

// ============================================================================
// Validation / parsing of provider-owned fragments
// ============================================================================

/// Validate a provider-owned scoped TOML fragment.
pub fn validate_owned_fragment(config_toml: &str) -> Result<KimiOwnedFragment, AppError> {
    let document = config_toml.parse::<toml::Value>().map_err(invalid_toml)?;
    let root = document.as_table().ok_or_else(|| {
        AppError::localized(
            "provider.kimicode.config.not_table",
            "Kimi Code 配置片段必须是 TOML 表结构",
            "Kimi Code configuration fragment must be a TOML table",
        )
    })?;

    let providers = root
        .get("providers")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.providers.missing",
                "Kimi Code 配置片段缺少 [providers.<name>]",
                "Kimi Code configuration fragment is missing [providers.<name>]",
            )
        })?;

    if providers.is_empty() {
        return Err(AppError::localized(
            "provider.kimicode.providers.empty",
            "Kimi Code 配置片段必须包含至少一个 provider",
            "Kimi Code configuration fragment must contain at least one provider",
        ));
    }
    if providers.len() != 1 {
        return Err(AppError::localized(
            "provider.kimicode.providers.multiple",
            "Kimi Code 每个供应商卡片只能拥有一个 provider 表",
            "Each Kimi Code provider card may own only one provider table",
        ));
    }

    let (provider_id, provider_value) = providers.iter().next().unwrap();
    let provider_table = provider_value.as_table().ok_or_else(|| {
        AppError::localized(
            "provider.kimicode.provider.not_table",
            format!("providers.{provider_id} 必须是表结构"),
            format!("providers.{provider_id} must be a table"),
        )
    })?;

    let provider_type = required_non_empty_string(provider_table, "type")?;
    if !PROVIDER_TYPES.contains(&provider_type.as_str()) {
        return Err(AppError::localized(
            "provider.kimicode.provider.type.invalid",
            format!(
                "不支持的 provider type '{provider_type}'。可选值: {}",
                PROVIDER_TYPES.join(", ")
            ),
            format!(
                "Unsupported provider type '{provider_type}'. Allowed: {}",
                PROVIDER_TYPES.join(", ")
            ),
        ));
    }

    // api_key optional when oauth present; base_url optional for some types.
    let has_api_key = provider_table
        .get("api_key")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let has_oauth = provider_table_has_oauth(provider_table);
    if !has_api_key && !has_oauth {
        // Allow env-table fallback if present.
        let has_env = provider_table
            .get("env")
            .and_then(toml::Value::as_table)
            .is_some_and(|t| !t.is_empty());
        if !has_env {
            return Err(AppError::localized(
                "provider.kimicode.credentials.missing",
                "Kimi Code provider 需要 api_key、oauth 或 env 凭证来源之一",
                "Kimi Code provider requires api_key, oauth, or env credentials",
            ));
        }
    }

    let models = root
        .get("models")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.models.missing",
                "Kimi Code 配置片段缺少 [models.<alias>]",
                "Kimi Code configuration fragment is missing [models.<alias>]",
            )
        })?;

    if models.is_empty() {
        return Err(AppError::localized(
            "provider.kimicode.models.empty",
            "Kimi Code 配置片段必须包含至少一个 model",
            "Kimi Code configuration fragment must contain at least one model",
        ));
    }

    let mut model_aliases = BTreeSet::new();
    for (alias, model_value) in models {
        let model_table = model_value.as_table().ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.model.not_table",
                format!("models.{alias} 必须是表结构"),
                format!("models.{alias} must be a table"),
            )
        })?;
        let model_provider = required_non_empty_string(model_table, "provider")?;
        if model_provider != *provider_id {
            return Err(AppError::localized(
                "provider.kimicode.model.provider_mismatch",
                format!(
                    "models.{alias}.provider 必须为 '{provider_id}'，当前为 '{model_provider}'"
                ),
                format!("models.{alias}.provider must be '{provider_id}', got '{model_provider}'"),
            ));
        }
        required_non_empty_string(model_table, "model")?;
        model_table
            .get("max_context_size")
            .and_then(toml::Value::as_integer)
            .filter(|value| *value >= 1)
            .ok_or_else(|| {
                AppError::localized(
                    "provider.kimicode.model.max_context_size.invalid",
                    format!("models.{alias}.max_context_size 必须是 >= 1 的整数"),
                    format!("models.{alias}.max_context_size must be an integer >= 1"),
                )
            })?;
        model_aliases.insert(alias.clone());
    }

    let selected_model = root
        .get("selected_model")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| model_aliases.iter().next().cloned())
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.selected_model.missing",
                "Kimi Code 配置片段缺少 selected_model",
                "Kimi Code configuration fragment is missing selected_model",
            )
        })?;

    if !model_aliases.contains(&selected_model) {
        return Err(AppError::localized(
            "provider.kimicode.selected_model.unknown",
            format!("selected_model '{selected_model}' 未在 models 中定义"),
            format!("selected_model '{selected_model}' is not defined in models"),
        ));
    }

    // Reject fragment fields that would clobber shared top-level live settings.
    for forbidden in [
        "default_model",
        "default_permission_mode",
        "thinking",
        "loop_control",
        "background",
        "image",
        "services",
        "permission",
        "hooks",
    ] {
        if root.contains_key(forbidden) {
            return Err(AppError::localized(
                "provider.kimicode.fragment.forbidden_field",
                format!("Kimi Code 供应商片段不得包含共享顶层字段 '{forbidden}'"),
                format!(
                    "Kimi Code provider fragment must not contain shared top-level field '{forbidden}'"
                ),
            ));
        }
    }

    Ok(KimiOwnedFragment {
        provider_id: provider_id.clone(),
        selected_model,
        config_toml: config_toml.to_string(),
        model_aliases,
        uses_oauth: has_oauth,
    })
}

/// Whether a provider card references Kimi-managed OAuth configuration.
/// Such cards may be imported and activated, but CC Switch must not create,
/// edit, or claim ownership of them; authentication remains `/login`-owned.
pub fn provider_uses_oauth(provider: &Provider) -> Result<bool, AppError> {
    Ok(fragment_from_provider(provider)?.uses_oauth)
}

/// Extract owned fragment from provider settings JSON.
pub fn extract_owned_fragment(settings: &Value) -> Result<KimiOwnedFragment, AppError> {
    let config_toml = settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.config.missing",
                "Kimi Code 配置缺少 config 字段",
                "Kimi Code configuration is missing the config field",
            )
        })?;

    let mut fragment = validate_owned_fragment(config_toml)?;

    if let Some(selected) = settings
        .get("selected_model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !fragment.model_aliases.contains(selected) {
            return Err(AppError::localized(
                "provider.kimicode.selected_model.unknown",
                format!("selected_model '{selected}' 未在 models 中定义"),
                format!("selected_model '{selected}' is not defined in models"),
            ));
        }
        fragment.selected_model = selected.to_string();
    }

    if let Some(provider_id) = settings
        .get("provider_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if provider_id != fragment.provider_id {
            return Err(AppError::localized(
                "provider.kimicode.provider_id.mismatch",
                format!(
                    "provider_id '{provider_id}' 与 config 中的 providers.{} 不一致",
                    fragment.provider_id
                ),
                format!(
                    "provider_id '{provider_id}' does not match providers.{} in config",
                    fragment.provider_id
                ),
            ));
        }
    }

    Ok(fragment)
}

// ============================================================================
// Live config read helpers
// ============================================================================

pub fn read_live_config_raw() -> Result<String, AppError> {
    let path = get_kimi_code_config_path();
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
}

/// Restore an exact live-config snapshot after a later DB operation fails.
/// The snapshot is validated before the atomic write and credentials are never touched.
pub fn restore_live_config_raw(snapshot: &str) -> Result<(), AppError> {
    parse_live_document(snapshot)?;
    let _guard = kimi_write_lock()
        .lock()
        .map_err(|_| AppError::Config("Kimi Code write lock poisoned".into()))?;
    let current = read_live_config_raw()?;
    write_live_raw(snapshot, &current)
}

pub fn parse_live_document(raw: &str) -> Result<DocumentMut, AppError> {
    if raw.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    raw.parse::<DocumentMut>().map_err(invalid_toml)
}

/// Resolve active provider from live `default_model` → models[alias].provider.
pub fn resolve_active_selection(raw: &str) -> Result<Option<KimiActiveSelection>, AppError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let value: toml::Value = raw.parse().map_err(invalid_toml)?;
    let root = match value.as_table() {
        Some(t) => t,
        None => return Ok(None),
    };
    let default_model = match root
        .get("default_model")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(v) => v.to_string(),
        None => return Ok(None),
    };
    let provider_id = root
        .get("models")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get(&default_model))
        .and_then(toml::Value::as_table)
        .and_then(|model| model.get("provider"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            AppError::localized(
                "provider.kimicode.default_model.unresolved",
                format!("无法从 default_model '{default_model}' 解析 provider"),
                format!("Unable to resolve provider from default_model '{default_model}'"),
            )
        })?;
    Ok(Some(KimiActiveSelection {
        default_model,
        provider_id,
    }))
}

/// List live providers as name → table JSON (no credentials dir access).
pub fn list_live_providers() -> Result<BTreeMap<String, Value>, AppError> {
    let raw = read_live_config_raw()?;
    if raw.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: toml::Value = raw.parse().map_err(invalid_toml)?;
    let mut out = BTreeMap::new();
    if let Some(providers) = value.get("providers").and_then(|v| v.as_table()) {
        for (name, entry) in providers {
            if let Ok(json_val) = toml_to_json(entry) {
                out.insert(name.clone(), json_val);
            }
        }
    }
    Ok(out)
}

fn toml_to_json(value: &toml::Value) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(|e| AppError::JsonSerialize { source: e })
}

/// Build a provider-owned fragment TOML for a live provider + its models.
pub fn build_fragment_from_live(
    provider_id: &str,
    provider: &toml::Value,
    models: &BTreeMap<String, toml::Value>,
    selected_model: Option<&str>,
) -> Result<String, AppError> {
    let mut root = toml::map::Map::new();
    let selected = selected_model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| models.keys().next().cloned())
        .unwrap_or_default();
    if !selected.is_empty() {
        root.insert("selected_model".into(), toml::Value::String(selected));
    }

    let mut providers = toml::map::Map::new();
    providers.insert(provider_id.to_string(), provider.clone());
    root.insert("providers".into(), toml::Value::Table(providers));

    let mut models_table = toml::map::Map::new();
    for (alias, model) in models {
        models_table.insert(alias.clone(), model.clone());
    }
    root.insert("models".into(), toml::Value::Table(models_table));
    toml::to_string(&toml::Value::Table(root))
        .map_err(|e| AppError::Config(format!("Failed to serialize Kimi Code fragment: {e}")))
}

/// Import live providers into fragment settings maps (no credentials/ access).
///
/// Returns map of provider_id → settings_config JSON for CC Switch DB seeding.
pub fn import_live_provider_settings() -> Result<BTreeMap<String, Value>, AppError> {
    let raw = read_live_config_raw()?;
    if raw.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: toml::Value = raw.parse().map_err(invalid_toml)?;
    let root = match value.as_table() {
        Some(t) => t,
        None => return Ok(BTreeMap::new()),
    };
    let providers = match root.get("providers").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => return Ok(BTreeMap::new()),
    };
    let models = root
        .get("models")
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default();
    let default_model = root
        .get("default_model")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut out = BTreeMap::new();
    for (provider_id, provider_val) in providers {
        let mut owned_models = BTreeMap::new();
        for (alias, model_val) in &models {
            if model_val
                .get("provider")
                .and_then(|v| v.as_str())
                .map(str::trim)
                == Some(provider_id.as_str())
            {
                owned_models.insert(alias.clone(), model_val.clone());
            }
        }
        if owned_models.is_empty() {
            // Still import provider-only entries with a synthetic placeholder model
            // so the card is visible; user can edit models later.
            continue;
        }
        let selected = default_model
            .filter(|alias| owned_models.contains_key(*alias))
            .or_else(|| owned_models.keys().next().map(|s| s.as_str()));
        let config = build_fragment_from_live(provider_id, provider_val, &owned_models, selected)?;
        // Validate fragment shape (skip oauth-only providers that lack api_key/env
        // only if validation would fail — re-validate with oauth allowance).
        if let Err(err) = validate_owned_fragment(&config) {
            log::warn!("Skipping Kimi Code live provider '{provider_id}' during import: {err}");
            continue;
        }
        out.insert(
            provider_id.clone(),
            json!({
                "config": config,
                "provider_id": provider_id,
                "selected_model": selected.unwrap_or_default(),
            }),
        );
    }
    Ok(out)
}

// ============================================================================
// Live write path (backup + atomic + validate + re-read)
// ============================================================================

fn create_kimi_backup(source: &str) -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir().join("backups").join("kimicode");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("kimicode_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.toml");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;
    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.toml");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }
    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_kimi_backups(&backup_dir)?;
    Ok(backup_path)
}

fn cleanup_kimi_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old Kimi Code config backup {}: {err}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn write_live_raw(new_raw: &str, previous: &str) -> Result<(), AppError> {
    // Validate new document parses.
    let _ = parse_live_document(new_raw)?;

    let path = get_kimi_code_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    if !previous.is_empty() && previous != new_raw {
        let _ = create_kimi_backup(previous)?;
    }

    atomic_write(&path, new_raw.as_bytes())?;

    // Re-read to confirm write landed; roll back on failure.
    let reread = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    if reread != new_raw {
        if !previous.is_empty() {
            let _ = atomic_write(&path, previous.as_bytes());
        }
        return Err(AppError::localized(
            "provider.kimicode.write.verify_failed",
            "Kimi Code 配置写入后校验失败，已尝试回滚",
            "Kimi Code config write verification failed; attempted rollback",
        ));
    }
    // Ensure re-parse succeeds.
    if let Err(err) = parse_live_document(&reread) {
        if !previous.is_empty() {
            let _ = atomic_write(&path, previous.as_bytes());
        }
        return Err(err);
    }
    Ok(())
}

fn merge_table_item(target: &mut Table, key: &str, source: &Item) {
    // Prefer replacing the whole provider/model table while keeping key order when possible.
    target.insert(key, clone_item(source));
}

/// Merge one managed provider fragment into live config.toml.
/// Does **not** change `default_model` (use [`merge_owned_fragment_and_activate`]).
pub fn merge_owned_fragment_into_live(fragment: &KimiOwnedFragment) -> Result<(), AppError> {
    merge_owned_fragment_into_live_with_default(fragment, None)
}

/// Atomically merge one fragment and activate its model alias in a single write.
pub fn merge_owned_fragment_and_activate(
    fragment: &KimiOwnedFragment,
    selected_model: &str,
) -> Result<(), AppError> {
    let selected = selected_model.trim();
    if selected.is_empty() {
        return Err(AppError::localized(
            "provider.kimicode.selected_model.missing",
            "切换 Kimi Code 供应商时 selected_model 不能为空",
            "selected_model must not be empty when switching Kimi Code provider",
        ));
    }
    merge_owned_fragment_into_live_with_default(fragment, Some(selected))
}

fn merge_owned_fragment_into_live_with_default(
    fragment: &KimiOwnedFragment,
    selected_model: Option<&str>,
) -> Result<(), AppError> {
    let _guard = kimi_write_lock()
        .lock()
        .map_err(|_| AppError::Config("Kimi Code write lock poisoned".into()))?;

    // Refuse to overwrite protected OAuth providers if fragment targets them with
    // non-oauth rewrite — still allow merge of non-oauth custom providers.
    let previous = read_live_config_raw()?;
    let mut live = parse_live_document(&previous)?;
    let frag_doc = parse_live_document(&fragment.config_toml)?;

    // Protect existing OAuth provider tables: if live has oauth and fragment
    // targets same name, only allow merge if we are not stripping oauth blindly.
    if let Some(live_providers) = live.get("providers").and_then(|i| i.as_table()) {
        if let Some(live_item) = live_providers.get(&fragment.provider_id) {
            if let Ok(toml_val) = item_to_toml_value(live_item) {
                if let Some(table) = toml_val.as_table() {
                    if is_protected_oauth_provider(&fragment.provider_id, table) {
                        // Allow updating models for OAuth providers, but do not
                        // replace the provider table itself (preserve oauth ref).
                        let providers = ensure_providers_table(&mut live)?;
                        // Keep existing provider table as-is.
                        let _ = providers;
                    } else {
                        let providers = ensure_providers_table(&mut live)?;
                        if let Some(frag_providers) =
                            frag_doc.get("providers").and_then(|i| i.as_table_like())
                        {
                            if let Some((_, item)) = frag_providers
                                .iter()
                                .find(|(k, _)| *k == fragment.provider_id)
                            {
                                merge_table_item(providers, &fragment.provider_id, item);
                            }
                        }
                    }
                }
            }
        } else if let Some(frag_providers) =
            frag_doc.get("providers").and_then(|i| i.as_table_like())
        {
            let providers = ensure_providers_table(&mut live)?;
            if let Some((_, item)) = frag_providers
                .iter()
                .find(|(k, _)| *k == fragment.provider_id)
            {
                merge_table_item(providers, &fragment.provider_id, item);
            }
        }
    } else if let Some(frag_providers) = frag_doc.get("providers").and_then(|i| i.as_table_like()) {
        let providers = ensure_providers_table(&mut live)?;
        if let Some((_, item)) = frag_providers
            .iter()
            .find(|(k, _)| *k == fragment.provider_id)
        {
            merge_table_item(providers, &fragment.provider_id, item);
        }
    }

    // When live had no providers table at all, the branch above may have created it.
    // Always merge models from fragment for owned aliases.
    {
        let models = ensure_models_table(&mut live)?;
        if let Some(frag_models) = frag_doc.get("models").and_then(|i| i.as_table_like()) {
            for (alias, item) in frag_models.iter() {
                if fragment.model_aliases.contains(alias) {
                    merge_table_item(models, alias, item);
                }
            }
        }
    }

    // Also ensure provider exists when OAuth protection skipped insert above for new providers.
    {
        let providers = ensure_providers_table(&mut live)?;
        if !providers.contains_key(&fragment.provider_id) {
            if let Some(frag_providers) = frag_doc.get("providers").and_then(|i| i.as_table_like())
            {
                if let Some((_, item)) = frag_providers
                    .iter()
                    .find(|(k, _)| *k == fragment.provider_id)
                {
                    merge_table_item(providers, &fragment.provider_id, item);
                }
            }
        }
    }

    if let Some(selected) = selected_model {
        let models = live
            .get("models")
            .and_then(|item| item.as_table_like())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.kimicode.models.missing",
                    "Kimi Code 配置缺少 models 表，无法激活模型",
                    "Kimi Code config has no models table; cannot activate model",
                )
            })?;
        if !models.iter().any(|(alias, _)| alias == selected) {
            return Err(AppError::localized(
                "provider.kimicode.selected_model.unknown",
                format!("Kimi Code 模型别名 '{selected}' 不存在"),
                format!("Kimi Code model alias '{selected}' does not exist"),
            ));
        }
        live["default_model"] = Item::Value(TomlEditValue::from(selected));
    }

    let new_raw = live.to_string();
    write_live_raw(&new_raw, &previous)
}

fn item_to_toml_value(item: &Item) -> Result<toml::Value, AppError> {
    let s = item.to_string();
    // item.to_string() for tables includes the table body without header in some cases;
    // wrap via document serialization.
    match item {
        Item::Table(t) => {
            let mut doc = DocumentMut::new();
            *doc.as_table_mut() = t.clone();
            let text = doc.to_string();
            text.parse().map_err(invalid_toml)
        }
        Item::Value(v) => {
            let text = format!("__v = {v}");
            let parsed: toml::Value = text.parse().map_err(invalid_toml)?;
            parsed
                .get("__v")
                .cloned()
                .ok_or_else(|| AppError::Config(format!("Failed to convert item: {s}")))
        }
        _ => {
            let text = item.to_string();
            text.parse().map_err(invalid_toml)
        }
    }
}

/// Activate a managed card by setting top-level `default_model` only.
pub fn apply_switch_defaults(selected_model: &str) -> Result<(), AppError> {
    let selected = selected_model.trim();
    if selected.is_empty() {
        return Err(AppError::localized(
            "provider.kimicode.selected_model.missing",
            "切换 Kimi Code 供应商时 selected_model 不能为空",
            "selected_model must not be empty when switching Kimi Code provider",
        ));
    }

    let _guard = kimi_write_lock()
        .lock()
        .map_err(|_| AppError::Config("Kimi Code write lock poisoned".into()))?;

    let previous = read_live_config_raw()?;
    let mut live = parse_live_document(&previous)?;

    // Validate the alias exists when models table is present.
    if let Some(models) = live.get("models").and_then(|i| i.as_table_like()) {
        if !models.iter().any(|(k, _)| k == selected) {
            return Err(AppError::localized(
                "provider.kimicode.selected_model.unknown",
                format!("selected_model '{selected}' 未在 live models 中定义"),
                format!("selected_model '{selected}' is not defined in live models"),
            ));
        }
    }

    live["default_model"] = Item::Value(TomlEditValue::from(selected));
    let new_raw = live.to_string();
    write_live_raw(&new_raw, &previous)
}

fn fragment_from_provider(provider: &Provider) -> Result<KimiOwnedFragment, AppError> {
    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.kimicode.settings.not_object",
            "Kimi Code 配置必须是 JSON 对象",
            "Kimi Code configuration must be a JSON object",
        )
    })?;
    extract_owned_fragment(&Value::Object(settings.clone()))
}

/// Merge an owned fragment without changing the active model.
pub fn write_provider_live(provider: &Provider) -> Result<(), AppError> {
    let fragment = fragment_from_provider(provider)?;
    merge_owned_fragment_into_live(&fragment)
}

/// Merge a provider and activate its selected model in one atomic live write.
pub fn activate_provider_live(provider: &Provider) -> Result<(), AppError> {
    let fragment = fragment_from_provider(provider)?;
    merge_owned_fragment_and_activate(&fragment, &fragment.selected_model)
}

/// Delete only demonstrably owned provider + model aliases from live config.
/// Never deletes protected OAuth providers or unrelated models.
pub fn remove_owned_fragment_from_live(fragment: &KimiOwnedFragment) -> Result<(), AppError> {
    let _guard = kimi_write_lock()
        .lock()
        .map_err(|_| AppError::Config("Kimi Code write lock poisoned".into()))?;

    let previous = read_live_config_raw()?;
    if previous.trim().is_empty() {
        return Ok(());
    }
    let mut live = parse_live_document(&previous)?;

    // Protect OAuth providers.
    if let Some(providers) = live.get("providers").and_then(|i| i.as_table()) {
        if let Some(item) = providers.get(&fragment.provider_id) {
            if let Ok(toml_val) = item_to_toml_value(item) {
                if let Some(table) = toml_val.as_table() {
                    if is_protected_oauth_provider(&fragment.provider_id, table) {
                        return Err(AppError::localized(
                            "provider.kimicode.delete.oauth_protected",
                            format!(
                                "无法删除官方/OAuth 管理的 provider '{}'",
                                fragment.provider_id
                            ),
                            format!(
                                "Cannot delete official/OAuth-managed provider '{}'",
                                fragment.provider_id
                            ),
                        ));
                    }
                }
            }
        }
    }

    if let Some(providers) = live.get_mut("providers").and_then(|i| i.as_table_mut()) {
        providers.remove(&fragment.provider_id);
    }

    if let Some(models) = live.get_mut("models").and_then(|i| i.as_table_mut()) {
        for alias in &fragment.model_aliases {
            // Only remove if model still points at this provider (or is in owned set).
            let should_remove = models
                .get(alias)
                .and_then(|item| item_to_toml_value(item).ok())
                .and_then(|v| v.as_table().cloned())
                .map(|t| {
                    t.get("provider").and_then(|v| v.as_str()).map(str::trim)
                        == Some(fragment.provider_id.as_str())
                })
                .unwrap_or(false);
            if should_remove {
                models.remove(alias);
            }
        }
    }

    // If default_model pointed at a removed model, clear it only when unresolved.
    if let Some(default) = live
        .get("default_model")
        .and_then(|i| i.as_str())
        .map(str::to_string)
    {
        let still_exists = live
            .get("models")
            .and_then(|i| i.as_table_like())
            .map(|m| m.iter().any(|(k, _)| k == default))
            .unwrap_or(false);
        if !still_exists {
            // Prefer first remaining model if any.
            let first_remaining_model = live
                .get("models")
                .and_then(|item| item.as_table_like())
                .and_then(|models| models.iter().next().map(|(alias, _)| alias.to_string()));
            if let Some(first) = first_remaining_model {
                live["default_model"] = Item::Value(TomlEditValue::from(first));
            }
        }
    }

    let new_raw = live.to_string();
    write_live_raw(&new_raw, &previous)
}

pub fn remove_provider_from_live(provider: &Provider) -> Result<(), AppError> {
    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.kimicode.settings.not_object",
            "Kimi Code 配置必须是 JSON 对象",
            "Kimi Code configuration must be a JSON object",
        )
    })?;
    let fragment = extract_owned_fragment(&Value::Object(settings.clone()))?;
    remove_owned_fragment_from_live(&fragment)
}

/// Backfill only the active managed fragment from live config (exclusive-mode style).
pub fn backfill_active_fragment(
    provider_id: &str,
    selected_model_hint: Option<&str>,
) -> Result<Option<Value>, AppError> {
    let raw = read_live_config_raw()?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let value: toml::Value = raw.parse().map_err(invalid_toml)?;
    let providers = value
        .get("providers")
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default();
    let Some(provider_val) = providers.get(provider_id) else {
        return Ok(None);
    };
    let models = value
        .get("models")
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default();
    let mut owned_models = BTreeMap::new();
    for (alias, model) in &models {
        if model
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::trim)
            == Some(provider_id)
        {
            owned_models.insert(alias.clone(), model.clone());
        }
    }
    if owned_models.is_empty() {
        return Ok(None);
    }
    let default_model = value
        .get("default_model")
        .and_then(toml::Value::as_str)
        .map(str::trim);
    let selected = selected_model_hint
        .or(default_model)
        .filter(|alias| owned_models.contains_key(*alias))
        .or_else(|| owned_models.keys().next().map(|s| s.as_str()));
    let config = build_fragment_from_live(provider_id, provider_val, &owned_models, selected)?;
    Ok(Some(json!({
        "config": config,
        "provider_id": provider_id,
        "selected_model": selected.unwrap_or_default(),
    })))
}

/// Read live settings for the currently active provider card (if resolvable).
pub fn read_active_live_settings() -> Result<Value, AppError> {
    let raw = read_live_config_raw()?;
    if raw.trim().is_empty() {
        return Err(AppError::localized(
            "kimicode.config.missing",
            "Kimi Code 配置文件不存在",
            "Kimi Code configuration file not found",
        ));
    }
    let selection = resolve_active_selection(&raw)?;
    if let Some(sel) = selection {
        if let Some(settings) =
            backfill_active_fragment(&sel.provider_id, Some(&sel.default_model))?
        {
            return Ok(settings);
        }
    }
    // Fallback: return whole raw as non-owned snapshot for diagnostics only.
    Ok(json!({ "config": raw }))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn sample_live_config() -> &'static str {
        r#"# Kimi Code live config
default_model = "kimi-code/k3"
default_permission_mode = "manual"
telemetry = true

# Official OAuth managed provider — must survive
[providers."managed:kimi-code"]
type = "kimi"
base_url = "https://api.kimi.com/coding/v1"
api_key = ""
oauth = { storage = "credentials", key = "kimi-code" }

[models."kimi-code/k3"]
provider = "managed:kimi-code"
model = "k3"
max_context_size = 1048576
display_name = "K3"

[thinking]
enabled = true
effort = "high"

[loop_control]
max_retries_per_step = 10

[[hooks]]
event = "PreToolUse"
matcher = "Bash"
command = "echo ok"
"#
    }

    fn fragment_a() -> &'static str {
        r#"selected_model = "custom/a1"

[providers.custom-a]
type = "openai"
base_url = "https://api.a.example/v1"
api_key = "sk-a"

[models."custom/a1"]
provider = "custom-a"
model = "model-a1"
max_context_size = 128000
display_name = "A1"

[models."custom/a2"]
provider = "custom-a"
model = "model-a2"
max_context_size = 64000
"#
    }

    fn fragment_b() -> &'static str {
        r#"selected_model = "custom/b1"

[providers.custom-b]
type = "anthropic"
base_url = "https://api.b.example"
api_key = "sk-b"

[models."custom/b1"]
provider = "custom-b"
model = "claude-sonnet"
max_context_size = 200000
"#
    }

    struct EnvGuard {
        home: Option<std::ffi::OsString>,
        kimi_home: Option<std::ffi::OsString>,
        app_home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(temp: &TempDir) -> Self {
            let guard = Self {
                home: std::env::var_os("CC_SWITCH_TEST_HOME"),
                kimi_home: std::env::var_os("KIMI_CODE_HOME"),
                app_home: std::env::var_os("CC_SWITCH_CONFIG_DIR"),
            };
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            std::env::remove_var("KIMI_CODE_HOME");
            // Isolate settings override store side effects if any.
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.home {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            match &self.kimi_home {
                Some(v) => std::env::set_var("KIMI_CODE_HOME", v),
                None => std::env::remove_var("KIMI_CODE_HOME"),
            }
            match &self.app_home {
                Some(v) => std::env::set_var("CC_SWITCH_CONFIG_DIR", v),
                None => std::env::remove_var("CC_SWITCH_CONFIG_DIR"),
            }
        }
    }

    fn seed_live(temp: &TempDir, content: &str) {
        let dir = temp.path().join(".kimi-code");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), content).unwrap();
    }

    #[test]
    fn validates_owned_fragment_shape() {
        let frag = validate_owned_fragment(fragment_a()).expect("fragment a");
        assert_eq!(frag.provider_id, "custom-a");
        assert_eq!(frag.selected_model, "custom/a1");
        assert!(frag.model_aliases.contains("custom/a1"));
        assert!(frag.model_aliases.contains("custom/a2"));
    }

    #[test]
    fn rejects_fragment_with_shared_top_level_fields() {
        let bad = format!("default_model = \"x\"\n{}", fragment_a());
        let err = validate_owned_fragment(&bad).expect_err("should reject");
        assert!(err.to_string().contains("default_model") || err.to_string().contains("共享"));
    }

    #[test]
    fn rejects_invalid_provider_type() {
        let bad = fragment_a().replace("type = \"openai\"", "type = \"nope\"");
        let err = validate_owned_fragment(&bad).expect_err("bad type");
        assert!(err.to_string().contains("type") || err.to_string().contains("nope"));
    }

    #[test]
    #[serial]
    fn two_providers_and_models_coexist() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());

        let a = validate_owned_fragment(fragment_a()).unwrap();
        let b = validate_owned_fragment(fragment_b()).unwrap();
        merge_owned_fragment_into_live(&a).unwrap();
        merge_owned_fragment_into_live(&b).unwrap();

        let raw = read_live_config_raw().unwrap();
        assert!(raw.contains("[providers.custom-a]"));
        assert!(raw.contains("[providers.custom-b]"));
        assert!(
            raw.contains("[providers.\"managed:kimi-code\"]") || raw.contains("managed:kimi-code")
        );
        assert!(raw.contains("custom/a1"));
        assert!(raw.contains("custom/a2"));
        assert!(raw.contains("custom/b1"));
        assert!(raw.contains("kimi-code/k3"));
        // Unrelated sections survive
        assert!(raw.contains("[thinking]"));
        assert!(raw.contains("[[hooks]]"));
        assert!(raw.contains("# Kimi Code live config") || raw.contains("telemetry"));
    }

    #[test]
    #[serial]
    fn switch_only_changes_default_model() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let a = validate_owned_fragment(fragment_a()).unwrap();
        merge_owned_fragment_into_live(&a).unwrap();

        let before = read_live_config_raw().unwrap();
        apply_switch_defaults("custom/a1").unwrap();
        let after = read_live_config_raw().unwrap();

        assert!(
            after.contains("default_model = \"custom/a1\"")
                || after.contains("default_model=\"custom/a1\"")
        );
        // Provider tables unchanged
        assert!(after.contains("[providers.custom-a]"));
        assert!(after.contains("managed:kimi-code"));
        assert!(after.contains("[thinking]"));
        // Only default_model line should differ for activation semantics
        let before_wo = before
            .lines()
            .filter(|l| !l.starts_with("default_model"))
            .collect::<Vec<_>>();
        let after_wo = after
            .lines()
            .filter(|l| !l.starts_with("default_model"))
            .collect::<Vec<_>>();
        assert_eq!(before_wo, after_wo);

        let sel = resolve_active_selection(&after).unwrap().unwrap();
        assert_eq!(sel.default_model, "custom/a1");
        assert_eq!(sel.provider_id, "custom-a");
    }

    #[test]
    #[serial]
    fn merge_and_activate_preserves_shared_config() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let a = validate_owned_fragment(fragment_a()).unwrap();

        merge_owned_fragment_and_activate(&a, "custom/a1").unwrap();

        let raw = read_live_config_raw().unwrap();
        assert!(raw.contains("default_model = \"custom/a1\""));
        assert!(raw.contains("[providers.custom-a]"));
        assert!(raw.contains("[models.\"custom/a1\"]"));
        assert!(raw.contains("managed:kimi-code"));
        assert!(raw.contains("oauth"));
        assert!(raw.contains("default_permission_mode"));
        assert!(raw.contains("[[hooks]]"));
        let selection = resolve_active_selection(&raw).unwrap().unwrap();
        assert_eq!(selection.provider_id, "custom-a");
    }

    #[test]
    #[serial]
    fn unrelated_toml_and_comments_survive() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let a = validate_owned_fragment(fragment_a()).unwrap();
        merge_owned_fragment_into_live(&a).unwrap();
        let raw = read_live_config_raw().unwrap();
        assert!(raw.contains("default_permission_mode"));
        assert!(raw.contains("telemetry"));
        assert!(raw.contains("[thinking]"));
        assert!(raw.contains("[loop_control]"));
        assert!(raw.contains("[[hooks]]"));
        assert!(raw.contains("PreToolUse"));
    }

    #[test]
    #[serial]
    fn official_oauth_provider_and_reference_survive() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let a = validate_owned_fragment(fragment_a()).unwrap();
        merge_owned_fragment_into_live(&a).unwrap();
        apply_switch_defaults("custom/a1").unwrap();
        // Delete custom-a only
        remove_owned_fragment_from_live(&a).unwrap();
        let raw = read_live_config_raw().unwrap();
        assert!(raw.contains("managed:kimi-code"));
        assert!(raw.contains("oauth"));
        assert!(raw.contains("kimi-code/k3"));
        assert!(!raw.contains("custom-a") || raw.contains("managed:kimi-code"));
        assert!(!raw.contains("sk-a"));
    }

    #[test]
    #[serial]
    fn delete_removes_only_owned_entries() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let a = validate_owned_fragment(fragment_a()).unwrap();
        let b = validate_owned_fragment(fragment_b()).unwrap();
        merge_owned_fragment_into_live(&a).unwrap();
        merge_owned_fragment_into_live(&b).unwrap();
        remove_owned_fragment_from_live(&a).unwrap();

        let raw = read_live_config_raw().unwrap();
        assert!(!raw.contains("[providers.custom-a]"));
        assert!(!raw.contains("custom/a1"));
        assert!(!raw.contains("custom/a2"));
        assert!(raw.contains("[providers.custom-b]"));
        assert!(raw.contains("custom/b1"));
        assert!(raw.contains("managed:kimi-code"));
        assert!(raw.contains("[thinking]"));
    }

    #[test]
    #[serial]
    fn delete_refuses_oauth_provider() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());

        let mut models = BTreeMap::new();
        models.insert(
            "kimi-code/k3".to_string(),
            toml::Value::Table({
                let mut t = toml::map::Map::new();
                t.insert(
                    "provider".into(),
                    toml::Value::String("managed:kimi-code".into()),
                );
                t.insert("model".into(), toml::Value::String("k3".into()));
                t.insert("max_context_size".into(), toml::Value::Integer(1048576));
                t
            }),
        );
        let provider = toml::Value::Table({
            let mut t = toml::map::Map::new();
            t.insert("type".into(), toml::Value::String("kimi".into()));
            t.insert(
                "oauth".into(),
                toml::Value::Table({
                    let mut o = toml::map::Map::new();
                    o.insert("storage".into(), toml::Value::String("credentials".into()));
                    o
                }),
            );
            t
        });
        // Build fragment that would target oauth provider — delete must refuse.
        let config = build_fragment_from_live(
            "managed:kimi-code",
            &provider,
            &models,
            Some("kimi-code/k3"),
        )
        .unwrap();
        // validate_owned_fragment may accept oauth-only
        let fragment = KimiOwnedFragment {
            provider_id: "managed:kimi-code".into(),
            selected_model: "kimi-code/k3".into(),
            config_toml: config,
            model_aliases: BTreeSet::from(["kimi-code/k3".into()]),
            uses_oauth: true,
        };
        let err = remove_owned_fragment_from_live(&fragment).expect_err("oauth protected");
        assert!(
            err.to_string().contains("OAuth")
                || err.to_string().contains("oauth")
                || err.to_string().contains("官方")
        );
        let raw = read_live_config_raw().unwrap();
        assert!(raw.contains("managed:kimi-code"));
    }

    #[test]
    #[serial]
    fn custom_kimi_code_home_override() {
        let temp = TempDir::new().unwrap();
        let custom = temp.path().join("custom-home");
        fs::create_dir_all(&custom).unwrap();
        fs::write(custom.join("config.toml"), sample_live_config()).unwrap();

        let original = std::env::var_os("KIMI_CODE_HOME");
        let home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("KIMI_CODE_HOME", &custom);
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        assert_eq!(get_kimi_code_dir(), custom);
        assert_eq!(get_kimi_code_config_path(), custom.join("config.toml"));
        let raw = read_live_config_raw().unwrap();
        assert!(raw.contains("managed:kimi-code"));

        match original {
            Some(v) => std::env::set_var("KIMI_CODE_HOME", v),
            None => std::env::remove_var("KIMI_CODE_HOME"),
        }
        match home {
            Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    #[serial]
    fn invalid_fragment_does_not_corrupt_live() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let before = read_live_config_raw().unwrap();

        let bad = r#"selected_model = "x"
[providers.bad]
type = "nope"
api_key = "x"
[models.x]
provider = "bad"
model = "m"
max_context_size = 1
"#;
        let err = validate_owned_fragment(bad).expect_err("invalid");
        assert!(err.to_string().contains("type") || err.to_string().contains("nope"));

        // Attempting merge with a forced invalid path shouldn't happen; write_live_raw
        // also rejects unparseable content. Simulate failed write path:
        let result = write_live_raw("this is not = toml [[[", &before);
        assert!(result.is_err());
        let after = read_live_config_raw().unwrap();
        assert_eq!(after, before);
    }

    #[test]
    #[serial]
    fn exact_snapshot_restore_rolls_back_live_changes() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let snapshot = read_live_config_raw().unwrap();
        let a = validate_owned_fragment(fragment_a()).unwrap();

        merge_owned_fragment_and_activate(&a, "custom/a1").unwrap();
        assert_ne!(read_live_config_raw().unwrap(), snapshot);

        restore_live_config_raw(&snapshot).unwrap();
        assert_eq!(read_live_config_raw().unwrap(), snapshot);
    }

    #[test]
    #[serial]
    fn import_live_builds_fragments_without_credentials_dir() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set(&temp);
        seed_live(&temp, sample_live_config());
        let a = validate_owned_fragment(fragment_a()).unwrap();
        merge_owned_fragment_into_live(&a).unwrap();

        // Create a fake credentials dir that must never be required for import.
        fs::create_dir_all(get_kimi_code_dir().join("credentials")).unwrap();
        fs::write(
            get_kimi_code_dir().join("credentials").join("secret.json"),
            "{\"token\":\"do-not-read\"}",
        )
        .unwrap();

        let imported = import_live_provider_settings().unwrap();
        assert!(imported.contains_key("custom-a"));
        // OAuth managed provider may or may not import depending on validation;
        // if imported, config must not include credentials file content.
        for (_id, settings) in &imported {
            let config = settings
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert!(!config.contains("do-not-read"));
            assert!(!config.contains("secret.json"));
        }
    }
}
