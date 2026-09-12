use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use yaml_rust::yaml::{Hash as YamlHash, Yaml};
use yaml_rust::{YamlEmitter, YamlLoader};

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;

pub const OFFICIAL_PROVIDER_ID: &str = "deepseek-official";
pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const DESKTOP_PROFILE_NAME: &str = "desktop";
const API_KEY_FIELD: &str = "apiKey";
const SETTINGS_NAMESPACE: &str = "llm-deepseek";
const PI_AI_NAMESPACE: &str = "llm-pi-ai";
const DEFAULT_MODEL_NAMESPACE: &str = "agent-default-model";
const API_KEY_REF: &str = "DEEPSEEK_API_KEY";
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
/// `apiKey` is intentionally excluded from live settings: official Harness keeps secrets in
/// `$DSH_HOME/.credentials.yaml`, not in `settings.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DeepSeekHarnessProviderConfig {
    #[serde(rename = "apiKey", default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(rename = "baseURL", default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
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

pub fn get_settings_path() -> PathBuf {
    get_dsh_home().join("settings.yaml")
}

pub fn get_credentials_path() -> PathBuf {
    get_dsh_home().join(".credentials.yaml")
}

pub fn get_profile_dir(profile: Option<&str>) -> Result<PathBuf, AppError> {
    let name = normalize_profile_name(profile)?;
    Ok(get_dsh_home().join("profiles").join(name))
}

fn normalize_profile_name(profile: Option<&str>) -> Result<String, AppError> {
    let name = profile.unwrap_or(DEFAULT_PROFILE_NAME).trim();
    if name.is_empty() {
        return Ok(DEFAULT_PROFILE_NAME.to_string());
    }
    if name == "." || name == ".." || name == "node_modules" || name.contains(['/', '\\']) {
        return Err(AppError::InvalidInput(format!(
            "Invalid DeepSeek Harness profile name: {name}"
        )));
    }
    Ok(name.to_string())
}

/// Write one provider to both official Harness surfaces.
///
/// Unrelated YAML sections and extra credentials survive, while the managed Harness section is
/// serialized canonically because CC Switch owns that complete schema namespace.
pub fn set_provider(
    provider_id: &str,
    config: &DeepSeekHarnessProviderConfig,
) -> Result<(), AppError> {
    let mut config = config.clone();
    if config
        .profile
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        config.profile = Some(DESKTOP_PROFILE_NAME.to_string());
    }
    if config
        .base_url
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        config.base_url = None;
    }
    let config_value = serde_json::to_value(&config)
        .map_err(|error| AppError::Config(format!("Serialize DeepSeek Harness config: {error}")))?;
    let object = config_value.as_object().ok_or_else(|| {
        AppError::Config("DeepSeek Harness configuration must be an object".to_string())
    })?;

    ensure_profile(config.profile.as_deref())?;

    {
        let mut section = YamlHash::new();
        for (key, value) in object {
            if key == API_KEY_FIELD {
                continue;
            }
            section.insert(mapping_key(key), yaml_value(value)?);
        }
        section.insert(
            mapping_key("apiKeyEnv"),
            Yaml::String(API_KEY_REF.to_string()),
        );
        update_yaml_document(&get_settings_path(), |document| {
            merge_mapping_value(document, SETTINGS_NAMESPACE, Yaml::Hash(section));
            Ok(())
        })?;
    }

    update_credential_reference(API_KEY_REF, config.api_key.as_deref())?;

    log::debug!("DeepSeek Harness provider '{provider_id}' written to live config");
    Ok(())
}

/// Remove a provider's CC Switch-managed settings and credential.
///
/// DeepSeek Harness has one official provider route, so deletion clears that route rather than
/// attempting a dictionary removal that the official schema cannot represent.
pub fn remove_provider() -> Result<(), AppError> {
    let settings_path = get_settings_path();
    if settings_path.exists() {
        update_yaml_document(&settings_path, |document| {
            if let Yaml::Hash(hash) = document {
                hash.remove(&mapping_key(SETTINGS_NAMESPACE));
            }
            Ok(())
        })?;
    }

    update_credential_reference(API_KEY_REF, None)?;
    Ok(())
}

pub fn provider_exists_in_live_config() -> Result<bool, AppError> {
    let document = parse_yaml_document(&get_settings_path())?;
    Ok(matches!(document, Yaml::Hash(hash) if hash.contains_key(&mapping_key(SETTINGS_NAMESPACE))))
}

pub fn read_native_state() -> Result<NativeState, AppError> {
    let settings = parse_yaml_document(&get_settings_path())?;
    let credentials = read_credential_references()?;
    let mut state = NativeState::default();
    let Some(root) = settings.as_hash() else {
        return Ok(state);
    };

    if let Some(section) = root
        .get(&mapping_key(SETTINGS_NAMESPACE))
        .and_then(Yaml::as_hash)
    {
        let config = yaml_hash_to_json(section)?;
        let name = "DeepSeek".to_string();
        let config = with_resolved_api_key(config, &credentials);
        state.providers.insert(
            OFFICIAL_PROVIDER_ID.to_string(),
            NativeProvider {
                name,
                source: NativeProviderSource::DeepSeek,
                config,
            },
        );
    }

    if let Some(providers) = root
        .get(&mapping_key(PI_AI_NAMESPACE))
        .and_then(Yaml::as_hash)
        .and_then(|section| section.get(&mapping_key("providers")))
        .and_then(Yaml::as_hash)
    {
        for (id, config) in providers {
            let Some(id) = id.as_str() else { continue };
            let Some(config_hash) = config.as_hash() else {
                continue;
            };
            let config = with_resolved_api_key(yaml_hash_to_json(config_hash)?, &credentials);
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

    if let Some(default_model) = root
        .get(&mapping_key(DEFAULT_MODEL_NAMESPACE))
        .and_then(Yaml::as_hash)
    {
        state.current_provider = yaml_string(default_model, "provider");
        state.current_model = yaml_string(default_model, "model");
    }
    Ok(state)
}

pub fn set_pi_ai_provider(provider_id: &str, config: &Value) -> Result<(), AppError> {
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
        update_credential_reference(reference, api_key.as_deref())?;
    }
    let section = yaml_value(&Value::Object(config))?;
    update_yaml_document(&get_settings_path(), |document| {
        let root = ensure_hash(document);
        let pi_ai = ensure_child_hash(root, PI_AI_NAMESPACE);
        let providers = ensure_child_hash(pi_ai, "providers");
        providers.insert(mapping_key(provider_id), section);
        Ok(())
    })
}

pub fn remove_pi_ai_provider(provider_id: &str) -> Result<(), AppError> {
    validate_route_id(provider_id)?;
    let settings_path = get_settings_path();
    if !settings_path.exists() {
        return Ok(());
    }
    let credential_ref = read_native_state()?
        .providers
        .get(provider_id)
        .and_then(|provider| provider.config.get("apiKeyEnv"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    update_yaml_document(&settings_path, |document| {
        if let Yaml::Hash(root) = document {
            if let Some(Yaml::Hash(section)) = root.get_mut(&mapping_key(PI_AI_NAMESPACE)) {
                if let Some(Yaml::Hash(providers)) = section.get_mut(&mapping_key("providers")) {
                    providers.remove(&mapping_key(provider_id));
                }
            }
        }
        Ok(())
    })?;
    if let Some(reference) = credential_ref {
        update_credential_reference(&reference, None)?;
    }
    Ok(())
}

pub fn set_current_model(provider_id: &str, model_id: &str) -> Result<(), AppError> {
    validate_route_id(provider_id)?;
    if model_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness model id cannot be empty".to_string(),
        ));
    }
    update_yaml_document(&get_settings_path(), |document| {
        let root = ensure_hash(document);
        let current = ensure_child_hash(root, DEFAULT_MODEL_NAMESPACE);
        current.insert(
            mapping_key("provider"),
            Yaml::String(provider_id.to_string()),
        );
        current.insert(mapping_key("model"), Yaml::String(model_id.to_string()));
        Ok(())
    })
}

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

fn read_credential_references() -> Result<HashMap<String, String>, AppError> {
    let path = get_credentials_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let document = parse_yaml_document(&path)?;
    let Some(root) = document.as_hash() else {
        return Ok(HashMap::new());
    };
    let refs = if root.get(&mapping_key("version")).is_some() {
        root.get(&mapping_key("refs")).and_then(Yaml::as_hash)
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
    update_yaml_document(&path, |document| {
        let root = ensure_hash(document);
        if !root.contains_key(&mapping_key("version")) {
            let legacy = root.clone();
            root.clear();
            root.insert(mapping_key("version"), Yaml::Integer(1));
            root.insert(mapping_key("refs"), Yaml::Hash(legacy));
        }
        let refs = ensure_child_hash(root, "refs");
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => {
                refs.insert(mapping_key(reference), Yaml::String(value.to_string()));
            }
            None => {
                refs.remove(&mapping_key(reference));
            }
        }
        Ok(())
    })
}

fn ensure_hash(document: &mut Yaml) -> &mut YamlHash {
    if !matches!(document, Yaml::Hash(_)) {
        *document = Yaml::Hash(YamlHash::new());
    }
    let Yaml::Hash(hash) = document else {
        unreachable!("document was normalized")
    };
    hash
}

fn ensure_child_hash<'a>(parent: &'a mut YamlHash, key: &str) -> &'a mut YamlHash {
    let key = mapping_key(key);
    if !matches!(parent.get(&key), Some(Yaml::Hash(_))) {
        parent.insert(key.clone(), Yaml::Hash(YamlHash::new()));
    }
    let Some(Yaml::Hash(hash)) = parent.get_mut(&key) else {
        unreachable!("child was normalized")
    };
    hash
}

fn yaml_hash_to_json(hash: &YamlHash) -> Result<Value, AppError> {
    let mut object = serde_json::Map::new();
    for (key, value) in hash {
        let key = key
            .as_str()
            .ok_or_else(|| AppError::Config("DSH YAML keys must be strings".to_string()))?;
        object.insert(key.to_string(), yaml_to_json(value)?);
    }
    Ok(Value::Object(object))
}

fn yaml_to_json(value: &Yaml) -> Result<Value, AppError> {
    Ok(match value {
        Yaml::Null | Yaml::BadValue => Value::Null,
        Yaml::Boolean(value) => Value::Bool(*value),
        Yaml::Integer(value) => Value::Number((*value).into()),
        Yaml::Real(value) => {
            serde_json::Number::from_f64(value.parse::<f64>().map_err(|error| {
                AppError::Config(format!("Invalid DSH real number '{value}': {error}"))
            })?)
            .map(Value::Number)
            .ok_or_else(|| AppError::Config(format!("Invalid DSH real number: {value}")))?
        }
        Yaml::String(value) => Value::String(value.clone()),
        Yaml::Array(values) => Value::Array(
            values
                .iter()
                .map(yaml_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Yaml::Hash(hash) => yaml_hash_to_json(hash)?,
        Yaml::Alias(_) => {
            return Err(AppError::Config(
                "DSH YAML aliases are not supported".to_string(),
            ))
        }
    })
}

fn yaml_string(hash: &YamlHash, key: &str) -> Option<String> {
    hash.get(&mapping_key(key))
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

fn ensure_profile(profile: Option<&str>) -> Result<(), AppError> {
    let profile_dir = get_profile_dir(profile)?;
    fs::create_dir_all(&profile_dir).map_err(|error| AppError::io(&profile_dir, error))?;

    let manifest_path = profile_dir.join("package.json");
    if !manifest_path.exists() {
        let profile_name = normalize_profile_name(profile)?;
        let manifest = serde_json::json!({
            "name": format!("dsh-profile-{profile_name}"),
            "private": true,
            "dependencies": {},
            "dsh": { "profile": { "bundles": [
                "@deepseek-ai/dsh-base",
                "@deepseek-ai/dsh-web-app",
                "@deepseek-ai/dsh-plugin-desktop",
            ] } }
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

fn parse_yaml_document(path: &Path) -> Result<Yaml, AppError> {
    if !path.exists() {
        return Ok(Yaml::Hash(YamlHash::new()));
    }
    let contents = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    let mut documents = YamlLoader::load_from_str(&contents)
        .map_err(|error| AppError::Config(format!("Parse {}: {error}", path.display())))?;
    match documents.len() {
        0 => Ok(Yaml::Hash(YamlHash::new())),
        1 => Ok(documents.remove(0)),
        _ => Err(AppError::Config(format!(
            "{} must contain exactly one YAML document",
            path.display()
        ))),
    }
}

fn update_yaml_document(
    path: &Path,
    update: impl FnOnce(&mut Yaml) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut document = parse_yaml_document(path)?;
    update(&mut document)?;
    let mut output = String::new();
    let mut emitter = YamlEmitter::new(&mut output);
    emitter
        .dump(&document)
        .map_err(|error| AppError::Config(format!("Serialize {}: {error:?}", path.display())))?;
    output.push('\n');
    if path.file_name().and_then(|name| name.to_str()) == Some(".credentials.yaml") {
        atomic_write_private(path, output.as_bytes())
    } else {
        crate::config::write_text_file(path, &output)
    }
}

fn mapping_key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

fn merge_mapping_value(document: &mut Yaml, key: &str, value: Yaml) {
    if !matches!(document, Yaml::Hash(_)) {
        *document = Yaml::Hash(YamlHash::new());
    }
    if let Yaml::Hash(hash) = document {
        hash.insert(mapping_key(key), value);
    }
}

fn yaml_value(value: &Value) -> Result<Yaml, AppError> {
    Ok(match value {
        Value::Null => Yaml::Null,
        Value::Bool(value) => Yaml::Boolean(*value),
        Value::Number(value) => {
            if let Some(number) = value.as_i64() {
                Yaml::Integer(number)
            } else if let Some(number) = value.as_f64() {
                Yaml::Real(number.to_string())
            } else {
                return Err(AppError::Config(format!("Unsupported number: {value}")));
            }
        }
        Value::String(value) => Yaml::String(value.clone()),
        Value::Array(values) => Yaml::Array(
            values
                .iter()
                .map(yaml_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(values) => {
            let mut hash = YamlHash::new();
            for (name, value) in values {
                hash.insert(mapping_key(name), yaml_value(value)?);
            }
            Yaml::Hash(hash)
        }
    })
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
    fn writes_official_settings_credentials_and_desktop_profile() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "# preserved\nother-plugin:\n  enabled: true\n",
            )
            .unwrap();

            let config = DeepSeekHarnessProviderConfig {
                api_key: Some("sk-test".to_string()),
                base_url: Some("https://example.com".to_string()),
                profile: None,
                models: Some(serde_json::json!([
                    { "id": "deepseek-v4-flash", "name": "Flash" }
                ])),
                ..Default::default()
            };
            set_provider("official", &config).unwrap();

            let settings = std::fs::read_to_string(home.join("settings.yaml")).unwrap();
            assert!(settings.contains(r#""other-plugin":"#));
            assert!(settings.contains(r#""llm-deepseek":"#));
            assert!(settings.contains(r#"baseURL: "https://example.com""#));
            assert!(settings.contains("apiKeyEnv: DEEPSEEK_API_KEY"));
            assert!(!settings.contains("sk-test"));

            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(credentials.contains("version: 1"));
            assert!(credentials.contains("refs:"));
            assert!(credentials.contains("DEEPSEEK_API_KEY: \"sk-test\""));
            assert!(home.join("profiles/desktop/package.json").exists());
            assert!(home.join("profiles/desktop/cordis.patch.yml").exists());
            assert!(home.join("profiles/desktop/pnpm-workspace.yaml").exists());
        });
    }

    #[test]
    #[serial]
    fn preserves_sibling_credentials_and_rejects_profile_traversal() {
        with_temp_home(|home| {
            std::fs::write(home.join(".credentials.yaml"), "OTHER_KEY: keep\n").unwrap();
            set_provider(
                "official",
                &DeepSeekHarnessProviderConfig {
                    api_key: Some("rotated".to_string()),
                    base_url: Some("https://api.deepseek.com/v1".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(credentials.contains("OTHER_KEY: keep"));
            assert!(credentials.contains("DEEPSEEK_API_KEY: rotated"));

            let error = get_profile_dir(Some("../outside")).unwrap_err();
            assert!(error
                .to_string()
                .contains("Invalid DeepSeek Harness profile"));
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
            assert!(!credentials.contains("DEEPSEEK_API_KEY"));
            assert!(credentials.contains("OTHER_KEY: keep"));
        });
    }

    #[test]
    #[serial]
    fn preserves_custom_providers_when_switching_managed_route() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "llm-pi-ai:\n  providers:\n    k3:\n      displayName: K3\nother-plugin: keep\n",
            )
            .unwrap();
            std::fs::write(home.join(".credentials.yaml"), "K3_API_KEY: keep\n").unwrap();

            set_provider(
                "official",
                &DeepSeekHarnessProviderConfig {
                    api_key: Some("sk-test".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

            let settings = std::fs::read_to_string(home.join("settings.yaml")).unwrap();
            assert!(settings.contains("llm-pi-ai"));
            assert!(settings.contains("displayName: K3"));
            assert!(settings.contains("llm-deepseek"));
            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(credentials.contains("K3_API_KEY: keep"));

            remove_provider().unwrap();

            let settings = std::fs::read_to_string(home.join("settings.yaml")).unwrap();
            assert!(!settings.contains("llm-deepseek"));
            assert!(settings.contains("displayName: K3"));
        });
    }

    #[test]
    #[serial]
    fn accepts_missing_base_url_for_official_runtime_fallback() {
        with_temp_home(|home| {
            for base_url in [None, Some(""), Some("   ")] {
                let base_url = base_url.map(ToOwned::to_owned);
                set_provider(
                    "official",
                    &DeepSeekHarnessProviderConfig {
                        api_key: Some("sk-test".to_string()),
                        base_url,
                        ..Default::default()
                    },
                )
                .unwrap();

                let settings = std::fs::read_to_string(home.join("settings.yaml")).unwrap();
                assert!(settings.contains("llm-deepseek"));
                assert!(!settings.contains("baseURL"));
            }
        });
    }

    #[test]
    #[serial]
    fn removes_managed_configuration_only() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "# preserved\nllm-deepseek:\n  baseURL: https://example.com\nother-plugin: keep\n",
            )
            .unwrap();
            std::fs::write(
                home.join(".credentials.yaml"),
                "DEEPSEEK_API_KEY: secret\nOTHER_KEY: keep\n",
            )
            .unwrap();

            remove_provider().unwrap();

            let settings = std::fs::read_to_string(home.join("settings.yaml")).unwrap();
            assert!(!settings.contains("llm-deepseek"));
            assert!(settings.contains(r#""other-plugin": keep"#));
            let credentials = std::fs::read_to_string(home.join(".credentials.yaml")).unwrap();
            assert!(!credentials.contains("DEEPSEEK_API_KEY"));
            assert!(credentials.contains("OTHER_KEY: keep"));
        });
    }

    #[test]
    #[serial]
    fn detects_managed_live_route() {
        with_temp_home(|home| {
            assert!(!provider_exists_in_live_config().unwrap());

            std::fs::write(
                home.join("settings.yaml"),
                "llm-deepseek:\n  baseURL: https://example.com\n",
            )
            .unwrap();
            assert!(provider_exists_in_live_config().unwrap());

            remove_provider().unwrap();
            assert!(!provider_exists_in_live_config().unwrap());
        });
    }

    #[test]
    #[serial]
    fn reads_official_and_pi_ai_native_providers_with_current_model() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "llm-deepseek:\n  baseURL: https://api.deepseek.com\n  apiKeyEnv: DEEPSEEK_API_KEY\n  models:\n    - id: deepseek-v4-pro\n      name: DeepSeek V4 Pro\nllm-pi-ai:\n  providers:\n    company-gateway:\n      displayName: Company Gateway\n      api: openai-completions\n      baseURL: https://gateway.example/v1\n      apiKeyEnv: COMPANY_GATEWAY_API_KEY\n      models:\n        - id: example-model-1\n          name: Example Model\nagent-default-model:\n  provider: company-gateway\n  model: example-model-1\n",
            )
            .unwrap();
            std::fs::write(
                home.join(".credentials.yaml"),
                "version: 1\nrefs:\n  DEEPSEEK_API_KEY: deepseek-secret\n  COMPANY_GATEWAY_API_KEY: gateway-secret\nrecords:\n  client-connection/browser-session:\n    kind: grant\n    payload:\n      version: 1\n      secret: keep\n",
            )
            .unwrap();

            let state = read_native_state().unwrap();
            assert_eq!(
                state.current_provider.as_deref(),
                Some("company-gateway")
            );
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
            let root = credentials.as_hash().unwrap();
            assert_eq!(root[&mapping_key("version")].as_i64(), Some(1));
            let refs = root[&mapping_key("refs")].as_hash().unwrap();
            assert_eq!(
                refs[&mapping_key("DEEPSEEK_API_KEY")].as_str(),
                Some("rotated")
            );
            assert_eq!(refs[&mapping_key("OTHER_KEY")].as_str(), Some("keep"));
            assert!(root.contains_key(&mapping_key("records")));
        });
    }

    #[test]
    #[serial]
    fn writes_and_removes_pi_ai_provider_without_touching_siblings() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "llm-pi-ai:\n  providers:\n    sibling:\n      displayName: Keep\nagent-default-model:\n  provider: sibling\n  model: keep-model\n",
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
                .as_hash()
                .and_then(|root| root.get(&mapping_key("refs")))
                .and_then(Yaml::as_hash)
                .unwrap();
            assert!(!refs.contains_key(&mapping_key("COMPANY_API_KEY")));
        });
    }
}
