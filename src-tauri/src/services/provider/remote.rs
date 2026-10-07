use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use toml_edit::{DocumentMut, Item};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;

use super::codex_direct;
use crate::codex_config::codex_auth_has_credential_login_material;
use crate::live::patch::toml::{TomlDocPatch, TomlSteps};
use crate::live::patch::{LivePatch, LiveWriteError};
use crate::live::project::claude::{direct_patch as claude_direct_patch, ClaudeProjection};
use crate::live::project::codex::{
    requires_openai_auth, RouteAuth, RouteWrite, OFFICIAL_PROXY_ROUTE_ID, ROUTE_ID,
};
use crate::live::project::gemini::GeminiProjection;

pub(super) const CLAUDE_SETTINGS_PATH: &str = "$HOME/.claude/settings.json";
pub(super) const CODEX_AUTH_PATH: &str = "$HOME/.codex/auth.json";
pub(super) const CODEX_CONFIG_PATH: &str = "$HOME/.codex/config.toml";
pub(super) const GEMINI_ENV_PATH: &str = "$HOME/.gemini/.env";
pub(super) const GEMINI_SETTINGS_PATH: &str = "$HOME/.gemini/settings.json";
/// Written next to config.toml when the provider needs a model catalog; not
/// part of the snapshot.
const CODEX_CATALOG_PATH: &str = "$HOME/.codex/cc-switch-model-catalog.json";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshHostEntry {
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionTarget {
    #[serde(default, rename = "type")]
    pub target_type: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedSshTarget {
    pub(super) label: String,
    pub(super) connect_target: String,
    pub(super) port: Option<u16>,
    pub(super) password: Option<String>,
}

impl ResolvedSshTarget {
    pub(super) fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteApplyResult {
    pub host_alias: String,
    pub app: String,
    pub provider_id: String,
    pub written_files: Vec<String>,
    pub removed_files: Vec<String>,
    pub overwrote_existing_config: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Remote state after the write, so callers don't need another round trip.
    pub remote_state: RemoteProviderState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfigFile {
    pub path: String,
    pub exists: bool,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProviderState {
    pub host_alias: String,
    pub app: String,
    pub provider: Option<Provider>,
    pub matched_provider_id: Option<String>,
    pub files: Vec<RemoteConfigFile>,
    pub has_existing_config: bool,
    pub has_unmanaged_config: bool,
    pub overwrite_warning: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The remote CLI points at the local gateway through the SSH tunnel.
    #[serde(default)]
    pub via_gateway: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImportResult {
    pub host_alias: String,
    pub app: String,
    pub provider: Provider,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProcessInfo {
    pub pid: u32,
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRestartResult {
    pub host_alias: String,
    pub app: String,
    pub stopped: Vec<RemoteProcessInfo>,
    pub force_killed: Vec<u32>,
}

/// Contents of the remote live config files for one app; `None` means missing.
pub(super) struct RemoteSnapshot {
    pub(super) files: Vec<(&'static str, Option<String>)>,
}

impl RemoteSnapshot {
    pub(super) fn get(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|(file_path, _)| *file_path == path)
            .and_then(|(_, content)| content.as_deref())
    }

    pub(super) fn has_any(&self) -> bool {
        self.files.iter().any(|(_, content)| content.is_some())
    }

    fn file_statuses(&self) -> Vec<RemoteConfigFile> {
        self.files
            .iter()
            .map(|(path, content)| RemoteConfigFile {
                path: path.to_string(),
                exists: content.is_some(),
                bytes: content.as_ref().map_or(0, String::len),
            })
            .collect()
    }
}

/// `content: None` removes the remote file.
pub(super) struct RemoteWrite {
    pub(super) path: &'static str,
    pub(super) content: Option<String>,
}

#[derive(Default)]
struct RemoteSettingsRead {
    settings_config: Option<Value>,
    warnings: Vec<String>,
    /// Remote files that exist but couldn't be parsed; their content is left
    /// out of `settings_config`.
    invalid_files: Vec<String>,
}

pub struct RemoteProviderService;

impl RemoteProviderService {
    pub fn list_ssh_hosts() -> Result<Vec<SshHostEntry>, AppError> {
        let config_path = crate::config::get_home_dir().join(".ssh").join("config");
        let mut hosts = Vec::new();
        let mut index = HashMap::new();
        let mut visited = HashSet::new();
        parse_ssh_config_file(&config_path, &mut hosts, &mut index, &mut visited)?;
        Ok(hosts)
    }

    pub fn apply_provider_to_remote(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        target: &SshConnectionTarget,
        force_overwrite: bool,
    ) -> Result<RemoteApplyResult, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let host_alias = target.label();
        let _in_flight = InFlightGuard::acquire(format!("{host_alias}\n{}", app_type.as_str()))
            .ok_or_else(|| AppError::Message(format!("远端 {host_alias} 正在切换中，请稍候")))?;

        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

        let snapshot = read_remote_snapshot(&app_type, &target)?;
        let has_existing_config = snapshot.has_any();
        let via_gateway =
            super::remote_gateway::snapshot_uses_gateway(&state.db, host_alias, &snapshot);
        let prev = if via_gateway {
            None
        } else {
            matched_remote_provider(state, &app_type, &snapshot)?
        };
        if has_existing_config && !force_overwrite && !via_gateway && prev.is_none() {
            return Err(AppError::Message(remote_overwrite_block_message(
                &app_type, host_alias,
            )));
        }

        let writes: Vec<RemoteWrite> =
            build_remote_writes(state, &app_type, prev.as_ref(), provider, &snapshot)?
                .into_iter()
                .filter(|write| write.content.as_deref() != snapshot.get(write.path))
                .filter(|write| write.content.is_some() || snapshot.get(write.path).is_some())
                .collect();
        let (written_files, removed_files) = apply_remote_writes(&target, &writes)?;

        let mut after = snapshot;
        for write in writes {
            if let Some((_, content)) = after.files.iter_mut().find(|(path, _)| *path == write.path)
            {
                *content = write.content;
            }
        }
        let remote_state = remote_state_from_snapshot(state, &app_type, host_alias, &after)?;

        Ok(RemoteApplyResult {
            host_alias: host_alias.to_string(),
            app: app_type.as_str().to_string(),
            provider_id: provider_id.to_string(),
            written_files,
            removed_files,
            overwrote_existing_config: has_existing_config,
            warnings: Vec::new(),
            remote_state,
        })
    }

    pub fn inspect_remote_provider(
        state: &AppState,
        app_type: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteProviderState, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let snapshot = read_remote_snapshot(&app_type, &target)?;
        remote_state_from_snapshot(state, &app_type, target.label(), &snapshot)
    }

    pub fn import_remote_provider(
        state: &AppState,
        app_type: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteImportResult, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let host_alias = target.label();

        let snapshot = read_remote_snapshot(&app_type, &target)?;
        let read = remote_settings_from_snapshot(&app_type, &snapshot)?;
        if !read.invalid_files.is_empty() {
            return Err(AppError::Message(format!(
                "远端 {} 格式损坏，无法同步到本地；请先在服务器上修复该文件",
                read.invalid_files.join("、")
            )));
        }
        let settings_config = read.settings_config.ok_or_else(|| {
            AppError::Message(format!(
                "远端 {host_alias} 没有可导入的 {} 配置",
                app_type.as_str()
            ))
        })?;

        if let Some(existing_id) =
            find_matching_local_provider(state, &app_type, &snapshot, Some(&settings_config))?
        {
            let providers = state.db.get_all_providers(app_type.as_str())?;
            let provider = providers
                .get(&existing_id)
                .cloned()
                .ok_or_else(|| AppError::Message(format!("本地供应商 {existing_id} 不存在")))?;
            return Ok(RemoteImportResult {
                host_alias: host_alias.to_string(),
                app: app_type.as_str().to_string(),
                provider,
            });
        }

        let provider_id = generate_remote_provider_id(state, &app_type, host_alias)?;
        let mut provider = Provider::with_id(
            provider_id,
            format!("远端 {host_alias}"),
            settings_config,
            None,
        );
        provider.category = Some("custom".to_string());
        provider.created_at = Some(chrono::Utc::now().timestamp_millis());
        provider.notes = Some(format!(
            "Downloaded from SSH host {host_alias} for {}",
            app_type.as_str()
        ));

        state.db.save_provider(app_type.as_str(), &provider)?;

        Ok(RemoteImportResult {
            host_alias: host_alias.to_string(),
            app: app_type.as_str().to_string(),
            provider,
        })
    }

    /// Stops every process of the SSH user that belongs to the app (CLI and
    /// IDE-extension binaries), so the next launch picks up the new config.
    pub fn restart_remote_app_processes(
        app_type: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteRestartResult, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;

        let command = format!("sh -s -- {}", app_type.as_str());
        let output = run_ssh_command(
            &target,
            &command,
            Some(REMOTE_STOP_APP_PROCESSES_SCRIPT.as_bytes()),
        )?;
        let (stopped, force_killed) = parse_stop_processes_output(&output);

        Ok(RemoteRestartResult {
            host_alias: target.label().to_string(),
            app: app_type.as_str().to_string(),
            stopped,
            force_killed,
        })
    }
}

/// Run as `sh -s -- <app>`. Matches on the executable name (plus node/bun
/// wrappers of the npm packages) so the pipeline's own awk/sh never match, and
/// skips IDE server/extension-host processes so the editor survives.
const REMOTE_STOP_APP_PROCESSES_SCRIPT: &str = r#"app="$1"
self=$$
targets=$(ps -u "$(id -u)" -o pid= -o comm= -o args= 2>/dev/null | awk -v self="$self" -v app="$app" '
{
  pid = $1
  n = split($2, parts, "/")
  comm = parts[n]
  if (pid == self) next
  args = $0
  sub(/^[ \t]*[0-9]+[ \t]+[^ \t]+[ \t]*/, "", args)
  if (args ~ /(extensionHost|bootstrap-fork|server-main\.js)/) next
  wrapper = (comm ~ /^(node|bun)$/)
  hit = 0
  if (app == "codex") {
    hit = (comm ~ /^codex/) || (wrapper && args ~ /(@openai\/codex|\/codex( |$))/)
  } else if (app == "claude") {
    hit = (comm == "claude") || (wrapper && args ~ /(@anthropic-ai\/claude-code|claude-code\/cli|\/claude( |$))/)
  } else if (app == "gemini") {
    hit = (comm == "gemini") || (wrapper && args ~ /(gemini-cli|\/gemini( |$))/)
  }
  if (hit) printf "%s\t%s\n", pid, args
}')
[ -z "$targets" ] && exit 0
printf '%s\n' "$targets"
pids=$(printf '%s\n' "$targets" | cut -f1)
kill -TERM $pids 2>/dev/null || true
alive=""
i=0
while [ "$i" -lt 5 ]; do
  sleep 1
  alive=""
  for p in $pids; do
    kill -0 "$p" 2>/dev/null && alive="$alive $p"
  done
  [ -z "$alive" ] && break
  i=$((i + 1))
done
if [ -n "$alive" ]; then
  kill -KILL $alive 2>/dev/null || true
  printf '__CC_SWITCH_FORCE_KILLED__%s\n' "$alive"
fi
"#;

fn parse_stop_processes_output(output: &str) -> (Vec<RemoteProcessInfo>, Vec<u32>) {
    const FORCE_MARKER: &str = "__CC_SWITCH_FORCE_KILLED__";
    let mut stopped = Vec::new();
    let mut force_killed = Vec::new();

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix(FORCE_MARKER) {
            force_killed.extend(
                rest.split_whitespace()
                    .filter_map(|pid| pid.parse::<u32>().ok()),
            );
            continue;
        }
        let Some((pid, command)) = line.split_once('\t') else {
            continue;
        };
        if let Ok(pid) = pid.trim().parse() {
            stopped.push(RemoteProcessInfo {
                pid,
                command: command.trim().to_string(),
            });
        }
    }

    (stopped, force_killed)
}

pub(super) fn remote_state_from_snapshot(
    state: &AppState,
    app_type: &AppType,
    host_alias: &str,
    snapshot: &RemoteSnapshot,
) -> Result<RemoteProviderState, AppError> {
    let read = remote_settings_from_snapshot(app_type, snapshot)?;
    let has_existing_config = snapshot.has_any();
    let matched_provider_id =
        find_matching_local_provider(state, app_type, snapshot, read.settings_config.as_ref())?;
    let provider = read.settings_config.map(|settings_config| {
        let mut provider = Provider::with_id(
            "remote-current".to_string(),
            format!("远端当前配置 ({host_alias})"),
            settings_config,
            None,
        );
        provider.category = Some("custom".to_string());
        provider.notes = Some(format!(
            "Imported preview from SSH host {host_alias} for {}",
            app_type.as_str()
        ));
        provider
    });
    let via_gateway = super::remote_gateway::snapshot_uses_gateway(&state.db, host_alias, snapshot);
    let has_unmanaged_config = has_existing_config && matched_provider_id.is_none() && !via_gateway;

    Ok(RemoteProviderState {
        host_alias: host_alias.to_string(),
        app: app_type.as_str().to_string(),
        provider,
        matched_provider_id,
        files: snapshot.file_statuses(),
        has_existing_config,
        has_unmanaged_config,
        overwrite_warning: has_unmanaged_config
            .then(|| remote_overwrite_warning(app_type, host_alias)),
        warnings: read.warnings,
        via_gateway,
    })
}

/// Rejects a second switch to the same host/app while one is still running.
pub(super) struct InFlightGuard(String);

impl InFlightGuard {
    fn in_flight() -> &'static Mutex<HashSet<String>> {
        static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
    }

    pub(super) fn acquire(key: String) -> Option<Self> {
        let mut keys = Self::in_flight().lock().unwrap_or_else(|e| e.into_inner());
        keys.insert(key.clone()).then(|| Self(key))
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let mut keys = Self::in_flight().lock().unwrap_or_else(|e| e.into_inner());
        keys.remove(&self.0);
    }
}

fn remote_overwrite_warning(app_type: &AppType, host_alias: &str) -> String {
    format!(
        "远端 {host_alias} 已有未同步的 {} 配置。切换会替换其中的供应商配置（MCP、项目信任等远端设置会保留），如需保留当前配置请先同步到本地。",
        app_type.as_str()
    )
}

fn remote_overwrite_block_message(app_type: &AppType, host_alias: &str) -> String {
    format!(
        "{} 如确认要覆盖，请重新点击确认。",
        remote_overwrite_warning(app_type, host_alias)
    )
}

pub(super) fn ensure_remote_supported(app_type: &AppType) -> Result<(), AppError> {
    if matches!(app_type, AppType::Claude | AppType::Codex | AppType::Gemini) {
        return Ok(());
    }

    Err(AppError::Message(format!(
        "远端配置暂时只支持 Claude、Codex 和 Gemini，当前应用为 {}",
        app_type.as_str()
    )))
}

pub(super) fn resolve_ssh_target(
    target: &SshConnectionTarget,
) -> Result<ResolvedSshTarget, AppError> {
    let is_manual = target
        .target_type
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case("manual"))
        .unwrap_or(false)
        || target
            .host
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());

    if is_manual {
        return resolve_manual_ssh_target(target);
    }

    let alias = target
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Message("请选择 SSH Host".to_string()))?;
    ensure_known_ssh_host(alias)?;
    Ok(ResolvedSshTarget {
        label: alias.to_string(),
        connect_target: alias.to_string(),
        port: None,
        password: None,
    })
}

fn resolve_manual_ssh_target(target: &SshConnectionTarget) -> Result<ResolvedSshTarget, AppError> {
    let host = target
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Message("请输入 SSH 服务器 IP 或域名".to_string()))?;
    if !is_safe_manual_ssh_host(host) {
        return Err(AppError::Message(
            "SSH 服务器地址包含不受支持的字符".to_string(),
        ));
    }

    let user = target
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(user) = user {
        if !is_safe_ssh_user(user) {
            return Err(AppError::Message(
                "SSH 用户名包含不受支持的字符".to_string(),
            ));
        }
    }

    if matches!(target.port, Some(0)) {
        return Err(AppError::Message("SSH 端口必须在 1-65535 之间".to_string()));
    }

    let connect_target = match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_string(),
    };
    if connect_target.starts_with('-') {
        return Err(AppError::Message("SSH 连接目标不合法".to_string()));
    }

    let mut label = connect_target.clone();
    if let Some(port) = target.port {
        label = format!("{label}:{port}");
    }

    Ok(ResolvedSshTarget {
        label,
        connect_target,
        port: target.port,
        password: target
            .password
            .as_ref()
            .filter(|value| !value.is_empty())
            .cloned(),
    })
}

fn ensure_known_ssh_host(host_alias: &str) -> Result<(), AppError> {
    if !is_safe_ssh_alias(host_alias) {
        return Err(AppError::Message(format!(
            "SSH Host '{host_alias}' 包含不受支持的字符"
        )));
    }

    let hosts = RemoteProviderService::list_ssh_hosts()?;
    if hosts.iter().any(|host| host.alias == host_alias) {
        return Ok(());
    }

    Err(AppError::Message(format!(
        "SSH Host '{host_alias}' 不在 ~/.ssh/config 中"
    )))
}

/// The local provider the remote files currently correspond to.
pub(super) fn matched_remote_provider(
    state: &AppState,
    app_type: &AppType,
    snapshot: &RemoteSnapshot,
) -> Result<Option<Provider>, AppError> {
    if !snapshot.has_any() {
        return Ok(None);
    }
    let read = remote_settings_from_snapshot(app_type, snapshot)?;
    let Some(id) =
        find_matching_local_provider(state, app_type, snapshot, read.settings_config.as_ref())?
    else {
        return Ok(None);
    };
    Ok(state
        .db
        .get_all_providers(app_type.as_str())?
        .shift_remove(&id))
}

/// A local provider matches when it was imported verbatim from the remote, or
/// when pushing it now would leave the remote files unchanged.
fn find_matching_local_provider(
    state: &AppState,
    app_type: &AppType,
    snapshot: &RemoteSnapshot,
    remote_settings: Option<&Value>,
) -> Result<Option<String>, AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;

    if let Some(remote_settings) = remote_settings {
        if let Some(id) = providers
            .iter()
            .find_map(|(id, provider)| (provider.settings_config == *remote_settings).then_some(id))
        {
            return Ok(Some(id.clone()));
        }
    }

    if !snapshot.has_any() {
        return Ok(None);
    }

    let candidates: Vec<(&String, Vec<RemoteWrite>)> = providers
        .iter()
        .filter_map(|(id, provider)| {
            Some((
                id,
                build_remote_writes(state, app_type, None, provider, snapshot).ok()?,
            ))
        })
        .collect();
    if let Some((id, _)) = candidates
        .iter()
        .find(|(_, writes)| remote_writes_match_snapshot(writes, snapshot))
    {
        return Ok(Some((*id).clone()));
    }

    // Codex rewrites its own config.toml (model picked in /model, desktop and
    // plugin state), so fall back to comparing only the endpoint in use.
    if matches!(app_type, AppType::Codex) {
        let remote = codex_endpoint_identity(
            snapshot.get(CODEX_CONFIG_PATH),
            snapshot.get(CODEX_AUTH_PATH),
        );
        if remote.is_some() {
            return Ok(candidates.iter().find_map(|(id, writes)| {
                let written = |path: &str| {
                    writes
                        .iter()
                        .find(|write| write.path == path)
                        .map(|write| write.content.as_deref())
                };
                let config =
                    written(CODEX_CONFIG_PATH).unwrap_or_else(|| snapshot.get(CODEX_CONFIG_PATH));
                let auth =
                    written(CODEX_AUTH_PATH).unwrap_or_else(|| snapshot.get(CODEX_AUTH_PATH));
                (codex_endpoint_identity(config, auth) == remote).then(|| (*id).clone())
            }));
        }
    }

    Ok(None)
}

/// Where a Codex config sends requests and with which credential: the active
/// custom provider's endpoint, or the built-in OpenAI route and its login.
fn codex_endpoint_identity(config_text: Option<&str>, auth_text: Option<&str>) -> Option<Value> {
    let table = toml::from_str::<toml::Table>(config_text.unwrap_or_default()).ok()?;
    let auth = auth_text.and_then(|text| serde_json::from_str::<Value>(text).ok());
    let auth_str = |key: &str| {
        auth.as_ref()
            .and_then(|auth| auth.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let url = |value: Option<&toml::Value>| {
        value
            .and_then(toml::Value::as_str)
            .map(|url| url.trim().trim_end_matches('/').to_string())
    };

    let provider_id = table
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .unwrap_or("openai");
    if let Some(provider) = table
        .get("model_providers")
        .and_then(|providers| providers.get(provider_id))
        .and_then(toml::Value::as_table)
    {
        let key = provider
            .get("experimental_bearer_token")
            .and_then(toml::Value::as_str)
            .map(|token| token.trim().to_string())
            .or_else(|| auth_str("OPENAI_API_KEY"));
        return Some(json!({
            "base_url": url(provider.get("base_url")),
            "wire_api": provider
                .get("wire_api")
                .and_then(toml::Value::as_str)
                .unwrap_or("responses"),
            "key": key,
        }));
    }

    let account = auth
        .as_ref()
        .and_then(|auth| auth.pointer("/tokens/account_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(json!({
        "provider": provider_id,
        "base_url": url(table.get("openai_base_url")),
        "key": auth_str("OPENAI_API_KEY"),
        "account": account,
    }))
}

fn remote_writes_match_snapshot(writes: &[RemoteWrite], snapshot: &RemoteSnapshot) -> bool {
    writes
        .iter()
        .filter(|write| write.path != CODEX_CATALOG_PATH)
        .all(
            |write| match (write.content.as_deref(), snapshot.get(write.path)) {
                (None, None) => true,
                (Some(expected), Some(actual)) => {
                    remote_contents_equivalent(write.path, expected, actual)
                }
                _ => false,
            },
        )
}

fn remote_contents_equivalent(path: &str, expected: &str, actual: &str) -> bool {
    if path.ends_with(".json") {
        if let (Ok(expected), Ok(actual)) = (
            serde_json::from_str::<Value>(expected),
            serde_json::from_str::<Value>(actual),
        ) {
            return expected == actual;
        }
    } else if path.ends_with(".toml") {
        if let (Some(expected), Some(actual)) = (
            codex_config_identity(expected),
            codex_config_identity(actual),
        ) {
            return expected == actual;
        }
    } else if path.ends_with(".env") {
        return crate::gemini_config::parse_env_file(expected)
            == crate::gemini_config::parse_env_file(actual);
    }
    expected.trim() == actual.trim()
}

/// Codex itself records trusted projects and dismissed notices in config.toml,
/// so those tables must not make an otherwise identical config look foreign.
fn codex_config_identity(config_text: &str) -> Option<toml::Table> {
    let mut table = toml::from_str::<toml::Table>(config_text).ok()?;
    table.remove("projects");
    table.remove("notice");
    Some(table)
}

fn generate_remote_provider_id(
    state: &AppState,
    app_type: &AppType,
    host_alias: &str,
) -> Result<String, AppError> {
    let existing_ids = state.db.get_provider_ids(app_type.as_str())?;
    let base = format!(
        "remote-{}-{}",
        app_type.as_str(),
        slugify_id_fragment(host_alias)
    );
    if !existing_ids.contains(&base) {
        return Ok(base);
    }

    for suffix in 2..1000 {
        let candidate = format!("{base}-{suffix}");
        if !existing_ids.contains(&candidate) {
            return Ok(candidate);
        }
    }

    Err(AppError::Message(format!(
        "无法为远端 {host_alias} 生成唯一供应商 ID"
    )))
}

fn slugify_id_fragment(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | '.' | '@' | ':') {
            Some('-')
        } else {
            None
        };

        let Some(next) = next else {
            continue;
        };
        if next == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
            slug.push(next);
        } else {
            last_dash = false;
            slug.push(next);
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "host".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Direct-mode writes for `provider`, patched onto the current remote files
/// the same way a local switch patches the local ones: only the key fields
/// change, the host's own settings stay. `prev` is the provider the remote
/// currently matches; the exclusive fields it brought in are removed.
fn build_remote_writes(
    state: &AppState,
    app_type: &AppType,
    prev: Option<&Provider>,
    provider: &Provider,
    snapshot: &RemoteSnapshot,
) -> Result<Vec<RemoteWrite>, AppError> {
    match app_type {
        AppType::Claude => Ok(vec![claude_remote_write(
            prev,
            &ClaudeProjection::of(&provider.settings_config),
            snapshot,
        )?]),
        AppType::Codex => build_remote_codex_writes(
            state.db.as_ref(),
            prev,
            codex_direct::Target::Direct(Some(provider)),
            None,
            snapshot,
        ),
        AppType::Gemini => {
            gemini_remote_writes(&super::gemini_direct::projection(provider)?, snapshot)
        }
        _ => unreachable!("unsupported app type checked by caller"),
    }
}

pub(super) fn patch_remote_file(
    patch: &dyn LivePatch,
    path: &'static str,
    snapshot: &RemoteSnapshot,
) -> Result<RemoteWrite, AppError> {
    let bytes = patch.apply(Path::new(path), snapshot.get(path).map(str::as_bytes))?;
    let content = String::from_utf8(bytes)
        .map_err(|e| AppError::Message(format!("{path} 写入内容不是 UTF-8: {e}")))?;
    Ok(RemoteWrite {
        path,
        content: Some(content),
    })
}

pub(super) fn claude_remote_write(
    prev: Option<&Provider>,
    target: &ClaudeProjection,
    snapshot: &RemoteSnapshot,
) -> Result<RemoteWrite, AppError> {
    let prev = prev.map(|provider| ClaudeProjection::of(&provider.settings_config));
    let patch = claude_direct_patch(prev.as_ref(), target);
    patch_remote_file(&patch, CLAUDE_SETTINGS_PATH, snapshot)
}

pub(super) fn gemini_remote_writes(
    projection: &GeminiProjection,
    snapshot: &RemoteSnapshot,
) -> Result<Vec<RemoteWrite>, AppError> {
    Ok(vec![
        patch_remote_file(&projection.env_patch(), GEMINI_ENV_PATH, snapshot)?,
        patch_remote_file(&projection.settings_patch(), GEMINI_SETTINGS_PATH, snapshot)?,
    ])
}

/// Runs the local Codex write plan against the remote files. The remote has
/// no login stash or managed accounts, so its own ChatGPT login is never
/// removed. `bearer_token` replaces the proxy placeholder in a proxy route.
pub(super) fn build_remote_codex_writes(
    db: &crate::database::Database,
    prev: Option<&Provider>,
    target: codex_direct::Target<'_>,
    bearer_token: Option<&str>,
    snapshot: &RemoteSnapshot,
) -> Result<Vec<RemoteWrite>, AppError> {
    let owner = prev.map_or(codex_direct::Owner::None, codex_direct::Owner::Provider);
    let planned = codex_direct::plan(db, &owner, &target, &codex_direct::Prepared::default())?;
    let mut config = planned.config;

    let (auth_write, login_on_disk) =
        remote_codex_auth(&planned.auth, snapshot.get(CODEX_AUTH_PATH))?;
    if let RouteWrite::Custom(table) = &mut config.route {
        if let Some(kind @ (RouteAuth::Bearer | RouteAuth::EnvKey)) = planned.stamp {
            table.insert(
                "requires_openai_auth",
                toml_edit::value(requires_openai_auth(kind, login_on_disk)),
            );
        }
        if let Some(token) = bearer_token {
            table.insert("experimental_bearer_token", toml_edit::value(token));
        }
    }

    let keep_id = KeepRemoteRouteId(remote_route_id(snapshot.get(CODEX_CONFIG_PATH)));
    let mut writes: Vec<RemoteWrite> = auth_write.into_iter().collect();
    writes.push(patch_remote_file(
        &TomlSteps(vec![&config, &keep_id]),
        CODEX_CONFIG_PATH,
        snapshot,
    )?);
    if let Some(catalog) = planned.catalog {
        writes.push(RemoteWrite {
            path: CODEX_CATALOG_PATH,
            content: Some(
                String::from_utf8(catalog)
                    .map_err(|e| AppError::Message(format!("Codex 模型目录不是 UTF-8: {e}")))?,
            ),
        });
    }
    Ok(writes)
}

/// Codex files sessions under the route's provider id, so a host that already
/// routes through its own custom table keeps that id instead of `custom`;
/// otherwise its earlier sessions could no longer be resumed.
struct KeepRemoteRouteId(Option<String>);

impl TomlDocPatch for KeepRemoteRouteId {
    fn apply_to(&self, _path: &Path, doc: &mut DocumentMut) -> Result<(), LiveWriteError> {
        let Some(id) = &self.0 else {
            return Ok(());
        };
        if doc.get("model_provider").and_then(Item::as_str) != Some(ROUTE_ID) {
            return Ok(());
        }
        let Some(providers) = doc
            .get_mut("model_providers")
            .and_then(Item::as_table_like_mut)
        else {
            return Ok(());
        };
        let Some(table) = providers.remove(ROUTE_ID) else {
            return Ok(());
        };
        providers.insert(id, table);
        if let Some(Item::Value(selector)) = doc.get_mut("model_provider") {
            let decor = selector.decor().clone();
            *selector = id.as_str().into();
            *selector.decor_mut() = decor;
        }
        Ok(())
    }
}

/// The remote's own custom route id, if it routes through one.
fn remote_route_id(config: Option<&str>) -> Option<String> {
    const NOT_CUSTOM: &[&str] = &[
        ROUTE_ID,
        OFFICIAL_PROXY_ROUTE_ID,
        "openai",
        "ollama",
        "lmstudio",
        "amazon-bedrock",
        "amazon-bedrock-runtime",
    ];
    let doc = config?.parse::<DocumentMut>().ok()?;
    let id = doc.get("model_provider")?.as_str()?.trim();
    if id.is_empty() || NOT_CUSTOM.contains(&id) {
        return None;
    }
    doc.get("model_providers")?
        .as_table_like()?
        .get(id)?
        .as_table_like()?;
    Some(id.to_string())
}

/// What happens to the remote `auth.json`, and whether a ChatGPT login stays
/// on disk afterwards (decides `requires_openai_auth`).
fn remote_codex_auth(
    goal: &codex_direct::AuthGoal,
    remote_auth: Option<&str>,
) -> Result<(Option<RemoteWrite>, bool), AppError> {
    use codex_direct::AuthGoal;

    let remote = remote_auth.and_then(|text| serde_json::from_str::<Value>(text).ok());
    let remote_login = remote
        .as_ref()
        .is_some_and(codex_auth_has_credential_login_material);
    Ok(match goal {
        AuthGoal::Official(auth) | AuthGoal::Managed(auth)
            if codex_auth_has_credential_login_material(auth) =>
        {
            let write = RemoteWrite {
                path: CODEX_AUTH_PATH,
                content: Some(json_pretty(auth)?),
            };
            (Some(write), true)
        }
        // The key now lives in the route table; a key-only auth.json left
        // behind would be sent nowhere and only confuses Codex's login screen.
        AuthGoal::ThirdParty if remote.is_some() && !remote_login => (
            Some(RemoteWrite {
                path: CODEX_AUTH_PATH,
                content: None,
            }),
            false,
        ),
        _ => (None, remote_login),
    })
}

pub(super) fn remote_config_paths(app_type: &AppType) -> &'static [&'static str] {
    match app_type {
        AppType::Claude => &[CLAUDE_SETTINGS_PATH],
        AppType::Codex => &[CODEX_AUTH_PATH, CODEX_CONFIG_PATH],
        AppType::Gemini => &[GEMINI_ENV_PATH, GEMINI_SETTINGS_PATH],
        _ => &[],
    }
}

pub(super) fn read_remote_snapshot(
    app_type: &AppType,
    target: &ResolvedSshTarget,
) -> Result<RemoteSnapshot, AppError> {
    let paths = remote_config_paths(app_type);
    let script = build_read_files_script(paths);
    let output = run_ssh_command_bytes(target, "sh -s", Some(script.as_bytes()))?;
    let contents = parse_read_files_output(&output, paths.len())?;
    Ok(RemoteSnapshot {
        files: paths.iter().copied().zip(contents).collect(),
    })
}

const REMOTE_FILE_HEADER: &str = "__CC_SWITCH_FILE__ ";

/// Prints `<header> <bytes>\n<content>` per file (`-` when missing), so all
/// files come back in one SSH round trip and are split by exact byte length.
/// Each file is copied once and both its size and bytes come from the copy, so
/// a concurrent atomic replace (Codex refreshing auth.json, another switch)
/// can't make the declared size disagree with the content.
fn build_read_files_script(paths: &[&str]) -> String {
    let mut script = String::from(
        "umask 077\nt=$(mktemp \"${TMPDIR:-/tmp}/cc-switch-read.XXXXXX\") || exit 1\ntrap 'rm -f \"$t\"' EXIT\n",
    );
    for path in paths {
        script.push_str(&format!(
            "f=\"{path}\"; if [ -f \"$f\" ] && cat \"$f\" > \"$t\" 2>/dev/null; then printf '{REMOTE_FILE_HEADER}%s\\n' \"$(wc -c < \"$t\" | tr -d ' ')\"; cat \"$t\"; else printf '{REMOTE_FILE_HEADER}-\\n'; fi\n"
        ));
    }
    script
}

fn parse_read_files_output(output: &[u8], count: usize) -> Result<Vec<Option<String>>, AppError> {
    let invalid = || AppError::Message("读取远端配置失败: 返回内容格式异常".to_string());
    let header = REMOTE_FILE_HEADER.as_bytes();
    let mut rest = output;
    let mut files = Vec::with_capacity(count);

    for _ in 0..count {
        let start = rest
            .windows(header.len())
            .position(|window| window == header)
            .ok_or_else(invalid)?;
        rest = &rest[start + header.len()..];
        let line_end = rest.iter().position(|b| *b == b'\n').ok_or_else(invalid)?;
        let size = std::str::from_utf8(&rest[..line_end])
            .map_err(|_| invalid())?
            .trim();
        rest = &rest[line_end + 1..];

        if size == "-" {
            files.push(None);
            continue;
        }
        let size: usize = size.parse().map_err(|_| invalid())?;
        if rest.len() < size {
            return Err(invalid());
        }
        files.push(Some(String::from_utf8_lossy(&rest[..size]).into_owned()));
        rest = &rest[size..];
    }

    Ok(files)
}

/// Applies every write in one SSH round trip; each file is replaced atomically.
pub(super) fn apply_remote_writes(
    target: &ResolvedSshTarget,
    writes: &[RemoteWrite],
) -> Result<(Vec<String>, Vec<String>), AppError> {
    if writes.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let script = build_write_files_script(writes)?;
    let output = run_ssh_command(target, "sh -s", Some(script.as_bytes()))?;

    let mut written = Vec::new();
    let mut removed = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("W ") {
            written.push(path.to_string());
        } else if let Some(path) = line.strip_prefix("R ") {
            removed.push(path.to_string());
        }
    }
    Ok((written, removed))
}

fn build_write_files_script(writes: &[RemoteWrite]) -> Result<String, AppError> {
    let mut script = String::from(
        "set -e\numask 077\nwrite_file() { file=\"$1\"; mkdir -p \"${file%/*}\"; tmp=\"$file.tmp.$$\"; printf '%s' \"$2\" > \"$tmp\"; mv \"$tmp\" \"$file\"; chmod 600 \"$file\" 2>/dev/null || true; printf 'W %s\\n' \"$file\"; }\n",
    );
    for write in writes {
        match &write.content {
            Some(content) => {
                if content.contains('\0') {
                    return Err(AppError::Message(format!(
                        "{} 含有 NUL 字符，无法写入远端",
                        write.path
                    )));
                }
                script.push_str(&format!(
                    "write_file \"{}\" {}\n",
                    write.path,
                    shell_single_quote(content)
                ));
            }
            None => script.push_str(&format!(
                "rm -f \"{0}\"; printf 'R %s\\n' \"{0}\"\n",
                write.path
            )),
        }
    }
    Ok(script)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn remote_settings_from_snapshot(
    app_type: &AppType,
    snapshot: &RemoteSnapshot,
) -> Result<RemoteSettingsRead, AppError> {
    match app_type {
        AppType::Claude => {
            let Some(content) = snapshot.get(CLAUDE_SETTINGS_PATH) else {
                return Ok(RemoteSettingsRead {
                    settings_config: None,
                    warnings: vec!["远端未找到 Claude Code settings.json".to_string()],
                    invalid_files: Vec::new(),
                });
            };

            let mut read = RemoteSettingsRead::default();
            read.settings_config = read.parse_json(CLAUDE_SETTINGS_PATH, content);
            Ok(read)
        }
        AppType::Codex => {
            let auth_content = snapshot.get(CODEX_AUTH_PATH);
            let config_content = snapshot.get(CODEX_CONFIG_PATH);

            let mut warnings = Vec::new();
            if auth_content.is_none() {
                warnings.push("远端未找到 Codex auth.json".to_string());
            }
            if config_content.is_none() {
                warnings.push("远端未找到 Codex config.toml".to_string());
            }
            let mut read = RemoteSettingsRead {
                warnings,
                ..Default::default()
            };
            if auth_content.is_none() && config_content.is_none() {
                return Ok(read);
            }

            let auth = auth_content
                .and_then(|content| read.parse_json(CODEX_AUTH_PATH, content))
                .unwrap_or_else(|| json!({}));
            read.settings_config =
                Some(json!({ "auth": auth, "config": config_content.unwrap_or_default() }));
            Ok(read)
        }
        AppType::Gemini => {
            let env_content = snapshot.get(GEMINI_ENV_PATH);
            let settings_content = snapshot.get(GEMINI_SETTINGS_PATH);

            if env_content.is_none() && settings_content.is_none() {
                return Ok(RemoteSettingsRead {
                    settings_config: None,
                    warnings: vec!["远端未找到 Gemini .env 或 settings.json".to_string()],
                    invalid_files: Vec::new(),
                });
            }

            let mut warnings = Vec::new();
            if env_content.is_none() {
                warnings.push("远端未找到 Gemini .env".to_string());
            }
            if settings_content.is_none() {
                warnings.push("远端未找到 Gemini settings.json".to_string());
            }

            let env_map = env_content
                .map(crate::gemini_config::parse_env_file)
                .unwrap_or_default();
            let env_json = crate::gemini_config::env_to_json(&env_map);
            let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));
            let mut read = RemoteSettingsRead {
                warnings,
                ..Default::default()
            };
            let settings = settings_content
                .and_then(|content| read.parse_json(GEMINI_SETTINGS_PATH, content))
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            read.settings_config = Some(json!({ "env": env_obj, "config": settings }));
            Ok(read)
        }
        _ => unreachable!("unsupported app type checked by caller"),
    }
}

impl RemoteSettingsRead {
    /// A broken remote file (e.g. a hand edit) becomes a warning instead of
    /// failing the whole inspection.
    fn parse_json(&mut self, path: &str, content: &str) -> Option<Value> {
        let display = path.replace("$HOME", "~");
        match serde_json::from_str::<Value>(content) {
            Ok(value) if value.is_object() => Some(value),
            Ok(_) => {
                self.warnings
                    .push(format!("远端 {display} 不是 JSON 对象，已忽略其内容"));
                self.invalid_files.push(display);
                None
            }
            Err(e) => {
                self.warnings.push(format!(
                    "远端 {display} 不是合法 JSON（{e}），已忽略其内容；请在服务器上修复或删除该文件"
                ));
                self.invalid_files.push(display);
                None
            }
        }
    }
}

pub(super) fn json_pretty(value: &Value) -> Result<String, AppError> {
    serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })
}

pub(super) fn run_ssh_command(
    target: &ResolvedSshTarget,
    remote_command: &str,
    stdin: Option<&[u8]>,
) -> Result<String, AppError> {
    let output = run_ssh_command_bytes(target, remote_command, stdin)?;
    Ok(String::from_utf8_lossy(&output).into_owned())
}

fn run_ssh_command_bytes(
    target: &ResolvedSshTarget,
    remote_command: &str,
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, AppError> {
    let mut command = Command::new("ssh");
    hide_ssh_console_window(&mut command);
    configure_ssh_command(&mut command, target, remote_command);
    let _askpass = configure_ssh_password(&mut command, target)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = command
        .spawn()
        .map_err(|e| AppError::Message(format!("启动 ssh 失败: {e}")))?;

    if let Some(input) = stdin {
        let Some(mut child_stdin) = child.stdin.take() else {
            return Err(AppError::Message("无法写入 ssh stdin".to_string()));
        };
        child_stdin
            .write_all(input)
            .map_err(|e| AppError::Message(format!("写入 ssh stdin 失败: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| AppError::Message(format!("等待 ssh 结束失败: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Message(if stderr.is_empty() {
            format!("ssh 命令失败，退出码: {}", output.status)
        } else {
            format!("ssh 命令失败: {stderr}")
        }));
    }

    Ok(output.stdout)
}

#[cfg(windows)]
fn hide_ssh_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_ssh_console_window(_command: &mut Command) {}

fn configure_ssh_command(command: &mut Command, target: &ResolvedSshTarget, remote_command: &str) {
    command.arg("-C");
    command.args(["-o", "ConnectTimeout=10", "-o", "ClearAllForwardings=yes"]);
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }

    if target.password.is_some() {
        command.args([
            "-o",
            "BatchMode=no",
            "-o",
            "NumberOfPasswordPrompts=1",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ]);
    } else {
        command.args(["-o", "BatchMode=yes", "-o", "NumberOfPasswordPrompts=0"]);
    }
    configure_ssh_multiplexing(command);

    command
        .arg("--")
        .arg(&target.connect_target)
        .arg(remote_command);
}

pub(super) fn configure_ssh_password(
    command: &mut Command,
    target: &ResolvedSshTarget,
) -> Result<Option<tempfile::NamedTempFile>, AppError> {
    let Some(password) = target.password.as_deref() else {
        return Ok(None);
    };

    let mut file = tempfile::Builder::new()
        .prefix("cc-switch-ssh-askpass-")
        .suffix(askpass_script_suffix())
        .tempfile()
        .map_err(|e| AppError::Message(format!("创建 SSH 密码辅助脚本失败: {e}")))?;
    file.write_all(askpass_script_content().as_bytes())
        .map_err(|e| AppError::Message(format!("写入 SSH 密码辅助脚本失败: {e}")))?;

    #[cfg(unix)]
    {
        let mut permissions = file
            .as_file()
            .metadata()
            .map_err(|e| AppError::Message(format!("读取 SSH 密码辅助脚本权限失败: {e}")))?
            .permissions();
        permissions.set_mode(0o700);
        file.as_file()
            .set_permissions(permissions)
            .map_err(|e| AppError::Message(format!("设置 SSH 密码辅助脚本权限失败: {e}")))?;
    }

    command.env("SSH_ASKPASS", file.path());
    command.env("SSH_ASKPASS_REQUIRE", "force");
    command.env("CC_SWITCH_SSH_PASSWORD", password);
    if std::env::var_os("DISPLAY").is_none() {
        command.env("DISPLAY", "cc-switch");
    }

    Ok(Some(file))
}

#[cfg(unix)]
fn askpass_script_suffix() -> &'static str {
    ".sh"
}

#[cfg(not(unix))]
fn askpass_script_suffix() -> &'static str {
    ".cmd"
}

#[cfg(unix)]
fn askpass_script_content() -> &'static str {
    "#!/bin/sh\nprintf '%s\\n' \"$CC_SWITCH_SSH_PASSWORD\"\n"
}

#[cfg(not(unix))]
fn askpass_script_content() -> &'static str {
    "@echo off\r\necho %CC_SWITCH_SSH_PASSWORD%\r\n"
}

/// Reuses one SSH connection per host for a couple of minutes, so reading,
/// writing and re-inspecting don't each pay for a full handshake.
#[cfg(unix)]
fn configure_ssh_multiplexing(command: &mut Command) {
    let dir = crate::config::get_home_dir().join(".ssh");
    configure_ssh_multiplexing_at(command, &dir);
}

#[cfg(unix)]
fn configure_ssh_multiplexing_at(command: &mut Command, dir: &Path) {
    if !dir.is_dir() {
        if fs::create_dir_all(dir).is_err() {
            return;
        }
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    // ssh splits option values on whitespace; skip rather than risk a bad path.
    let control_path = dir.join("cc-switch-%C");
    let control_path = control_path.to_string_lossy();
    // %C expands to 40 bytes, and OpenSSH appends a 17-byte temporary suffix
    // when creating the listener. macOS sun_path has room for 104 bytes,
    // including the terminator. Fall back to a fresh connection for long paths.
    if control_path.chars().any(char::is_whitespace) || control_path.len() - 2 + 40 + 17 >= 104 {
        command.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
        return;
    }
    command.args([
        "-o".to_string(),
        "ControlMaster=auto".to_string(),
        "-o".to_string(),
        format!("ControlPath={control_path}"),
        "-o".to_string(),
        "ControlPersist=120".to_string(),
    ]);
}

// Windows OpenSSH has no ControlMaster support.
#[cfg(not(unix))]
fn configure_ssh_multiplexing(_command: &mut Command) {}

fn is_safe_ssh_alias(alias: &str) -> bool {
    !alias.trim().is_empty()
        && !alias.starts_with('-')
        && alias
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@' | ':'))
}

fn is_safe_manual_ssh_host(host: &str) -> bool {
    !host.trim().is_empty()
        && !host.starts_with('-')
        && !host.contains('@')
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '[' | ']'))
}

fn is_safe_ssh_user(user: &str) -> bool {
    !user.trim().is_empty()
        && !user.starts_with('-')
        && user
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn parse_ssh_config_file(
    path: &Path,
    hosts: &mut Vec<SshHostEntry>,
    index: &mut HashMap<String, usize>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    parse_ssh_config_content(&content, path, hosts, index, visited)
}

fn parse_ssh_config_content(
    content: &str,
    source_path: &Path,
    hosts: &mut Vec<SshHostEntry>,
    index: &mut HashMap<String, usize>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut current_aliases: Vec<String> = Vec::new();
    let source = source_path.to_string_lossy().to_string();
    let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));

    for raw_line in content.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        let values: Vec<&str> = parts.collect();
        if values.is_empty() {
            continue;
        }

        match keyword.to_ascii_lowercase().as_str() {
            "include" => {
                for pattern in values {
                    for include_path in expand_include_pattern(pattern, base_dir)? {
                        parse_ssh_config_file(&include_path, hosts, index, visited)?;
                    }
                }
            }
            "host" => {
                current_aliases = values
                    .into_iter()
                    .filter_map(normalize_host_alias)
                    .collect::<Vec<_>>();
                for alias in &current_aliases {
                    if index.contains_key(alias) {
                        continue;
                    }
                    let position = hosts.len();
                    index.insert(alias.clone(), position);
                    hosts.push(SshHostEntry {
                        alias: alias.clone(),
                        host_name: None,
                        user: None,
                        port: None,
                        source: Some(source.clone()),
                    });
                }
            }
            "hostname" | "user" | "port" => {
                let value = values.join(" ");
                for alias in &current_aliases {
                    let Some(position) = index.get(alias).copied() else {
                        continue;
                    };
                    let host = &mut hosts[position];
                    match keyword.to_ascii_lowercase().as_str() {
                        "hostname" if host.host_name.is_none() => {
                            host.host_name = Some(value.clone())
                        }
                        "user" if host.user.is_none() => host.user = Some(value.clone()),
                        "port" if host.port.is_none() => host.port = Some(value.clone()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn normalize_host_alias(raw: &str) -> Option<String> {
    let alias = raw.trim().trim_matches('"').trim_matches('\'');
    if alias.is_empty()
        || alias.contains('*')
        || alias.contains('?')
        || alias.starts_with('!')
        || !is_safe_ssh_alias(alias)
    {
        return None;
    }
    Some(alias.to_string())
}

fn expand_include_pattern(pattern: &str, base_dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let path = expand_ssh_path(pattern, base_dir);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    if !file_name.contains('*') && !file_name.contains('?') {
        return Ok(vec![path]);
    }

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(Vec::new()),
    };

    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if wildcard_match(&file_name, &name) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn expand_ssh_path(raw: &str, base_dir: &Path) -> PathBuf {
    let expanded = if raw == "~" {
        crate::config::get_home_dir()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        crate::config::get_home_dir().join(rest)
    } else {
        PathBuf::from(raw)
    };

    if expanded.is_relative() {
        base_dir.join(expanded)
    } else {
        expanded
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match (pattern.first(), value.first()) {
            (None, None) => true,
            (None, Some(_)) => false,
            (Some(b'*'), _) => {
                inner(&pattern[1..], value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            (Some(b'?'), Some(_)) => inner(&pattern[1..], &value[1..]),
            (Some(a), Some(b)) if a == b => inner(&pattern[1..], &value[1..]),
            _ => false,
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_endpoint_identity_ignores_model_and_host_state() {
        let local = r#"model_provider = "custom"
model = "gpt-5.4"
notify = ["/Applications/Codex.app/notify"]

[desktop]
dock-icon-preference = "auto"

[model_providers.custom]
name = "pro"
base_url = "https://api.example.com/v1/"
wire_api = "responses"
experimental_bearer_token = "sk-1"
"#;
        let remote = r#"model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"

[model_providers.custom]
name = "Remote"
base_url = "https://api.example.com/v1"
experimental_bearer_token = "sk-1"
"#;
        assert_eq!(
            codex_endpoint_identity(Some(local), None),
            codex_endpoint_identity(Some(remote), None)
        );
        let other_key = remote.replace("sk-1", "sk-2");
        assert_ne!(
            codex_endpoint_identity(Some(local), None),
            codex_endpoint_identity(Some(&other_key), None)
        );
    }

    #[test]
    fn claude_remote_write_replaces_key_fields_and_keeps_host_settings() {
        let remote = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://old.example.com",
                "ANTHROPIC_API_KEY": "old-key",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "old-opus",
                "HTTPS_PROXY": "http://proxy:3128"
            },
            "permissions": { "allow": ["Bash(ls)"] },
            "hooks": { "Stop": [] },
            "modelPicker": {
                "replaceBuiltInOptions": true,
                "options": [{ "model": "ccs-claude-old--old-model" }]
            }
        });
        let snapshot = RemoteSnapshot {
            files: vec![(CLAUDE_SETTINGS_PATH, Some(remote.to_string()))],
        };
        let target = ClaudeProjection::of(&json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://new.example.com",
                "ANTHROPIC_AUTH_TOKEN": "new-token",
                "ANTHROPIC_MODEL": "new-model",
                "CLAUDE_CODE_AUTO_MODE_SERVER": "0"
            }
        }));

        let write = claude_remote_write(None, &target, &snapshot).unwrap();
        let written: Value = serde_json::from_str(write.content.as_deref().unwrap()).unwrap();

        assert_eq!(
            written["env"]["ANTHROPIC_BASE_URL"],
            "https://new.example.com"
        );
        assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], "new-token");
        assert_eq!(written["env"]["ANTHROPIC_MODEL"], "new-model");
        assert_eq!(written["env"]["CLAUDE_CODE_AUTO_MODE_SERVER"], "0");
        assert!(written["env"].get("ANTHROPIC_API_KEY").is_none());
        assert!(written["env"].get("ANTHROPIC_DEFAULT_OPUS_MODEL").is_none());
        assert_eq!(written["env"]["HTTPS_PROXY"], "http://proxy:3128");
        assert_eq!(written["permissions"], remote["permissions"]);
        assert_eq!(written["hooks"], remote["hooks"]);
        assert!(written.get("modelPicker").is_none());
    }

    #[test]
    fn remote_codex_catalog_replaces_foreign_pointer_and_keeps_login_and_route_id() {
        let db = crate::database::Database::memory().unwrap();
        let remote = r#"model_provider = "remote-route"
model_catalog_json = "/opt/other-tool/models.json"

[model_providers.remote-route]
name = "old"
base_url = "https://old.example.com/v1"

[mcp_servers.tool]
command = "tool"
"#;
        let login = json!({ "tokens": { "access_token": "remote-login" } }).to_string();
        let snapshot = RemoteSnapshot {
            files: vec![
                (CODEX_CONFIG_PATH, Some(remote.to_string())),
                (CODEX_AUTH_PATH, Some(login)),
                (CODEX_CATALOG_PATH, None),
            ],
        };
        let mut provider = Provider::with_id(
            "new".to_string(),
            "New".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "new-key" },
                "config": "model_provider = \"custom\"\nmodel = \"gpt-5.5\"\n[model_providers.custom]\nbase_url = \"https://new.example.com/v1\"\n",
                "modelCatalog": { "models": [{ "model": "gpt-5.5" }] }
            }),
            None,
        );
        for with_catalog in [true, false] {
            if !with_catalog {
                provider
                    .settings_config
                    .as_object_mut()
                    .unwrap()
                    .remove("modelCatalog");
            }
            let writes = build_remote_codex_writes(
                &db,
                None,
                codex_direct::Target::Direct(Some(&provider)),
                None,
                &snapshot,
            )
            .unwrap();
            assert!(!writes.iter().any(|write| write.path == CODEX_AUTH_PATH));
            let config = writes
                .iter()
                .find(|write| write.path == CODEX_CONFIG_PATH)
                .unwrap();
            let doc = config
                .content
                .as_deref()
                .unwrap()
                .parse::<DocumentMut>()
                .unwrap();
            assert_eq!(doc["model_provider"].as_str(), Some("remote-route"));
            assert_eq!(doc["mcp_servers"]["tool"]["command"].as_str(), Some("tool"));
            if with_catalog {
                assert_eq!(
                    doc["model_catalog_json"].as_str(),
                    Some(crate::codex_config::CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
                );
                assert!(writes.iter().any(|write| write.path == CODEX_CATALOG_PATH));
            } else {
                assert!(doc.get("model_catalog_json").is_none());
            }
        }
    }

    #[test]
    fn claude_remote_write_refuses_broken_settings() {
        let snapshot = RemoteSnapshot {
            files: vec![(CLAUDE_SETTINGS_PATH, Some("{\"env\": {}}}".to_string()))],
        };
        assert!(claude_remote_write(None, &ClaudeProjection::default(), &snapshot).is_err());
    }

    #[test]
    fn gemini_remote_write_preserves_host_settings() {
        let snapshot = RemoteSnapshot {
            files: vec![
                (GEMINI_ENV_PATH, Some("# host\nGEMINI_API_KEY=old-key\nGEMINI_SANDBOX=docker\n".to_string())),
                (GEMINI_SETTINGS_PATH, Some(json!({
                    "security": { "auth": { "selectedType": "oauth-personal", "useExternal": true } },
                    "model": { "name": "old-model", "compressionThreshold": 0.5 },
                    "mcpServers": { "tool": { "command": "host-tool" } }
                }).to_string())),
            ],
        };
        let target = GeminiProjection::of(
            &json!({
                "env": {
                    "GEMINI_API_KEY": "new-key",
                    "GOOGLE_GEMINI_BASE_URL": "https://new.example.com"
                },
                "config": { "model": { "name": "new-model" } }
            }),
            false,
        );
        let writes = gemini_remote_writes(&target, &snapshot).unwrap();
        let env = writes
            .iter()
            .find(|write| write.path == GEMINI_ENV_PATH)
            .unwrap()
            .content
            .as_deref()
            .unwrap();
        assert!(env.contains("GEMINI_API_KEY=new-key"));
        assert!(env.contains("GOOGLE_GEMINI_BASE_URL=https://new.example.com"));
        assert!(!env.contains("old-key"));
        assert!(env.contains("# host"));
        assert!(env.contains("GEMINI_SANDBOX=docker"));
        let settings: Value = serde_json::from_str(
            writes
                .iter()
                .find(|write| write.path == GEMINI_SETTINGS_PATH)
                .unwrap()
                .content
                .as_deref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["security"]["auth"]["selectedType"],
            "gemini-api-key"
        );
        assert_eq!(settings["security"]["auth"]["useExternal"], true);
        assert_eq!(settings["model"]["name"], "new-model");
        assert_eq!(settings["model"]["compressionThreshold"], 0.5);
        assert_eq!(settings["mcpServers"]["tool"]["command"], "host-tool");
    }

    #[test]
    fn remote_codex_auth_never_drops_a_remote_login() {
        use codex_direct::AuthGoal;

        let login = json!({ "tokens": { "access_token": "at", "account_id": "acc" } }).to_string();
        let (write, login_on_disk) =
            remote_codex_auth(&AuthGoal::ThirdParty, Some(&login)).unwrap();
        assert!(write.is_none());
        assert!(login_on_disk);

        let key_only = json!({ "OPENAI_API_KEY": "sk-old" }).to_string();
        let (write, login_on_disk) =
            remote_codex_auth(&AuthGoal::ThirdParty, Some(&key_only)).unwrap();
        assert!(write.is_some_and(|write| write.content.is_none()));
        assert!(!login_on_disk);

        let (write, _) = remote_codex_auth(&AuthGoal::ThirdParty, Some("{ broken")).unwrap();
        assert!(write.is_none());

        let row = json!({ "tokens": { "refresh_token": "rt" } });
        let (write, login_on_disk) =
            remote_codex_auth(&AuthGoal::Official(row), Some(&key_only)).unwrap();
        assert!(write.unwrap().content.unwrap().contains("refresh_token"));
        assert!(login_on_disk);

        let (write, login_on_disk) =
            remote_codex_auth(&AuthGoal::Official(json!({})), Some(&login)).unwrap();
        assert!(write.is_none());
        assert!(login_on_disk);
    }

    #[test]
    fn remote_codex_route_keeps_the_hosts_provider_id() {
        let remote = r#"model_provider = "OpenAI" # remote bucket
model = "gpt-5.5"

[model_providers.OpenAI]
name = "old"
base_url = "https://old.example.com/v1"
experimental_bearer_token = "sk-old"

[mcp_servers.tool]
command = "tool"
"#;
        let mut doc = remote.parse::<DocumentMut>().unwrap();
        doc["model_provider"] = toml_edit::value(ROUTE_ID);
        let mut table = toml_edit::Table::new();
        table.insert("name", toml_edit::value("new"));
        table.insert("base_url", toml_edit::value("https://new.example.com/v1"));
        doc["model_providers"]
            .as_table_like_mut()
            .unwrap()
            .insert(ROUTE_ID, toml_edit::Item::Table(table));

        KeepRemoteRouteId(remote_route_id(Some(remote)))
            .apply_to(Path::new(CODEX_CONFIG_PATH), &mut doc)
            .unwrap();

        assert_eq!(doc["model_provider"].as_str(), Some("OpenAI"));
        assert_eq!(
            doc["model_providers"]["OpenAI"]["base_url"].as_str(),
            Some("https://new.example.com/v1")
        );
        assert!(doc["model_providers"].get(ROUTE_ID).is_none());
        assert!(!doc.to_string().contains("sk-old"));
        assert_eq!(doc["mcp_servers"]["tool"]["command"].as_str(), Some("tool"));

        assert_eq!(remote_route_id(Some("model_provider = \"openai\"\n")), None);
        assert_eq!(remote_route_id(Some("model_provider = \"x\"\n")), None);
    }

    #[test]
    fn broken_remote_auth_json_is_a_warning() {
        let snapshot = RemoteSnapshot {
            files: vec![
                (
                    CODEX_AUTH_PATH,
                    Some("{\n  \"OPENAI_API_KEY\": \"x\"\n}}\n".to_string()),
                ),
                (CODEX_CONFIG_PATH, Some("model = \"m\"\n".to_string())),
            ],
        };
        let read = remote_settings_from_snapshot(&AppType::Codex, &snapshot).unwrap();
        assert_eq!(read.invalid_files, vec!["~/.codex/auth.json".to_string()]);
        assert!(read.warnings.iter().any(|w| w.contains("auth.json")));
        assert_eq!(read.settings_config.unwrap()["config"], "model = \"m\"\n");
    }

    #[test]
    fn parse_ssh_config_collects_plain_hosts_and_metadata() {
        let content = r#"
Host *
  ServerAliveInterval 30

Host dev prod-box
  HostName 10.0.0.2
  User deploy
  Port 2200

Host ignored-*
  HostName ignored.example.com

Host "quoted"
  HostName quoted.example.com
"#;
        let mut hosts = Vec::new();
        let mut index = HashMap::new();
        let mut visited = HashSet::new();
        parse_ssh_config_content(
            content,
            Path::new("/tmp/ssh-config"),
            &mut hosts,
            &mut index,
            &mut visited,
        )
        .unwrap();

        assert_eq!(
            hosts
                .iter()
                .map(|host| host.alias.as_str())
                .collect::<Vec<_>>(),
            vec!["dev", "prod-box", "quoted"]
        );
        assert_eq!(hosts[0].host_name.as_deref(), Some("10.0.0.2"));
        assert_eq!(hosts[0].user.as_deref(), Some("deploy"));
        assert_eq!(hosts[0].port.as_deref(), Some("2200"));
    }

    #[test]
    fn wildcard_match_supports_star_and_question_mark() {
        assert!(wildcard_match("conf.d/*", "conf.d/dev"));
        assert!(wildcard_match("host?.conf", "host1.conf"));
        assert!(!wildcard_match("host?.conf", "host12.conf"));
    }

    #[test]
    fn resolve_manual_ssh_target_builds_label_without_saving_password() -> Result<(), AppError> {
        let target = SshConnectionTarget {
            target_type: Some("manual".to_string()),
            alias: None,
            host: Some("10.0.0.2".to_string()),
            user: Some("deploy".to_string()),
            port: Some(2200),
            password: Some("secret".to_string()),
        };

        let resolved = resolve_ssh_target(&target)?;

        assert_eq!(resolved.label, "deploy@10.0.0.2:2200");
        assert_eq!(resolved.connect_target, "deploy@10.0.0.2");
        assert_eq!(resolved.port, Some(2200));
        assert_eq!(resolved.password.as_deref(), Some("secret"));
        Ok(())
    }

    #[test]
    fn resolve_manual_ssh_target_rejects_unsafe_host() {
        let target = SshConnectionTarget {
            target_type: Some("manual".to_string()),
            alias: None,
            host: Some("-oProxyCommand=bad".to_string()),
            user: Some("deploy".to_string()),
            port: Some(22),
            password: None,
        };

        assert!(resolve_ssh_target(&target).is_err());
    }

    #[test]
    fn remote_writes_match_ignores_codex_runtime_tables_and_json_formatting() {
        let snapshot = RemoteSnapshot {
            files: vec![
                (CODEX_AUTH_PATH, None),
                (
                    CODEX_CONFIG_PATH,
                    Some(
                        "model = \"gpt-5.5\"\n\n[projects.\"/srv/app\"]\ntrust_level = \"trusted\"\n"
                            .to_string(),
                    ),
                ),
                (CLAUDE_SETTINGS_PATH, Some("{\"env\":{\"A\":\"1\"}}".to_string())),
            ],
        };
        let writes = vec![
            RemoteWrite {
                path: CODEX_AUTH_PATH,
                content: None,
            },
            RemoteWrite {
                path: CODEX_CONFIG_PATH,
                content: Some("model = \"gpt-5.5\"\n".to_string()),
            },
            RemoteWrite {
                path: CLAUDE_SETTINGS_PATH,
                content: Some("{\n  \"env\": {\n    \"A\": \"1\"\n  }\n}".to_string()),
            },
        ];

        assert!(remote_writes_match_snapshot(&writes, &snapshot));

        let mismatched = [RemoteWrite {
            path: CODEX_CONFIG_PATH,
            content: Some("model = \"gpt-5.4\"\n".to_string()),
        }];
        assert!(!remote_writes_match_snapshot(&mismatched, &snapshot));
    }

    #[cfg(unix)]
    fn run_local_sh(home: &Path, script: &str) -> Vec<u8> {
        let mut child = Command::new("sh")
            .arg("-s")
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        output.stdout
    }

    #[cfg(unix)]
    #[test]
    fn batched_write_and_read_scripts_round_trip_through_sh() {
        let home = tempfile::tempdir().unwrap();
        let tricky = "model = \"it's $HOME `x` \\\\ \\n\"\n# 中文\nlast line without newline";
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::write(home.path().join(".codex/auth.json"), "{\"old\":true}").unwrap();

        let writes = [
            RemoteWrite {
                path: CODEX_CONFIG_PATH,
                content: Some(tricky.to_string()),
            },
            RemoteWrite {
                path: CODEX_AUTH_PATH,
                content: None,
            },
        ];
        let output = run_local_sh(home.path(), &build_write_files_script(&writes).unwrap());
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("W ") && output.contains("R "));
        assert_eq!(
            fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(),
            tricky
        );
        assert!(!home.path().join(".codex/auth.json").exists());

        let paths = [CODEX_AUTH_PATH, CODEX_CONFIG_PATH];
        let output = run_local_sh(home.path(), &build_read_files_script(&paths));
        assert_eq!(
            parse_read_files_output(&output, paths.len()).unwrap(),
            vec![None, Some(tricky.to_string())]
        );
    }

    #[test]
    fn parse_read_files_output_rejects_truncated_content() {
        let output = format!("{REMOTE_FILE_HEADER}10\nshort");
        assert!(parse_read_files_output(output.as_bytes(), 1).is_err());
        assert!(parse_read_files_output(b"", 1).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn long_ssh_config_directory_disables_multiplexing() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("long-config-directory".repeat(4));
        let mut command = Command::new("ssh");
        configure_ssh_multiplexing_at(&mut command, &dir);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(args, ["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
    }

    #[test]
    fn in_flight_guard_blocks_duplicate_until_dropped() {
        let first = InFlightGuard::acquire("test-host\ncodex".to_string());
        assert!(first.is_some());
        assert!(InFlightGuard::acquire("test-host\ncodex".to_string()).is_none());
        drop(first);
        assert!(InFlightGuard::acquire("test-host\ncodex".to_string()).is_some());
    }

    #[test]
    fn parse_stop_processes_output_collects_stopped_and_forced() {
        let output = "123\tcodex app-server\n456\tnode /usr/lib/node_modules/@openai/codex/bin/codex.js\n__CC_SWITCH_FORCE_KILLED__ 456\n";

        let (stopped, forced) = parse_stop_processes_output(output);

        assert_eq!(
            stopped,
            vec![
                RemoteProcessInfo {
                    pid: 123,
                    command: "codex app-server".to_string(),
                },
                RemoteProcessInfo {
                    pid: 456,
                    command: "node /usr/lib/node_modules/@openai/codex/bin/codex.js".to_string(),
                },
            ]
        );
        assert_eq!(forced, vec![456]);
        assert_eq!(parse_stop_processes_output(""), (Vec::new(), Vec::new()));
    }
}
