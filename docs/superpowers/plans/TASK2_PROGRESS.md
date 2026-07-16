# Task 2 Progress — Provider credentials DB authoritative

Date: 2026-07-16 (updated, green)

## Done
1. Fixed earlier proxy/provider tests (skip-existing + dynamic ports).
2. `ProviderMutationCoordinator` + CAS DAO + audit/recovery scaffold + unit tests.
3. `ProviderMutationRequest.skip_live_projection` added.
4. `project_live_credentials` helper for proxy takeover projection.
5. **Exclusive same-id update path** now goes through CAS mutate:
   - reads `expected_revision` from DB
   - auto-confirms credential fields (provider-edit UI intent)
   - `skip_live_projection` when proxy takeover / live backup present
   - post-mutate side-effects restored from HEAD:
     - ClaudeDesktop always `write_live_with_common_config`
     - Claude/Codex under takeover: `update_live_backup_from_provider` +
       `sync_*_live_from_provider_while_proxy_active`
     - non-takeover exclusive current: MCP reproject
6. **Switch path**: removed Live→DB credential backfill (ADR 0001).
   Keeps common-config sync from live only.
7. `pub(crate) mod live` for security module access.
8. Import-from-live tests aligned with skip-existing (DB wins).
9. ClaudeDesktop takeover test uses ephemeral `listen_port: 0` +
   asserts against bound port from DB.

## Test status (2026-07-16)
- `services::provider_security` — green (17 unit tests)
- `services::provider` — green after exclusive CAS + takeover side-effects
- `services::proxy` — green (50)

## Remaining (optional / follow-up)
- Additive / rename / OMO update paths still use older `save_provider` style
  (not exclusive same-id CAS). Migrate when ready.
- Unused import warnings in `provider_security/mod.rs` and `services/mod.rs`
  (non-blocking).

## Design notes
- DB is SSOT for credentials (ADR 0001). Live is a projection.
- Live import must not overwrite existing DB providers (skip-existing).
- Proxy takeover owns Live placeholders; update path updates backup +
  proxy-safe live sync, never writes real credentials into live under takeover.
- Provider-edit UI auto-confirms credential fields so CAS mutate can apply
  apiKey/baseUrl without a separate confirmation step.

## Commands
```
cd src-tauri
cargo test --lib services::provider_security -- --test-threads=1
cargo test --lib services::provider:: -- --test-threads=1
cargo test --lib services::proxy:: -- --test-threads=1
```
