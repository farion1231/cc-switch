//! Skill service - business logic for skill management

use crate::app_config::{AppType, InstalledSkill};
use crate::database::Database;
use crate::error::AppError;
use crate::store::AppState;

/// Skill business logic service
pub struct SkillService;

impl SkillService {
    /// Get all installed skills
    pub fn get_all_installed(db: &Database) -> Result<Vec<InstalledSkill>, AppError> {
        db.get_all_skills()
    }

    /// Get a single skill
    pub fn get_skill(db: &Database, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        db.get_skill(id)
    }

    /// Install a skill
    pub fn install(state: &AppState, skill: &InstalledSkill) -> Result<(), AppError> {
        state.db.save_skill(skill)
    }

    /// Uninstall a skill
    pub fn uninstall(state: &AppState, id: &str) -> Result<(), AppError> {
        state.db.delete_skill(id)
    }

    /// Toggle skill for an app
    pub fn toggle_app(
        state: &AppState,
        id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut skill = state
            .db
            .get_skill(id)?
            .ok_or_else(|| AppError::Message(format!("Skill {} not found", id)))?;

        skill.apps.set_enabled_for(&app, enabled);
        state.db.save_skill(&skill)
    }
}
