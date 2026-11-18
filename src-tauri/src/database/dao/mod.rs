//! Data Access Object layer
//!
//! Database access operations for each domain

pub mod custom_icons;
pub mod failover;
pub mod mcp;
pub mod profiles;
pub mod prompts;
pub mod providers;
pub mod providers_seed;
pub mod proxy;
pub mod settings;
pub mod skills;
pub mod stream_check;
pub mod universal_providers;
pub mod usage_rollup;

// 所有 DAO 方法都通过 Database impl 提供，无需单独导出
// 导出 FailoverQueueItem / Profile 供外部使用
pub use failover::FailoverQueueItem;
pub use profiles::Profile;

use crate::app_config::AppType;
use rusqlite::types::{ToSql, ToSqlOutput};

/// 库里 `app_type` 列存的就是 [`AppType::as_str`]，DAO 直接绑定 `&AppType`。
impl ToSql for AppType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}
