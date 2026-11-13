//! 各 WSL 发行版里 Claude Code、Codex、Gemini CLI、OpenCode 的安装情况。
//!
//! 只有检测到某个客户端装在某个发行版里，才在设置页和供应商页给出那个发行版的配置入口
//! （已经保存过的目标照常显示）。探测要进发行版跑一次登录 shell，结果缓存在
//! `~/.cc-switch/wsl-tool-installs.json`：运行中的发行版缓存过期后重新探测，停止的发行版
//! 只用上次结果（不为此启动它），用户点「重新检测」时才全部重新探测。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::model;
use super::wsl::WslInventory;

/// 运行中的发行版，探测结果超过这么久就重新探测。
const MAX_AGE_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallStatus {
    Installed,
    /// 找到了但跑不起来（比如 Node 版本不够）。
    Broken,
    NotInstalled,
    /// 探测失败，不等于未安装。
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInstall {
    pub status: InstallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 发行版里 `command -v` 找到的可执行文件路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub detected_at: i64,
    /// 这次没有重新探测，用的是上次结果（发行版没在运行）。
    #[serde(default)]
    pub cached: bool,
}

impl ToolInstall {
    pub fn present(&self) -> bool {
        matches!(
            self.status,
            InstallStatus::Installed | InstallStatus::Broken
        )
    }
}

/// 发行版 → 应用 ID → 安装情况。
pub type Installs = BTreeMap<String, BTreeMap<String, ToolInstall>>;

fn cache_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join("wsl-tool-installs.json")
}

fn lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

pub fn load_cache() -> Installs {
    std::fs::read(cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn store(installs: &Installs) {
    match serde_json::to_vec_pretty(installs) {
        Ok(bytes) => {
            if let Err(err) = crate::config::atomic_write_private(&cache_path(), &bytes) {
                log::warn!("保存 WSL 安装检测结果失败: {err}");
            }
        }
        Err(err) => log::warn!("序列化 WSL 安装检测结果失败: {err}"),
    }
}

fn probe_distro(distro: &str) -> BTreeMap<String, ToolInstall> {
    let now = chrono::Utc::now().timestamp();
    let handles: Vec<_> = model::supported_apps()
        .iter()
        .map(|app| {
            let tool = app.as_str().to_string();
            let distro = distro.to_string();
            std::thread::spawn(move || {
                let state = crate::commands::wsl_tool_state(&tool, &distro);
                (tool, state)
            })
        })
        .collect();
    handles
        .into_iter()
        .filter_map(|handle| handle.join().ok())
        .map(|(tool, state)| {
            use crate::commands::WslToolState;
            let (status, version, path, error) = match state {
                WslToolState::Installed { version, path } => (
                    InstallStatus::Installed,
                    Some(version).filter(|v| !v.is_empty()),
                    path,
                    None,
                ),
                WslToolState::Broken { error, path } => {
                    (InstallStatus::Broken, None, path, Some(error))
                }
                WslToolState::NotInstalled => (InstallStatus::NotInstalled, None, None, None),
                WslToolState::Unknown(error) => (InstallStatus::Unknown, None, None, Some(error)),
            };
            (
                tool,
                ToolInstall {
                    status,
                    version,
                    path,
                    error: error.map(|e| e.chars().take(300).collect()),
                    detected_at: now,
                    cached: false,
                },
            )
        })
        .collect()
}

/// 当前的安装情况。运行中的发行版在缓存过期或 `refresh` 时重新探测（应用页「检查更新」）；
/// 停止的发行版只在 `start_stopped` 时探测（会启动它），否则用上次结果并标 `cached`。
pub fn detect(inventory: &WslInventory, refresh: bool, start_stopped: bool) -> Installs {
    if !inventory.host_windows {
        return Installs::new();
    }
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = load_cache();
    let now = chrono::Utc::now().timestamp();
    let mut changed = false;
    let mut out = Installs::new();
    let jobs: Vec<_> = inventory
        .distros
        .iter()
        .filter(|env| {
            let fresh = cache
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&env.distro))
                .is_some_and(|(_, tools)| {
                    tools.len() == model::supported_apps().len()
                        && tools.values().all(|t| now - t.detected_at < MAX_AGE_SECS)
                });
            start_stopped || (env.running && (refresh || !fresh))
        })
        .map(|env| {
            let distro = env.distro.clone();
            std::thread::spawn(move || (distro.clone(), probe_distro(&distro)))
        })
        .collect();
    for job in jobs {
        if let Ok((distro, tools)) = job.join() {
            cache.retain(|name, _| !name.eq_ignore_ascii_case(&distro));
            cache.insert(distro, tools);
            changed = true;
        }
    }
    if changed {
        store(&cache);
    }
    for env in &inventory.distros {
        let Some((_, tools)) = cache
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&env.distro))
        else {
            continue;
        };
        let mut tools = tools.clone();
        for tool in tools.values_mut() {
            tool.cached = !env.running || now - tool.detected_at >= MAX_AGE_SECS;
        }
        out.insert(env.distro.clone(), tools);
    }
    out
}

/// 某个发行版里装没装这个应用：`None` 表示还没探测过。
pub fn installed(installs: &Installs, distro: &str, app: &str) -> Option<bool> {
    installs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(distro))
        .and_then(|(_, tools)| tools.get(app))
        .and_then(|tool| match tool.status {
            InstallStatus::Installed | InstallStatus::Broken => Some(true),
            InstallStatus::NotInstalled => Some(false),
            InstallStatus::Unknown => None,
        })
}

/// 应用页装完 / 更新完之后，下次概览重新探测这个发行版。
pub fn invalidate(distro: &str) {
    let _guard = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = load_cache();
    let before = cache.len();
    cache.retain(|name, _| !name.eq_ignore_ascii_case(distro));
    if cache.len() != before {
        store(&cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(status: InstallStatus) -> ToolInstall {
        ToolInstall {
            status,
            version: None,
            path: None,
            error: None,
            detected_at: 0,
            cached: false,
        }
    }

    #[test]
    fn unknown_probes_are_not_reported_as_missing() {
        let mut installs = Installs::new();
        installs.insert(
            "Alpine".into(),
            BTreeMap::from([
                ("codex".to_string(), tool(InstallStatus::Installed)),
                ("claude".to_string(), tool(InstallStatus::NotInstalled)),
                ("gemini".to_string(), tool(InstallStatus::Unknown)),
                ("opencode".to_string(), tool(InstallStatus::Broken)),
            ]),
        );
        assert_eq!(installed(&installs, "alpine", "codex"), Some(true));
        assert_eq!(installed(&installs, "Alpine", "claude"), Some(false));
        assert_eq!(installed(&installs, "Alpine", "gemini"), None);
        assert_eq!(installed(&installs, "Alpine", "opencode"), Some(true));
        assert_eq!(installed(&installs, "Debian", "codex"), None);
    }

    #[test]
    fn non_windows_hosts_never_probe() {
        let inventory = WslInventory {
            host_windows: false,
            wsl_available: false,
            distros: Vec::new(),
            error: None,
        };
        assert!(detect(&inventory, true, true).is_empty());
    }
}
