//! 「备份与恢复」页的备份占用总览：列出 CC Switch 在本机留下的各类备份、
//! 它们的位置与大小，由用户自己决定删不删。
//!
//! 安全边界：前端只能传类别 id，路径一律由这里按写入方的口径算出，绝不接收
//! 前端给的路径；删除也只删该类别自己的目录或按文件名匹配到的文件。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::codex_history_migration;
use crate::config::{get_app_config_dir, get_home_dir};
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::DeviceStore;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupLocation {
    /// 类别 id，前端据此取文案；删除 / 显示也只认它。
    pub id: String,
    /// 在文件管理器里显示的位置（目录）。
    pub path: String,
    pub size_bytes: u64,
    /// 条目数：整目录类别是顶层条目数（如迁移代数、Skill 备份个数），
    /// 按文件匹配的类别是匹配到的文件数。
    pub item_count: u64,
    pub last_modified: Option<String>,
    /// 数据库备份在上方列表逐个管理，这里不提供整类删除。
    pub deletable: bool,
}

#[derive(Clone, Copy)]
enum Scope {
    /// 整个目录都是这一类备份。
    WholeDir,
    /// 目录下文件名匹配的文件。
    Files(fn(&str) -> bool),
}

struct Category {
    id: &'static str,
    dir: PathBuf,
    scope: Scope,
    deletable: bool,
}

fn categories() -> Vec<Category> {
    let app_dir = get_app_config_dir();
    let app_backups = app_dir.join("backups");
    // 写入引擎与旧版接管备份按设备目录落盘（不受 app_config_dir 覆盖影响）。
    let device_backups = get_home_dir().join(".cc-switch").join("backups");
    let env_backups =
        crate::services::env_manager::backup_dir().unwrap_or_else(|_| device_backups.clone());

    let whole = |id, dir| Category {
        id,
        dir,
        scope: Scope::WholeDir,
        deletable: true,
    };
    let files = |id, dir, matcher| Category {
        id,
        dir,
        scope: Scope::Files(matcher),
        deletable: true,
    };

    vec![
        Category {
            id: "database",
            dir: app_backups.clone(),
            scope: Scope::Files(|name| name.ends_with(".db")),
            deletable: false,
        },
        whole("skills", app_dir.join("skill-backups")),
        whole(
            "codexHistoryUnify",
            app_backups.join(codex_history_migration::OFFICIAL_UNIFY_MIGRATION_NAME),
        ),
        whole(
            "codexHistoryUnifyRestore",
            app_backups.join(codex_history_migration::OFFICIAL_UNIFY_RESTORE_BACKUP_NAME),
        ),
        whole(
            "codexHistoryProviderMigration",
            app_backups.join(codex_history_migration::MIGRATION_NAME),
        ),
        whole(
            "liveFirstWrite",
            DeviceStore::for_device().first_write_backup_dir(),
        ),
        whole("hermes", app_backups.join("hermes")),
        whole("openclaw", app_backups.join("openclaw")),
        whole("proxyLiveBackup", device_backups.join("proxy-live-backup")),
        files("envVars", env_backups, |name| {
            name.starts_with("env-backup-") && name.ends_with(".json")
        }),
        // 以下为旧版本遗留，当前版本不再读写。
        files("legacyConfigBackups", app_backups, |name| {
            name.starts_with("backup_") && name.ends_with(".json")
        }),
        files("legacyConfigArchive", app_dir.clone(), |name| {
            name == "config.json.migrated"
        }),
        files("legacyDbUpgrade", app_dir.clone(), |name| {
            name.starts_with("cc-switch.db.") && name.contains("-backup-")
        }),
        whole("legacyToolSearch", app_dir.join("toolsearch-backups")),
    ]
}

#[derive(Default)]
struct Usage {
    size: u64,
    items: u64,
    latest: Option<SystemTime>,
}

impl Usage {
    fn add_file(&mut self, meta: &fs::Metadata) {
        self.size += meta.len();
        if let Ok(modified) = meta.modified() {
            self.latest = Some(self.latest.map_or(modified, |t| t.max(modified)));
        }
    }
}

/// 递归累加目录大小；不跟随符号链接，避免把链接目标算进来。
fn walk(path: &Path, usage: &mut Usage) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                walk(&entry.path(), usage);
            }
        }
    } else {
        usage.add_file(&meta);
    }
}

fn matched_files(dir: &Path, matcher: fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|entry| matcher(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect()
}

fn measure(category: &Category) -> Usage {
    let mut usage = Usage::default();
    match category.scope {
        Scope::WholeDir => {
            if let Ok(entries) = fs::read_dir(&category.dir) {
                for entry in entries.flatten() {
                    if entry.file_name() == ".DS_Store" {
                        continue;
                    }
                    usage.items += 1;
                    walk(&entry.path(), &mut usage);
                }
            }
        }
        Scope::Files(matcher) => {
            for file in matched_files(&category.dir, matcher) {
                usage.items += 1;
                walk(&file, &mut usage);
            }
        }
    }
    usage
}

/// 列出本机存在的备份位置（没有内容的类别不返回，数据库备份除外）。
pub fn list_locations() -> Vec<BackupLocation> {
    categories()
        .into_iter()
        .filter_map(|category| {
            let usage = if category.id == "database" {
                database_usage()
            } else {
                measure(&category)
            };
            if usage.items == 0 && category.id != "database" {
                return None;
            }
            Some(BackupLocation {
                id: category.id.to_string(),
                path: category.dir.to_string_lossy().into_owned(),
                size_bytes: usage.size,
                item_count: usage.items,
                last_modified: usage.latest.map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.to_rfc3339()
                }),
                deletable: category.deletable,
            })
        })
        .collect()
}

/// 数据库备份沿用上方列表的口径（拿备份文件锁），两处数字保持一致。
fn database_usage() -> Usage {
    let mut usage = Usage::default();
    if let Ok(entries) = Database::list_backups() {
        usage.items = entries.len() as u64;
        usage.size = entries.iter().map(|entry| entry.size_bytes).sum();
        usage.latest = entries
            .iter()
            .filter_map(|entry| chrono::DateTime::parse_from_rfc3339(&entry.created_at).ok())
            .max()
            .map(SystemTime::from);
    }
    usage
}

fn find(id: &str) -> Result<Category, AppError> {
    categories()
        .into_iter()
        .find(|category| category.id == id)
        .ok_or_else(|| AppError::InvalidInput(format!("未知的备份类别: {id}")))
}

/// 删除一类备份，返回释放的字节数。
pub fn delete_location(id: &str) -> Result<u64, AppError> {
    let category = find(id)?;
    if !category.deletable {
        return Err(AppError::InvalidInput(format!(
            "备份类别 {id} 不支持整体删除"
        )));
    }
    // 与写这些目录的操作串行：Codex 迁移 / 还原正在写备份时不能抽掉目录；
    // Skill 卸载、恢复同样持有 Skills 状态锁。
    let _codex_guard = id
        .starts_with("codex")
        .then(codex_history_migration::lock_history_op_for_backup_cleanup);
    let _skill_guard = (id == "skills").then(crate::services::skill::skill_state_write_guard);

    let freed = measure(&category).size;
    match category.scope {
        Scope::WholeDir => {
            if category.dir.exists() {
                fs::remove_dir_all(&category.dir).map_err(|e| AppError::io(&category.dir, e))?;
            }
        }
        Scope::Files(matcher) => {
            for file in matched_files(&category.dir, matcher) {
                fs::remove_file(&file).map_err(|e| AppError::io(&file, e))?;
            }
        }
    }
    log::info!(
        "已删除备份类别 {id}（{}），释放 {freed} 字节",
        category.dir.display()
    );
    Ok(freed)
}

/// 在文件管理器中显示该类备份所在的目录。
pub fn location_dir(id: &str) -> Result<PathBuf, AppError> {
    let category = find(id)?;
    if !category.dir.exists() {
        return Err(AppError::InvalidInput(format!(
            "目录不存在: {}",
            category.dir.display()
        )));
    }
    Ok(category.dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_dir_measure_counts_top_level_entries_and_skips_ds_store() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("gen");
        fs::create_dir_all(dir.join("a/jsonl")).unwrap();
        fs::write(dir.join("a/jsonl/x.jsonl"), b"12345").unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        fs::write(dir.join("b/meta.json"), b"12").unwrap();
        fs::write(dir.join(".DS_Store"), b"ignored").unwrap();

        let usage = measure(&Category {
            id: "test",
            dir,
            scope: Scope::WholeDir,
            deletable: true,
        });
        assert_eq!(usage.items, 2);
        assert_eq!(usage.size, 7);
        assert!(usage.latest.is_some());
    }

    #[test]
    fn file_scope_only_matches_its_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("backup_20251011_023730.json"), b"abc").unwrap();
        fs::write(tmp.path().join("db_backup_20261005_203537.db"), b"db").unwrap();
        fs::create_dir_all(tmp.path().join("backup_dir.json")).unwrap();

        let matcher: fn(&str) -> bool =
            |name| name.starts_with("backup_") && name.ends_with(".json");
        let files = matched_files(tmp.path(), matcher);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("backup_20251011_023730.json"));
    }

    #[test]
    fn unknown_or_database_category_cannot_be_deleted() {
        assert!(delete_location("../etc").is_err());
        assert!(delete_location("database").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn walk_does_not_follow_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("big"), vec![0u8; 1024]).unwrap();
        let dir = tmp.path().join("gen");
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();

        let mut usage = Usage::default();
        walk(&dir, &mut usage);
        assert!(usage.size < 1024);
    }
}
