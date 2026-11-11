#![allow(non_snake_case)]

mod agy_account;
mod auth;
mod balance;
mod claude_account;
mod codex_oauth;
mod coding_plan;
mod config;
mod copilot;
mod custom_icon;
mod deeplink;
mod dsh;
mod env;
mod failover;
mod global_proxy;
mod hermes;
mod import_export;
mod kimi_oauth;
mod mcp;
mod misc;
mod model_fetch;
mod omo;
mod omp;
mod openclaw;
mod pi;
mod plugin;
mod profile;
mod prompt;
mod provider;
mod proxy;
mod session_manager;
mod settings;
pub mod skill;
mod stream_check;
mod subscription;
mod sync_support;
mod xai_oauth;

mod lightweight;
mod s3_sync;
mod usage;
mod webdav_sync;
mod workspace;

#[cfg(test)]
mod provider_concurrency_tests;
#[cfg(test)]
mod startup_tests;

pub use agy_account::*;
pub use auth::*;
pub use balance::*;
pub use claude_account::*;
pub use codex_oauth::*;
pub use coding_plan::*;
pub use config::*;
pub use copilot::*;
pub use custom_icon::*;
pub use deeplink::*;
pub use dsh::*;
pub use env::*;
pub use failover::*;
pub use global_proxy::*;
pub use hermes::*;
pub use import_export::*;
pub use kimi_oauth::*;
pub use mcp::*;
pub use misc::*;
pub use model_fetch::*;
pub use omo::*;
pub(crate) use omp::*;
pub use openclaw::*;
pub(crate) use pi::*;
pub use plugin::*;
pub use profile::*;
pub use prompt::*;
pub use provider::*;
pub use proxy::*;
pub use session_manager::*;
pub use settings::*;
pub use skill::*;
pub use stream_check::*;
pub use subscription::*;
pub use xai_oauth::*;

pub use lightweight::*;
pub use s3_sync::*;
pub use usage::*;
pub use webdav_sync::*;
pub use workspace::*;

/// 同步命令在 Tauri 2 里跑在主线程上：要等数据库连接锁（或等切换锁，切换时要拿库）的
/// 命令放到阻塞线程池里跑，否则会话同步占着连接锁时整个窗口和托盘都跟着卡住。
/// 同 `usage.rs` 的 `run_db_query`，只是闭包拿 `AppState`（服务层大多收它）、错误是 `String`。
async fn run_db_task<T, F>(
    state: &tauri::State<'_, crate::store::AppState>,
    task: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&crate::store::AppState) -> Result<T, String> + Send + 'static,
{
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || task(&state))
        .await
        .map_err(|error| format!("数据库任务失败: {error}"))?
}

#[cfg(not(target_os = "windows"))]
pub(crate) use misc::{login_shell_path, merge_path_segments, CommandDeadline};
