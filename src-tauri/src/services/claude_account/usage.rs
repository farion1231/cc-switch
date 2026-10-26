//! 查一个账号的额度时，访问令牌过期了怎么办：在它的目录里跑一次 `claude -p /usage`。
//!
//! 这条命令由 Claude Code 自己回答、不问模型：它按自己的锁刷新令牌、查额度，把结果缓存进那个
//! 目录 `.claude.json` 的 `cachedUsageUtilization`（和 `/api/oauth/usage` 的返回同一个结构），
//! CC Switch 读这份缓存。CC Switch 自己从不刷新令牌。

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::login::clear_account_env;

/// 同一个账号两次 `/usage` 至少隔这么久（失败也算）：令牌刷新后就能直接查，用不着再跑。
const RUN_INTERVAL: Duration = Duration::from_secs(10 * 60);
const RUN_TIMEOUT: Duration = Duration::from_secs(45);
const LOCATE_TIMEOUT: Duration = Duration::from_secs(15);
/// 令牌过期时只信这么新的缓存：最近一次 `/usage` 跑成功写下的。更旧的说明这段时间里没跑成
/// （登录失效、没装 claude…），显示它会把「要重新登录」藏起来。
pub const CACHE_MAX_AGE: Duration =
    Duration::from_secs(RUN_INTERVAL.as_secs() + RUN_TIMEOUT.as_secs());

static LAST_RUN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
/// 一次只跑一个：几张卡同时过期时排队，不同时起一串进程。
static RUNNING: Mutex<()> = Mutex::new(());

/// Claude Code 缓存的额度。
#[derive(Debug, Clone, PartialEq)]
pub struct CachedUsage {
    pub fetched_at_ms: i64,
    /// `/api/oauth/usage` 的返回
    pub utilization: Value,
}

/// 读 `.claude.json`（`profile_path`）里 Claude Code 缓存的额度，只认 `account_uuid` 的。
pub fn read_cached(profile_path: &Path, account_uuid: &str) -> Option<CachedUsage> {
    let doc: Value = serde_json::from_slice(&std::fs::read(profile_path).ok()?).ok()?;
    let cached = doc.get("cachedUsageUtilization")?;
    // 缓存跟账号走：Claude Code 自己也只认 accountUuid 对得上的
    if cached.get("accountUuid").and_then(Value::as_str) != Some(account_uuid) {
        return None;
    }
    let fetched = cached.get("fetchedAtMs")?;
    let fetched_at_ms = fetched
        .as_i64()
        .or_else(|| fetched.as_f64().map(|at| at as i64))?;
    let utilization = cached.get("utilization").filter(|v| v.is_object())?.clone();
    Some(CachedUsage {
        fetched_at_ms,
        utilization,
    })
}

/// 在 `config_dir`（`None` 是默认的 `~/.claude`）里跑一次 `claude -p /usage`，让 Claude Code
/// 刷新令牌、更新额度缓存。同一个账号 [`RUN_INTERVAL`] 内只跑一次，返回是否真的跑了。阻塞。
pub fn refresh_via_cli(account: &str, config_dir: Option<&Path>) -> bool {
    // 刚跑过的号不用排在别的号后面，直接读缓存
    if ran_recently(account, false) {
        return false;
    }
    let _running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    // 排队时同一个号可能刚被别的请求跑完
    if ran_recently(account, true) {
        return false;
    }
    if let Err(error) = run_usage(config_dir) {
        log::warn!("[ClaudeAccount] 在账号目录里查额度失败: {error}");
    }
    true
}

/// `account` 在 [`RUN_INTERVAL`] 内跑过没有；`claim` 时没跑过就记成现在跑（失败也算）。
fn ran_recently(account: &str, claim: bool) -> bool {
    let mut last = LAST_RUN.lock().unwrap_or_else(|e| e.into_inner());
    let last = last.get_or_insert_with(HashMap::new);
    if last
        .get(account)
        .is_some_and(|at| at.elapsed() < RUN_INTERVAL)
    {
        return true;
    }
    if claim {
        last.insert(account.to_string(), Instant::now());
    }
    false
}

fn run_usage(config_dir: Option<&Path>) -> Result<(), String> {
    let mut cmd = crate::commands::detected_tool_command("claude", &usage_args(), LOCATE_TIMEOUT)?;
    clear_account_env(&mut cmd);
    match config_dir {
        Some(dir) => cmd.env("CLAUDE_CONFIG_DIR", dir),
        None => cmd.env_remove("CLAUDE_CONFIG_DIR"),
    };
    // 设了它，/usage 不联网、只打印一小时内别处留下的读数
    cmd.env_remove("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC")
        // 不在共享的 projects/ 里给工作目录建记忆目录
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_ERROR_REPORTING", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .current_dir(crate::config::get_home_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("无法启动 claude: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if started.elapsed() < RUN_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Ok(None) => {
                crate::commands::terminate_child_tree(&mut child);
                let _ = child.wait();
                return Err("超时".to_string());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// 只要 Claude Code 自己回答的 /usage：不带工具、MCP、任何设置来源，不留会话。
fn usage_args() -> [&'static str; 10] {
    [
        "-p",
        "/usage",
        "--output-format",
        "json",
        "--tools",
        "",
        "--strict-mcp-config",
        "--setting-sources",
        "",
        "--no-session-persistence",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, doc: Value) -> std::path::PathBuf {
        let path = dir.join(".claude.json");
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        path
    }

    #[test]
    fn reads_only_the_accounts_own_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let usage = serde_json::json!({"five_hour": {"utilization": 13.0, "resets_at": null}});
        let path = write(
            tmp.path(),
            serde_json::json!({"cachedUsageUtilization": {
                "fetchedAtMs": 1_700_000_000_000_i64,
                "accountUuid": "acc-1",
                "utilization": usage,
            }}),
        );
        assert_eq!(
            read_cached(&path, "acc-1"),
            Some(CachedUsage {
                fetched_at_ms: 1_700_000_000_000,
                utilization: usage,
            })
        );
        assert_eq!(read_cached(&path, "acc-2"), None);
    }

    #[test]
    fn no_cache_without_an_account_or_utilization() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(
            tmp.path(),
            serde_json::json!({"cachedUsageUtilization": {
                "fetchedAtMs": 1, "utilization": {}
            }}),
        );
        assert_eq!(read_cached(&path, "acc-1"), None);
        assert_eq!(read_cached(&tmp.path().join("missing.json"), "acc-1"), None);
    }

    #[test]
    fn usage_run_asks_no_model_and_keeps_no_session() {
        let args = usage_args();
        assert_eq!(&args[..2], ["-p", "/usage"]);
        assert!(args.contains(&"--no-session-persistence"));
        assert!(args.contains(&"--strict-mcp-config"));
    }
}
