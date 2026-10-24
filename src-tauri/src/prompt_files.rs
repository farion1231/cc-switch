use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::app_config::AppType;
use crate::codex_config::get_codex_auth_path;
use crate::config::get_claude_settings_path;
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::openclaw_config::get_openclaw_dir;
use crate::opencode_config::get_opencode_dir;

// Device-owned policy: deliberately separate from the synced database and
// general settings saves. A stale settings form or restored snapshot cannot
// silently opt this machine back into prompt-file management.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PromptFileProtection {
    claude: bool,
    codex: bool,
    // Releasing protection does not give the saved template ownership of the
    // independently maintained file. Only a successful explicit write does.
    claude_detached: bool,
    codex_detached: bool,
}

static PROMPT_FILE_POLICY_LOCK: Mutex<()> = Mutex::new(());

fn protection_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join("prompt-file-protection.json")
}

// NotFound can mean an existing file or ancestor is a dangling symlink. Only
// accept absence once its nearest existing ancestor resolves to a directory.
fn confirm_missing(path: &Path, original: std::io::Error) -> Result<(), AppError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| AppError::io(path, error))?
            .join(path)
    };
    match std::fs::symlink_metadata(&absolute) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            for ancestor in absolute.ancestors().skip(1) {
                match std::fs::symlink_metadata(ancestor) {
                    Ok(_) => {
                        // create_dir_all can make missing/../existing start
                        // resolving to an existing file. Do not call that an
                        // empty target or normalize through an unresolved link.
                        let suffix = absolute.strip_prefix(ancestor).expect("path ancestor");
                        if suffix
                            .components()
                            .any(|part| !matches!(part, std::path::Component::Normal(_)))
                        {
                            return Err(AppError::io(path, original));
                        }
                        let metadata = std::fs::metadata(ancestor)
                            .map_err(|error| AppError::io(ancestor, error))?;
                        return if metadata.is_dir() {
                            Ok(())
                        } else {
                            Err(AppError::io(path, original))
                        };
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(AppError::io(ancestor, error)),
                }
            }
            Err(AppError::io(path, original))
        }
        Ok(_) => Err(AppError::io(path, original)),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn read_protection() -> Result<PromptFileProtection, AppError> {
    let path = protection_path();
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| AppError::json(&path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            confirm_missing(&path, error)?;
            Ok(Default::default())
        }
        Err(error) => Err(AppError::io(&path, error)),
    }
}

pub(crate) fn read_independent_prompt(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            confirm_missing(path, error)?;
            Ok(None)
        }
        Err(error) => Err(AppError::io(path, error)),
    }
}

pub(crate) struct PromptFileAccess {
    _guard: Option<MutexGuard<'static, ()>>,
    pub protected: bool,
    pub detached: bool,
}

impl PromptFileAccess {
    /// Hold this through the write. Enabling protection waits for existing
    /// writes to finish and prevents subsequent writes, including restore.
    pub fn acquire(app: &AppType) -> Result<Self, AppError> {
        if !matches!(app, AppType::Claude | AppType::Codex) {
            return Ok(Self {
                _guard: None,
                protected: false,
                detached: false,
            });
        }
        let guard = PROMPT_FILE_POLICY_LOCK
            .lock()
            .map_err(|error| AppError::Message(error.to_string()))?;
        let protection = read_protection()?;
        let protected = match app {
            AppType::Claude => protection.claude,
            AppType::Codex => protection.codex,
            _ => unreachable!(),
        };
        let detached = protected
            || match app {
                AppType::Claude => protection.claude_detached,
                AppType::Codex => protection.codex_detached,
                _ => unreachable!(),
            };
        Ok(Self {
            _guard: Some(guard),
            protected,
            detached,
        })
    }

    pub fn require_write(&self) -> Result<(), AppError> {
        if self.protected {
            return Err(AppError::localized(
                "prompts.file_protected",
                "本机规则文件保护已开启，请先在提示词页面关闭保护。",
                "Local rule file protection is on. Turn it off on the Prompts page before making changes.",
            ));
        }
        Ok(())
    }

    /// Call only after the requested file and template writes succeed. On a
    /// policy-write error the caller reports failure and the file stays detached,
    /// so a later read cannot silently backfill independently maintained text.
    pub fn record_managed_write(&self, app: &AppType) -> Result<(), AppError> {
        if !self.detached {
            return Ok(());
        }
        let mut policy = read_protection()?;
        match app {
            AppType::Claude => policy.claude_detached = false,
            AppType::Codex => policy.codex_detached = false,
            _ => return Ok(()),
        }
        crate::config::write_json_file(&protection_path(), &policy)
    }
}

pub fn get_prompt_file_protection(app: &AppType) -> Result<bool, AppError> {
    Ok(PromptFileAccess::acquire(app)?.protected)
}

pub fn set_prompt_file_protection(app: &AppType, protected: bool) -> Result<(), AppError> {
    if !matches!(app, AppType::Claude | AppType::Codex) {
        return Err(AppError::InvalidInput(
            "Prompt file protection supports Claude Code and Codex".into(),
        ));
    }
    let _guard = PROMPT_FILE_POLICY_LOCK
        .lock()
        .map_err(|error| AppError::Message(error.to_string()))?;
    let mut protection = read_protection()?;
    match app {
        AppType::Claude => {
            protection.claude_detached |= protection.claude || protected;
            protection.claude = protected;
        }
        AppType::Codex => {
            protection.codex_detached |= protection.codex || protected;
            protection.codex = protected;
        }
        _ => unreachable!(),
    }
    crate::config::write_json_file(&protection_path(), &protection)
}

pub(crate) fn validate_prompt_content(app: &AppType, content: &str) -> Result<(), AppError> {
    if matches!(app, AppType::Mcode) && content.len() > 32 * 1024 {
        return Err(AppError::InvalidInput(
            "MCode global instructions must not exceed 32 KiB".into(),
        ));
    }
    Ok(())
}

/// 返回指定应用所使用的提示词文件路径。
pub fn prompt_file_path(app: &AppType) -> Result<PathBuf, AppError> {
    if matches!(app, AppType::ClaudeDesktop) {
        return Err(AppError::localized(
            "app.prompts_unsupported",
            "当前应用暂不支持 Prompts",
            "This app does not support Prompts",
        ));
    }

    let base_dir: PathBuf = match app {
        AppType::Claude => get_base_dir_with_fallback(get_claude_settings_path(), ".claude")?,
        AppType::Codex => get_base_dir_with_fallback(get_codex_auth_path(), ".codex")?,
        AppType::Gemini => get_gemini_dir(),
        AppType::GrokBuild => crate::grok_config::get_grok_config_dir(),
        AppType::OpenCode => get_opencode_dir(),
        AppType::OpenClaw => get_openclaw_dir(),
        AppType::Hermes => crate::hermes_config::get_hermes_dir(),
        AppType::Pi => crate::pi_config::get_pi_agent_dir()?,
        AppType::Mcode => crate::mcode_config::data_dir(),
        AppType::Kimi => crate::kimi_config::data_dir(),
        // agy 只从各「定制根目录」读独立的 AGENTS.md；全局那一个是 ~/.gemini/config，Key 模式的
        // 私有 --gemini_dir 也经软链共享它。别用 ~/.gemini/GEMINI.md：那是 Gemini CLI 的。
        AppType::Agy => crate::agy_config::customizations_dir(),
        // omp 只读按 profile 切换的 agent 目录里的 AGENTS.md（上游 discovery/builtin.ts）
        AppType::Omp => crate::omp_config::get_omp_agent_dir()?,
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    let filename = match app {
        AppType::Claude => "CLAUDE.md",
        AppType::Codex => "AGENTS.md",
        AppType::Gemini => "GEMINI.md",
        AppType::GrokBuild | AppType::OpenCode | AppType::OpenClaw => "AGENTS.md",
        AppType::Hermes => "SOUL.md",
        AppType::Pi | AppType::Mcode | AppType::Agy | AppType::Kimi | AppType::Omp => "AGENTS.md",
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    Ok(base_dir.join(filename))
}

/// 把用户主目录前缀换成 `~`，给界面显示用（复制路径仍用完整路径）。
pub fn display_path(path: &Path) -> String {
    display_path_with_home(path, &crate::config::get_home_dir())
}

fn display_path_with_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        Err(_) => path.display().to_string(),
    }
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

    #[test]
    fn mcode_instructions_limit_counts_utf8_bytes() {
        assert!(validate_prompt_content(&AppType::Mcode, &"a".repeat(32768)).is_ok());
        assert!(validate_prompt_content(&AppType::Mcode, &"a".repeat(32769)).is_err());
        assert!(validate_prompt_content(&AppType::Mcode, &"中".repeat(10923)).is_err());
        assert!(validate_prompt_content(&AppType::OpenCode, &"中".repeat(10923)).is_ok());
    }

    #[test]
    fn display_path_shortens_the_home_prefix_only() {
        let home = Path::new("/Users/me");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            display_path_with_home(&home.join(".hermes").join("SOUL.md"), home),
            format!("~{sep}.hermes{sep}SOUL.md")
        );
        assert_eq!(display_path_with_home(home, home), "~");
        assert_eq!(
            display_path_with_home(Path::new("/opt/hermes/SOUL.md"), home),
            "/opt/hermes/SOUL.md"
        );
        // 只认整段目录前缀，不把 /Users/meow 当成 /Users/me 下面
        assert_eq!(
            display_path_with_home(Path::new("/Users/meow/SOUL.md"), home),
            "/Users/meow/SOUL.md"
        );
    }

    #[test]
    fn hermes_prompt_file_uses_soul_md() {
        let path = prompt_file_path(&AppType::Hermes).expect("Hermes prompt path");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("SOUL.md")
        );
    }

    #[test]
    fn pi_prompt_file_uses_agents_md() {
        let path = prompt_file_path(&AppType::Pi).expect("Pi prompt path");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("AGENTS.md")
        );
    }
}
