//! Codex enhanced runtime: discovery, launch policy, CDP, state.

pub mod cdp;
pub mod discovery;
pub mod launcher;
pub mod state;

pub use launcher::{
    launch_enhanced_codex, reinject_enhancements, CodexRuntimeHandle, LaunchEnhancedCodexResult,
};
pub use state::CodexRuntimeState;

// Re-export hooks for unit tests / future DI
#[cfg(test)]
#[allow(unused_imports)]
pub use launcher::{launch_with_hooks, FakeHooks, LaunchHooks};
