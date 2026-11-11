//! 各应用的插件管理。
//!
//! 插件状态（已装列表、启用开关、市场、缓存目录）都归应用自己的 CLI 管，格式随版本变，
//! 这里尽量不碰它们的私有文件，而是调官方 CLI 的 `--json` 输出：
//!
//! - Claude Code：`claude plugin list|install|uninstall|enable|disable|update` 和
//!   `claude plugin marketplace list|add|remove|update`。
//! - Codex：`codex plugin list|add|remove` 和 `codex plugin marketplace list|add|remove|upgrade`。
//!   Codex 没有启用 / 停用命令，开关就是 `config.toml` 的 `[plugins."<id>"] enabled`，
//!   经写入引擎原位改这一个键。
//! - Grok Build、MiniMax Code、OpenClaw、Pi：见各自的子模块。
//!
//! 写锁只在 CC Switch 自己写文件时拿（Codex 和 Pi 的启停、Pi 本地包的卸载）。CLI 改自己的配置和用户手改
//! 文件是一回事：切换供应商写之前本来就会重读比对，所以 CLI 调用期间（安装、更新最长
//! 300 秒）不拿锁，不挡切换、托盘和退出。CLI 的 stdin 是 null，要确认的操作立刻失败。
//!
//! CLI 读的配置目录和 CC Switch 读的必须是同一份：设置里覆盖了目录时，用各应用的环境变量
//! （`CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`GROK_HOME`、`PI_CODING_AGENT_DIR`……）告诉 CLI。

#[cfg(test)]
mod cli_fixtures;
mod grok;
mod mcode;
mod openclaw;
mod pi;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app_config::AppType;
use crate::database::Database;
use crate::live::engine::LiveFile;
use crate::live::patch::toml::TomlPatch;
use crate::live::patch::KeyPath;
use crate::mode::operation::{AppWrite, FileChange};
use crate::mode::state::{op, PendingTarget};

/// 工具没装（或 WSL 里找不到）时返回的错误码，前端据此显示安装提示。
pub const CLI_NOT_INSTALLED: &str = "PLUGIN_CLI_NOT_INSTALLED";

/// Claude Code 要用户亲自确认市场声明的命令（命令来源的插件、取归档用的 headersHelper）
/// 时返回的错误码。CC Switch 不传 `-y` 代为同意，前端提示用户去终端里装。
pub const NEEDS_TERMINAL_CONFIRM: &str = "PLUGIN_NEEDS_TERMINAL_CONFIRM";

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
    Pi,
}

impl PluginApp {
    fn tool(self) -> &'static str {
        match self {
            PluginApp::Claude => "claude",
            PluginApp::Codex => "codex",
            PluginApp::Grok => "grok",
            PluginApp::Mcode => "mcode",
            PluginApp::Openclaw => "openclaw",
            PluginApp::Pi => "pi",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    /// 交给 CLI 的插件 id：Claude Code / Codex / MiniMax Code 是 `name@marketplace`，
    /// Grok Build 是插件名，OpenClaw 是插件 id（目录里是 ClawHub 包名），Pi 是包来源
    /// （`npm:<包名>`、`git:…`、本地路径）。
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
        PluginApp::Pi => return pi::validate_source(id),
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
        PluginApp::Pi => {
            let dir = crate::pi_config::get_pi_agent_dir()
                .unwrap_or_else(|_| crate::config::get_home_dir().join(".pi").join("agent"));
            let env = vec![("PI_CODING_AGENT_DIR", dir.to_string_lossy().into_owned())];
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

/// 给用户看的报错最多这么多行 / 字符，超出的截掉。
const MAX_DETAIL_LINES: usize = 10;
const MAX_DETAIL_CHARS: usize = 800;

/// 失败时给用户看的那句话：去掉 Codex 的 `WARNING:` 提示行和 `Error:` / `✘` 前缀。
/// CLI 自己崩溃时只留异常和 Node 版本；其余按行数、字符数截断。
fn failure_detail(out: &CliOutput) -> String {
    if let Some(summary) = node_crash_summary(&out.stderr) {
        return summary;
    }
    truncate_detail(raw_failure_detail(out))
}

/// Node 未捕获异常的 stderr 是「出错位置 / 出错的那行源码 / `^` / `XxxError: …` / 调用栈 /
/// `Node.js vX`」。打包压缩过的 CLI（如 Pi）那行源码能有上百 KB，只留异常那行和 Node 版本。
fn node_crash_summary(stderr: &str) -> Option<String> {
    let mut lines = stderr.lines().map(str::trim);
    let node = lines
        .clone()
        .rev()
        .find(|line| !line.is_empty())
        .filter(|line| line.starts_with("Node.js v"))?;
    let error = lines.find(|line| is_js_error_line(line))?;
    Some(truncate_detail(format!("{error} ({node})")))
}

/// `TypeError: …`、`Error: …`、`Error [ERR_REQUIRE_ESM]: …` 这样的异常行。
fn is_js_error_line(line: &str) -> bool {
    let Some((head, _)) = line.split_once(':') else {
        return false;
    };
    let name = head.split_once(" [").map_or(head, |(name, _)| name);
    name.len() <= 40 && name.ends_with("Error") && name.chars().all(|c| c.is_ascii_alphanumeric())
}

fn truncate_detail(text: String) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    let mut cut = lines.len() > MAX_DETAIL_LINES;
    lines.truncate(MAX_DETAIL_LINES);
    let mut detail = lines.join("\n");
    if detail.chars().count() > MAX_DETAIL_CHARS {
        detail = detail.chars().take(MAX_DETAIL_CHARS).collect();
        cut = true;
    }
    if cut {
        detail = format!("{}…", detail.trim_end());
    }
    detail
}

fn raw_failure_detail(out: &CliOutput) -> String {
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
///
/// 要确认命令时失败结果里带 `shownCommand`（stdin 不是终端，CLI 只显示不执行，立刻返回）。
fn claude_action_result(out: &CliOutput) -> Result<Value, String> {
    let payload = parse_json_payload(&out.stdout);
    if let Some(value) = payload.as_ref() {
        if value.get("outcome").and_then(Value::as_str) == Some("ok") && out.success {
            return Ok(value.clone());
        }
        if value.get("shownCommand").is_some_and(Value::is_object) {
            return Err(NEEDS_TERMINAL_CONFIRM.to_string());
        }
        if let Some(message) = str_field(value, "message") {
            return Err(message);
        }
    }
    Err(failure_detail(out))
}

/// `claude plugin update --json`：已是最新版时 `updateOutcome` 为 `up_to_date`。
fn claude_update_outcome(value: &Value) -> PluginActionOutcome {
    PluginActionOutcome {
        changed: value.get("updateOutcome").and_then(Value::as_str) != Some("up_to_date"),
        message: str_field(value, "message"),
    }
}

/// `codex plugin marketplace add --json`：加过的再加一次 `alreadyAdded` 为 true。
fn codex_marketplace_add_outcome(value: &Value) -> PluginActionOutcome {
    PluginActionOutcome {
        changed: value.get("alreadyAdded").and_then(Value::as_bool) != Some(true),
        message: str_field(value, "marketplaceName"),
    }
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

fn codex_marketplace_needs_upgrade(config_text: &str, name: &str) -> Result<bool, String> {
    let doc = config_text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("Failed to parse config.toml: {e}"))?;
    Ok(doc
        .get("marketplaces")
        .and_then(|markets| markets.get(name))
        .and_then(|market| market.get("source_type"))
        .and_then(toml_edit::Item::as_str)
        == Some("git"))
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

/// 设置 `config.toml` 里 `[plugins."<id>"] enabled` 的补丁，其余内容原样保留。
fn codex_plugin_enabled_patch(id: &str, enabled: bool) -> TomlPatch {
    TomlPatch {
        set: vec![(
            KeyPath::new(&["plugins", id, "enabled"]),
            toml_edit::value(enabled),
        )],
        ..TomlPatch::default()
    }
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
        // OpenClaw、Pi 的列表命令不同，不走这里
        PluginApp::Openclaw | PluginApp::Pi => {
            return Err(unsupported(app, "plugin list --available"))
        }
    })
}

/// 已安装的插件。不带 `--available`，CLI 不用展开整份市场目录（Claude Code 的官方目录有
/// 几千项），每次启停、安装后的刷新都走这条便宜的路。
pub fn list_installed(app: PluginApp) -> Result<Vec<PluginEntry>, String> {
    match app {
        PluginApp::Openclaw => openclaw::list_installed(),
        PluginApp::Pi => pi::list_installed(),
        _ => run_list(app, false).map(|listing| listing.installed),
    }
}

/// 所有市场里的插件（含已安装的，那几项按目录条目给出：未安装、未启用）。
/// 目录只在市场增删、刷新时变，前端用「目录 − 已安装」得到「发现」，装、卸都不用重读它。
pub fn list_catalog(app: PluginApp) -> Result<Vec<PluginEntry>, String> {
    match app {
        PluginApp::Openclaw => openclaw::list_catalog(),
        PluginApp::Pi => pi::list_catalog(),
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
    // MiniMax Code 正式版禁用了市场管理；OpenClaw、Pi 没有可增删的市场。
    if matches!(app, PluginApp::Mcode | PluginApp::Openclaw | PluginApp::Pi) {
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
        PluginApp::Mcode | PluginApp::Openclaw | PluginApp::Pi => Vec::new(),
    })
}

fn ok_outcome(value: &Value) -> PluginActionOutcome {
    PluginActionOutcome {
        changed: true,
        message: str_field(value, "message"),
    }
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
    match app {
        // 不传 -y：市场声明了安装命令的插件要用户在终端里亲自确认，CC Switch 不代为同意。
        // CLI 的 stdin 是 null，这种插件会立刻失败（`NEEDS_TERMINAL_CONFIRM`），不会等到超时。
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
        // 包能执行代码：前端先弹确认框
        PluginApp::Pi => {
            if !pi::can_pass_to_cli(id) {
                return Err(pi::terminal_hint("install", id));
            }
            run_action(app, &["install", id], NETWORK_TIMEOUT)
        }
    }
}

pub fn uninstall_plugin(
    db: &Database,
    app: PluginApp,
    id: &str,
) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    match app {
        PluginApp::Claude => run_cli(
            app,
            &[
                "plugin",
                "uninstall",
                id,
                "--scope",
                "user",
                "--keep-data",
                "--json",
            ],
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
        // npm / git 来源要删安装目录（npm uninstall 可能要联网）
        PluginApp::Pi if pi::can_pass_to_cli(id) => {
            run_action(app, &["remove", id], NETWORK_TIMEOUT)
        }
        PluginApp::Pi if pi::is_local_source(id) => {
            pi::remove_local_entry(db, id).map(|changed| PluginActionOutcome {
                changed,
                message: None,
            })
        }
        PluginApp::Pi => Err(pi::terminal_hint("remove", id)),
    }
}

pub fn set_plugin_enabled(
    db: &Database,
    app: PluginApp,
    id: &str,
    enabled: bool,
) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    let verb = if enabled { "enable" } else { "disable" };
    match app {
        PluginApp::Claude => run_cli(
            app,
            &["plugin", verb, id, "--scope", "user", "--json"],
            TOGGLE_TIMEOUT,
        )
        .and_then(|out| claude_action_result(&out))
        .map(|v| ok_outcome(&v)),
        PluginApp::Codex => {
            write_codex_plugin_enabled(db, id, enabled).map(|changed| PluginActionOutcome {
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
        // Pi 没有包级开关命令，改 settings.json 里的资源过滤
        PluginApp::Pi => pi::set_enabled(db, id, enabled).map(|changed| PluginActionOutcome {
            changed,
            message: None,
        }),
    }
}

/// 改 `config.toml` 里这个插件的开关；返回文件是否有变化。
///
/// 插件管理里只有这一步是 CC Switch 自己写文件，和切换供应商写 `config.toml` 走同一条路：
/// 拿 Codex 的写锁、先补完上一次没做完的写入，经写入引擎按 0600 发布（文件里有第三方
/// Key）；发布前重读比对，CLI 或用户恰好改过就以新内容为底重算。
fn write_codex_plugin_enabled(db: &Database, id: &str, enabled: bool) -> Result<bool, String> {
    let write = AppWrite::begin(db, AppType::Codex.as_str()).map_err(|e| e.to_string())?;
    let report = write
        .run(
            op::APPLY,
            &[FileChange {
                file: LiveFile::private(crate::codex_config::get_codex_config_path()),
                patch: &codex_plugin_enabled_patch(id, enabled),
            }],
            PendingTarget::default(),
        )
        .map_err(|e| e.to_string())?;
    Ok(!report.changed.is_empty())
}

/// 更新到市场里的最新版。Claude Code、Grok Build、OpenClaw、Pi 有这个命令；Codex 先刷新市场再
/// 重新安装；MiniMax Code 的 CLI 没有更新命令。
pub fn update_plugin(
    db: &Database,
    app: PluginApp,
    id: &str,
) -> Result<PluginActionOutcome, String> {
    validate_app_plugin_id(app, id)?;
    match app {
        PluginApp::Claude => {
            let out = run_cli(
                app,
                &["plugin", "update", id, "--scope", "user", "--json"],
                NETWORK_TIMEOUT,
            )?;
            claude_action_result(&out).map(|v| claude_update_outcome(&v))
        }
        PluginApp::Codex => {
            let (_, market) = split_id(id);
            let config =
                crate::codex_config::read_codex_config_text().map_err(|e| e.to_string())?;
            // upgrade 只适用于配置的 Git 市场；本地和内置/托管市场直接交给 add。
            if codex_marketplace_needs_upgrade(&config, &market)? {
                let out = run_cli(
                    app,
                    &["plugin", "marketplace", "upgrade", &market, "--json"],
                    NETWORK_TIMEOUT,
                )?;
                codex_action_result(&out)?;
            }
            // `plugin add` 会把开关写回 enabled = true；用户停用过的，装完再停用回去。
            let was_disabled = codex_plugin_enabled_in_text(&config, id) == Some(false);
            let outcome = run_cli(app, &["plugin", "add", id, "--json"], NETWORK_TIMEOUT)
                .and_then(|out| codex_action_result(&out))
                .map(|v| ok_outcome(&v))?;
            if was_disabled {
                write_codex_plugin_enabled(db, id, false)?;
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
        // 本地来源直接加载目录，没有可更新的
        PluginApp::Pi if pi::is_local_source(id) => Ok(PluginActionOutcome {
            changed: false,
            message: None,
        }),
        PluginApp::Pi => {
            if !pi::can_pass_to_cli(id) {
                return Err(pi::terminal_hint("update", id));
            }
            let out = run_cli(app, &["update", id], NETWORK_TIMEOUT)?;
            if !out.success {
                return Err(failure_detail(&out));
            }
            Ok(PluginActionOutcome {
                changed: pi::update_changed(&out.stdout),
                message: None,
            })
        }
        PluginApp::Mcode => Err(unsupported(app, "plugin update")),
    }
}

pub fn add_marketplace(app: PluginApp, source: &str) -> Result<PluginActionOutcome, String> {
    let source = validate_marketplace_source(source)?;
    match app {
        PluginApp::Claude | PluginApp::Codex => {
            let out = run_cli(
                app,
                &["plugin", "marketplace", "add", &source, "--json"],
                NETWORK_TIMEOUT,
            )?;
            if app == PluginApp::Claude {
                claude_action_result(&out).map(|v| ok_outcome(&v))
            } else {
                codex_action_result(&out).map(|v| codex_marketplace_add_outcome(&v))
            }
        }
        // Grok Build 的 `marketplace add` 不认 `--json`
        PluginApp::Grok => run_action(
            app,
            &["plugin", "marketplace", "add", &source],
            NETWORK_TIMEOUT,
        ),
        PluginApp::Mcode | PluginApp::Openclaw | PluginApp::Pi => {
            Err(unsupported(app, "marketplace management"))
        }
    }
}

pub fn remove_marketplace(app: PluginApp, name: &str) -> Result<PluginActionOutcome, String> {
    validate_marketplace_name(name)?;
    match app {
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
        PluginApp::Mcode | PluginApp::Openclaw | PluginApp::Pi => {
            Err(unsupported(app, "marketplace management"))
        }
    }
}

/// 逐个刷新有名字的市场；一个失败不挡其余的。
fn refresh_each(
    app: PluginApp,
    refresh: impl Fn(&str) -> Result<(), String>,
) -> Result<PluginActionOutcome, String> {
    let markets = list_marketplaces(app)?;
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
        PluginApp::Codex => run_cli(
            app,
            &["plugin", "marketplace", "upgrade", "--json"],
            NETWORK_TIMEOUT,
        )
        .and_then(|out| codex_action_result(&out))
        .map(|v| ok_outcome(&v)),
        // ClawHub 托管的目录
        PluginApp::Openclaw => run_json(
            app,
            &["plugins", "marketplace", "refresh", "--json"],
            NETWORK_TIMEOUT,
        )
        .map(|v| ok_outcome(&v)),
        // 目录是直接查 npm registry 的，没有本地缓存要刷新；前端随后重读目录
        PluginApp::Pi => Ok(PluginActionOutcome {
            changed: true,
            message: None,
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
            (PluginApp::Pi, "pi"),
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
    fn codex_marketplace_upgrade_only_applies_to_configured_git_sources() {
        let config = r#"
[marketplaces.team]
source_type = "git"
source = "https://github.com/team/plugins.git"

[marketplaces.local]
source_type = "local"
source = "/plugins"
"#;
        assert!(codex_marketplace_needs_upgrade(config, "team").unwrap());
        assert!(!codex_marketplace_needs_upgrade(config, "local").unwrap());
        assert!(!codex_marketplace_needs_upgrade(config, "openai-api-curated").unwrap());
        assert!(!codex_marketplace_needs_upgrade("", "openai-global").unwrap());
        assert!(codex_marketplace_needs_upgrade("not = [valid", "team").is_err());
    }

    fn apply_toggle(text: &str, id: &str, enabled: bool) -> Result<String, String> {
        use crate::live::patch::LivePatch;
        codex_plugin_enabled_patch(id, enabled)
            .apply(Path::new("config.toml"), Some(text.as_bytes()))
            .map(|bytes| String::from_utf8(bytes).unwrap())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn codex_enable_toggle_preserves_other_content() {
        let text = "# keep me\nmodel = \"gpt\"\n\n[plugins.\"a@m\"]\nenabled = true # trailing\n\n[mcp_servers]\n";
        let next = apply_toggle(text, "a@m", false).unwrap();
        assert!(next.contains("# keep me"));
        assert!(next.contains("model = \"gpt\""));
        assert!(next.contains("[mcp_servers]"));
        assert!(next.contains("enabled = false # trailing"));
        let doc: toml::Table = toml::from_str(&next).unwrap();
        assert_eq!(doc["plugins"]["a@m"]["enabled"].as_bool(), Some(false));

        let added = apply_toggle(&next, "b@m", true).unwrap();
        let doc: toml::Table = toml::from_str(&added).unwrap();
        assert_eq!(doc["plugins"]["b@m"]["enabled"].as_bool(), Some(true));
        assert_eq!(doc["plugins"]["a@m"]["enabled"].as_bool(), Some(false));

        let fresh = apply_toggle("", "c@m", false).unwrap();
        assert_eq!(fresh.trim(), "[plugins.\"c@m\"]\nenabled = false");

        let inline =
            apply_toggle("plugins = { \"a@m\" = { enabled = true } }\n", "a@m", false).unwrap();
        let doc: toml::Table = toml::from_str(&inline).unwrap();
        assert_eq!(doc["plugins"]["a@m"]["enabled"].as_bool(), Some(false));
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
        assert!(apply_toggle("not = [valid", "a@m", true).is_err());
        assert!(apply_toggle("plugins = 1\n", "a@m", true).is_err());
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

    fn failed_with_stderr(stderr: String) -> CliOutput {
        CliOutput {
            success: false,
            stdout: String::new(),
            stderr,
        }
    }

    /// Pi 1.5.2 在 Node 22.12 上崩溃时的 stderr 形状：出错那行是整段压缩源码（实测约 118 KB）。
    #[test]
    fn failure_detail_summarizes_node_crash() {
        let minified = "var a=1;".repeat(20_000);
        let stderr = format!(
            "file:///D:/npm/node_modules/@earendil-works/pi-coding-agent/dist/bundle/chunks/chunk.js:47\n\
             {minified}\n\
             \u{20}   ^\n\n\
             TypeError: Cannot read properties of undefined (reading 'bind')\n\
             \u{20}   at file:///D:/npm/chunk.js:277:6032\n\
             \u{20}   at ModuleJobSync.runSync (node:internal/modules/esm/module_job:395:35)\n\n\
             Node.js v22.12.0\n"
        );
        assert_eq!(
            failure_detail(&failed_with_stderr(stderr)),
            "TypeError: Cannot read properties of undefined (reading 'bind') (Node.js v22.12.0)"
        );

        let stderr = "node:internal/modules/cjs/loader:1228\n  throw err;\n  ^\n\n\
                      Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'x'\n    at foo\n\nNode.js v24.20.0\n";
        assert_eq!(
            failure_detail(&failed_with_stderr(stderr.into())),
            "Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'x' (Node.js v24.20.0)"
        );
    }

    #[test]
    fn failure_detail_truncates_long_output() {
        // 不是 Node 崩溃：一行超长
        let detail = failure_detail(&failed_with_stderr("x".repeat(5_000)));
        assert_eq!(detail.chars().count(), MAX_DETAIL_CHARS + 1);
        assert!(detail.ends_with('…'));

        // 行数太多
        let many: String = (0..50).map(|i| format!("line {i}\n")).collect();
        let detail = failure_detail(&failed_with_stderr(many));
        assert_eq!(detail.lines().count(), MAX_DETAIL_LINES);
        assert!(detail.starts_with("line 0\n") && detail.ends_with("line 9…"));

        // 短的原样保留（前端靠错误码匹配）
        assert_eq!(
            failure_detail(&failed_with_stderr("PLUGIN_AUTH_REQUIRED\n".into())),
            "PLUGIN_AUTH_REQUIRED"
        );
        // 提到 Node 版本但不是崩溃的提示也原样保留
        let mcode =
            "Minimax Code supports Node.js 22.19+, 24, 25, or 26; current version is 22.12.0.";
        assert_eq!(failure_detail(&failed_with_stderr(mcode.into())), mcode);
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

    /// 临时 HOME，结束时还原（`CC_SWITCH_TEST_HOME` 是进程级的，用到它的测试都要 `#[serial]`）。
    struct HomeGuard(tempfile::TempDir, Option<std::ffi::OsString>);

    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().unwrap();
            Self(dir, previous)
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.1.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    /// Codex 的启停开关经写入引擎写 `config.toml`：只改这一个键、按 0600 写、拿 Codex 的写锁。
    #[test]
    #[serial_test::serial]
    fn codex_toggle_goes_through_the_write_engine() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let home = HomeGuard::new();
        let config = home.0.path().join(".codex").join("config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let before = "model_provider = \"custom\"\n\n[model_providers.custom]\nexperimental_bearer_token = \"sk-third-party\"\n\n[plugins.\"a@m\"]\nenabled = true # keep\n";
        std::fs::write(&config, before).unwrap();
        let db = Arc::new(Database::memory().unwrap());

        // 别的写入方拿着 Codex 的写锁时，开关要等它放手
        let guard = crate::live::engine::lock_app("codex");
        let done = Arc::new(AtomicBool::new(false));
        let worker = {
            let (db, done) = (db.clone(), done.clone());
            std::thread::spawn(move || {
                let outcome = set_plugin_enabled(&db, PluginApp::Codex, "a@m", false);
                done.store(true, Ordering::SeqCst);
                outcome
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !done.load(Ordering::SeqCst),
            "toggle must wait for the lock"
        );
        drop(guard);
        let outcome = worker.join().unwrap().unwrap();
        assert!(outcome.changed);

        let after = std::fs::read_to_string(&config).unwrap();
        assert_eq!(
            after,
            before.replace("enabled = true # keep", "enabled = false # keep")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&config).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let again = set_plugin_enabled(&db, PluginApp::Codex, "a@m", false).unwrap();
        assert!(!again.changed);
    }
}
