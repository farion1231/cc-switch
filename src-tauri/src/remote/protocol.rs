//! 桌面端协议兼容入口。
//!
//! 实现位于独立 protocol crate，以保证 Linux Agent 不需要反向依赖 Tauri 主包。现有桌面
//! 调用路径继续从这里重导出，避免迁移阶段产生无关的调用方改动。

pub use cc_switch_protocol::protocol::*;
