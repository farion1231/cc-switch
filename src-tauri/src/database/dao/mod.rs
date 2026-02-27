//! Data Access Object layer
//!
//! Database access operations for each domain

pub mod failover;
pub mod mcp;
pub mod prompts;
pub mod providers;
pub mod proxy;
pub mod settings;
pub mod skills;
pub mod stream_check;
pub mod universal_providers;

// 所有 DAO 方法都通过 Database impl 提供，无需单独导出
// 导出 FailoverQueueItem 供外部使用
pub use failover::FailoverQueueItem;
