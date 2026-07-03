//! Claude Cowork（桌面版）会话日志使用追踪
//!
//! Cowork 会话不写入 ~/.claude/projects/，而是保存在 Claude 桌面应用的
//! 数据目录下。每个会话沙箱内嵌一份与 Claude Code 完全同构的 transcript：
//!
//! ```text
//! <Claude 桌面数据目录>/local-agent-mode-sessions/
//!   <workspace-id>/<group-id>/
//!     local_<id>.json          ← 会话元数据（标题等）
//!     local_<id>/
//!       audit.jsonl            ← 审计副本，见下方说明，必须排除
//!       .claude/projects/<项目目录>/
//!         <sessionId>.jsonl    ← 真正的 transcript，格式与 Claude Code 一致
//!         <sessionId>/subagents/agent-*.jsonl   ← Task 子代理（与 Claude Code 布局一致）
//!     agent/
//!       local_<id>/            ← group 级常驻代理沙箱（如记忆维护，Windows 实测），
//!                                内部结构同 local_<id>/，其消耗同样真实计费
//! ```
//!
//! JSONL 记录格式（`type=="assistant"` + `message.usage`）与 ~/.claude/projects
//! 完全一致，解析、去重、增量同步全部复用 session_usage；本模块只负责发现
//! 各会话沙箱内嵌的 projects 目录。
//!
//! ## 为什么不扫 audit.jsonl
//!
//! `local_<id>/audit.jsonl` 是同一会话的审计副本，但其 usage 是流式起始
//! 的部分快照（实测某会话实际 output 327K token，audit 中只有 10K），
//! 计入会导致重复且错误的统计。实测它位于 `.claude/projects` 之外，扫内嵌
//! projects 目录本就不会命中；收集时仍按文件名显式排除，不依赖这个布局巧合。

use crate::config::get_home_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::services::session_usage::{collect_jsonl_files, sync_single_file, SessionSyncResult};
use std::fs;
use std::path::{Path, PathBuf};

/// Cowork 会话写入 proxy_request_logs.data_source 的标记
pub const COWORK_DATA_SOURCE: &str = "cowork_session_log";

// 注：macOS/Windows 布局均已真机实测；Linux 路径按 Electron appData 约定
// （Claude Desktop Linux beta 2026-06-30 发布，含 Cowork），目录不存在时同步为 no-op。

/// Cowork 会话根目录
///
/// Claude 桌面版是 Electron 应用，会话目录挂在 userData（appData/Claude）下：
///   macOS:   ~/Library/Application Support/Claude/
///   Windows: %APPDATA%\Claude\（企业漫游配置可能重定向到 home 之外，优先读环境变量）
///   Linux:   $XDG_CONFIG_HOME/Claude/ 或 ~/.config/Claude/（Linux beta 自 2026-06 起支持 Cowork）
///
/// 设置 CC_SWITCH_TEST_HOME 时忽略 APPDATA/XDG_CONFIG_HOME，一律从测试 home
/// 拼固定子路径，避免测试触碰真实用户目录。
fn cowork_sessions_dir() -> Option<PathBuf> {
    let isolated = std::env::var("CC_SWITCH_TEST_HOME")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let home = get_home_dir();

    let app_data = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else if cfg!(target_os = "windows") {
        dir_from_env(std::env::var_os("APPDATA"), isolated)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
    } else if cfg!(target_os = "linux") {
        dir_from_env(std::env::var_os("XDG_CONFIG_HOME"), isolated)
            .unwrap_or_else(|| home.join(".config"))
    } else {
        return None;
    };

    Some(app_data.join("Claude").join("local-agent-mode-sessions"))
}

/// 环境变量目录值 → PathBuf；测试隔离模式或值为空时返回 None（走 home 回退）。
fn dir_from_env(value: Option<std::ffi::OsString>, isolated: bool) -> Option<PathBuf> {
    if isolated {
        return None;
    }
    let value = value?;
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

/// 同步 Cowork 会话日志到使用统计数据库
pub fn sync_cowork_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: vec![],
    };

    let root = match cowork_sessions_dir() {
        Some(dir) if dir.exists() => dir,
        _ => return Ok(result),
    };

    for file_path in collect_cowork_jsonl_files(&root) {
        result.files_scanned += 1;

        match sync_single_file(db, &file_path, COWORK_DATA_SOURCE) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("{}: {e}", file_path.display());
                log::warn!("[COWORK-SYNC] 文件解析失败: {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[COWORK-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Cowork 会话沙箱内嵌 projects 目录下的 .jsonl 文件
///
/// 与 session_usage 一样按固定深度遍历，不递归：
///   root/<workspace-id>/<group-id>/local_<id>/.claude/projects/          (会话沙箱)
///   root/<workspace-id>/<group-id>/agent/local_<id>/.claude/projects/    (group 级常驻代理沙箱，Windows 实测)
/// 命中的 projects 目录交给 collect_jsonl_files 复用收集，主会话与
/// <sessionId>/subagents/ 下的 Task 子代理都在其中（布局与 Claude Code 一致）。
fn collect_cowork_jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for workspace in read_child_dirs(root) {
        for group in read_child_dirs(&workspace) {
            for entry in read_child_dirs(&group) {
                match entry.file_name().and_then(|n| n.to_str()) {
                    Some(name) if name.starts_with("local_") => {
                        push_sandbox_jsonl_files(&entry, &mut files);
                    }
                    // Windows 版在 group 级 agent/ 下有常驻代理沙箱（如 local_ditto_<id>/），
                    // 其消耗同样真实计费，一并计入
                    Some("agent") => {
                        for agent_sandbox in read_child_dirs(&entry) {
                            let is_sandbox = agent_sandbox
                                .file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with("local_"));
                            if is_sandbox {
                                push_sandbox_jsonl_files(&agent_sandbox, &mut files);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    files
}

/// 收集单个会话沙箱内嵌 projects 目录下的 .jsonl 文件。
fn push_sandbox_jsonl_files(sandbox: &Path, files: &mut Vec<PathBuf>) {
    let projects = sandbox.join(".claude").join("projects");
    if projects.is_dir() {
        files.extend(
            collect_jsonl_files(&projects)
                .into_iter()
                // audit.jsonl 无论在哪一层都不计入，见模块注释
                .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("audit.jsonl")),
        );
    }
}

/// 返回 `dir` 下直接子层的所有目录（不递归）。
fn read_child_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;

    /// 构造一个仿真 Cowork 会话根目录，返回 (root, transcript 目录)
    fn make_cowork_layout(tag: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("cc-switch-cowork-{tag}-{}", uuid::Uuid::new_v4()));
        let sandbox = root
            .join("workspace-uuid")
            .join("group-uuid")
            .join("local_abc123");
        let projects = sandbox
            .join(".claude")
            .join("projects")
            .join("-sandbox-outputs");
        fs::create_dir_all(&projects).unwrap();
        (root, projects)
    }

    #[test]
    fn test_dir_from_env_resolution() {
        use std::ffi::OsString;

        // 正常模式：非空环境变量生效（企业重定向的 APPDATA / 自定义 XDG_CONFIG_HOME）
        assert_eq!(
            dir_from_env(Some(OsString::from("/redirected/roaming")), false),
            Some(PathBuf::from("/redirected/roaming"))
        );
        // 空值视为未设置 → 走 home 回退
        assert_eq!(dir_from_env(Some(OsString::new()), false), None);
        assert_eq!(dir_from_env(None, false), None);
        // CC_SWITCH_TEST_HOME 隔离模式：忽略真实环境变量，测试不触碰真实用户目录
        assert_eq!(
            dir_from_env(Some(OsString::from("/redirected/roaming")), true),
            None
        );
    }

    #[test]
    fn test_cowork_sessions_dir_shape() {
        // 三个受支持平台上都应返回 …/Claude/local-agent-mode-sessions
        let dir = cowork_sessions_dir().expect("当前平台应支持 Cowork 目录解析");
        assert_eq!(
            dir.file_name().and_then(|n| n.to_str()),
            Some("local-agent-mode-sessions")
        );
        assert_eq!(
            dir.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("Claude")
        );
    }

    #[test]
    fn test_collect_excludes_audit_jsonl() {
        let (root, projects) = make_cowork_layout("collect");
        let sandbox = root
            .join("workspace-uuid")
            .join("group-uuid")
            .join("local_abc123");

        // 真正的 transcript + 子 agent
        let subagents = projects.join("session-1").join("subagents");
        fs::create_dir_all(&subagents).unwrap();
        fs::write(projects.join("session-1.jsonl"), "{}").unwrap();
        fs::write(subagents.join("agent-x.jsonl"), "{}").unwrap();
        // audit.jsonl 放三个位置：实测位置（沙箱直下）+ 两个可扫描到的防御位置
        fs::write(sandbox.join("audit.jsonl"), "{}").unwrap();
        fs::write(projects.join("audit.jsonl"), "{}").unwrap();
        fs::write(
            projects
                .join("session-1")
                .join("subagents")
                .join("audit.jsonl"),
            "{}",
        )
        .unwrap();
        // 非 local_ 前缀目录不是会话沙箱
        let other = root
            .join("workspace-uuid")
            .join("group-uuid")
            .join("skills");
        fs::create_dir_all(other.join(".claude").join("projects")).unwrap();
        fs::write(
            other.join(".claude").join("projects").join("stray.jsonl"),
            "{}",
        )
        .unwrap();
        // Windows 版 group 级常驻代理沙箱：group/agent/local_ditto_<id>/
        let ditto = root
            .join("workspace-uuid")
            .join("group-uuid")
            .join("agent")
            .join("local_ditto_xyz")
            .join(".claude")
            .join("projects")
            .join("-ditto-outputs");
        fs::create_dir_all(&ditto).unwrap();
        fs::write(ditto.join("ditto-session.jsonl"), "{}").unwrap();

        let files = collect_cowork_jsonl_files(&root);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        assert_eq!(files.len(), 3, "只应收集沙箱内嵌 projects 下的 transcript");
        assert!(paths.iter().any(|p| p.contains("session-1.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-x.jsonl")));
        assert!(
            paths.iter().any(|p| p.contains("ditto-session.jsonl")),
            "Windows 的 agent/ 下常驻代理沙箱必须被收集"
        );
        assert!(
            !paths.iter().any(|p| p.contains("audit.jsonl")),
            "audit.jsonl 的 usage 是流式部分快照，绝不能计入"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn test_sync_marks_cowork_data_source() -> Result<(), AppError> {
        let db = Database::memory()?;
        let (root, projects) = make_cowork_layout("sync");

        let line = r#"{"type":"assistant","message":{"id":"msg_cowork1","model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":200,"cache_read_input_tokens":5000,"cache_creation_input_tokens":100},"stop_reason":"end_turn"},"timestamp":"2026-06-27T11:17:00Z","sessionId":"cowork-session-1"}"#;
        fs::write(projects.join("session-1.jsonl"), format!("{line}\n")).unwrap();

        let mut imported = 0;
        for file in collect_cowork_jsonl_files(&root) {
            let (i, _) = sync_single_file(&db, &file, COWORK_DATA_SOURCE)?;
            imported += i;
        }
        assert_eq!(imported, 1);

        let conn = lock_conn!(db.conn);
        let (data_source, app_type): (String, String) = conn.query_row(
            "SELECT data_source, app_type FROM proxy_request_logs WHERE request_id = 'session:msg_cowork1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(data_source, COWORK_DATA_SOURCE);
        assert_eq!(app_type, "claude");
        drop(conn);

        fs::remove_dir_all(&root).ok();
        Ok(())
    }
}
