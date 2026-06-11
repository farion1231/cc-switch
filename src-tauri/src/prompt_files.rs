use std::path::PathBuf;

use crate::app_config::AppType;
use crate::codex_config::get_codex_auth_path;
use crate::config::get_home_dir;
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::openclaw_config::get_openclaw_dir;
use crate::opencode_config::get_opencode_dir;

/// 返回指定应用所使用的提示词文件路径。
pub fn prompt_file_path(app: &AppType) -> Result<PathBuf, AppError> {
    if matches!(app, AppType::ClaudeDesktop) {
        return Err(AppError::localized(
            "claude_desktop.prompts_unsupported",
            "Claude Desktop 暂不支持 Prompts",
            "Claude Desktop does not support Prompts",
        ));
    }

    let base_dir: PathBuf = match app {
        AppType::Claude => crate::settings::get_claude_configured_override_dir()
            .unwrap_or_else(|| get_home_dir().join(".claude")),
        AppType::Codex => get_base_dir_with_fallback(get_codex_auth_path(), ".codex")?,
        AppType::Gemini => get_gemini_dir(),
        AppType::OpenCode => get_opencode_dir(),
        AppType::OpenClaw => get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    let filename = match app {
        AppType::Claude => "CLAUDE.md",
        AppType::Codex => "AGENTS.md",
        AppType::Gemini => "GEMINI.md",
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => "AGENTS.md",
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    Ok(base_dir.join(filename))
}

fn get_base_dir_with_fallback(
    primary_path: PathBuf,
    fallback_dir: &str,
) -> Result<PathBuf, AppError> {
    primary_path
        .parent()
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(fallback_dir)))
        .ok_or_else(|| {
            AppError::localized(
                "home_dir_not_found",
                format!("无法确定 {fallback_dir} 配置目录：用户主目录不存在"),
                format!("Cannot determine {fallback_dir} config directory: user home not found"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = crate::settings::update_settings(crate::settings::AppSettings::default());
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[test]
    #[serial]
    fn claude_prompt_path_ignores_provider_profile_override() {
        let home = TempHome::new();
        let profile_dir = home.path().join("external-profile");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_provider_config_dir: Some(profile_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set provider profile override");

        let path = prompt_file_path(&AppType::Claude).expect("prompt path");

        assert_eq!(path, home.path().join(".claude").join("CLAUDE.md"));
    }
}
