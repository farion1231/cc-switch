use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::script_market::MarketScript;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserScriptConfig {
    pub enabled: bool,
    pub scripts: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub market: BTreeMap<String, MarketScriptInstall>,
}

impl Default for UserScriptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scripts: BTreeMap::new(),
            market: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarketScriptInstall {
    pub id: String,
    pub name: String,
    pub version: String,
    pub script_url: String,
    pub homepage: String,
    pub installed_at: String,
}

#[derive(Debug, Clone)]
pub struct UserScriptManager {
    builtin_dir: PathBuf,
    user_dir: PathBuf,
    config_path: PathBuf,
    config_lock: Arc<Mutex<()>>,
}

impl UserScriptManager {
    pub fn new(
        builtin_dir: impl Into<PathBuf>,
        user_dir: impl Into<PathBuf>,
        config_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            builtin_dir: builtin_dir.into(),
            user_dir: user_dir.into(),
            config_path: config_path.into(),
            config_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn load_config(&self) -> UserScriptConfig {
        let _guard = self.config_lock.lock().unwrap();
        self.load_config_unlocked()
    }

    fn load_config_unlocked(&self) -> UserScriptConfig {
        let Ok(text) = fs::read_to_string(&self.config_path) else {
            return UserScriptConfig::default();
        };
        let Ok(Value::Object(raw)) = serde_json::from_str::<Value>(&text) else {
            return UserScriptConfig::default();
        };
        config_from_object(&raw)
    }

    pub fn save_config(&self, config: &UserScriptConfig) -> anyhow::Result<()> {
        let _guard = self.config_lock.lock().unwrap();
        self.save_config_unlocked(config)
    }

    fn save_config_unlocked(&self, config: &UserScriptConfig) -> anyhow::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create user script config directory {}",
                    parent.display()
                )
            })?;
        }
        crate::settings::atomic_write(
            &self.config_path,
            serde_json::to_string_pretty(config)?.as_bytes(),
        )
    }

    pub fn set_global_enabled(&self, enabled: bool) -> anyhow::Result<UserScriptConfig> {
        let _guard = self.config_lock.lock().unwrap();
        let mut config = self.load_config_unlocked();
        config.enabled = enabled;
        self.save_config_unlocked(&config)?;
        Ok(config)
    }

    pub fn set_script_enabled(&self, key: &str, enabled: bool) -> anyhow::Result<UserScriptConfig> {
        let _guard = self.config_lock.lock().unwrap();
        let mut config = self.load_config_unlocked();
        config.scripts.insert(key.to_string(), enabled);
        self.save_config_unlocked(&config)?;
        Ok(config)
    }

    pub fn delete_user_script(&self, key: &str) -> anyhow::Result<UserScriptConfig> {
        let Some(file_name) = key.strip_prefix("user:").filter(|value| !value.is_empty()) else {
            anyhow::bail!("only user scripts can be deleted");
        };
        if file_name.contains(['/', '\\']) || file_name == "." || file_name == ".." {
            anyhow::bail!("invalid user script key");
        }
        let path = self.user_dir.join(file_name);
        let canonical_user_dir = self
            .user_dir
            .canonicalize()
            .or_else(|_| {
                fs::create_dir_all(&self.user_dir)?;
                self.user_dir.canonicalize()
            })
            .with_context(|| {
                format!(
                    "failed to resolve user script directory {}",
                    self.user_dir.display()
                )
            })?;
        if path.exists() {
            let canonical_path = path
                .canonicalize()
                .with_context(|| format!("failed to resolve user script {}", path.display()))?;
            if !canonical_path.starts_with(&canonical_user_dir) {
                anyhow::bail!("refusing to delete script outside user script directory");
            }
            fs::remove_file(&canonical_path).with_context(|| {
                format!("failed to delete user script {}", canonical_path.display())
            })?;
        }

        let _guard = self.config_lock.lock().unwrap();
        let mut config = self.load_config_unlocked();
        config.scripts.remove(key);
        config.market.remove(key);
        self.save_config_unlocked(&config)?;
        Ok(config)
    }

    pub fn user_script_path_for_market_id(&self, id: &str) -> PathBuf {
        self.user_dir.join(market_script_filename(id))
    }

    pub fn record_market_install(&self, script: &MarketScript) -> anyhow::Result<UserScriptConfig> {
        let _guard = self.config_lock.lock().unwrap();
        let mut config = self.load_config_unlocked();
        let key = format!("user:{}", market_script_filename(&script.id));
        config.scripts.entry(key.clone()).or_insert(true);
        config.market.insert(
            key,
            MarketScriptInstall {
                id: script.id.clone(),
                name: script.name.clone(),
                version: script.version.clone(),
                script_url: script.script_url.clone(),
                homepage: script.homepage.clone(),
                installed_at: current_unix_timestamp_string(),
            },
        );
        self.save_config_unlocked(&config)?;
        Ok(config)
    }

    pub fn inventory(&self) -> anyhow::Result<Value> {
        let config = self.load_config();
        let scripts = self.scan_scripts(&config)?;
        Ok(json!({
            "enabled": config.enabled,
            "builtin_dir": self.builtin_dir.to_string_lossy(),
            "user_dir": self.user_dir.to_string_lossy(),
            "scripts": scripts
        }))
    }

    pub fn build_enabled_bundle(&self) -> anyhow::Result<String> {
        let config = self.load_config();
        if !config.enabled {
            return Ok(String::new());
        }
        let mut blocks = Vec::new();
        for script in self.scan_script_files(&config)? {
            if !script.enabled {
                continue;
            }
            let source = fs::read_to_string(&script.path)
                .unwrap_or_else(|error| format!("throw new Error({});", json!(error.to_string())));
            blocks.push(wrap_script(&script, &source));
        }
        Ok(blocks.join("\n"))
    }

    fn scan_scripts(&self, config: &UserScriptConfig) -> anyhow::Result<Vec<Value>> {
        Ok(self
            .scan_script_files(config)?
            .into_iter()
            .map(|script| {
                let market = config.market.get(&script.key);
                let status = if !config.enabled || !script.enabled {
                    "disabled"
                } else {
                    "not_loaded"
                };
                json!({
                    "key": script.key,
                    "name": script.name,
                    "source": script.source,
                    "enabled": script.enabled,
                    "status": status,
                    "error": "",
                    "market_id": market.as_ref().map(|item| item.id.as_str()).unwrap_or(""),
                    "version": market.as_ref().map(|item| item.version.as_str()).unwrap_or(""),
                    "installed": market.is_some(),
                    "source_url": market.as_ref().map(|item| item.script_url.as_str()).unwrap_or(""),
                    "homepage": market.as_ref().map(|item| item.homepage.as_str()).unwrap_or("")
                })
            })
            .collect())
    }

    fn scan_script_files(&self, config: &UserScriptConfig) -> anyhow::Result<Vec<UserScriptFile>> {
        fs::create_dir_all(&self.user_dir).with_context(|| {
            format!(
                "failed to create user scripts directory {}",
                self.user_dir.display()
            )
        })?;
        let mut scripts = Vec::new();
        self.append_scripts("builtin", &self.builtin_dir, config, &mut scripts)?;
        self.append_scripts("user", &self.user_dir, config, &mut scripts)?;
        Ok(scripts)
    }

    fn append_scripts(
        &self,
        source: &str,
        directory: &std::path::Path,
        config: &UserScriptConfig,
        scripts: &mut Vec<UserScriptFile>,
    ) -> anyhow::Result<()> {
        let Ok(entries) = fs::read_dir(directory) else {
            return Ok(());
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("js"))
            .collect::<Vec<_>>();
        paths.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        });

        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let key = format!("{source}:{name}");
            scripts.push(UserScriptFile {
                enabled: config.scripts.get(&key).copied().unwrap_or(true),
                key,
                name,
                source: source.to_string(),
                path,
            });
        }
        Ok(())
    }
}

#[derive(Debug)]
struct UserScriptFile {
    key: String,
    name: String,
    source: String,
    path: PathBuf,
    enabled: bool,
}

fn wrap_script(script: &UserScriptFile, source: &str) -> String {
    format!(
        r#"
(() => {{
  window.__codexPlusUserScripts = window.__codexPlusUserScripts || {{ scripts: {{}} }};
  const key = {key};
  window.__codexPlusUserScripts.scripts[key] = {{ key, name: {name}, source: {source_name}, status: "loading", error: "", loadedAt: new Date().toISOString() }};
  try {{
{source}
    window.__codexPlusUserScripts.scripts[key].status = "loaded";
    window.__codexPlusUserScripts.scripts[key].loadedAt = new Date().toISOString();
  }} catch (error) {{
    window.__codexPlusUserScripts.scripts[key].status = "failed";
    window.__codexPlusUserScripts.scripts[key].error = String(error && (error.stack || error.message) || error);
  }}
}})();
"#,
        key = json!(script.key).to_string(),
        name = json!(script.name).to_string(),
        source_name = json!(script.source).to_string(),
        source = source
    )
}

fn config_from_object(raw: &Map<String, Value>) -> UserScriptConfig {
    let enabled = raw.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let scripts = raw
        .get("scripts")
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_bool()?)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let market = raw
        .get("market")
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), market_install_from_value(value)?)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    UserScriptConfig {
        enabled,
        scripts,
        market,
    }
}

pub fn market_script_filename(id: &str) -> String {
    let sanitized = sanitize_market_id(id);
    format!(
        "market-{}.js",
        if sanitized.is_empty() {
            "script".to_string()
        } else {
            sanitized
        }
    )
}

fn market_install_from_value(value: &Value) -> Option<MarketScriptInstall> {
    let raw = value.as_object()?;
    Some(MarketScriptInstall {
        id: string_field(raw, "id")?,
        name: string_field(raw, "name").unwrap_or_default(),
        version: string_field(raw, "version")?,
        script_url: string_field(raw, "script_url")?,
        homepage: string_field(raw, "homepage").unwrap_or_default(),
        installed_at: string_field(raw, "installed_at").unwrap_or_default(),
    })
}

fn string_field(raw: &Map<String, Value>, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn sanitize_market_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn current_unix_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
