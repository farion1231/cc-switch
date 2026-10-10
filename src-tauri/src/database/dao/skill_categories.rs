//! Categories organize Skills without changing their paths or enabled apps.
use crate::app_config::SkillCategory;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection};

fn normalized_name(name: &str) -> Result<(String, String), AppError> {
    let name = name.trim();
    if !(1..=50).contains(&name.chars().count()) {
        return Err(AppError::localized(
            "skills.category.invalid_name",
            "分类名称须为 1–50 个字符",
            "Category names must contain 1–50 characters",
        ));
    }
    Ok((name.to_owned(), name.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::{InstalledSkill, SkillApps};

    fn seed(db: &Database, id: &str) -> InstalledSkill {
        let skill = InstalledSkill {
            id: id.into(),
            name: id.into(),
            directory: id.into(),
            category_id: None,
            description: None,
            repo_owner: None,
            repo_name: None,
            repo_branch: None,
            readme_url: None,
            apps: SkillApps {
                codex: true,
                ..Default::default()
            },
            installed_at: 1,
            content_hash: None,
            updated_at: 0,
        };
        db.save_skill(&skill).unwrap();
        skill
    }

    #[test]
    fn category_crud_validates_unicode_names_and_stable_ids() {
        let db = Database::memory().unwrap();
        for invalid in ["", "  ", &"类".repeat(51)] {
            assert!(db.create_skill_category(invalid).is_err());
        }
        let category = db.create_skill_category("  DéVELOPMENT  ").unwrap();
        assert_eq!(category.name, "DéVELOPMENT");
        assert!(db.create_skill_category("dévelopment").is_err());
        let other = db.create_skill_category("Other").unwrap();
        assert!(db.rename_skill_category(&other.id, "DÉVELOPMENT").is_err());
        db.rename_skill_category(&category.id, "  Development  ")
            .unwrap();
        let renamed = db
            .get_skill_categories()
            .unwrap()
            .into_iter()
            .find(|c| c.id == category.id)
            .unwrap();
        assert_eq!(renamed.name, "Development");
        assert!(db.rename_skill_category("missing", "New").is_err());
        db.delete_skill_category(&other.id).unwrap();
        assert_eq!(db.get_skill_categories().unwrap().len(), 1);
    }

    #[test]
    fn assignment_rolls_back_and_deletion_preserves_skills_and_apps() {
        let db = Database::memory().unwrap();
        let original = seed(&db, "alpha");
        seed(&db, "beta");
        let category = db.create_skill_category("Development").unwrap();
        assert!(db
            .set_skill_categories(&["alpha".into(), "missing".into()], Some(&category.id))
            .is_err());
        assert_eq!(
            db.get_installed_skill("alpha")
                .unwrap()
                .unwrap()
                .category_id,
            None
        );
        assert!(db
            .set_skill_categories(&["alpha".into()], Some("missing"))
            .is_err());
        db.set_skill_categories(&["alpha".into(), "beta".into()], Some(&category.id))
            .unwrap();
        assert_eq!(
            db.get_all_installed_skills().unwrap()["beta"]
                .category_id
                .as_deref(),
            Some(category.id.as_str())
        );
        db.delete_skill_category(&category.id).unwrap();
        let stored = db.get_installed_skill("alpha").unwrap().unwrap();
        assert_eq!(stored.category_id, None);
        assert_eq!(stored.apps, original.apps);
        assert_eq!(stored.directory, original.directory);
        assert_eq!(db.get_all_installed_skills().unwrap().len(), 2);
    }
}

fn require_category(conn: &Connection, id: &str) -> Result<(), AppError> {
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM skill_categories WHERE id = ?1)",
        [id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(AppError::localized(
            "skills.category.missing",
            "分类不存在",
            "Category does not exist",
        ));
    }
    Ok(())
}

fn require_unique(conn: &Connection, key: &str, except: &str) -> Result<(), AppError> {
    if conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM skill_categories WHERE name_key = ?1 AND id != ?2)",
        params![key, except],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(AppError::localized(
            "skills.category.duplicate",
            "分类名称已存在",
            "Category name already exists",
        ));
    }
    Ok(())
}

impl Database {
    pub fn get_skill_categories(&self) -> Result<Vec<SkillCategory>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt =
            conn.prepare("SELECT id, name FROM skill_categories ORDER BY name_key, id")?;
        let categories = stmt
            .query_map([], |row| {
                Ok(SkillCategory {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(categories)
    }

    pub fn create_skill_category(&self, name: &str) -> Result<SkillCategory, AppError> {
        let (name, key) = normalized_name(name)?;
        let conn = lock_conn!(self.conn);
        require_unique(&conn, &key, "")?;
        let category = SkillCategory {
            id: uuid::Uuid::new_v4().to_string(),
            name,
        };
        conn.execute(
            "INSERT INTO skill_categories (id, name, name_key) VALUES (?1, ?2, ?3)",
            params![category.id, category.name, key],
        )?;
        Ok(category)
    }

    pub fn rename_skill_category(&self, id: &str, name: &str) -> Result<(), AppError> {
        let (name, key) = normalized_name(name)?;
        let conn = lock_conn!(self.conn);
        require_category(&conn, id)?;
        require_unique(&conn, &key, id)?;
        conn.execute(
            "UPDATE skill_categories SET name = ?1, name_key = ?2 WHERE id = ?3",
            params![name, key, id],
        )?;
        Ok(())
    }

    /// One transaction: deleting a category never removes a Skill or toggles apps.
    pub fn delete_skill_category(&self, id: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        require_category(&tx, id)?;
        tx.execute(
            "UPDATE skills SET category_id = NULL WHERE category_id = ?1",
            [id],
        )?;
        tx.execute("DELETE FROM skill_categories WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Missing categories or Skills roll back the entire batch.
    pub fn set_skill_categories(
        &self,
        ids: &[String],
        category_id: Option<&str>,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        if let Some(id) = category_id {
            require_category(&tx, id)?;
        }
        for id in ids {
            if tx.execute(
                "UPDATE skills SET category_id = ?1 WHERE id = ?2",
                params![category_id, id],
            )? == 0
            {
                return Err(AppError::localized(
                    "skills.category.skill_missing",
                    "Skill 已被卸载，请刷新后重试",
                    "Skill is no longer installed. Refresh and try again.",
                ));
            }
        }
        tx.commit()?;
        Ok(())
    }
}
