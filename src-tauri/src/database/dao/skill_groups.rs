//! Persistent project groups for locally installed Skills.
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillProjectGroup {
    pub id: String,
    pub name: String,
    pub skill_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::InstalledSkill;
    use crate::services::skill::SkillService;
    use serde_json::json;
    use std::sync::Arc;

    fn skill(id: &str) -> InstalledSkill {
        serde_json::from_value(json!({
            "id": id, "name": id, "directory": id,
            "apps": { "claude": true }, "installedAt": 1, "updatedAt": 0
        }))
        .unwrap()
    }

    fn group(id: &str, ids: &[&str]) -> SkillProjectGroup {
        SkillProjectGroup {
            id: id.into(),
            name: id.into(),
            skill_ids: ids.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn skill_project_members_survive_save_and_metadata_update() {
        let db = Database::memory().unwrap();
        let mut local = skill("a");
        db.save_skill(&local).unwrap();
        db.save_skill_project_group(&group("project", &["a"]))
            .unwrap();
        local.name = "Updated".into();
        local.apps.codex = true;
        db.save_skill(&local).unwrap();
        local.updated_at = 2;
        db.update_skill_metadata(&local).unwrap();
        assert_eq!(db.get_skill_project_groups().unwrap()[0].skill_ids, ["a"]);
        assert!(db.get_installed_skill("a").unwrap().unwrap().apps.codex);
    }

    #[test]
    fn skill_project_move_and_dissolve_preserve_skills_and_switches() {
        let db = Database::memory().unwrap();
        db.save_skill(&skill("a")).unwrap();
        db.save_skill_project_group(&group("first", &["a"]))
            .unwrap();
        db.save_skill_project_group(&group("second", &["a"]))
            .unwrap();
        let groups = db.get_skill_project_groups().unwrap();
        assert!(groups[0].skill_ids.is_empty());
        assert_eq!(groups[1].skill_ids, ["a"]);
        db.delete_skill_project_group("second").unwrap();
        assert!(db.get_installed_skill("a").unwrap().unwrap().apps.claude);
        assert!(db.get_skill_project_groups().unwrap()[0]
            .skill_ids
            .is_empty());
    }

    #[test]
    fn skill_project_invalid_member_rolls_back_move_and_rename() {
        let db = Database::memory().unwrap();
        db.save_skill(&skill("a")).unwrap();
        db.save_skill_project_group(&group("first", &["a"]))
            .unwrap();
        assert!(db
            .save_skill_project_group(&group("second", &["a", "missing"]))
            .is_err());
        assert_eq!(
            db.get_skill_project_groups().unwrap(),
            [group("first", &["a"])]
        );
        let mut repo = skill("repo");
        repo.repo_owner = Some("owner".into());
        repo.repo_name = Some("repo".into());
        db.save_skill(&repo).unwrap();
        assert!(db
            .save_skill_project_group(&group("first", &["repo"]))
            .is_err());
        assert_eq!(
            db.get_skill_project_groups().unwrap(),
            [group("first", &["a"])]
        );
    }

    #[test]
    fn skill_project_uninstall_cleans_membership_without_deleting_group() {
        let db = Database::memory().unwrap();
        db.save_skill(&skill("a")).unwrap();
        db.save_skill_project_group(&group("project", &["a"]))
            .unwrap();
        db.delete_skill("a").unwrap();
        assert_eq!(
            db.get_skill_project_groups().unwrap(),
            [group("project", &[])]
        );
    }

    #[test]
    fn skill_project_group_service_trims_names_and_rejects_stale_groups() {
        let db = Arc::new(Database::memory().unwrap());
        db.save_skill(&skill("a")).unwrap();
        let group = SkillService::save_project_group(
            &db,
            None,
            " Project ".into(),
            vec!["a".into(), "a".into()],
        )
        .unwrap();
        assert_eq!(group.name, "Project");
        assert_eq!(group.skill_ids, ["a"]);
        assert!(SkillService::save_project_group(&db, None, "  ".into(), vec![]).is_err());
        assert!(SkillService::save_project_group(
            &db,
            Some("missing".into()),
            "Name".into(),
            vec![]
        )
        .is_err());
    }

    #[test]
    fn skill_project_groups_round_trip_through_backup_and_sync() {
        let db = Database::memory().unwrap();
        // Match a normal, already-migrated installation.
        Database::set_user_version(&db.conn.lock().unwrap(), crate::database::SCHEMA_VERSION)
            .unwrap();
        db.save_skill(&skill("a")).unwrap();
        db.save_skill_project_group(&group("project", &["a"]))
            .unwrap();
        for sql in [
            db.export_sql_string().unwrap(),
            db.export_sql_string_for_sync().unwrap(),
        ] {
            let restored = Database::memory().unwrap();
            restored.import_sql_string(&sql).unwrap();
            assert_eq!(
                restored.get_skill_project_groups().unwrap(),
                [group("project", &["a"])]
            );
            assert!(
                restored
                    .get_installed_skill("a")
                    .unwrap()
                    .unwrap()
                    .apps
                    .claude
            );
        }
    }
}

impl Database {
    pub fn get_skill_project_groups(&self) -> Result<Vec<SkillProjectGroup>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT g.id, g.name, m.skill_id FROM skill_project_groups g
             LEFT JOIN skill_project_group_members m ON m.group_id = g.id
             ORDER BY g.name COLLATE NOCASE, g.id, m.skill_id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut groups: Vec<SkillProjectGroup> = Vec::new();
        for row in rows {
            let (id, name, skill_id) = row.map_err(|e| AppError::Database(e.to_string()))?;
            if groups.last().is_none_or(|group| group.id != id) {
                groups.push(SkillProjectGroup {
                    id,
                    name,
                    skill_ids: Vec::new(),
                });
            }
            if let Some(skill_id) = skill_id {
                if let Some(group) = groups.last_mut() {
                    group.skill_ids.push(skill_id);
                }
            }
        }
        Ok(groups)
    }

    pub fn save_skill_project_group(&self, group: &SkillProjectGroup) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        // Validate before moving members; a stale/invalid selection must not partially edit groups.
        for id in &group.skill_ids {
            let local: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM skills WHERE id = ?1
                 AND (COALESCE(repo_owner, '') = '' OR COALESCE(repo_name, '') = ''))",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            if !local {
                return Err(AppError::InvalidInput(format!(
                    "Local Skill not found: {id}"
                )));
            }
        }
        tx.execute(
            "INSERT INTO skill_project_groups (id, name) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![group.id, group.name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM skill_project_group_members WHERE group_id = ?1",
            params![group.id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        for id in &group.skill_ids {
            // Each local Skill belongs to one project; moving it does not affect its app toggles.
            tx.execute(
                "INSERT INTO skill_project_group_members (skill_id, group_id) VALUES (?1, ?2)
                 ON CONFLICT(skill_id) DO UPDATE SET group_id = excluded.group_id",
                params![id, group.id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn delete_skill_project_group(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM skill_project_groups WHERE id = ?1",
            params![id],
        )
        .map(|count| count > 0)
        .map_err(|e| AppError::Database(e.to_string()))
    }
}
