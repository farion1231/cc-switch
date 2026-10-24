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

    // `save_skill_repo` 是按 (owner, name) 的 INSERT OR REPLACE：同一个坐标已经配过
    // 私有标记、凭据或分支时，用 deeplink 的缺省值整行写回会把它们抹掉，仓库此后
    // 静默走公开路径、只报 404。已有行要保留应用内的配置，deeplink 只覆盖它真正带
    // 了的字段。
    let existing = state.db.get_skill_repo(&owner, &name)?;
    // deeplink 表达不了「这个仓库是私有的」——私有标记与凭据只能由用户在仓库管理
    // 面板里配置，这里从已有行继承，没有就按公开。
    let (private, credential_id) = match &existing {
        Some(existing) => (existing.private, existing.credential_id.clone()),
        None => (false, None),
    };
    // 请求里没写 branch 时，「main」只是缺省值，不该覆盖用户改过的分支
    let branch = match (request.branch.as_deref(), &existing) {
        (Some(branch), _) => branch.to_string(),
        (None, Some(existing)) if !existing.branch.is_empty() => existing.branch.clone(),
        (None, _) => "main".to_string(),
    };

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

    let repo = SkillRepo {
        owner: owner.clone(),
        name: name.clone(),
        branch,
        enabled: request.enabled.unwrap_or(true),
        private,
        credential_id,
    };

    // Save using Database
    state.db.save_skill_repo(&repo)?;

    log::info!("Successfully added skill repo '{owner}/{name}'");

    Ok(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;

    fn state_with(repo: SkillRepo) -> AppState {
        let db = Arc::new(Database::memory().expect("memory db"));
        db.save_skill_repo(&repo).expect("save repo");
        AppState::new(db)
    }

    fn configured_repo() -> SkillRepo {
        SkillRepo {
            owner: "acme".to_string(),
            name: "skills".to_string(),
            branch: "develop".to_string(),
            enabled: true,
            private: true,
            credential_id: Some("c1".to_string()),
        }
    }

    fn request(branch: Option<&str>) -> DeepLinkImportRequest {
        DeepLinkImportRequest {
            resource: "skill".to_string(),
            repo: Some("acme/skills".to_string()),
            branch: branch.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn deeplink_import_keeps_the_existing_private_binding_and_branch() {
        // save_skill_repo 是 INSERT OR REPLACE：deeplink 只带坐标，用缺省值整行写回
        // 会把用户配好的私有标记、凭据和分支一起抹掉，仓库此后静默走公开路径。
        let state = state_with(configured_repo());

        import_skill_from_deeplink(&state, request(None)).expect("import");

        let stored = state
            .db
            .get_skill_repo("acme", "skills")
            .expect("get")
            .expect("repo exists");
        assert!(stored.private, "私有标记不能被 deeplink 抹掉");
        assert_eq!(stored.credential_id.as_deref(), Some("c1"));
        assert_eq!(
            stored.branch, "develop",
            "请求没带 branch 时不能覆盖用户改过的分支"
        );
    }

    #[test]
    fn an_explicit_deeplink_branch_still_wins() {
        let state = state_with(configured_repo());

        import_skill_from_deeplink(&state, request(Some("release"))).expect("import");

        let stored = state
            .db
            .get_skill_repo("acme", "skills")
            .expect("get")
            .expect("repo exists");
        assert_eq!(stored.branch, "release");
        assert!(stored.private);
        assert_eq!(stored.credential_id.as_deref(), Some("c1"));
    }
}
