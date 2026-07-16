//! Codex system-prompt rewrite and reasoning continuation (Phase 3).

pub mod continuation;
pub mod orchestrator;
pub mod prompt;
pub mod stream;
pub mod usage;

#[cfg(test)]
mod tests;

pub use continuation::{
    build_continue_request, decide_continuation, grid_multiple, ContinuationDecision,
    ContinuationEligibility, ContinuationStopReason, GRID_OFFSET, GRID_STEP, MAX_CONTINUE_ROUNDS,
    MIN_GRID_MULTIPLE,
};
pub use orchestrator::{
    run_pinned_continuation_loop, LogicalCodexRequestResult, NoCost, PinnedResponsesSender,
    PromptMeta, RoundCostEstimator,
};
pub use prompt::{
    rewrite_codex_system_prompt, CodexRequestProtocol, CodexReasoningContinuationConfig,
    CodexSystemPromptConfig, PromptRewriteMetadata,
};
pub use stream::{concat_sse_rounds, extract_terminal_output, parse_sse_to_round, strip_intermediate_completed};
pub use usage::{
    ContinuationRoundResult,
};
