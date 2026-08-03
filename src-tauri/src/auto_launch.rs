use crate::error::AppError;

// ── Windows: 手动管理注册表，修复 auto-launch crate 不加引号的 bug ──────────
// auto-launch v0.5.0 写入 HKCU\...\Run 时没有给路径加双引号，
// 导致路径含空格的应用（如 "CC Switch"）开机无法自启。
// 此处直接使用 winreg 写入带引号的路径，并同步管理
// StartupApproved\Run 以保持与任务管理器"启动"标签页一致。

#[cfg(target_os = "windows")]
mod platform {
    use std::path::PathBuf;
    use winreg::enums::*;
    use winreg::RegKey;

    const AUTORUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    // 与 auto-launch crate 保持一致的 12 字节值，表示"已启用"
    const TASK_MANAGER_OVERRIDE_ENABLED_VALUE: [u8; 12] = [
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    /// 获取当前可执行文件路径
    fn exe_path() -> Result<PathBuf, AppError> {
        std::env::current_exe()
            .map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))
    }

    /// 去除路径首尾的双引号（兼容已被引号包裹的路径）
    fn strip_quotes(s: &str) -> &str {
        s.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(s)
    }

    pub fn enable(app_name: &str) -> Result<(), AppError> {
        let path = exe_path()?;
        let path_str = path.to_string_lossy();
        // 关键修复：用双引号包裹路径，处理路径中的空格
        let value = format!("\"{}\"", path_str);

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let auto_run = hkcu
            .create_subkey(AUTORUN_KEY)
            .map_err(|e| AppError::Message(format!("打开注册表失败: {e}")))?;
        auto_run
            .1
            .set_value(app_name, &value)
            .map_err(|e| AppError::Message(format!("写入注册表失败: {e}")))?;

        // 同步设置 StartupApproved\Run，使任务管理器显示为"已启用"
        if let Ok(approved) = hkcu.create_subkey(APPROVED_KEY) {
            let _ = approved
                .1
                .set_binary_value(app_name, &TASK_MANAGER_OVERRIDE_ENABLED_VALUE);
        }

        log::info!("已启用开机自启");
        Ok(())
    }

    pub fn disable(app_name: &str) -> Result<(), AppError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // 删除 Run 注册表项
        if let Ok(auto_run) = hkcu.open_subkey_with_flags(AUTORUN_KEY, KEY_WRITE) {
            let _ = auto_run.delete_value(app_name);
        }

        // 删除 StartupApproved\Run 注册表项
        if let Ok(approved) = hkcu.open_subkey_with_flags(APPROVED_KEY, KEY_WRITE) {
            let _ = approved.delete_value(app_name);
        }

        log::info!("已禁用开机自启");
        Ok(())
    }

    pub fn is_enabled(app_name: &str) -> Result<bool, AppError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let auto_run = hkcu
            .open_subkey(AUTORUN_KEY)
            .map_err(|e| AppError::Message(format!("打开注册表失败: {e}")))?;

        let stored: String = auto_run
            .get_value(app_name)
            .map_err(|e| AppError::Message(format!("读取注册表失败: {e}")))?;

        if stored.is_empty() {
            return Ok(false);
        }

        let current = exe_path()?;
        let current_str = current.to_string_lossy();

        // 对比时去除引号，兼容旧版无引号的注册表值
        Ok(strip_quotes(&stored) == current_str.as_ref())
    }
}

// ── macOS / Linux: 继续使用 auto-launch crate ───────────────────────────────

#[cfg(not(target_os = "windows"))]
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// 获取 macOS 上的 .app bundle 路径
/// 将 `/path/to/CC Switch.app/Contents/MacOS/CC Switch` 转换为 `/path/to/CC Switch.app`
#[cfg(target_os = "macos")]
fn get_macos_app_bundle_path(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let path_str = exe_path.to_string_lossy();
    // 查找 .app/Contents/MacOS/ 模式
    if let Some(app_pos) = path_str.find(".app/Contents/MacOS/") {
        let app_bundle_end = app_pos + 4; // ".app" 的结束位置
        Some(std::path::PathBuf::from(&path_str[..app_bundle_end]))
    } else {
        None
    }
}

/// 初始化 AutoLaunch 实例（仅 macOS / Linux 使用）
#[cfg(not(target_os = "windows"))]
fn get_auto_launch() -> Result<AutoLaunch, AppError> {
    let app_name = "CC Switch";
    let exe_path = std::env::current_exe()
        .map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;

    // macOS 需要使用 .app bundle 路径，否则 AppleScript login item 会打开终端
    #[cfg(target_os = "macos")]
    let app_path = get_macos_app_bundle_path(&exe_path).unwrap_or(exe_path);

    #[cfg(not(target_os = "macos"))]
    let app_path = exe_path;

    // 使用 AutoLaunchBuilder 消除平台差异
    // macOS: 使用 AppleScript 方式（默认），需要 .app bundle 路径
    // Linux: 使用 XDG autostart
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(app_name)
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))?;

    Ok(auto_launch)
}

/// 启用开机自启
pub fn enable_auto_launch() -> Result<(), AppError> {
    let app_name = "CC Switch";

    #[cfg(target_os = "windows")]
    {
        platform::enable(app_name)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let auto_launch = get_auto_launch()?;
        auto_launch
            .enable()
            .map_err(|e| AppError::Message(format!("启用开机自启失败: {e}")))?;
        log::info!("已启用开机自启");
        Ok(())
    }
}

/// 禁用开机自启
pub fn disable_auto_launch() -> Result<(), AppError> {
    let app_name = "CC Switch";

    #[cfg(target_os = "windows")]
    {
        platform::disable(app_name)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let auto_launch = get_auto_launch()?;
        auto_launch
            .disable()
            .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
        log::info!("已禁用开机自启");
        Ok(())
    }
}

/// 检查是否已启用开机自启
pub fn is_auto_launch_enabled() -> Result<bool, AppError> {
    let app_name = "CC Switch";

    #[cfg(target_os = "windows")]
    {
        platform::is_enabled(app_name)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let auto_launch = get_auto_launch()?;
        auto_launch
            .is_enabled()
            .map_err(|e| AppError::Message(format!("检查开机自启状态失败: {e}")))
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_valid() {
        let exe_path =
            std::path::Path::new("/Applications/CC Switch.app/Contents/MacOS/CC Switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(
            result,
            Some(std::path::PathBuf::from("/Applications/CC Switch.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_with_spaces() {
        let exe_path =
            std::path::Path::new("/Users/test/My Apps/CC Switch.app/Contents/MacOS/CC Switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(
            result,
            Some(std::path::PathBuf::from(
                "/Users/test/My Apps/CC Switch.app"
            ))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_not_in_bundle() {
        let exe_path = std::path::Path::new("/usr/local/bin/cc-switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(result, None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_dev_build() {
        // 开发环境下的路径通常不在 .app bundle 内
        let exe_path = std::path::Path::new("/Users/dev/project/target/debug/cc-switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(result, None);
    }
}
