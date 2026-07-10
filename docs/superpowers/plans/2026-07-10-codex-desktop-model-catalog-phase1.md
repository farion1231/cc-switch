# Codex Desktop Model Catalog Phase 1 Implementation Plan

**Goal:** Make native Responses custom-model catalog entries conform to the current Codex model metadata contract, without adding Statsig/LevelDB mutation or changing provider routing/authentication.

**Base:** PR #3118 head (`codex/claude-official-api-switching-clean`), implemented on `codex/pr3118-codex-desktop-model-picker`.

## Scope

- Add a regression test proving generated native Responses entries contain an explicit `use_responses_lite` boolean.
- Keep the current native catalog behavior that omits inherited `model_messages`.
- Add the minimal generator change required to satisfy the regression test.
- Verify existing catalog and provider tests remain green.

## Non-goals

- No Codex Desktop Statsig or Chromium LevelDB writes.
- No changes to API-key versus ChatGPT authentication.
- No local proxy/takeover changes.
- No packaging until the focused code and tests pass.

## TDD sequence

1. Add a focused failing test in `src-tauri/src/codex_config.rs` for native Responses catalog metadata.
2. Run the focused test and confirm it fails because `use_responses_lite` is absent.
3. Update `codex_catalog_model_entry` with the minimal explicit field.
4. Re-run the focused test, relevant Codex catalog tests, formatting, and backend checks.
5. Stop after phase-one verification so the generated installer can be tested separately by the user.

## Compatibility decision

For third-party native Responses providers, set `use_responses_lite` to `false`. This avoids opting custom gateways into OpenAI-specific Responses Lite behavior while still providing the explicit schema field required by Codex Desktop model metadata parsing.
