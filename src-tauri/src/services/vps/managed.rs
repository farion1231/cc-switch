use std::sync::Arc;

use super::{
    required_apps, state_lock, validate_id, SecretString, VpsDocument, VpsServer, VpsService,
};
use crate::database::Database;
use crate::services::skill::{skill_state_write_guard, SkillService};
use anyhow::{anyhow, Context, Result};

impl VpsService {
    pub fn get_servers_with_skills(&self, db: &Arc<Database>) -> Result<Vec<VpsServer>> {
        let _guard = skill_state_write_guard();
        self.reconcile_skills_unlocked(db)
    }

    pub fn save_server_with_skills(
        &self,
        db: &Arc<Database>,
        server: VpsServer,
    ) -> Result<Vec<VpsServer>> {
        self.save_server_with_skills_and_password(db, server, None)
    }

    pub fn save_server_with_skills_and_password(
        &self,
        db: &Arc<Database>,
        server: VpsServer,
        password: Option<String>,
    ) -> Result<Vec<VpsServer>> {
        let password = SecretString::provided(password);
        server.validate()?;
        let _guard = skill_state_write_guard();
        self.mutate_with_skills_unlocked(db, Some((&server, password.as_ref())), |document| {
            Self::upsert(document, server.clone());
        })
    }

    pub fn delete_server_with_skills(
        &self,
        db: &Arc<Database>,
        id: &str,
    ) -> Result<Vec<VpsServer>> {
        validate_id(id)?;
        let _guard = skill_state_write_guard();
        self.mutate_with_skills_unlocked(db, None, |document| Self::remove(document, id))
    }

    /// Lock order: global sync operation -> Skill state -> VPS document -> database.
    pub(crate) fn reconcile_skills_unlocked(&self, db: &Arc<Database>) -> Result<Vec<VpsServer>> {
        self.mutate_with_skills_unlocked(db, None, |_| {})
    }

    fn mutate_with_skills_unlocked(
        &self,
        db: &Arc<Database>,
        authentication: Option<(&VpsServer, Option<&SecretString>)>,
        mutate: impl FnOnce(&mut VpsDocument),
    ) -> Result<Vec<VpsServer>> {
        let _guard = state_lock()
            .lock()
            .map_err(|_| anyhow!("VPS state lock is poisoned"))?;
        let (mut document, bytes) = self.read_document()?;
        mutate(&mut document);
        if bytes.is_none()
            && document.servers.is_empty()
            && db
                .get_installed_skill(crate::services::skill::vps::SKILL_ID)?
                .is_none()
            && !self.root.join("skill-state.json").exists()
        {
            return Ok(Vec::new());
        }
        let plan =
            SkillService::prepare_vps_skill(db, &self.root, &required_apps(&document.servers))?;
        self.apply_authenticated(document, bytes, authentication)?;
        SkillService::apply_vps_skill(db, plan).context(
            "VPS host data is saved, but Skill deployment is incomplete; retry before reporting client access",
        )?;
        Ok(self.read_document()?.0.servers)
    }
}

#[cfg(test)]
#[path = "managed_tests.rs"]
pub(super) mod tests;
