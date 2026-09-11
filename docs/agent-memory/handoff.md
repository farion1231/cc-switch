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
- DSH provider add/edit now uses the same shared form shell as OpenCode (preset picker, name/website/notes, provider key, JSON editor, meta/providerType assembly) plus DSH structured fields (credential ref, base URL, API format, model catalog, default model). The previous standalone `DeepSeekHarnessProviderForm` was removed.
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

- Version: `3.20.2-dsh.6` (dsh.5 plus the full OpenCode preset catalog and inline model-catalog parity).
- Path: `release/CC-Switch-3.20.2-dsh.6-DSH-Codex-arm64.dmg`
- SHA-256: `d9364f3b397eaacc9e5e47e1ea73a00e5af89fe8b5ea6e9146a347eac9041239`
- Installed and launched locally as 3.20.2-dsh.6; OCR-verified the DSH add dialog lists the full derived OpenCode preset set.
- Local data note: `~/.dsh/settings.yaml` drifted during testing (yuzuvalley gained `MiniMax-M2.7-highspeed` and the default model changed); credential values are unchanged. Backup: `backups/settings.yaml.backup-20260911-162828`.

## Next Step

Confirm with the user whether to keep the drifted `~/.dsh/settings.yaml` or restore the backup. Then continue visual acceptance of the DSH add/edit model catalog (inline fetched-model dropdown, Add appends a row) and report any remaining OpenCode/Pi parity gaps.
