SUPERGOAL_PHASE_START
Phase: 7 of 8 — Polish Provider UX
Task: Expose the new cross-protocol capabilities clearly in settings and provider setup.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 9
Evidence required: UI screenshot paths or textual walkthrough, test names, command summaries
Depends on phases: 1, 2, 3, 4, 5, 6

## Work

Make the UX honest and usable. Users should understand which app can target which provider, which mode is required, which features are lossy, and where to set target model names per Claude family. Avoid marketing copy; use operational labels, warnings, disabled states, and focused help text.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- UI walkthrough or screenshot paths.
- Frontend test names and pass summary.
- Copy/capability warning summary.
- Command summary with exit codes.

## Deliverables

- Provider presets or badges for Claude via Codex/OpenAI, Codex via Claude/Anthropic, Gemini OAuth for Claude, and Gemini OAuth for Codex.
- Capability warnings for proxy-only/direct-mode unsupported paths.
- Credential-state UI for Gemini OAuth import/login.
- Tests for new/changed frontend controls.
- Updated i18n strings at least for English, with existing locale fallback behavior preserved.

## Acceptance Criteria

- Add Provider search can find the new cross-protocol presets.
- Settings/routing UI makes app protocol vs provider protocol clear.
- Per-model provider+target-model controls still work after new presets are added.
- Direct-mode-incompatible selections are disabled or show a clear warning before use.
- Gemini OAuth credential errors are visible without exposing secrets.
- Empty/loading/error states are covered for the new provider controls.
- Keyboard navigation works for combobox and routing controls.
- UI tests cover the primary new flows.
- Mandatory commands pass.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
