//! SSH 远程运行时的协议与执行边界。
//!
//! 该模块必须保持无 WebView 依赖，使同一套核心逻辑既能被 Tauri 桌面端使用，
//! 也能被 Linux 上的无界面 Agent 使用。

pub mod capabilities;
pub mod client;
pub mod embedded_agent;
pub mod ephemeral_deploy;
pub mod models;
pub mod protocol;
pub mod runtime;
pub mod ssh;
pub mod ssh_config;
pub mod target_store;
