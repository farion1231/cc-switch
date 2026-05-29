use crate::error::AppError;
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

/// 初始化 AutoLaunch 实例
fn get_auto_launch() -> Result<AutoLaunch, AppError> {
    let app_name = "CC Switch";
    let exe_path =
        std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;

    // macOS 需要使用 .app bundle 路径，否则 AppleScript login item 会打开终端
    #[cfg(target_os = "macos")]
    let app_path = get_macos_app_bundle_path(&exe_path).unwrap_or(exe_path);

    #[cfg(not(target_os = "macos"))]
    let app_path = exe_path;

    // 使用 AutoLaunchBuilder 消除平台差异
    // macOS: 使用 AppleScript 方式（默认），需要 .app bundle 路径
    // Windows/Linux: 使用注册表/XDG autostart
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(app_name)
        .set_app_path(&app_path.to_string_lossy())
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

/// 迁移由旧版本写入的 macOS 登录项。
///
/// PR #462 之前的构建版本将 `.app/Contents/MacOS/<exe>` 这一可执行文件路径
/// 直接注册为登录项；这条路径在 LaunchServices 看来是 "Unix Executable
/// File"，被打开时会附带一个终端窗口（用户能看到 cc-switch 的全部启动日志）。
/// 老版本注册时使用的显示名也是 `cc-switch`（小写连字符），而当前代码使用
/// `CC Switch`，因此 [`is_auto_launch_enabled`] 永远查不到这一条旧记录，
/// 它会一直驻留在系统登录项里。
///
/// 此函数在启动期调用一次：
/// 1. 通过 AppleScript 查找名为 `cc-switch` 或 `CC Switch`、且路径包含
///    `/Contents/MacOS/` 的登录项 —— 这种路径形态一定是错的；
/// 2. 删除命中的登录项；
/// 3. 若确实删除了任何条目，立即用正确的 `.app` bundle 路径重新启用开机
///    自启，保留用户原本的意图。
///
/// 在非 macOS 平台上是空操作。
#[cfg(target_os = "macos")]
pub fn migrate_legacy_macos_login_items() {
    use std::process::Command;

    // AppleScript 返回 "true" / "false" 表示是否发生了删除。
    // 同时检查两种可能存在的显示名（旧版用 "cc-switch"，新版用 "CC Switch"），
    // 这样即便当前版本曾经写错了一次也能被自愈。
    const SCRIPT: &str = r#"
        on cleanup(itemName)
            tell application "System Events"
                try
                    set li to login item itemName
                    if (path of li) contains "/Contents/MacOS/" then
                        delete li
                        return true
                    end if
                end try
                return false
            end tell
        end cleanup

        set didRemove to false
        if cleanup("cc-switch") then set didRemove to true
        if cleanup("CC Switch") then set didRemove to true
        return didRemove
    "#;

    let output = match Command::new("osascript").arg("-e").arg(SCRIPT).output() {
        Ok(o) => o,
        Err(e) => {
            log::warn!("Skipped legacy login item migration (osascript not runnable): {e}");
            return;
        }
    };

    if !output.status.success() {
        log::warn!(
            "Legacy login item migration script exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return;
    }

    let removed = String::from_utf8_lossy(&output.stdout).trim() == "true";
    if !removed {
        return;
    }

    log::info!(
        "✓ Removed legacy macOS login item with .app/Contents/MacOS/ path (would have caused a Terminal window on login)"
    );

    // 重新注册以保留用户的开机自启意图，这一次会走 .app bundle 路径。
    match enable_auto_launch() {
        Ok(()) => log::info!("✓ Re-registered auto-launch using the .app bundle path"),
        Err(e) => log::warn!("Failed to re-register auto-launch after legacy cleanup: {e}"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn migrate_legacy_macos_login_items() {
    // 仅 macOS 受此 bug 影响，其他平台无需处理。
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_valid() {
        let exe_path = std::path::Path::new("/Applications/CC Switch.app/Contents/MacOS/CC Switch");
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

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn test_migrate_legacy_login_items_is_noop_on_non_macos() {
        // Must compile and not panic on Windows / Linux. The function is a
        // no-op there and has no observable effect to assert on.
        super::migrate_legacy_macos_login_items();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_migrate_legacy_login_items_compiles_on_macos() {
        // We can't safely exercise the AppleScript path under `cargo test`
        // because it would touch the developer's actual login items.
        // This test exists purely to keep the function reachable from the
        // test target and surface any compile-time regressions.
        let _ = super::migrate_legacy_macos_login_items;
    }
}
