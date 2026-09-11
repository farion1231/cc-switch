# DSH Full Parity Design

## Goal

Bring the DeepSeek Harness app surface to Pi/OpenCode-level completeness: hide unsupported entries, add session history, expose live state and default-model control, fix speed-test and remove semantics, and complete settings/i18n/preset polish. Native DSH files remain authoritative.

## Decisions

- MCP and Skills entries are hidden for DSH: DSH 2.0.5 has no MCP configuration namespace in `settings.yaml` and no `~/.dsh/skills` directory. Wiring CC Switch to surfaces the DSH runtime does not read would be decorative.
- Token usage statistics are excluded: DSH session events (`assistant/message`, `step/end`, `turn/end`) carry no token usage fields, so there is no data source. Session history browsing is implementable and included.
- DSH does not gain local proxy, failover, OAuth, or protocol conversion (unchanged from `docs/architecture/dsh-native.md`).

## A. Dangling entry fixes

- `src/App.tsx`: exclude `deepseek-harness` from `hasMcpSupport` and `hasSkillsSupport`, and add view fallbacks so a persisted `mcp`/`skills` view resets to the provider page (mirror the existing Pi exclusion at the same locations).
- `src-tauri/src/services/stream_check.rs`: add an `AppType::DeepSeekHarness` arm in `resolve_base_url` that reads `baseURL` from the provider's `settings_config`, so the existing `EndpointSpeedTest` button works instead of returning "does not support proxy adapters".

## B. Session history

DSH stores one directory per project under `~/.dsh/sessions/<encoded-cwd>/`, each session in `<session-id>/session.jsonl.zstd` (zstd-compressed JSONL; the `zstd` crate is already a dependency).

- New `src-tauri/src/session_manager/providers/deepseek_harness.rs`, modeled on the Pi provider:
  - Scan: enumerate session files, parse the `session` header event (id, createdAt, cwd) and `session/title`, plus first user message as a fallback title. The project display name prefers the `cwd` field from the `session` header; the encoded directory name is only a fallback because DSH's `--`/`~XX~` escaping is lossy for paths containing literal tildes.
  - Load: parse `user/message`, `assistant/message`, `text-chunks`, and `reasoning-chunks` events into the shared message model.
  - Delete: remove the session directory.
  - Roots: project roots come from the encoded directory names.
- Register the provider in `session_manager/providers/mod.rs` and wire it into `scan_sessions`, `load_messages`, `delete_session_with_roots`, and `provider_roots` in `session_manager/mod.rs`.
- Frontend: add `deepseek-harness` to `hasSessionSupport` (`src/App.tsx`), the sessions `ProviderFilter` and filter UI (`SessionManagerPage.tsx`).
- Malformed or truncated session files are skipped with a log line; a corrupt file must not break listing of other sessions.

## C. Live state and default-model commands

- New `src-tauri/src/commands/deepseek_harness.rs`:
  - `get_dsh_current_state` returns native provider ids and the current provider/model from `read_native_state()`.
  - `set_dsh_current_model(provider_id, model)` wraps the existing `deepseek_harness_config::set_current_model`, validating that the provider exists and the model belongs to it; updates the mirrored DB row's current marker.
- Register both in `src-tauri/src/lib.rs` and export from `commands/mod.rs`.
- Frontend: new `src/lib/api/dsh.ts` and `src/lib/query/dsh.ts` mirroring the Pi modules; `ProviderList.isProviderInConfig` uses real native state for DSH instead of returning `true` unconditionally; `App.tsx` wires `onSetAsDefault` for DSH so a card's model can be set as the DSH default without switching provider.

## D. Source-aware live removal

`ProviderService::remove_from_live_config` (`services/provider/mod.rs`) currently clears the whole `llm-deepseek` section regardless of which provider was targeted. Make it source-aware via `meta.providerType`: `dsh_pi_ai` removes only `llm-pi-ai.providers.<id>` and its credential reference; `dsh_deepseek` clears the official section as today. Credential refs of untouched providers are preserved.

## E. Settings, i18n, presets

- Directory override: add a DSH row to `DirectorySettings`/`useDirectorySettings` (remove `deepseek-harness` from the `Exclude`), add `get_dsh_override_dir` to `settings.rs`, and honor the override in `deepseek_harness_config::get_dsh_home` (precedence: CC Switch override, then `$DSH_HOME`, then `~/.dsh`).
- i18n: add a `dsh` section to `en.json` and `zh.json` covering the provider-page title, form labels, toasts, and mutation errors, following the existing `pi.*` structure.
- Presets: extend `deepseekHarnessProviderPresets.ts` with a generic OpenAI-compatible preset alongside the official DeepSeek one.
- First-run prompt auto-import loops in `app_config.rs` include DSH so `~/.dsh/AGENTS.md` is imported like other apps.

## Safety

- All existing DSH safety rules still apply: YAML parsed structurally, atomic writes, unrelated namespaces preserved, version-1 credential layout, `records` untouched, no secret values in logs or UI.
- Session parsing never follows symlinks out of `~/.dsh/sessions` and bounds file size before decompression.
- The directory override is validated as an absolute path and only affects CC Switch's own reads/writes; it never moves user files.

## Testing

- Rust: fixture-based tests for session scan/load/delete (generated zstd JSONL in a temp dir), source-aware `remove_from_live_config`, speed-check base-URL resolution, `set_dsh_current_model` validation, and directory-override precedence. All run under `pnpm test:dsh`.
- Frontend: tests for the dsh query hooks, `isProviderInConfig` behavior, hidden MCP/Skills entries, and session filter inclusion.
- Full gates before release: `pnpm test:dsh`, TypeScript, rustfmt, and `pnpm build:dsh:macos` with mounted DMG verification.
