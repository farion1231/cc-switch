use std::collections::BTreeMap;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::str::FromStr;

use serde::Serialize;
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::normalize_claude_models_in_value;
use crate::settings::AppSettings;
use crate::store::AppState;

const DESKTOP_DOMAIN: &str = "com.anthropic.claudefordesktop";
const WINDOWS_REG_PATH: &str = r"HKEY_CURRENT_USER\SOFTWARE\Policies\Claude";
const DEFAULT_GATEWAY_API_KEY: &str = "PROXY_MANAGED";
const MOBILECONFIG_PROFILE_UUID: &str = "4D61C397-00A0-4B2E-97C2-2A3B50887001";
const MOBILECONFIG_PAYLOAD_UUID: &str = "4D61C397-00A0-4B2E-97C2-2A3B50887002";

#[derive(Debug, Clone, Copy)]
pub enum ClaudeDesktopExportFormat {
    Json,
    Mobileconfig,
    Reg,
}

impl ClaudeDesktopExportFormat {
    pub fn default_filename(self) -> &'static str {
        match self {
            Self::Json => "claude-desktop-3p.json",
            Self::Mobileconfig => "claude-desktop-3p.mobileconfig",
            Self::Reg => "claude-desktop-3p.reg",
        }
    }
}

impl FromStr for ClaudeDesktopExportFormat {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "mobileconfig" => Ok(Self::Mobileconfig),
            "reg" => Ok(Self::Reg),
            other => Err(AppError::InvalidInput(format!(
                "未知的 Claude Desktop 导出格式: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopPreview {
    pub domain: String,
    pub registry_path: String,
    pub current_provider_id: Option<String>,
    pub current_provider_name: Option<String>,
    pub gateway_base_url: String,
    pub gateway_api_key: String,
    pub gateway_auth_scheme: String,
    pub inference_models: Vec<String>,
    pub managed_mcp_count: usize,
    pub proxy_running: bool,
    pub local_proxy_enabled: bool,
    pub config_json: String,
    pub mobileconfig: String,
    pub windows_reg: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopModeStatus {
    pub platform: String,
    pub supported: bool,
    pub app_installed: bool,
    pub managed_config_path: Option<String>,
    pub managed_config_exists: bool,
    pub third_party_mode_enabled: bool,
    pub gateway_mode_enabled: bool,
    pub current_inference_provider: Option<String>,
    pub current_gateway_base_url: Option<String>,
    pub current_gateway_auth_scheme: Option<String>,
    pub current_code_tab_enabled: Option<bool>,
    pub current_local_mcp_enabled: Option<bool>,
    pub current_managed_mcp_count: Option<usize>,
    pub expected_gateway_base_url: String,
    pub matches_expected_gateway: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDesktopConfig {
    inference_provider: String,
    inference_gateway_base_url: String,
    inference_gateway_api_key: String,
    inference_gateway_auth_scheme: String,
    inference_gateway_headers: Vec<String>,
    inference_models: Vec<String>,
    is_claude_code_for_desktop_enabled: bool,
    is_local_dev_mcp_enabled: bool,
    managed_mcp_servers: Vec<Value>,
}

impl ClaudeDesktopConfig {
    fn from_parts(
        gateway_base_url: String,
        settings: &AppSettings,
        inference_models: Vec<String>,
        managed_mcp_servers: Vec<Value>,
    ) -> Self {
        Self {
            inference_provider: "gateway".to_string(),
            inference_gateway_base_url: gateway_base_url,
            inference_gateway_api_key: DEFAULT_GATEWAY_API_KEY.to_string(),
            inference_gateway_auth_scheme: settings.claude_desktop_gateway_auth_scheme.clone(),
            inference_gateway_headers: settings.claude_desktop_gateway_headers.clone(),
            inference_models,
            is_claude_code_for_desktop_enabled: settings.claude_desktop_code_tab_enabled,
            is_local_dev_mcp_enabled: settings.claude_desktop_local_mcp_enabled,
            managed_mcp_servers,
        }
    }

    fn to_json_pretty(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(self).map_err(|source| AppError::JsonSerialize { source })
    }

    fn to_managed_entries(&self) -> Result<Vec<ManagedEntry>, AppError> {
        Ok(vec![
            ManagedEntry::string("inferenceProvider", self.inference_provider.clone()),
            ManagedEntry::string(
                "inferenceGatewayBaseUrl",
                self.inference_gateway_base_url.clone(),
            ),
            ManagedEntry::string(
                "inferenceGatewayApiKey",
                self.inference_gateway_api_key.clone(),
            ),
            ManagedEntry::string(
                "inferenceGatewayAuthScheme",
                self.inference_gateway_auth_scheme.clone(),
            ),
            ManagedEntry::string(
                "inferenceGatewayHeaders",
                serde_json::to_string(&self.inference_gateway_headers)
                    .map_err(|source| AppError::JsonSerialize { source })?,
            ),
            ManagedEntry::string(
                "inferenceModels",
                serde_json::to_string(&self.inference_models)
                    .map_err(|source| AppError::JsonSerialize { source })?,
            ),
            ManagedEntry::boolean(
                "isClaudeCodeForDesktopEnabled",
                self.is_claude_code_for_desktop_enabled,
            ),
            ManagedEntry::boolean("isLocalDevMcpEnabled", self.is_local_dev_mcp_enabled),
            ManagedEntry::string(
                "managedMcpServers",
                serde_json::to_string(&self.managed_mcp_servers)
                    .map_err(|source| AppError::JsonSerialize { source })?,
            ),
        ])
    }

    fn to_mobileconfig(&self) -> Result<String, AppError> {
        let entries = self.to_managed_entries()?;
        Ok(render_mobileconfig(&entries))
    }

    fn to_windows_reg(&self) -> Result<String, AppError> {
        let entries = self.to_managed_entries()?;
        Ok(render_windows_reg(&entries))
    }
}

#[derive(Debug, Clone)]
struct ManagedEntry {
    key: &'static str,
    value: ManagedValue,
}

impl ManagedEntry {
    fn string(key: &'static str, value: String) -> Self {
        Self {
            key,
            value: ManagedValue::String(value),
        }
    }

    fn boolean(key: &'static str, value: bool) -> Self {
        Self {
            key,
            value: ManagedValue::Boolean(value),
        }
    }
}

#[derive(Debug, Clone)]
enum ManagedValue {
    String(String),
    Boolean(bool),
}

pub async fn build_preview(state: &AppState) -> Result<ClaudeDesktopPreview, AppError> {
    let settings = crate::settings::get_settings();
    let current_provider_id =
        crate::settings::get_effective_current_provider(&state.db, &AppType::Claude)?;
    let current_provider = match current_provider_id.as_deref() {
        Some(id) => state
            .db
            .get_all_providers(AppType::Claude.as_str())?
            .shift_remove(id),
        None => None,
    };

    let gateway_base_url = state
        .proxy_service
        .get_loopback_origin()
        .await
        .map_err(AppError::Config)?;
    let inference_models = build_inference_models(current_provider.as_ref());
    let managed_mcp_servers = build_managed_mcp_servers(state, &settings)?;
    let managed_mcp_count = managed_mcp_servers.len();
    let config = ClaudeDesktopConfig::from_parts(
        gateway_base_url.clone(),
        &settings,
        inference_models.clone(),
        managed_mcp_servers,
    );

    let proxy_running = state.proxy_service.is_running().await;
    let local_proxy_enabled = settings.enable_local_proxy;

    Ok(ClaudeDesktopPreview {
        domain: DESKTOP_DOMAIN.to_string(),
        registry_path: WINDOWS_REG_PATH.to_string(),
        current_provider_name: current_provider
            .as_ref()
            .map(|provider| provider.name.clone()),
        current_provider_id,
        gateway_base_url,
        gateway_api_key: DEFAULT_GATEWAY_API_KEY.to_string(),
        gateway_auth_scheme: settings.claude_desktop_gateway_auth_scheme.clone(),
        inference_models,
        managed_mcp_count,
        proxy_running,
        local_proxy_enabled,
        config_json: config.to_json_pretty()?,
        mobileconfig: config.to_mobileconfig()?,
        windows_reg: config.to_windows_reg()?,
        warnings: build_warnings(
            current_provider.as_ref(),
            local_proxy_enabled,
            proxy_running,
            managed_mcp_count,
        ),
    })
}

pub async fn export_to_path(
    state: &AppState,
    format: ClaudeDesktopExportFormat,
    path: &Path,
) -> Result<(), AppError> {
    let preview = build_preview(state).await?;
    let content = match format {
        ClaudeDesktopExportFormat::Json => preview.config_json,
        ClaudeDesktopExportFormat::Mobileconfig => preview.mobileconfig,
        ClaudeDesktopExportFormat::Reg => preview.windows_reg,
    };

    write_text_file(path, &content)
}

pub async fn detect_mode_status(state: &AppState) -> Result<ClaudeDesktopModeStatus, AppError> {
    let expected_gateway_base_url = state
        .proxy_service
        .get_loopback_origin()
        .await
        .map_err(AppError::Config)?;

    #[cfg(target_os = "macos")]
    {
        return detect_mode_status_macos(expected_gateway_base_url);
    }

    #[allow(unreachable_code)]
    Ok(ClaudeDesktopModeStatus {
        platform: std::env::consts::OS.to_string(),
        supported: false,
        app_installed: false,
        managed_config_path: None,
        managed_config_exists: false,
        third_party_mode_enabled: false,
        gateway_mode_enabled: false,
        current_inference_provider: None,
        current_gateway_base_url: None,
        current_gateway_auth_scheme: None,
        current_code_tab_enabled: None,
        current_local_mcp_enabled: None,
        current_managed_mcp_count: None,
        expected_gateway_base_url,
        matches_expected_gateway: false,
        warnings: vec!["当前平台暂未实现 Claude Desktop 3P mode 自动检测。".to_string()],
    })
}

fn build_warnings(
    current_provider: Option<&Provider>,
    local_proxy_enabled: bool,
    proxy_running: bool,
    managed_mcp_count: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();

    if current_provider.is_none() {
        warnings
            .push("当前还没有选中 Claude 供应商，导出的模型列表会回退到通用默认值。".to_string());
    }
    if !local_proxy_enabled {
        warnings.push(
            "本地代理功能当前未开启，Claude Desktop 导入配置后还需要先在 cc-switch 里启用代理。"
                .to_string(),
        );
    }
    if !proxy_running {
        warnings.push("本地代理当前未运行，Claude Desktop 连接前请先启动代理服务。".to_string());
    }
    if managed_mcp_count == 0 {
        warnings.push(
            "当前没有可转换的远程 Claude MCP 服务器，managedMcpServers 将导出为空数组。"
                .to_string(),
        );
    }

    warnings
}

#[cfg(target_os = "macos")]
fn detect_mode_status_macos(
    expected_gateway_base_url: String,
) -> Result<ClaudeDesktopModeStatus, AppError> {
    let managed_config_path = find_macos_managed_config_path();
    let managed_config_exists = managed_config_path.is_some();
    let app_installed = find_macos_claude_app_path().is_some();
    let managed_config = match managed_config_path.as_ref() {
        Some(path) => Some(read_macos_managed_config(path)?),
        None => None,
    };

    let current_inference_provider =
        read_string_field(managed_config.as_ref(), "inferenceProvider");
    let current_gateway_base_url =
        read_string_field(managed_config.as_ref(), "inferenceGatewayBaseUrl");
    let current_gateway_auth_scheme =
        read_string_field(managed_config.as_ref(), "inferenceGatewayAuthScheme");
    let current_code_tab_enabled =
        read_bool_field(managed_config.as_ref(), "isClaudeCodeForDesktopEnabled");
    let current_local_mcp_enabled =
        read_bool_field(managed_config.as_ref(), "isLocalDevMcpEnabled");
    let current_managed_mcp_count =
        read_json_array_len_field(managed_config.as_ref(), "managedMcpServers");

    let third_party_mode_enabled = current_inference_provider.is_some();
    let gateway_mode_enabled = current_inference_provider.as_deref() == Some("gateway");
    let matches_expected_gateway = gateway_mode_enabled
        && current_gateway_base_url.as_deref() == Some(expected_gateway_base_url.as_str());

    let mut warnings = Vec::new();
    if !app_installed {
        warnings.push(
            "未找到 Claude.app，请先确认 Claude Desktop 已安装在 /Applications 或 ~/Applications。"
                .to_string(),
        );
    }
    if !managed_config_exists {
        warnings.push(
            "未发现 Claude Desktop 的受管配置文件，说明 .mobileconfig 还没有真正安装到系统。"
                .to_string(),
        );
    } else if !third_party_mode_enabled {
        warnings.push("已找到受管配置文件，但里面没有 inferenceProvider，Claude Desktop 仍不会进入 third-party mode。".to_string());
    } else if !gateway_mode_enabled {
        warnings.push(format!(
            "当前 inferenceProvider 是 {:?}，不是 gateway。",
            current_inference_provider
        ));
    } else if !matches_expected_gateway {
        warnings.push("当前已安装的 gatewayBaseUrl 和 cc-switch 当前代理地址不一致，Claude Desktop 可能连到旧地址。".to_string());
    }

    if managed_config_exists {
        warnings.push("如果你刚安装完 .mobileconfig，请完全退出 Claude Desktop 后重新打开，再刷新这里的状态。".to_string());
    }

    Ok(ClaudeDesktopModeStatus {
        platform: "macos".to_string(),
        supported: true,
        app_installed,
        managed_config_path: managed_config_path.map(|path| path.display().to_string()),
        managed_config_exists,
        third_party_mode_enabled,
        gateway_mode_enabled,
        current_inference_provider,
        current_gateway_base_url,
        current_gateway_auth_scheme,
        current_code_tab_enabled,
        current_local_mcp_enabled,
        current_managed_mcp_count,
        expected_gateway_base_url,
        matches_expected_gateway,
        warnings,
    })
}

#[cfg(target_os = "macos")]
fn find_macos_managed_config_path() -> Option<PathBuf> {
    macos_managed_config_candidates()
        .into_iter()
        .find(|path| path.exists())
}

#[cfg(target_os = "macos")]
fn macos_managed_config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(user) = current_username() {
        candidates.push(
            PathBuf::from("/Library/Managed Preferences")
                .join(user)
                .join(format!("{DESKTOP_DOMAIN}.plist")),
        );
    }
    candidates.push(
        PathBuf::from("/Library/Managed Preferences").join(format!("{DESKTOP_DOMAIN}.plist")),
    );
    candidates.push(
        crate::config::get_home_dir()
            .join("Library")
            .join("Managed Preferences")
            .join(format!("{DESKTOP_DOMAIN}.plist")),
    );
    candidates
}

#[cfg(target_os = "macos")]
fn current_username() -> Option<String> {
    std::env::var("USER")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            crate::config::get_home_dir()
                .file_name()
                .map(|value| value.to_string_lossy().trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

#[cfg(target_os = "macos")]
fn find_macos_claude_app_path() -> Option<PathBuf> {
    [
        PathBuf::from("/Applications/Claude.app"),
        crate::config::get_home_dir()
            .join("Applications")
            .join("Claude.app"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

#[cfg(target_os = "macos")]
fn read_macos_managed_config(path: &Path) -> Result<Value, AppError> {
    let output = Command::new("/usr/bin/plutil")
        .arg("-convert")
        .arg("json")
        .arg("-o")
        .arg("-")
        .arg(path)
        .output()
        .map_err(|e| AppError::io(path, e))?;

    if !output.status.success() {
        return Err(AppError::Config(format!(
            "读取 Claude Desktop managed config 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    serde_json::from_slice(&output.stdout).map_err(|source| AppError::Json {
        path: path.display().to_string(),
        source,
    })
}

fn read_string_field(config: Option<&Value>, key: &str) -> Option<String> {
    config
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_bool_field(config: Option<&Value>, key: &str) -> Option<bool> {
    config
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_bool())
}

fn read_json_array_len_field(config: Option<&Value>, key: &str) -> Option<usize> {
    let raw = config.and_then(|value| value.get(key))?;
    match raw {
        Value::Array(items) => Some(items.len()),
        Value::String(text) => serde_json::from_str::<Vec<Value>>(text)
            .ok()
            .map(|items| items.len()),
        _ => None,
    }
}

fn build_inference_models(provider: Option<&Provider>) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = BTreeMap::new();

    if let Some(provider) = provider {
        let mut settings = provider.settings_config.clone();
        let _ = normalize_claude_models_in_value(&mut settings);

        if let Some(env) = settings.get("env").and_then(|value| value.as_object()) {
            for key in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            ] {
                if let Some(model) = env.get(key).and_then(|value| value.as_str()) {
                    let trimmed = model.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_string(), ()).is_none() {
                        ordered.push(trimmed.to_string());
                    }
                }
            }
        }
    }

    if ordered.is_empty() {
        ordered.extend(
            ["sonnet", "haiku", "opus"]
                .into_iter()
                .map(ToString::to_string),
        );
    }

    ordered
}

fn build_managed_mcp_servers(
    state: &AppState,
    settings: &AppSettings,
) -> Result<Vec<Value>, AppError> {
    if !settings.claude_desktop_include_managed_mcp {
        return Ok(Vec::new());
    }

    let servers = state.db.get_all_mcp_servers()?;
    let mut managed = Vec::new();

    for server in servers.values() {
        if !server.apps.claude {
            continue;
        }

        let Some(url) = server.server.get("url").and_then(|value| value.as_str()) else {
            continue;
        };
        let url = url.trim();
        if url.is_empty() {
            continue;
        }

        let transport = server
            .server
            .get("type")
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_ascii_lowercase());

        if matches!(transport.as_deref(), Some("stdio")) {
            continue;
        }

        let mut entry = serde_json::Map::new();
        entry.insert("name".to_string(), json!(server.name));
        entry.insert("url".to_string(), json!(url));

        if let Some(transport) = transport {
            if matches!(transport.as_str(), "http" | "sse") {
                entry.insert("transport".to_string(), json!(transport));
            }
        }

        if let Some(headers) = server
            .server
            .get("headers")
            .and_then(|value| value.as_object())
        {
            let normalized_headers = headers
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.trim())))
                .filter(|(_, value)| !value.is_empty())
                .map(|(key, value)| (key, Value::String(value.to_string())))
                .collect::<serde_json::Map<String, Value>>();
            if !normalized_headers.is_empty() {
                entry.insert("headers".to_string(), Value::Object(normalized_headers));
            }
        }

        managed.push(Value::Object(entry));
    }

    Ok(managed)
}

fn render_mobileconfig(entries: &[ManagedEntry]) -> String {
    let managed_entries = entries
        .iter()
        .map(render_mobileconfig_entry)
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>PayloadType</key>
      <string>com.apple.ManagedClient.preferences</string>
      <key>PayloadVersion</key>
      <integer>1</integer>
      <key>PayloadIdentifier</key>
      <string>cc-switch.claude-desktop.3p.managed</string>
      <key>PayloadUUID</key>
      <string>{payload_uuid}</string>
      <key>PayloadDisplayName</key>
      <string>Claude Desktop Third-Party Gateway</string>
      <key>PayloadContent</key>
      <dict>
        <key>{domain}</key>
        <dict>
          <key>Forced</key>
          <array>
            <dict>
              <key>mcx_preference_settings</key>
              <dict>
{managed_entries}
              </dict>
            </dict>
          </array>
        </dict>
      </dict>
    </dict>
  </array>
  <key>PayloadDisplayName</key>
  <string>Claude Desktop Third-Party Gateway</string>
  <key>PayloadIdentifier</key>
  <string>cc-switch.claude-desktop.3p</string>
  <key>PayloadRemovalDisallowed</key>
  <false/>
  <key>PayloadType</key>
  <string>Configuration</string>
  <key>PayloadUUID</key>
  <string>{profile_uuid}</string>
  <key>PayloadVersion</key>
  <integer>1</integer>
</dict>
</plist>
"#,
        payload_uuid = MOBILECONFIG_PAYLOAD_UUID,
        profile_uuid = MOBILECONFIG_PROFILE_UUID,
        domain = DESKTOP_DOMAIN,
        managed_entries = managed_entries,
    )
}

fn render_mobileconfig_entry(entry: &ManagedEntry) -> String {
    match &entry.value {
        ManagedValue::String(value) => format!(
            "                <key>{}</key>\n                <string>{}</string>",
            xml_escape(entry.key),
            xml_escape(value)
        ),
        ManagedValue::Boolean(value) => format!(
            "                <key>{}</key>\n                <{} />",
            xml_escape(entry.key),
            if *value { "true" } else { "false" }
        ),
    }
}

fn render_windows_reg(entries: &[ManagedEntry]) -> String {
    let values = entries
        .iter()
        .map(render_windows_reg_entry)
        .collect::<Vec<_>>()
        .join("\n");

    format!("Windows Registry Editor Version 5.00\n\n[{WINDOWS_REG_PATH}]\n{values}\n")
}

fn render_windows_reg_entry(entry: &ManagedEntry) -> String {
    match &entry.value {
        ManagedValue::String(value) => {
            format!("\"{}\"=\"{}\"", entry.key, reg_escape(value))
        }
        ManagedValue::Boolean(value) => {
            format!("\"{}\"=dword:{:08x}", entry.key, if *value { 1 } else { 0 })
        }
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn reg_escape(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('\n', r"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_with_models() -> Provider {
        Provider {
            id: "provider-1".to_string(),
            name: "Provider 1".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_MODEL": "claude-sonnet-4-5",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-5",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-1"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn build_inference_models_prefers_provider_models() {
        let models = build_inference_models(Some(&provider_with_models()));
        assert_eq!(
            models,
            vec![
                "claude-sonnet-4-5".to_string(),
                "claude-opus-4-1".to_string(),
                "claude-haiku-4-5".to_string()
            ]
        );
    }

    #[test]
    fn build_inference_models_falls_back_to_aliases() {
        let models = build_inference_models(None);
        assert_eq!(
            models,
            vec![
                "sonnet".to_string(),
                "haiku".to_string(),
                "opus".to_string()
            ]
        );
    }

    #[test]
    fn windows_reg_renders_bool_and_string_values() {
        let config = ClaudeDesktopConfig {
            inference_provider: "gateway".to_string(),
            inference_gateway_base_url: "http://127.0.0.1:3456".to_string(),
            inference_gateway_api_key: DEFAULT_GATEWAY_API_KEY.to_string(),
            inference_gateway_auth_scheme: "x-api-key".to_string(),
            inference_gateway_headers: vec!["X-Test: 1".to_string()],
            inference_models: vec!["sonnet".to_string()],
            is_claude_code_for_desktop_enabled: true,
            is_local_dev_mcp_enabled: false,
            managed_mcp_servers: vec![],
        };

        let reg = config.to_windows_reg().expect("reg output");
        assert!(reg.contains("\"inferenceProvider\"=\"gateway\""));
        assert!(reg.contains("\"isClaudeCodeForDesktopEnabled\"=dword:00000001"));
        assert!(reg.contains("\"isLocalDevMcpEnabled\"=dword:00000000"));
    }

    #[test]
    fn read_json_array_len_field_supports_json_strings() {
        let config = json!({
            "managedMcpServers": "[{\"name\":\"A\"},{\"name\":\"B\"}]"
        });

        assert_eq!(
            read_json_array_len_field(Some(&config), "managedMcpServers"),
            Some(2)
        );
    }
}
