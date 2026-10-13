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
  `device_id`, optional user-chosen name, committed/pending writer nonce, `seq`.
- Stored in the database, not `settings.json`: a lost or fresh database always
  starts a **new** identity and can never overwrite the history a previous
  database published. No hardware identifiers are used.
- A copied CC Switch data folder (or a restored backup) shares the identity;
  the writer nonce detects it. The user chooses **Use a new identity** (the old
  directory remains as that installation's history) or **Take over** (only when
  the other copy no longer runs).

## Local storage (schema v21)

| Table                          | Purpose                                                       |
| ------------------------------ | ------------------------------------------------------------- |
| `usage_sync_identity`          | This installation's identity and writer nonces                |
| `usage_sync_devices`           | Remote device status, coverage, last upload/fetch, last error |
| `usage_sync_remote_partitions` | Import cursor: object SHA-256 per (device, partition)         |
| `usage_sync_remote_logs`       | Read-only cache shaped like `proxy_request_logs`              |
| `usage_sync_remote_rollups`    | Read-only cache shaped like `usage_daily_rollups`             |

- Migration v20 → v21 only creates these tables (idempotent DDL shared with
  fresh databases); existing data is untouched, and the usual pre-migration
  backup runs.
- All five tables are in `SYNC_SKIP_TABLES` (rows never enter `db.sql`) and
  `SYNC_PRESERVE_TABLES` (a snapshot download keeps the local rows).
- Restoring an older full backup recreates empty tables; the next sync
  re-imports remote devices and, because the identity row is gone, publishes
  under a new device id (see limitations).
- Settings (`usageSync` in `settings.json`, device-local): `enabled`,
  `autoSync`, `backend` (`webdav`|`s3`, stored explicitly), `detailLevel`
  (`records`|`daily`), `intervalMinutes` (5–1440), `status`. Saved only through
  the dedicated commands so a stale full-settings save cannot roll it back.

## Statistics semantics

`UsageDeviceScope` (`Local` | `All` | `Device(id)`) is threaded through summary,
per-app summary, trends, first-usage date, provider stats, model stats, request
logs and request detail. The old method names remain as local-scope wrappers.

- **Local**: unchanged SQL.
- **Device(id)**: that device's cached rows only (its own de-duplication was
  applied on the source).
- **All**: `UNION ALL` of local rows and remote rows. Local rows keep the
  session/proxy de-duplication (keyed by `request_id` instead of `rowid`
  inside the union); remote records are dropped when
  - this device holds the same `request_id` (local details or the Pi
    `session_usage_dedup` ledger) → counted once, as local;
  - another remote device holds it → the lexicographically smallest device id
    keeps it.
- Provider names come from the source device (`providerName` in the segment).
- Daily-only devices contribute like local rollups: only fully covered days are
  counted (the UI says so).
- Time handling: records carry absolute UTC timestamps, so they are bucketed by
  the viewer's own local calendar (DST-correct). Day-level data (rollups and
  daily mode) is labelled with the source device's local date.
- Remote rows are never passed to the local cost back-fill (which writes to
  `proxy_request_logs`); they keep the source device's estimate.
- Provider limit checks and the per-session reader summary stay local.

## Privacy

Shared in `records` mode: request id, data source, timestamp, app, provider id
and name, model names, token counts, cost estimates, cost multiplier, status
code, latency / first-token / duration, streaming flag.
Note that some CLI log sources build their request id from the CLI's own
session/message identifiers (e.g. `gemini_session:{session}:{message}`).

`daily` mode shares only per-day aggregates: no request ids, timestamps or
timings. Cross-device de-duplication is then impossible and such devices have
no remote request log.

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

- Cross-device de-duplication needs identical request ids. Records that share
  a session but get device-specific ids are counted per device, and so are
  duplicates once both devices have rolled the day up (after 30 days).
- Session-vs-proxy duplicates across _different_ devices are not merged (no
  provable identity); each device's own heuristic still applies locally.
- Day-level data is attributed to the source device's calendar date and only
  counts for fully covered days; hour-level trends show only record-level data.
- After restoring an old backup or losing the database, the installation
  publishes under a new identity; the old entry stays (hide it in Devices).
  Overlapping record-level history is still de-duplicated by request id.
- Provider limit checks remain per device.
