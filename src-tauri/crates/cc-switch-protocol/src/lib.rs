//! 桌面端与临时 SSH Agent 共享的线协议边界。
//!
//! 该 crate 必须保持与 Tauri、窗口系统和业务数据库解耦，确保 Agent 的传递依赖不会
//! 意外带入 GUI 栈。新增协议类型时应优先放在这里，并通过桌面兼容模块重导出。

pub mod capabilities;
pub mod protocol;
