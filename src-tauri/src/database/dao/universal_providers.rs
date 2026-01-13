//! 统一供应商 (Universal Provider) DAO
//!
//! 提供统一供应商的 CRUD 操作。

use crate::database::{lock_conn, to_json_string, Database};
use crate::error::AppError;
use crate::provider::UniversalProvider;
use std::collections::HashMap;

/// 统一供应商的 Settings Key
const UNIVERSAL_PROVIDERS_KEY: &str = "universal_providers";

impl Database {
    /// 获取所有统一供应商
    pub fn get_all_universal_providers(
        &self,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result: Option<String> = stmt
            .query_row([UNIVERSAL_PROVIDERS_KEY], |row| row.get(0))
            .ok();

        match result {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Database(format!("解析统一供应商数据失败: {e}"))),
            None => Ok(HashMap::new()),
        }
    }

    /// 获取单个统一供应商
    pub fn get_universal_provider(&self, id: &str) -> Result<Option<UniversalProvider>, AppError> {
        let providers = self.get_all_universal_providers()?;
        Ok(providers.get(id).cloned())
    }

    /// 保存统一供应商（添加或更新）
    pub fn save_universal_provider(&self, provider: &UniversalProvider) -> Result<(), AppError> {
        let mut providers = self.get_all_universal_providers()?;
        providers.insert(provider.id.clone(), provider.clone());
        self.save_all_universal_providers(&providers)
    }

    /// 删除统一供应商
    pub fn delete_universal_provider(&self, id: &str) -> Result<bool, AppError> {
        let mut providers = self.get_all_universal_providers()?;
        let existed = providers.remove(id).is_some();
        if existed {
            self.save_all_universal_providers(&providers)?;
        }
        Ok(existed)
    }

    /// 保存所有统一供应商（内部方法）
    fn save_all_universal_providers(
        &self,
        providers: &HashMap<String, UniversalProvider>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let json = to_json_string(providers)?;

        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
            [UNIVERSAL_PROVIDERS_KEY, &json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }
}
