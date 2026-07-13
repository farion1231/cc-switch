use crate::codex_config::{read_and_validate_codex_config_text, write_codex_live_config_atomic};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use toml_edit::{value, DocumentMut};

const CLI_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginApp {
    Codex,
    Claude,
}

impl PluginApp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginScope {
    User,
    Project,
    Local,
}

impl PluginScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActions {
    pub install: bool,
    pub update: bool,
    pub enable: bool,
    pub disable: bool,
    pub uninstall: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginClientStatus {
    pub app: PluginApp,
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
    pub supported_actions: PluginActions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedPlugin {
    pub plugin_id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub app: PluginApp,
    pub marketplace_name: String,
    pub installed: bool,
    pub enabled: bool,
    pub scope: Option<String>,
    pub project_path: Option<String>,
    pub source: Option<String>,
    pub supported_actions: PluginActions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplace {
    pub name: String,
    pub app: PluginApp,
    pub source_type: Option<String>,
    pub source: Option<String>,
    pub root: Option<String>,
    pub supports_refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionResult {
    pub success: bool,
    pub requires_restart: bool,
    pub command_summary: String,
}

fn validate_nonempty(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.trim_start().starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(format!("Invalid {label}"));
    }
    #[cfg(target_os = "windows")]
    if value
        .chars()
        .any(|ch| matches!(ch, '&' | '|' | '<' | '>' | '^' | '%' | '!'))
    {
        return Err(format!("Invalid {label}"));
    }
    Ok(())
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), String> {
    validate_nonempty(plugin_id, "plugin id")?;
    let Some((name, marketplace)) = plugin_id.split_once('@') else {
        return Err("Plugin id must use plugin@marketplace format".to_string());
    };
    if name.is_empty() || marketplace.is_empty() {
        return Err("Plugin id must use plugin@marketplace format".to_string());
    }
    Ok(())
}

fn executable_for(app: PluginApp) -> PathBuf {
    crate::commands::resolve_tool_executable(app.as_str())
        .unwrap_or_else(|| PathBuf::from(app.as_str()))
}

#[cfg(target_os = "windows")]
fn command_for(path: &Path, args: &[&str]) -> Result<Command, String> {
    let is_script = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
    if is_script {
        if path.to_string_lossy().chars().any(|ch| {
            matches!(
                ch,
                '&' | '|' | '<' | '>' | '^' | '%' | '!' | '(' | ')' | ';' | ','
            )
        }) {
            return Err("CLI path contains characters unsupported by cmd.exe".to_string());
        }
        let mut command = Command::new("cmd");
        command.args(["/D", "/S", "/C"]).arg(path).args(args);
        Ok(command)
    } else {
        let mut command = Command::new(path);
        command.args(args);
        Ok(command)
    }
}

#[cfg(not(target_os = "windows"))]
fn command_for(path: &Path, args: &[&str]) -> Result<Command, String> {
    let mut command = Command::new(path);
    command.args(args);
    Ok(command)
}

async fn run_cli(app: PluginApp, args: &[&str], cwd: Option<&str>) -> Result<String, String> {
    let path = executable_for(app);
    let mut command = command_for(&path, args)?;
    if let Some(cwd) = cwd {
        validate_nonempty(cwd, "project path")?;
        if !Path::new(cwd).is_dir() {
            return Err(format!("Project directory does not exist: {cwd}"));
        }
        command.current_dir(cwd);
    }
    command.kill_on_drop(true);
    let output = timeout(CLI_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("{} command timed out after 60 seconds", app.as_str()))?
        .map_err(|e| format!("Failed to run {}: {e}", app.as_str()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        let message = if stderr.is_empty() { stdout } else { stderr };
        Err(concise_cli_error(&message))
    }
}

fn concise_cli_error(message: &str) -> String {
    let lowercase = message.to_ascii_lowercase();
    let end = lowercase.find("stack backtrace:").unwrap_or(message.len());
    let summary = message[..end].trim();
    if summary.is_empty() {
        message.trim().to_string()
    } else {
        summary.to_string()
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_string))
}

fn source_summary(value: Option<&Value>) -> Option<String> {
    let source = value?;
    if let Some(text) = source.as_str() {
        return Some(text.to_string());
    }
    string_field(source, &["url", "path", "source"])
}

fn plugin_from_value(
    app: PluginApp,
    value: &Value,
    installed_default: bool,
) -> Option<UnifiedPlugin> {
    let plugin_id = string_field(value, &["pluginId", "id"])?;
    let (fallback_name, fallback_marketplace) = plugin_id.split_once('@')?;
    let fallback_name = fallback_name.to_string();
    let fallback_marketplace = fallback_marketplace.to_string();
    let installed = value
        .get("installed")
        .and_then(Value::as_bool)
        .unwrap_or(installed_default);
    let enabled = value
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(installed);
    let marketplace_name =
        string_field(value, &["marketplaceName"]).unwrap_or(fallback_marketplace);
    Some(UnifiedPlugin {
        plugin_id,
        name: string_field(value, &["name"]).unwrap_or(fallback_name),
        description: string_field(value, &["description"]),
        version: string_field(value, &["version"]).filter(|version| version != "unknown"),
        app,
        marketplace_name,
        installed,
        enabled,
        scope: string_field(value, &["scope"]),
        project_path: string_field(value, &["projectPath"]),
        source: source_summary(value.get("source")),
        supported_actions: PluginActions {
            install: !installed,
            update: installed && app == PluginApp::Claude,
            enable: installed && !enabled,
            disable: installed && enabled,
            uninstall: installed,
        },
    })
}

pub fn parse_plugins_json(
    app: PluginApp,
    raw: &str,
    include_available: bool,
) -> Result<Vec<UnifiedPlugin>, String> {
    let root: Value = serde_json::from_str(raw).map_err(|e| format!("Invalid plugin JSON: {e}"))?;
    let installed_values = root
        .get("installed")
        .and_then(Value::as_array)
        .or_else(|| root.as_array())
        .cloned()
        .unwrap_or_default();
    let available_values = root
        .get("available")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut plugins = BTreeMap::<String, UnifiedPlugin>::new();
    if include_available {
        for value in available_values {
            if let Some(plugin) = plugin_from_value(app, &value, false) {
                plugins.insert(plugin.plugin_id.clone(), plugin);
            }
        }
    }
    for value in installed_values {
        if let Some(mut plugin) = plugin_from_value(app, &value, true) {
            if let Some(available) = plugins.get(&plugin.plugin_id) {
                plugin.description = plugin.description.or_else(|| available.description.clone());
                plugin.source = plugin.source.or_else(|| available.source.clone());
            }
            plugins.insert(plugin.plugin_id.clone(), plugin);
        }
    }
    Ok(plugins.into_values().collect())
}

fn merge_contextual_plugin_states(
    plugins: &mut [UnifiedPlugin],
    contextual_plugins: &[UnifiedPlugin],
    project_path: &str,
) {
    for plugin in plugins
        .iter_mut()
        .filter(|plugin| plugin.project_path.as_deref() == Some(project_path))
    {
        let Some(contextual) = contextual_plugins.iter().find(|candidate| {
            candidate.plugin_id == plugin.plugin_id
                && candidate.scope == plugin.scope
                && candidate.project_path == plugin.project_path
        }) else {
            continue;
        };
        plugin.enabled = contextual.enabled;
        plugin.supported_actions.enable = plugin.installed && !plugin.enabled;
        plugin.supported_actions.disable = plugin.installed && plugin.enabled;
    }
}

pub fn parse_marketplaces_json(
    app: PluginApp,
    raw: &str,
) -> Result<Vec<PluginMarketplace>, String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid marketplace JSON: {e}"))?;
    let values = root
        .get("marketplaces")
        .and_then(Value::as_array)
        .or_else(|| root.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(values
        .iter()
        .filter_map(|value| {
            let name = string_field(value, &["name"])?;
            let nested_source = value.get("marketplaceSource");
            let source_type = string_field(value, &["sourceType", "source"])
                .or_else(|| nested_source.and_then(|s| string_field(s, &["sourceType"])));
            let supports_refresh = app == PluginApp::Claude
                || source_type
                    .as_deref()
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("git"));
            Some(PluginMarketplace {
                name,
                app,
                source_type,
                source: string_field(value, &["url", "repo"])
                    .or_else(|| nested_source.and_then(|s| string_field(s, &["source"]))),
                root: string_field(value, &["root", "installLocation"]),
                supports_refresh,
            })
        })
        .collect())
}

pub async fn client_status(app: PluginApp) -> PluginClientStatus {
    let version = match run_cli(app, &["--version"], None).await {
        Ok(version) => version,
        Err(error) => {
            return PluginClientStatus {
                app,
                available: false,
                version: None,
                error: Some(error),
                supported_actions: PluginActions::default(),
            };
        }
    };
    match run_cli(app, &["plugin", "--help"], None).await {
        Ok(_) => PluginClientStatus {
            app,
            available: true,
            version: Some(version),
            error: None,
            supported_actions: PluginActions {
                install: true,
                update: app == PluginApp::Claude,
                enable: true,
                disable: true,
                uninstall: true,
            },
        },
        Err(error) => PluginClientStatus {
            app,
            available: false,
            version: None,
            error: Some(error),
            supported_actions: PluginActions::default(),
        },
    }
}

pub async fn list_plugins(
    app: PluginApp,
    include_available: bool,
) -> Result<Vec<UnifiedPlugin>, String> {
    let raw = match app {
        PluginApp::Codex => {
            run_cli(app, &["plugin", "list", "--available", "--json"], None).await?
        }
        PluginApp::Claude => {
            run_cli(app, &["plugin", "list", "--available", "--json"], None).await?
        }
    };
    let mut plugins = parse_plugins_json(app, &raw, include_available)?;
    if app == PluginApp::Claude {
        let project_paths = plugins
            .iter()
            .filter(|plugin| matches!(plugin.scope.as_deref(), Some("project" | "local")))
            .filter_map(|plugin| plugin.project_path.clone())
            .collect::<BTreeSet<_>>();
        for project_path in project_paths {
            if !Path::new(&project_path).is_dir() {
                continue;
            }
            let Ok(contextual_raw) =
                run_cli(app, &["plugin", "list", "--json"], Some(&project_path)).await
            else {
                continue;
            };
            if let Ok(contextual_plugins) = parse_plugins_json(app, &contextual_raw, false) {
                merge_contextual_plugin_states(&mut plugins, &contextual_plugins, &project_path);
            }
        }
    }
    Ok(plugins)
}

pub async fn list_marketplaces(app: PluginApp) -> Result<Vec<PluginMarketplace>, String> {
    let raw = run_cli(app, &["plugin", "marketplace", "list", "--json"], None).await?;
    parse_marketplaces_json(app, &raw)
}

fn action_result(summary: impl Into<String>, requires_restart: bool) -> PluginActionResult {
    PluginActionResult {
        success: true,
        requires_restart,
        command_summary: summary.into(),
    }
}

pub async fn add_marketplace(app: PluginApp, source: &str) -> Result<PluginActionResult, String> {
    validate_nonempty(source, "marketplace source")?;
    match app {
        PluginApp::Codex => {
            run_cli(
                app,
                &["plugin", "marketplace", "add", source, "--json"],
                None,
            )
            .await?;
        }
        PluginApp::Claude => {
            run_cli(
                app,
                &["plugin", "marketplace", "add", source, "--scope", "user"],
                None,
            )
            .await?;
        }
    }
    Ok(action_result(
        format!("{} plugin marketplace add <source>", app.as_str()),
        false,
    ))
}

pub async fn refresh_marketplace(app: PluginApp, name: &str) -> Result<PluginActionResult, String> {
    validate_nonempty(name, "marketplace name")?;
    let marketplace = list_marketplaces(app)
        .await?
        .into_iter()
        .find(|marketplace| marketplace.name == name)
        .ok_or_else(|| format!("Marketplace not found: {name}"))?;
    if !marketplace.supports_refresh {
        return Err(format!(
            "Marketplace '{name}' is local or built in and cannot be refreshed manually"
        ));
    }
    let verb = if app == PluginApp::Codex {
        "upgrade"
    } else {
        "update"
    };
    run_cli(app, &["plugin", "marketplace", verb, name], None).await?;
    Ok(action_result(
        format!("{} plugin marketplace {verb} {name}", app.as_str()),
        false,
    ))
}

pub async fn remove_marketplace(app: PluginApp, name: &str) -> Result<PluginActionResult, String> {
    validate_nonempty(name, "marketplace name")?;
    run_cli(app, &["plugin", "marketplace", "remove", name], None).await?;
    Ok(action_result(
        format!("{} plugin marketplace remove {name}", app.as_str()),
        false,
    ))
}

pub async fn install_plugin(
    app: PluginApp,
    plugin_id: &str,
    scope: Option<PluginScope>,
    project_path: Option<&str>,
) -> Result<PluginActionResult, String> {
    validate_plugin_id(plugin_id)?;
    match app {
        PluginApp::Codex => {
            run_cli(app, &["plugin", "add", plugin_id, "--json"], None).await?;
        }
        PluginApp::Claude => {
            let scope = scope.unwrap_or(PluginScope::User);
            if scope != PluginScope::User && project_path.is_none() {
                return Err("Project or local scope requires a project directory".to_string());
            }
            run_cli(
                app,
                &["plugin", "install", plugin_id, "--scope", scope.as_str()],
                project_path,
            )
            .await?;
        }
    }
    Ok(action_result(
        format!("{} plugin install {plugin_id}", app.as_str()),
        true,
    ))
}

pub async fn update_plugin(
    app: PluginApp,
    plugin_id: &str,
    scope: Option<PluginScope>,
    project_path: Option<&str>,
) -> Result<PluginActionResult, String> {
    validate_plugin_id(plugin_id)?;
    if app != PluginApp::Claude {
        return Err(
            "Codex does not support single-plugin updates; refresh its marketplace instead"
                .to_string(),
        );
    }
    let scope = scope.unwrap_or(PluginScope::User);
    run_cli(
        app,
        &["plugin", "update", plugin_id, "--scope", scope.as_str()],
        project_path,
    )
    .await?;
    Ok(action_result(
        format!("claude plugin update {plugin_id}"),
        true,
    ))
}

pub async fn uninstall_plugin(
    app: PluginApp,
    plugin_id: &str,
    scope: Option<PluginScope>,
    project_path: Option<&str>,
) -> Result<PluginActionResult, String> {
    validate_plugin_id(plugin_id)?;
    let verb = if app == PluginApp::Codex {
        "remove"
    } else {
        "uninstall"
    };
    let mut args = vec!["plugin", verb, plugin_id];
    if app == PluginApp::Codex {
        args.push("--json");
    } else {
        args.extend(["--scope", scope.unwrap_or(PluginScope::User).as_str()]);
    }
    run_cli(app, &args, project_path).await?;
    Ok(action_result(
        format!("{} plugin {verb} {plugin_id}", app.as_str()),
        true,
    ))
}

pub fn update_codex_plugin_enabled_toml(
    raw: &str,
    plugin_id: &str,
    enabled: bool,
) -> Result<String, String> {
    validate_plugin_id(plugin_id)?;
    let mut document = if raw.trim().is_empty() {
        DocumentMut::new()
    } else {
        raw.parse::<DocumentMut>()
            .map_err(|e| format!("TOML parse error: {e}"))?
    };
    if document.get("plugins").is_none() {
        document["plugins"] = toml_edit::table();
    }
    let plugins = document["plugins"]
        .as_table_mut()
        .ok_or_else(|| "Codex plugins config is not a table".to_string())?;
    if !plugins.contains_key(plugin_id) {
        plugins[plugin_id] = toml_edit::table();
    }
    let plugin = plugins[plugin_id]
        .as_table_mut()
        .ok_or_else(|| format!("Codex plugin config is not a table: {plugin_id}"))?;
    plugin["enabled"] = value(enabled);
    Ok(document.to_string())
}

pub async fn set_plugin_enabled(
    app: PluginApp,
    plugin_id: &str,
    enabled: bool,
    scope: Option<PluginScope>,
    project_path: Option<&str>,
) -> Result<PluginActionResult, String> {
    validate_plugin_id(plugin_id)?;
    if app == PluginApp::Claude {
        let verb = if enabled { "enable" } else { "disable" };
        let scope = scope.unwrap_or(PluginScope::User);
        run_cli(
            app,
            &["plugin", verb, plugin_id, "--scope", scope.as_str()],
            project_path,
        )
        .await?;
        return Ok(action_result(
            format!("claude plugin {verb} {plugin_id}"),
            true,
        ));
    }

    let raw = read_and_validate_codex_config_text().map_err(|e| e.to_string())?;
    let updated = update_codex_plugin_enabled_toml(&raw, plugin_id, enabled)?;
    write_codex_live_config_atomic(Some(&updated)).map_err(|e| e.to_string())?;
    Ok(action_result(
        format!("codex config plugins.{plugin_id}.enabled={enabled}"),
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_and_claude_shapes() {
        let codex = serde_json::json!({
            "installed": [{
                "pluginId": "ponytail@ponytail",
                "name": "ponytail",
                "version": "1.0.0",
                "installed": true,
                "enabled": true
            }],
            "available": [{
                "pluginId": "new@market",
                "name": "new",
                "description": "New plugin"
            }]
        })
        .to_string();
        let plugins = parse_plugins_json(PluginApp::Codex, &codex, true).unwrap();
        assert_eq!(plugins.len(), 2);
        assert!(plugins
            .iter()
            .any(|plugin| plugin.plugin_id == "ponytail@ponytail" && plugin.installed));

        let claude = r#"[{
            "id":"ponytail@ponytail",
            "version":"1.0.0",
            "scope":"user",
            "enabled":false,
            "futureField":true
        }]"#;
        let plugins = parse_plugins_json(PluginApp::Claude, claude, false).unwrap();
        assert_eq!(plugins[0].name, "ponytail");
        assert!(plugins[0].supported_actions.enable);
    }

    #[test]
    fn parsers_tolerate_empty_missing_and_future_fields() {
        assert!(parse_plugins_json(PluginApp::Codex, "{}", true)
            .unwrap()
            .is_empty());
        assert!(parse_plugins_json(PluginApp::Claude, "[]", false)
            .unwrap()
            .is_empty());

        let marketplaces = serde_json::json!({
            "marketplaces": [{
                "name": "ponytail",
                "marketplaceSource": {
                    "sourceType": "github",
                    "source": "DietrichGebert/ponytail"
                },
                "futureField": { "ignored": true }
            }, {}]
        })
        .to_string();
        let parsed = parse_marketplaces_json(PluginApp::Claude, &marketplaces).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "ponytail");
        assert_eq!(parsed[0].source.as_deref(), Some("DietrichGebert/ponytail"));
        assert!(parsed[0].supports_refresh);
    }

    #[test]
    fn codex_only_refreshes_git_marketplaces() {
        let marketplaces = serde_json::json!({
            "marketplaces": [
                {
                    "name": "openai-bundled",
                    "marketplaceSource": {
                        "sourceType": "local",
                        "source": "/tmp/openai-bundled"
                    }
                },
                {
                    "name": "openai-curated",
                    "root": "/tmp/plugins"
                },
                {
                    "name": "ponytail",
                    "marketplaceSource": {
                        "sourceType": "git",
                        "source": "https://github.com/DietrichGebert/ponytail.git"
                    }
                }
            ]
        })
        .to_string();

        let parsed = parse_marketplaces_json(PluginApp::Codex, &marketplaces).unwrap();

        assert!(!parsed[0].supports_refresh);
        assert!(!parsed[1].supports_refresh);
        assert!(parsed[2].supports_refresh);
    }

    #[test]
    fn cli_errors_hide_rust_backtraces() {
        let error = concise_cli_error(
            "Error: marketplace is not configured as a Git marketplace\nStack backtrace:\n0: frame",
        );
        assert_eq!(
            error,
            "Error: marketplace is not configured as a Git marketplace"
        );
    }

    #[test]
    fn merges_project_plugin_state_from_its_own_directory() {
        let outside_project = r#"[{
            "id":"ponytail@ponytail",
            "scope":"project",
            "enabled":false,
            "projectPath":"/tmp/the_old_days"
        }]"#;
        let inside_project = r#"[{
            "id":"ponytail@ponytail",
            "scope":"project",
            "enabled":true,
            "projectPath":"/tmp/the_old_days"
        }]"#;
        let mut plugins = parse_plugins_json(PluginApp::Claude, outside_project, false).unwrap();
        let contextual = parse_plugins_json(PluginApp::Claude, inside_project, false).unwrap();

        merge_contextual_plugin_states(&mut plugins, &contextual, "/tmp/the_old_days");

        assert!(plugins[0].enabled);
        assert!(!plugins[0].supported_actions.enable);
        assert!(plugins[0].supported_actions.disable);
    }

    #[test]
    fn codex_toggle_preserves_other_config_and_hooks() {
        let input = r#"# keep me
[plugins."ponytail@ponytail"]
enabled = true

[hooks.state."ponytail@ponytail:hook"]
trusted_hash = "sha256:abc"
"#;
        let output = update_codex_plugin_enabled_toml(input, "ponytail@ponytail", false).unwrap();
        assert!(output.contains("# keep me"));
        assert!(output.contains("enabled = false"));
        assert!(output.contains("trusted_hash = \"sha256:abc\""));
    }

    #[test]
    fn rejects_invalid_plugin_ids_and_toml() {
        assert!(update_codex_plugin_enabled_toml("", "ponytail", true).is_err());
        assert!(
            update_codex_plugin_enabled_toml("not = [toml", "ponytail@ponytail", true).is_err()
        );
    }

    #[tokio::test]
    async fn project_scope_requires_directory_before_running_cli() {
        let error = install_plugin(
            PluginApp::Claude,
            "ponytail@ponytail",
            Some(PluginScope::Project),
            None,
        )
        .await
        .unwrap_err();
        assert!(error.contains("requires a project directory"));
    }
}
