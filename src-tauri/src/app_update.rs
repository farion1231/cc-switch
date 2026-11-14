//! 应用自动更新：开关打开后在后台下载新版本，装不装由用户确认。
//!
//! 下载好的安装包放在进程级缓存里，不放在前端：轻量模式会销毁主窗口，React 状态跟着没了，
//! 后台下好的包不能跟着丢。主窗口重建后用 [`status`] 取回「已下载、待安装」。
//!
//! 只缓存在内存里（十几 MB），不落盘：落盘的包下次启动要重新验签、还要处理残留，换来的只是
//! 退出后不用再下一次，不值。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

/// 待安装的版本变了就发给前端：载荷是 `Option<ReadyUpdate>`，`null` 表示缓存作废了
/// （版本被撤回），提示要收起。
pub const UPDATE_READY_EVENT: &str = "app-update-ready";

/// 启动后第一次检查的延迟：避开启动时的接管、导入等重活。
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
/// CC Switch 常驻托盘，一挂就是几天；只在启动时查一次，这个开关就形同虚设。
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

struct PendingUpdate {
    update: Update,
    bytes: Vec<u8>,
}

static PENDING: Mutex<Option<PendingUpdate>> = Mutex::new(None);
static DOWNLOADING: AtomicBool = AtomicBool::new(false);
/// 弹窗、关于页、托盘三个入口共用：同一时间只能有一次安装（安装会替换应用、做退出清理）。
static INSTALLING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadyUpdate {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoUpdateStatus {
    /// 这台机器能不能自动安装（便携版、Linux 非 AppImage 不行）。
    pub supported: bool,
    pub downloading: bool,
    /// 已下载、待安装的版本。开关关着时不报（关掉即视为不要了）。
    pub ready: Option<ReadyUpdate>,
    /// 有应用在用路由服务：重启会切断进行中的请求，安装提示里要写明。
    pub routing_in_use: bool,
}

/// 便携版：可执行文件旁边有 `portable.ini`。便携版没有安装器，只能去下载页手动换。
pub fn is_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("portable.ini").is_file()))
        .unwrap_or(false)
}

/// Tauri updater 在 Linux 上只会替换 AppImage；deb / rpm 装的版本由系统包管理器管。
#[cfg(target_os = "linux")]
fn platform_supports_self_install() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

#[cfg(not(target_os = "linux"))]
fn platform_supports_self_install() -> bool {
    true
}

pub fn auto_install_supported() -> bool {
    !is_portable() && platform_supports_self_install()
}

fn enabled() -> bool {
    crate::settings::get_settings().auto_update_enabled
}

fn pending_guard() -> std::sync::MutexGuard<'static, Option<PendingUpdate>> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

/// 已下载、待安装的版本；开关关着时视为没有。
pub fn ready() -> Option<ReadyUpdate> {
    if !enabled() {
        return None;
    }
    pending_guard().as_ref().map(|pending| ReadyUpdate {
        version: pending.update.version.clone(),
        notes: pending.update.body.clone(),
    })
}

pub fn status(routing_in_use: bool) -> AutoUpdateStatus {
    AutoUpdateStatus {
        supported: auto_install_supported(),
        downloading: DOWNLOADING.load(Ordering::SeqCst),
        ready: ready(),
        routing_in_use,
    }
}

/// 缓存里这个版本的安装包（复制一份）。不从缓存取走：安装失败时还能再装，离线也能重试；
/// 安装成功进程就退出了，缓存不用清。
pub fn pending_bytes_for(version: &str) -> Option<Vec<u8>> {
    pending_guard()
        .as_ref()
        .filter(|pending| pending.update.version == version)
        .map(|pending| pending.bytes.clone())
}

/// 离线时的退路：检查不了更新，就装已经下好的那个版本（同样只复制、不取走）。
pub fn any_pending() -> Option<(Update, Vec<u8>)> {
    pending_guard()
        .as_ref()
        .map(|pending| (pending.update.clone(), pending.bytes.clone()))
}

/// 缓存作废（服务器上已经没有这个版本了）：清掉，并让前端提示和托盘入口一起收起。
pub fn discard_pending(app: &AppHandle) {
    if pending_guard().take().is_some() {
        log::info!("已下载的应用更新不再可用，已丢弃");
        let _ = app.emit(UPDATE_READY_EVENT, None::<ReadyUpdate>);
        crate::tray::refresh_tray_menu(app);
    }
}

/// 安装期间持有；安装失败返回时随 drop 释放，成功时进程直接退出 / 重启。
pub struct InstallGuard;

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

pub fn is_installing() -> bool {
    INSTALLING.load(Ordering::SeqCst)
}

/// 拿到安装权；已经有一次安装在跑就返回 `None`。
pub fn begin_install() -> Option<InstallGuard> {
    // 不能写 `then_some(InstallGuard)`：守卫会先构造出来，拿不到安装权时随即 drop，
    // 反而把别人持有的锁清掉。
    if INSTALLING.swap(true, Ordering::SeqCst) {
        None
    } else {
        Some(InstallGuard)
    }
}

pub fn build_updater(app: &AppHandle) -> Result<Updater, String> {
    let builder = app.updater_builder();
    // Windows 上 updater 以 `msiexec /passive /promptrestart` 安装 MSI。passive 没有模态框、
    // 未指定重启选项时，安装目录里有文件被占用，Windows Installer 会不经确认直接重启系统
    // （#8534）。`/promptrestart` 只对应 REBOOTPROMPT，挡不住这一点，因此显式追加
    // REBOOT=ReallySuppress（即 `/norestart`）。旧版本的 updater 不带这个参数，
    // 由 MSI 模板自身的 REBOOT 属性兜底。
    #[cfg(target_os = "windows")]
    let builder = builder.installer_arg("REBOOT=ReallySuppress");
    builder
        .build()
        .map_err(|e| format!("初始化更新器失败: {e}"))
}

struct DownloadingFlag;

impl Drop for DownloadingFlag {
    fn drop(&mut self) {
        DOWNLOADING.store(false, Ordering::SeqCst);
    }
}

/// 检查并下载最新版本，放进缓存，然后通知前端和托盘。已经下好同一版本就直接返回。
/// 同时只跑一份：正在下载时再调用，返回当前缓存（可能为空）。
pub async fn download_latest(app: &AppHandle) -> Result<Option<ReadyUpdate>, String> {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Ok(ready());
    }
    let _flag = DownloadingFlag;

    let Some(update) = build_updater(app)?
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?
    else {
        // 已是最新（或缓存的版本被撤回了）：缓存的包不能再装。
        discard_pending(app);
        return Ok(None);
    };

    let cached_same_version = pending_guard()
        .as_ref()
        .is_some_and(|pending| pending.update.version == update.version);
    if cached_same_version {
        return Ok(ready());
    }

    log::info!("后台下载应用更新: {}", update.version);
    // download() 内部会校验签名，拿到字节即说明包可信。
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| format!("下载更新失败: {e}"))?;
    log::info!("应用更新已下载，等待用户确认安装: {}", update.version);

    let ready_update = ReadyUpdate {
        version: update.version.clone(),
        notes: update.body.clone(),
    };
    *pending_guard() = Some(PendingUpdate { update, bytes });

    let _ = app.emit(UPDATE_READY_EVENT, Some(&ready_update));
    crate::tray::refresh_tray_menu(app);
    Ok(Some(ready_update))
}

/// 开关打开时，启动一分钟后检查一次，之后每 6 小时一次。
///
/// 开发构建不跑：dev 和正式版共用 `~/.cc-switch/settings.json`，开着开关的话每次 `pnpm dev`
/// 都会下一遍正式安装包，而 dev 二进制根本装不了。需要时在关于页手动触发。
pub fn spawn_background_loop(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            if enabled() && auto_install_supported() {
                if let Err(e) = download_latest(&app).await {
                    log::warn!("自动更新下载失败: {e}");
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_install_runs_at_a_time_and_failure_releases_it() {
        let first = begin_install().expect("first install gets the slot");
        assert!(is_installing());
        assert!(begin_install().is_none(), "second entry point is refused");
        // 被拒的那次不能顺手把锁放掉，第三个入口照样要被拒
        assert!(is_installing());
        assert!(
            begin_install().is_none(),
            "third entry point is refused too"
        );
        drop(first); // 安装失败返回
        assert!(!is_installing());
        let again = begin_install();
        assert!(again.is_some());
    }
}
