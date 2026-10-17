//! A small console entry point for an existing desktop configuration.
//! Reads do not initialize the GUI, repair settings, or migrate the database.

use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use clap::{Parser, Subcommand};
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    app_config::AppType, config, database::Database, mode::current, AppState, Provider,
    ProviderService,
};

#[derive(Debug, Parser)]
#[command(
    name = "cc-switch-cli",
    version,
    about = "Inspect and switch existing CC Switch providers without opening a window"
)]
pub struct Cli {
    /// Application (default: claude; status defaults to all applications)
    #[arg(short, long, global = true)]
    pub app: Option<AppType>,
    /// Output machine-readable JSON (credentials are never exported)
    #[arg(long, global = true)]
    pub json: bool,
    /// CC Switch data directory; use this if the desktop uses a custom location
    #[arg(long, global = true)]
    pub config_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show saved provider selections and direct/proxy mode (not a health check)
    Status,
    /// List saved providers; * marks the provider currently in use
    List,
    /// Show the provider currently in use
    Current,
    /// Show a provider's endpoint, model, and live configuration paths
    Config { provider: Option<String> },
    /// Switch a Claude Code, Codex, or Gemini provider using the desktop service
    Use {
        /// Exact provider ID or a unique, case-insensitive name
        provider: String,
        /// Resolve and preview without writing files or creating a backup
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    app: String,
    id: String,
    name: String,
    current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

fn read_error() -> String {
    "Could not read an existing, compatible CC Switch database. Initialize providers with the matching desktop version first; check --config-dir for a custom data location.".into()
}

/// Run a parsed command. This must be called once per process, before settings load.
pub fn execute(cli: Cli) -> Result<String, String> {
    if let Some(dir) = &cli.config_dir {
        if !dir.is_dir() {
            return Err("--config-dir must point to an existing CC Switch data directory".into());
        }
        crate::app_store::update_cached_override(Some(dir.clone()));
    }
    let settings_path = config::get_home_dir().join(".cc-switch/settings.json");
    match fs::read(&settings_path) {
        Ok(bytes) => {
            serde_json::from_slice::<crate::AppSettings>(&bytes).map_err(|_| {
                "Could not parse device settings; repair them in the desktop first.".to_string()
            })?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not read device settings.".into()),
    }
    let db = Database::open_read_only().map_err(|_| read_error())?;
    let app = cli.app.clone().unwrap_or(AppType::Claude);
    let value = match cli.command {
        Command::Status => {
            let apps: Vec<_> = cli
                .app
                .map_or_else(|| AppType::all().collect(), |app| vec![app]);
            let mut selections = Vec::new();
            for app in apps {
                let mode = saved_mode(&app)?;
                let providers = db
                    .get_all_providers(app.as_str())
                    .map_err(|_| read_error())?;
                let id = current::provider_for_read_only(&db, &app, current::Purpose::InUse)
                    .map_err(|_| read_error())?;
                selections.push(json!({
                    "app": app.as_str(),
                    "mode": if mode.is_proxy() { "proxy" } else { "direct" },
                    "provider": id.as_ref().and_then(|id| providers.get(id)).map(|p| summary(&app, p, id.as_deref())),
                }));
            }
            json!({"selections":selections, "note":"Saved configuration, not a process or network health check."})
        }
        command => {
            let mode = saved_mode(&app)?;
            // Read DAO rows directly: some additive ProviderService::list paths import live data.
            let providers = db
                .get_all_providers(app.as_str())
                .map_err(|_| read_error())?;
            let id = current::provider_for_read_only(&db, &app, current::Purpose::InUse)
                .map_err(|_| read_error())?;
            match command {
                Command::List => json!(providers
                    .values()
                    .map(|p| summary(&app, p, id.as_deref()))
                    .collect::<Vec<_>>()),
                Command::Current => {
                    let provider = resolve_current(&providers, id.as_deref())?;
                    json!(summary(&app, provider, id.as_deref()))
                }
                Command::Config { provider } => {
                    let provider = match provider {
                        Some(target) => resolve(&providers, &target)?,
                        None => resolve_current(&providers, id.as_deref())?,
                    };
                    json!({"provider":summary(&app,provider,id.as_deref()),"liveFiles":live_files(&app)})
                }
                Command::Use { provider, dry_run } => {
                    if !matches!(app, AppType::Claude | AppType::Codex | AppType::Gemini) {
                        return Err("Switching currently supports claude, codex, and gemini; other applications can be inspected.".into());
                    }
                    let provider = resolve(&providers, &provider)?;
                    let selected = summary(&app, provider, id.as_deref());
                    if dry_run {
                        json!({"dryRun":true,"provider":selected,"liveFiles":live_files(&app)})
                    } else if selected.current {
                        json!({"switched":false,"provider":selected})
                    } else {
                        // A separate process cannot supervise the desktop's running proxy.
                        if mode.is_proxy() {
                            return Err(
                                "Exit local routing in the desktop before switching from this CLI."
                                    .into(),
                            );
                        }
                        let backup = backup(&db, &app).map_err(|_| {
                            "Could not create the pre-switch backup; nothing was switched."
                                .to_string()
                        })?;
                        let provider_id = provider.id.clone();
                        drop(db);
                        let writable = Database::init().map_err(|_| {
                            format!(
                                "Could not initialize switching; backup: {}",
                                backup.display()
                            )
                        })?;
                        let state = AppState::new(Arc::new(writable));
                        let result = ProviderService::switch(&state,app.clone(),&provider_id)
                            .map_err(|_| format!("Provider switch failed; inspect the desktop configuration. Backup: {}",backup.display()))?;
                        let current_id =
                            ProviderService::current(&state, app.clone()).map_err(|_| {
                                format!("Could not verify the switch; backup: {}", backup.display())
                            })?;
                        if current_id != provider_id {
                            return Err(format!(
                                "Switch verification failed; backup: {}",
                                backup.display()
                            ));
                        }
                        // Do not expose service warning strings, which may contain configuration data.
                        json!({"switched":true,"provider":summary(&app,provider,Some(&provider_id)),"backup":backup,"warningCount":result.warnings.len()})
                    }
                }
                Command::Status => unreachable!(),
            }
        }
    };
    if cli.json {
        serde_json::to_string_pretty(&value).map_err(|_| "Could not encode output".into())
    } else if let Some(rows) = value.as_array() {
        Ok(rows
            .iter()
            .map(|row| {
                format!(
                    "{} {}\t{}",
                    if row["current"] == true { "*" } else { " " },
                    row["id"].as_str().unwrap_or_default(),
                    row["name"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    } else {
        serde_json::to_string_pretty(&value).map_err(|_| "Could not encode output".into())
    }
}

fn saved_mode(app: &AppType) -> Result<crate::mode::state::ModeState, String> {
    if !app.supports_local_proxy() {
        return Ok(crate::mode::state::ModeState::default());
    }
    crate::mode::state::mode_state_read_only(
        &crate::live::engine::DeviceStore::for_device(),
        app.as_str(),
    )
    .map_err(|_| {
        "Could not read local routing state; inspect it in the desktop before continuing.".into()
    })
}

fn resolve_current<'a>(
    providers: &'a IndexMap<String, Provider>,
    id: Option<&str>,
) -> Result<&'a Provider, String> {
    id.and_then(|id|providers.get(id)).ok_or_else(||"No current provider. Select an application/provider in the desktop, or pass a provider to config.".into())
}

fn resolve<'a>(
    providers: &'a IndexMap<String, Provider>,
    target: &str,
) -> Result<&'a Provider, String> {
    if let Some(provider) = providers.get(target) {
        return Ok(provider);
    }
    let matches: Vec<_> = providers
        .values()
        .filter(|p| p.name.trim().eq_ignore_ascii_case(target.trim()))
        .collect();
    match matches.as_slice() {
        [provider] => Ok(provider),
        [] => Err("Provider not found; use list to find its ID.".into()),
        _ => Err("Provider name is ambiguous; specify an exact ID.".into()),
    }
}

fn summary(app: &AppType, provider: &Provider, current_id: Option<&str>) -> Summary {
    let config = &provider.settings_config;
    let env = config.get("env").unwrap_or(&Value::Null);
    let toml = config
        .get("config")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<toml::Value>().ok());
    let table = toml
        .as_ref()
        .and_then(|v| v.get("model_provider"))
        .and_then(toml::Value::as_str)
        .and_then(|id| toml.as_ref()?.get("model_providers")?.get(id));
    let endpoint = [
        env.get("ANTHROPIC_BASE_URL"),
        env.get("GOOGLE_GEMINI_BASE_URL"),
        config.get("baseURL"),
        config.get("base_url"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .or_else(|| table?.get("base_url")?.as_str())
    .and_then(safe_endpoint);
    let model = [
        env.get("ANTHROPIC_MODEL"),
        env.get("GEMINI_MODEL"),
        config.get("model"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .or_else(|| toml.as_ref()?.get("model")?.as_str())
    .map(str::to_owned);
    Summary {
        app: app.as_str().into(),
        id: provider.id.clone(),
        name: provider.name.clone(),
        current: current_id == Some(&provider.id),
        endpoint,
        model,
    }
}

fn safe_endpoint(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    // Only show the origin: custom endpoints can also embed credentials in their path.
    Some(url.origin().ascii_serialization())
}

fn live_files(app: &AppType) -> Vec<PathBuf> {
    match app {
        AppType::Claude => vec![config::get_claude_settings_path()],
        AppType::Codex => vec![
            crate::codex_config::get_codex_config_path(),
            crate::codex_config::get_codex_auth_path(),
        ],
        AppType::Gemini => vec![
            crate::gemini_config::get_gemini_env_path(),
            crate::gemini_config::get_gemini_settings_path(),
        ],
        _ => Vec::new(),
    }
}

fn backup(db: &Database, app: &AppType) -> Result<PathBuf, crate::AppError> {
    let device_dir = config::get_home_dir().join(".cc-switch");
    let dir = device_dir
        .join("backups")
        .join(format!("cli-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).map_err(|e| crate::AppError::io(&dir, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| crate::AppError::io(&dir, e))?;
    }
    let database = dir.join("cc-switch.db");
    let source = db
        .conn
        .lock()
        .map_err(|e| crate::AppError::Database(e.to_string()))?;
    let mut destination = rusqlite::Connection::open(&database)
        .map_err(|e| crate::AppError::Database(e.to_string()))?;
    rusqlite::backup::Backup::new(&source, &mut destination)
        .and_then(|backup| backup.run_to_completion(128, Duration::from_millis(5), None))
        .map_err(|e| crate::AppError::Database(e.to_string()))?;
    drop(destination);
    drop(source);
    let mut paths = vec![
        device_dir.join("settings.json"),
        device_dir.join("live-state.json"),
    ];
    paths.extend(live_files(app));
    let mut manifest = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let saved = format!("file-{index}");
        let data = match fs::read(&path) {
            Ok(data) => Some(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(crate::AppError::io(&path, e)),
        };
        if let Some(data) = &data {
            fs::write(dir.join(&saved), data).map_err(|e| crate::AppError::io(&dir, e))?;
        }
        manifest.push(json!({"path":path,"saved":data.map(|_|saved)}));
    }
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|e| crate::AppError::JsonSerialize { source: e })?,
    )
    .map_err(|e| crate::AppError::io(&dir, e))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_never_exposes_credentials_or_paths() {
        assert_eq!(
            safe_endpoint("https://user:password@example.test/secret?key=secret#secret"),
            Some("https://example.test".into())
        );
        assert_eq!(safe_endpoint("file:///private"), None);
    }

    #[test]
    fn resolves_exact_ids_before_names_and_rejects_ambiguity() {
        let mut providers = IndexMap::new();
        providers.insert(
            "one".into(),
            Provider::with_id("one".into(), "Same".into(), json!({}), None),
        );
        providers.insert(
            "two".into(),
            Provider::with_id("two".into(), "Same".into(), json!({}), None),
        );
        assert_eq!(resolve(&providers, "one").unwrap().id, "one");
        assert!(resolve(&providers, " same ").is_err());
        assert!(resolve(&providers, "missing").is_err());
    }

    #[test]
    fn config_summary_does_not_serialize_auth_or_raw_toml() {
        let provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({"auth":{"OPENAI_API_KEY":"secret"},"config":"model = 'test-model'\nmodel_provider = 'custom'\n[model_providers.custom]\nbase_url = 'https://user:secret@example.test/secret'\nexperimental_bearer_token = 'secret'"}),
            None,
        );
        let value = serde_json::to_value(summary(&AppType::Codex, &provider, Some("p"))).unwrap();
        assert_eq!(value["endpoint"], "https://example.test");
        assert_eq!(value["model"], "test-model");
        assert!(!value.to_string().contains("secret"));
    }
}
