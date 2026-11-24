//! Skills 命令层
//!
//! v3.10.0+ 统一管理架构：
//! - 支持三应用开关（Claude/Codex/Gemini）
//! - SSOT 存储在 ~/.cc-switch/skills/

use super::run_db_task;
use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::error::AppError;
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, MigrationResult, SkillAppSyncOutcome,
    SkillBackupEntry, SkillDiscoveryResult, SkillRepo, SkillService, SkillStorageLocation,
    SkillUninstallResult, SkillUpdateCheckResult, SkillsShSearchResult, ZipInstallResult,
};
use crate::services::skill_repo_tokens::RepoTokenList;
use crate::store::AppState;
use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// SkillService 状态包装
pub struct SkillServiceState(pub Arc<SkillService>);

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, AppError> {
    AppType::from_str(app)
}

// ========== 统一管理命令 ==========

/// 获取所有已安装的 Skills
///
/// 只读数据库，不扫描 SSOT 目录；元数据刷新由
/// [`refresh_local_skill_metadata`] 单独在后台完成（issue #7848）。
#[tauri::command]
pub async fn get_installed_skills(
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    run_db_task(&app_state, |state| {
        SkillService::get_all_installed(&state.db).map_err(|e| e.to_string())
    })
    .await
}

/// 刷新本地 Skill 的过期元数据（手动编辑 SKILL.md 后同步名称/描述/哈希）。
///
/// 全目录哈希可能读入较大的资源文件，放到阻塞线程池执行，避免占用同步命令
/// 线程；返回实际更新的数量，前端仅在大于 0 时刷新列表。
#[tauri::command]
pub async fn refresh_local_skill_metadata(app_state: State<'_, AppState>) -> Result<usize, String> {
    run_db_task(&app_state, |state| {
        SkillService::refresh_local_skill_metadata(&state.db).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn get_skill_backups() -> Result<Vec<SkillBackupEntry>, String> {
    SkillService::list_backups().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_skill_backup(backup_id: String) -> Result<bool, String> {
    SkillService::delete_backup(&backup_id).map_err(|e| e.to_string())?;
    Ok(true)
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

/// 卸载 Skill（新版统一卸载）
#[tauri::command]
pub async fn uninstall_skill_unified(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    run_db_task(&app_state, move |state| {
        SkillService::uninstall(&state.db, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn restore_skill_backup(
    backup_id: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;
    run_db_task(&app_state, move |state| {
        SkillService::restore_from_backup(&state.db, &backup_id, &app_type)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn toggle_skill_app(
    id: String,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    run_db_task(&app_state, move |state| {
        SkillService::toggle_app(&state.db, &id, &app_type, enabled).map_err(|e| e.to_string())?;
        Ok::<_, String>(true)
    })
    .await
}

#[tauri::command]
pub async fn scan_unmanaged_skills(
    app_state: State<'_, AppState>,
) -> Result<Vec<UnmanagedSkill>, String> {
    run_db_task(&app_state, |state| {
        SkillService::scan_unmanaged(&state.db).map_err(|e| e.to_string())
    })
    .await
}

/// 从应用目录导入 Skills
#[tauri::command]
pub async fn import_skills_from_apps(
    imports: Vec<ImportSkillSelection>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    run_db_task(&app_state, move |state| {
        SkillService::import_from_apps(&state.db, imports).map_err(|e| e.to_string())
    })
    .await
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取），并逐仓库报告没读到的仓库
///
/// `force_refresh` 为 true（手动刷新）时绕过按仓库的发现缓存。
#[tauri::command]
pub async fn discover_available_skills(
    force_refresh: Option<bool>,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<SkillDiscoveryResult, String> {
    let repos = app_state.db.get_skill_repos()?;
    service
        .0
        .discover_available_report(repos, force_refresh.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

/// 只发现一个已保存的仓库（添加、重新启用仓库后用，不重拉其它仓库）。
/// 仓库不存在或已停用时返回空结果。
#[tauri::command]
pub async fn discover_repo_skills(
    owner: String,
    name: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<SkillDiscoveryResult, String> {
    let repos = app_state.db.get_skill_repos()?;
    let Some(repo) = repos
        .into_iter()
        .find(|repo| repo.owner == owner && repo.name == name)
    else {
        return Ok(SkillDiscoveryResult::default());
    };
    Ok(service.0.discover_repo(&repo).await)
}

/// 检查 Skills 更新，并逐仓库报告没读到的仓库
#[tauri::command]
pub async fn check_skill_updates(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<SkillUpdateCheckResult, String> {
    service
        .0
        .check_updates_report(&app_state.db)
        .await
        .map_err(|e| e.to_string())
}

/// 立即重新同步：按数据库里的开关和当前同步方式，把 Skill 重新投影到各应用目录，
/// 逐应用返回结果。只动 Skills 目录，不碰各应用的配置文件。
#[tauri::command]
pub async fn resync_skills_to_apps(
    app_state: State<'_, AppState>,
) -> Result<Vec<SkillAppSyncOutcome>, String> {
    let db = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || SkillService::resync_all_apps(&db))
        .await
        .map_err(|e| format!("重新同步 Skill 失败: {e}"))
}

#[tauri::command]
pub async fn update_skill(
    id: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    service
        .0
        .update_skill(&app_state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

/// 迁移 Skill 存储位置
#[tauri::command]
pub async fn migrate_skill_storage(
    target: SkillStorageLocation,
    app_state: State<'_, AppState>,
) -> Result<MigrationResult, String> {
    SkillService::migrate_storage(&app_state.db, target).map_err(|e| e.to_string())
}

/// CC Switch 目录下放 Skill 主副本的位置（改过配置目录就是改后的）
fn cc_switch_skills_dir() -> std::path::PathBuf {
    crate::config::get_app_config_dir().join("skills")
}

/// 「存储与同步」里显示的 CC Switch 目录路径
#[tauri::command]
pub async fn get_cc_switch_skills_dir() -> Result<String, String> {
    Ok(cc_switch_skills_dir().to_string_lossy().to_string())
}

/// 在系统文件管理器里打开 CC Switch 目录下的 Skills 目录（不存在就先建）
#[tauri::command]
pub async fn open_cc_switch_skills_dir(handle: AppHandle) -> Result<bool, String> {
    let dir = cc_switch_skills_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    crate::host_open::open_path(&handle, &dir).map_err(|e| format!("打开文件夹失败: {e}"))?;
    Ok(true)
}

/// 在系统文件管理器里打开某个已安装 Skill 的主副本目录
#[tauri::command]
pub async fn open_skill_dir(handle: AppHandle, directory: String) -> Result<bool, String> {
    let dir = SkillService::get_installed_skill_dir(&directory).map_err(|e| e.to_string())?;
    if !dir.is_dir() {
        return Err(format!("目录不存在: {}", dir.display()));
    }
    crate::host_open::open_path(&handle, &dir).map_err(|e| format!("打开文件夹失败: {e}"))?;
    Ok(true)
}

/// 搜索 skills.sh 公共目录
#[tauri::command]
pub async fn search_skills_sh(
    query: String,
    limit: usize,
    offset: usize,
) -> Result<SkillsShSearchResult, String> {
    SkillService::search_skills_sh(&query, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

// ========== 仓库管理命令 ==========

#[tauri::command]
pub async fn get_skill_repos(app_state: State<'_, AppState>) -> Result<Vec<SkillRepo>, String> {
    run_db_task(&app_state, |state| state.db.get_skill_repos()).await
}

#[tauri::command]
pub async fn add_skill_repo(
    mut repo: SkillRepo,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    // 主机名不分大小写，统一存小写；校验只认规范形式。
    repo.host = repo.host.trim().to_ascii_lowercase();
    // 整个结构体由前端反序列化而来，host/owner/name/branch 会被拼进归档下载 URL。
    // 主防线在 download_repo，这里让非法值当场报错而不是沉淀进表。
    SkillService::validate_repo(&repo).map_err(|e| e.to_string())?;
    run_db_task(&app_state, move |state| {
        state.db.save_skill_repo(&repo)?;
        Ok::<_, AppError>(true)
    })
    .await
}

#[tauri::command]
pub async fn remove_skill_repo(
    owner: String,
    name: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    run_db_task(&app_state, move |state| {
        state.db.delete_skill_repo(&owner, &name)?;
        Ok::<_, AppError>(true)
    })
    .await
}

// ========== 仓库访问令牌（#9666） ==========
//
// 令牌按主机存在本机（钥匙串或 0600 文件），不进数据库、不随同步。读写钥匙串在 macOS
// 上会起 `security` 子进程，都放到阻塞线程里做。

/// 设置了访问令牌的主机（不含令牌本身）
#[tauri::command]
pub async fn get_skill_repo_tokens(
    service: State<'_, SkillServiceState>,
) -> Result<RepoTokenList, String> {
    let service = Arc::clone(&service.0);
    tokio::task::spawn_blocking(move || service.repo_token_list())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 保存（覆盖）一台主机的访问令牌，返回规范化后的主机名
#[tauri::command]
pub async fn set_skill_repo_token(
    host: String,
    token: String,
    service: State<'_, SkillServiceState>,
) -> Result<String, String> {
    let service = Arc::clone(&service.0);
    tokio::task::spawn_blocking(move || service.set_repo_token(&host, &token))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 删除一台主机的访问令牌，返回规范化后的主机名
#[tauri::command]
pub async fn delete_skill_repo_token(
    host: String,
    service: State<'_, SkillServiceState>,
) -> Result<String, String> {
    let service = Arc::clone(&service.0);
    tokio::task::spawn_blocking(move || service.delete_repo_token(&host))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 从 ZIP 文件安装 Skills；目录名已被占用的如实列在 `skipped` 里（#3749）
#[tauri::command]
pub async fn install_skills_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<ZipInstallResult, String> {
    let app_type = parse_app_type(&current_app)?;
    run_db_task(&app_state, move |state| {
        let path = std::path::Path::new(&file_path);
        SkillService::install_from_zip(&state.db, path, &app_type).map_err(|e| e.to_string())
    })
    .await
}
