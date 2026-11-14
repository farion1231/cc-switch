//! 「我的图标」：用户上传过的自定义图标库
//!
//! 存在 settings 表 `custom_icons` 键下，是一个 JSON 数组，随云同步。图标库只是挑选用的
//! 素材：供应商选中某个图标时，`providers.icon` 存的是图片本身（data URI），不是这里的 id，
//! 所以删掉或改名图标库里的条目，不影响已经在用它的供应商。

use serde::{Deserialize, Serialize};

use crate::database::{lock_conn, to_json_string, Database};
use crate::error::AppError;

/// 图标库的 settings 键，导入清理（`services/provider/icon.rs`）也按这个键找
pub const CUSTOM_ICONS_KEY: &str = "custom_icons";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomIconEntry {
    pub id: String,
    pub name: String,
    /// `data:image/png;base64,…`，和 `providers.icon` 里的自定义图标同一种格式
    pub data: String,
    #[serde(default)]
    pub created_at: i64,
    /// 较新版本写进来的字段，读进来再原样写回去
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Database {
    /// 读出图标库，按数组顺序（新上传的在前）。
    ///
    /// 解析不了的条目跳过，不让一条坏数据挡住整个图标库；数组本身解析不了时当作空库。
    pub fn get_custom_icons(&self) -> Result<Vec<CustomIconEntry>, AppError> {
        let Some(raw) = self.get_setting(CUSTOM_ICONS_KEY)? else {
            return Ok(Vec::new());
        };
        let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else {
            log::warn!("图标库数据无法解析，按空库处理");
            return Ok(Vec::new());
        };
        Ok(values
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect())
    }

    pub fn save_custom_icons(&self, icons: &[CustomIconEntry]) -> Result<(), AppError> {
        let json = to_json_string(&icons)?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            [CUSTOM_ICONS_KEY, &json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
