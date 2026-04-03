#[cfg(target_os = "windows")]
pub mod dedupe;
#[cfg(target_os = "windows")]
pub mod server;
#[cfg(not(target_os = "windows"))]
pub mod stub;
#[cfg(target_os = "windows")]
pub mod toast;
pub mod types;

#[cfg(target_os = "windows")]
pub use server::ClaudeNotifyService;
#[cfg(not(target_os = "windows"))]
pub use stub::ClaudeNotifyService;
