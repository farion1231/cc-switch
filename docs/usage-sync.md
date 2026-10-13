# Cross-device Usage Sync

Usage Sync lets every CC Switch installation publish its own usage statistics
to the WebDAV or S3 storage that is already configured for Cloud Sync, and
import the usage of the user's other devices. The Usage page can then show
**This device**, **All devices**, or **one specific device**.

It is opt-in (off by default) and fully separate from the configuration
snapshot sync (`db.sql` + `skills.zip` + `manifest.json`).

## Goals and non-goals

- Reuse the existing WebDAV / S3 connection settings and HTTP primitives.
- Never write remote data into the authoritative local usage tables
  (`proxy_request_logs`, `usage_daily_rollups`).
- Never share a SQLite file, prompts, responses, credentials, session files,
  error messages or file paths.
- Keep the existing local statistics byte-for-byte identical: the local scope
  produces the same SQL as before.
- Do not merge records across devices by time/token similarity; only by a
  provable stable identity.
- Costs stay estimates computed by each device from its own model prices.

## Remote layout

```
{remote_root}/
├── v2/db-v6/{profile}/…                      ← configuration snapshot (unchanged)
└── usage-v1/{profile}/devices/{device_id}/
    ├── manifest.json                         ← written last
    └── parts/{partition}-{hash16}.json.gz    ← immutable, content-addressed
```

- Every device writes **only** below its own `devices/{device_id}/` prefix, so
  concurrent uploads from different devices never touch the same object and no
  shared index file exists.
- Devices are **discovered** by listing `devices/`:
  WebDAV `PROPFIND Depth: 1`, S3 `ListObjectsV2` with `delimiter=/` and
  continuation-token pagination.
- `profile` and `remote_root` are taken from the selected backend's settings,
  so Cloud Sync profiles also partition usage.

### Partitions

| Key            | Content                                                                                                                                    |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `d-YYYY-MM-DD` | One local calendar day (source device time zone) of detail data: request records (`records` mode) or that day's aggregates (`daily` mode). |
| `m-YYYY-MM`    | The device's `usage_daily_rollups` rows of one month (history older than the 30-day detail window).                                        |

A local row is either a detail row or part of a rollup, never both, and maps to
exactly one partition. When the source device rolls a day up, the `d-` partition
disappears from its manifest and the rows appear in the `m-` partition; readers
apply both changes in one transaction, so totals never double count or dip.

### Manifest (`format: cc-switch-usage-sync`, `version: 1`)

```jsonc
{
  "format": "cc-switch-usage-sync",
  "version": 1,
  "deviceId": "0f8fad5b-…",
  "deviceName": "Work PC",
  "platform": "windows",
  "appVersion": "4.0.10",
  "seq": 42, // monotonic per device
  "writerNonce": "…", // random per write, see "Device identity"
  "updatedAt": 1760000000, // unix seconds of this upload
  "detailLevel": "records", // or "daily"
  "utcOffsetMinutes": 480,
  "coverage": { "firstDate": "…", "lastDate": "…", "detailFrom": "…" },
  "partitions": {
    "d-2026-10-13": {
      "object": "parts/d-2026-10-13-1a2b3c4d5e6f7a8b.json.gz",
      "sha256": "<sha256 of the gzip bytes>",
      "size": 1234,
      "contentSha256": "<sha256 of the canonical JSON>",
      "logs": 120,
      "rollups": 0,
    },
  },
  "retired": [{ "object": "parts/…", "retiredAt": 1759990000 }],
}
```

### Segment (`format: cc-switch-usage-segment`, `version: 1`)

Gzip-compressed canonical JSON (fixed field order, rows sorted):

- `logs[]`: `requestId, dataSource, createdAt, appType, providerId,
providerName, model, requestModel?, pricingModel?, inputTokens, outputTokens,
cacheReadTokens, cacheCreationTokens, inputTokenSemantics, *CostUsd,
costMultiplier, statusCode, latencyMs, firstTokenMs?, durationMs?, isStreaming`
- `rollups[]`: the `usage_daily_rollups` shape plus `providerName`.
- `aliases[]` (`d-` partitions, request-level mode only): `requestId, dataSource`
  of rows the device drops as duplicates of its own rows (a proxy row and the
  session-log line of the same call). They are counted through the other row.
- `ledger[]` (`m-` partitions, request-level mode only): identities of
  rolled-up requests — `requestId, dataSource, kind (0 counted / 1 alias),
date + rollup dimensions, providerName, success, token counts (fresh input),
totalCostUsd` — i.e. exactly what each request contributed to which rollup group.
- `inherited[]` (`m-` partitions, with manifest `lineage`): id-less rollup
  totals copied from a parent identity (see "Copied installations").

`aliases`, `ledger`, `inherited` and the manifest's `lineage` are additions
within protocol v1. They are omitted when empty, so segments without them keep
byte-identical canonical JSON (unchanged content hashes), and older v1 readers
ignore them and keep their previous behaviour.

Raw token counts are shipped together with `inputTokenSemantics`, so the
viewer applies the exact same cache-normalisation SQL (`fresh_input_sql`) as the
source device.

## Write protocol (per device)

1. Export the local partitions from the authoritative tables under the DB lock.
   Detail rows pass through the same session/proxy de-duplication filter the
   dashboard uses, so a remote viewer sees what the device reports for itself.
2. Read the device's own manifest. If its `writerNonce` is neither the last
   committed nor the pending nonce of this database, another installation is
   writing with the same identity → **identity conflict**, uploads pause
   (downloads continue).
3. Upload only partitions whose `contentSha256` changed, as new immutable
   objects named by content hash.
4. Persist a fresh nonce as _pending_, PUT the manifest, then commit the nonce
   and `seq`. A crash between the PUT and the commit is recognised by the
   pending nonce, not reported as a conflict.
5. Superseded objects are listed in `retired` and deleted after a 24 h grace
   period, so a reader that fetched the previous manifest can still finish.
   A failed delete keeps the object listed and is retried.
6. Unchanged data does not rewrite the manifest, except for an hourly
   heartbeat so other devices can tell idle from offline.

Request cost per steady-state sync: 1 GET (own manifest), 1 LIST, one GET per
other device's manifest, plus PUTs/GETs only for changed partitions (typically
today's).

## Read protocol (per remote device)

1. Skip hidden devices; GET the manifest; unchanged SHA-256 → done.
2. Validate format, version, device id (must equal the directory name), keys,
   object names (strict shape, no traversal), checksums and limits
   (manifest ≤ 4 MiB, object ≤ 64 MiB, decompressed ≤ 256 MiB).
3. Ignore manifests with a lower `seq` than already applied (stale caches,
   eventually consistent stores).
4. Download changed partitions, verify size + SHA-256 of the object and of the
   decompressed content, validate every row.
5. In **one transaction**: delete changed/removed partitions of that device,
   insert the new rows, record the per-partition cursor and the device row.
   Any failure rolls back the whole device; previously imported data stays.

A newer protocol version is reported as _Newer version_ and the cached data is
kept; a corrupt or missing object is reported as _Error_ and retried next sync.
Devices whose directory vanished stay cached and are flagged _Not found_.

## Device identity

- `usage_sync_identity` (single row in the local database): random UUIDv4
  `device_id`, optional user-chosen name, committed/pending writer nonce, `seq`,
  and — after a split — `parent_device_id` / `forked_at`.
- Stored in the database, not `settings.json`: a lost or fresh database always
  starts a **new** identity and can never overwrite the history a previous
  database published. No hardware identifiers are used.
- `seq` uses checked arithmetic and must stay within `i64`; a manifest outside
  that range is rejected by readers and an exhausted own sequence is an error,
  never a wrap-around.

### Copied installations

A copied CC Switch data folder, or a restored backup, shares the identity. The
writer nonce detects it and uploads pause (downloads continue). The user
chooses:

- **Use a new identity** (a second computer that keeps running): a new id is
  created with `lineage.parentDeviceId` = the previous id. The copied history
  is then counted once:
  - request-level rows and ledger-covered rollups carry request ids and are
    de-duplicated by the identity rule below;
  - the id-less remainder of the rollups at the moment of the split
    (`usage_daily_rollups` minus ledger-covered contributions, per rollup
    group) is snapshotted into `usage_sync_inherited`, published as
    `inherited[]`, and subtracted from this installation **while the parent's
    data is present** on the viewer. If the parent is hidden (retired or
    replaced), the copy carries that history instead.
  - requests made after the split have new ids and count for whichever
    installation made them. The parent continues normally; an offline parent
    keeps its cached history.
- **Take over** (the other copy no longer runs, e.g. this computer was restored
  from a backup): this installation adopts the remote writer nonce and
  republishes its own data under the same id. If both copies keep running they
  overwrite each other, which the dialog states.

## Local storage (schema v21)

| Table                          | Purpose                                                                                   |
| ------------------------------ | ----------------------------------------------------------------------------------------- |
| `usage_sync_identity`          | This installation's identity, writer nonces, lineage                                      |
| `usage_sync_devices`           | Remote device status, coverage, UTC offset, lineage, last upload/fetch, last error        |
| `usage_sync_remote_partitions` | Import cursor: object SHA-256 per (device, partition)                                     |
| `usage_sync_remote_logs`       | Read-only cache shaped like `proxy_request_logs`                                          |
| `usage_sync_remote_rollups`    | Read-only cache shaped like `usage_daily_rollups`                                         |
| `usage_sync_remote_claims`     | Remote ledger entries and aliases                                                         |
| `usage_sync_remote_inherited`  | Remote inherited (id-less) history with its parent id                                     |
| `usage_sync_remote_multi`      | Ids held by more than one remote device (query gate)                                      |
| `usage_sync_ledger_suppressed` | Materialized losing ledger copies (rebuilt on sync, rollup, hide/unhide, identity change) |
| `usage_sync_inherited`         | This installation's inherited history after a split                                       |
| `usage_rollup_ledger`          | Identity ledger of this device's rolled-up requests                                       |

- `usage_rollup_ledger` is written by `rollup_and_prune` in the same savepoint
  as the rollup itself, for every pruned row (counted or alias), whether or
  not usage sync is enabled, so enabling sync later does not lose the ability
  to de-duplicate history. Entries older than **365 days** are deleted at each
  rollup; the Codex usage rebuild deletes the ledger entries of the rollups it
  deletes.
- Migration v20 → v21 only creates these tables (idempotent DDL shared with
  fresh databases); existing data is untouched, and the usual pre-migration
  backup runs. History rolled up before the upgrade has no ledger entries.
- All eleven tables are in `SYNC_SKIP_TABLES` (rows never enter `db.sql`) and
  `SYNC_PRESERVE_TABLES` (a snapshot download keeps the local rows).
- Restoring an older full backup recreates empty tables; the next sync
  re-imports remote devices and, because the identity row is gone, publishes
  under a new device id (no lineage: hide the old entry).
- Settings (`usageSync` in `settings.json`, device-local): `enabled`,
  `autoSync`, `backend` (`webdav`|`s3`, stored explicitly), `detailLevel`
  (`records`|`daily`), `intervalMinutes` (5–1440), `status`. Saved only through
  the dedicated commands so a stale full-settings save cannot roll it back.

### Storage and transfer cost (measured, 50k rolled-up requests per device)

- Local ledger: ~162 bytes per rolled-up request (≈ 8 MB per 50k), bounded by
  the 365-day retention.
- Remote: ledger entries compress to ≈ 10 bytes each; 3 devices × 50k
  rolled-up requests including rollups ≈ 1.5 MB. Only changed month partitions
  are re-uploaded (normally the current month, once a day after rollup).
- Viewer cache: one `usage_sync_remote_claims` row per remote ledger entry.

## Compatibility

- **Schema bump (v20 → v21)**: like every upstream schema bump, a CC Switch
  build with schema v20 refuses a configuration snapshot (`db.sql`) uploaded by
  this build with "database version too new". This applies to ordinary Cloud
  Sync between devices even when usage sync is never enabled: update all
  devices that share a Cloud Sync profile together. `DB_COMPAT_VERSION` (the
  remote snapshot directory `v2/db-v6`) is unchanged; downloading v20
  snapshots into this build works.
- Downgrading the app after the upgrade is refused by the existing
  "database version too new" check; restore the pre-migration backup instead.
- Usage protocol: unknown fields are ignored within v1; newer-version
  manifests/segments are reported and never overwritten by an older client.
- Lazy table creation without a schema bump was considered and rejected: the
  ledger is written by the core rollup path on every installation, and
  creating it on demand would leave its existence and the snapshot
  skip/preserve lists dependent on whether usage sync was ever enabled.

## Statistics semantics

`UsageDeviceScope` (`Local` | `All` | `Device(id)`) is threaded through summary,
per-app summary, trends, first-usage date, provider stats, model stats, request
logs and request detail. The old method names remain as local-scope wrappers.

- **Local**: unchanged SQL.
- **Device(id)**: that device's cached rows only (its own de-duplication was
  applied on the source).
- **All**: local and remote request-level rows plus local and remote rollups,
  combined by one **identity rule** that gives the same result on every viewer:
  every copy of a request id — a request-level row, a ledger entry, or an
  alias, on any device including this one — is a claim.
  1. If any device holds the id as an alias, every counted copy is dropped:
     the aliasing device already counts the request through its other row.
  2. Otherwise the counted copy of the smallest device id wins.

  Losing request-level rows are filtered out at query time. Losing ledger
  copies (materialized in `usage_sync_ledger_suppressed`) are added as
  **negative rollup rows of exactly their contribution in their own rollup
  group**, so tokens, cost, success counts and dimensions stay exact and no
  day, provider or device is excluded wholesale. Inherited history is
  subtracted the same way while the parent's data is present.

- Freshness: request-level copies are decided at query time; ledger copies are
  re-evaluated after every sync, rollup, hide/unhide and identity change. A
  request imported late on this device after another device already rolled it
  up is reconciled at the next of these events (at most one sync interval).
- Exactness classes shown in the UI:
  - **exact** — request-level data and history with request ids (ledger);
  - **not de-duplicable** — day-level history without ids (rollups made before
    the ledger existed, ledger entries past retention, daily-only sharing). It
    is computed per rollup group (`rollups − ledger − inherited`) and shown per
    device with its date range; it is counted once per device holding it;
  - **not detectable** — a proxy row on one device and the session-log row of
    the same call on another device have different ids; both are counted.
- Provider names come from the source device (`providerName` in the segment).
- Daily-only devices contribute like local rollups: only fully covered days are
  counted (the UI says so).
- Time handling: request records carry absolute UTC timestamps and are
  bucketed by the viewer's own calendar (DST-correct). Day-level data (rollups,
  ledger, daily mode) is labelled with the **source device's** local date; the
  devices sheet shows each device's UTC offset and the All view names devices
  whose calendar differs from the viewer's. Range filters include day-level rows
  only for days fully inside the range.
- Remote rows are never passed to the local cost back-fill (which writes to
  `proxy_request_logs`); they keep the source device's estimate. Costs are
  estimates, not subscription charges.
- Negative rows carry their rollup group's average latency, so removing a
  duplicate leaves the group's average latency unchanged.
- Provider limit checks and the per-session reader summary stay local.

## Privacy

Shared in `records` mode: request id, data source, timestamp, app, provider id
and name, model names, token counts, cost estimates, cost multiplier, status
code, latency / first-token / duration, streaming flag; for rolled-up history
(up to one year) the request id, data source, date, rollup dimensions and
per-request token/cost contribution; ids of locally dropped duplicates.
Some CLI log sources build their request id from the CLI's own session/message
identifiers (e.g. `gemini_session:{session}:{message}`).

`daily` mode shares only per-day aggregates (plus id-less inherited totals after
a split): no request ids, timestamps or timings. Cross-device de-duplication is
then impossible; such devices are reported as "not de-duplicable" and have no
remote request log.

Never shared: API keys, OAuth tokens, credentials, prompts and responses,
session files, error messages, session ids as a field, file paths, the database.

Transport: the UI warns when WebDAV/S3 uses plain `http://` to a non-loopback
host. URLs in errors are redacted; credentials are never logged or shown.

## UI

- Usage page: device filter (shown when usage sync is on or remote data
  exists), **Devices** sheet (status, last upload, coverage, errors,
  hide/show, sync now), notices for daily-only and outdated devices, a Device
  column in request logs and a "recorded on another device" note in request
  detail.
- Settings → Data → Cross-device usage sync: enable, backend, shared detail,
  auto sync + interval, device name, remote location, insecure-transport
  warning, identity conflict resolution, sync now.

## Testing

```bash
cd src-tauri
CC_SWITCH_TEST_HOME="$(mktemp -d)" cargo test --lib usage_sync
# time zones (Unix)
for tz in UTC America/New_York Asia/Kathmandu Australia/Lord_Howe Pacific/Chatham; do
  TZ=$tz cargo test --lib -- usage_sync usage_stats usage_rollup
done
# live servers (opt-in, isolated synthetic data)
USAGE_SYNC_IT_WEBDAV_URL=… USAGE_SYNC_IT_WEBDAV_USER=… USAGE_SYNC_IT_WEBDAV_PASS=… \
USAGE_SYNC_IT_S3_ENDPOINT=… USAGE_SYNC_IT_S3_BUCKET=… USAGE_SYNC_IT_S3_AK=… USAGE_SYNC_IT_S3_SK=… \
cargo test --lib usage_sync::tests_http::live -- --ignored --test-threads=1
```

- `usage_sync::protocol::tests`: keys, object names, traversal, checksums,
  determinism, validation, version gate.
- `usage_sync::tests` (in-memory remote): idempotence, heartbeat, discovery,
  concurrent uploads, offline catch-up, interrupted upload, crash between PUT
  and commit, corrupt/missing objects, per-device failure isolation, shared
  session records, no heuristic cross-device merge, source de-duplication,
  rollup transition, GC (+ retry), cost/model corrections, daily mode privacy,
  sensitive-field leak check, cloned identity + resolution, newer protocol,
  stale manifests, hidden/vanished devices, injection-safe scope, viewer-local
  bucketing, config snapshot isolation, v20 → v21 migration.
- `usage_sync::tests_http`: real reqwest transport against fake WebDAV and
  S3 servers (Basic auth, MKCOL, PROPFIND, ListObjectsV2 pagination, DELETE).

## Known limitations

- Day-level history without request ids (rolled up before the ledger existed,
  older than 365 days, or shared as daily totals only) cannot be
  de-duplicated: it is counted once per device that holds it. The UI shows the
  amount and date range per device.
- A proxy row on one device and the session-log row of the same call on
  another device cannot be matched (different ids); both are counted. Each
  device's own proxy/session heuristic still applies locally.
- Records that share a session but get device-specific ids (for example a
  request id that embeds a device-specific provider id) are counted per device.
- A request imported late after another device already rolled it up may be
  counted twice until the next sync, rollup or hide/unhide (≤ one sync
  interval).
- Day-level data is attributed to the source device's calendar date and only
  counts for fully covered days; hour-level trends show only record-level data.
- After restoring an old backup or losing the database, the installation
  publishes under a new identity without lineage; the old entry stays (hide it
  in Devices). Request-level and ledger-covered history is still
  de-duplicated by id.
- Orphaned objects left by a same-identity write race or an interrupted upload
  are found by a listing of the device's own `parts/` at most once per 24 h per
  process and deleted after the normal 24 h grace period.
- Provider limit checks remain per device.
