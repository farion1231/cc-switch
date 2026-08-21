use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use yaml_rust::yaml::{Hash as YamlHash, Yaml};
use yaml_rust::{YamlEmitter, YamlLoader};

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;

pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const DESKTOP_PROFILE_NAME: &str = "desktop";
const API_KEY_FIELD: &str = "apiKey";
const SETTINGS_NAMESPACE: &str = "llm-deepseek";
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

pub fn get_dsh_home() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".dsh"))
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

    if let Some(api_key) = config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        update_yaml_document(&get_credentials_path(), |document| {
            merge_mapping_value(document, API_KEY_REF, Yaml::String(api_key.to_string()));
            Ok(())
        })?;
    }

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

    let credentials_path = get_credentials_path();
    if credentials_path.exists() {
        update_yaml_document(&credentials_path, |document| {
            if let Yaml::Hash(hash) = document {
                hash.remove(&mapping_key(API_KEY_REF));
            }
            Ok(())
        })?;
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
            assert_eq!(credentials, "---\nDEEPSEEK_API_KEY: \"sk-test\"\n");
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
}
