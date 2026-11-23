//! WSL 发行版枚举和用户目录探测。
//!
//! - 枚举只用 `wsl.exe -l -q`（全部）和 `wsl.exe -l -q --running`（运行中），输出不随
//!   系统语言变化，也不会启动任何发行版；
//! - 只读运行中发行版的 UNC 文件和历史缓存，不执行 WSL shell，不启动停止的发行版；
//!   无法确定 HOME 或自定义环境目录时，由用户填写目标目录；
//! - 结果缓存在设备状态目录（`wsl-environments.json`），不同步。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(windows)]
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(any(windows, test))]
use super::model::is_valid_distro_name;
use super::model::LinuxEnv;
use crate::error::AppError;

/// Docker Desktop、Rancher Desktop、Podman 等工具自带的系统发行版，不作为写入目标。
#[cfg(any(windows, test))]
const SYSTEM_DISTROS: &[&str] = &[
    "docker-desktop",
    "docker-desktop-data",
    "rancher-desktop",
    "rancher-desktop-data",
];

/// HOME 等环境信息多久重新探测一次（运行中的发行版）。
const ENV_MAX_AGE_SECS: i64 = 6 * 60 * 60;

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

#[cfg(windows)]
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
#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
pub fn parse_distro_list(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let name = line.trim().trim_matches('\u{0}').trim();
        if name.is_empty()
            || !is_valid_distro_name(name)
            || SYSTEM_DISTROS.iter().any(|s| s.eq_ignore_ascii_case(name))
            || name.to_ascii_lowercase().starts_with("podman-machine")
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
    let child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = crate::commands::CommandDeadline::from_timeout(Some(timeout));
    crate::commands::wait_child_output(child, deadline)
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

/// 这个发行版现在是否在运行（不启动它）。
#[cfg(windows)]
pub fn is_running(distro: &str) -> bool {
    list(true).is_ok_and(|running| running.iter().any(|r| r.eq_ignore_ascii_case(distro)))
}

/// 检查 WSL 发行版是否仍已注册；`wsl.exe -l -q` 成功时才返回 `Ok(bool)`，失败返回 `Err` 以免误判。
pub fn is_registered(distro: &str) -> Result<bool, String> {
    #[cfg(windows)]
    {
        list(false).map(|all| all.iter().any(|name| name.eq_ignore_ascii_case(distro)))
    }
    #[cfg(not(windows))]
    {
        let _ = distro;
        Err("WSL is only available on Windows".to_string())
    }
}

/// 只读运行中发行版的 UNC 文件，不执行 shell、不启动或重启发行版。
#[cfg(windows)]
pub fn probe(distro: &str) -> Result<LinuxEnv, String> {
    if !is_valid_distro_name(distro) || !is_running(distro) {
        return Err(format!("WSL · {distro} is not running; start it manually"));
    }
    let root = PathBuf::from(format!(r"\\wsl.localhost\{distro}"));
    let passwd = std::fs::read_to_string(root.join("etc/passwd")).unwrap_or_default();
    let wsl_config = std::fs::read_to_string(root.join("etc/wsl.conf")).unwrap_or_default();
    let mut env = cached_env(distro)
        .map(|entry| entry.env)
        .unwrap_or_default();
    env.home = home_from_files(&passwd, &wsl_config).or(env.home);
    let network = std::fs::read_to_string(crate::config::get_home_dir().join(".wslconfig"))
        .unwrap_or_default();
    // 每次刷新重新解析，不能把上次 mirrored 缓存带到已改回 NAT 的配置中。
    env.networking_mode = Some(network_mode_from_config(&network));
    if let Ok(routes) = std::fs::read_to_string(root.join("proc/net/route")) {
        env.gateway = routes
            .lines()
            .find_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.get(1).copied() != Some("00000000") {
                    return None;
                }
                let gateway = u32::from_str_radix(fields.get(2)?, 16).ok()?;
                (gateway != 0).then(|| std::net::Ipv4Addr::from(gateway.to_le_bytes()).to_string())
            })
            .or(env.gateway);
    }
    store_cache(distro, &env);
    Ok(env)
}

#[cfg(any(windows, test))]
fn network_mode_from_config(config: &str) -> String {
    let mut wsl2 = false;
    config
        .lines()
        .find_map(|line| {
            let line = line.split(['#', ';']).next().unwrap_or_default().trim();
            if line.starts_with('[') {
                wsl2 = line.eq_ignore_ascii_case("[wsl2]");
                return None;
            }
            let (key, value) = line.split_once('=')?;
            (wsl2 && key.trim().eq_ignore_ascii_case("networkingMode"))
                .then(|| value.trim().to_ascii_lowercase())
        })
        .unwrap_or_else(|| "nat".into())
}

#[cfg(any(windows, test))]
fn home_from_files(passwd: &str, wsl_config: &str) -> Option<String> {
    let mut user_section = false;
    let default_user = wsl_config.lines().find_map(|line| {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            user_section = line.eq_ignore_ascii_case("[user]");
            return None;
        }
        let (key, value) = line.split_once('=')?;
        (user_section && key.trim().eq_ignore_ascii_case("default")).then(|| value.trim())
    });
    let candidates: Vec<_> = passwd
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            let name = *fields.first()?;
            let uid: u32 = fields.get(2)?.parse().ok()?;
            let home = *fields.get(5)?;
            let selected = default_user.map_or((1000..65534).contains(&uid), |user| user == name);
            (selected && home.starts_with('/')).then(|| home.to_string())
        })
        .collect();
    (candidates.len() == 1).then(|| candidates[0].clone())
}

#[cfg(not(windows))]
pub fn probe(_distro: &str) -> Result<LinuxEnv, String> {
    Err("WSL is only available on Windows".to_string())
}

/// 只枚举并读取运行中发行版的文件；停止的发行版仅使用缓存。
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
                // 运行中的发行版有新近的 HOME 结果就不再进去探测：每次概览都进发行版跑命令，
                // 会一直重置它的空闲计时，让本该自动停下的发行版停不下来。
                let fresh = cached.as_ref().is_some_and(|c| {
                    c.env.home.is_some()
                        && chrono::Utc::now().timestamp() - c.detected_at < ENV_MAX_AGE_SECS
                });
                if is_running && (probe_stopped || !fresh) {
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
                        running: is_running,
                        env: cached.as_ref().map(|c| c.env.clone()).unwrap_or_default(),
                        cached: !is_running && cached.is_some(),
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

/// 优先缓存；允许刷新时只读运行中发行版的 UNC 文件，绝不启动发行版。
pub fn resolve_env(distro: &str, allow_start: bool) -> Result<LinuxEnv, AppError> {
    if let Some(cached) = cached_env(distro).filter(|c| c.env.home.is_some()) {
        return Ok(cached.env);
    }
    if !allow_start {
        return Err(home_unresolved(distro, None));
    }
    probe(distro).map_err(|err| home_unresolved(distro, Some(err)))
}

fn has_fresh_network_env(cached: &CachedEnv) -> bool {
    cached.env.home.is_some()
        && (cached.env.networking_mode.as_deref() == Some("mirrored")
            || cached.env.gateway.is_some())
        && chrono::Utc::now().timestamp() - cached.detected_at < ENV_MAX_AGE_SECS
}

/// 解析网络相关环境：优先使用带可用网络信息的新近缓存；`force_probe` 或缓存过期/缺网关时重新探测。
pub fn resolve_network_env(
    distro: &str,
    allow_start: bool,
    force_probe: bool,
) -> Result<LinuxEnv, AppError> {
    if !force_probe {
        if let Some(cached) = cached_env(distro).filter(has_fresh_network_env) {
            return Ok(cached.env);
        }
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
            parse_distro_list(&decode_wsl_output(
                b"Alpine\ndocker-desktop\npodman-machine-default\nalpine\n\n"
            )),
            vec!["Alpine"]
        );
    }

    #[test]
    fn filesystem_home_requires_an_unambiguous_or_explicit_user() {
        let passwd = "root:x:0:0::/root:/bin/sh\nme:x:1000:1000::/home/me:/bin/bash\n";
        assert_eq!(home_from_files(passwd, ""), Some("/home/me".into()));
        assert_eq!(
            home_from_files(passwd, "[user]\ndefault = root"),
            Some("/root".into())
        );
        let multiple = format!("{passwd}other:x:1001:1001::/srv/other:/bin/sh\n");
        assert_eq!(home_from_files(&multiple, ""), None);
        assert_eq!(
            home_from_files(&multiple, "[user]\ndefault = other # chosen"),
            Some("/srv/other".into())
        );
        assert_eq!(
            home_from_files(&multiple, "[user]\ndefault = missing"),
            None
        );
        assert_eq!(
            home_from_files("me:x:1000:1000::relative:/bin/sh", ""),
            None
        );
    }

    #[test]
    fn network_config_accepts_spacing_and_clears_stale_mirrored_mode() {
        assert_eq!(
            network_mode_from_config("[wsl2]\nnetworkingMode = Mirrored # LAN"),
            "mirrored"
        );
        assert_eq!(
            network_mode_from_config("[wsl2]\nnetworkingMode=nat"),
            "nat"
        );
        assert_eq!(network_mode_from_config(""), "nat");
        assert_eq!(
            network_mode_from_config("[other]\nnetworkingMode=mirrored"),
            "nat"
        );
    }
}
