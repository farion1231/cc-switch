//! 全部集成测试编进这一个测试二进制。
//!
//! `tests/` 下每个顶层 `.rs` 文件都会单独编成一个测试二进制，每个都要静态链接整个
//! `cc_switch_lib` 和全部依赖：链接时间和 target 目录的占用都随文件数成倍增长。
//! 新的集成测试写成本目录下的模块，在这里登记，不要再在 `tests/` 顶层加文件。

mod support;

mod app_config_load;
mod app_type_parse;
mod copilot_commands;
mod deeplink_import;
mod dsh_commands;
mod golden;
mod hermes_roundtrip;
mod import_export_sync;
mod kimi_commands;
mod mcode_commands;
mod mcp_commands;
mod profile_roundtrip;
mod prompt_live_sync;
mod provider_commands;
mod provider_service;
mod proxy_commands;
mod skill_sync;
mod workbuddy_commands;
