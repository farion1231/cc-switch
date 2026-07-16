# T12 Progress Snapshot (2026-07-16)

## Done
- Backend `src-tauri/src/services/codex_reasoning/` module:
  - `prompt.rs` — system prompt rewrite + identity correction + fingerprint
  - `mod.rs` re-exports `CodexSystemPromptConfig`, `CodexReasoningContinuationConfig`
- `ProviderMeta` fields (Rust + TS): `codexSystemPrompt`, `codexReasoningContinuation`
- Types re-exported from `codex_reasoning` (no duplicate structs in `provider.rs`)
- `forwarder.rs` wires `rewrite_codex_system_prompt` **before** protocol conversion
- Unit tests: `cargo test --lib codex_reasoning::prompt` → **6/6 green**
- FE:
  - `src/components/providers/forms/CodexReasoningSettings.tsx`
  - wired into `ProviderForm.tsx` / `CodexFormFields.tsx`
  - `tests/components/CodexReasoningSettings.test.tsx`
  - `tests/utils/providerMetaUtils.test.ts` extended

## Remaining for T12 commit
1. Run FE unit tests:
   `pnpm test:unit -- tests/components/CodexReasoningSettings.test.tsx tests/utils/providerMetaUtils.test.ts`
2. `pnpm typecheck` (or scoped check)
3. Commit:
   ```
   git add src-tauri/src/services/codex_reasoning src-tauri/src/provider.rs src-tauri/src/proxy/forwarder.rs src/components/providers/forms src/types.ts src/utils/providerMetaUtils.ts tests
   git commit -m "feat(codex): provider system prompt rewrite + reasoning settings (T12)"
   ```

## Next
- **T13**: reasoning continuation core (`continuation.rs`, `stream.rs`, `usage.rs`)
- Never kill ordinary Codex process

## Key design
- Prompt rewrite runs outbound, before Chat/Anthropic conversion
- Fingerprint only (32 hex) — never log/return full prompt text
- Continuation toggle independent from system-prompt toggle
- max_rounds clamped 0..=3 (UI 1..=3, default 3)
