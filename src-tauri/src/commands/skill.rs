//! Skills 命令层
//!
//! v3.10.0+ 统一管理架构：
//! - 支持三应用开关（Claude/Codex/Gemini）
//! - SSOT 存储在 ~/.cc-switch/skills/

use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::error::format_skill_error;
use crate::services::skill::{DiscoverableSkill, Skill, SkillRepo, SkillService};
use crate::store::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

/// SkillService 状态包装
pub struct SkillServiceState(pub Arc<SkillService>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBatchFailure {
    pub key: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBatchInstallResult {
    pub installed: Vec<InstalledSkill>,
    pub failed: Vec<SkillBatchFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateStatus {
    pub id: String,
    pub state: String,
    pub latest: Option<DiscoverableSkill>,
}

fn build_skill_update_statuses(
    db: &Arc<crate::database::Database>,
    discoverable: Vec<DiscoverableSkill>,
) -> Result<Vec<SkillUpdateStatus>, String> {
    let latest_by_key = discoverable
        .into_iter()
        .map(|skill| (skill.key.to_lowercase(), skill))
        .collect::<std::collections::HashMap<_, _>>();

    let installed = SkillService::get_all_installed(db).map_err(|e| e.to_string())?;
    let mut updates = Vec::with_capacity(installed.len());

    for skill in installed {
        if let Some(latest) = latest_by_key.get(&skill.id.to_lowercase()) {
            let current_hash = skill
                .content_hash
                .clone()
                .or_else(|| SkillService::get_installed_content_hash(&skill.directory).ok());
            let state = match (&current_hash, &latest.content_hash) {
                (Some(current), Some(next)) if current != next => "update_available",
                (Some(_), Some(_)) => "up_to_date",
                _ => "unknown",
            };
            updates.push(SkillUpdateStatus {
                id: skill.id.clone(),
                state: state.to_string(),
                latest: Some(latest.clone()),
            });
        } else {
            updates.push(SkillUpdateStatus {
                id: skill.id.clone(),
                state: "not_found".to_string(),
                latest: None,
            });
        }
    }

    Ok(updates)
}

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, String> {
    match app.to_lowercase().as_str() {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        "gemini" => Ok(AppType::Gemini),
        "opencode" => Ok(AppType::OpenCode),
        _ => Err(format!("不支持的 app 类型: {app}")),
    }
}

// ========== 统一管理命令 ==========

/// 获取所有已安装的 Skills
#[tauri::command]
pub fn get_installed_skills(app_state: State<'_, AppState>) -> Result<Vec<InstalledSkill>, String> {
    SkillService::get_all_installed(&app_state.db).map_err(|e| e.to_string())
}

/// 安装 Skill（新版统一安装）
///
/// 参数：
/// - skill: 从发现列表获取的技能信息
/// - current_app: 当前选中的应用，安装后默认启用该应用
#[tauri::command]
pub async fn install_skill_unified(
    skill: DiscoverableSkill,
    current_app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_skills_unified_batch(
    skills: Vec<DiscoverableSkill>,
    current_app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<SkillBatchInstallResult, String> {
    let app_type = parse_app_type(&current_app)?;
    let mut installed = Vec::new();
    let mut failed = Vec::new();

    for skill in skills {
        match service.0.install(&app_state.db, &skill, &app_type).await {
            Ok(item) => installed.push(item),
            Err(err) => failed.push(SkillBatchFailure {
                key: skill.key,
                error: err.to_string(),
            }),
        }
    }

    Ok(SkillBatchInstallResult { installed, failed })
}

/// 卸载 Skill（新版统一卸载）
#[tauri::command]
pub fn uninstall_skill_unified(id: String, app_state: State<'_, AppState>) -> Result<bool, String> {
    SkillService::uninstall(&app_state.db, &id).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 切换 Skill 的应用启用状态
#[tauri::command]
pub fn toggle_skill_app(
    id: String,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    SkillService::toggle_app(&app_state.db, &id, &app_type, enabled).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 扫描未管理的 Skills
#[tauri::command]
pub fn scan_unmanaged_skills(
    app_state: State<'_, AppState>,
) -> Result<Vec<UnmanagedSkill>, String> {
    SkillService::scan_unmanaged(&app_state.db).map_err(|e| e.to_string())
}

/// 从应用目录导入 Skills
#[tauri::command]
pub fn import_skills_from_apps(
    directories: Vec<String>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    SkillService::import_from_apps(&app_state.db, directories).map_err(|e| e.to_string())
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取）
#[tauri::command]
pub async fn discover_available_skills(
    force_refresh: Option<bool>,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<DiscoverableSkill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    service
        .0
        .discover_available_with_policy(repos, &app_state.db, force_refresh.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_installed_skill_updates(
    force_refresh: Option<bool>,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<SkillUpdateStatus>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let discoverable = service
        .0
        .discover_available_with_policy(repos, &app_state.db, force_refresh.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())?;
    build_skill_update_statuses(&app_state.db, discoverable)
}

#[tauri::command]
pub async fn update_skills_unified_batch(
    ids: Vec<String>,
    force_refresh: Option<bool>,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<SkillBatchInstallResult, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let discoverable = service
        .0
        .discover_available_with_policy(repos, &app_state.db, force_refresh.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())?;
    let latest_by_key = discoverable
        .into_iter()
        .map(|skill| (skill.key.to_lowercase(), skill))
        .collect::<std::collections::HashMap<_, _>>();

    let mut installed = Vec::new();
    let mut failed = Vec::new();
    for id in ids {
        let existing = app_state
            .db
            .get_installed_skill(&id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Skill not found: {id}"))?;
        let Some(latest) = latest_by_key.get(&id.to_lowercase()) else {
            failed.push(SkillBatchFailure {
                key: id.clone(),
                error: "Skill not found in repository".to_string(),
            });
            continue;
        };
        match service
            .0
            .update_installed(&app_state.db, &existing, latest)
            .await
        {
            Ok(item) => installed.push(item),
            Err(err) => failed.push(SkillBatchFailure {
                key: id.clone(),
                error: err.to_string(),
            }),
        }
    }

    Ok(SkillBatchInstallResult { installed, failed })
}

// ========== 兼容旧 API 的命令 ==========

/// 获取技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    service
        .0
        .list_skills(repos, &app_state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills_for_app(
    app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    // 新版本不再区分应用，统一返回所有技能
    let _ = parse_app_type(&app)?; // 验证 app 参数有效
    get_skills(service, app_state).await
}

/// 安装技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    install_skill_for_app("claude".to_string(), directory, service, app_state).await
}

/// 安装指定应用的技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill_for_app(
    app: String,
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;

    // 先获取技能信息
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let skills = service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())?;

    let skill = skills
        .into_iter()
        .find(|s| {
            let install_name = std::path::Path::new(&s.directory)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.directory.clone());
            install_name.eq_ignore_ascii_case(&directory)
                || s.directory.eq_ignore_ascii_case(&directory)
        })
        .ok_or_else(|| {
            format_skill_error(
                "SKILL_NOT_FOUND",
                &[("directory", &directory)],
                Some("checkRepoUrl"),
            )
        })?;

    service
        .0
        .install(&app_state.db, &skill, &app_type)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

/// 卸载技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill(directory: String, app_state: State<'_, AppState>) -> Result<bool, String> {
    uninstall_skill_for_app("claude".to_string(), directory, app_state)
}

/// 卸载指定应用的技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill_for_app(
    app: String,
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let _ = parse_app_type(&app)?; // 验证参数

    // 通过 directory 找到对应的 skill id
    let skills = SkillService::get_all_installed(&app_state.db).map_err(|e| e.to_string())?;

    let skill = skills
        .into_iter()
        .find(|s| s.directory.eq_ignore_ascii_case(&directory))
        .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;

    SkillService::uninstall(&app_state.db, &skill.id).map_err(|e| e.to_string())?;

    Ok(true)
}

// ========== 仓库管理命令 ==========

/// 获取技能仓库列表
#[tauri::command]
pub fn get_skill_repos(app_state: State<'_, AppState>) -> Result<Vec<SkillRepo>, String> {
    app_state.db.get_skill_repos().map_err(|e| e.to_string())
}

/// 添加技能仓库
#[tauri::command]
pub fn add_skill_repo(repo: SkillRepo, app_state: State<'_, AppState>) -> Result<bool, String> {
    app_state
        .db
        .save_skill_repo(&repo)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 删除技能仓库
#[tauri::command]
pub fn remove_skill_repo(
    owner: String,
    name: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .delete_skill_repo(&owner, &name)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 从 ZIP 文件安装 Skills
#[tauri::command]
pub fn install_skills_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    let app_type = parse_app_type(&current_app)?;
    let path = std::path::Path::new(&file_path);

    SkillService::install_from_zip(&app_state.db, path, &app_type).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_skill_update_statuses, SkillUpdateStatus};
    use crate::app_config::{InstalledSkill, SkillApps};
    use crate::database::Database;
    use crate::services::skill::{DiscoverableSkill, SkillService};
    use serial_test::serial;
    use std::env;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
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

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
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
        }
    }

    fn create_db() -> Arc<Database> {
        Arc::new(Database::init().expect("init db"))
    }

    fn installed_skill(id: &str, directory: &str, content_hash: Option<&str>) -> InstalledSkill {
        InstalledSkill {
            id: id.to_string(),
            name: directory.to_string(),
            description: None,
            directory: directory.to_string(),
            repo_owner: Some("owner".to_string()),
            repo_name: Some("repo".to_string()),
            repo_branch: Some("main".to_string()),
            readme_url: None,
            content_hash: content_hash.map(str::to_string),
            apps: SkillApps::default(),
            installed_at: 1,
        }
    }

    fn discoverable_skill(
        key: &str,
        directory: &str,
        content_hash: Option<&str>,
    ) -> DiscoverableSkill {
        DiscoverableSkill {
            key: key.to_string(),
            name: directory.to_string(),
            description: String::new(),
            directory: directory.to_string(),
            readme_url: None,
            repo_owner: "owner".to_string(),
            repo_name: "repo".to_string(),
            repo_branch: "main".to_string(),
            content_hash: content_hash.map(str::to_string),
        }
    }

    fn write_skill_dir(directory: &str, contents: &str) {
        let path = SkillService::get_ssot_dir()
            .expect("ssot dir")
            .join(directory);
        fs::create_dir_all(&path).expect("create skill dir");
        fs::write(path.join("SKILL.md"), contents).expect("write skill file");
    }

    fn status_by_id<'a>(statuses: &'a [SkillUpdateStatus], id: &str) -> &'a SkillUpdateStatus {
        statuses
            .iter()
            .find(|item| item.id == id)
            .expect("status exists")
    }

    #[test]
    #[serial]
    fn build_skill_update_statuses_covers_all_states() {
        let _home = TempHome::new();
        let db = create_db();

        write_skill_dir("skill-a", "hash-a");
        write_skill_dir("skill-c", "hash-c");

        db.save_skill(&installed_skill("owner/repo:skill-a", "skill-a", None))
            .expect("save skill a");
        db.save_skill(&installed_skill(
            "owner/repo:skill-b",
            "skill-b",
            Some("same-hash"),
        ))
        .expect("save skill b");
        db.save_skill(&installed_skill("owner/repo:skill-c", "skill-c", None))
            .expect("save skill c");
        db.save_skill(&installed_skill("owner/repo:skill-d", "skill-d", None))
            .expect("save skill d");

        let statuses = build_skill_update_statuses(
            &db,
            vec![
                discoverable_skill("owner/repo:skill-a", "skill-a", Some("next-hash")),
                discoverable_skill("owner/repo:skill-b", "skill-b", Some("same-hash")),
                discoverable_skill("owner/repo:skill-c", "skill-c", None),
                discoverable_skill("owner/repo:skill-e", "skill-e", Some("unused")),
            ],
        )
        .expect("build statuses");

        assert_eq!(
            status_by_id(&statuses, "owner/repo:skill-a").state,
            "update_available"
        );
        assert_eq!(
            status_by_id(&statuses, "owner/repo:skill-b").state,
            "up_to_date"
        );
        assert_eq!(status_by_id(&statuses, "owner/repo:skill-c").state, "unknown");
        assert_eq!(
            status_by_id(&statuses, "owner/repo:skill-d").state,
            "not_found"
        );
    }
}
