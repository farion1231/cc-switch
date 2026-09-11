# CC Switch DSH Native

This repository is the standalone development project for DeepSeek Harness support in CC Switch.

## Scope

- Keep the upstream CC Switch application intact while adding DSH as a first-class app.
- Use Codex as the UI and provider-management design reference.
- Use Pi only as the reference for native-file-to-database synchronization.
- Do not copy Codex OAuth, local proxy takeover, failover, or protocol conversion into DSH.

## Architecture

- `src-tauri/src/deepseek_harness_config.rs` owns DSH YAML and credential files.
- `src-tauri/src/services/provider/deepseek_harness.rs` owns native-provider synchronization and lifecycle behavior.
- `src/components/providers/forms/DeepSeekHarnessProviderForm.tsx` owns the Codex-style structured DSH form.
- `docs/architecture/dsh-native.md` is the durable architecture reference.

## Safety

- Never log or commit API key values from `~/.dsh/.credentials.yaml` or `~/.cc-switch/cc-switch.db`.
- Preserve unrelated DSH YAML namespaces and credential `records`.
- Back up real user state before any write-path integration test.
- Keep build outputs, local backups, and preview apps under ignored directories.

## Verification

- Run `pnpm test:dsh` for focused DSH verification.
- Run full frontend and Rust suites before a release candidate.
- Build macOS artifacts with `pnpm build:dsh:macos`.
- Mount and verify every DMG before presenting it as installable.

## Project Records

- Append concise work records to `docs/agent-memory/operations/YYYY-MM.md`.
- Keep durable corrections in `docs/agent-memory/lessons.md`.
- Do not hand-edit Codex-managed global memory files.
