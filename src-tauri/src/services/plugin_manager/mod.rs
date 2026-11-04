//! 各应用的插件管理。
//!
//! 插件状态（已装列表、启用开关、市场、缓存目录）都归应用自己的 CLI 管，格式随版本变，
//! 这里尽量不碰它们的私有文件，而是调官方 CLI 的 `--json` 输出：
//!
//! - Claude Code：`claude plugin list|install|uninstall|enable|disable|update` 和
//!   `claude plugin marketplace list|add|remove|update`。
//! - Codex：`codex plugin list|add|remove` 和 `codex plugin marketplace list|add|remove|upgrade`。
//!   Codex 没有启用 / 停用命令，开关就是 `config.toml` 的 `[plugins."<id>"] enabled`，
//!   用 `toml_edit` 原位改这一个键。
//! - Grok Build、MiniMax Code、OpenClaw：见各自的子模块。
//!
//! CLI 读的配置目录和 CC Switch 读的必须是同一份：设置里覆盖了目录时，用各应用的环境变量
//! （`CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`GROK_HOME`……）告诉 CLI。

mod grok;
mod mcode;
mod openclaw;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具没装（或 WSL 里找不到）时返回的错误码，前端据此显示安装提示。
pub const CLI_NOT_INSTALLED: &str = "PLUGIN_CLI_NOT_INSTALLED";

const LIST_TIMEOUT: Duration = Duration::from_secs(90);
const TOGGLE_TIMEOUT: Duration = Duration::from_secs(60);
/// 安装、更新、加市场都可能要 git clone。
const NETWORK_TIMEOUT: Duration = Duration::from_secs(300);

const MAX_SOURCE_LEN: usize = 512;

/// 序列化名和前端的应用 id（`AppId`）一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginApp {
    Claude,
    Codex,
    #[serde(rename = "grokbuild")]
    Grok,
    Mcode,
    Openclaw,
}

impl PluginApp {
    fn tool(self) -> &'static str {
        match self {
            PluginApp::Claude => "claude",
            PluginApp::Codex => "codex",
            PluginApp::Grok => "grok",
            PluginApp::Mcode => "mcode",
            PluginApp::Openclaw => "openclaw",
        }
    }

    /// 同一个应用的写操作排队执行：两个 CLI 进程同时改同一份配置会互相覆盖。
    fn write_lock(self) -> &'static Mutex<()> {
        static CLAUDE: Mutex<()> = Mutex::new(());
        static CODEX: Mutex<()> = Mutex::new(());
        static GROK: Mutex<()> = Mutex::new(());
        static MCODE: Mutex<()> = Mutex::new(());
        static OPENCLAW: Mutex<()> = Mutex::new(());
        match self {
            PluginApp::Claude => &CLAUDE,
            PluginApp::Codex => &CODEX,
            PluginApp::Grok => &GROK,
            PluginApp::Mcode => &MCODE,
            PluginApp::Openclaw => &OPENCLAW,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    /// 交给 CLI 的插件 id：Claude Code / Codex / MiniMax Code 是 `name@marketplace`，
    /// Grok Build 是插件名，OpenClaw 是插件 id（目录里是 ClawHub 包名）。
    pub id: String,
    pub name: String,
    /// 来自哪个市场；没有市场概念的（OpenClaw 已装的）为空。
    pub marketplace: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    /// Claude Code 的安装范围（user / project / local / managed）；其它应用没有。
    pub scope: Option<String>,
    /// CC Switch 能不能启停它：Claude Code 只管 user 范围（项目范围的跟着项目目录走）。
    pub manageable: bool,
    /// 能不能卸载、更新：OpenClaw 随程序自带的插件只能开关。
    pub removable: bool,
    /// 市场允许安装（Codex 的 `installPolicy` 不是 `NOT_AVAILABLE`）。
    pub installable: bool,
    /// 目录条目装好后在已安装列表里的 id，和 `id` 不同时才有：OpenClaw 目录给的是 ClawHub
    /// 包名（`@openclaw/memory-lancedb`），装好后的插件 id 是 `memory-lancedb`。
    pub installed_id: Option<String>,
    pub install_path: Option<String>,
    pub updated_at: Option<String>,
}

/// 一次 `plugin list` 的解析结果（`available` 已去掉已安装的 id）。
#[derive(Debug, Clone, Default, PartialEq)]
struct PluginListing {
    pub installed: Vec<PluginEntry>,
    pub available: Vec<PluginEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplace {
    pub name: String,
    /// 来源（`owner/repo`、URL、本地路径）；CLI 没给时为空。
    pub source: Option<String>,
    /// 市场在本机的目录。
    pub location: Option<String>,
    /// 用户自己加的才能删；Codex 内置的官方市场不在 `config.toml` 里，删不掉。
    pub removable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionOutcome {
    /// 操作确实改了东西（更新时：已经是最新版为 false）。
    pub changed: bool,
    pub message: Option<String>,
}

// ───────────────────────────── 参数校验 ─────────────────────────────

fn is_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// 插件 id 必须写全 `name@marketplace`：只写名字时 CLI 会在所有市场里挑，可能挑错。
pub fn validate_plugin_id(id: &str) -> Result<(), String> {
    match id.split_once('@') {
        Some((name, market)) if is_name_segment(name) && is_name_segment(market) => Ok(()),
        _ => Err(format!("Invalid plugin id: {id}")),
    }
}

/// npm 风格的包名：`name` 或 `@scope/name`（OpenClaw 的插件 id 和 ClawHub 包名都是这样）。
fn is_package_name(id: &str) -> bool {
    match id.strip_prefix('@') {
        Some(rest) => matches!(
            rest.split_once('/'),
            Some((scope, name)) if is_name_segment(scope) && is_name_segment(name)
        ),
        None => is_name_segment(id),
    }
}

/// 按应用校验交给 CLI 的插件 id。
fn validate_app_plugin_id(app: PluginApp, id: &str) -> Result<(), String> {
    let ok = match app {
        PluginApp::Claude | PluginApp::Codex | PluginApp::Mcode => {
            return validate_plugin_id(id);
        }
        PluginApp::Grok => is_name_segment(id),
        PluginApp::Openclaw => is_package_name(id),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("Invalid plugin id: {id}"))
    }
}

/// 这个应用的 CLI 没有对应命令（前端本该隐藏入口）。
fn unsupported(app: PluginApp, what: &str) -> String {
    format!("{} does not support {what}", app.tool())
}

pub fn validate_marketplace_name(name: &str) -> Result<(), String> {
    if is_name_segment(name) {
        Ok(())
    } else {
        Err(format!("Invalid marketplace name: {name}"))
    }
}

/// 市场来源：`owner/repo`、`owner/repo@ref`、`https://…`、`git@host:owner/repo.git`、
/// 不含空格和反斜杠的本地路径。不能以 `-` 开头（会被当成选项）。
pub fn validate_marketplace_source(source: &str) -> Result<String, String> {
    let source = source.trim();
    if source.is_empty()
        || source.len() > MAX_SOURCE_LEN
        || source.starts_with('-')
        || !crate::commands::is_safe_tool_command_arg(source)
    {
        return Err(format!("Invalid marketplace source: {source}"));
    }
    Ok(source.to_string())
}

// ───────────────────────────── 调 CLI ─────────────────────────────

/// 判断 Claude 覆盖目录是不是就是默认的 `~/.claude`（本机或 WSL 里的家目录）。
/// 是的话不设 `CLAUDE_CONFIG_DIR`：设了它 Claude Code 会改去目录里找 `.claude.json`。
fn is_default_claude_dir(dir: &Path) -> bool {
    if dir == crate::config::get_home_dir().join(".claude") {
        return true;
    }
    let normalized = dir.to_string_lossy().replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    let lower = normalized.to_ascii_lowercase();
    if !(lower.starts_with("//wsl$/") || lower.starts_with("//wsl.localhost/")) {
        return false;
    }
    // //wsl$/<distro>/home/<user>/.claude 或 //wsl$/<distro>/root/.claude
    let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    matches!(
        parts.as_slice(),
        [_, _, "home", _, ".claude"] | [_, _, "root", ".claude"]
    )
}

/// CLI 的环境变量和工作目录。
fn cli_context(app: PluginApp) -> (Vec<(&'static str, String)>, PathBuf) {
    let (dir, env) = match app {
        PluginApp::Claude => {
            let dir = crate::config::get_claude_config_dir();
            let env = crate::settings::get_claude_override_dir()
                .filter(|d| !is_default_claude_dir(d))
                .map(|d| vec![("CLAUDE_CONFIG_DIR", d.to_string_lossy().into_owned())])
                .unwrap_or_default();
            (dir, env)
        }
        PluginApp::Codex => {
            // 总是显式传：启停开关由 CC Switch 直接改 `<dir>/config.toml`，CLI 必须读写同一份；
            // 否则 GUI 进程继承来的 CODEX_HOME 会让两边各改各的。`~/.codex` 本就是 Codex 的默认值。
            let dir = crate::codex_config::get_codex_config_dir();
            let env = vec![("CODEX_HOME", dir.to_string_lossy().into_owned())];
            (dir, env)
        }
        // 下面几个同理总是显式传，CLI 和 CC Switch 用同一个目录（Grok Build 的启停状态由
        // CC Switch 直接读 `<GROK_HOME>/config.toml`）。
        PluginApp::Grok => {
            let dir = crate::grok_config::get_grok_config_dir();
            let env = vec![("GROK_HOME", dir.to_string_lossy().into_owned())];
            (dir, env)
        }
        PluginApp::Mcode => {
            let dir = crate::mcode_config::data_dir();
            let env = vec![("MINIMAX_DATA_DIR", dir.to_string_lossy().into_owned())];
            (dir, env)
        }
        PluginApp::Openclaw => {
            let dir = crate::openclaw_config::get_openclaw_dir();
            let config = crate::openclaw_config::get_openclaw_config_path();
            let env = vec![
                ("OPENCLAW_STATE_DIR", dir.to_string_lossy().into_owned()),
                (
                    "OPENCLAW_CONFIG_PATH",
                    config.to_string_lossy().into_owned(),
                ),
            ];
            (dir, env)
        }
    };
    let working_dir = if std::fs::create_dir_all(&dir).is_ok() {
        dir
    } else {
        crate::config::get_home_dir()
    };
    (env, working_dir)
}

struct CliOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_cli(app: PluginApp, args: &[&str], timeout: Duration) -> Result<CliOutput, String> {
    let tool = app.tool();
    let (env, working_dir) = cli_context(app);
    let output = crate::commands::run_detected_tool_command_with_timeout(
        tool,
        args,
        Some(timeout),
        &env,
        &working_dir,
    )
    .map_err(|e| {
        if e == format!("{tool} is not installed") {
            CLI_NOT_INSTALLED.to_string()
        } else {
            e
        }
    })?;
    // WSL 里 `command -v` 找不到工具时外层脚本 exit 127。
    if output.status.code() == Some(127) {
        return Err(CLI_NOT_INSTALLED.to_string());
    }
    Ok(CliOutput {
        success: output.status.success(),
        stdout: crate::commands::decode_command_output(&output.stdout),
        stderr: crate::commands::decode_command_output(&output.stderr),
    })
}

/// 从 stdout 里取 JSON：整段就是 JSON；或最后一行是 JSON（Claude 的 `--json` 结果行）；
/// 或前面混了提示行，从第一个 `{` / `[` 开始解析。
fn parse_json_payload(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    if let Some(value) = trimmed
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| line.starts_with('{') || line.starts_with('['))
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        return Some(value);
    }
    let start = trimmed.find(['{', '['])?;
    serde_json::Deserializer::from_str(&trimmed[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// 失败时给用户看的那句话：去掉 Codex 的 `WARNING:` 提示行和 `Error:` / `✘` 前缀。
fn failure_detail(out: &CliOutput) -> String {
    let pick = |text: &str| -> String {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("WARNING:"))
            .map(|line| {
                line.trim_start_matches('✘')
                    .trim_start()
                    .trim_start_matches("Error:")
                    .trim()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let detail = pick(&out.stderr);
    if !detail.is_empty() {
        return detail;
    }
    let detail = pick(&out.stdout);
    if detail.is_empty() {
        "Command failed".to_string()
    } else {
        detail
    }
}

/// Claude 的 `--json` 结果行：`{"outcome":"ok"|"failed","message":…}`。
fn claude_action_result(out: &CliOutput) -> Result<Value, String> {
    let payload = parse_json_payload(&out.stdout);
    if let Some(value) = payload.as_ref() {
        if value.get("outcome").and_then(Value::as_str) == Some("ok") && out.success {
            return Ok(value.clone());
        }
        if let Some(message) = str_field(value, "message") {
            return Err(message);
        }
    }
    Err(failure_detail(out))
}

fn codex_action_result(out: &CliOutput) -> Result<Value, String> {
    if !out.success {
        return Err(failure_detail(out));
    }
    Ok(parse_json_payload(&out.stdout).unwrap_or(Value::Null))
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn split_id(id: &str) -> (String, String) {
    match id.split_once('@') {
        Some((name, market)) => (name.to_string(), market.to_string()),
        None => (id.to_string(), String::new()),
    }
}

// ───────────────────────────── 插件清单（plugin.json） ─────────────────────────────

#[derive(Default)]
struct ManifestInfo {
    display_name: Option<String>,
    description: Option<String>,
    category: Option<String>,
}

/// 依次找 `<dir>/<manifest_dir>/plugin.json`（空串表示插件根目录），用第一个读得到的。
fn read_manifest_any(dir: &Path, manifest_dirs: &[&str]) -> ManifestInfo {
    manifest_dirs
        .iter()
        .find_map(|manifest_dir| read_manifest_file(&dir.join(manifest_dir).join("plugin.json")))
        .unwrap_or_default()
}

fn read_manifest(dir: &Path, manifest_dir: &str) -> ManifestInfo {
    read_manifest_file(&dir.join(manifest_dir).join("plugin.json")).unwrap_or_default()
}

fn read_manifest_file(path: &Path) -> Option<ManifestInfo> {
    let text = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&text).ok()?;
    let interface = value.get("interface").cloned().unwrap_or(Value::Null);
    Some(ManifestInfo {
        display_name: str_field(&interface, "displayName"),
        description: str_field(&interface, "shortDescription")
            .or_else(|| str_field(&value, "description")),
        category: str_field(&interface, "category"),
    })
}

// ───────────────────────────── Claude Code ─────────────────────────────

fn parse_claude_listing(value: &Value) -> PluginListing {
    // 不带 --available 时是数组；带了是 {installed, available}
    let (installed_raw, available_raw) = match value {
        Value::Array(items) => (items.clone(), Vec::new()),
        _ => (
            value
                .get("installed")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            value
                .get("available")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        ),
    };

    let mut available: Vec<PluginEntry> = available_raw
        .iter()
        .filter_map(|item| {
            let id = str_field(item, "pluginId")?;
            let (name, market) = split_id(&id);
            Some(PluginEntry {
                name: str_field(item, "name").unwrap_or(name),
                marketplace: str_field(item, "marketplaceName").unwrap_or(market),
                version: str_field(item, "version"),
                description: str_field(item, "description"),
                installable: true,
                id,
                ..Default::default()
            })
        })
        .collect();

    let installed: Vec<PluginEntry> = installed_raw
        .iter()
        .filter_map(|item| {
            let id = str_field(item, "id").or_else(|| str_field(item, "pluginId"))?;
            let (name, market) = split_id(&id);
            let scope = str_field(item, "scope");
            let install_path = str_field(item, "installPath");
            let manifest = install_path
                .as_deref()
                .map(|p| read_manifest(Path::new(p), ".claude-plugin"))
                .unwrap_or_default();
            let catalog_description = available
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| a.description.clone());
            Some(PluginEntry {
                name,
                marketplace: market,
                display_name: manifest.display_name,
                version: str_field(item, "version"),
                description: manifest.description.or(catalog_description),
                category: manifest.category,
                installed: true,
                enabled: item
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                manageable: scope.as_deref().is_none_or(|s| s == "user"),
                removable: scope.as_deref().is_none_or(|s| s == "user"),
                installable: true,
                installed_id: None,
                scope,
                install_path,
                updated_at: str_field(item, "lastUpdated")
                    .or_else(|| str_field(item, "installedAt")),
                id,
            })
        })
        .collect();

    available.retain(|a| !installed.iter().any(|i| i.id == a.id));
    PluginListing {
        installed,
        available,
    }
}

fn parse_claude_marketplaces(value: &Value) -> Vec<PluginMarketplace> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = str_field(item, "name")?;
                    let source = str_field(item, "repo")
                        .or_else(|| str_field(item, "url"))
                        .or_else(|| str_field(item, "path"))
                        .or_else(|| str_field(item, "source"));
                    Some(PluginMarketplace {
                        name,
                        source,
                        location: str_field(item, "installLocation"),
                        removable: true,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────────── Codex ─────────────────────────────

fn parse_codex_entry(item: &Value) -> Option<PluginEntry> {
    let id = str_field(item, "pluginId")?;
    let (name, market) = split_id(&id);
    let source_path = item
        .get("source")
        .filter(|s| s.get("source").and_then(Value::as_str) == Some("local"))
        .and_then(|s| str_field(s, "path"));
    let manifest = source_path
        .as_deref()
        .map(|p| read_manifest(Path::new(p), ".codex-plugin"))
        .unwrap_or_default();
    Some(PluginEntry {
        name: str_field(item, "name").unwrap_or(name),
        marketplace: str_field(item, "marketplaceName").unwrap_or(market),
        display_name: manifest.display_name,
        version: str_field(item, "version"),
        description: manifest.description,
        category: manifest.category,
        installed: item
            .get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        enabled: item
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        manageable: true,
        removable: true,
        installable: str_field(item, "installPolicy").as_deref() != Some("NOT_AVAILABLE"),
        installed_id: None,
        install_path: None,
        updated_at: None,
        scope: None,
        id,
    })
}

fn parse_codex_listing(value: &Value) -> PluginListing {
    let collect = |key: &str| -> Vec<PluginEntry> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(parse_codex_entry).collect())
            .unwrap_or_default()
    };
    let installed = collect("installed");
    let mut available = collect("available");
    available.retain(|a| !a.installed && !installed.iter().any(|i| i.id == a.id));
    PluginListing {
        installed,
        available,
    }
}

fn codex_configured_marketplaces(config_text: &str) -> toml_edit::DocumentMut {
    config_text.parse().unwrap_or_default()
}

fn parse_codex_marketplaces(value: &Value, config_text: &str) -> Vec<PluginMarketplace> {
    let doc = codex_configured_marketplaces(config_text);
    let configured = doc.get("marketplaces").and_then(|m| m.as_table_like());
    value
        .get("marketplaces")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = str_field(item, "name")?;
                    let entry = configured.and_then(|t| t.get(&name));
                    let source = entry
                        .and_then(|e| e.get("source"))
                        .and_then(|s| s.as_str())
                        .map(|s| s.trim_start_matches(r"\\?\").to_string());
                    Some(PluginMarketplace {
                        removable: entry.is_some(),
                        source,
                        location: str_field(item, "root"),
                        name,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `config.toml` 里 `[plugins."<id>"] enabled` 的值；没写或解析不了时为 `None`。
fn codex_plugin_enabled_in_text(text: &str, id: &str) -> Option<bool> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("plugins")?.get(id)?.get("enabled")?.as_bool()
}

/// 在 `config.toml` 里设置 `[plugins."<id>"] enabled`，其余内容原样保留。
fn set_codex_plugin_enabled_in_text(text: &str, id: &str, enabled: bool) -> Result<String, String> {
    use toml_edit::{table, value, DocumentMut, Item};

    let mut doc = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|e| format!("Failed to parse config.toml: {e}"))?
    };

    if !doc.contains_key("plugins") {
        let mut plugins = toml_edit::Table::new();
        plugins.set_implicit(true);
        doc.insert("plugins", Item::Table(plugins));
    }
    let plugins = doc["plugins"]
        .as_table_like_mut()
        .ok_or_else(|| "config.toml: `plugins` is not a table".to_string())?;

    match plugins.get_mut(id) {
        Some(entry) => {
            let entry = entry
                .as_table_like_mut()
                .ok_or_else(|| format!("config.toml: `plugins.\"{id}\"` is not a table"))?;
            entry.insert("enabled", value(enabled));
        }
        None => {
            let mut entry = table();
            entry["enabled"] = value(enabled);
            plugins.insert(id, entry);
        }
    }
    Ok(doc.to_string())
}

// ───────────────────────────── 对外接口 ─────────────────────────────

/// 跑一条输出 JSON 的命令，失败时返回 CLI 的报错。
fn run_json(app: PluginApp, args: &[&str], timeout: Duration) -> Result<Value, String> {
    let out = run_cli(app, args, timeout)?;
    if !out.success {
        return Err(failure_detail(&out));
    }
    parse_json_payload(&out.stdout)
        .ok_or_else(|| format!("Unexpected output from `{} {}`", app.tool(), args.join(" ")))
}

/// `plugin list [--available] --json`（Claude Code、Codex、MiniMax Code、Grok Build 都是这条）。
fn run_list(app: PluginApp, with_catalog: bool) -> Result<PluginListing, String> {
    let args: &[&str] = if with_catalog {
        &["plugin", "list", "--available", "--json"]
    } else {
        &["plugin", "list", "--json"]
    };
    let value = run_json(app, args, LIST_TIMEOUT)?;
    Ok(match app {
        PluginApp::Claude => parse_claude_listing(&value),
        PluginApp::Codex => parse_codex_listing(&value),
        PluginApp::Mcode => mcode::parse_listing(&value),
        PluginApp::Grok => grok::parse_listing(&value, &grok::read_config_text()),
        // OpenClaw 的列表命令不同，不走这里
        PluginApp::Openclaw => return Err(unsupported(app, "plugin list --available")),
    })
}

/// 已安装的插件。不带 `--available`，CLI 不用展开整份市场目录（Claude Code 的官方目录有
/// 几千项），每次启停、安装后的刷新都走这条便宜的路。
pub fn list_installed(app: PluginApp) -> Result<Vec<PluginEntry>, String> {
    match app {
        PluginApp::Openclaw => openclaw::list_installed(),
        _ => run_list(app, false).map(|listing| listing.installed),
    }
}

/// 所有市场里的插件（含已安装的，那几项按目录条目给出：未安装、未启用）。
/// 目录只在市场增删、刷新时变，前端用「目录 − 已安装」得到「发现」，装、卸都不用重读它。
pub fn list_catalog(app: PluginApp) -> Result<Vec<PluginEntry>, String> {
    match app {
        PluginApp::Openclaw => openclaw::list_catalog(),
        _ => run_list(app, true).map(catalog_from_listing),
    }
}

fn catalog_from_listing(listing: PluginListing) -> Vec<PluginEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut catalog = Vec::with_capacity(listing.available.len() + listing.installed.len());
    for entry in listing.available {
        if seen.insert(entry.id.clone()) {
            catalog.push(entry);
        }
    }
    for entry in listing.installed {
        if seen.insert(entry.id.clone()) {
            catalog.push(PluginEntry {
                installed: false,
                enabled: false,
                scope: None,
                manageable: true,
                removable: true,
                install_path: None,
                updated_at: None,
                ..entry
            });
        }
    }
    catalog
}

pub fn list_marketplaces(app: PluginApp) -> Result<Vec<PluginMarketplace>, String> {
    // MiniMax Code 正式版禁用了市场管理；OpenClaw 没有可增删的市场。
    if matches!(app, PluginApp::Mcode | PluginApp::Openclaw) {
        return Err(unsupported(app, "marketplace management"));
    }
    let out = run_cli(
        app,
        &["plugin", "marketplace", "list", "--json"],
        LIST_TIMEOUT,
    )?;
    if !out.success {
        return Err(failure_detail(&out));
    }
    let value = parse_json_payload(&out.stdout).unwrap_or(Value::Null);
    Ok(match app {
        PluginApp::Claude => parse_claude_marketplaces(&value),
        PluginApp::Codex => {
            let config = crate::codex_config::read_codex_config_text().unwrap_or_default();
            parse_codex_marketplaces(&value, &config)
        }
        PluginApp::Grok => grok::parse_marketplaces(&value),
        PluginApp::Mcode | PluginApp::Openclaw => Vec::new(),
    })
}

fn ok_outcome(value: &Value) -> PluginActionOutcome {
    PluginActionOutcome {
        changed: true,
        message: str_field(value, "message"),
    }
}

fn locked<T>(app: PluginApp, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let _guard = app
        .write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f()
}

/// 只看退出码的写操作（Grok Build、OpenClaw 成功时只打印一句话）。
fn run_action(
    app: PluginApp,
    args: &[&str],
    timeout: Duration,
) -> Result<PluginActionOutcome, String> {
    let out = run_cli(app, args, timeout)?;
    if !out.success {
        return Err(failure_detail(&out));
    }
    Ok(PluginActionOutcome {
        changed: true,
        message: None,
    })
}

/// 安装。Grok Build 和 OpenClaw 的 CLI 要求显式确认信任（`--trust` / `--accept-capabilities`）
/// 才肯装：前端对这两个应用先弹确认框，用户确认后才会调到这里。
pub fn install_plugin(app: PluginApp, id: &str) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    locked(app, || match app {
        // 不传 -y：市场声明了安装命令的插件要用户在终端里亲自确认，CC Switch 不代为同意。
        PluginApp::Claude => run_cli(
            app,
            &["plugin", "install", id, "--scope", "user", "--json"],
            NETWORK_TIMEOUT,
        )
        .and_then(|out| claude_action_result(&out))
        .map(|v| ok_outcome(&v)),
        // Codex 和 MiniMax Code 都是 `plugin add <name@marketplace> --json`
        PluginApp::Codex | PluginApp::Mcode => {
            run_cli(app, &["plugin", "add", id, "--json"], NETWORK_TIMEOUT)
                .and_then(|out| codex_action_result(&out))
                .map(|v| ok_outcome(&v))
        }
        PluginApp::Grok => run_action(app, &["plugin", "install", id, "--trust"], NETWORK_TIMEOUT),
        PluginApp::Openclaw => {
            let spec = openclaw::install_spec(id);
            run_action(
                app,
                &["plugins", "install", &spec, "--accept-capabilities"],
                NETWORK_TIMEOUT,
            )
        }
    })
}

pub fn uninstall_plugin(app: PluginApp, id: &str) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    locked(app, || match app {
        PluginApp::Claude => run_cli(
            app,
            &["plugin", "uninstall", id, "--scope", "user", "--json"],
            TOGGLE_TIMEOUT,
        )
        .and_then(|out| claude_action_result(&out))
        .map(|v| ok_outcome(&v)),
        PluginApp::Codex | PluginApp::Mcode => {
            run_cli(app, &["plugin", "remove", id, "--json"], TOGGLE_TIMEOUT)
                .and_then(|out| codex_action_result(&out))
                .map(|v| ok_outcome(&v))
        }
        // 不加 `--confirm` / `--force` 时 CLI 会在终端里等确认；前端已经弹过确认框
        PluginApp::Grok => run_action(
            app,
            &["plugin", "uninstall", id, "--confirm"],
            TOGGLE_TIMEOUT,
        ),
        PluginApp::Openclaw => run_action(
            app,
            &["plugins", "uninstall", id, "--force"],
            TOGGLE_TIMEOUT,
        ),
    })
}

pub fn set_plugin_enabled(
    app: PluginApp,
    id: &str,
    enabled: bool,
) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    let verb = if enabled { "enable" } else { "disable" };
    locked(app, || match app {
        PluginApp::Claude => run_cli(
            app,
            &["plugin", verb, id, "--scope", "user", "--json"],
            TOGGLE_TIMEOUT,
        )
        .and_then(|out| claude_action_result(&out))
        .map(|v| ok_outcome(&v)),
        PluginApp::Codex => {
            write_codex_plugin_enabled(id, enabled).map(|changed| PluginActionOutcome {
                changed,
                message: None,
            })
        }
        PluginApp::Mcode => run_cli(app, &["plugin", verb, id, "--json"], TOGGLE_TIMEOUT)
            .and_then(|out| codex_action_result(&out))
            .map(|v| ok_outcome(&v)),
        PluginApp::Grok => run_action(app, &["plugin", verb, id], TOGGLE_TIMEOUT),
        // 不代传 `--accept-capabilities`：要权限确认的插件，CLI 会报错让用户去终端里确认
        PluginApp::Openclaw => run_action(app, &["plugins", verb, id], TOGGLE_TIMEOUT),
    })
}

/// 改 `config.toml` 里这个插件的开关；返回文件是否有变化。调用方持有写锁。
fn write_codex_plugin_enabled(id: &str, enabled: bool) -> Result<bool, String> {
    let path = crate::codex_config::get_codex_config_path();
    let text = crate::codex_config::read_codex_config_text().map_err(|e| e.to_string())?;
    let next = set_codex_plugin_enabled_in_text(&text, id, enabled)?;
    if next == text {
        return Ok(false);
    }
    crate::config::write_text_file(&path, &next).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 更新到市场里的最新版。Claude Code、Grok Build、OpenClaw 有这个命令；Codex 先刷新市场再
/// 重新安装；MiniMax Code 的 CLI 没有更新命令。
pub fn update_plugin(app: PluginApp, id: &str) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    locked(app, || match app {
        PluginApp::Claude => {
            let out = run_cli(
                app,
                &["plugin", "update", id, "--scope", "user", "--json"],
                NETWORK_TIMEOUT,
            )?;
            let value = claude_action_result(&out)?;
            Ok(PluginActionOutcome {
                changed: value.get("updateOutcome").and_then(Value::as_str) != Some("up_to_date"),
                message: str_field(&value, "message"),
            })
        }
        PluginApp::Codex => {
            let (_, market) = split_id(id);
            // 本地市场没有快照可刷，upgrade 会报错；忽略它，照样按当前市场内容重装。
            let _ = run_cli(
                app,
                &["plugin", "marketplace", "upgrade", &market, "--json"],
                NETWORK_TIMEOUT,
            );
            // `plugin add` 会把开关写回 enabled = true；用户停用过的，装完再停用回去。
            let was_disabled = crate::codex_config::read_codex_config_text()
                .ok()
                .and_then(|text| codex_plugin_enabled_in_text(&text, id))
                == Some(false);
            let outcome = run_cli(app, &["plugin", "add", id, "--json"], NETWORK_TIMEOUT)
                .and_then(|out| codex_action_result(&out))
                .map(|v| ok_outcome(&v))?;
            if was_disabled {
                write_codex_plugin_enabled(id, false)?;
            }
            Ok(outcome)
        }
        PluginApp::Grok => {
            let out = run_cli(app, &["plugin", "update", id], NETWORK_TIMEOUT)?;
            if !out.success {
                return Err(failure_detail(&out));
            }
            Ok(PluginActionOutcome {
                changed: grok::update_changed(&out.stdout),
                message: None,
            })
        }
        PluginApp::Openclaw => {
            let out = run_cli(app, &["plugins", "update", id], NETWORK_TIMEOUT)?;
            if !out.success {
                return Err(failure_detail(&out));
            }
            Ok(PluginActionOutcome {
                changed: openclaw::update_changed(&out.stdout),
                message: None,
            })
        }
        PluginApp::Mcode => Err(unsupported(app, "plugin update")),
    })
}

pub fn add_marketplace(app: PluginApp, source: &str) -> Result<PluginActionOutcome, String> {
    let source = validate_marketplace_source(source)?;
    locked(app, || match app {
        PluginApp::Claude | PluginApp::Codex => {
            let out = run_cli(
                app,
                &["plugin", "marketplace", "add", &source, "--json"],
                NETWORK_TIMEOUT,
            )?;
            if app == PluginApp::Claude {
                claude_action_result(&out).map(|v| ok_outcome(&v))
            } else {
                codex_action_result(&out).map(|v| PluginActionOutcome {
                    changed: v.get("alreadyAdded").and_then(Value::as_bool) != Some(true),
                    message: str_field(&v, "marketplaceName"),
                })
            }
        }
        // Grok Build 的 `marketplace add` 不认 `--json`
        PluginApp::Grok => run_action(
            app,
            &["plugin", "marketplace", "add", &source],
            NETWORK_TIMEOUT,
        ),
        PluginApp::Mcode | PluginApp::Openclaw => Err(unsupported(app, "marketplace management")),
    })
}

pub fn remove_marketplace(app: PluginApp, name: &str) -> Result<PluginActionOutcome, String> {
    validate_marketplace_name(name)?;
    locked(app, || match app {
        PluginApp::Claude | PluginApp::Codex => {
            let out = run_cli(
                app,
                &["plugin", "marketplace", "remove", name, "--json"],
                TOGGLE_TIMEOUT,
            )?;
            if app == PluginApp::Claude {
                claude_action_result(&out).map(|v| ok_outcome(&v))
            } else {
                codex_action_result(&out).map(|v| ok_outcome(&v))
            }
        }
        // 连带卸掉从这个市场装的插件（CLI 自己的行为，前端确认框里写明了）
        PluginApp::Grok => run_action(
            app,
            &["plugin", "marketplace", "remove", name],
            TOGGLE_TIMEOUT,
        ),
        PluginApp::Mcode | PluginApp::Openclaw => Err(unsupported(app, "marketplace management")),
    })
}

/// 逐个刷新有名字的市场；一个失败不挡其余的。
fn refresh_each(
    app: PluginApp,
    refresh: impl Fn(&str) -> Result<(), String>,
) -> Result<PluginActionOutcome, String> {
    let markets = list_marketplaces(app)?;
    locked(app, || {
        let failures: Vec<String> = markets
            .iter()
            .filter_map(|market| {
                refresh(&market.name)
                    .err()
                    .map(|e| format!("{}: {e}", market.name))
            })
            .collect();
        if failures.is_empty() {
            Ok(PluginActionOutcome {
                changed: !markets.is_empty(),
                message: None,
            })
        } else {
            Err(failures.join("\n"))
        }
    })
}

/// 从来源重新拉取所有市场的目录。
pub fn refresh_marketplaces(app: PluginApp) -> Result<PluginActionOutcome, String> {
    match app {
        // `claude plugin marketplace update --json` 必须点名
        PluginApp::Claude => refresh_each(app, |name| {
            run_cli(
                app,
                &["plugin", "marketplace", "update", name, "--json"],
                NETWORK_TIMEOUT,
            )
            .and_then(|out| claude_action_result(&out))
            .map(|_| ())
        }),
        // 不点名时 Grok Build 只看 config.toml 里声明的来源，命令行加的不刷
        PluginApp::Grok => refresh_each(app, |name| {
            run_action(
                app,
                &["plugin", "marketplace", "update", name],
                NETWORK_TIMEOUT,
            )
            .map(|_| ())
        }),
        PluginApp::Codex => locked(app, || {
            let out = run_cli(
                app,
                &["plugin", "marketplace", "upgrade", "--json"],
                NETWORK_TIMEOUT,
            )?;
            codex_action_result(&out).map(|v| ok_outcome(&v))
        }),
        // ClawHub 托管的目录
        PluginApp::Openclaw => locked(app, || {
            run_json(
                app,
                &["plugins", "marketplace", "refresh", "--json"],
                NETWORK_TIMEOUT,
            )
            .map(|v| ok_outcome(&v))
        }),
        PluginApp::Mcode => Err(unsupported(app, "marketplace refresh")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plugin_id_requires_name_and_marketplace() {
        assert!(validate_plugin_id("frontend-design@claude-plugins-official").is_ok());
        assert!(validate_plugin_id("a.b_c@d-1").is_ok());
        for bad in [
            "frontend-design",
            "@market",
            "name@",
            "-x@m",
            "a@b@c",
            "a b@m",
            "a@m;rm",
            "--scope@user",
        ] {
            assert!(validate_plugin_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn app_ids_match_the_frontend() {
        for (app, id) in [
            (PluginApp::Claude, "claude"),
            (PluginApp::Codex, "codex"),
            (PluginApp::Grok, "grokbuild"),
            (PluginApp::Mcode, "mcode"),
            (PluginApp::Openclaw, "openclaw"),
        ] {
            assert_eq!(serde_json::to_value(app).unwrap(), json!(id));
            assert_eq!(serde_json::from_value::<PluginApp>(json!(id)).unwrap(), app);
        }
    }

    #[test]
    fn plugin_ids_are_validated_per_app() {
        assert!(validate_app_plugin_id(PluginApp::Mcode, "computer-use@official").is_ok());
        assert!(validate_app_plugin_id(PluginApp::Mcode, "computer-use").is_err());
        assert!(validate_app_plugin_id(PluginApp::Grok, "hello").is_ok());
        assert!(validate_app_plugin_id(PluginApp::Grok, "hello@mkt").is_err());
        for ok in ["memory-lancedb", "@openclaw/memory-lancedb", "a2a"] {
            assert!(
                validate_app_plugin_id(PluginApp::Openclaw, ok).is_ok(),
                "{ok}"
            );
        }
        for bad in [
            "@scope",
            "@/x",
            "@a/b/c",
            "a/b",
            "--force",
            "clawhub:x",
            "x y",
            "",
        ] {
            assert!(
                validate_app_plugin_id(PluginApp::Openclaw, bad).is_err(),
                "{bad}"
            );
            assert!(
                validate_app_plugin_id(PluginApp::Grok, bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn marketplace_source_rejects_options_and_shell_syntax() {
        assert_eq!(
            validate_marketplace_source("  anthropics/claude-plugins-official ").unwrap(),
            "anthropics/claude-plugins-official"
        );
        assert!(validate_marketplace_source("https://github.com/o/r.git").is_ok());
        assert!(validate_marketplace_source("git@github.com:o/r.git").is_ok());
        assert!(validate_marketplace_source("owner/repo@main").is_ok());
        for bad in ["", "--sparse", "a b", "o/r&calc", r"C:\x", "$(id)"] {
            assert!(validate_marketplace_source(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn json_payload_tolerates_prefix_lines() {
        assert_eq!(parse_json_payload("[]\n"), Some(json!([])));
        assert_eq!(
            parse_json_payload("Cloning…\nDone\n{\"outcome\":\"ok\"}\n"),
            Some(json!({"outcome": "ok"}))
        );
        assert_eq!(
            parse_json_payload("WARNING: x\n{\n  \"installed\": []\n}\n"),
            Some(json!({"installed": []}))
        );
        assert_eq!(parse_json_payload("nothing here"), None);
    }

    #[test]
    fn claude_listing_merges_and_dedupes() {
        let value = json!({
            "installed": [
                {
                    "id": "frontend-design@official",
                    "version": "1.1.1",
                    "scope": "user",
                    "enabled": false,
                    "installPath": "/nonexistent",
                    "lastUpdated": "2026-11-04T02:24:46.033Z"
                },
                { "id": "proj@official", "scope": "project", "enabled": true }
            ],
            "available": [
                {
                    "pluginId": "frontend-design@official",
                    "name": "frontend-design",
                    "description": "Frontend design skill",
                    "marketplaceName": "official",
                    "version": "1.1.1"
                },
                {
                    "pluginId": "superpowers@official",
                    "name": "superpowers",
                    "description": "Core skills",
                    "marketplaceName": "official"
                }
            ]
        });
        let listing = parse_claude_listing(&value);
        assert_eq!(listing.installed.len(), 2);
        let fd = &listing.installed[0];
        assert_eq!(fd.name, "frontend-design");
        assert_eq!(fd.marketplace, "official");
        assert!(fd.installed && !fd.enabled && fd.manageable);
        assert_eq!(fd.description.as_deref(), Some("Frontend design skill"));
        assert_eq!(fd.updated_at.as_deref(), Some("2026-11-04T02:24:46.033Z"));
        assert!(
            !listing.installed[1].manageable,
            "project scope is read-only"
        );
        assert_eq!(listing.available.len(), 1);
        assert_eq!(listing.available[0].id, "superpowers@official");
    }

    #[test]
    fn catalog_includes_installed_entries_as_catalog_items() {
        let listing = PluginListing {
            installed: vec![PluginEntry {
                id: "a@m".into(),
                name: "a".into(),
                installed: true,
                enabled: true,
                scope: Some("project".into()),
                manageable: false,
                install_path: Some("/x".into()),
                description: Some("from manifest".into()),
                ..Default::default()
            }],
            available: vec![PluginEntry {
                id: "b@m".into(),
                name: "b".into(),
                installable: true,
                ..Default::default()
            }],
        };
        let catalog = catalog_from_listing(listing);
        assert_eq!(
            catalog.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["b@m", "a@m"]
        );
        let a = &catalog[1];
        assert!(!a.installed && !a.enabled && a.manageable);
        assert_eq!(a.scope, None);
        assert_eq!(a.install_path, None);
        assert_eq!(a.description.as_deref(), Some("from manifest"));
    }

    #[test]
    fn claude_listing_accepts_plain_array() {
        let listing = parse_claude_listing(&json!([{ "id": "a@b", "enabled": true }]));
        assert_eq!(listing.installed.len(), 1);
        assert!(listing.installed[0].enabled);
        assert!(listing.available.is_empty());
    }

    #[test]
    fn codex_listing_reads_policy_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("game-studio");
        std::fs::create_dir_all(plugin_dir.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_dir.join(".codex-plugin/plugin.json"),
            r#"{"name":"game-studio","description":"long","interface":{"displayName":"Game Studio","shortDescription":"Ship games","category":"Coding"}}"#,
        )
        .unwrap();
        let value = json!({
            "installed": [{
                "pluginId": "game-studio@testmkt",
                "name": "game-studio",
                "marketplaceName": "testmkt",
                "version": "0.1.2",
                "installed": true,
                "enabled": false,
                "source": { "source": "local", "path": plugin_dir.to_string_lossy() },
                "installPolicy": "AVAILABLE"
            }],
            "available": [
                {
                    "pluginId": "game-studio@testmkt",
                    "installed": true,
                    "installPolicy": "AVAILABLE"
                },
                {
                    "pluginId": "locked@testmkt",
                    "name": "locked",
                    "installed": false,
                    "installPolicy": "NOT_AVAILABLE"
                }
            ]
        });
        let listing = parse_codex_listing(&value);
        assert_eq!(listing.installed.len(), 1);
        let gs = &listing.installed[0];
        assert_eq!(gs.display_name.as_deref(), Some("Game Studio"));
        assert_eq!(gs.description.as_deref(), Some("Ship games"));
        assert_eq!(gs.category.as_deref(), Some("Coding"));
        assert!(gs.installed && !gs.enabled && gs.manageable);
        assert_eq!(listing.available.len(), 1);
        assert_eq!(listing.available[0].id, "locked@testmkt");
        assert!(!listing.available[0].installable);
    }

    #[test]
    fn codex_marketplaces_mark_configured_as_removable() {
        let value = json!({ "marketplaces": [
            { "name": "openai-api-curated", "root": "C:\\Users\\me\\.codex\\.tmp\\plugins" },
            { "name": "testmkt", "root": "C:\\mkt" }
        ]});
        let config = "[marketplaces.testmkt]\nsource_type = \"local\"\nsource = '\\\\?\\C:\\mkt'\n";
        let markets = parse_codex_marketplaces(&value, config);
        assert_eq!(markets.len(), 2);
        assert!(!markets[0].removable);
        assert!(markets[1].removable);
        assert_eq!(markets[1].source.as_deref(), Some(r"C:\mkt"));
    }

    #[test]
    fn codex_enable_toggle_preserves_other_content() {
        let text = "# keep me\nmodel = \"gpt\"\n\n[plugins.\"a@m\"]\nenabled = true # trailing\n\n[mcp_servers]\n";
        let next = set_codex_plugin_enabled_in_text(text, "a@m", false).unwrap();
        assert!(next.contains("# keep me"));
        assert!(next.contains("model = \"gpt\""));
        assert!(next.contains("[mcp_servers]"));
        let doc: toml::Table = toml::from_str(&next).unwrap();
        assert_eq!(doc["plugins"]["a@m"]["enabled"].as_bool(), Some(false));

        let added = set_codex_plugin_enabled_in_text(&next, "b@m", true).unwrap();
        let doc: toml::Table = toml::from_str(&added).unwrap();
        assert_eq!(doc["plugins"]["b@m"]["enabled"].as_bool(), Some(true));
        assert_eq!(doc["plugins"]["a@m"]["enabled"].as_bool(), Some(false));

        let fresh = set_codex_plugin_enabled_in_text("", "c@m", false).unwrap();
        assert_eq!(fresh.trim(), "[plugins.\"c@m\"]\nenabled = false");
    }

    #[test]
    fn codex_enabled_reader() {
        let text = "[plugins.\"a@m\"]
enabled = false
[plugins.\"b@m\"]
";
        assert_eq!(codex_plugin_enabled_in_text(text, "a@m"), Some(false));
        assert_eq!(codex_plugin_enabled_in_text(text, "b@m"), None);
        assert_eq!(codex_plugin_enabled_in_text(text, "c@m"), None);
        assert_eq!(codex_plugin_enabled_in_text("bad = [", "a@m"), None);
    }

    #[test]
    fn codex_enable_rejects_unparsable_config() {
        assert!(set_codex_plugin_enabled_in_text("not = [valid", "a@m", true).is_err());
    }

    #[test]
    fn failure_detail_strips_noise() {
        let out = CliOutput {
            success: false,
            stdout: String::new(),
            stderr: "WARNING: proceeding\nError: plugin `nope` was not found\n".into(),
        };
        assert_eq!(failure_detail(&out), "plugin `nope` was not found");
    }

    #[test]
    fn claude_failed_outcome_uses_message() {
        let out = CliOutput {
            success: false,
            stdout: "{\"outcome\":\"failed\",\"message\":\"Plugin not found\"}\n".into(),
            stderr: "✘ Failed".into(),
        };
        assert_eq!(claude_action_result(&out).unwrap_err(), "Plugin not found");
    }

    #[test]
    fn default_claude_dir_detection_handles_wsl() {
        assert!(is_default_claude_dir(Path::new(
            r"\\wsl$\Ubuntu\home\jane\.claude"
        )));
        assert!(is_default_claude_dir(Path::new(
            r"\\wsl.localhost\Ubuntu\root\.claude\"
        )));
        assert!(!is_default_claude_dir(Path::new(
            r"\\wsl$\Ubuntu\home\jane\alt-claude"
        )));
        assert!(!is_default_claude_dir(Path::new("/opt/claude-alt")));
    }
}
