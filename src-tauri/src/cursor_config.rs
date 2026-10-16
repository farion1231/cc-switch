//! Cursor IDE 配置文件读写模块
//!
//! Cursor 是一个基于 VS Code 的 AI 编辑器。与 CLI 工具不同，Cursor 没有原生的
//! "供应商配置文件"。CC Switch 管理 `~/.cursor/env.json` 作为 SSOT 供应商配置，
//! 并通过 CC Switch 本地代理拦截和路由 Cursor 的 API 调用。
//!
//! ## 配置结构
//!
//! CC Switch 内部使用扁平的标准格式存储 Cursor 供应商配置：
//! ```json
//! {
//!   "baseUrl": "https://api.anthropic.com",
//!   "apiKey": "sk-ant-...",
//!   "model": "claude-sonnet-4-20250514"
//! }
//! ```
//!
//! 写入 `~/.cursor/env.json` 时，根据 base URL 转换为对应的环境变量名：
//! - Anthropic 兼容: `ANTHROPIC_BASE_URL` + `ANTHROPIC_API_KEY`
//! - OpenAI 兼容: `OPENAI_BASE_URL` + `OPENAI_API_KEY`
//!
//! Cursor 为 switch 模式（非 additive）：env.json 仅包含当前供应商的配置。

use crate::config::write_json_file;
use crate::error::AppError;
use crate::settings::get_cursor_override_dir;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Cursor 配置目录
///
/// 解析顺序:
/// 1. CCS 设置 `cursor_config_dir`（显式覆盖）
/// 2. 平台默认（`~/.cursor/`）
pub fn get_cursor_dir() -> PathBuf {
    if let Some(override_dir) = get_cursor_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".cursor")
}

/// 获取 Cursor env.json 配置文件路径
///
/// 返回 `~/.cursor/env.json`
pub fn get_cursor_env_path() -> PathBuf {
    get_cursor_dir().join("env.json")
}

/// 确保 Cursor 配置目录存在
fn ensure_cursor_dir() -> Result<(), AppError> {
    let dir = get_cursor_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        log::info!("Created Cursor config directory: {:?}", dir);
    }
    Ok(())
}

// ============================================================================
// Read/Write Functions
// ============================================================================

/// 读取 Cursor env.json 配置
///
/// 如果文件不存在，返回空 JSON 对象。
pub fn read_cursor_env() -> Result<Value, AppError> {
    let path = get_cursor_env_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    if content.trim().is_empty() {
        return Ok(json!({}));
    }

    serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Cursor env.json: {}: {e}",
            path.display()
        ))
    })
}

/// 写入 Cursor env.json 配置
pub fn write_cursor_env(config: &Value) -> Result<(), AppError> {
    ensure_cursor_dir()?;
    let path = get_cursor_env_path();
    write_json_file(&path, config)
}

// ============================================================================
// Provider Management
// ============================================================================

/// 获取所有提供商配置
pub fn get_providers() -> Result<Value, AppError> {
    read_cursor_env()
}

/// 获取单个提供商配置
// 目前仅测试使用，保留给后续命令接入
#[allow(dead_code)]
pub fn get_provider(id: &str) -> Result<Option<Value>, AppError> {
    let config = read_cursor_env()?;
    Ok(config.get(id).cloned())
}

/// 将 CC Switch 标准扁平格式 `{ baseUrl, apiKey, model }` 转换为
/// env.json 的环境变量对象：
/// - Anthropic 兼容: `ANTHROPIC_BASE_URL` + `ANTHROPIC_API_KEY`
/// - OpenAI 兼容: `OPENAI_BASE_URL` + `OPENAI_API_KEY`
///
/// NOTE: 不写 MODEL env var。Cursor 读到不认识的模型名会校验报错。
/// 模型映射由 cc-switch proxy 的 modelCatalog 处理，不通过 env.json 传递。
// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
pub fn provider_config_to_env(provider_config: &Value) -> Value {
    let base_url = provider_config
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let api_key = provider_config
        .get("apiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Determine env-var prefix from the base URL heuristically.
    // Default to Anthropic-style keys (Cursor's primary model provider).
    let is_openai = base_url.contains("openai.com")
        || base_url.contains("deepseek.com")
        || base_url.contains("api.openai.com");
    let (base_url_key, api_key_key) = if is_openai {
        ("OPENAI_BASE_URL", "OPENAI_API_KEY")
    } else {
        ("ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY")
    };

    let mut env = serde_json::Map::new();
    if !base_url.is_empty() {
        env.insert(
            base_url_key.to_string(),
            Value::String(base_url.to_string()),
        );
    }
    if !api_key.is_empty() {
        env.insert(api_key_key.to_string(), Value::String(api_key.to_string()));
    }
    Value::Object(env)
}

/// 写入当前供应商配置到 env.json。
///
/// **不再写入**：Cursor 的 "Override OpenAI Base URL"/API Key 权威来源是云端账户同步，
/// 本地写 env.json 一律无效（2026-08-05 实测），且 Cursor 云端 SSRF 拦私网。
/// Cursor 配置统一走手工粘贴（公网隧道地址 + ccsk 密钥），见 #5 review。
pub fn set_provider(_provider_config: &Value) -> Result<(), AppError> {
    log::info!("Cursor set_provider：不写 env.json（云端账户同步，本地写入无效），使用手工粘贴");
    Ok(())
}

/// 删除提供商配置
// 目前仅测试使用，保留给后续命令接入
#[allow(dead_code)]
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_cursor_env()?;

    if let Some(obj) = config.as_object_mut() {
        obj.remove(id);
    }

    write_cursor_env(&config)
}

// ============================================================================
// Subscription Status Bypass (for proxy takeover)
// ============================================================================

/// Cursor 状态数据库路径
// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
fn get_cursor_state_db_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    } else if cfg!(target_os = "windows") {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Cursor/User/globalStorage/state.vscdb")
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Cursor/User/globalStorage/state.vscdb")
    }
}

// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
const SUBSCRIPTION_KEY: &str = "cursorAuth/stripeSubscriptionStatus";

/// 读取 Cursor 本地缓存的订阅状态
// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
pub fn read_subscription_status_internal() -> Result<Option<String>, AppError> {
    let path = get_cursor_state_db_path();
    if !path.exists() {
        return Ok(None);
    }
    let conn = rusqlite::Connection::open(&path)
        .map_err(|e| AppError::Config(format!("Failed to open Cursor state DB: {e}")))?;
    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key = ?1")
        .map_err(|e| AppError::Config(format!("Failed to prepare query: {e}")))?;
    let result: Option<String> = stmt
        .query_row([SUBSCRIPTION_KEY], |row| row.get(0))
        .optional()
        .map_err(|e| AppError::Config(format!("Failed to read subscription status: {e}")))?;
    Ok(result)
}

/// 写入 Cursor 本地缓存的订阅状态，绕过客户端计费拦截。
/// 保存原始值以便恢复。
// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
fn write_subscription_status(status: &str) -> Result<(), AppError> {
    let path = get_cursor_state_db_path();
    let conn = rusqlite::Connection::open(&path)
        .map_err(|e| AppError::Config(format!("Failed to open Cursor state DB: {e}")))?;
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)",
        [SUBSCRIPTION_KEY, status],
    )
    .map_err(|e| AppError::Config(format!("Failed to write subscription status: {e}")))?;
    Ok(())
}

/// Proxy 接管时绕过 Cursor 本地订阅检查
// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
pub fn bypass_subscription_check() -> Result<(), AppError> {
    let current = read_subscription_status_internal()?;
    if current.as_deref() == Some("active") {
        return Ok(()); // Already active, nothing to do
    }
    // Save original as backup
    if let Some(ref orig) = current {
        // Write "active" to bypass the local check
        write_subscription_status("active")?;
        log::info!("Cursor subscription status bypassed (was: {orig})");
    } else {
        write_subscription_status("active")?;
        log::info!("Cursor subscription status set to active (no previous value)");
    }
    Ok(())
}

/// 恢复 Cursor 订阅状态到原始值
// TODO(rebase): Cursor 订阅绕过 / 公网隧道这些 helper 原本由旧版 services/proxy.rs 的
// takeover 流程调用；上游已把 takeover 重构进 mode::controller，二者尚未重新接线。
// 未经接线不可贸然调用（bypass 必须与 restore 成对，否则会永久改动用户 Cursor 订阅态）。
#[allow(dead_code)]
pub fn restore_subscription_status(original: Option<&str>) -> Result<(), AppError> {
    match original {
        Some(status) => {
            write_subscription_status(status)?;
            log::info!("Cursor subscription status restored to: {status}");
        }
        None => {
            // Remove our injected value
            write_subscription_status("")?;
            log::info!("Cursor subscription status cleared");
        }
    }
    Ok(())
}

// ============================================================================
// MCP Configuration Helpers (for future use)
// ============================================================================

/// 获取项目级 `.cursor/mcp.json` 路径
///
/// Cursor 的 MCP 配置是项目级的，存储在项目根目录的 `.cursor/mcp.json` 中。
/// 格式为标准的 MCP 服务器配置。
#[allow(dead_code)]
pub fn get_cursor_project_mcp_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".cursor").join("mcp.json")
}

/// 获取全局 Cursor MCP 配置路径
pub fn get_cursor_global_mcp_path() -> PathBuf {
    get_cursor_dir().join("mcp.json")
}

/// 读取 Cursor MCP 配置中的服务器 map
///
/// Cursor MCP 配置格式与 VS Code 一致：
/// ```json
/// { "mcpServers": { "server-id": { "command": "...", "args": [], "env": {} } } }
/// ```
pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let path = get_cursor_global_mcp_path();

    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    if content.trim().is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let config: Value = serde_json::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Cursor mcp.json: {}: {e}",
            path.display()
        ))
    })?;

    Ok(config
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default())
}

/// 写入 Cursor MCP 配置的服务器 map
pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    ensure_cursor_dir()?;
    let path = get_cursor_global_mcp_path();
    let config = json!({ "mcpServers": servers });
    write_json_file(&path, &config)
}

// ============================================================================
// BYOK（Settings → Models 的 Override OpenAI Base URL / API Key）
// ============================================================================

/// Cursor 存 applicationUser 持久化存储的那行 key（前缀匹配，抗版本升级）。
const BYOK_KEY_PREFIX: &str =
    "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";

/// 回滚点目录。
///
/// 绝不能硬编码 `/tmp`——普通 Windows 没有这个目录，而落盘发生在写库之前，
/// 首次配置会直接停在 NotFound（PR #8270 评审 P2）。故取系统临时目录。
fn byok_backup_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Cursor 本地 BYOK 现状（`cursor_byok_status` 的载荷）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorByokState {
    pub base_url: Option<String>,
    pub use_openai_key: bool,
    pub model_count: usize,
    /// 已写入 Cursor 的可用模型名列表。换供应商但仍在同一条隧道时 `baseUrl` 不变，
    /// 前端只能靠这份列表判断「目标模型真的已经写进去了」。
    pub model_ids: Vec<String>,
}

/// 写 BYOK 的结果（`cursor_write_byok` 的载荷）。`backup_path` 是回滚点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorWriteByokResult {
    pub ok: bool,
    pub backup_path: Option<String>,
    pub written_base_url: Option<String>,
}

/// ItemTable 里 applicationUser 那一行（key 用真实值，不用前缀，保证 INSERT OR REPLACE 命中同一行）。
struct AppUserRow {
    key: String,
    value: String,
}

/// Cursor 主进程的 argv[0] 后缀。Electron 子进程叫 `Cursor Helper (…)`，不会命中。
const CURSOR_MAIN_PROCESS_SUFFIX: &str = "Cursor.app/Contents/MacOS/Cursor";

/// 判定一行进程命令行是否为 Cursor **主进程**。
///
/// `pgrep -fl` 输出是 `<pid> <完整命令行>`，光标在 **argv[0]**：带 `--new-window` / 工作区路径
/// 启动时整行不再以可执行文件结尾，按「整行结尾」判定会把运行中的 Cursor 误判成「已退出」。
/// 故先剥掉纯数字的 pid 前缀（既有单测会传不带 pid 的裸路径），再只对 argv[0] 做后缀匹配。
pub(crate) fn is_cursor_main_process_line(line: &str) -> bool {
    let line = line.trim();
    let rest = match line.split_once(' ') {
        Some((first, rest)) if !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()) => {
            rest
        }
        _ => line,
    };
    // argv[0]：到第一个空白为止（含空格的 helper 路径本就不会命中主进程后缀）。
    let argv0 = rest.split_whitespace().next().unwrap_or("");
    argv0.ends_with(CURSOR_MAIN_PROCESS_SUFFIX)
}

/// 进程探测的结果。`Undetectable` = 拿不到可靠的进程列表（平台不支持 / `pgrep` 不可用）。
pub(crate) enum CursorProcessProbe {
    Lines(Vec<String>),
    Undetectable,
}

/// 真实探测：macOS 用 `pgrep -fl`（`-f` 全命令行，`-l` 带上命令行便于二次精确判定）。
///
/// `pgrep` 退出码 1 = 没匹配到（正常「没在跑」）；其它失败（找不到 `pgrep`、被信号打断等）
/// 一律视为**探测不可用**，由上层保守地当成「运行中」。
fn probe_cursor_processes() -> CursorProcessProbe {
    match std::process::Command::new("pgrep")
        .arg("-fl")
        .arg(CURSOR_MAIN_PROCESS_SUFFIX)
        .output()
    {
        Ok(out) if out.status.success() => CursorProcessProbe::Lines(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::to_string)
                .collect(),
        ),
        Ok(out) if out.status.code() == Some(1) => CursorProcessProbe::Lines(Vec::new()),
        _ => CursorProcessProbe::Undetectable,
    }
}

/// 把「进程探测」注入进来，便于单测（不真的 pgrep 用户机器）。
///
/// `Undetectable` 保守地判为「运行中」——宁可不写 BYOK，也不能在 Cursor 运行时写。
pub(crate) fn cursor_running_with(probe: impl FnOnce() -> CursorProcessProbe) -> bool {
    match probe() {
        CursorProcessProbe::Undetectable => true,
        CursorProcessProbe::Lines(lines) => {
            lines.iter().any(|line| is_cursor_main_process_line(line))
        }
    }
}

/// 保留既有「注入命令行列表」的单测入口（等价于 `Lines` 探测）。
///
/// 仅测试使用（生产路径走 `cursor_running_with` + 真实探测），故标 `#[cfg(test)]`：
/// 否则它在非测试构建里是死代码，会让 CI 的 `clippy -- -D warnings` 失败。
#[cfg(test)]
pub(crate) fn is_cursor_running_with(lines: impl FnOnce() -> Vec<String>) -> bool {
    cursor_running_with(|| CursorProcessProbe::Lines(lines()))
}

/// Cursor 是否正在运行。
///
/// 运行中 Cursor 会在内存里持有 reactive storage 快照、退出时回写，因此读/写都必须在
/// 它退出之后（2026-10-13 实测）。
///
/// **非 macOS 恒为 true**：那些平台没有可靠的进程探测方式，探测不可用时保守地视为「运行中」，
/// 从而阻止自动写入（否则运行中写库会被 Cursor 退出时回写覆盖）。
pub fn is_cursor_running() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    cursor_running_with(probe_cursor_processes)
}

/// 把 applicationUser 行的原始 JSON 解析成 BYOK 现状。任何解析失败都降级为「读不到」。
pub(crate) fn parse_byok_state(raw: Option<&str>) -> CursorByokState {
    let Some(text) = raw else {
        return CursorByokState::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return CursorByokState::default();
    };
    // 缺失 / 非数组 / 含非字符串 → 能取多少取多少（全失败则空数组），不 panic。
    let model_ids: Vec<String> = value
        .get("availableAPIKeyModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    CursorByokState {
        base_url: value
            .get("openAIBaseUrl")
            .and_then(Value::as_str)
            .map(str::to_string),
        use_openai_key: value
            .get("useOpenAIKey")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        model_count: model_ids.len(),
        model_ids,
    }
}

/// 读 Cursor 本地 BYOK 现状（读 `state.vscdb` 的 applicationUser 行）。
pub fn read_byok_state() -> CursorByokState {
    read_byok_state_from_path(&get_cursor_state_db_path())
}

/// 指定路径版本（便于单测）。文件缺失 / 打不开 / 无该行 / JSON 坏 → 一律降级，不 panic。
pub(crate) fn read_byok_state_from_path(path: &Path) -> CursorByokState {
    parse_byok_state(
        read_app_user_row(path)
            .as_ref()
            .map(|row| row.value.as_str()),
    )
}

/// 读 ItemTable 里 applicationUser 那一行（任何失败都返回 None，不抛错）。
fn read_app_user_row(path: &Path) -> Option<AppUserRow> {
    if !path.exists() {
        return None;
    }
    let conn = rusqlite::Connection::open(path).ok()?;
    let mut stmt = conn
        .prepare("SELECT key, value FROM ItemTable WHERE key LIKE ?1 ORDER BY key LIMIT 1")
        .ok()?;
    let pattern = format!("{BYOK_KEY_PREFIX}%");
    let (key, raw) = stmt
        .query_row([pattern], |row| {
            let key: String = row.get(0)?;
            let value: rusqlite::types::Value = row.get(1)?;
            Ok((key, value))
        })
        .optional()
        .ok()??;
    Some(AppUserRow {
        key,
        value: sqlite_value_to_string(raw)?,
    })
}

/// 按键精确读 value。
fn read_value_by_key(path: &Path, key: &str) -> Option<String> {
    let conn = rusqlite::Connection::open(path).ok()?;
    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key = ?1")
        .ok()?;
    let raw: rusqlite::types::Value = stmt.query_row([key], |row| row.get(0)).optional().ok()??;
    sqlite_value_to_string(raw)
}

fn sqlite_value_to_string(value: rusqlite::types::Value) -> Option<String> {
    match value {
        rusqlite::types::Value::Text(text) => Some(text),
        rusqlite::types::Value::Blob(bytes) => String::from_utf8(bytes).ok(),
        _ => None,
    }
}

/// 推进到下一个非空白字节。
fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// 从 `start`（`"`）扫描一个 JSON 字符串，返回（内容区间, 结束引号后一位）。
fn scan_json_string(json: &str, start: usize) -> Option<((usize, usize), usize)> {
    let bytes = json.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let content_start = start + 1;
    let mut i = content_start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(((content_start, i), i + 1)),
            _ => i += 1,
        }
    }
    None
}

/// 从 `start`（某 JSON 值首字节）扫描到该值结束，返回其后一位。
fn scan_json_value_end(json: &str, start: usize) -> Option<usize> {
    let bytes = json.as_bytes();
    let first = *bytes.get(start)?;
    match first {
        b'"' => scan_json_string(json, start).map(|(_, after)| after),
        b'{' | b'[' => {
            let close = if first == b'{' { b'}' } else { b']' };
            let mut depth = 0i32;
            let mut i = start;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => i = scan_json_string(json, i)?.1,
                    b if b == first => {
                        depth += 1;
                        i += 1;
                    }
                    b if b == close => {
                        depth -= 1;
                        i += 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                    _ => i += 1,
                }
            }
            None
        }
        b't' => json
            .get(start..start + 4)
            .is_some_and(|s| s == "true")
            .then_some(start + 4),
        b'f' => json
            .get(start..start + 5)
            .is_some_and(|s| s == "false")
            .then_some(start + 5),
        b'n' => json
            .get(start..start + 4)
            .is_some_and(|s| s == "null")
            .then_some(start + 4),
        _ => {
            let mut i = start;
            while i < bytes.len()
                && matches!(bytes[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
            {
                i += 1;
            }
            (i > start).then_some(i)
        }
    }
}

/// 在扁平对象 JSON 里定位**顶层**键 `key` 的值区间 `[start, end)`（不含键与冒号）。
/// 只认深度 1 的键，故嵌套对象里的同名键、以及字符串值里的同形文本都不会被误命中。
fn find_top_level_value_span(json: &str, key: &str) -> Option<(usize, usize)> {
    let bytes = json.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                i += 1;
            }
            b'"' => {
                let (content, after) = scan_json_string(json, i)?;
                let colon = skip_ws(bytes, after);
                // 只有「键」才紧跟冒号；值字符串后面是 `,` 或 `}`。
                if depth != 1 || bytes.get(colon) != Some(&b':') {
                    i = after;
                    continue;
                }
                let value_start = skip_ws(bytes, colon + 1);
                let value_end = scan_json_value_end(json, value_start)?;
                if &json[content.0..content.1] == key {
                    return Some((value_start, value_end));
                }
                i = value_end;
            }
            _ => i += 1,
        }
    }
    None
}

/// 把顶层键 `key` 的值替换成 `replacement`（已序列化好的 JSON 值），其余字节原样保留。
fn replace_top_level_value(json: &str, key: &str, replacement: &str) -> Result<String, AppError> {
    let (start, end) = find_top_level_value_span(json, key).ok_or_else(|| {
        AppError::Config(format!(
            "Cursor applicationUser JSON 缺少键 {key}，拒绝写入以免破坏结构"
        ))
    })?;
    let mut out = String::with_capacity(json.len() + replacement.len());
    out.push_str(&json[..start]);
    out.push_str(replacement);
    out.push_str(&json[end..]);
    Ok(out)
}

/// **外科式**替换三个 token：`openAIBaseUrl` / `useOpenAIKey` / `availableAPIKeyModels`。
///
/// 不做「反序列化再序列化」——那会重排键序 / 改动转义，破坏其余约 131 个键的字节表现。
pub(crate) fn replace_byok_tokens(
    json: &str,
    base_url: &str,
    model_ids: &[String],
) -> Result<String, AppError> {
    let base_url_json =
        serde_json::to_string(base_url).map_err(|e| AppError::JsonSerialize { source: e })?;
    let models_json =
        serde_json::to_string(model_ids).map_err(|e| AppError::JsonSerialize { source: e })?;
    let json = replace_top_level_value(json, "openAIBaseUrl", &base_url_json)?;
    let json = replace_top_level_value(&json, "useOpenAIKey", "true")?;
    replace_top_level_value(&json, "availableAPIKeyModels", &models_json)
}

/// 写 Cursor BYOK 三字段（Base URL + `useOpenAIKey=true` + 可用模型列表）。
///
/// **必须 Cursor 完全退出后才能写**：运行中它持有内存快照，退出时回写会覆盖我们的写入。
pub fn write_byok(base_url: &str, model_ids: &[String]) -> Result<CursorWriteByokResult, AppError> {
    write_byok_at(
        &get_cursor_state_db_path(),
        base_url,
        model_ids,
        &byok_backup_dir(),
        is_cursor_running,
    )
}

/// 可注入版本（单测用）：`running` 注入「Cursor 是否在跑」，`backup_dir` 注入回滚点目录。
pub(crate) fn write_byok_at(
    db_path: &Path,
    base_url: &str,
    model_ids: &[String],
    backup_dir: &Path,
    running: impl FnOnce() -> bool,
) -> Result<CursorWriteByokResult, AppError> {
    if running() {
        return Err(AppError::Message(
            "Cursor 正在运行，请完全退出后再写入".to_string(),
        ));
    }
    let row = read_app_user_row(db_path).ok_or_else(|| {
        AppError::Config(format!(
            "找不到 Cursor 的 applicationUser 存储（{}），无法写入",
            db_path.display()
        ))
    })?;
    let updated = replace_byok_tokens(&row.value, base_url, model_ids)?;

    // 1) 备份原始行（回滚点）
    let backup_path = backup_dir.join(format!(
        "cursor-appuser-PREWRITE-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    // 备份目录可能尚不存在（系统临时目录被清理、或注入的自定义目录），
    // 先安全创建再落盘：否则首次配置会以 NotFound 失败、引导停在 writeFailed。
    fs::create_dir_all(backup_dir).map_err(|e| AppError::io(backup_dir, e))?;
    fs::write(&backup_path, &row.value).map_err(|e| AppError::io(&backup_path, e))?;

    // 2) 落库（用真实 key，保证替换原行而非新增行）
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| AppError::Config(format!("打开 Cursor 状态库失败: {e}")))?;
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)",
        rusqlite::params![row.key, updated],
    )
    .map_err(|e| AppError::Config(format!("写入 Cursor BYOK 失败: {e}")))?;
    drop(conn);

    // 3) 读回校验：不一致就报错，绝不谎报成功
    if read_value_by_key(db_path, &row.key).as_deref() != Some(updated.as_str()) {
        return Err(AppError::Config(
            "写入后读回校验不一致，已放弃（请检查 Cursor 状态库是否被占用）".to_string(),
        ));
    }

    Ok(CursorWriteByokResult {
        ok: true,
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        written_base_url: Some(base_url.to_string()),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_get_cursor_dir_default() {
        let dir = get_cursor_dir();
        assert!(dir.ends_with(".cursor"));
    }

    #[test]
    fn test_get_cursor_env_path() {
        let path = get_cursor_env_path();
        assert!(path.ends_with("env.json"));
    }

    #[test]
    fn test_read_cursor_env_empty() {
        // Without a real ~/.cursor/env.json, this should return empty object
        // Since the file doesn't exist by default
    }

    #[test]
    fn test_set_and_remove_provider() {
        // Test provider CRUD operations
        // Note: These tests require a writable filesystem
    }

    // ==================== BYOK 读写 ====================

    /// 带若干「干扰键」的 applicationUser JSON：嵌套对象里也出现 `openAIBaseUrl` 字样、
    /// 字符串值里带逗号 / 花括号 / 引号 / 反斜杠。任何「反序列化再序列化」的实现都会改动
    /// 这些键的字节表现（键序 / 转义），故此 fixture 能抓住「用会破坏其它键的正则/解析」。
    const FIXTURE: &str = r##"{"alpha":{"nested":"openAIBaseUrl"},"beta":"a,string}with\"quotes\" and \\ backslash","openAIBaseUrl":null,"useOpenAIKey":false,"availableAPIKeyModels":[],"gamma":12345,"delta":true,"epsilon":null,"zeta":[1,2,{"k":"v"}],"omega":0}"##;

    /// 造一个只有 ItemTable 的 state.vscdb，并把 applicationUser 那条塞进去。
    fn make_state_db(path: &Path, value: &str) {
        let conn = rusqlite::Connection::open(path).expect("open state db");
        conn.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .expect("create ItemTable");
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![BYOK_KEY_PREFIX, value],
        )
        .expect("insert applicationUser row");
    }

    #[test]
    fn replace_byok_tokens_is_surgical() {
        let base_url = "https://random-words.trycloudflare.com/cursor/v1";
        let models = vec!["custom-k3".to_string(), "custom-k4".to_string()];

        let out = replace_byok_tokens(FIXTURE, base_url, &models).expect("replace tokens");

        // 独立构造期望值：仅替换 fixture 里三处目标 token，其余字节原样。
        let expected = FIXTURE
            .replace(
                r#""openAIBaseUrl":null"#,
                &format!(r#""openAIBaseUrl":"{base_url}""#),
            )
            .replace(r#""useOpenAIKey":false"#, r#""useOpenAIKey":true"#)
            .replace(
                r#""availableAPIKeyModels":[]"#,
                r#""availableAPIKeyModels":["custom-k3","custom-k4"]"#,
            );
        assert_eq!(
            out, expected,
            "除三个目标键外，其余键必须逐字节不变（不得重排键序 / 重写转义）"
        );

        // 干扰键即便与目标键同形，也不受影响。
        let value: Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(value["alpha"]["nested"], json!("openAIBaseUrl"));
        assert_eq!(
            value["beta"],
            json!("a,string}with\"quotes\" and \\ backslash")
        );
        assert_eq!(value["gamma"], json!(12345));
        assert_eq!(value["zeta"][2]["k"], json!("v"));
        assert_eq!(value["openAIBaseUrl"], json!(base_url));
        assert_eq!(value["useOpenAIKey"], json!(true));
        assert_eq!(value["availableAPIKeyModels"], json!(models));
    }

    #[test]
    fn write_byok_refuses_while_cursor_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("state.vscdb");
        make_state_db(&db_path, FIXTURE);

        let err = write_byok_at(
            &db_path,
            "https://x.trycloudflare.com/cursor/v1",
            &["custom-k3".to_string()],
            dir.path(),
            || true, // 注入「Cursor 正在运行」
        )
        .expect_err("Cursor 运行中必须拒写");

        assert!(
            err.to_string().contains("Cursor 正在运行"),
            "错误文案须提示先退出 Cursor，实际：{err}"
        );
        // 拒写时库必须一字未动
        assert_eq!(
            read_app_user_row(&db_path).expect("row").value,
            FIXTURE,
            "拒写不得留下任何改动"
        );
    }

    #[test]
    fn write_byok_roundtrips_and_backs_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("state.vscdb");
        let backup_dir = dir.path().join("backups");
        std::fs::create_dir_all(&backup_dir).expect("mkdir backup");
        make_state_db(&db_path, FIXTURE);

        let base_url = "https://x.trycloudflare.com/cursor/v1";
        let models = vec!["custom-k3".to_string()];
        let result =
            write_byok_at(&db_path, base_url, &models, &backup_dir, || false).expect("write byok");

        assert!(result.ok);
        assert_eq!(result.written_base_url.as_deref(), Some(base_url));
        // 回滚点内容 = 写入前的原始行，且文件名带 PREWRITE 前缀
        let backup = result.backup_path.expect("backup path");
        assert!(
            backup.contains("cursor-appuser-PREWRITE-"),
            "实际：{backup}"
        );
        assert_eq!(
            std::fs::read_to_string(&backup).expect("read backup"),
            FIXTURE
        );

        // 落库后读回一致
        let state = read_byok_state_from_path(&db_path);
        assert_eq!(state.base_url.as_deref(), Some(base_url));
        assert!(state.use_openai_key);
        assert_eq!(state.model_count, 1);
    }

    /// 回归（PR #8270 P2）：回滚点目录尚不存在时也必须能写入。
    ///
    /// 备份 `fs::write` 发生在写库之前且原先不建父目录，普通 Windows 没有 `/tmp`，
    /// 首次配置会直接以 `NotFound` 失败，引导停在 writeFailed。
    #[test]
    fn write_byok_creates_missing_backup_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("state.vscdb");
        make_state_db(&db_path, FIXTURE);

        // 回滚点目录还不存在（故意留两层，验证递归创建）。
        let backup_dir = dir.path().join("nested").join("backups");
        assert!(!backup_dir.exists(), "前置：备份目录必须尚不存在");

        let base_url = "https://x.trycloudflare.com/cursor/v1";
        let models = vec!["custom-k3".to_string()];
        let result = write_byok_at(&db_path, base_url, &models, &backup_dir, || false)
            .expect("备份父目录不存在时也必须成功写入");

        assert!(result.ok);
        let backup = result.backup_path.expect("backup path");
        assert!(
            Path::new(&backup).exists(),
            "回滚点必须真的落盘，实际：{backup}"
        );
        assert_eq!(
            std::fs::read_to_string(&backup).expect("read backup"),
            FIXTURE
        );
    }

    /// 回归（PR #8270 P2）：回滚点目录必须取系统临时目录，不能硬编码 `/tmp`
    /// （Windows 无 `/tmp`）。
    #[test]
    fn byok_backup_dir_uses_system_temp_dir() {
        assert_eq!(
            byok_backup_dir(),
            std::env::temp_dir(),
            "回滚点目录必须是系统临时目录"
        );
    }

    #[test]
    fn read_byok_state_degrades_when_unavailable() {
        let dir = tempfile::tempdir().expect("tempdir");

        // 1) 文件不存在
        assert_eq!(
            read_byok_state_from_path(&dir.path().join("missing.vscdb")),
            CursorByokState::default(),
            "文件缺失须降级为「读不到」，不得 panic"
        );

        // 2) 库存在但没有该行
        let empty = dir.path().join("empty.vscdb");
        let conn = rusqlite::Connection::open(&empty).expect("open");
        conn.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .expect("create");
        drop(conn);
        assert_eq!(
            read_byok_state_from_path(&empty),
            CursorByokState::default()
        );

        // 3) JSON 坏了
        let broken = dir.path().join("broken.vscdb");
        make_state_db(&broken, "{ not json");
        assert_eq!(
            read_byok_state_from_path(&broken),
            CursorByokState::default()
        );

        // 4) 键缺失（合法 JSON 但无 BYOK 字段）
        let partial = dir.path().join("partial.vscdb");
        make_state_db(&partial, r#"{"gamma":1}"#);
        assert_eq!(
            read_byok_state_from_path(&partial),
            CursorByokState::default()
        );
    }

    /// 回归（P2）：`cursor_byok_status` 必须暴露**具体** model id 列表。
    ///
    /// 同一公网隧道下换供应商时 `baseUrl` 不变，前端只比对 baseUrl + 数量会把「B 家的模型列表
    /// 还没写」误判成「已写好」。这里断言序列化载荷（前端契约）里带 `modelIds`。
    #[test]
    fn parse_byok_state_exposes_model_ids() {
        let raw = r#"{"openAIBaseUrl":"https://x/cursor/v1","useOpenAIKey":true,"availableAPIKeyModels":["a","b"]}"#;
        let view = serde_json::to_value(parse_byok_state(Some(raw))).expect("serialize");
        assert_eq!(view["modelIds"], json!(["a", "b"]));
        assert_eq!(view["modelCount"], json!(2), "model_count 由列表派生");

        // 非数组 / 含非字符串 / 缺失 → 能取多少取多少，全失败则空数组，不 panic
        let mixed = r#"{"availableAPIKeyModels":["ok",1,null,"ok2"]}"#;
        assert_eq!(
            serde_json::to_value(parse_byok_state(Some(mixed))).expect("serialize")["modelIds"],
            json!(["ok", "ok2"])
        );
        let not_array = r#"{"availableAPIKeyModels":"nope"}"#;
        assert_eq!(
            serde_json::to_value(parse_byok_state(Some(not_array))).expect("serialize")["modelIds"],
            json!([])
        );
        assert_eq!(
            serde_json::to_value(parse_byok_state(Some(r#"{"gamma":1}"#))).expect("serialize")
                ["modelIds"],
            json!([])
        );
    }

    #[test]
    fn is_cursor_running_matches_only_main_process() {
        // 注入「取候选命令行」：pgrep 已按子串预过滤，这里再按 argv[0] 结尾精确判定。
        assert!(is_cursor_running_with(|| vec![
            "4242 /Applications/Cursor.app/Contents/MacOS/Cursor".to_string()
        ]));
        assert!(is_cursor_running_with(|| vec![
            "1 /usr/bin/something".to_string(),
            "/Applications/Cursor.app/Contents/MacOS/Cursor".to_string(),
        ]));
        // 空列表 / 只有 Electron 子进程 helper → 不算「Cursor 在跑」
        assert!(!is_cursor_running_with(Vec::<String>::new));
        // 这条 fixture 必须是 helper 的**真实** argv[0] 形态：Electron 的 helper 可执行文件在
        // `Contents/Frameworks/<Helper>.app/Contents/MacOS/…` 下，本来就命不中主进程后缀。
        // 旧写法 `…/MacOS/Cursor Helper (Renderer)`（同一目录下）的 argv[0] 恰好就是主进程
        // 可执行文件、`Helper (Renderer)` 落在 argv[1]，与主进程不可区分——那不是 pgrep -fl 会
        // 产出的真实形态，只会诱导「遇到 Helper 就特判」的错修。
        let real_helper = "7 /Applications/Cursor.app/Contents/Frameworks/Cursor Helper (Renderer).app/Contents/MacOS/Cursor Helper (Renderer)".to_string();
        assert!(!is_cursor_running_with(|| vec![real_helper]));
    }

    /// 回归（P2）：Cursor 带参数（`--new-window` / 工作区路径）启动时仍必须判为「运行中」。
    ///
    /// `pgrep -fl` 输出是 `<pid> <完整命令行>`，带参数时整行不再以可执行文件结尾；旧实现按
    /// 「整行结尾」判定会误判成「已退出」，于是在 Cursor 运行中仍然写状态库 → 被它退出时回写覆盖。
    #[test]
    fn is_cursor_running_matches_main_process_with_args() {
        assert!(is_cursor_running_with(|| vec![
            "4242 /Applications/Cursor.app/Contents/MacOS/Cursor --new-window".to_string()
        ]));
        assert!(is_cursor_running_with(|| vec![
            "4242 /Applications/Cursor.app/Contents/MacOS/Cursor /Users/x/proj".to_string()
        ]));
    }

    /// 回归（P2）：探测不可用时必须**保守**判为「运行中」，阻止自动写入。
    ///
    /// 写 BYOK 必须等 Cursor 完全退出（运行中它持有内存快照、退出时回写会覆盖）。Windows/Linux
    /// 没有可靠的进程探测方式，macOS 上 `pgrep` 也可能被 PATH/权限挡住——此时宁可不写。
    #[test]
    fn cursor_running_with_undetectable_probe_is_conservative() {
        assert!(
            cursor_running_with(|| CursorProcessProbe::Undetectable),
            "探测不可用必须视为「运行中」（保守，阻止自动写入）"
        );
        assert!(!cursor_running_with(|| {
            CursorProcessProbe::Lines(vec!["1 /usr/bin/something".to_string()])
        }));
    }
}
