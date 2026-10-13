# Optional cross-device usage over WebDAV

Initially implemented against upstream `df40d53c36b604e044b4d5b9091aeb961aa56990`
(4.0.8), then rebased onto `479964f2ef59e291f563218ac674a1fd82dcdbd3` (4.0.10).
Related issue: [#8146](https://github.com/farion1231/cc-switch/issues/8146).
The initial upstream branch inventory was fetched and inspected before implementation:
`feat/usage-dashboard-enhance`, `feat/usage-dashboard-refinement`,
`feat/usage-improvements`, `feat/usage-model-extraction`, and
`webdav-followup-auto-sync-limit` contain local usage or configuration sync work,
not device-separated usage replication. Main still skips/preserves usage tables
in `database/backup.rs`; `sync_protocol` triggers configuration changes only.

## Use

1. Configure and enable the existing WebDAV connection on every device. Use the
   same remote root and profile. Configuration auto-sync is independent.
2. In Usage, open **Data sources**, find **Device usage sync**, give the
   installation a readable name, enable usage sync, and save. This is off by
   default. No endpoint is hard-coded.
3. Select **This device**, **All devices**, or a specific downloaded device.
   The local view retains existing request details, pricing and provider stats.
   The cross-device view provides whole-date/app/model filters, token components,
   recorded USD costs and an accounting-basis filter for proxy usage or estimates.
4. The dedicated worker exchanges usage every 60 seconds, including on startup.
   **Sync usage now** is also available. A failed exchange is retried on later
   ticks without needing another request; its error and last-good cache remain.

Cross-device dates are whole UTC days. Existing historical daily rollups only
contain device-local calendar dates; those dates are preserved and their rows
are unverified. They cannot be accurately rebucketed into UTC or sliced into
partial days. The local view retains its existing date/time semantics.

## Isolation and provenance

`usage_sync_meta` contains a UUID generated locally, readable name, opt-in state,
revision and status. `usage_sync_events` is a minimal durable usage ledger;
`usage_sync_remote` stores replaceable remote snapshots keyed by transport
namespace and device UUID. All three tables are excluded from configuration
export and preserved during configuration import. They never feed
`proxy_request_logs`, `usage_daily_rollups`, session cursors, or provider limits.
Thus downloading configuration neither clones the installation identity nor
re-uploads another device's statistics. MCP environments, local paths and
provider selection retain the existing configuration-sync behavior.

A full SQL backup intentionally remains a full backup. Copying a full database
or a home directory to another installation can clone its device UUID. This
is not a supported provisioning method. Conditional writes prevent simultaneous
head overwrites, and revision conflicts are surfaced; they cannot prove that
an entire installation has not been cloned. Provision devices independently.

At first opt-in, existing daily rollups are captured once as unverified baseline
rows, with device-scoped identities. Existing retained logs are also unverified.
Previously merged databases with lost device provenance must not be used as
multiple devices' baselines: their historical totals could overlap.

New proxy rows with explicit `input_token_semantics` and timestamps after first
opt-in can be verified as local proxy observations. Legacy semantics, session
imports, and historical data remain unverified. This is deliberately conservative:
request IDs establish identity, not globally unique billing provenance.

The snapshot first applies upstream `effective_usage_log_filter` to suppress
local session/proxy overlap. For session copies, identity is SHA-256 over a
versioned, unambiguous JSON tuple of application, data source and request ID;
proxy identity also includes the local UUID. Claude uses provider message IDs;
Codex uses the canonical root thread/event index from the upstream importer;
Gemini and OpenCode use their session/message identities; Pi uses the importer's
request hash (with its durable `session_usage_dedup` rules). Copies with identical
identity and normalized usage count once. Different observations of one identity
(including different prices) are excluded and reported as conflicts. Distinct
IDs are not merged merely because token counts happen to match.

Upstream `session_usage_dedup` primarily retains Pi request/semantic identities
after pruning, without quantities or device provenance. It cannot reconstruct
old globally unique totals. Codex positional identities may change after a
rewrite, and proxy/session correlation across different devices is not provable.
Consequently the session subtotal is always visibly unverified; it can still
contain overlap with proxy observations, or rewritten sessions with new IDs.
Never add it to the verified subtotal and call the result exact.

Before log pruning, only events about to be pruned are captured inside the rollup savepoint
and marked archived in that same transaction. Later snapshots use the ledger,
not ledger plus new daily rollups. Live corrections replace events; ordinary
live deletions remove receipts. Codex rebuild clears Codex receipts alongside
its existing log and rollup reset. Disabling networking keeps the ledger and
continues archiving identities so resuming does not lose intervening usage.
If receipt capture fails after first opt-in, the pruning savepoint rolls back
instead of deleting data that cannot later be synchronized. A never-enabled
installation skips capture and archival marking. Recent retained logs are not
scanned by the archive hook.

## Token and cost semantics

Capture uses the existing `fresh_input_sql` helper, including the explicit
legacy / total / fresh semantics and the shared cache-inclusive application
list. Total-inclusive input subtracts both cache reads and writes; legacy input
follows upstream's read-only adjustment. The wire format always carries fresh
input, output, cache read, and cache write separately. It never normalizes a
received value again. Total tokens are their sum. Costs are the recorded USD
values, aggregated with `rust_decimal`; prices are not transferred or recomputed
using another device's pricing rules.

## Wire protocol and consistency

Separate namespace, independent of the configuration protocol/DB compatibility:

```
<remoteRoot>/<profile>/usage-v1/devices/<uuid>/
  <sha256>.json  # immutable minimal usage snapshot
  head.json      # format, version, uuid, revision, hash
```

There is no shared writable device index. WebDAV `PROPFIND Depth: 1` discovers
UUID directories. Hrefs are restricted to direct children of the same server
and root; external URLs, traversal and nested paths are ignored. XML DTDs are
rejected. The listing is bounded at 1 MiB and 100 devices.

A snapshot has `format: cc-switch-usage`, `version: 1`, device UUID/name,
monotonic revision, capture time, and usage events. It contains no API keys,
provider configs/IDs, raw request/session IDs, conversation content, errors,
MCP environments, filesystem paths or session scan cursors. Historical identity
hashes include the local rollup dimensions without sending those dimensions.

Publication uploads the content-addressed artifact first, then conditionally
publishes the head using `If-Match` (existing ETag) or `If-None-Match: *` (new
head). An in-process mutex serializes manual/automatic exchanges. A reader
fetches the head and exactly its immutable artifact, verifies hash, version,
UUID/revision and event validity, then replaces one device's cache in one SQL
statement. Older revisions are ignored; an equal revision with different data
is rejected. Unsupported, missing or corrupt snapshots retain last-good data
and report an error. One incompatible device does not prevent importing other
valid devices.

The server must honor conditional PUT and supply ETags on GET. There is no
unsafe fallback when an existing head lacks an ETag. No guarantee can be made
for a server that ignores HTTP preconditions. A torn new artifact/head cannot
replace a previously valid local cache. An interrupted artifact upload is retried
with a new revision; unreferenced objects are harmless.

The 60-second worker transfers only usage, never `db.sql` or skills. It compares
the local usage/name signature and current published hash; unchanged usage skips
artifact/head PUTs but still downloads device updates. Changed usage publishes
a replaceable **full minimal snapshot**, capped at 64 MiB / 200,000 events per device and 128 MiB of remote cache per namespace.
There are no delta acknowledgments or increment-only totals. This enables
idempotent retries, deletions and price corrections without addition drift.
Old immutable objects are not automatically garbage-collected; an administrator
may remove objects other than the hash referenced by each device's head when no
reader/upload is active. Cache and local ledger capacity remain finite practical
limits; exceeding limits fails visibly instead of dropping history.

## Scope and limitations

- WebDAV only in this change. S3 configuration synchronization remains unchanged.
  The versioned snapshot/head types are transport-independent, but S3 usage
  support still needs object discovery and conditional-head publication; adding
  usage to its configuration artifact set would break isolation.
- All-device totals cover **known downloaded devices**, at their displayed last
  snapshot times. They are not a claim that every device is online/current.
  An offline or removed device retains its last-good cache. There is no remote
  deletion/device-forget UI in this first version.
- Device-local historical dates, unknown provenance, positional session rewrites,
  and cross-device proxy/session overlap cannot become precise automatically.
- No request content or local paths are available in remote views. Provider
  comparison and request details remain local; cross-device grouping is by date,
  application and model.
- The ledger starts at opt-in. It cannot recover identities pruned earlier.
- The contributor reported normal data display and synchronization when testing
  this change on Windows, Mac Mini and MacBook with their WebDAV deployment.
  The rebased source has separate automated validation; see
  `cross-device-usage-validation.md` for results and coverage limits.

## Validation

Use `CC_SWITCH_TEST_HOME` for Rust tests. Never point tests at real user config.

```
CC_SWITCH_TEST_HOME=/tmp/cc-switch-usage-fixtures cargo test usage_sync --lib
CC_SWITCH_TEST_HOME=/tmp/cc-switch-usage-fixtures cargo test usage_listing --lib
CC_SWITCH_TEST_HOME=/tmp/cc-switch-usage-fixtures cargo test usage_rollup --lib
pnpm exec vitest run tests/components/DeviceUsageDashboard.test.tsx tests/lib/deviceUsage.test.ts
pnpm typecheck
cargo fmt --check
cargo clippy -- -D warnings
```

Tests use in-memory databases and a synthetic localhost WebDAV server, covering
retry after failed head publication, immutable snapshots, unchanged upload
suppression, conditional concurrent writers, last-good cache retention,
namespace isolation, idempotent/stale/conflicting revisions, normalization,
archive invariance, copied sessions, provenance and sensitive-field exclusion.

## Device dashboard presentation

Device scopes are filters in the existing `UsageDashboard`. There is one header,
application selector, model menu, date picker, hero, trend/heatmap area and tabbed
table area.

A pure adapter maps snapshot buckets to the data shapes already used by
`UsageHero`, `UsageTrendChart`, `UsageHeatmap` and `ModelStatsTable`. Those components
accept optional snapshot data and disable their local queries when supplied.
Their presentation and interactions are shared; metric expansion and chart
selection persist while switching devices. `ModelStatsTable` supports daily
buckets for the usage-breakdown tab as well as model grouping. Unknown success
rates and response speeds display a dash. Remote scopes cannot select local-only
provider statistics or pricing settings; explanatory tips accompany those tabs.

`UsageDateRangePicker` adds a UTC-day mode on the existing calendar and preset UI.
It excludes rolling 24-hour selection, time inputs and live-end controls because
snapshot data has only whole-day precision. Its default local/time mode remains
unchanged. UTC wall dates round-trip through the calendar without depending on
the host time zone. Selecting all dates uses the existing heatmap.

Device selection is a small menu with the original filter-menu sizing and check
indicators. Sync settings use the existing `SourceCard` inside
`UsageDataSourcesSheet`; there is no additional page toolbar or settings sheet.
The original header sync action synchronizes device snapshots in remote scopes
and retains its local session-scan behavior in the local scope. Query refresh
uses the original dashboard interval setting.

The local scope calls `get_usage_sync_status` for settings and cached device
labels only; it does not capture usage, write a snapshot revision, or aggregate
remote events. Device reports are requested only in device scopes. Remote
exchanges do not emit the local `usage-log-recorded` event or invalidate local
statistics. Actual local log events also invalidate device reports so their
local contribution stays current.

The UI labels verified observations as proxy usage and unverified observations
as estimates. Snapshot provenance, event identities and the v1 format do not
change. The original table toolbar hosts the provenance filter, which drives
metrics, charts and tables together. Totals containing estimates remain explicit.
