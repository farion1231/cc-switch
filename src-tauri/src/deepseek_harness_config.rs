use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml::Mapping;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;

pub const OFFICIAL_PROVIDER_ID: &str = "deepseek-official";
/// Desktop-only disk configuration. This adapter does not discover a running
/// CLI profile or its runtime overlays, and does not promise runtime hot reload.
pub const DESKTOP_PROFILE_NAME: &str = "desktop";
const API_KEY_FIELD: &str = "apiKey";
const SETTINGS_ENTRY_ID: &str = "llm-deepseek";
const SETTINGS_ENTRY_NAME: &str = "@deepseek-ai/dsh-llm-deepseek-api-key";
const PI_AI_ENTRY_ID: &str = "llm-pi-ai";
const PI_AI_ENTRY_NAME: &str = "@deepseek-ai/dsh-llm-pi-ai";
const DEFAULT_MODEL_ENTRY_ID: &str = "agent-default-model";
const DEFAULT_MODEL_ENTRY_NAME: &str = "@deepseek-ai/dsh-agent-default-model";
const API_KEY_REF: &str = "DEEPSEEK_API_KEY";
/// CC Switch 早期版本写在 `llm-deepseek` 段里的字段，当前 schema
/// (`deepSeekConfigFields + apiKeyEnv`) 没有：迁移遗留 settings.yaml 时必须丢弃，
/// 否则整段会被 DSH 的 validatePaths 拒掉。
const LEGACY_INVALID_FIELDS: [&str; 1] = ["profile"];
const PROFILE_PATCH_TEMPLATE: &str = "\
# Your patch layer for this dsh profile, applied after every bundle layer:
# a top-level YAML array of loader patch entries (id-targeted config
# overrides, disables, and insert lists; `!!js` expressions allowed).
[]
";
const PROFILE_PNPM_WORKSPACE: &str = "\
packages:
  - .

nodeLinker: hoisted
autoInstallPeers: false
";

/// A DeepSeek Harness provider profile stored by CC Switch.
///
/// `apiKey` is intentionally excluded from the profile patch: official Harness keeps
/// secrets in `$DSH_HOME/.credentials.yaml`, not in `cordis.patch.yml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DeepSeekHarnessProviderConfig {
    #[serde(rename = "apiKey", default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(rename = "baseURL", default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(
        rename = "reasoningEffort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<String>,
    #[serde(rename = "maxTokens", default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Value>,
    /// Historical CC Switch rows may carry unknown keys; they are tolerated on
    /// load but never re-emitted into the patch entry.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeProviderSource {
    DeepSeek,
    PiAi,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeProvider {
    pub name: String,
    pub source: NativeProviderSource,
    pub config: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NativeState {
    pub providers: indexmap::IndexMap<String, NativeProvider>,
    pub current_provider: Option<String>,
    pub current_model: Option<String>,
}

pub fn get_dsh_home() -> PathBuf {
    resolve_dsh_home(
        settings_override(),
        std::env::var_os("DSH_HOME"),
        get_home_dir().join(".dsh"),
    )
}

#[cfg(not(test))]
fn settings_override() -> Option<PathBuf> {
    crate::settings::get_dsh_override_dir()
}

#[cfg(test)]
fn settings_override() -> Option<PathBuf> {
    None
}

fn resolve_dsh_home(
    settings_override: Option<PathBuf>,
    env_override: Option<std::ffi::OsString>,
    default_path: PathBuf,
) -> PathBuf {
    if let Some(path) = settings_override {
        return path;
    }
    env_override
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(default_path)
}

pub fn get_credentials_path() -> PathBuf {
    get_dsh_home().join(".credentials.yaml")
}

/// Desktop profile patch, layered before the home patch. Disk persistence
/// does not guarantee a running DSH process has reconciled the edit.
pub fn get_profile_patch_path() -> PathBuf {
    get_profile_dir().join("cordis.patch.yml")
}

pub fn get_profile_dir() -> PathBuf {
    get_dsh_home().join("profiles").join(DESKTOP_PROFILE_NAME)
}

/// Write one provider to the active profile patch entry.
///
/// Unrelated patch entries survive untouched; the managed `llm-deepseek`
/// entry's config is replaced wholesale because CC Switch owns that complete
/// schema namespace.
pub fn set_provider(
    provider_id: &str,
    config: &DeepSeekHarnessProviderConfig,
) -> Result<(), AppError> {
    with_native_transaction(|| set_provider_inner(provider_id, config))
}

fn set_provider_inner(
    provider_id: &str,
    config: &DeepSeekHarnessProviderConfig,
) -> Result<(), AppError> {
    ensure_profile()?;
    import_legacy_settings()?;

    // Emit only schema-known fields: legacy `extra` keys would make DSH's
    // validatePaths reject the whole entry on import.
    let mut section = Mapping::new();
    if let Some(base_url) = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        section.insert(yaml_key("baseURL"), Yaml::String(base_url.to_string()));
    }
    if let Some(thinking) = config
        .thinking
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        section.insert(yaml_key("thinking"), Yaml::String(thinking.to_string()));
    }
    if let Some(effort) = config
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        section.insert(
            yaml_key("reasoningEffort"),
            Yaml::String(effort.to_string()),
        );
    }
    if let Some(max_tokens) = config.max_tokens {
        section.insert(yaml_key("maxTokens"), Yaml::Number(max_tokens.into()));
    }
    if let Some(models) = config
        .models
        .as_ref()
        .filter(|models| models.as_array().is_some_and(|entries| !entries.is_empty()))
    {
        section.insert(yaml_key("models"), json_to_yaml(models)?);
    }
    section.insert(yaml_key("apiKeyEnv"), Yaml::String(API_KEY_REF.to_string()));

    update_patch(|entries| {
        let config = ensure_entry_config(entries, SETTINGS_ENTRY_ID, Some(SETTINGS_ENTRY_NAME));
        *config = section;
        Ok(())
    })?;

    guard_shared_credential(API_KEY_REF, config.api_key.as_deref(), OFFICIAL_PROVIDER_ID)?;
    update_credential_reference(API_KEY_REF, config.api_key.as_deref())?;

    log::debug!("DeepSeek Harness provider '{provider_id}' written to profile patch");
    Ok(())
}

/// Disable the official route with a persistent empty override, removing its
/// credential only when no surviving route (including disabled routes) shares it.
pub fn remove_provider() -> Result<(), AppError> {
    with_native_transaction(remove_provider_inner)
}

fn remove_provider_inner() -> Result<(), AppError> {
    import_legacy_settings()?;
    update_patch(|entries| {
        remove_entry_config(entries, SETTINGS_ENTRY_ID);
        Ok(())
    })?;

    // Custom routes may legally share DEEPSEEK_API_KEY; keep the credential
    // alive when a surviving route still points at it.
    let still_referenced = read_native_state()?.providers.values().any(|provider| {
        provider.config.get("apiKeyEnv").and_then(Value::as_str) == Some(API_KEY_REF)
    });
    if !still_referenced && guard_shared_credential(API_KEY_REF, None, OFFICIAL_PROVIDER_ID).is_ok()
    {
        update_credential_reference(API_KEY_REF, None)?;
    }
    Ok(())
}

pub fn provider_exists_in_live_config() -> Result<bool, AppError> {
    Ok(read_native_state()?
        .providers
        .contains_key(OFFICIAL_PROVIDER_ID))
}

pub fn read_native_state() -> Result<NativeState, AppError> {
    let effective = effective_sections(None)?;
    let mut deepseek_section = effective.get(SETTINGS_ENTRY_ID).cloned();
    let mut pi_ai_section = effective.get(PI_AI_ENTRY_ID).cloned();
    let mut default_model_section = effective.get(DEFAULT_MODEL_ENTRY_ID).cloned();

    // The legacy settings.yaml is imported into the active profile once per
    // DSH process. Until that import runs, mirror it on top of the patch
    // values (DSH layers the imported section over the current config).
    for (ns, values) in read_legacy_sections()? {
        let target = match ns.as_str() {
            SETTINGS_ENTRY_ID => &mut deepseek_section,
            PI_AI_ENTRY_ID => &mut pi_ai_section,
            DEFAULT_MODEL_ENTRY_ID => &mut default_model_section,
            _ => continue,
        };
        let mut values = values;
        if ns == SETTINGS_ENTRY_ID {
            if let Some(map) = values.as_mapping_mut() {
                for field in LEGACY_INVALID_FIELDS {
                    map.remove(yaml_key(field));
                }
            }
        }
        if let Some(values) = values.as_mapping() {
            merge_mapping(target.get_or_insert_with(Mapping::new), values);
        }
    }

    let credentials = read_credential_references()?;
    let mut state = NativeState::default();

    if let Some(section) = deepseek_section.as_ref().filter(|s| !s.is_empty()) {
        let config = with_resolved_api_key(yaml_to_json(section)?, &credentials);
        state.providers.insert(
            OFFICIAL_PROVIDER_ID.to_string(),
            NativeProvider {
                name: "DeepSeek".to_string(),
                source: NativeProviderSource::DeepSeek,
                config,
            },
        );
    }

    if let Some(providers) = pi_ai_section
        .as_ref()
        .and_then(|section| section.get(yaml_key("providers")))
        .and_then(Yaml::as_mapping)
    {
        for (id, config) in providers {
            let Some(id) = id.as_str() else { continue };
            let Some(config_map) = config.as_mapping() else {
                continue;
            };
            let config = with_resolved_api_key(yaml_to_json(config_map)?, &credentials);
            let name = config
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(id)
                .to_string();
            state.providers.insert(
                id.to_string(),
                NativeProvider {
                    name,
                    source: NativeProviderSource::PiAi,
                    config,
                },
            );
        }
    }

    if let Some(default_model) = default_model_section.as_ref() {
        state.current_provider = mapping_string(default_model, "provider");
        state.current_model = mapping_string(default_model, "model");
    }
    Ok(state)
}

pub fn set_pi_ai_provider(provider_id: &str, config: &Value) -> Result<(), AppError> {
    with_native_transaction(|| set_pi_ai_provider_inner(provider_id, config))
}

fn set_pi_ai_provider_inner(provider_id: &str, config: &Value) -> Result<(), AppError> {
    validate_route_id(provider_id)?;
    let mut config = config.as_object().cloned().ok_or_else(|| {
        AppError::InvalidInput("DeepSeek Harness provider must be an object".to_string())
    })?;
    let api_key = config
        .remove(API_KEY_FIELD)
        .and_then(|value| value.as_str().map(ToOwned::to_owned));
    let credential_ref = config
        .get("apiKeyEnv")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    if let Some(reference) = credential_ref.as_deref() {
        validate_credential_ref(reference)?;
        guard_shared_credential(reference, api_key.as_deref(), provider_id)?;
        update_credential_reference(reference, api_key.as_deref())?;
    }
    let section = json_to_yaml(&Value::Object(config))?;
    ensure_profile()?;
    import_legacy_settings()?;
    let inherited = effective_sections(None)?
        .get(PI_AI_ENTRY_ID)
        .cloned()
        .unwrap_or_default();
    update_patch(|entries| {
        let config = ensure_entry_config(entries, PI_AI_ENTRY_ID, Some(PI_AI_ENTRY_NAME));
        *config = inherited;
        let providers = ensure_child_mapping(config, "providers");
        providers.insert(yaml_key(provider_id), section);
        Ok(())
    })
}

pub fn remove_pi_ai_provider(provider_id: &str) -> Result<(), AppError> {
    with_native_transaction(|| remove_pi_ai_provider_inner(provider_id))
}

fn remove_pi_ai_provider_inner(provider_id: &str) -> Result<(), AppError> {
    validate_route_id(provider_id)?;
    import_legacy_settings()?;
    let credential_ref = read_native_state()?
        .providers
        .get(provider_id)
        .and_then(|provider| provider.config.get("apiKeyEnv"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let effective = effective_sections(None)?;
    let mut section = effective.get(PI_AI_ENTRY_ID).cloned().unwrap_or_default();
    if let Some(providers) = section
        .get_mut(yaml_key("providers"))
        .and_then(Yaml::as_mapping_mut)
    {
        providers.remove(yaml_key(provider_id));
    }
    // Persist an explicit empty providers object; dropping config resurrects bundles.
    section
        .entry(yaml_key("providers"))
        .or_insert(Yaml::Mapping(Mapping::new()));
    update_patch(|entries| {
        *ensure_entry_config(entries, PI_AI_ENTRY_ID, Some(PI_AI_ENTRY_NAME)) = section;
        Ok(())
    })?;
    if let Some(reference) = credential_ref {
        // Custom routes may share one credential ref (including DEEPSEEK_API_KEY):
        // only clear it when no surviving route still points at it.
        let still_referenced = read_native_state()?.providers.values().any(|provider| {
            provider.config.get("apiKeyEnv").and_then(Value::as_str) == Some(reference.as_str())
        });
        if !still_referenced && guard_shared_credential(&reference, None, provider_id).is_ok() {
            update_credential_reference(&reference, None)?;
        }
    }
    Ok(())
}

pub fn set_current_model(provider_id: &str, model_id: &str) -> Result<(), AppError> {
    with_native_transaction(|| set_current_model_inner(provider_id, model_id))
}

fn set_current_model_inner(provider_id: &str, model_id: &str) -> Result<(), AppError> {
    validate_route_id(provider_id)?;
    if model_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness model id cannot be empty".to_string(),
        ));
    }
    ensure_profile()?;
    import_legacy_settings()?;
    update_patch(|entries| {
        let config = ensure_entry_config(
            entries,
            DEFAULT_MODEL_ENTRY_ID,
            Some(DEFAULT_MODEL_ENTRY_NAME),
        );
        config.insert(yaml_key("provider"), Yaml::String(provider_id.to_string()));
        config.insert(yaml_key("model"), Yaml::String(model_id.to_string()));
        Ok(())
    })
}

/// Drop `agent-default-model` when it still points at `provider_id`.
///
/// The provider/model pair is coupled in DSH: keeping the namespace after the
/// referenced route is deleted would leave the Harness runtime targeting a
/// provider that no longer exists. Returns whether the pointer was cleared.
pub fn clear_current_model_if_provider(provider_id: &str) -> Result<bool, AppError> {
    with_native_transaction(|| clear_current_model_if_provider_inner(provider_id))
}

fn clear_current_model_if_provider_inner(provider_id: &str) -> Result<bool, AppError> {
    import_legacy_settings()?;
    let effective = effective_sections(None)?;
    let points_at_provider = effective
        .get(DEFAULT_MODEL_ENTRY_ID)
        .and_then(|config| mapping_string(config, "provider"))
        .is_some_and(|provider| provider == provider_id);
    if !points_at_provider {
        return Ok(false);
    }
    update_patch(|entries| {
        remove_entry_config(entries, DEFAULT_MODEL_ENTRY_ID);
        Ok(())
    })?;
    if effective_sections(None)?.contains_key(DEFAULT_MODEL_ENTRY_ID) {
        return Err(unsupported(
            "default model clear is overridden by the home patch",
        ));
    }
    Ok(true)
}

// ── Profile patch layer ───────────────────────────────────────────────

fn managed_ids() -> [&'static str; 3] {
    [SETTINGS_ENTRY_ID, PI_AI_ENTRY_ID, DEFAULT_MODEL_ENTRY_ID]
}
fn managed_name(id: &str) -> Option<&'static str> {
    match id {
        SETTINGS_ENTRY_ID => Some(SETTINGS_ENTRY_NAME),
        PI_AI_ENTRY_ID => Some(PI_AI_ENTRY_NAME),
        DEFAULT_MODEL_ENTRY_ID => Some(DEFAULT_MODEL_ENTRY_NAME),
        _ => None,
    }
}
fn unsupported(message: &str) -> AppError {
    AppError::Config(format!(
        "Unsupported Desktop configuration: {message}; edit it in DSH instead"
    ))
}
fn contains_expression(value: &Yaml) -> bool {
    match value {
        Yaml::Tagged(_) => true,
        Yaml::Sequence(values) => values.iter().any(contains_expression),
        Yaml::Mapping(values) => values
            .iter()
            .any(|(k, v)| k.as_str() == Some("__jsExpr") || contains_expression(v)),
        _ => false,
    }
}

// A bounded projection of three namespaces, NOT a runtime loader. Overrides
// replace config wholesale; !!js is never evaluated.
fn project_patches(
    rows: &mut HashMap<String, Mapping>,
    patches: &[Yaml],
    inserting: bool,
) -> Result<(), AppError> {
    for patch in patches {
        let map = patch
            .as_mapping()
            .ok_or_else(|| unsupported("nonobject entry"))?;
        if let Some(insert) = map.get(yaml_key("insert")) {
            if map.contains_key(yaml_key("id")) {
                return Err(unsupported("group-targeted insert"));
            }
            project_patches(
                rows,
                insert
                    .as_sequence()
                    .ok_or_else(|| unsupported("nonliteral insert"))?,
                true,
            )?;
            continue;
        }
        if inserting && map.get(yaml_key("group")).and_then(Yaml::as_bool) == Some(true) {
            if map
                .get(yaml_key("disabled"))
                .is_some_and(|v| v != &Yaml::Bool(false))
            {
                return Err(unsupported("conditional bundle group"));
            }
            project_patches(
                rows,
                map.get(yaml_key("config"))
                    .and_then(Yaml::as_sequence)
                    .ok_or_else(|| unsupported("nonliteral group"))?,
                true,
            )?;
            continue;
        }
        let id = map
            .get(yaml_key("id"))
            .and_then(Yaml::as_str)
            .ok_or_else(|| unsupported("nonliteral/missing id"))?;
        let Some(name) = managed_name(id) else {
            continue;
        };
        if contains_expression(patch) {
            return Err(unsupported("expression in managed entry"));
        }
        if map
            .get(yaml_key("name"))
            .and_then(Yaml::as_str)
            .is_some_and(|n| n != name)
        {
            continue;
        }
        if inserting {
            if rows.insert(id.to_string(), map.clone()).is_some() {
                return Err(unsupported("duplicate managed id"));
            }
        } else {
            let target = rows
                .get_mut(id)
                .ok_or_else(|| unsupported("managed target not in bundle"))?;
            for (k, v) in map {
                if !matches!(k.as_str(), Some("id" | "name")) {
                    target.insert(k.clone(), v.clone());
                }
            }
        }
    }
    Ok(())
}

fn base_rows() -> Result<HashMap<String, Mapping>, AppError> {
    let dir = get_profile_dir();
    let path = dir.join("package.json");
    let manifest = if path.exists() {
        Some(
            serde_json::from_str::<Value>(
                &fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?,
            )
            .map_err(|_| unsupported("invalid profile manifest"))?,
        )
    } else {
        None
    };
    let mut rows = HashMap::new();
    let bundles = manifest
        .as_ref()
        .and_then(|m| m.pointer("/dsh/profile/bundles"));
    // Verified against the shipped rc.2 patch files. These optional layers
    // only add review/team/inspector plugins, never the managed LLM/default rows.
    // Unknown layers still fail closed; readable local packages are composed
    // below rather than ignored in favor of this installation fallback.
    const DESKTOP_BUNDLES: [&str; 5] = [
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-web-app",
        "@deepseek-ai/dsh-experimental-auto-review",
        "@deepseek-ai/dsh-experimental-agent-team-profile",
        "@deepseek-ai/dsh-experimental-inspector-profile",
    ];
    let standard_bundles = bundles.and_then(Value::as_array).is_some_and(|list| {
        list.len() >= 2
            && list[0].as_str() == Some(DESKTOP_BUNDLES[0])
            && list[1].as_str() == Some(DESKTOP_BUNDLES[1])
            && list.iter().all(|name| {
                name.as_str()
                    .is_some_and(|name| DESKTOP_BUNDLES.contains(&name))
            })
    });
    let standard_local = bundles.and_then(Value::as_array).is_some_and(|list| {
        list.iter().filter_map(Value::as_str).any(|name| {
            dir.join("node_modules")
                .join(name)
                .join("package.json")
                .exists()
        })
    });
    if manifest.is_none() || (standard_bundles && !standard_local) {
        // Verified rc.2 standard Desktop projection. No executable bundle code,
        // schema defaults or credential/environment values are inferred.
        for id in managed_ids() {
            let mut row = Mapping::new();
            row.insert(yaml_key("name"), yaml_key(managed_name(id).unwrap()));
            rows.insert(id.to_string(), row);
        }
        if manifest.is_some() {
            rows.get_mut(DEFAULT_MODEL_ENTRY_ID).unwrap().insert(
                yaml_key("config"),
                json_to_yaml(
                    &serde_json::json!({"provider": OFFICIAL_PROVIDER_ID,"model":"deepseek-flash"}),
                )?,
            );
        }
        return Ok(rows);
    }
    for bundle in bundles
        .and_then(Value::as_array)
        .ok_or_else(|| unsupported("bundle list"))?
    {
        let name = bundle.as_str().ok_or_else(|| unsupported("bundle name"))?;
        if name.contains("..") || name.starts_with('/') {
            return Err(unsupported("bundle name"));
        }
        let package = dir.join("node_modules").join(name);
        let path = package.join("package.json");
        let text = fs::read_to_string(&path).map_err(|_| {
            unsupported("bundle not locally readable; asar/installation resolution unavailable")
        })?;
        let data: Value =
            serde_json::from_str(&text).map_err(|_| unsupported("bundle manifest"))?;
        if data.get("engines").is_some() || data.pointer("/dsh/requires").is_some() {
            return Err(unsupported("bundle compatibility requirements"));
        }
        let patch = data
            .pointer("/dsh/bundle/patch")
            .ok_or_else(|| unsupported("bundle patch path"))?;
        let paths = match patch {
            Value::String(_) => vec![patch],
            Value::Array(paths) => paths.iter().collect(),
            _ => return Err(unsupported("bundle patch path")),
        };
        for path in paths {
            let path = path
                .as_str()
                .ok_or_else(|| unsupported("bundle patch path"))?;
            if Path::new(path).is_absolute()
                || Path::new(path)
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(unsupported("bundle patch escapes package"));
            }
            let path = package.join(path);
            if !path.is_file() {
                return Err(unsupported("missing bundle patch"));
            }
            project_patches(&mut rows, &load_patch_entries(&path)?, false)?;
        }
    }
    Ok(rows)
}
fn composed_sections(entries: &[Yaml], home: bool) -> Result<HashMap<String, Mapping>, AppError> {
    let mut rows = base_rows()?;
    project_patches(&mut rows, entries, false)?;
    if home {
        project_patches(
            &mut rows,
            &load_patch_entries(&get_dsh_home().join("cordis.patch.yml"))?,
            false,
        )?;
    }
    let mut sections = HashMap::new();
    for (id, row) in rows {
        match row.get(yaml_key("disabled")) {
            Some(Yaml::Bool(true)) => continue,
            None | Some(Yaml::Bool(false)) => {}
            _ => return Err(unsupported("nonboolean disabled")),
        }
        if let Some(config) = row.get(yaml_key("config")) {
            sections.insert(
                id,
                config
                    .as_mapping()
                    .ok_or_else(|| unsupported("managed config is not an object"))?
                    .clone(),
            );
        }
    }
    Ok(sections)
}
fn profile_sections(entries: &[Yaml]) -> Result<HashMap<String, Mapping>, AppError> {
    composed_sections(entries, false)
}
fn effective_sections(entries: Option<&[Yaml]>) -> Result<HashMap<String, Mapping>, AppError> {
    match entries {
        Some(entries) => composed_sections(entries, true),
        None => composed_sections(&load_patch_entries(&get_profile_patch_path())?, true),
    }
}

// Preflight both documents before the first write; compensate any subsequent
// failure. Does not provide a cross-process lock or runtime reconciliation.
pub(crate) fn with_native_transaction<T>(
    change: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    effective_sections(None)?;
    read_credential_references()?;
    read_legacy_sections()?;
    let home = get_dsh_home();
    let paths = [
        get_profile_patch_path(),
        get_credentials_path(),
        home.join("settings.yaml"),
        home.join("settings.yaml.imported"),
    ];
    let snapshots = paths
        .iter()
        .map(|p| {
            if p.exists() {
                fs::read(p).map(Some).map_err(|e| AppError::io(p, e))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let result = change();
    if result.is_err() {
        for (path, before) in paths.iter().zip(snapshots) {
            let current = if path.exists() {
                Some(fs::read(path).map_err(|e| AppError::io(path, e))?)
            } else {
                None
            };
            if current == before {
                continue;
            }
            let restore = match before {
                Some(bytes) => atomic_write_private(path, &bytes),
                None => {
                    if path.exists() {
                        fs::remove_file(path).map_err(|e| AppError::io(path, e))
                    } else {
                        Ok(())
                    }
                }
            };
            if restore.is_err() {
                return Err(AppError::Config("Desktop write and rollback failed; inspect profile and credentials before retrying".into()));
            }
        }
    }
    result
}

fn yaml_key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

fn update_patch(
    update: impl FnOnce(&mut Vec<Yaml>) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let patch_path = get_profile_patch_path();
    let mut entries = load_patch_entries(&patch_path)?;
    update(&mut entries)?;
    let intended = profile_sections(&entries)?;
    let effective = effective_sections(Some(&entries))?;
    // Conservatively reject any managed home conflict, even an unrelated one.
    // Comparing only changed values misses a repeated request whose profile
    // value already matches, but whose effective home-overridden value does not.
    for id in managed_ids() {
        if intended.get(id) != effective.get(id) {
            return Err(AppError::Config(format!("Desktop configuration for {id} is overridden by the home patch; edit it in DSH instead")));
        }
    }
    let document = serde_yaml::to_string(&entries).map_err(|error| {
        AppError::Config(format!("Serialize {}: {error}", patch_path.display()))
    })?;
    atomic_write_private(&patch_path, document.as_bytes())
}

fn load_patch_entries(path: &Path) -> Result<Vec<Yaml>, AppError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_yaml::from_str::<Yaml>(&contents) {
        Ok(Yaml::Sequence(entries)) => Ok(entries),
        Ok(_) => Err(AppError::Config(format!(
            "{} must contain a YAML sequence of patch entries",
            path.display()
        ))),
        Err(error) => Err(AppError::Config(format!(
            "Parse {}: {error}",
            path.display()
        ))),
    }
}

fn matches_entry(entry: &Yaml, id: &str) -> bool {
    let Some(map) = entry.as_mapping() else {
        return false;
    };
    if map.get(yaml_key("insert")).is_some() {
        return false;
    }
    map.get(yaml_key("id")).and_then(Yaml::as_str) == Some(id)
        && map
            .get(yaml_key("name"))
            .and_then(Yaml::as_str)
            .is_none_or(|name| Some(name) == managed_name(id))
}

/// Locate (or append) the patch entry for `id` and return its `config`
/// mapping, creating an empty config when missing. Mirrors the loader's
/// match rule: `id` equal, no `insert`, and no conflicting `name`.
fn ensure_entry_config<'a>(
    entries: &'a mut Vec<Yaml>,
    id: &str,
    name: Option<&str>,
) -> &'a mut Mapping {
    let index = match entries.iter().rposition(|entry| {
        matches_entry(entry, id)
            && name.is_none_or(|name| {
                entry
                    .get("name")
                    .and_then(Yaml::as_str)
                    .is_none_or(|existing| existing == name)
            })
    }) {
        Some(index) => index,
        None => {
            let mut map = Mapping::new();
            map.insert(yaml_key("id"), Yaml::String(id.to_string()));
            if let Some(name) = name {
                map.insert(yaml_key("name"), Yaml::String(name.to_string()));
            }
            map.insert(yaml_key("config"), Yaml::Mapping(Mapping::new()));
            entries.push(Yaml::Mapping(map));
            entries.len() - 1
        }
    };
    let entry = entries[index].as_mapping_mut().expect("entry is a mapping");
    entry.insert(yaml_key("disabled"), Yaml::Bool(false));
    if !entry.get(yaml_key("config")).is_some_and(Yaml::is_mapping) {
        entry.insert(yaml_key("config"), Yaml::Mapping(Mapping::new()));
    }
    entry
        .get_mut(yaml_key("config"))
        .and_then(Yaml::as_mapping_mut)
        .expect("config was normalized")
}

/// Keep a disabled, empty override so a lower bundle/earlier same-id config
/// cannot reappear. A subsequent set explicitly re-enables the target.
fn remove_entry_config(entries: &mut Vec<Yaml>, id: &str) {
    let config = ensure_entry_config(entries, id, managed_name(id));
    config.clear();
    let entry = entries
        .iter_mut()
        .rev()
        .find(|entry| matches_entry(entry, id))
        .unwrap()
        .as_mapping_mut()
        .unwrap();
    entry.insert(yaml_key("disabled"), Yaml::Bool(true));
}

fn ensure_child_mapping<'a>(parent: &'a mut Mapping, key: &str) -> &'a mut Mapping {
    if !parent.get(yaml_key(key)).is_some_and(Yaml::is_mapping) {
        parent.insert(yaml_key(key), Yaml::Mapping(Mapping::new()));
    }
    parent
        .get_mut(yaml_key(key))
        .and_then(Yaml::as_mapping_mut)
        .expect("child was normalized")
}

/// DSH's `mergeLayers`: objects merge recursively, everything else replaces.
fn merge_mapping(under: &mut Mapping, over: &Mapping) {
    for (key, value) in over {
        match (under.get_mut(key), value) {
            (Some(Yaml::Mapping(under_child)), Yaml::Mapping(over_child)) => {
                merge_mapping(under_child, over_child);
            }
            _ => {
                under.insert(key.clone(), value.clone());
            }
        }
    }
}

// ── Legacy settings.yaml migration ────────────────────────────────────

/// Validate the complete legacy document and persist its patch changes before
/// retiring the source. The enclosing native transaction restores both files on
/// failure, so a failed migration cannot discard the original legacy document.
fn import_legacy_settings() -> Result<(), AppError> {
    let home = get_dsh_home();
    let path = home.join("settings.yaml");
    if !path.exists() {
        return Ok(());
    }
    let contents = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    let imported_path = home.join("settings.yaml.imported");
    let sections = match serde_yaml::from_str::<Yaml>(&contents) {
        Ok(Yaml::Mapping(sections)) => sections,
        _ => return Err(unsupported("legacy settings must be a valid object")),
    };
    for (key, value) in &sections {
        if key.as_str().is_none() || !value.is_mapping() || contains_expression(value) {
            return Err(unsupported("legacy section must be a literal object"));
        }
    }
    if imported_path.exists() {
        return Err(unsupported(
            "settings.yaml.imported already exists; preserve both originals and migrate in DSH",
        ));
    }
    let mut next = load_patch_entries(&get_profile_patch_path())?;
    for (section, values) in &sections {
        let Some(section_name) = section.as_str() else {
            continue;
        };
        let ns = legacy_section_ns(section_name);
        let mut values = values.clone();
        if ns == SETTINGS_ENTRY_ID {
            if let Some(map) = values.as_mapping_mut() {
                for field in LEGACY_INVALID_FIELDS {
                    map.remove(yaml_key(field));
                }
            }
        }
        let Some(values_map) = values.as_mapping().filter(|map| !map.is_empty()) else {
            continue;
        };
        let name = match ns {
            SETTINGS_ENTRY_ID => Some(SETTINGS_ENTRY_NAME),
            PI_AI_ENTRY_ID => Some(PI_AI_ENTRY_NAME),
            DEFAULT_MODEL_ENTRY_ID => Some(DEFAULT_MODEL_ENTRY_NAME),
            _ => None,
        };
        let config = ensure_entry_config(&mut next, ns, name);
        merge_mapping(config, values_map);
    }
    // Validate and persist the complete migration before retiring the source.
    update_patch(|entries| {
        *entries = next;
        Ok(())
    })?;
    fs::rename(&path, &imported_path).map_err(|e| AppError::io(&path, e))?;
    log::info!(
        "DSH: imported {} into the active profile patch",
        imported_path.display()
    );
    Ok(())
}

/// `SettingsForms.LEGACY_SECTION_ENTRIES`: removed settings.yaml sections
/// whose entry now lives under another id.
fn legacy_section_ns(section: &str) -> &str {
    match section {
        "ui-developer-tools" => "ui-settings",
        "ui-onboarding" => "ui-settings-general",
        "shell" => {
            if cfg!(windows) {
                "pwsh-sandbox"
            } else {
                "bash-sandbox"
            }
        }
        other => other,
    }
}

fn read_legacy_sections() -> Result<Vec<(String, Yaml)>, AppError> {
    let path = get_dsh_home().join("settings.yaml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    match serde_yaml::from_str::<Yaml>(&contents) {
        Ok(Yaml::Mapping(sections)) => Ok(sections
            .into_iter()
            .filter_map(|(section, values)| {
                let ns = legacy_section_ns(section.as_str()?).to_string();
                Some((ns, values))
            })
            .collect()),
        Ok(_) => Err(unsupported("legacy settings must be an object")),
        Err(error) => Err(AppError::Config(format!(
            "Parse {}: {error}",
            path.display()
        ))),
    }
}

// ── Credentials ───────────────────────────────────────────────────────

fn with_resolved_api_key(mut config: Value, credentials: &HashMap<String, String>) -> Value {
    let reference = config
        .get("apiKeyEnv")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    if let (Some(reference), Some(object)) = (reference, config.as_object_mut()) {
        if let Some(api_key) = credentials.get(&reference) {
            object.insert(API_KEY_FIELD.to_string(), Value::String(api_key.clone()));
        }
    }
    config
}

fn guard_shared_credential(
    reference: &str,
    next: Option<&str>,
    owner: &str,
) -> Result<(), AppError> {
    let current = read_credential_references()?;
    let next = next.map(str::trim).filter(|v| !v.is_empty());
    if current.get(reference).map(String::as_str) == next {
        return Ok(());
    }
    // Include disabled namespace configs too: disabled is not relinquishment of
    // ownership, and re-enabling must not silently receive another route's key.
    let mut entries = load_patch_entries(&get_profile_patch_path())?;
    entries.extend(load_patch_entries(
        &get_dsh_home().join("cordis.patch.yml"),
    )?);
    for entry in entries.iter_mut() {
        if let Some(map) = entry.as_mapping_mut() {
            map.remove(yaml_key("disabled"));
        }
    }
    let sections = profile_sections(&entries)?;
    if owner != OFFICIAL_PROVIDER_ID
        && sections
            .get(SETTINGS_ENTRY_ID)
            .and_then(|s| mapping_string(s, "apiKeyEnv"))
            .as_deref()
            == Some(reference)
    {
        return Err(unsupported(
            "credential reference is shared with another route",
        ));
    }
    if let Some(providers) = sections
        .get(PI_AI_ENTRY_ID)
        .and_then(|s| s.get(yaml_key("providers")))
        .and_then(Yaml::as_mapping)
    {
        for (id, config) in providers {
            if id.as_str() != Some(owner)
                && config.get("apiKeyEnv").and_then(Yaml::as_str) == Some(reference)
            {
                return Err(unsupported(
                    "credential reference is shared with another route",
                ));
            }
        }
    }
    Ok(())
}

/// Reserved references, including currently unused credentials (never values).
pub fn credential_reference_names() -> Result<Vec<String>, AppError> {
    let mut names: Vec<_> = read_credential_references()?.into_keys().collect();
    names.sort();
    Ok(names)
}

fn validate_credentials(document: &Yaml) -> Result<(), AppError> {
    let root = document
        .as_mapping()
        .ok_or_else(|| unsupported("credentials must be an object"))?;
    let refs = if let Some(version) = root.get(yaml_key("version")) {
        if version.as_i64() != Some(1) {
            return Err(unsupported("credentials version"));
        }
        if root
            .get(yaml_key("records"))
            .is_some_and(|v| !v.is_mapping())
        {
            return Err(unsupported("credential records must be an object"));
        }
        root.get(yaml_key("refs"))
            .and_then(Yaml::as_mapping)
            .ok_or_else(|| unsupported("credential refs must be an object"))?
    } else {
        root
    };
    for (key, value) in refs {
        if key.as_str().is_none() || value.as_str().is_none() {
            return Err(unsupported("credential references must contain strings"));
        }
    }
    Ok(())
}

fn read_credential_references() -> Result<HashMap<String, String>, AppError> {
    let path = get_credentials_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let document = parse_yaml_document(&path)?;
    validate_credentials(&document)?;
    let root = document.as_mapping().unwrap();
    let refs = if root.get(yaml_key("version")).is_some() {
        root.get(yaml_key("refs")).and_then(Yaml::as_mapping)
    } else {
        Some(root)
    };
    Ok(refs
        .into_iter()
        .flat_map(|refs| refs.iter())
        .filter_map(|(key, value)| Some((key.as_str()?.to_string(), value.as_str()?.to_string())))
        .collect())
}

fn update_credential_reference(reference: &str, value: Option<&str>) -> Result<(), AppError> {
    validate_credential_ref(reference)?;
    let path = get_credentials_path();
    if value.map(str::trim).unwrap_or("").is_empty() && !path.exists() {
        return Ok(());
    }
    let mut document = parse_yaml_document(&path)?;
    validate_credentials(&document)?;
    let root = match &mut document {
        Yaml::Mapping(map) => map,
        _ => {
            document = Yaml::Mapping(Mapping::new());
            document.as_mapping_mut().expect("just normalized")
        }
    };
    if !root.contains_key(yaml_key("version")) {
        let legacy = root.clone();
        root.clear();
        root.insert(yaml_key("version"), Yaml::Number(1.into()));
        root.insert(yaml_key("refs"), Yaml::Mapping(legacy));
    }
    let refs = ensure_child_mapping(root, "refs");
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            refs.insert(yaml_key(reference), Yaml::String(value.to_string()));
        }
        None => {
            refs.remove(yaml_key(reference));
        }
    }
    let output = serde_yaml::to_string(&document)
        .map_err(|error| AppError::Config(format!("Serialize {}: {error}", path.display())))?;
    atomic_write_private(&path, output.as_bytes())
}

fn parse_yaml_document(path: &Path) -> Result<Yaml, AppError> {
    if !path.exists() {
        return Ok(Yaml::Mapping(Mapping::new()));
    }
    let contents = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    serde_yaml::from_str::<Yaml>(&contents)
        .map_err(|error| AppError::Config(format!("Parse {}: {error}", path.display())))
}

// ── Conversions & validation ──────────────────────────────────────────

fn yaml_to_json(mapping: &Mapping) -> Result<Value, AppError> {
    serde_json::to_value(Yaml::Mapping(mapping.clone()))
        .map_err(|error| AppError::Config(format!("Convert DSH YAML to JSON: {error}")))
}

fn json_to_yaml(value: &Value) -> Result<Yaml, AppError> {
    serde_yaml::to_value(value)
        .map_err(|error| AppError::Config(format!("Convert JSON to DSH YAML: {error}")))
}

fn mapping_string(mapping: &Mapping, key: &str) -> Option<String> {
    mapping
        .get(yaml_key(key))
        .and_then(Yaml::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn validate_route_id(value: &str) -> Result<(), AppError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AppError::InvalidInput(format!(
            "Invalid DeepSeek Harness provider id: {value}"
        )));
    }
    Ok(())
}

fn validate_credential_ref(value: &str) -> Result<(), AppError> {
    if value.is_empty()
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || (index > 0 && byte.is_ascii_digit())
        })
    {
        return Err(AppError::InvalidInput(format!(
            "Invalid DeepSeek Harness credential reference: {value}"
        )));
    }
    Ok(())
}

/// Initialize the desktop profile the way DSH's `initProfile` does, with the
/// base bundle list the Electron app itself writes (`dsh-base` + the web app
/// bundle); existing files are never touched.
fn ensure_profile() -> Result<(), AppError> {
    let profile_dir = get_profile_dir();
    fs::create_dir_all(&profile_dir).map_err(|error| AppError::io(&profile_dir, error))?;

    let manifest_path = profile_dir.join("package.json");
    if !manifest_path.exists() {
        let manifest = serde_json::json!({
            "name": format!("dsh-profile-{DESKTOP_PROFILE_NAME}"),
            "private": true,
            "dependencies": {},
            "dsh": { "profile": {
                "bundles": [
                    "@deepseek-ai/dsh-base",
                    "@deepseek-ai/dsh-web-app",
                ],
            } }
        });
        crate::config::write_json_file(&manifest_path, &manifest)?;
    }

    initialize_if_missing(
        &profile_dir.join("cordis.patch.yml"),
        PROFILE_PATCH_TEMPLATE,
    )?;
    initialize_if_missing(
        &profile_dir.join("pnpm-workspace.yaml"),
        PROFILE_PNPM_WORKSPACE,
    )?;
    Ok(())
}

fn initialize_if_missing(path: &Path, contents: &str) -> Result<(), AppError> {
    if !path.exists() {
        crate::config::write_text_file(path, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_temp_home(test: impl FnOnce(&Path)) {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(directory.path())));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }

    fn patch_path(home: &Path) -> PathBuf {
        home.join("profiles/desktop/cordis.patch.yml")
    }

    #[test]
    #[serial]
    fn audit_regressions_disabled_name_and_reenable() {
        with_temp_home(|home| {
            fs::create_dir_all(home.join("profiles/desktop")).unwrap();
            fs::write(patch_path(home), "- id: llm-deepseek\n  config: {baseURL: old}\n  disabled: true\n- id: llm-deepseek\n  name: wrong-plugin\n  config: {baseURL: wrong}\n").unwrap();
            assert!(!read_native_state()
                .unwrap()
                .providers
                .contains_key(OFFICIAL_PROVIDER_ID));
            set_provider(
                OFFICIAL_PROVIDER_ID,
                &DeepSeekHarnessProviderConfig {
                    base_url: Some("new".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                read_native_state().unwrap().providers[OFFICIAL_PROVIDER_ID].config["baseURL"],
                "new"
            );
        });
    }

    #[test]
    #[serial]
    fn audit_regressions_home_conflict_and_malformed_are_no_write() {
        with_temp_home(|home| {
            set_pi_ai_provider(
                "route",
                &serde_json::json!({"apiKeyEnv":"ROUTE_KEY", "apiKey":"old"}),
            )
            .unwrap();
            let before = fs::read(home.join(".credentials.yaml")).unwrap();
            fs::write(patch_path(home), "not-a-list: true\n").unwrap();
            assert!(set_pi_ai_provider(
                "route",
                &serde_json::json!({"apiKeyEnv":"ROUTE_KEY", "apiKey":"new"})
            )
            .is_err());
            assert_eq!(before, fs::read(home.join(".credentials.yaml")).unwrap());
        });
        with_temp_home(|home| {
            set_current_model("old", "model").unwrap();
            let before = fs::read(patch_path(home)).unwrap();
            fs::write(
                home.join("cordis.patch.yml"),
                "- id: agent-default-model\n  config: {provider: home, model: home}\n",
            )
            .unwrap();
            assert!(set_current_model("new", "new").is_err());
            assert_eq!(before, fs::read(patch_path(home)).unwrap());
        });
    }

    #[test]
    #[serial]
    fn audit_regressions_bad_credentials_and_legacy_preserve_original() {
        with_temp_home(|home| {
            set_current_model("old", "model").unwrap();
            let before = fs::read(patch_path(home)).unwrap();
            fs::write(
                home.join(".credentials.yaml"),
                "version: 1\nrefs: []\nrecords: {keep: yes}\n",
            )
            .unwrap();
            assert!(set_provider(
                OFFICIAL_PROVIDER_ID,
                &DeepSeekHarnessProviderConfig::default()
            )
            .is_err());
            assert_eq!(before, fs::read(patch_path(home)).unwrap());
            fs::write(home.join("settings.yaml"), "bad: [\n").unwrap();
            assert!(import_legacy_settings().is_err());
            assert_eq!(
                fs::read_to_string(home.join("settings.yaml")).unwrap(),
                "bad: [\n"
            );
        });
    }

    #[test]
    #[serial]
    fn local_bundle_delete_and_clear_do_not_resurrect_inherited_values() {
        with_temp_home(|home| {
            let profile = home.join("profiles/desktop");
            let bundle = profile.join("node_modules/local-fixture");
            fs::create_dir_all(&bundle).unwrap();
            fs::write(
                profile.join("package.json"),
                r#"{"dsh":{"profile":{"bundles":["local-fixture"]}}}"#,
            )
            .unwrap();
            fs::write(
                bundle.join("package.json"),
                r#"{"dsh":{"bundle":{"patch":"./patch.yml"}}}"#,
            )
            .unwrap();
            fs::write(bundle.join("patch.yml"), "- insert:\n  - id: llm-pi-ai\n    name: '@deepseek-ai/dsh-llm-pi-ai'\n    config: {providers: {old: {displayName: Old}, sibling: {displayName: Keep}}}\n  - id: agent-default-model\n    name: '@deepseek-ai/dsh-agent-default-model'\n    config: {provider: old, model: inherited}\n").unwrap();
            assert!(read_native_state().unwrap().providers.contains_key("old"));
            set_pi_ai_provider("new", &serde_json::json!({"displayName":"New"})).unwrap();
            assert!(read_native_state()
                .unwrap()
                .providers
                .contains_key("sibling"));
            remove_pi_ai_provider("old").unwrap();
            assert!(!read_native_state().unwrap().providers.contains_key("old"));
            assert!(clear_current_model_if_provider("old").unwrap());
            assert_eq!(read_native_state().unwrap().current_provider, None);
            remove_pi_ai_provider("new").unwrap();
            remove_pi_ai_provider("sibling").unwrap();
            assert!(read_native_state().unwrap().providers.is_empty());
            assert!(fs::read_to_string(patch_path(home))
                .unwrap()
                .contains("providers: {}"));
        });
    }

    #[test]
    #[serial]
    fn expressions_and_unreadable_bundles_fail_closed_without_writes() {
        with_temp_home(|home| {
            fs::create_dir_all(home.join("profiles/desktop")).unwrap();
            fs::write(
                patch_path(home),
                "- id: llm-pi-ai\n  disabled: !!js process.exit()\n",
            )
            .unwrap();
            let before = fs::read(patch_path(home)).unwrap();
            assert!(set_pi_ai_provider("new", &serde_json::json!({})).is_err());
            assert_eq!(before, fs::read(patch_path(home)).unwrap());
            fs::write(patch_path(home), "[]\n").unwrap();
            fs::write(
                home.join("profiles/desktop/package.json"),
                r#"{"dsh":{"profile":{"bundles":["unreadable"]}}}"#,
            )
            .unwrap();
            assert!(read_native_state().is_err());
        });
    }

    #[test]
    #[serial]
    fn shared_key_conflict_and_outer_transaction_roll_back() {
        with_temp_home(|home| {
            set_pi_ai_provider(
                "first",
                &serde_json::json!({"apiKeyEnv":"SHARED", "apiKey":"same"}),
            )
            .unwrap();
            set_pi_ai_provider(
                "second",
                &serde_json::json!({"apiKeyEnv":"SHARED", "apiKey":"same"}),
            )
            .unwrap();
            let patch = fs::read(patch_path(home)).unwrap();
            let credentials = fs::read(home.join(".credentials.yaml")).unwrap();
            assert!(set_pi_ai_provider(
                "second",
                &serde_json::json!({"apiKeyEnv":"SHARED", "apiKey":"different"})
            )
            .is_err());
            fs::write(
                home.join("cordis.patch.yml"),
                "- id: agent-default-model\n  config: {provider: home, model: home}\n",
            )
            .unwrap();
            let result = with_native_transaction(|| {
                set_pi_ai_provider(
                    "third",
                    &serde_json::json!({"apiKeyEnv":"THIRD", "apiKey":"temporary"}),
                )?;
                set_current_model("third", "model")
            });
            assert!(result.is_err());
            assert_eq!(patch, fs::read(patch_path(home)).unwrap());
            assert_eq!(
                credentials,
                fs::read(home.join(".credentials.yaml")).unwrap()
            );
        });
    }

    #[test]
    #[serial]
    fn repeated_profile_value_does_not_bypass_home_conflict() {
        with_temp_home(|home| {
            set_current_model("profile", "profile-model").unwrap();
            let before = fs::read(patch_path(home)).unwrap();
            fs::write(
                home.join("cordis.patch.yml"),
                "- id: agent-default-model\n  config: {provider: home, model: home-model}\n",
            )
            .unwrap();
            assert!(set_current_model("profile", "profile-model").is_err());
            assert_eq!(before, fs::read(patch_path(home)).unwrap());
        });
    }

    #[test]
    fn dsh_home_prefers_settings_override_then_env_then_default() {
        let override_dir = PathBuf::from("/tmp/dsh-override");
        let env_dir = PathBuf::from("/tmp/dsh-env");
        let default_dir = PathBuf::from("/tmp/dsh-default");
        assert_eq!(
            resolve_dsh_home(
                Some(override_dir.clone()),
                Some(env_dir.clone().into_os_string()),
                default_dir.clone()
            ),
            override_dir
        );
        assert_eq!(
            resolve_dsh_home(
                None,
                Some(env_dir.clone().into_os_string()),
                default_dir.clone()
            ),
            env_dir
        );
        assert_eq!(
            resolve_dsh_home(None, Some("".into()), default_dir.clone()),
            default_dir
        );
        assert_eq!(
            resolve_dsh_home(None, None, default_dir.clone()),
            default_dir
        );
    }

    #[test]
    #[serial]
    fn writes_official_route_into_desktop_profile_patch() {
        with_temp_home(|home| {
            let config = DeepSeekHarnessProviderConfig {
                api_key: Some("sk-test".to_string()),
                base_url: Some("https://example.com".to_string()),
                models: Some(serde_json::json!([
                    { "id": "deepseek-v4-flash", "name": "Flash" }
                ])),
                ..Default::default()
            };
            set_provider("official", &config).unwrap();

            let patch = std::fs::read_to_string(patch_path(home)).unwrap();
            assert!(patch.contains("- id: llm-deepseek"));
            assert!(patch.contains("name: '@deepseek-ai/dsh-llm-deepseek-api-key'"));
            assert!(patch.contains("baseURL: https://example.com"));
            assert!(patch.contains("apiKeyEnv: DEEPSEEK_API_KEY"));
            assert!(!patch.contains("sk-test"));
            assert!(!patch.contains("profile"));

            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(credentials.contains("DEEPSEEK_API_KEY"));
            assert!(home.join("profiles/desktop/package.json").exists());
            assert!(home.join("profiles/desktop/pnpm-workspace.yaml").exists());
        });
    }

    #[test]
    #[serial]
    fn ensure_profile_bundle_list_matches_the_desktop_app() {
        with_temp_home(|home| {
            ensure_profile().unwrap();
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(home.join("profiles/desktop/package.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                manifest["dsh"]["profile"]["bundles"],
                serde_json::json!(["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"])
            );
            assert!(manifest["dsh"]["profile"].get("patchReload").is_none());
            // Re-running must never rewrite an initialized profile.
            let before = std::fs::read_to_string(patch_path(home)).unwrap();
            ensure_profile().unwrap();
            assert_eq!(before, std::fs::read_to_string(patch_path(home)).unwrap());
        });
    }

    #[test]
    #[serial]
    fn preserves_unrelated_patch_entries_and_disables() {
        with_temp_home(|home| {
            std::fs::create_dir_all(home.join("profiles/desktop")).unwrap();
            std::fs::write(
                patch_path(home),
                "- id: ui-theme\n  name: \"@deepseek-ai/dsh-client-ui-theme\"\n  config:\n    preference: system\n- id: computer-use-cua-driver-native\n  disabled: false\n",
            )
            .unwrap();

            set_provider(
                "official",
                &DeepSeekHarnessProviderConfig {
                    api_key: Some("sk-test".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

            let state = read_native_state().unwrap();
            assert!(state.providers.contains_key(OFFICIAL_PROVIDER_ID));
            let patch = std::fs::read_to_string(patch_path(home)).unwrap();
            assert!(patch.contains("ui-theme"));
            assert!(patch.contains("preference: system"));
            assert!(patch.contains("disabled: false"));
        });
    }

    #[test]
    #[serial]
    fn imports_legacy_settings_once_and_strips_invalid_fields() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "llm-deepseek:\n  baseURL: https://legacy.example.com\n  profile: desktop\n  apiKeyEnv: DEEPSEEK_API_KEY\nllm-pi-ai:\n  providers:\n    k3:\n      displayName: K3\n      baseURL: https://k3.example.com\nui-theme:\n  preference: dark\nagent-default-model:\n  provider: k3\n  model: k3\n",
            )
            .unwrap();

            let state = read_native_state().unwrap();
            // The read path mirrors the pending import (legacy over patch).
            assert!(state.providers.contains_key("k3"));
            assert_eq!(state.current_provider.as_deref(), Some("k3"));

            set_provider(
                "official",
                &DeepSeekHarnessProviderConfig {
                    api_key: Some("sk-test".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    models: Some(serde_json::json!([{ "id": "deepseek-v4-pro" }])),
                    ..Default::default()
                },
            )
            .unwrap();

            // The write imported the legacy document away first.
            assert!(!home.join("settings.yaml").exists());
            assert!(home.join("settings.yaml.imported").exists());

            let state = read_native_state().unwrap();
            assert_eq!(
                state.providers[OFFICIAL_PROVIDER_ID].config["baseURL"],
                "https://api.deepseek.com"
            );
            assert!(state.providers.contains_key("k3"));
            assert_eq!(state.current_provider.as_deref(), Some("k3"));

            let patch = std::fs::read_to_string(patch_path(home)).unwrap();
            assert!(patch.contains("ui-theme"));
            assert!(patch.contains("preference: dark"));
            // The schema-invalid `profile` key must not survive the import.
            assert!(!patch.contains("profile:"));
        });
    }

    #[test]
    #[serial]
    fn remove_official_route_keeps_sibling_entries_and_shared_credentials() {
        with_temp_home(|home| {
            set_provider(
                "official",
                &DeepSeekHarnessProviderConfig {
                    api_key: Some("sk-test".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    models: Some(serde_json::json!([{ "id": "deepseek-v4-pro" }])),
                    ..Default::default()
                },
            )
            .unwrap();
            set_pi_ai_provider(
                "company",
                &serde_json::json!({
                    "displayName": "Company",
                    "api": "openai-completions",
                    "baseURL": "https://gateway.example/v1",
                    "apiKeyEnv": "DEEPSEEK_API_KEY",
                    "apiKey": "sk-test",
                    "models": [{ "id": "glm-5.3" }]
                }),
            )
            .unwrap();

            remove_provider().unwrap();

            let state = read_native_state().unwrap();
            assert!(!state.providers.contains_key(OFFICIAL_PROVIDER_ID));
            assert!(state.providers.contains_key("company"));
            // The custom route shares DEEPSEEK_API_KEY: the credential survives.
            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(credentials.contains("DEEPSEEK_API_KEY"));
        });
    }

    #[test]
    #[serial]
    fn writes_versioned_credentials_and_preserves_records() {
        with_temp_home(|home| {
            std::fs::write(
                home.join(".credentials.yaml"),
                "version: 1\nrefs:\n  OTHER_KEY: keep\nrecords:\n  client-connection/browser-session:\n    kind: grant\n    payload:\n      secret: keep-record\n",
            )
            .unwrap();

            set_provider(
                OFFICIAL_PROVIDER_ID,
                &DeepSeekHarnessProviderConfig {
                    api_key: Some("rotated".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

            let credentials = parse_yaml_document(&home.join(".credentials.yaml")).unwrap();
            let root = credentials.as_mapping().unwrap();
            assert_eq!(
                root.get(yaml_key("version")).and_then(Yaml::as_i64),
                Some(1)
            );
            let refs = root
                .get(yaml_key("refs"))
                .and_then(Yaml::as_mapping)
                .unwrap();
            assert_eq!(
                refs.get(yaml_key("DEEPSEEK_API_KEY"))
                    .and_then(Yaml::as_str),
                Some("rotated")
            );
            assert_eq!(
                refs.get(yaml_key("OTHER_KEY")).and_then(Yaml::as_str),
                Some("keep")
            );
            assert!(root.contains_key(yaml_key("records")));
        });
    }

    #[test]
    #[serial]
    fn clears_stale_credentials_when_api_key_is_removed() {
        with_temp_home(|home| {
            std::fs::write(
                home.join(".credentials.yaml"),
                "DEEPSEEK_API_KEY: secret\nOTHER_KEY: keep\n",
            )
            .unwrap();

            set_provider(
                "official",
                &DeepSeekHarnessProviderConfig {
                    api_key: None,
                    base_url: Some("https://api.deepseek.com".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(!credentials.contains("DEEPSEEK_API_KEY:"));
            assert!(credentials.contains("OTHER_KEY: keep"));
        });
    }

    #[test]
    #[serial]
    fn reads_official_and_pi_ai_native_providers_with_current_model() {
        with_temp_home(|home| {
            std::fs::create_dir_all(home.join("profiles/desktop")).unwrap();
            std::fs::write(
                patch_path(home),
                "- id: llm-deepseek\n  name: \"@deepseek-ai/dsh-llm-deepseek-api-key\"\n  config:\n    baseURL: https://api.deepseek.com\n    apiKeyEnv: DEEPSEEK_API_KEY\n    models:\n      - id: deepseek-v4-pro\n        name: DeepSeek V4 Pro\n- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      company-gateway:\n        displayName: Company Gateway\n        api: openai-completions\n        baseURL: https://gateway.example/v1\n        apiKeyEnv: COMPANY_GATEWAY_API_KEY\n        models:\n          - id: example-model-1\n            name: Example Model\n- id: agent-default-model\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: company-gateway\n    model: example-model-1\n",
            )
            .unwrap();
            std::fs::write(
                home.join(".credentials.yaml"),
                "version: 1\nrefs:\n  DEEPSEEK_API_KEY: deepseek-secret\n  COMPANY_GATEWAY_API_KEY: gateway-secret\nrecords:\n  client-connection/browser-session:\n    kind: grant\n    payload:\n      version: 1\n      secret: keep\n",
            )
            .unwrap();

            let state = read_native_state().unwrap();
            assert_eq!(state.current_provider.as_deref(), Some("company-gateway"));
            assert_eq!(state.current_model.as_deref(), Some("example-model-1"));
            assert_eq!(state.providers.len(), 2);

            let official = state.providers.get(OFFICIAL_PROVIDER_ID).unwrap();
            assert_eq!(official.source, NativeProviderSource::DeepSeek);
            assert_eq!(official.config["apiKey"], "deepseek-secret");

            let gateway = state.providers.get("company-gateway").unwrap();
            assert_eq!(gateway.source, NativeProviderSource::PiAi);
            assert_eq!(gateway.name, "Company Gateway");
            assert_eq!(gateway.config["baseURL"], "https://gateway.example/v1");
            assert_eq!(gateway.config["apiKey"], "gateway-secret");
        });
    }

    #[test]
    #[serial]
    fn writes_and_removes_pi_ai_provider_without_touching_siblings() {
        with_temp_home(|home| {
            std::fs::create_dir_all(home.join("profiles/desktop")).unwrap();
            std::fs::write(
                patch_path(home),
                "- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      sibling:\n        displayName: Keep\n- id: agent-default-model\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: sibling\n    model: keep-model\n",
            )
            .unwrap();
            std::fs::write(home.join(".credentials.yaml"), "version: 1\nrefs: {}\n").unwrap();

            let config = serde_json::json!({
                "displayName": "Company Gateway",
                "api": "openai-completions",
                "baseURL": "https://gateway.example/v1",
                "apiKeyEnv": "COMPANY_API_KEY",
                "apiKey": "secret",
                "models": [{ "id": "glm-5.3", "name": "GLM 5.3" }]
            });
            set_pi_ai_provider("company", &config).unwrap();
            set_current_model("company", "glm-5.3").unwrap();

            let state = read_native_state().unwrap();
            assert!(state.providers.contains_key("sibling"));
            assert!(state.providers.contains_key("company"));
            assert_eq!(state.current_provider.as_deref(), Some("company"));
            assert_eq!(state.current_model.as_deref(), Some("glm-5.3"));

            remove_pi_ai_provider("company").unwrap();
            let state = read_native_state().unwrap();
            assert!(state.providers.contains_key("sibling"));
            assert!(!state.providers.contains_key("company"));
            let credentials = parse_yaml_document(&home.join(".credentials.yaml")).unwrap();
            let refs = credentials
                .as_mapping()
                .and_then(|root| root.get(yaml_key("refs")))
                .and_then(Yaml::as_mapping)
                .unwrap();
            assert!(!refs.contains_key(yaml_key("COMPANY_API_KEY")));
            let patch = std::fs::read_to_string(patch_path(home)).unwrap();
            assert!(patch.contains("agent-default-model"));
        });
    }

    #[test]
    #[serial]
    fn removing_the_last_pi_ai_provider_retains_empty_override() {
        with_temp_home(|home| {
            set_pi_ai_provider(
                "company",
                &serde_json::json!({
                    "displayName": "Company",
                    "api": "openai-completions",
                    "baseURL": "https://gateway.example/v1",
                    "apiKeyEnv": "COMPANY_API_KEY",
                    "apiKey": "secret",
                    "models": [{ "id": "glm-5.3" }]
                }),
            )
            .unwrap();
            assert!(read_native_state()
                .unwrap()
                .providers
                .contains_key("company"));

            remove_pi_ai_provider("company").unwrap();

            let patch = std::fs::read_to_string(patch_path(home)).unwrap();
            assert!(patch.contains("providers: {}"));
            assert!(!read_native_state()
                .unwrap()
                .providers
                .contains_key("company"));
        });
    }

    #[test]
    #[serial]
    fn clear_current_model_only_fires_for_the_matching_provider() {
        with_temp_home(|home| {
            set_pi_ai_provider(
                "company",
                &serde_json::json!({
                    "displayName": "Company",
                    "api": "openai-completions",
                    "baseURL": "https://gateway.example/v1",
                    "apiKeyEnv": "COMPANY_API_KEY",
                    "apiKey": "secret",
                    "models": [{ "id": "glm-5.3" }]
                }),
            )
            .unwrap();
            set_current_model("company", "glm-5.3").unwrap();

            assert!(!clear_current_model_if_provider("other").unwrap());
            assert!(clear_current_model_if_provider("company").unwrap());

            let state = read_native_state().unwrap();
            assert_eq!(state.current_provider, None);
            assert_eq!(state.current_model, None);
            let patch = std::fs::read_to_string(patch_path(home)).unwrap();
            assert!(patch.contains("agent-default-model"));
            assert!(patch.contains("disabled: true"));
        });
    }

    #[test]
    #[serial]
    fn desktop_shipped_optional_bundles_do_not_block_provider_management() {
        with_temp_home(|home| {
            let profile = home.join("profiles/desktop");
            fs::create_dir_all(&profile).unwrap();
            fs::write(
                profile.join("package.json"),
                serde_json::to_string(&serde_json::json!({
                    "dsh": {"profile": {"bundles": [
                        "@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app",
                        "@deepseek-ai/dsh-experimental-auto-review",
                        "@deepseek-ai/dsh-experimental-agent-team-profile",
                        "@deepseek-ai/dsh-experimental-inspector-profile"
                    ]}}
                }))
                .unwrap(),
            )
            .unwrap();
            set_pi_ai_provider(
                "local-check",
                &serde_json::json!({
                    "apiKeyEnv": "LOCAL_CHECK_API_KEY", "apiKey": "test-only",
                    "models": [{"id": "second"}]
                }),
            )
            .unwrap();
            set_current_model("local-check", "second").unwrap();
            let state = read_native_state().unwrap();
            assert!(state.providers.contains_key("local-check"));
            assert_eq!(state.current_model.as_deref(), Some("second"));
        });
    }

    #[test]
    #[serial]
    fn legacy_extra_fields_are_not_re_emitted() {
        with_temp_home(|_home| {
            // Simulate a historical CC Switch row that still carries `profile`
            // and other unknown keys inside settings_config.
            let config = DeepSeekHarnessProviderConfig {
                api_key: Some("sk-test".to_string()),
                base_url: Some("https://api.deepseek.com".to_string()),
                ..Default::default()
            };
            let mut value = serde_json::to_value(&config).unwrap();
            value["profile"] = serde_json::json!("desktop");
            value["mystery"] = serde_json::json!("drop me");
            let config: DeepSeekHarnessProviderConfig = serde_json::from_value(value).unwrap();
            assert!(config.extra.contains_key("mystery"));

            set_provider("official", &config).unwrap();
            let patch = std::fs::read_to_string(patch_path(_home)).unwrap();
            assert!(!patch.contains("mystery"));
            assert!(!patch.contains("profile"));
        });
    }
}
