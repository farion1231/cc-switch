//! 客户端文件的设备本地状态（`live-state.json`）、直连 / 代理两种模式，以及多文件操作
//! 的写前意图和崩溃恢复。

pub mod contract;
pub mod controller;
pub mod current;
pub mod operation;
pub mod stack;
pub mod state;
