# Cross-device usage validation

Base: upstream `479964f2ef59e291f563218ac674a1fd82dcdbd3` (4.0.10).
Branch: `feat/cross-device-usage`; related issue: #8146.
Latest source validation: 2026-10-13, macOS / aarch64, repository Rust toolchain
and pnpm lockfile (Rust 1.95, pnpm 10.12.3, local Node 26.10.0).
Test fixtures use isolated `CC_SWITCH_TEST_HOME` directories,
in-memory databases and a synthetic localhost WebDAV server. No production
database, session corpus or WebDAV endpoint was used for these checks.

| Check | Result |
| --- | --- |
| Full frontend unit suite | 216 files, 2627 tests passed |
| Device components and adapters with `TZ=Pacific/Honolulu` | 15 passed |
| Full `cargo test -- --test-threads=1` | 3842 passed (3655 library + 187 integration), 0 failed, 9 existing tests ignored |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo fmt --check` | Passed |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed; existing bundle-size warning remains |
| `git diff --check` | Passed |

The ignored tests remain ignored; no real user corpus was substituted.
CI uses `cargo nextest run --profile ci` on Linux, macOS and Windows,
plus a separate Windows/WSL2 filesystem job; those CI jobs were not run locally.
Frontend CI uses Node 20, so the local results do not reproduce its environment.

The feature commit was rebased onto the base above after fetching 72 upstream
commits. Conflicts in Cargo.lock, ModelStatsTable and statsColumns were resolved
by retaining upstream's icon dependency, left-aligned table styles and
first-token statistics while adding snapshot support. The PR diff retains only
the original 34 feature files, with all upstream locale keys and values preserved.

## Compatibility checks

- Local dashboard uses only device status; a failed status request does not
  replace or block local statistics or request logs. The default calendar keeps
  rolling 24-hour selection, time inputs and live-end controls.
- Original model/provider success-rate formatting is retained for numeric values;
  unavailable snapshot response metrics render a dash. The local model table
  retains upstream first-token values and prefers exact over estimated medians.
- Device scopes share the existing hero, trend/heatmap, calendar, table and sheet;
  metric expansion and selected trend metric persist across scope changes.
- An installation that never enabled usage sync performs ordinary pruning without
  generating receipts or a device identity. Opted-in archive capture scans only
  logs about to be pruned; invalid recent rows do not block old-log cleanup.
- Receipt capture failure rolls back opted-in pruning. Codex rebuild removes only
  Codex receipts. Configuration synchronization preserves local identity, ledger
  and remote cache; remote imports never enter local logs or rollups.
- HTTP tests cover conditional concurrent writers, retry, unchanged uploads,
  identity/hash/version validation and retention of last-good remote data.
  Remote exchange does not emit a local-log-write notification.
- All pre-existing locale keys/values are unchanged; new text is confined to
  `usage.devices` in en, zh, zh-TW and ja. UI primitives, global styles, original
  usage query/API implementations and provider-management code are unchanged.

## Manual validation and remaining coverage

Before the compatibility audit, local/device scopes, UTC calendar, data-source
sheet and 650px compact layout were inspected using synthetic browser fixtures.
An arm64 macOS app was built and installed at that stage. The subsequent audit
fixes have source tests, Clippy and renderer-build verification; no new app or ZIP
was built as part of that audit.

Before the upstream rebase, the contributor reported testing this change on
Windows, Mac Mini and MacBook with their configured WebDAV service. Data display and cross-device
synchronization appeared to work normally in those tests. This is contributor
manual validation, separate from the automated checks above.

Remaining coverage includes other WebDAV server implementations, large datasets
and notarization. S3 usage transport is outside this feature's scope.
The fake server exercises real local HTTP GET/PUT/MKCOL/PROPFIND and ETag
preconditions; it cannot establish compliance of production servers.
