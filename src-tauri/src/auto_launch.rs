use crate::error::AppError;
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

const APP_NAME: &str = "CC Switch";

/// 初始化 AutoLaunch 实例
fn get_auto_launch() -> Result<AutoLaunch, AppError> {
    let exe_path =
        std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;

    // 使用 AutoLaunchBuilder 消除平台差异
    // macOS: 使用 LaunchAgent plist 方式（兼容 macOS 13+），
    //        plist 的 ProgramArguments[0] 需要完整的可执行文件路径（非 .app 目录）
    // Windows/Linux: 使用注册表/XDG autostart
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe_path.to_string_lossy())
        .set_use_launch_agent(true)
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))?;

    Ok(auto_launch)
}

/// 启用开机自启
pub fn enable_auto_launch() -> Result<(), AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .enable()
        .map_err(|e| AppError::Message(format!("启用开机自启失败: {e}")))?;
    log::info!("已启用开机自启");
    Ok(())
}

/// 禁用开机自启
pub fn disable_auto_launch() -> Result<(), AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .disable()
        .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
    log::info!("已禁用开机自启");
    Ok(())
}

/// 检查是否已启用开机自启
pub fn is_auto_launch_enabled() -> Result<bool, AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .is_enabled()
        .map_err(|e| AppError::Message(format!("检查开机自启状态失败: {e}")))
}

/// 从旧的 AppleScript login item 迁移到 LaunchAgent plist（仅 macOS）
/// 应用启动时调用一次，幂等操作
#[cfg(target_os = "macos")]
pub fn migrate_from_applescript() {
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("迁移检查跳过：无法获取应用路径: {e}");
            return;
        }
    };

    // Old code used .app bundle path for AppleScript login items,
    // so we must match that when checking/disabling the old item.
    let old_app_path = get_macos_app_bundle_path(&exe_path)
        .unwrap_or_else(|| exe_path.clone());

    // Build an AutoLaunch instance using the OLD AppleScript method
    // (without set_use_launch_agent)
    let old_auto_launch = match AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&old_app_path.to_string_lossy())
        .build()
    {
        Ok(al) => al,
        Err(e) => {
            log::warn!("迁移检查跳过：无法创建旧 AutoLaunch 实例: {e}");
            return;
        }
    };

    // Check if old AppleScript login item exists
    let old_enabled = match old_auto_launch.is_enabled() {
        Ok(enabled) => enabled,
        Err(e) => {
            log::warn!("迁移检查跳过：无法检查旧 AppleScript 状态: {e}");
            return;
        }
    };

    if !old_enabled {
        log::debug!("无旧 AppleScript login item，跳过迁移");
        return;
    }

    log::info!("检测到旧 AppleScript login item，开始迁移到 LaunchAgent...");

    // Enable new LaunchAgent first, then disable old AppleScript.
    // This order ensures the user never loses auto-launch capability:
    // if new fails, old is preserved; brief dual-launch is harmless.
    if let Err(e) = enable_auto_launch() {
        log::error!("迁移失败：无法启用新 LaunchAgent: {e}，保留旧 AppleScript login item");
        return;
    }
    log::info!("已启用新 LaunchAgent");

    // Disable old AppleScript login item
    if let Err(e) = old_auto_launch.disable() {
        log::warn!("迁移部分完成：新 LaunchAgent 已启用，但无法禁用旧 AppleScript login item: {e}");
        return;
    }

    log::info!("迁移完成：已从 AppleScript 切换到 LaunchAgent");
}

/// Convert exe path to .app bundle path for legacy AppleScript login item matching.
/// e.g. `/Applications/CC Switch.app/Contents/MacOS/CC Switch` → `/Applications/CC Switch.app`
#[cfg(target_os = "macos")]
fn get_macos_app_bundle_path(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let path_str = exe_path.to_string_lossy();
    if let Some(app_pos) = path_str.find(".app/Contents/MacOS/") {
        let app_bundle_end = app_pos + 4; // end of ".app"
        Some(std::path::PathBuf::from(&path_str[..app_bundle_end]))
    } else {
        None
    }
}
