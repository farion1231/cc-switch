# T9 Progress — Codex user scripts + market

**Updated:** 2026-07-16
**Status:** WIP → compiling/tests

## Done commits
- T7 `61ab58a` — enhanced Codex runtime (never kill ordinary Codex)
- T8 `6572ae4` — configurable page enhancements + inject bundle

## T9 files
### Backend
- `src-tauri/src/services/codex_scripts.rs` — list/import/enable/delete/market install + atomic hash check
- `src-tauri/src/services/mod.rs` — `pub mod codex_scripts`
- `src-tauri/src/commands/codex_workbench.rs` — script commands + reinject_after_script_change
- `src-tauri/src/lib.rs` — command registration
- `src-tauri/src/app_store.rs` — `set_app_config_dir_override_for_test` (cfg test)
- bundle inject: user scripts snippet when `userScriptRuntime` enabled

### Frontend
- `src/types/codexWorkbench.ts` — UserScriptInfo / MarketIndex / ScriptInstallRequest
- `src/lib/api/codexWorkbench.ts` — invoke wrappers
- `src/lib/query/codexWorkbench.ts` — queries + mutations (no auto market refresh)
- `src/components/codex-workbench/ScriptsTab.tsx`
- `CodexWorkbenchPage.tsx` — ScriptsTab wired

## Constraints
- Market never auto-fetches; only explicit refresh/install
- Install is atomic; hash mismatch preserves old version
- Path traversal rejected
- Mutation → rebuild bundle + reinject if enhanced Codex running
- Never kill ordinary Codex

## Next
1. `cargo test --manifest-path src-tauri/Cargo.toml codex_scripts -- --nocapture`
2. Fix compile errors
3. Commit: `feat(codex): manage Codex user scripts and market`
4. Continue T10 plugins
