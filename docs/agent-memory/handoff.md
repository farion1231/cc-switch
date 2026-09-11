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
- Source-aware provider add, edit, delete, and switch behavior.
- Codex-style structured DSH provider form and explicit provider-page identity.
- DSH Desktop entry in Local Environment Check.
- Reusable focused test and macOS package scripts.

## Verification

- `pnpm test:dsh`: passing.
- Focused Rust: 14 DSH tests plus one environment detection test.
- Focused frontend: 37 tests.
- TypeScript and rustfmt: passing.
- Latest mounted release artifact before project packaging: `3.20.2-dsh.2`, arm64, ad-hoc signature verified.

## Known Limitations

- The macOS package is ad-hoc signed, not notarized with an Apple Developer ID.
- DSH environment detection currently targets `/Applications/DSH Desktop.app` on macOS; Windows/Linux discovery remains CLI-based fallback.
- DSH form tests emit pre-existing React `act(...)` warnings but pass.
- Full upstream frontend/Rust suites are not part of the default focused script; set `DSH_FULL_TESTS=1` when preparing a merge candidate.

## Next Step

Run `pnpm build:dsh:macos`, verify the generated file under `release/`, and install it for visual acceptance of the provider page and Local Environment Check.
