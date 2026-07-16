//! Secure local bridge + bootstrap JS bundle for Codex enhancements.

pub mod bridge;
pub mod bundle;

pub use bridge::{start_bridge, BridgeHandle};
pub use bundle::build_bootstrap_bundle;
