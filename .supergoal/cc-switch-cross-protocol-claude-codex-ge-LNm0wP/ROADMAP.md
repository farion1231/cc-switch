# Roadmap: cc-switch Cross-Protocol Providers

## Objective

Review and stabilize the work already merged in PR #1, then implement and verify cross-protocol provider routing so Claude can use Codex/OpenAI, Codex can use Claude/Anthropic, and both Claude and Codex can use Gemini with Google OAuth credentials.

## Stack

- Desktop app: Tauri 2
- Backend/proxy: Rust, Axum, reqwest, rusqlite
- Frontend: React, TypeScript, Vite, Tailwind/shadcn-style components
- Package manager: pnpm
- Key commands: `pnpm typecheck`, `pnpm format:check`, `pnpm test:unit`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`

## Phases

1. **Normalize Base** — choose the correct branch/ref, preserve PR #1, and establish a green baseline.
2. **Harden Prior Work** — review/fix per-model routing, Add Provider dropdown, and Gemini OAuth refresh behavior.
3. **Build Fixture Harness** — add golden fixtures and protocol capability matrix for Anthropic, OpenAI/Codex, Responses, and Gemini.
4. **Wire Codex To Claude** — implement non-streaming Codex consumer -> Anthropic provider routing and transforms.
5. **Implement Streaming Bridge** — add SSE conversion for Anthropic <-> OpenAI Responses/Chat streaming.
6. **Wire Gemini OAuth** — make Gemini OAuth usable from Claude and Codex via the same provider routing seam.
7. **Polish Provider UX** — expose clear cross-protocol presets, capability warnings, credential states, and target-model controls.
8. **Polish & Harden** — full regression sweep, live validation checklist, security review, docs, and final audit.

## Key Assumptions

- Work starts from `garcia/main` (`86e43881`) or a branch that intentionally reapplies it onto upstream.
- Existing Claude -> Codex/OpenAI support should be preserved and covered by tests, not rewritten unnecessarily.
- Codex -> Claude and Codex -> Gemini require proxy/takeover mode; direct mode remains protocol-native.
- CI must not require real provider secrets. Live tests are manual/evidence-based and use local env vars or app config only.

## Risk Mitigation

- Streaming and tool calls: fixture-first testing before live wiring.
- Upstream divergence: dedicated normalization phase with diff evidence.
- OAuth security: redaction tests and no secrets in logs, DB dumps, fixtures, screenshots, or plan files.

## Done Means

- All eight phases print `SUPERGOAL_PHASE_DONE`.
- Final audit reruns frontend and Rust gates successfully.
- At least one non-streaming and one streaming fixture exists for each new cross-protocol direction.
- Manual live validation checklist proves Claude -> Codex, Codex -> Claude, Claude -> Gemini OAuth, and Codex -> Gemini OAuth, or clearly marks any provider-account limitation as external.
