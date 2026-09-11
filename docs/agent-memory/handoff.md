# Handoff

## Project

- Goal: maintain a standalone CC Switch fork with first-class DSH native providers and a Codex-style UI.
- Branch: `dsh-native` after local branch normalization.
- Upstream base: CC Switch 3.20.2 plus PR #6526 merge ref.
- Current release version: `3.20.2-dsh.2`.

## Implemented

- Native DSH YAML and version-1 credential adapter.
- `llm-deepseek` and `llm-pi-ai.providers` synchronization.
- Current provider/model synchronization from `agent-default-model`.
- Source-aware provider add, edit, delete, switch, and remove-from-live behavior.
- Codex-style structured DSH provider form and explicit provider-page identity.
- DSH Desktop entry in Local Environment Check.
- Reusable focused test and macOS package scripts.
- Live state query (`get_dsh_current_state`) and default-model command (`set_dsh_current_model`) wired to provider cards (set-as-default with model dropdown, remove-from-config, in-config badges).
- Zstd session history browsing and deletion in the session manager (`~/.dsh/sessions/**/session.jsonl.zstd`).
- Reachability (speed test) base-URL resolution for DSH providers.
- MCP and Skills entries hidden for DSH (DSH 2.0.5 has neither surface).
- `~/.dsh` directory override in Settings (dshConfigDir).
- `dsh.*` i18n section in all four locales; OpenAI-compatible provider preset; first-run AGENTS.md prompt import.

## Verification

- `DSH_FULL_TESTS=1 pnpm test:dsh`: passing (full Rust 2847 lib tests + integration suites; frontend 1104 tests).
- Vitest config now excludes ignored `work/`/`release/` build artifacts (previously 15 false failures).
- TypeScript and rustfmt: passing.
- Latest mounted release artifact: `3.20.2-dsh.2` (pre-iteration). A new build has not been produced after the parity iteration.

## Known Limitations

- The macOS package is ad-hoc signed, not notarized with an Apple Developer ID.
- DSH environment detection currently targets `/Applications/DSH Desktop.app` on macOS; Windows/Linux discovery remains CLI-based fallback.
- DSH sessions carry no token usage fields; usage statistics are intentionally absent.
- Removing the provider that is currently selected in `agent-default-model` leaves a dangling default pointer (pre-existing DSH lifecycle gap, also present in delete).
- Two pi-ai routes sharing one `apiKeyEnv` credential ref: removing one route clears the shared ref (pre-existing credential design).
- DSH form tests emit pre-existing React `act(...)` warnings but pass.

## Latest Artifact

- Version: `3.20.2-dsh.4` (dsh.3 plus the app-visibility fix).
- Path: `release/CC-Switch-3.20.2-dsh.4-DSH-Codex-arm64.dmg`
- SHA-256: `ab2ddc8f28d20ebf09fa6df602c64c90ce8ea0974875e35478dc509093f3346c`
- Installed and launched locally as 3.20.2-dsh.4; OCR-verified that Settings > Main Page Display now includes the DeepSeek Harness toggle. Native `settings.yaml` untouched; session manager lists 19 real DSH sessions.

## Next Step

Continue visual acceptance: provider-card hover actions (set-as-default dropdown, remove-from-config) and the DSH entry in Settings > directory overrides. Report any further incompleteness with the specific screen.
