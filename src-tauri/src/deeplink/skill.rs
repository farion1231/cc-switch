//! Skill import from deep link
//!
//! Handles importing skill repository configurations via ccswitch:// URLs.

use super::DeepLinkImportRequest;
use crate::error::AppError;
use crate::services::skill::SkillRepo;
use crate::store::AppState;

/// Import a skill from deep link request
pub fn import_skill_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a skill request
    if request.resource != "skill" {
        return Err(AppError::InvalidInput(format!(
            "Expected skill resource, got '{}'",
            request.resource
        )));
    }

    // Parse repo
    let repo_str = request
        .repo
        .ok_or_else(|| AppError::InvalidInput("Missing 'repo' field for skill".to_string()))?;

    let parts: Vec<&str> = repo_str.split('/').collect();
    if parts.len() != 2 {
        return Err(AppError::InvalidInput(format!(
            "Invalid repo format: expected 'owner/name', got '{repo_str}'"
        )));
    }
    let owner = parts[0].to_string();
    let name = parts[1].to_string();
    let branch = request.branch.unwrap_or_else(|| "main".to_string());

    // deeplink 是不可信输入，且 branch 会被拼进归档下载 URL：URL 解析会消解点段，
    // `../../../releases/download/v1/evil` 之类会把落点改写成攻击者可上传的
    // release asset。主防线在 SkillService::download_repo，这里是入库前的纵深拦截，
    // 让用户当场看到错误而不是让脏数据沉淀进 skill_repos 表。
    crate::services::skill::SkillService::validate_repo_ref(&owner, &name, &branch).map_err(
        |_| {
            AppError::InvalidInput(format!(
                "Invalid skill repository reference: '{owner}/{name}' branch '{branch}'"
            ))
        },
    )?;

    // Create SkillRepo
    let repo = SkillRepo {
        owner: owner.clone(),
        name: name.clone(),
        branch,
        enabled: request.enabled.unwrap_or(true),
    };

    // Save using Database
    state.db.save_skill_repo(&repo)?;

    log::info!("Successfully added skill repo '{owner}/{name}'");

    Ok(format!("{owner}/{name}"))
}
