use std::path::{Path, PathBuf};

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

use crate::error::AppError;

const APP_NAME: &str = "CC Switch";
const TEST_STATE_FILE_ENV: &str = "CC_SWITCH_TEST_AUTO_LAUNCH_STATE_FILE";
const TEST_CURRENT_EXE_ENV: &str = "CC_SWITCH_TEST_CURRENT_EXE";

pub struct AutoLaunchService;

impl AutoLaunchService {
    pub fn enable() -> Result<(), AppError> {
        if let Some(path) = test_state_file_path() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| AppError::Io {
                    path: parent.display().to_string(),
                    source,
                })?;
            }
            std::fs::write(&path, b"1").map_err(|source| AppError::Io {
                path: path.display().to_string(),
                source,
            })?;
            return Ok(());
        }

        auto_launch()?
            .enable()
            .map_err(|e| AppError::Message(format!("启用开机自启失败: {e}")))?;
        Ok(())
    }

    pub fn disable() -> Result<(), AppError> {
        if let Some(path) = test_state_file_path() {
            if path.exists() {
                std::fs::remove_file(&path).map_err(|source| AppError::Io {
                    path: path.display().to_string(),
                    source,
                })?;
            }
            return Ok(());
        }

        auto_launch()?
            .disable()
            .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
        Ok(())
    }

    pub fn is_enabled() -> Result<bool, AppError> {
        if let Some(path) = test_state_file_path() {
            return Ok(path.exists());
        }

        auto_launch()?
            .is_enabled()
            .map_err(|e| AppError::Message(format!("检查开机自启状态失败: {e}")))
    }
}

fn auto_launch() -> Result<AutoLaunch, AppError> {
    let exe_path = current_exe_path()?;

    #[cfg(target_os = "macos")]
    let app_path = macos_app_bundle_path(&exe_path).unwrap_or(exe_path);

    #[cfg(not(target_os = "macos"))]
    let app_path = exe_path;

    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))
}

fn current_exe_path() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os(TEST_CURRENT_EXE_ENV) {
        return Ok(PathBuf::from(path));
    }

    std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))
}

fn test_state_file_path() -> Option<PathBuf> {
    std::env::var_os(TEST_STATE_FILE_ENV).map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_path(exe_path: &Path) -> Option<PathBuf> {
    let path_str = exe_path.to_string_lossy();
    let app_pos = path_str.find(".app/Contents/MacOS/")?;
    let app_bundle_end = app_pos + 4;
    Some(PathBuf::from(&path_str[..app_bundle_end]))
}

#[cfg(test)]
mod tests {
    use super::AutoLaunchService;
    #[cfg(target_os = "macos")]
    use super::macos_app_bundle_path;
    use serial_test::serial;
    use tempfile::tempdir;

    const TEST_STATE_FILE_ENV: &str = "CC_SWITCH_TEST_AUTO_LAUNCH_STATE_FILE";

    #[test]
    #[serial]
    fn auto_launch_round_trip_uses_state_file_override() -> Result<(), crate::error::AppError> {
        let temp = tempdir().expect("tempdir");
        let state_file = temp.path().join("auto-launch.state");
        std::env::set_var(TEST_STATE_FILE_ENV, &state_file);

        assert!(!AutoLaunchService::is_enabled()?);
        AutoLaunchService::enable()?;
        assert!(AutoLaunchService::is_enabled()?);
        AutoLaunchService::disable()?;
        assert!(!AutoLaunchService::is_enabled()?);

        std::env::remove_var(TEST_STATE_FILE_ENV);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bundle_path_is_detected() {
        let exe_path = std::path::Path::new("/Applications/CC Switch.app/Contents/MacOS/CC Switch");
        assert_eq!(
            macos_app_bundle_path(exe_path),
            Some(std::path::PathBuf::from("/Applications/CC Switch.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bundle_path_returns_none_for_non_bundle() {
        let exe_path = std::path::Path::new("/usr/local/bin/cc-switch");
        assert_eq!(macos_app_bundle_path(exe_path), None);
    }
}
