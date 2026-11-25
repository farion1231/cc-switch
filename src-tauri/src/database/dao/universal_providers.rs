//! 统一供应商 (Universal Provider) DAO
//!
//! 提供统一供应商的 CRUD 操作。

use crate::app_config::AppType;
use crate::database::{lock_conn, to_json_string, Database};
use crate::error::AppError;
use crate::provider::UniversalProvider;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// 统一供应商的 Settings Key
const UNIVERSAL_PROVIDERS_KEY: &str = "universal_providers";

impl Database {
    /// 获取所有统一供应商
    pub fn get_all_universal_providers(
        &self,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        let conn = lock_conn!(self.conn);

        load_providers(&conn)
    }

    /// 获取单个统一供应商
    pub fn get_universal_provider(&self, id: &str) -> Result<Option<UniversalProvider>, AppError> {
        let providers = self.get_all_universal_providers()?;
        Ok(providers.get(id).cloned())
    }

    /// 保存统一供应商（添加或更新）
    pub fn save_universal_provider(&self, provider: &UniversalProvider) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        let mut providers = load_providers(&tx)?;
        providers.insert(provider.id.clone(), provider.clone());
        save_providers(&tx, &providers)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove the parent and generated children together, preserving live failover queues.
    pub fn delete_universal_provider(&self, id: &str) -> Result<bool, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        let mut providers = load_providers(&tx)?;
        if providers.remove(id).is_none() {
            return Ok(false);
        }

        let children = [AppType::Claude, AppType::Codex, AppType::Gemini].map(|app| {
            let child_id = format!("universal-{app}-{id}");
            (app, child_id)
        });
        for (app, child_id) in &children {
            super::failover::guard_failover_queue_removal(&tx, app, child_id)?;
        }
        for (app, child_id) in children {
            tx.execute(
                "DELETE FROM providers WHERE app_type = ?1 AND id = ?2",
                params![app, child_id],
            )?;
        }
        save_providers(&tx, &providers)?;
        tx.commit()?;
        Ok(true)
    }
}

fn load_providers(conn: &Connection) -> Result<HashMap<String, UniversalProvider>, AppError> {
    let json = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [UNIVERSAL_PROVIDERS_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match json {
        Some(json) => serde_json::from_str(&json)
            .map_err(|e| AppError::Database(format!("解析统一供应商数据失败: {e}"))),
        None => Ok(HashMap::new()),
    }
}

fn save_providers(
    conn: &Connection,
    providers: &HashMap<String, UniversalProvider>,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
        params![UNIVERSAL_PROVIDERS_KEY, to_json_string(providers)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universal_delete_keeps_the_parent_and_children_if_failover_blocks_one() {
        let db = Database::memory().unwrap();
        let mut parent = UniversalProvider::new(
            "example".into(),
            "Example".into(),
            "custom".into(),
            "https://api.example".into(),
            "sk-test".into(),
        );
        parent.apps.claude = true;
        parent.apps.codex = true;
        db.save_universal_provider(&parent).unwrap();
        let claude = parent.to_claude_provider().unwrap();
        let codex = parent.to_codex_provider().unwrap();
        db.save_provider(&AppType::Claude, &claude).unwrap();
        db.save_provider(&AppType::Codex, &codex).unwrap();
        db.add_to_failover_queue(&AppType::Codex, &codex.id)
            .unwrap();
        db.set_auto_failover_enabled_sync(&AppType::Codex, true)
            .unwrap();

        assert!(db.delete_universal_provider(&parent.id).is_err());
        assert!(db.get_universal_provider(&parent.id).unwrap().is_some());
        assert!(db.provider_exists(&claude.id, &AppType::Claude).unwrap());
        assert!(db.provider_exists(&codex.id, &AppType::Codex).unwrap());

        db.set_auto_failover_enabled_sync(&AppType::Codex, false)
            .unwrap();
        assert!(db.delete_universal_provider(&parent.id).unwrap());
        assert!(db.get_universal_provider(&parent.id).unwrap().is_none());
        assert!(!db.provider_exists(&claude.id, &AppType::Claude).unwrap());
        assert!(!db.provider_exists(&codex.id, &AppType::Codex).unwrap());
    }
}
