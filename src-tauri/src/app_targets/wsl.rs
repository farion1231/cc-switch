//! WSL 发行版枚举和用户目录探测。
//!
//! - 枚举只用 `wsl.exe -l -q`（全部）和 `wsl.exe -l -q --running`（运行中），输出不随
//!   系统语言变化，也不会启动任何发行版；
//! - HOME 等环境变量要进发行版里读：默认只探测运行中的发行版，停止的用上次结果并标
//!   「上次检测」，只有写入或用户点「重新检测」时才允许启动；
//! - 结果缓存在设备状态目录（`wsl-environments.json`），不同步。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(windows)]
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::model::{is_valid_distro_name, LinuxEnv};
use crate::error::AppError;

/// Docker Desktop 等自带的系统发行版，不作为写入目标。
const SYSTEM_DISTROS: &[&str] = &[
    "docker-desktop",
    "docker-desktop-data",
    "rancher-desktop",
    "rancher-desktop-data",
];

#[cfg(windows)]
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedEnv {
    #[serde(flatten)]
    pub env: LinuxEnv,
    /// 探测时间（Unix 秒）。
    pub detected_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslEnvironment {
    pub distro: String,
    /// 本次枚举到了这个发行版。已保存但枚举不到的发行版由调用方补上并标不可用。
    pub available: bool,
    pub running: bool,
    #[serde(flatten)]
    pub env: LinuxEnv,
    /// 环境信息来自缓存（发行版未运行，没有重新探测）。
    pub cached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_at: Option<i64>,
    /// 本次探测失败的原因（超时、权限不足等），不等于「未安装」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslInventory {
    /// 宿主是 Windows（多环境界面只在 Windows 上出现）。
    pub host_windows: bool,
    /// 能运行 `wsl.exe` 并列出发行版。
    pub wsl_available: bool,
    pub distros: Vec<WslEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn cache_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join("wsl-environments.json")
}

fn cache_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

pub fn load_cache() -> BTreeMap<String, CachedEnv> {
    let _guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    std::fs::read(cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn store_cache(distro: &str, env: &LinuxEnv) {
    let _guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut cache: BTreeMap<String, CachedEnv> = std::fs::read(cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    cache.retain(|name, _| !name.eq_ignore_ascii_case(distro));
    cache.insert(
        distro.to_string(),
        CachedEnv {
            env: env.clone(),
            detected_at: chrono::Utc::now().timestamp(),
        },
    );
    match serde_json::to_vec_pretty(&cache) {
        Ok(bytes) => {
            if let Err(err) = crate::config::atomic_write_private(&cache_path(), &bytes) {
                log::warn!("保存 WSL 环境缓存失败: {err}");
            }
        }
        Err(err) => log::warn!("序列化 WSL 环境缓存失败: {err}"),
    }
}

pub fn cached_env(distro: &str) -> Option<CachedEnv> {
    load_cache()
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(distro))
        .map(|(_, env)| env)
}

/// `wsl.exe` 默认输出 UTF-16LE（设了 `WSL_UTF8=1` 时是 UTF-8），两种都认。
pub fn decode_wsl_output(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).take(16).all(|b| *b == 0)
        || bytes.starts_with(&[0xFF, 0xFE]);
    if looks_utf16 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_start_matches('\u{feff}')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

/// `wsl.exe -l -q` 的发行版名单（去掉系统发行版和非法名称）。
pub fn parse_distro_list(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let name = line.trim().trim_matches('\u{0}').trim();
        if name.is_empty()
            || !is_valid_distro_name(name)
            || SYSTEM_DISTROS.iter().any(|s| s.eq_ignore_ascii_case(name))
            || out
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            continue;
        }
        out.push(name.to_string());
    }
    out
}

const PROBE_MARKER: &str = "__CC_SWITCH_ENV__";

/// 在发行版里打印环境变量的脚本：登录 shell 读 `~/.profile`，标记行之后才是结果，
/// profile 打印的其他内容不影响解析。
pub fn probe_script() -> String {
    format!(
        "printf '%s\\n' '{PROBE_MARKER}' \"$HOME\" \"${{CLAUDE_CONFIG_DIR:-}}\" \"${{CODEX_HOME:-}}\" \"${{XDG_CONFIG_HOME:-}}\""
    )
}

pub fn parse_probe_output(output: &str) -> Option<LinuxEnv> {
    let mut lines = output
        .lines()
        .skip_while(|line| line.trim() != PROBE_MARKER);
    lines.next()?;
    let mut field = || {
        lines
            .next()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let env = LinuxEnv {
        home: field(),
        claude_config_dir: field(),
        codex_home: field(),
        xdg_config_home: field(),
    };
    env.home
        .as_deref()
        .filter(|home| home.starts_with('/'))
        .map(|_| env.clone())
}

#[cfg(windows)]
fn wsl_command(args: &[&str]) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new("wsl.exe");
    command
        .args(args)
        .env("WSL_UTF8", "1")
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

/// 跑一个命令，超时就杀掉。
#[cfg(windows)]
fn run_with_timeout(
    mut command: std::process::Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timeout after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(err.to_string()),
        }
    }
}

#[cfg(windows)]
fn list(running_only: bool) -> Result<Vec<String>, String> {
    let args: &[&str] = if running_only {
        &["-l", "-q", "--running"]
    } else {
        &["-l", "-q"]
    };
    let output = run_with_timeout(wsl_command(args), Duration::from_secs(10))?;
    let text = decode_wsl_output(&output.stdout);
    if !output.status.success() {
        // 没有运行中的发行版时 `--running` 也可能返回非零；名单为空即可。
        if running_only {
            return Ok(Vec::new());
        }
        let err = decode_wsl_output(&output.stderr);
        return Err(format!("{} {}", text.trim(), err.trim()).trim().to_string());
    }
    Ok(parse_distro_list(&text))
}

/// 进发行版里读 HOME 等环境变量。发行版没在运行时会被启动。
#[cfg(windows)]
pub fn probe(distro: &str) -> Result<LinuxEnv, String> {
    if !is_valid_distro_name(distro) {
        return Err(format!("invalid distro name: {distro}"));
    }
    let script = probe_script();
    let output = run_with_timeout(
        wsl_command(&["-d", distro, "--exec", "sh", "-lc", &script]),
        PROBE_TIMEOUT,
    )?;
    let text = decode_wsl_output(&output.stdout);
    let env = parse_probe_output(&text).ok_or_else(|| {
        let err = decode_wsl_output(&output.stderr);
        format!("no HOME reported: {}", err.trim())
    })?;
    store_cache(distro, &env);
    Ok(env)
}

#[cfg(not(windows))]
pub fn probe(_distro: &str) -> Result<LinuxEnv, String> {
    Err("WSL is only available on Windows".to_string())
}

/// 枚举发行版。`probe_stopped` 为真时连停止的发行版也启动探测（用户点「重新检测」）。
pub fn inventory(probe_stopped: bool) -> WslInventory {
    #[cfg(windows)]
    {
        let all = match list(false) {
            Ok(all) => all,
            Err(error) => {
                return WslInventory {
                    host_windows: true,
                    wsl_available: false,
                    distros: Vec::new(),
                    error: Some(error),
                }
            }
        };
        let running = list(true).unwrap_or_default();
        let cache = load_cache();
        let distros = all
            .into_iter()
            .map(|distro| {
                let is_running = running.iter().any(|r| r.eq_ignore_ascii_case(&distro));
                let cached = cache
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&distro))
                    .map(|(_, env)| env.clone());
                if is_running || probe_stopped {
                    match probe(&distro) {
                        Ok(env) => WslEnvironment {
                            distro,
                            available: true,
                            running: true,
                            env,
                            cached: false,
                            detected_at: Some(chrono::Utc::now().timestamp()),
                            error: None,
                        },
                        Err(error) => WslEnvironment {
                            distro,
                            available: true,
                            running: is_running,
                            env: cached.as_ref().map(|c| c.env.clone()).unwrap_or_default(),
                            cached: cached.is_some(),
                            detected_at: cached.map(|c| c.detected_at),
                            error: Some(error),
                        },
                    }
                } else {
                    WslEnvironment {
                        distro,
                        available: true,
                        running: false,
                        env: cached.as_ref().map(|c| c.env.clone()).unwrap_or_default(),
                        cached: cached.is_some(),
                        detected_at: cached.map(|c| c.detected_at),
                        error: None,
                    }
                }
            })
            .collect();
        WslInventory {
            host_windows: true,
            wsl_available: true,
            distros,
            error: None,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = probe_stopped;
        WslInventory {
            host_windows: false,
            wsl_available: false,
            distros: Vec::new(),
            error: None,
        }
    }
}

/// 解析一个发行版的环境：优先缓存；没有缓存且 `allow_start` 时进发行版探测（会启动它）。
pub fn resolve_env(distro: &str, allow_start: bool) -> Result<LinuxEnv, AppError> {
    if let Some(cached) = cached_env(distro).filter(|c| c.env.home.is_some()) {
        return Ok(cached.env);
    }
    if !allow_start {
        return Err(home_unresolved(distro, None));
    }
    probe(distro).map_err(|err| home_unresolved(distro, Some(err)))
}

fn home_unresolved(distro: &str, detail: Option<String>) -> AppError {
    let suffix = detail.map(|d| format!(" ({d})")).unwrap_or_default();
    AppError::localized(
        "app_targets.home_unresolved",
        format!("未获取到 WSL 发行版 {distro} 的用户目录，请填写自定义目录{suffix}"),
        format!("Could not resolve the home directory of WSL distro {distro}; enter a custom directory{suffix}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_utf8_listings() {
        let utf16: Vec<u8> = "\u{feff}Ubuntu-24.04\r\nDebian\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(
            parse_distro_list(&decode_wsl_output(&utf16)),
            vec!["Ubuntu-24.04", "Debian"]
        );
        assert_eq!(
            parse_distro_list(&decode_wsl_output(b"Alpine\ndocker-desktop\nalpine\n\n")),
            vec!["Alpine"]
        );
    }

    #[test]
    fn probe_output_ignores_profile_noise() {
        let out = format!("Welcome!\n{PROBE_MARKER}\n/home/me\n\n/srv/codex\n/home/me/.cfg\n");
        let env = parse_probe_output(&out).expect("parsed");
        assert_eq!(env.home.as_deref(), Some("/home/me"));
        assert_eq!(env.claude_config_dir, None);
        assert_eq!(env.codex_home.as_deref(), Some("/srv/codex"));
        assert_eq!(env.xdg_config_home.as_deref(), Some("/home/me/.cfg"));
        assert_eq!(parse_probe_output("no marker"), None);
        assert_eq!(parse_probe_output(&format!("{PROBE_MARKER}\n\n")), None);
    }

    #[test]
    fn probe_script_prints_the_marker_first() {
        assert!(probe_script().starts_with("printf '%s\\n' '__CC_SWITCH_ENV__'"));
    }
}
