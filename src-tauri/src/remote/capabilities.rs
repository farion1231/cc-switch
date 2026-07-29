//! 桌面端远程能力表兼容入口。
//!
//! 白名单由 protocol crate 统一维护，确保桌面客户端与临时 Agent 对同一命令集、超时和
//! 幂等语义达成一致。

pub use cc_switch_protocol::capabilities::*;
