//! API Key 变量刷新脚本的执行层（契约 v2）
//!
//! 在脚本目录中按 `index.mjs` > `index.js` 查找入口，用 node 执行（cwd = 脚本目录），
//! 解析 stdout 中的 JSON `{ apiKey, expiresAt?, refreshAfter?, message? }`。
//!
//! 退出码：0 成功；2 永久失败（停止自动重试，需要用户处理）；其它为临时失败（指数退避）。
//! stderr 仅作日志，写入前对新旧值脱敏。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::error::AppError;

/// 变量的刷新配置（脚本目录与可选参数）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRefreshConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 脚本目录（绝对路径）；其中按 index.mjs > index.js 查找入口
    #[serde(default)]
    pub script_dir: String,
    /// 超时秒数（默认 30，范围 5..=300）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// 在过期前多少秒刷新（默认 300）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_time_secs: Option<u64>,
}

/// 变量的刷新运行状态（时间均为 Unix 毫秒）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRefreshState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_after: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_script: Option<String>,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub needs_attention: bool,
}

pub const CONTRACT_VERSION: &str = "2";
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const MIN_TIMEOUT_SECS: u64 = 5;
pub const MAX_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_LEAD_TIME_SECS: u64 = 300;
/// 脚本未给出过期信息时的固定刷新间隔
pub const FALLBACK_INTERVAL_MS: i64 = 60 * 60 * 1000;
pub const MAX_STDOUT_BYTES: usize = 64 * 1024;
pub const MAX_STDERR_KEEP_BYTES: usize = 4 * 1024;
const MAX_MESSAGE_CHARS: usize = 500;
const MAX_ERROR_SUMMARY_CHARS: usize = 200;
const SCRIPT_CANDIDATES: [&str; 2] = ["index.mjs", "index.js"];

// ---------------------------------------------------------------------------
// 基础类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshReason {
    Startup,
    Scheduled,
    Switch,
    Manual,
    Test,
    Retry,
}

impl RefreshReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Scheduled => "scheduled",
            Self::Switch => "switch",
            Self::Manual => "manual",
            Self::Test => "test",
            Self::Retry => "retry",
        }
    }
}

/// 解析后的脚本入口
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedScript {
    pub dir: PathBuf,
    pub path: PathBuf,
    /// "index.mjs" 或 "index.js"
    pub file: String,
    /// 同时存在 index.mjs 与 index.js 时为 true（index.js 被忽略）
    pub ignored_index_js: bool,
}

/// 脚本 stdout 解析结果（时间为 Unix 毫秒）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshOutput {
    pub api_key: String,
    pub expires_at: Option<i64>,
    pub refresh_after: Option<i64>,
    pub message: Option<String>,
}

/// 退出码分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    Success,
    Retryable,
    Permanent,
}

/// 单次脚本运行的失败
#[derive(Debug, Clone)]
pub struct RunFailure {
    pub permanent: bool,
    pub error: String,
}

/// 单次脚本运行成功
#[derive(Debug, Clone)]
pub struct ScriptRun {
    pub output: RefreshOutput,
    pub resolved_file: String,
}

/// 传给脚本的上下文（会变成 CC_SWITCH_* 环境变量）
#[derive(Debug, Clone, Default)]
pub struct ScriptContext {
    pub var_name: String,
    pub reason: &'static str,
    pub current_expires_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// 配置默认值与调度纯函数
// ---------------------------------------------------------------------------

pub fn effective_timeout_secs(cfg: &ApiKeyRefreshConfig) -> u64 {
    cfg.timeout_secs
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

pub fn effective_lead_time_ms(cfg: &ApiKeyRefreshConfig) -> i64 {
    let secs = cfg.lead_time_secs.unwrap_or(DEFAULT_LEAD_TIME_SECS);
    (secs.min(7 * 24 * 3600) as i64) * 1000
}

/// 指数退避：30s → 1m → 2m → 5m → 10m → 15m（封顶）
pub fn backoff(consecutive_failures: u32) -> Duration {
    const STEPS: [u64; 6] = [30, 60, 120, 300, 600, 900];
    let idx = consecutive_failures.saturating_sub(1) as usize;
    Duration::from_secs(STEPS[idx.min(STEPS.len() - 1)])
}

/// 下一次应当刷新的时间（Unix 毫秒）；`i64::MAX` 表示不自动刷新
pub fn next_due_ms(cfg: &ApiKeyRefreshConfig, state: &ApiKeyRefreshState) -> i64 {
    if !cfg.enabled || state.needs_attention {
        return i64::MAX;
    }
    if state.consecutive_failures > 0 {
        let base = state.last_attempt_at.unwrap_or(0);
        return base.saturating_add(backoff(state.consecutive_failures).as_millis() as i64);
    }
    if let Some(refresh_after) = state.refresh_after {
        return refresh_after;
    }
    if let Some(expires_at) = state.expires_at {
        return expires_at.saturating_sub(effective_lead_time_ms(cfg));
    }
    match state.last_success_at {
        Some(last) => last.saturating_add(FALLBACK_INTERVAL_MS),
        None => 0,
    }
}

pub fn needs_refresh(cfg: &ApiKeyRefreshConfig, state: &ApiKeyRefreshState, now_ms: i64) -> bool {
    now_ms >= next_due_ms(cfg, state)
}

/// 当前 Key 是否已过期或即将过期（含从未成功获取）
pub fn key_expiring(cfg: &ApiKeyRefreshConfig, state: &ApiKeyRefreshState, now_ms: i64) -> bool {
    if state.last_success_at.is_none() {
        return true;
    }
    match state.expires_at {
        Some(expires_at) => now_ms >= expires_at.saturating_sub(effective_lead_time_ms(cfg)),
        None => false,
    }
}

/// 切换前是否需要先刷新：已启用、未处于永久失败、且 Key 已过期/即将过期
pub fn should_refresh_on_switch(
    cfg: &ApiKeyRefreshConfig,
    state: &ApiKeyRefreshState,
    now_ms: i64,
) -> bool {
    cfg.enabled && !state.needs_attention && key_expiring(cfg, state, now_ms)
}

pub fn key_expired_now(state: &ApiKeyRefreshState, now_ms: i64) -> bool {
    state.last_success_at.is_none() || state.expires_at.is_some_and(|e| now_ms >= e)
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------------
// 脚本定位
// ---------------------------------------------------------------------------

/// 在脚本目录中按 index.mjs > index.js 查找入口
pub fn resolve_script(dir: &str) -> Result<ResolvedScript, AppError> {
    let trimmed = dir.trim();
    let dir_path = PathBuf::from(trimmed);
    if trimmed.is_empty() || !dir_path.is_absolute() || !dir_path.is_dir() {
        return Err(AppError::localized(
            "api_key_vars.script_dir_invalid",
            format!("脚本目录无效（需为已存在目录的绝对路径）: {trimmed}"),
            format!("Invalid script directory (must be an absolute path to an existing directory): {trimmed}"),
        ));
    }
    let found: Vec<&str> = SCRIPT_CANDIDATES
        .iter()
        .copied()
        .filter(|name| dir_path.join(name).is_file())
        .collect();
    let Some(first) = found.first() else {
        return Err(AppError::localized(
            "api_key_vars.script_not_found",
            format!("在 {trimmed} 中未找到 index.mjs 或 index.js"),
            format!("Neither index.mjs nor index.js was found in {trimmed}"),
        ));
    };
    Ok(ResolvedScript {
        path: dir_path.join(first),
        dir: dir_path,
        file: (*first).to_string(),
        ignored_index_js: found.len() > 1,
    })
}

// ---------------------------------------------------------------------------
// node 定位
// ---------------------------------------------------------------------------

#[cfg(windows)]
const NODE_BIN: &str = "node.exe";
#[cfg(not(windows))]
const NODE_BIN: &str = "node";

fn node_not_found() -> AppError {
    AppError::localized(
        "api_key_vars.node_not_found",
        "未找到 node，请安装 Node.js 或在设置中指定 node 路径",
        "node was not found; install Node.js or set the node path in settings",
    )
}

fn search_path_for_node() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(NODE_BIN))
        .find(|candidate| candidate.is_file())
}

fn common_node_locations() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(not(windows))]
    {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
            out.push(PathBuf::from(dir).join(NODE_BIN));
        }
        if let Some(home) = dirs::home_dir() {
            out.push(home.join(".volta/bin").join(NODE_BIN));
            let nvm = home.join(".nvm/versions/node");
            if let Ok(entries) = std::fs::read_dir(&nvm) {
                let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
                versions.sort();
                for v in versions.into_iter().rev() {
                    out.push(v.join("bin").join(NODE_BIN));
                }
            }
        }
    }
    #[cfg(windows)]
    {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base).join("nodejs").join(NODE_BIN));
            }
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            out.push(PathBuf::from(appdata).join("nvm").join(NODE_BIN));
        }
    }
    out
}

#[cfg(not(windows))]
async fn login_shell_node() -> Option<PathBuf> {
    static CACHE: OnceLock<Mutex<Option<Option<PathBuf>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache.lock().ok().and_then(|c| c.clone()) {
        return cached;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut cmd = tokio::process::Command::new(shell);
    cmd.args(["-lc", "command -v node"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let result = match tokio::time::timeout(Duration::from_secs(5), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.is_absolute() && p.is_file())
        }
        _ => None,
    };
    if let Ok(mut c) = cache.lock() {
        *c = Some(result.clone());
    }
    result
}

/// 查找 node：设置项 → PATH → 登录 shell（非 Windows）→ 常见目录
pub async fn resolve_node(configured: Option<&str>) -> Result<PathBuf, AppError> {
    if let Some(configured) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Ok(path);
        }
        return Err(AppError::localized(
            "api_key_vars.node_not_found",
            format!("设置中指定的 node 不存在: {configured}"),
            format!("The configured node path does not exist: {configured}"),
        ));
    }
    if let Some(path) = search_path_for_node() {
        return Ok(path);
    }
    #[cfg(not(windows))]
    if let Some(path) = login_shell_node().await {
        return Ok(path);
    }
    common_node_locations()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(node_not_found)
}

// ---------------------------------------------------------------------------
// 契约解析
// ---------------------------------------------------------------------------

fn invalid_expires(field: &str, detail: impl std::fmt::Display) -> AppError {
    AppError::localized(
        "api_key_vars.invalid_expires_at",
        format!("脚本输出的 {field} 无效: {detail}"),
        format!("Invalid {field} in script output: {detail}"),
    )
}

/// 解析时间字段：带时区的 ISO-8601 或 Unix 秒；返回 Unix 毫秒
fn parse_time(field: &str, value: &Value) -> Result<Option<i64>, AppError> {
    match value {
        Value::Null => Ok(None),
        Value::Number(n) => {
            let secs = n
                .as_f64()
                .ok_or_else(|| invalid_expires(field, "not a number"))?;
            if !secs.is_finite() || secs < 0.0 {
                return Err(invalid_expires(field, "must be a non-negative number"));
            }
            if secs > 1e12 {
                return Err(invalid_expires(
                    field,
                    "looks like milliseconds; use Unix seconds or ISO-8601",
                ));
            }
            Ok(Some((secs * 1000.0).round() as i64))
        }
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s.trim())
            .map(|dt| Some(dt.timestamp_millis()))
            .map_err(|e| invalid_expires(field, format!("{e} (ISO-8601 with timezone required)"))),
        _ => Err(invalid_expires(field, "must be a string or number")),
    }
}

/// 严格按契约 v1 解析 stdout
pub fn parse_output(stdout: &[u8], now_ms: i64) -> Result<RefreshOutput, AppError> {
    if stdout.len() > MAX_STDOUT_BYTES {
        return Err(AppError::localized(
            "api_key_vars.stdout_too_large",
            "脚本 stdout 超过 64 KiB",
            "Script stdout exceeds 64 KiB",
        ));
    }
    let text = std::str::from_utf8(stdout).map_err(|_| stdout_not_json("not UTF-8"))?;
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|e| stdout_not_json(e.to_string()))?;
    let Value::Object(obj) = value else {
        return Err(stdout_not_json("top-level value must be an object"));
    };
    let api_key = obj
        .get("apiKey")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty() && !k.chars().any(char::is_whitespace))
        .ok_or_else(|| {
            AppError::localized(
                "api_key_vars.invalid_api_key",
                "脚本输出缺少有效的 apiKey（需为非空且不含空白的字符串）",
                "Script output lacks a valid apiKey (non-empty string without whitespace)",
            )
        })?
        .to_string();
    let expires_at = parse_time("expiresAt", obj.get("expiresAt").unwrap_or(&Value::Null))?;
    let refresh_after = parse_time(
        "refreshAfter",
        obj.get("refreshAfter").unwrap_or(&Value::Null),
    )?;
    if let Some(e) = expires_at {
        if e <= now_ms {
            return Err(AppError::localized(
                "api_key_vars.expired_key_returned",
                "脚本返回的 Key 已过期",
                "The script returned an already-expired key",
            ));
        }
    }
    let message = obj
        .get("message")
        .and_then(Value::as_str)
        .map(|m| m.chars().take(MAX_MESSAGE_CHARS).collect::<String>());
    Ok(RefreshOutput {
        api_key,
        expires_at,
        refresh_after,
        message,
    })
}

fn stdout_not_json(detail: impl std::fmt::Display) -> AppError {
    AppError::localized(
        "api_key_vars.stdout_not_json",
        format!("脚本 stdout 不是单个 JSON 对象: {detail}"),
        format!("Script stdout is not a single JSON object: {detail}"),
    )
}

pub fn classify_exit(code: Option<i32>) -> ExitClass {
    match code {
        Some(0) => ExitClass::Success,
        Some(2) => ExitClass::Permanent,
        _ => ExitClass::Retryable,
    }
}

// ---------------------------------------------------------------------------
// 脱敏
// ---------------------------------------------------------------------------

pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets.iter().filter(|s| s.len() >= 4) {
        out = out.replace(secret, "[REDACTED]");
    }
    out
}

fn tail_bytes(bytes: &[u8], max: usize) -> String {
    let start = bytes.len().saturating_sub(max);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

fn last_line_summary(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(|l| l.chars().take(MAX_ERROR_SUMMARY_CHARS).collect())
}

// ---------------------------------------------------------------------------
// 执行
// ---------------------------------------------------------------------------

pub fn build_env(ctx: &ScriptContext) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "CC_SWITCH_CONTRACT_VERSION".to_string(),
            CONTRACT_VERSION.to_string(),
        ),
        ("CC_SWITCH_VAR_NAME".to_string(), ctx.var_name.clone()),
        ("CC_SWITCH_REASON".to_string(), ctx.reason.to_string()),
    ];
    if let Some(expires) = ctx
        .current_expires_at
        .and_then(chrono::DateTime::from_timestamp_millis)
    {
        env.push((
            "CC_SWITCH_CURRENT_EXPIRES_AT".to_string(),
            expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ));
    }
    env
}

pub fn build_command(
    node: &Path,
    script: &ResolvedScript,
    ctx: &ScriptContext,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(node);
    cmd.arg(&script.path)
        .current_dir(&script.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // 继承用户环境后覆盖 CC_SWITCH_*，并清除可能残留的同名变量
    for (key, _) in std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v))) {
        if key.starts_with("CC_SWITCH_") && key != "CC_SWITCH_TEST_HOME" {
            cmd.env_remove(key);
        }
    }
    cmd.envs(build_env(ctx));
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn retryable(error: AppError) -> RunFailure {
    RunFailure {
        permanent: false,
        error: error.to_string(),
    }
}

/// 执行脚本并解析输出
pub async fn run_script(
    cfg: &ApiKeyRefreshConfig,
    ctx: &ScriptContext,
    node_setting: Option<&str>,
    known_secrets: &[&str],
) -> Result<ScriptRun, RunFailure> {
    let script = resolve_script(&cfg.script_dir).map_err(retryable)?;
    let node = resolve_node(node_setting).await.map_err(retryable)?;
    let timeout = Duration::from_secs(effective_timeout_secs(cfg));

    let mut child = build_command(&node, &script, ctx).spawn().map_err(|e| {
        retryable(AppError::localized(
            "api_key_vars.spawn_failed",
            format!("启动脚本失败: {e}"),
            format!("Failed to start script: {e}"),
        ))
    })?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let collect = async {
        let read_stdout = async {
            let mut buf = Vec::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                // 多读 1 字节用于判断超限
                let _ = pipe
                    .take((MAX_STDOUT_BYTES + 1) as u64)
                    .read_to_end(&mut buf)
                    .await;
            }
            buf
        };
        let read_stderr = async {
            let mut buf = Vec::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let _ = pipe.take(1024 * 1024).read_to_end(&mut buf).await;
            }
            buf
        };
        let (out, err) = tokio::join!(read_stdout, read_stderr);
        let status = child.wait().await;
        (out, err, status)
    };

    let (stdout, stderr, status) = match tokio::time::timeout(timeout, collect).await {
        Ok(result) => result,
        Err(_) => {
            // collect 被丢弃时 child 一并丢弃，kill_on_drop 会结束进程
            return Err(retryable(AppError::localized(
                "api_key_vars.timeout",
                format!("脚本执行超时（{} 秒）", timeout.as_secs()),
                format!("Script timed out after {} seconds", timeout.as_secs()),
            )));
        }
    };

    let parsed_key = parse_output(&stdout, now_ms()).ok().map(|o| o.api_key);
    let mut secrets: Vec<&str> = known_secrets.to_vec();
    if let Some(k) = parsed_key.as_deref() {
        secrets.push(k);
    }
    let stderr_text = redact(&tail_bytes(&stderr, MAX_STDERR_KEEP_BYTES), &secrets);
    if !stderr_text.trim().is_empty() {
        log::info!("[ApiKeyVars] {} stderr:\n{}", ctx.var_name, stderr_text);
    }

    let status = status.map_err(|e| {
        retryable(AppError::localized(
            "api_key_vars.spawn_failed",
            format!("等待脚本结束失败: {e}"),
            format!("Failed to wait for script: {e}"),
        ))
    })?;
    match classify_exit(status.code()) {
        ExitClass::Success => {
            let output = parse_output(&stdout, now_ms()).map_err(retryable)?;
            Ok(ScriptRun {
                output,
                resolved_file: script.file,
            })
        }
        class => {
            let summary =
                last_line_summary(&stderr_text).unwrap_or_else(|| format!("exit status: {status}"));
            let permanent = class == ExitClass::Permanent;
            let err = if permanent {
                AppError::localized(
                    "api_key_vars.exit_permanent",
                    format!("脚本报告永久失败（退出码 2）: {summary}"),
                    format!("Script reported a permanent failure (exit 2): {summary}"),
                )
            } else {
                AppError::localized(
                    "api_key_vars.exit_retryable",
                    format!("脚本执行失败（{status}）: {summary}"),
                    format!("Script failed ({status}): {summary}"),
                )
            };
            Err(RunFailure {
                permanent,
                error: err.to_string(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg() -> ApiKeyRefreshConfig {
        ApiKeyRefreshConfig {
            enabled: true,
            script_dir: String::new(),
            timeout_secs: None,
            lead_time_secs: None,
        }
    }

    const NOW: i64 = 1_760_000_000_000;

    #[test]
    fn resolve_script_prefers_mjs_then_js() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap().to_string();
        let err = resolve_script(&d).unwrap_err().to_string();
        assert!(err.contains("index.mjs"), "{err}");

        std::fs::write(dir.path().join("index.js"), "").unwrap();
        let r = resolve_script(&d).unwrap();
        assert_eq!(r.file, "index.js");
        assert!(!r.ignored_index_js);

        std::fs::write(dir.path().join("index.mjs"), "").unwrap();
        let r = resolve_script(&d).unwrap();
        assert_eq!(r.file, "index.mjs");
        assert!(r.ignored_index_js);
        assert_eq!(r.dir, dir.path());
    }

    #[test]
    fn resolve_script_skips_directories_named_index() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("index.mjs")).unwrap();
        std::fs::write(dir.path().join("index.js"), "").unwrap();
        let r = resolve_script(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(r.file, "index.js");
    }

    #[test]
    fn resolve_script_rejects_invalid_dirs() {
        assert!(resolve_script("").is_err());
        assert!(resolve_script("relative/dir").is_err());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, "").unwrap();
        let err = resolve_script(file.to_str().unwrap()).unwrap_err();
        assert!(matches!(
            err,
            AppError::Localized {
                key: "api_key_vars.script_dir_invalid",
                ..
            }
        ));
        let missing = dir.path().join("missing");
        assert!(resolve_script(missing.to_str().unwrap()).is_err());
    }

    #[test]
    fn parse_minimal_and_full() {
        let o = parse_output(br#" {"apiKey":"sk-abc"} "#, NOW).unwrap();
        assert_eq!(o.api_key, "sk-abc");
        assert_eq!(o.expires_at, None);

        let o = parse_output(
            br#"{"apiKey":"k1","expiresAt":"2025-10-10T14:00:00+08:00","refreshAfter":1760001000,"message":"ok","extra":1}"#,
            NOW,
        )
        .unwrap();
        assert_eq!(o.expires_at, Some(1_760_076_000_000));
        assert_eq!(o.refresh_after, Some(1_760_001_000_000));
        assert_eq!(o.message.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_unix_seconds_and_rejects_millis() {
        let o = parse_output(br#"{"apiKey":"k","expiresAt":1760003600}"#, NOW).unwrap();
        assert_eq!(o.expires_at, Some(1_760_003_600_000));
        let e = parse_output(br#"{"apiKey":"k","expiresAt":1760003600000}"#, NOW).unwrap_err();
        assert!(e.to_string().contains("milliseconds"));
    }

    #[test]
    fn parse_rejects_bad_inputs() {
        // 无时区的 ISO
        assert!(parse_output(br#"{"apiKey":"k","expiresAt":"2026-10-10T14:00:00"}"#, NOW).is_err());
        // 过期
        let e = parse_output(br#"{"apiKey":"k","expiresAt":1}"#, NOW).unwrap_err();
        assert!(matches!(
            e,
            AppError::Localized {
                key: "api_key_vars.expired_key_returned",
                ..
            }
        ));
        // 空 / 含空白 key
        assert!(parse_output(br#"{"apiKey":""}"#, NOW).is_err());
        assert!(parse_output(br#"{"apiKey":"a b"}"#, NOW).is_err());
        assert!(parse_output(br#"{"apiKey":123}"#, NOW).is_err());
        // 非 JSON / 多个 JSON / 混入日志 / 数组
        assert!(parse_output(b"sk-abc", NOW).is_err());
        assert!(parse_output(br#"{"apiKey":"a"}{"apiKey":"b"}"#, NOW).is_err());
        assert!(parse_output(b"log line\n{\"apiKey\":\"a\"}", NOW).is_err());
        assert!(parse_output(br#"["a"]"#, NOW).is_err());
        // 过大
        let big = format!(
            r#"{{"apiKey":"k","pad":"{}"}}"#,
            "x".repeat(MAX_STDOUT_BYTES)
        );
        let e = parse_output(big.as_bytes(), NOW).unwrap_err();
        assert!(matches!(
            e,
            AppError::Localized {
                key: "api_key_vars.stdout_too_large",
                ..
            }
        ));
    }

    #[test]
    fn exit_code_mapping() {
        assert_eq!(classify_exit(Some(0)), ExitClass::Success);
        assert_eq!(classify_exit(Some(1)), ExitClass::Retryable);
        assert_eq!(classify_exit(Some(2)), ExitClass::Permanent);
        assert_eq!(classify_exit(Some(75)), ExitClass::Retryable);
        assert_eq!(classify_exit(None), ExitClass::Retryable);
    }

    #[test]
    fn backoff_steps() {
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(2), Duration::from_secs(60));
        assert_eq!(backoff(4), Duration::from_secs(300));
        assert_eq!(backoff(100), Duration::from_secs(900));
    }

    #[test]
    fn scheduling_rules() {
        let c = cfg();
        let mut st = ApiKeyRefreshState::default();
        // 从未成功：立即
        assert_eq!(next_due_ms(&c, &st), 0);
        st.last_success_at = Some(NOW);
        // 无过期信息：60 分钟后
        assert_eq!(next_due_ms(&c, &st), NOW + FALLBACK_INTERVAL_MS);
        st.expires_at = Some(NOW + 3_600_000);
        assert_eq!(next_due_ms(&c, &st), NOW + 3_600_000 - 300_000);
        st.refresh_after = Some(NOW + 1000);
        assert_eq!(next_due_ms(&c, &st), NOW + 1000);
        st.consecutive_failures = 2;
        st.last_attempt_at = Some(NOW);
        assert_eq!(next_due_ms(&c, &st), NOW + 60_000);
        assert!(!needs_refresh(&c, &st, NOW + 59_999));
        assert!(needs_refresh(&c, &st, NOW + 60_000));
        st.needs_attention = true;
        assert_eq!(next_due_ms(&c, &st), i64::MAX);
        let disabled = ApiKeyRefreshConfig {
            enabled: false,
            ..cfg()
        };
        assert_eq!(
            next_due_ms(&disabled, &ApiKeyRefreshState::default()),
            i64::MAX
        );
    }

    #[test]
    fn switch_refresh_rules() {
        let c = ApiKeyRefreshConfig {
            lead_time_secs: Some(60),
            ..cfg()
        };
        let mut st = ApiKeyRefreshState::default();
        assert!(should_refresh_on_switch(&c, &st, NOW));
        st.last_success_at = Some(NOW - 1000);
        st.expires_at = Some(NOW + 3_600_000);
        assert!(!should_refresh_on_switch(&c, &st, NOW));
        st.expires_at = Some(NOW + 30_000);
        assert!(should_refresh_on_switch(&c, &st, NOW));
        st.needs_attention = true;
        assert!(!should_refresh_on_switch(&c, &st, NOW));
    }

    #[test]
    fn timeout_clamped() {
        assert_eq!(effective_timeout_secs(&cfg()), 30);
        assert_eq!(
            effective_timeout_secs(&ApiKeyRefreshConfig {
                timeout_secs: Some(1),
                ..cfg()
            }),
            5
        );
        assert_eq!(
            effective_timeout_secs(&ApiKeyRefreshConfig {
                timeout_secs: Some(9999),
                ..cfg()
            }),
            300
        );
    }

    #[test]
    fn env_contains_contract_vars() {
        let ctx = ScriptContext {
            var_name: "MY_KEY".into(),
            reason: "test",
            current_expires_at: Some(1_760_076_000_000),
        };
        let env: HashMap<_, _> = build_env(&ctx).into_iter().collect();
        assert_eq!(env["CC_SWITCH_CONTRACT_VERSION"], "2");
        assert_eq!(env["CC_SWITCH_VAR_NAME"], "MY_KEY");
        assert_eq!(env["CC_SWITCH_REASON"], "test");
        assert!(!env.contains_key("CC_SWITCH_PROVIDER_ID"));
        assert!(!env.contains_key("CC_SWITCH_APP_TYPE"));
        assert_eq!(env["CC_SWITCH_CURRENT_EXPIRES_AT"], "2025-10-10T06:00:00Z");
    }

    #[test]
    fn masking_and_redaction() {
        assert_eq!(mask_key("short"), "****");
        assert_eq!(mask_key("sk-1234567890abcd"), "sk-1…abcd");
        assert_eq!(
            redact("got sk-secret ok", &["sk-secret"]),
            "got [REDACTED] ok"
        );
        assert_eq!(last_line_summary("a\nlast \n\n").as_deref(), Some("last"));
    }

    fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn script_cfg(dir: &Path) -> ApiKeyRefreshConfig {
        ApiKeyRefreshConfig {
            enabled: true,
            script_dir: dir.to_str().unwrap().to_string(),
            timeout_secs: Some(5),
            lead_time_secs: None,
        }
    }

    fn test_ctx() -> ScriptContext {
        ScriptContext {
            var_name: "MY_KEY".into(),
            reason: "test",
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn run_script_end_to_end_with_node() {
        if !node_available() {
            eprintln!("node not available; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // index.mjs 优先，并检查 cwd 与环境变量
        std::fs::write(
            dir.path().join("index.mjs"),
            r#"
import path from "node:path";
const ok = process.env.CC_SWITCH_VAR_NAME === "MY_KEY"
  && process.env.CC_SWITCH_REASON === "test"
  && path.resolve(process.cwd()) === path.resolve(path.dirname(new URL(import.meta.url).pathname));
console.error("log line with secret sk-from-mjs-123");
process.stdout.write(JSON.stringify({ apiKey: ok ? "sk-from-mjs-123" : "bad-env", expiresAt: Math.floor(Date.now()/1000) + 3600 }));
"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("index.js"),
            r#"console.log(JSON.stringify({apiKey:"sk-from-js"}));"#,
        )
        .unwrap();
        let run = run_script(&script_cfg(dir.path()), &test_ctx(), None, &[])
            .await
            .unwrap();
        assert_eq!(run.output.api_key, "sk-from-mjs-123");
        assert_eq!(run.resolved_file, "index.mjs");
        assert!(run.output.expires_at.is_some());

        // 删除 index.mjs 回退到 index.js（CJS）
        std::fs::remove_file(dir.path().join("index.mjs")).unwrap();
        let run = run_script(&script_cfg(dir.path()), &test_ctx(), None, &[])
            .await
            .unwrap();
        assert_eq!(run.output.api_key, "sk-from-js");
        assert_eq!(run.resolved_file, "index.js");
    }

    #[tokio::test]
    async fn run_script_exit_codes_and_timeout() {
        if !node_available() {
            eprintln!("node not available; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("index.mjs");

        std::fs::write(
            &script,
            r#"console.error("auth rejected"); process.exit(2);"#,
        )
        .unwrap();
        let f = run_script(&script_cfg(dir.path()), &test_ctx(), None, &[])
            .await
            .unwrap_err();
        assert!(f.permanent);
        assert!(f.error.contains("auth rejected"), "{}", f.error);

        std::fs::write(
            &script,
            r#"console.error("network down"); process.exit(1);"#,
        )
        .unwrap();
        let f = run_script(&script_cfg(dir.path()), &test_ctx(), None, &[])
            .await
            .unwrap_err();
        assert!(!f.permanent);
        assert!(f.error.contains("network down"));

        std::fs::write(&script, r#"console.log("not json");"#).unwrap();
        let f = run_script(&script_cfg(dir.path()), &test_ctx(), None, &[])
            .await
            .unwrap_err();
        assert!(!f.permanent);

        std::fs::write(&script, r#"setTimeout(() => {}, 60000);"#).unwrap();
        let started = std::time::Instant::now();
        let f = run_script(&script_cfg(dir.path()), &test_ctx(), None, &[])
            .await
            .unwrap_err();
        assert!(
            f.error.contains("超时") || f.error.contains("timed out"),
            "{}",
            f.error
        );
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[tokio::test]
    async fn resolve_node_rejects_missing_configured_path() {
        let err = resolve_node(Some("/definitely/not/node"))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Localized {
                key: "api_key_vars.node_not_found",
                ..
            }
        ));
    }
}
