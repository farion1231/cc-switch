# Codex temporary-session usage

Codex side chats are ephemeral forks: they run in memory and do not create the
JSONL files used by the regular Codex usage importer. CC Switch can receive their
usage through Codex's opt-in OpenTelemetry log exporter.

## Enable the local collector

Add this to the Codex common configuration in CC Switch and enable that common
configuration for the relevant providers. For a standalone Codex installation,
add it to its `config.toml` instead:

```toml
[otel]
log_user_prompt = false
exporter = { otlp-http = { endpoint = "http://127.0.0.1:4319/v1/cc-switch/codex/logs", protocol = "json" } }
```

Restart CC Switch and Codex after changing this configuration. CC Switch starts
the receiver only when the configured exporter has this path, the JSON protocol,
and an explicit port on `127.0.0.1`. Another free port can be used. A running
collector responds at `http://127.0.0.1:4319/cc-switch/codex-otel/health`.

If an OTel exporter is already configured for another destination, do not replace
it unintentionally. This integration accepts one local OTLP/JSON destination; it
does not configure forwarding to other collectors. Remove this exporter setting
and restart both applications to disable the integration.

## Accounting

- Only `codex.sse_event` events with kind `response.completed` and valid usage
  counts are imported. Input includes cache reads and writes; reasoning output
  is already part of output and is not added again.
- Usage appears under **Codex (Temporary)**, with source `codex_otel`, the model,
  timestamp, tokens, configured Fast tier and reasoning effort. It covers
  non-persisted Codex sessions, including side chats; internal `codex-auto-review`
  requests are excluded.
- Codex 0.153.4 also emits an input-only completion for `generate=false`
  WebSocket prewarming. For GPT-5.x/GPT-6 reasoning models, the verified signature
  has zero output, cache and reasoning tokens, with no service tier, effort or
  first-token timestamp. These events are retained as `codex_otel_prewarm` and
  excluded from request lists, statistics and rollups. Zero output alone is not
  sufficient to exclude a request. Other versions and model families retain the
  existing behavior until their telemetry format is verified. Previously stored
  rows lack the version and first-token fields, so they are not automatically
  reclassified from token counts alone.
- The local Codex `state_5.sqlite` thread index is read without modification.
  Persisted threads remain on the existing JSONL import path, even before their
  first file scan. If that index is unavailable, sanitized usage stays queued
  until the next incoming batch or scheduled/manual session scan.
- Costs use the configured model rates and fixed Fast factors. They have no
  automatic long-context surcharge. OTel's tier is the requested tier, so these
  are local estimates rather than confirmed provider charges.
- Retries have stable event IDs. A durable marker prevents replay from restoring
  a deleted or rolled-up request. Existing proxy fingerprint matching prevents
  duplicate accounting, including when proxy logs arrive later. Persisted session
  rows also take precedence over telemetry rows for the same thread. These rules
  apply to statistics and daily rollups as well as the request list.

The receiver binds only to loopback, rejects browser-origin submissions, and
accepts bounded JSON batches. It retains only sanitized accounting fields; raw
telemetry bodies, prompts, tool output, account details and credentials are not
saved. An exporter may still transmit other event types to the local receiver;
those events are discarded. Accepted events can be at most seven days old or
five minutes in the future, and replay markers are retained for eight days.
Pending events and replay markers stay local during WebDAV/S3 configuration
sync and are preserved when importing a remote configuration snapshot.

CC Switch must be running to receive events. Codex batches asynchronously; an
outage longer than its retry period can lose events. This integration cannot
retroactively recover temporary-chat usage that was never exported.

## Verification

Regression tests cover real OTLP field shapes, input/cache semantics, fixed Fast
pricing, replay protection, persisted-thread exclusion, late proxy/session
deduplication, pending-queue recovery, and invalid or oversized HTTP requests.

An optional integration test launches an installed Codex executable with an
isolated home, an ephemeral session, a local mock Responses API and a local CC
Switch collector. It uses a synthetic key and no real model API:

```powershell
$env:CC_SWITCH_CODEX_OTEL_PROBE_BIN = 'C:\path\to\codex.exe'
cargo test --lib real_ephemeral_codex_exports_into_collector -- --ignored
```

Sources: [Codex app-server](https://learn.chatgpt.com/docs/app-server) and
[OpenTelemetry configuration](https://learn.chatgpt.com/docs/config-file/config-advanced).
