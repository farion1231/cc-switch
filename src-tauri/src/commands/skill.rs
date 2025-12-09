use crate::error::format_skill_error;
use crate::services::skill::SkillState;
use crate::services::{Skill, SkillRepo, SkillService};
use crate::store::AppState;
use chrono::Utc;
use std::sync::Arc;
use tauri::State;

pub struct SkillServiceState(pub Arc<SkillService>);

#[tauri::command]
pub async fn get_skills(
    service: State<'_, SkillServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Skill>, String> {
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;

    let skills = service
        .0
        .list_skills(repos)
        .await
        .map_err(|e| e.to_string())?;

    // 自动同步本地已安装的 skills 到数据库
    // 这样用户在首次运行时，已有的 skills 会被自动记录
    let existing_states = app_state.db.get_skills().unwrap_or_default();

    for skill in &skills {
        if skill.installed && !existing_states.contains_key(&skill.directory) {
            // 本地有该 skill，但数据库中没有记录，自动添加
            if let Err(e) = app_state.db.update_skill_state(
                &skill.directory,
                &SkillState {
                    installed: true,
                    installed_at: Utc::now(),
                },
            ) {
                log::warn!("同步本地 skill {} 状态到数据库失败: {}", skill.directory, e);
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
    // 先在不持有写锁的情况下收集仓库与技能信息
    let repos = app_state.db.get_skill_repos().map_err(|e| e.to_string())?;

    let skills = service
        .0
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
        };

        service
            .0
            .install_skill(directory.clone(), repo)
            .await
            .map_err(|e| e.to_string())?;
    }

    app_state
        .db
        .update_skill_state(
            &directory,
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
    service
        .0
        .uninstall_skill(directory.clone())
        .map_err(|e| e.to_string())?;

    // Remove from database by setting installed = false
    app_state
        .db
        .update_skill_state(
            &directory,
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
