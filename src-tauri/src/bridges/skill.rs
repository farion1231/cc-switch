use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::bridges::support::{convert, to_core_app_type, with_core_state};
use crate::error::AppError;
use crate::services::skill::{DiscoverableSkill, Skill, SkillRepo};

pub fn get_installed_skills() -> Result<Vec<InstalledSkill>, AppError> {
    let skills =
        with_core_state(|state| cc_switch_core::SkillService::get_all_installed(&state.db))?;
    convert(skills)
}

pub async fn install_skill_unified(
    skill: DiscoverableSkill,
    current_app: AppType,
) -> Result<InstalledSkill, AppError> {
    let state = crate::bridges::support::fresh_core_state()?;
    let service = cc_switch_core::SkillService::new();
    let skill = convert(skill)?;
    let installed = service
        .install(&state.db, &skill, &to_core_app_type(current_app))
        .await
        .map_err(crate::bridges::support::map_core_err)?;
    convert(installed)
}

pub fn uninstall_skill_unified(id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::SkillService::uninstall(&state.db, id))
}

pub fn toggle_skill_app(id: &str, app: AppType, enabled: bool) -> Result<(), AppError> {
    with_core_state(|state| {
        cc_switch_core::SkillService::toggle_app(&state.db, id, &to_core_app_type(app), enabled)
    })
}

pub fn scan_unmanaged_skills() -> Result<Vec<UnmanagedSkill>, AppError> {
    let skills = with_core_state(|state| cc_switch_core::SkillService::scan_unmanaged(&state.db))?;
    convert(skills)
}

pub fn import_skills_from_apps(directories: Vec<String>) -> Result<Vec<InstalledSkill>, AppError> {
    let skills = with_core_state(|state| {
        cc_switch_core::SkillService::import_from_apps(&state.db, directories)
    })?;
    convert(skills)
}

pub async fn discover_available_skills() -> Result<Vec<DiscoverableSkill>, AppError> {
    let state = crate::bridges::support::fresh_core_state()?;
    let service = cc_switch_core::SkillService::new();
    let repos = cc_switch_core::SkillService::get_repos_or_default(&state.db)
        .map_err(crate::bridges::support::map_core_err)?;
    let skills = service
        .discover_available(repos)
        .await
        .map_err(crate::bridges::support::map_core_err)?;
    convert(skills)
}

pub async fn get_skills() -> Result<Vec<Skill>, AppError> {
    let state = crate::bridges::support::fresh_core_state()?;
    let service = cc_switch_core::SkillService::new();
    let repos = cc_switch_core::SkillService::get_repos_or_default(&state.db)
        .map_err(crate::bridges::support::map_core_err)?;
    let skills = service
        .list_skills(repos, &state.db)
        .await
        .map_err(crate::bridges::support::map_core_err)?;
    convert(skills)
}

pub fn get_skill_repos() -> Result<Vec<SkillRepo>, AppError> {
    let repos =
        with_core_state(|state| cc_switch_core::SkillService::get_repos_or_default(&state.db))?;
    convert(repos)
}

pub fn add_skill_repo(repo: SkillRepo) -> Result<(), AppError> {
    let repo: cc_switch_core::services::skill::SkillRepo = convert(repo)?;
    with_core_state(move |state| {
        let mut store = cc_switch_core::services::skill::SkillStore {
            skills: std::collections::HashMap::new(),
            repos: cc_switch_core::SkillService::get_repos_or_default(&state.db)?,
        };
        let service = cc_switch_core::SkillService::new();
        service.add_repo(&mut store, repo.clone())?;
        for existing in &store.repos {
            state.db.save_skill_repo(existing)?;
        }
        Ok(())
    })
}

pub fn remove_skill_repo(owner: &str, name: &str) -> Result<(), AppError> {
    with_core_state(|state| {
        state.db.delete_skill_repo(owner, name)?;
        Ok(())
    })
}

pub fn install_skills_from_zip(
    file_path: &str,
    current_app: AppType,
) -> Result<Vec<InstalledSkill>, AppError> {
    let path = std::path::Path::new(file_path);
    let skills = with_core_state(|state| {
        cc_switch_core::SkillService::install_from_zip(
            &state.db,
            path,
            &to_core_app_type(current_app.clone()),
        )
    })?;
    convert(skills)
}
