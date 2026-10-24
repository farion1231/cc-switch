//! Skills 命令层
//!
//! v3.10.0+ 统一管理架构：
//! - 支持三应用开关（Claude/Codex/Gemini）
//! - SSOT 存储在 ~/.cc-switch/skills/

use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::database::Database;
use crate::error::format_skill_error;
use crate::services::git_credential::{self, validate_new_credential, GitCredential, SecretStore};
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, MigrationResult, RepoAuth, Skill, SkillAppSyncOutcome,
    SkillBackupEntry, SkillDiscoveryResult, SkillRepo, SkillService, SkillStorageLocation,
    SkillUninstallResult, SkillUpdateCheckResult, SkillsShSearchResult, ZipInstallResult,
};
use crate::store::AppState;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

/// SkillService 状态包装
pub struct SkillServiceState(pub Arc<SkillService>);

/// 密钥链状态包装。
///
/// 放在状态里而不是让 SkillService 自己持有，是为了让下载路径继续只依赖显式传入
/// 的 [`RepoAuth`]：服务层拿不到密钥链，也就无从"顺手"去读它。
pub struct SecretStoreState(pub Arc<dyn SecretStore>);

/// 读一次凭据，供本次命令的全部仓库共用。
///
/// 表里能有几枚凭据就有几次密钥链读取（Windows 上每次都是一趟系统调用），相对于
/// 随后的仓库下载可以忽略，所以不做按需解析——那会把密钥链读取塞进下载路径，正是
/// 要避免的隐式凭据获取。
///
/// 读不出来的凭据不进表：用到它的私有仓库会报"未配置访问令牌"，用户重新选一枚即可。
fn repo_auth(db: &Database, secrets: &dyn SecretStore) -> RepoAuth {
    let credentials = match db.list_git_credentials() {
        Ok(credentials) => credentials,
        Err(error) => {
            log::warn!("列出 Git 凭据失败，本次按未配置处理: {error}");
            return RepoAuth::anonymous();
        }
    };

    let mut resolved = HashMap::new();
    for credential in credentials {
        match secrets.get(&credential.host, &credential.id) {
            Ok(Some(secret)) => {
                resolved.insert(credential.id, secret);
            }
            Ok(None) => log::warn!("凭据 {} 不在系统密钥链里", credential.name),
            Err(error) => log::warn!("读取凭据失败，按未配置处理: {error}"),
        }
    }
    RepoAuth::from_secrets(resolved)
}

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

// ========== Git 凭据命令 ==========

/// 列出所有凭据的元数据（**不含 token 本体**），下拉框按这个顺序展示
#[tauri::command]
pub async fn list_git_credentials(
    app_state: State<'_, AppState>,
) -> Result<Vec<GitCredential>, String> {
    // 同步命令在 Tauri 2 里跑在主线程上；密钥链与 SQLite 都是阻塞 I/O
    let db = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.list_git_credentials().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("读取访问令牌列表失败: {e}"))?
}

/// 新建一枚凭据：元数据入库，token 进系统密钥链
///
/// 先写密钥链再落库，并读回确认：反过来的话，密钥链写失败会在库里留下一枚永远读不出
/// token 的凭据，而界面显示得好好的。
#[tauri::command]
pub async fn create_git_credential(
    name: String,
    token: String,
    secrets: State<'_, SecretStoreState>,
    app_state: State<'_, AppState>,
) -> Result<GitCredential, String> {
    // 同步命令在 Tauri 2 里跑在主线程上；密钥链与 SQLite 都是阻塞 I/O，放主线程
    // 会让窗口在系统凭据库响应慢时直接卡住
    let db = app_state.db.clone();
    let secrets = secrets.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        create_credential(&db, secrets.as_ref(), &name, &token)
    })
    .await
    .map_err(|e| format!("创建访问令牌失败: {e}"))?
}

/// 回滚时丢弃一条刚写入的密钥链条目。
///
/// 失败只记日志：调用方此时已经在返回另一个错误，再抛一个只会盖住真正的原因；
/// 但也不能静默——否则没人知道留下了一条孤儿。
fn discard_secret(secrets: &dyn SecretStore, host: &str, id: &str) {
    if let Err(error) = secrets.delete(host, id) {
        log::warn!("回滚密钥链条目失败，可能留下一条孤儿凭据 {id}: {error}");
    }
}

/// 新建凭据的核心逻辑。与 Tauri 状态解耦，便于直接单测。
fn create_credential(
    db: &Database,
    secrets: &dyn SecretStore,
    name: &str,
    token: &str,
) -> Result<GitCredential, String> {
    let host = git_credential::GITHUB_HOST;
    validate_new_credential(token).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().timestamp();
    let credential = GitCredential::new(&uuid::Uuid::new_v4().to_string(), host, name, now);
    secrets
        .set(host, &credential.id, token.trim())
        .map_err(|e| e.to_string())?;

    // 从这里往后任何一步失败，密钥链里那条都已经写进去了。没有元数据行它就是一
    // 条谁也看不到、应用内也删不掉的孤儿，所以每条失败路径都要顺手清掉。
    let stored = match secrets.get(host, &credential.id) {
        Ok(Some(secret)) => secret,
        Ok(None) => {
            discard_secret(secrets, host, &credential.id);
            return Err(format_skill_error(
                "KEYRING_UNAVAILABLE",
                &[("host", host)],
                Some("checkKeyring"),
            ));
        }
        Err(error) => {
            discard_secret(secrets, host, &credential.id);
            return Err(error.to_string());
        }
    };
    if stored.trim().is_empty() {
        discard_secret(secrets, host, &credential.id);
        return Err(format_skill_error(
            "KEYRING_UNAVAILABLE",
            &[("host", host)],
            Some("checkKeyring"),
        ));
    }

    if let Err(error) = db.save_git_credential(&credential) {
        discard_secret(secrets, host, &credential.id);
        return Err(error.to_string());
    }
    Ok(credential)
}

/// 删除一枚凭据（撤销入口）
///
/// 返回被清空引用的仓库坐标，供界面提示影响面。
#[tauri::command]
pub async fn delete_git_credential(
    id: String,
    secrets: State<'_, SecretStoreState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let db = app_state.db.clone();
    let secrets = secrets.0.clone();
    tauri::async_runtime::spawn_blocking(move || delete_credential(&db, secrets.as_ref(), &id))
        .await
        .map_err(|e| format!("删除访问令牌失败: {e}"))?
}

/// 删除凭据的核心逻辑。与 Tauri 状态解耦，便于直接单测。
fn delete_credential(
    db: &Database,
    secrets: &dyn SecretStore,
    id: &str,
) -> Result<Vec<String>, String> {
    let credential = db.get_git_credential(id).map_err(|e| e.to_string())?;

    // 先动密钥链，再动元数据。顺序反过来的话，密钥链删失败时元数据行已经没了，
    // 重试时 `get_git_credential` 返回 None，那条 token 就永久留在系统凭据库里：
    // 列表由元数据驱动，界面看不到它，应用内也没有任何入口能删——而用户以为已经撤销。
    //
    // 反过来的失败方向是可恢复的：密钥链删掉了、元数据删失败，剩下一条读不出
    // secret 的元数据，界面报「密钥链不可用」，再点一次删除即可（密钥链那边已是
    // NoEntry，按成功处理）。
    // 元数据行不存在时也按 id 再试一次：上一次删除可能已经删掉元数据、却在密钥链
    // 这一步失败，留下一条谁也引用不到的孤儿。`SecretStore::delete` 把 NoEntry 当
    // 成功，重复执行安全。
    let host = credential
        .as_ref()
        .map(|credential| credential.host.clone())
        .unwrap_or_else(|| git_credential::GITHUB_HOST.to_string());
    secrets.delete(&host, id).map_err(|e| e.to_string())?;

    let affected = db.delete_git_credential(id).map_err(|e| e.to_string())?;

    Ok(affected
        .into_iter()
        .map(|(owner, name)| format!("{owner}/{name}"))
        .collect())
}

// ========== 统一管理命令 ==========

/// 获取所有已安装的 Skills
#[tauri::command]
pub fn get_installed_skills(app_state: State<'_, AppState>) -> Result<Vec<InstalledSkill>, String> {
    SkillService::get_all_installed(&app_state.db).map_err(|e| e.to_string())
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
    credentials: State<'_, SecretStoreState>,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;
    let auth = repo_auth(&app_state.db, credentials.0.as_ref());

    service
        .0
        .install(&app_state.db, &skill, &app_type, &auth)
        .await
        .map_err(|e| e.to_string())
}

/// 卸载 Skill（新版统一卸载）
#[tauri::command]
pub fn uninstall_skill_unified(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    SkillService::uninstall(&app_state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_skill_backup(
    backup_id: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<InstalledSkill, String> {
    let app_type = parse_app_type(&current_app)?;
    SkillService::restore_from_backup(&app_state.db, &backup_id, &app_type)
        .map_err(|e| e.to_string())
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
    imports: Vec<ImportSkillSelection>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledSkill>, String> {
    SkillService::import_from_apps(&app_state.db, imports).map_err(|e| e.to_string())
}

// ========== 发现功能命令 ==========

/// 发现可安装的 Skills（从仓库获取），并逐仓库报告没读到的仓库
#[tauri::command]
pub async fn discover_available_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<SkillDiscoveryResult, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let auth = repo_auth(&app_state.db, credentials.0.as_ref());
    service
        .0
        .discover_available_report(repos, &auth)
        .await
        .map_err(|e| e.to_string())
}

/// 检查 Skills 更新，并逐仓库报告没读到的仓库
#[tauri::command]
pub async fn check_skill_updates(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<SkillUpdateCheckResult, String> {
    let auth = repo_auth(&app_state.db, credentials.0.as_ref());
    service
        .0
        .check_updates_report(&app_state.db, &auth)
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

/// 更新单个 Skill
#[tauri::command]
pub async fn update_skill(
    id: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<InstalledSkill, String> {
    let auth = repo_auth(&app_state.db, credentials.0.as_ref());
    service
        .0
        .update_skill(&app_state.db, &id, &auth)
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
    handle
        .opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;
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

// ========== 兼容旧 API 的命令 ==========

/// 获取技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<Vec<Skill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let auth = repo_auth(&app_state.db, credentials.0.as_ref());
    service
        .0
        .list_skills(repos, &app_state.db, &auth)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的技能列表（兼容旧 API）
#[tauri::command]
pub async fn get_skills_for_app(
    app: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<Vec<Skill>, String> {
    // 新版本不再区分应用，统一返回所有技能
    let _ = parse_app_type(&app)?; // 验证 app 参数有效
    get_skills(service, app_state, credentials).await
}

/// 安装技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<bool, String> {
    install_skill_for_app(
        "claude".to_string(),
        directory,
        service,
        app_state,
        credentials,
    )
    .await
}

/// 安装指定应用的技能（兼容旧 API）
#[tauri::command]
pub async fn install_skill_for_app(
    app: String,
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    let auth = repo_auth(&app_state.db, credentials.0.as_ref());

    // 先获取技能信息
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;
    let skills = service
        .0
        .discover_available(repos, &auth)
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
        .install(&app_state.db, &skill, &app_type, &auth)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

/// 卸载技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill(
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    uninstall_skill_for_app("claude".to_string(), directory, app_state)
}

/// 卸载指定应用的技能（兼容旧 API）
#[tauri::command]
pub fn uninstall_skill_for_app(
    app: String,
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<SkillUninstallResult, String> {
    let _ = parse_app_type(&app)?; // 验证参数

    // 通过 directory 找到对应的 skill id
    let skills = SkillService::get_all_installed(&app_state.db).map_err(|e| e.to_string())?;

    let skill = skills
        .into_iter()
        .find(|s| s.directory.eq_ignore_ascii_case(&directory))
        .ok_or_else(|| format!("未找到已安装的 Skill: {directory}"))?;

    SkillService::uninstall(&app_state.db, &skill.id).map_err(|e| e.to_string())
}

// ========== 仓库管理命令 ==========

/// 获取技能仓库列表
#[tauri::command]
pub fn get_skill_repos(app_state: State<'_, AppState>) -> Result<Vec<SkillRepo>, String> {
    app_state.db.get_skill_repos().map_err(|e| e.to_string())
}

/// 添加技能仓库
#[tauri::command]
pub async fn add_skill_repo(
    repo: SkillRepo,
    app_state: State<'_, AppState>,
    credentials: State<'_, SecretStoreState>,
) -> Result<bool, String> {
    // 整个结构体由前端反序列化而来，owner/name/branch 会被拼进归档下载 URL。
    // 主防线在 download_repo，这里让非法值当场报错而不是沉淀进表。
    // 私有仓库还要确认当前凭据真能读到它，否则先落库、等发现技能时才报错，
    // 用户看到的会是一个不含"凭据"字样的 404。
    //
    // 但这条命令同时是前端的「启用/停用」开关（SkillsPage 的 setRepoEnabled 走同一
    // 条路）。预检要联网并读取密钥链，停用一个私有仓库不该依赖这两件事都能成——只有
    // 仓库是新的、或私有标记与凭据绑定真的变了，才需要重新确认能读到它。
    let existing = app_state
        .db
        .get_skill_repo(&repo.owner, &repo.name)
        .map_err(|e| e.to_string())?;
    let binding_changed = match &existing {
        Some(existing) => {
            existing.private != repo.private || existing.credential_id != repo.credential_id
        }
        None => true,
    };

    if binding_changed {
        let auth = repo_auth(&app_state.db, credentials.0.as_ref());
        SkillService::validate_repo_access(&repo, &auth)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        // 纯本地写回：坐标仍要守住，但不联网
        SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)
            .map_err(|e| e.to_string())?;
    }

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

/// 从 ZIP 文件安装 Skills；目录名已被占用的如实列在 `skipped` 里（#3749）
#[tauri::command]
pub fn install_skills_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<ZipInstallResult, String> {
    let app_type = parse_app_type(&current_app)?;
    let path = std::path::Path::new(&file_path);

    SkillService::install_from_zip(&app_state.db, path, &app_type).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::services::git_credential::testing::InMemorySecretStore;

    fn repo_with_credential(credential_id: &str) -> SkillRepo {
        SkillRepo {
            owner: "acme".to_string(),
            name: "skills".to_string(),
            branch: "main".to_string(),
            enabled: true,
            private: true,
            credential_id: Some(credential_id.to_string()),
        }
    }

    #[test]
    fn create_credential_stores_metadata_and_secret() {
        let db = Database::memory().expect("memory db");
        let store = InMemorySecretStore::new();

        let created = create_credential(&db, &store, "工作账号", "ghp_abc123").expect("create");

        assert_eq!(created.name, "工作账号");
        assert_eq!(created.host, git_credential::GITHUB_HOST);
        assert_eq!(
            db.get_git_credential(&created.id).expect("get").unwrap(),
            created
        );
        assert_eq!(
            store
                .get(git_credential::GITHUB_HOST, &created.id)
                .expect("read secret")
                .as_deref(),
            Some("ghp_abc123")
        );
    }

    #[test]
    fn create_credential_leaves_nothing_behind_when_the_keychain_is_unavailable() {
        let db = Database::memory().expect("memory db");
        let store = InMemorySecretStore::unavailable();

        assert!(create_credential(&db, &store, "工作账号", "ghp_abc123").is_err());
        // 元数据不能落库：否则列表里会有一枚永远读不出 secret 的凭据
        assert!(db.list_git_credentials().expect("list").is_empty());
    }

    #[test]
    fn deleting_a_credential_removes_secret_metadata_and_repository_references() {
        let db = Database::memory().expect("memory db");
        let store = InMemorySecretStore::new();
        let created = create_credential(&db, &store, "工作账号", "ghp_abc123").expect("create");
        db.save_skill_repo(&repo_with_credential(&created.id))
            .expect("save repo");

        let affected = delete_credential(&db, &store, &created.id).expect("delete");

        assert_eq!(affected, vec!["acme/skills".to_string()]);
        assert!(db.get_git_credential(&created.id).expect("get").is_none());
        assert!(store
            .get(git_credential::GITHUB_HOST, &created.id)
            .expect("read secret")
            .is_none());
        assert!(db
            .get_skill_repo("acme", "skills")
            .expect("get repo")
            .expect("repo exists")
            .credential_id
            .is_none());
    }

    /// 回归：密钥链删不掉时，元数据与仓库引用必须原样留着。
    ///
    /// 顺序反过来的话，元数据行先没了，重试时 `get_git_credential` 返回 None，
    /// 那条 token 就永久留在系统凭据库里——列表由元数据驱动，界面看不到它，
    /// 应用内也没有任何入口能删，而用户以为已经撤销。
    #[test]
    fn a_failed_keychain_delete_keeps_the_metadata_so_a_retry_can_finish() {
        let db = Database::memory().expect("memory db");
        let mut store = InMemorySecretStore::new();
        let created = create_credential(&db, &store, "工作账号", "ghp_abc123").expect("create");
        db.save_skill_repo(&repo_with_credential(&created.id))
            .expect("save repo");

        store.delete_fails = true;
        assert!(delete_credential(&db, &store, &created.id).is_err());

        assert!(
            db.get_git_credential(&created.id).expect("get").is_some(),
            "密钥链删失败时元数据必须还在，重试才有机会继续"
        );
        assert_eq!(
            db.get_skill_repo("acme", "skills")
                .expect("get repo")
                .expect("repo exists")
                .credential_id
                .as_deref(),
            Some(created.id.as_str())
        );

        // 平台恢复后重试：现在能真正删干净
        store.delete_fails = false;
        delete_credential(&db, &store, &created.id).expect("retry delete");
        assert!(db.get_git_credential(&created.id).expect("get").is_none());
        assert!(store
            .get(git_credential::GITHUB_HOST, &created.id)
            .expect("read secret")
            .is_none());
    }
}
