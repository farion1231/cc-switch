//! Claude Desktop 会话迁移。
//!
//! Claude Desktop 把 Code 会话按**账号**分目录存放，且没有索引文件——应用只是直接
//! 列出「当前登录账号」对应的目录：
//!
//! ```text
//! <ClaudeData>/claude-code-sessions/<账号UUID>/<组织UUID>/local_*.json
//! ```
//!
//! 每个 `local_*.json` 就是一次会话（自带 title、时间戳、cwd、对话轮次）。切换/重新
//! 登录到另一个账号时，应用读取的是另一个目录，于是之前账号的全部会话都「消失」了——
//! 其实它们只是躺在上一个账号的文件夹里。
//!
//! 本模块把**来源账号**目录下的 `local_*.json` 复制到**目标账号**目录，使会话在目标
//! 账号下也能显示。约束：
//! - 只复制会话文件（`local_*.json`），绝不碰凭据 / OAuth token / `config.json`；
//! - 非破坏性：来源目录保持不变，目标目录已有的同名文件不覆盖；
//! - 只搬**历史**，不搬**用量额度**——每个账号各自的 limit 不变。

use crate::claude_desktop_config::get_claude_desktop_data_dir;
use crate::error::AppError;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `claude-code-sessions` 子目录名。
const SESSIONS_DIR: &str = "claude-code-sessions";
/// 会话文件名前缀。
const SESSION_FILE_PREFIX: &str = "local_";
/// Claude Desktop 记录「当前登录账号」的配置文件。
const DESKTOP_CONFIG_FILE: &str = "config.json";
/// `config.json` 中「当前登录账号」字段。
const LAST_KNOWN_ACCOUNT_KEY: &str = "lastKnownAccountUuid";

/// 一个账号 / 组织下的会话分组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSessionAccount {
    /// 账号 UUID（`claude-code-sessions/<账号>/` 目录名）。
    pub account_uuid: String,
    /// 组织 UUID（`.../<账号>/<组织>/` 目录名）。
    pub org_uuid: String,
    /// 该目录下的会话（`local_*.json`）数量。
    pub session_count: usize,
    /// 是否为当前登录账号（来自 `config.json` 的 `lastKnownAccountUuid`）。
    pub is_current: bool,
}

/// 一次迁移的结果报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateReport {
    pub from_account: String,
    pub from_org: String,
    pub to_account: String,
    pub to_org: String,
    /// 来源目录的会话数。
    pub source_count: usize,
    /// 迁移前目标目录的会话数。
    pub dest_count_before: usize,
    /// 来源中目标尚不存在的会话数（`dry_run` 时即为「将新增」的数量）。
    pub pending: usize,
    /// 实际复制的会话数（`dry_run` 时为 0）。
    pub copied: usize,
    /// 迁移后目标目录的会话数。
    pub dest_count_after: usize,
    pub dry_run: bool,
}

fn io_ctx(context: impl Into<String>, source: std::io::Error) -> AppError {
    AppError::IoContext {
        context: context.into(),
        source,
    }
}

/// 判断是否为一份会话文件（`local_*.json`）。
fn is_session_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with(SESSION_FILE_PREFIX) && name.ends_with(".json")
}

/// 收集某个目录下所有会话文件的文件名（不含路径）。
fn session_file_names(dir: &Path) -> Result<BTreeSet<String>, AppError> {
    let mut names = BTreeSet::new();
    if !dir.is_dir() {
        return Ok(names);
    }
    let entries =
        std::fs::read_dir(dir).map_err(|e| io_ctx(format!("读取目录失败: {}", dir.display()), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_ctx(format!("遍历目录失败: {}", dir.display()), e))?;
        let path = entry.path();
        if is_session_file(&path) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                names.insert(name.to_string());
            }
        }
    }
    Ok(names)
}

/// 列出某个账号目录下的组织子目录名。
fn org_dirs(account_dir: &Path) -> Result<Vec<String>, AppError> {
    let mut orgs = Vec::new();
    if !account_dir.is_dir() {
        return Ok(orgs);
    }
    let entries = std::fs::read_dir(account_dir)
        .map_err(|e| io_ctx(format!("读取目录失败: {}", account_dir.display()), e))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| io_ctx(format!("遍历目录失败: {}", account_dir.display()), e))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            orgs.push(name.to_string());
        }
    }
    orgs.sort();
    Ok(orgs)
}

/// 读取当前登录账号 UUID（`<data>/config.json` -> `lastKnownAccountUuid`）。失败或缺失时返回 None。
fn read_current_account(data_dir: &Path) -> Option<String> {
    let path = data_dir.join(DESKTOP_CONFIG_FILE);
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get(LAST_KNOWN_ACCOUNT_KEY)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// 扫描 `root`（即 `claude-code-sessions` 目录）下的所有账号/组织分组。
fn scan_accounts_at(
    root: &Path,
    current_account: Option<&str>,
) -> Result<Vec<DesktopSessionAccount>, AppError> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut account_dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| io_ctx(format!("读取目录失败: {}", root.display()), e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    account_dirs.sort();

    for account_dir in account_dirs {
        let Some(account_uuid) = account_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let is_current = current_account == Some(account_uuid);
        for org_uuid in org_dirs(&account_dir)? {
            let count = session_file_names(&account_dir.join(&org_uuid))?.len();
            out.push(DesktopSessionAccount {
                account_uuid: account_uuid.to_string(),
                org_uuid,
                session_count: count,
                is_current,
            });
        }
    }
    Ok(out)
}

/// 解析 org：显式给定则原样返回；否则在唯一时自动选取，多个/为空时报错。
fn resolve_org(root: &Path, account: &str, org: Option<&str>, label: &str) -> Result<String, AppError> {
    let account_dir = root.join(account);
    if !account_dir.is_dir() {
        return Err(AppError::Config(format!(
            "{label}账号目录不存在：{account}"
        )));
    }
    if let Some(org) = org {
        if !account_dir.join(org).is_dir() {
            return Err(AppError::Config(format!(
                "{label}组织目录不存在：{account}/{org}"
            )));
        }
        return Ok(org.to_string());
    }
    let orgs = org_dirs(&account_dir)?;
    match orgs.len() {
        0 => Err(AppError::Config(format!(
            "{label}账号 {account} 下没有组织目录；请先用该账号打开一次 Claude Desktop"
        ))),
        1 => Ok(orgs.into_iter().next().unwrap()),
        _ => Err(AppError::Config(format!(
            "{label}账号 {account} 存在多个组织，请显式指定组织：{}",
            orgs.join(", ")
        ))),
    }
}

/// 在 `root` 下执行迁移（核心逻辑，供测试直接调用）。
fn migrate_at(
    root: &Path,
    from_account: &str,
    from_org: Option<&str>,
    to_account: &str,
    to_org: Option<&str>,
    dry_run: bool,
) -> Result<MigrateReport, AppError> {
    let from_org = resolve_org(root, from_account, from_org, "来源")?;
    let to_org = resolve_org(root, to_account, to_org, "目标")?;

    let src_dir = root.join(from_account).join(&from_org);
    let dst_dir = root.join(to_account).join(&to_org);

    if src_dir == dst_dir {
        return Err(AppError::Config(
            "来源与目标是同一目录，无需迁移".into(),
        ));
    }

    let src_names = session_file_names(&src_dir)?;
    if src_names.is_empty() {
        return Err(AppError::Config(format!(
            "来源 {from_account}/{from_org} 下没有会话文件"
        )));
    }
    let dst_before = session_file_names(&dst_dir)?;
    let pending: Vec<&String> = src_names.difference(&dst_before).collect();

    if dry_run {
        return Ok(MigrateReport {
            from_account: from_account.to_string(),
            from_org,
            to_account: to_account.to_string(),
            to_org,
            source_count: src_names.len(),
            dest_count_before: dst_before.len(),
            pending: pending.len(),
            copied: 0,
            dest_count_after: dst_before.len(),
            dry_run: true,
        });
    }

    std::fs::create_dir_all(&dst_dir)
        .map_err(|e| io_ctx(format!("创建目标目录失败: {}", dst_dir.display()), e))?;

    let mut copied = 0usize;
    for name in &pending {
        let src = src_dir.join(name);
        let dst = dst_dir.join(name);
        // 双重保险：绝不覆盖已存在的目标文件。
        if dst.exists() {
            continue;
        }
        std::fs::copy(&src, &dst)
            .map_err(|e| io_ctx(format!("复制会话失败: {}", src.display()), e))?;
        copied += 1;
    }

    // 完整性校验：来源的每个文件名此刻都应存在于目标。
    let dst_after = session_file_names(&dst_dir)?;
    let missing: Vec<&String> = src_names.difference(&dst_after).collect();
    if !missing.is_empty() {
        return Err(AppError::Config(format!(
            "迁移后仍有 {} 个会话未出现在目标目录，请重试",
            missing.len()
        )));
    }

    Ok(MigrateReport {
        from_account: from_account.to_string(),
        from_org,
        to_account: to_account.to_string(),
        to_org,
        source_count: src_names.len(),
        dest_count_before: dst_before.len(),
        pending: pending.len(),
        copied,
        dest_count_after: dst_after.len(),
        dry_run: false,
    })
}

/// 列出所有账号/组织分组（含当前登录标记）。供 command 调用。
pub fn list_accounts() -> Result<Vec<DesktopSessionAccount>, AppError> {
    let data_dir = get_claude_desktop_data_dir()?;
    let current = read_current_account(&data_dir);
    scan_accounts_at(&data_dir.join(SESSIONS_DIR), current.as_deref())
}

/// 迁移会话。`to_account` 默认为当前登录账号。供 command 调用。
pub fn migrate(
    from_account: &str,
    from_org: Option<&str>,
    to_account: Option<&str>,
    to_org: Option<&str>,
    dry_run: bool,
) -> Result<MigrateReport, AppError> {
    let data_dir = get_claude_desktop_data_dir()?;
    let root = data_dir.join(SESSIONS_DIR);
    let to_account = match to_account {
        Some(acc) => acc.to_string(),
        None => read_current_account(&data_dir).ok_or_else(|| {
            AppError::Config("无法确定当前登录账号，请显式指定目标账号".into())
        })?,
    };
    migrate_at(&root, from_account, from_org, &to_account, to_org, dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// 在 `root/<acct>/<org>/` 下写入若干 `local_<id>.json` 会话文件。
    fn seed(root: &Path, acct: &str, org: &str, ids: &[&str]) {
        let dir = root.join(acct).join(org);
        fs::create_dir_all(&dir).unwrap();
        for id in ids {
            fs::write(dir.join(format!("local_{id}.json")), b"{}").unwrap();
        }
    }

    fn names(root: &Path, acct: &str, org: &str) -> BTreeSet<String> {
        session_file_names(&root.join(acct).join(org)).unwrap()
    }

    #[test]
    fn scan_counts_sessions_and_marks_current() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "acctA", "orgA", &["1", "2", "3"]);
        seed(root, "acctB", "orgB", &["9"]);

        let mut got = scan_accounts_at(root, Some("acctB")).unwrap();
        got.sort_by(|a, b| a.account_uuid.cmp(&b.account_uuid));

        assert_eq!(got.len(), 2);
        assert_eq!(got[0].account_uuid, "acctA");
        assert_eq!(got[0].session_count, 3);
        assert!(!got[0].is_current);
        assert_eq!(got[1].account_uuid, "acctB");
        assert_eq!(got[1].session_count, 1);
        assert!(got[1].is_current);
    }

    #[test]
    fn scan_ignores_non_session_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "acctA", "orgA", &["1"]);
        // 干扰文件：非 local_ 前缀、非 .json，均不应计入。
        let dir = root.join("acctA").join("orgA");
        fs::write(dir.join(".DS_Store"), b"x").unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        fs::write(dir.join("session.json"), b"{}").unwrap();

        let got = scan_accounts_at(root, None).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].session_count, 1);
    }

    #[test]
    fn scan_missing_root_is_empty() {
        let tmp = TempDir::new().unwrap();
        let got = scan_accounts_at(&tmp.path().join("nope"), None).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn migrate_dry_run_copies_nothing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "old", "o1", &["1", "2", "3"]);
        seed(root, "new", "n1", &["3"]); // 已有 3，应视作已存在

        let report = migrate_at(root, "old", None, "new", None, true).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.source_count, 3);
        assert_eq!(report.dest_count_before, 1);
        assert_eq!(report.pending, 2); // 1、2 待迁移
        assert_eq!(report.copied, 0);
        // 目标未变。
        assert_eq!(names(root, "new", "n1").len(), 1);
    }

    #[test]
    fn migrate_copies_only_missing_and_never_overwrites() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "old", "o1", &["1", "2", "3"]);
        seed(root, "new", "n1", &["3"]);
        // 把目标里已存在的 3 写成不同内容，验证不被覆盖。
        fs::write(root.join("new").join("n1").join("local_3.json"), b"KEEP").unwrap();

        let report = migrate_at(root, "old", None, "new", None, false).unwrap();
        assert!(!report.dry_run);
        assert_eq!(report.pending, 2);
        assert_eq!(report.copied, 2);
        assert_eq!(report.dest_count_after, 3);

        // 目标现在有 1、2、3 全部。
        let dst = names(root, "new", "n1");
        assert!(dst.contains("local_1.json"));
        assert!(dst.contains("local_2.json"));
        assert!(dst.contains("local_3.json"));
        // 既有的 3 未被覆盖。
        let kept = fs::read(root.join("new").join("n1").join("local_3.json")).unwrap();
        assert_eq!(kept, b"KEEP");
        // 来源保持不变。
        assert_eq!(names(root, "old", "o1").len(), 3);
    }

    #[test]
    fn migrate_into_existing_empty_dest_org() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "old", "o1", &["1", "2"]);
        // 目标账号/组织已存在但为空（用户刚登录、还没有会话）。
        fs::create_dir_all(root.join("new").join("n1")).unwrap();

        let report = migrate_at(root, "old", None, "new", Some("n1"), false).unwrap();
        assert_eq!(report.copied, 2);
        assert_eq!(names(root, "new", "n1").len(), 2);
    }

    #[test]
    fn migrate_rejects_nonexistent_dest_org() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "old", "o1", &["1"]);
        fs::create_dir_all(root.join("new")).unwrap(); // 账号目录在，但无任何组织目录
        let err = migrate_at(root, "old", None, "new", None, false).unwrap_err();
        assert!(err.to_string().contains("没有组织目录"));
    }

    #[test]
    fn migrate_rejects_same_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "same", "o1", &["1"]);
        let err = migrate_at(root, "same", None, "same", None, false).unwrap_err();
        assert!(err.to_string().contains("同一目录"));
    }

    #[test]
    fn migrate_errors_on_ambiguous_org() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        seed(root, "multi", "o1", &["1"]);
        seed(root, "multi", "o2", &["2"]);
        seed(root, "dest", "d1", &["9"]);
        let err = migrate_at(root, "multi", None, "dest", None, false).unwrap_err();
        assert!(err.to_string().contains("多个组织"));
    }
}
