//! Opt-in LAN Mobile Web UI server for CC Switch.
//!
//! Runs on a dedicated control-plane port (default `15722`, separate from the
//! local data-plane proxy port `15721`) with strict `Origin`/`Sec-Fetch-Site`
//! protection, constant-time token auth, `HttpOnly; SameSite=Strict` session
//! cookies, secret-stripped responses, and read-only default mode.

pub mod agent_remote;
pub mod config;
pub mod handlers;
pub mod net;
pub mod security;
pub mod service;

pub use config::{LanWebConfig, LanWebMode};
pub use service::{LanWebConfigUpdate, LanWebService, LanWebStatus};
