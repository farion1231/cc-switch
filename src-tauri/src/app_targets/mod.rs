//! Windows / WSL 多环境配置写入目标。
//!
//! Windows 用户可以为 Claude Code、Codex、Gemini CLI、OpenCode 各自选择 Windows、一个或
//! 多个 WSL 发行版作为写入目标，并指定一个主环境：
//!
//! - 主环境是读取来源（导入、回填、外部修改检测、会话读取），旧 `*ConfigDir` 是它的兼容
//!   投影，所有现有读取入口因此自动指向主环境；
//! - 写入先走主环境的现有链路（指针、模式、代理不变），成功后在每个跟随环境的作用域里
//!   复用同一套写入（[`scope`]），按目标隔离状态、备份和登录暂存；
//! - 默认仍只有 Windows；旧 WSL UNC 目录升级后只写原目录（[`model::derive_from_legacy`]）；
//!   取消目标不删文件。

pub mod installs;
pub mod model;
pub mod scope;
pub mod service;
pub mod status;
pub mod sync;
pub mod wsl;

#[cfg(test)]
mod tests;
