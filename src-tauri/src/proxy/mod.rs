//! 代理服务器模块
//!
//! 提供本地HTTP代理服务，支持多Provider故障转移和请求透传

pub mod error;
mod forwarder;
mod handlers;
mod health;
pub mod providers;
mod router;
pub(crate) mod server;
pub(crate) mod types;

// 公开导出给外部使用（commands, services等模块需要）
#[allow(unused_imports)]
pub use error::ProxyError;
#[allow(unused_imports)]
pub use types::{ProxyConfig, ProxyServerInfo, ProxyStatus};

// 内部模块间共享（供子模块使用）
// 注意：这个导出用于模块内部，编译器可能警告未使用但实际被子模块使用
#[allow(unused_imports)]
pub(crate) use types::*;
