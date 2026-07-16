//! Codex system-prompt rewrite and reasoning continuation (Phase 3).

pub mod continuation;
pub mod orchestrator;
pub mod prompt;
pub mod stream;
pub mod usage;

#[cfg(test)]
mod tests;

pub use continuation::ContinuationEligibility;
pub use orchestrator::{run_pinned_continuation_loop, NoCost, PinnedResponsesSender, PromptMeta};
pub use prompt::{
    rewrite_codex_system_prompt, CodexReasoningContinuationConfig, CodexRequestProtocol,
    CodexSystemPromptConfig,
};
pub use stream::parse_sse_to_round;
pub use usage::ContinuationRoundResult;
