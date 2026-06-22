# Supergoal Thinking: cc-switch Cross-Protocol Providers

## Goals

Enable cc-switch to route across the major coding-agent surfaces without fragile one-off hacks:

- Use Claude/Anthropic providers from Codex.
- Use Codex/OpenAI providers from Claude Code and Claude Desktop, preserving the already-working paths and making them explicit/tested.
- Use Gemini via Google OAuth from Claude and Codex.
- Preserve the prior PR work: English-first UI, Add Provider dropdown, per-Claude-family provider+target-model routing, Gemini OAuth refresh, and reverse Chat transform foundation.

## Constraints

- Brownfield Tauri app: React/TypeScript frontend, Rust proxy backend, SQLite config.
- The current local `HEAD` is upstream `farion1231/main`; the merged work lives on fetched ref `garcia/main` (`86e43881`).
- Upstream has diverged in the same files touched by PR #1: proxy routing, transforms, preset selector, i18n, model route DAO/schema.
- Cross-protocol translation is lossy. Tool calls, system/developer messages, thinking, cache tokens, images, and streaming SSE must be explicitly characterized.
- Live upstream validation requires user-owned API/OAuth credentials. Unit and fixture tests should not require secrets.

## Top Risks

1. Streaming protocol mismatch breaks real Codex/Claude usage.
   Mitigation: build fixture-driven SSE converters before wiring live forwarding; validate streaming chunks, final usage, tool calls, and cancellation behavior.

2. Rebase/merge against upstream silently drops prior PR behavior.
   Mitigation: start with a source-of-truth phase that checks out a branch from `garcia/main`, reconciles upstream intentionally, and adds characterization tests for all PR #1 behavior before new work.

3. OAuth handling may leak secrets or refresh the wrong credential shape.
   Mitigation: keep credentials server-side/Rust-only, redact logs, unit-test token parsing/refresh caching, and require manual live validation with throwaway credentials.

## Dependencies

- Phase 1 must decide the base branch and preserve PR #1 before any new feature work.
- Phase 2 must create golden fixtures; otherwise protocol work becomes guesswork.
- Codex -> Claude and Codex -> Gemini share the same Codex-consumer routing seam, so Claude should land first and Gemini reuse that seam.
- Google login can be split from Google OAuth backend support: pasted/imported OAuth creds first, full loopback/device login later if credentials/client IDs are available.

## Open Questions / Assumptions

- Assume we continue from `garcia/main` and reconcile upstream in a controlled branch, rather than building on the current upstream-only `HEAD`.
- Assume live validation can use user-provided Claude, Codex/OpenAI, and Gemini OAuth credentials later; secrets will not be committed or written to plan artifacts.
- Assume "Gemini OAuth" means Google OAuth refresh-token based access, with an in-app login flow as a later polish if OAuth client details are available.
- Assume provider routing should stay in proxy/takeover mode only; direct mode will remain protocol-native.

## Best Practices Applied

- Use characterization tests around existing behavior before changing a large proxy.
- Keep translation functions pure and fixture-tested; wire them into the forwarder only after non-streaming and streaming fixtures pass.
- Separate capability matrix/UI wiring from protocol conversion logic.
- Treat final live traffic tests as required evidence, but keep CI green without secrets.
