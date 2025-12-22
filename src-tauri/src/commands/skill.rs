use crate::app_config::AppType;
use crate::error::format_skill_error;
use crate::services::skill::SkillState;
use crate::services::{Skill, SkillRepo, SkillService};
use crate::store::AppState;
use chrono::Utc;
use reqwest::Client;
use std::sync::Arc;
use tauri::State;

pub struct SkillServiceState(pub Arc<SkillService>);

/// 解析 app 参数为 AppType
fn parse_app_type(app: &str) -> Result<AppType, String> {
    match app.to_lowercase().as_str() {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        "gemini" => Ok(AppType::Gemini),
        _ => Err(format!("不支持的 app 类型: {app}")),
    }
}

/// 根据 app_type 生成带前缀的 skill key
fn get_skill_key(app_type: &AppType, directory: &str) -> String {
    let prefix = match app_type {
        AppType::Claude => "claude",
        AppType::Codex => "codex",
        AppType::Gemini => "gemini",
    };
    format!("{prefix}:{directory}")
}

#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    get_skills_for_app("claude".to_string(), service, app_state).await
}

#[tauri::command]
pub async fn get_skills_for_app(
    app: String,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let app_type = parse_app_type(&app)?;
    let service = SkillService::new_for_app(app_type.clone()).map_err(|e| e.to_string())?;

    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;

    let skills = service
        .list_skills(repos)
        .await
        .map_err(|e| e.to_string())?;

    // 自动同步本地已安装的 skills 到数据库
    // 这样用户在首次运行时，已有的 skills 会被自动记录
    let existing_states = app_state.db.get_skills().unwrap_or_default();

    for skill in &skills {
        if skill.installed {
            let key = get_skill_key(&app_type, &skill.directory);
            if !existing_states.contains_key(&key) {
                // 本地有该 skill，但数据库中没有记录，自动添加
                if let Err(e) = app_state.db.update_skill_state(
                    &key,
                    &SkillState {
                        installed: true,
                        installed_at: Utc::now(),
                    },
                ) {
                    log::warn!("同步本地 skill {key} 状态到数据库失败: {e}");
                }
            }
        }
    }

    Ok(skills)
}

#[tauri::command]
pub async fn install_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    install_skill_for_app("claude".to_string(), directory, service, app_state).await
}

#[tauri::command]
pub async fn install_skill_for_app(
    app: String,
    directory: String,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    let service = SkillService::new_for_app(app_type.clone()).map_err(|e| e.to_string())?;

    // 先在不持有写锁的情况下收集仓库与技能信息
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;

    let skills = service
        .list_skills(repos)
        .await
        .map_err(|e| e.to_string())?;

    let skill = skills
        .iter()
        .find(|s| s.directory.eq_ignore_ascii_case(&directory))
        .ok_or_else(|| {
            format_skill_error(
                "SKILL_NOT_FOUND",
                &[("directory", &directory)],
                Some("checkRepoUrl"),
            )
        })?;

    if !skill.installed {
        let repo = SkillRepo {
            owner: skill.repo_owner.clone().ok_or_else(|| {
                format_skill_error(
                    "MISSING_REPO_INFO",
                    &[("directory", &directory), ("field", "owner")],
                    None,
                )
            })?,
            name: skill.repo_name.clone().ok_or_else(|| {
                format_skill_error(
                    "MISSING_REPO_INFO",
                    &[("directory", &directory), ("field", "name")],
                    None,
                )
            })?,
            branch: skill
                .repo_branch
                .clone()
                .unwrap_or_else(|| "main".to_string()),
            enabled: true,
            base_url: None,
            access_token: None,
            auth_header: None,
        };

        service
            .install_skill(directory.clone(), repo)
            .await
            .map_err(|e| e.to_string())?;
    }

    let key = get_skill_key(&app_type, &directory);
    app_state
        .db
        .update_skill_state(
            &key,
            &SkillState {
                installed: true,
                installed_at: Utc::now(),
            },
        )
        .map_err(|e| e.to_string())?;

    Ok(true)
}

#[tauri::command]
pub fn uninstall_skill(
    directory: String,
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    uninstall_skill_for_app("claude".to_string(), directory, service, app_state)
}

#[tauri::command]
pub fn uninstall_skill_for_app(
    app: String,
    directory: String,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    let service = SkillService::new_for_app(app_type.clone()).map_err(|e| e.to_string())?;

    service
        .uninstall_skill(directory.clone())
        .map_err(|e| e.to_string())?;

    // Remove from database by setting installed = false
    let key = get_skill_key(&app_type, &directory);
    app_state
        .db
        .update_skill_state(
            &key,
            &SkillState {
                installed: false,
                installed_at: Utc::now(),
            },
        )
        .map_err(|e| e.to_string())?;

    Ok(true)
}

#[tauri::command]
pub fn get_skill_repos(
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<SkillRepo>, String> {
    app_state.db.get_skill_repos().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_skill_repo(
    repo: SkillRepo,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .save_skill_repo(&repo)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn remove_skill_repo(
    owner: String,
    name: String,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .delete_skill_repo(&owner, &name)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取单个仓库的技能列表
/// 
/// 用于渐进式加载，每个仓库独立加载其技能
#[tauri::command]
pub async fn get_skills_for_repo(
    app: String,
    repo_owner: String,
    repo_name: String,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let app_type = parse_app_type(&app)?;
    let service = SkillService::new_for_app(app_type.clone()).map_err(|e| e.to_string())?;

    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;

    let skills = service
        .list_skills_for_repo(&repos, &repo_owner, &repo_name)
        .await
        .map_err(|e| e.to_string())?;

    // 自动同步本地已安装的 skills 到数据库
    let existing_states = app_state.db.get_skills().unwrap_or_default();

    for skill in &skills {
        if skill.installed {
            let key = get_skill_key(&app_type, &skill.directory);
            if !existing_states.contains_key(&key) {
                if let Err(e) = app_state.db.update_skill_state(
                    &key,
                    &SkillState {
                        installed: true,
                        installed_at: Utc::now(),
                    },
                ) {
                    log::warn!("同步本地 skill {key} 状态到数据库失败: {e}");
                }
            }
        }
    }

    Ok(skills)
}

/// 测试私有仓库连接
/// 使用实际的 ZIP 下载 URL 测试，确保认证头对下载有效
#[tauri::command]
pub async fn test_repo_connection(url: String, access_token: String) -> Result<String, String> {
    let client = Client::builder()
        .user_agent("cc-switch")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    // 从仓库 URL 构建 ZIP 下载 URL 进行测试
    // 例如: https://git.rccchina.com/hugo.huang/wilbur-test 
    //    -> https://git.rccchina.com/hugo.huang/wilbur-test/-/archive/main/wilbur-test-main.zip
    let test_url = build_test_download_url(&url);
    log::info!("测试下载 URL: {}", test_url);

    // 认证方式列表：依次尝试
    // 注意：GitLab 下载 ZIP 需要 PRIVATE-TOKEN，所以放在前面
    let auth_methods: Vec<(&str, String)> = vec![
        ("PRIVATE-TOKEN", access_token.clone()),                   // GitLab
        ("Authorization", format!("token {}", access_token)),      // GitHub/Gitea
        ("Authorization", format!("Bearer {}", access_token)),     // GitHub/Bitbucket
    ];

    for (header_name, header_value) in &auth_methods {
        log::debug!("尝试认证头: {} = {}...", header_name, &header_value[..header_value.len().min(10)]);
        
        match client
            .head(&test_url)
            .header(*header_name, header_value)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let content_type = response.headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                
                log::debug!("认证头 {} 响应状态: {}, Content-Type: {}", header_name, status, content_type);
                
                // 检查是否成功且返回的是 ZIP 文件（不是 HTML 登录页面）
                if (status.is_success() || status.as_u16() == 302) 
                    && !content_type.contains("text/html") {
                    log::info!("连通测试成功，使用认证头: {}", header_name);
                    return Ok(header_name.to_string());
                }
            }
            Err(e) => {
                log::debug!("认证头 {} 请求失败: {}", header_name, e);
                continue;
            }
        }
    }

    // 所有认证方式都失败
    Err("连接失败，请检查 URL 和 Token 是否正确".to_string())
}

/// 从仓库 URL 构建测试用的 ZIP 下载 URL
fn build_test_download_url(repo_url: &str) -> String {
    let url = repo_url.trim_end_matches('/');
    
    // 提取 owner 和 repo_name
    let parts: Vec<&str> = url.rsplitn(3, '/').collect();
    let (repo_name, owner) = if parts.len() >= 2 {
        (parts[0], parts[1])
    } else {
        ("repo", "owner")
    };
    
    // 提取 base_url
    let base_url = if parts.len() >= 3 {
        url.strip_suffix(&format!("/{}/{}", owner, repo_name)).unwrap_or(url)
    } else {
        url
    };
    
    // 默认使用 GitLab API 格式（支持 Token 认证）
    if url.contains("github.com") {
        format!("{}/archive/refs/heads/main.zip", url)
    } else {
        // GitLab API 格式
        let encoded_path = format!("{}%2F{}", owner, repo_name);
        format!("{}/api/v4/projects/{}/repository/archive.zip?sha=main", base_url, encoded_path)
    }
}

/// 获取本地独有的技能列表
/// 
/// 返回所有本地安装的技能中，不属于任何远程仓库的技能。
/// 用于渐进式加载时单独显示本地技能。
#[tauri::command]
pub fn get_local_skills(
    app: String,
    remote_skills: Vec<Skill>,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let app_type = parse_app_type(&app)?;
    let service = SkillService::new_for_app(app_type.clone()).map_err(|e| e.to_string())?;

    let local_skills = service
        .list_local_skills(&remote_skills)
        .map_err(|e| e.to_string())?;

    // 自动同步本地已安装的 skills 到数据库
    let existing_states = app_state.db.get_skills().unwrap_or_default();

    for skill in &local_skills {
        if skill.installed {
            let key = get_skill_key(&app_type, &skill.directory);
            if !existing_states.contains_key(&key) {
                if let Err(e) = app_state.db.update_skill_state(
                    &key,
                    &SkillState {
                        installed: true,
                        installed_at: Utc::now(),
                    },
                ) {
                    log::warn!("同步本地 skill {key} 状态到数据库失败: {e}");
                }
            }
        }
    }

    Ok(local_skills)
}

/// 切换仓库的启用状态
/// 
/// 用于控制仓库是否在 Skills 页面中显示
#[tauri::command]
pub fn toggle_repo_enabled(
    owner: String,
    name: String,
    enabled: bool,
    _service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .toggle_skill_repo_enabled(&owner, &name, enabled)
        .map_err(|e| e.to_string())?;
    Ok(true)
}
