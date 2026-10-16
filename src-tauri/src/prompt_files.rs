use std::path::{Path, PathBuf};

use crate::app_config::AppType;
use crate::codex_config::get_codex_auth_path;
use crate::config::get_claude_settings_path;
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::openclaw_config::get_openclaw_dir;
use crate::opencode_config::get_opencode_dir;

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
        // Cursor 的用户级规则目录是 `~/.cursor/rules/*.mdc`（不是 `~/.cursor/` 下的
        // AGENTS.md —— 那个位置 Cursor 不读）。
        AppType::Cursor => crate::cursor_config::get_cursor_dir().join("rules"),
        AppType::Pi => crate::pi_config::get_pi_agent_dir()?,
        AppType::Mcode => crate::mcode_config::data_dir(),
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    let filename = match app {
        AppType::Claude => "CLAUDE.md",
        AppType::Codex => "AGENTS.md",
        AppType::Gemini => "GEMINI.md",
        AppType::GrokBuild | AppType::OpenCode | AppType::OpenClaw => "AGENTS.md",
        // 该目录里会有用户自己的多个 `.mdc`（如 force-tdd / code-is-ground-truth），
        // cc-switch 只写自己这一个，绝不碰别的。
        AppType::Cursor => "cc-switch.mdc",
        AppType::Hermes => "SOUL.md",
        AppType::Pi | AppType::Mcode => "AGENTS.md",
        AppType::ClaudeDesktop => unreachable!("handled above"),
    };

    Ok(base_dir.join(filename))
}

/// Cursor 规则文件缺少 frontmatter 时补上的默认头。
///
/// `.mdc` 的**应用方式由 frontmatter 决定**：`alwaysApply: true` = 总是注入。没有 frontmatter
/// 的规则不会被当作「总是生效」——只写正文等于写了也不生效。
const CURSOR_PROMPT_FRONTMATTER: &str =
    "---\ndescription: CC Switch 管理的 Cursor 用户规则\nalwaysApply: true\n---\n\n";

/// 真正写进提示词文件的正文。
///
/// 只有 Cursor 需要加工（补 [`CURSOR_PROMPT_FRONTMATTER`]）；其余应用原样写入。已经带
/// frontmatter（忽略前导空白）时不加工，避免重复添加、也避免覆盖用户自己写的 `globs` 等设置。
pub fn prompt_file_body(app: &AppType, content: &str) -> String {
    if !matches!(app, AppType::Cursor) || content.trim_start().starts_with("---") {
        return content.to_string();
    }
    format!("{CURSOR_PROMPT_FRONTMATTER}{content}")
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

    /// 回归：Cursor 的**用户级**规则在 `~/.cursor/rules/*.mdc`（见 cursor.com/docs/rules：
    /// User rule files in `~/.cursor/rules`），**不是** `~/.cursor/AGENTS.md` —— 后者 Cursor
    /// 根本不读（AGENTS.md 只在项目根/子目录生效）。该目录里会有用户自己的多个 `.mdc`，
    /// cc-switch 只能管自己那一个。
    #[test]
    fn cursor_prompt_file_lives_in_the_rules_dir() {
        let path = prompt_file_path(&AppType::Cursor).expect("Cursor prompt path");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("cc-switch.mdc"),
            "实际：{}",
            path.display()
        );
        assert_eq!(
            path.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str()),
            Some("rules"),
            "必须落在 ~/.cursor/rules 下，实际：{}",
            path.display()
        );
    }

    /// 回归：`.mdc` 的**应用方式由 frontmatter 决定**，没有 frontmatter 的规则不会按
    /// 「总是生效」应用 —— 只写正文等于写了也不生效（与刚踩的同类坑一致）。
    #[test]
    fn cursor_prompt_body_gets_a_default_frontmatter() {
        let body = prompt_file_body(&AppType::Cursor, "# 我的规则\n");

        assert!(body.starts_with("---\n"), "实际：{body}");
        assert!(body.contains("alwaysApply: true"), "实际：{body}");
        assert!(body.ends_with("# 我的规则\n"), "正文须原样保留：{body}");
    }

    #[test]
    fn cursor_prompt_body_keeps_an_existing_frontmatter() {
        let existing = "---\ndescription: x\nalwaysApply: false\n---\n\n# 正文\n";
        assert_eq!(prompt_file_body(&AppType::Cursor, existing), existing);

        // 前面有空白也算「已有 frontmatter」，不能重复添加
        let padded = "\n\n---\nalwaysApply: false\n---\n";
        assert_eq!(prompt_file_body(&AppType::Cursor, padded), padded);
    }

    #[test]
    fn other_apps_write_the_body_verbatim() {
        assert_eq!(prompt_file_body(&AppType::Claude, "# hi"), "# hi");
        assert_eq!(prompt_file_body(&AppType::Codex, "# hi"), "# hi");
    }
}
