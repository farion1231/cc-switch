use std::sync::Arc;

use super::{required_apps, state_lock, validate_id, VpsDocument, VpsServer, VpsService};
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
        server.validate()?;
        let _guard = skill_state_write_guard();
        self.mutate_with_skills_unlocked(db, |document| {
            if let Some(existing) = document
                .servers
                .iter_mut()
                .find(|item| item.id == server.id)
            {
                *existing = server;
            } else {
                document.servers.push(server);
            }
        })
    }

    pub fn delete_server_with_skills(
        &self,
        db: &Arc<Database>,
        id: &str,
    ) -> Result<Vec<VpsServer>> {
        validate_id(id)?;
        let _guard = skill_state_write_guard();
        self.mutate_with_skills_unlocked(db, |document| {
            document.servers.retain(|server| server.id != id)
        })
    }

    /// Lock order: global sync operation -> Skill state -> VPS document -> database.
    pub(crate) fn reconcile_skills_unlocked(&self, db: &Arc<Database>) -> Result<Vec<VpsServer>> {
        self.mutate_with_skills_unlocked(db, |_| {})
    }

    fn mutate_with_skills_unlocked(
        &self,
        db: &Arc<Database>,
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
        self.apply(document, bytes)?;
        SkillService::apply_vps_skill(db, plan).context(
            "VPS host data is saved, but Skill deployment is incomplete; retry before reporting client access",
        )?;
        Ok(self.read_document()?.0.servers)
    }
}

#[cfg(test)]
#[path = "managed_tests.rs"]
mod tests;
