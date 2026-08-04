# Codex Cache Review Fixes Design

## Goal

Resolve the three findings from the fourth Codex review of PR #6035 without creating a cache-refresh oscillation or weakening official-login behavior.

## Root causes

1. `models_cache.json` is both an upstream-owned official cache and a cc-switch-owned rendered catalog. After cc-switch replaces its models, preserving the upstream `etag` makes the rendered catalog look like a clean official baseline.
2. The cache refresher follows the global proxy server lifecycle instead of the Codex per-app takeover lifecycle. It can therefore publish proxy-only slots while Codex bypasses the proxy.
3. Official model discovery reads only `tokens.access_token`; `tokens.account_id` is lost even though ChatGPT workspace requests may require it.

## Design

### Official baseline and rendered cache ownership

Add a cc-switch sidecar file named `cc-switch-official-models-cache.json`. It stores the last non-empty, non-cc-switch-owned `models_cache.json` observed before an official-login merge.

Because builds before this fix could preserve an official `etag` on a rendered custom cache, the first non-cc-switch cache seen without a sidecar is not trusted immediately. Its `etag`/`fetched_at` fingerprint is recorded as `awaiting_official_refresh`; official-login aggregation removes that uncertain live cache and waits for Codex to fetch a different official snapshot. Only the different snapshot becomes the clean baseline. Once the sidecar is established, every cc-switch rewrite observes and saves a newer clean official live cache before overwriting it.

Switching back to official login never leaves a fresh cc-switch-owned cache in place. With an established sidecar it restores or merges that clean baseline; without one it removes the rendered live cache so Codex immediately refetches instead of waiting for the 300-second TTL.

When official login and custom mappings are enabled:

- If the live cache is official-owned and non-empty, refresh the sidecar from it.
- Otherwise, load the sidecar.
- If neither source supplies a reliable official baseline, leave the live cache untouched so Codex can fetch one.
- Merge custom entries into the clean baseline, then write the rendered live cache with a fresh `W/"cc-switch-..."` etag.

All aggregate-mode and regular-provider cache rewrites also receive a fresh cc-switch etag. No cc-switch rewrite may preserve a server etag.

This keeps a clean official catalog separate from the rendered catalog, preserves same-slug official entries in the baseline, and allows a later genuine Codex refresh to replace the sidecar.

### Refresher lifecycle

Before each background refresh, read the Codex row from `proxy_config`. If its `enabled` flag is false, keep the refresher alive but skip the write so later Codex takeover can resume without restarting a global proxy that another app still uses. The existence of a current Codex provider or another application's active proxy is not sufficient. Publication acquires the same per-app switch lock used by takeover and hot switching, then re-reads both the enabled flag and current provider under that lock before writing.

The synchronous cache write performed during a successful Codex takeover remains unchanged because it already occurs inside the takeover path.

The official sidecar records when a distinct official `etag`/`fetched_at` snapshot was captured. Reusing the same snapshot never extends that timestamp. After 300 seconds, official-login aggregation moves the sidecar back to `awaiting_official_refresh` and removes the rendered live cache, allowing Codex to fetch newly released or retired official models instead of having the 240-second refresher perpetually renew stale data. A missing timestamp is a legacy sidecar and receives a current capture time once; a malformed, non-string, or abnormally future timestamp is quarantined instead of becoming immortal.

### Mapping order and compatibility aliases

When the configured current Codex provider is the built-in official provider, resolve `codexCustomModels` before consulting the normal failover queue. A mapped slot uses only its explicitly bound provider, even if the official provider is circuit-open or omitted from the queue; an unmapped aggregate-mode slot is rejected before it can leak to an unrelated failover provider.

Exact public slot names remain valid in both official-login and aggregate modes. Legacy `upstreamModel` alias matching is limited to aggregate mode because official-login mode also exposes genuine official slugs, which must never be captured by a custom entry's upstream alias.

### Official authentication context

Replace the access-token-only reader with a credentials reader returning:

```rust
pub(crate) struct CodexAuthCredentials {
    pub access_token: String,
    pub account_id: Option<String>,
}
```

Both values come from the same `auth.json` snapshot and are trimmed. Official model fetching accepts `Option<&str>` and adds `chatgpt-account-id` only when present. Managed OAuth continues to require its known account ID.

## Error handling

- Failure to read or parse the official baseline sidecar follows the existing cache behavior: treat it as unavailable and allow Codex to recover by fetching a clean catalog.
- Failure to persist a newly observed official baseline is returned as an `AppError`; do not publish a merged cache that cannot be reproduced safely.
- Missing account ID remains supported for personal accounts; only the access token is mandatory.

## Tests

Add regression tests proving:

- Aggregate and regular-provider rewrites replace an official etag with a cc-switch etag.
- Official-login merge saves the clean official baseline, writes a cc-switch-owned rendered cache, and can repeat the merge from the sidecar.
- A later genuine official cache refresh replaces the stored baseline.
- A pre-sidecar cache with an official-looking etag is quarantined until a different official snapshot is observed.
- Cache refresh eligibility follows the Codex per-app proxy flag, not another app's flag.
- A disabled refresher waits and later resumes, while the shared switch lock prevents post-disable or stale-provider publication.
- An expired or invalid-time official baseline is quarantined, repeated observation of the same fingerprint does not extend its TTL, exact 299/300/301-second boundaries are deterministic, and a distinct official snapshot replaces it.
- A mapped aggregate slot resolves before failover selection, while an unmapped aggregate slot is rejected even when another failover provider is available.
- Official-login requests never match a custom route through `upstreamModel`; aggregate-mode legacy aliases remain supported.
- Credential parsing returns the trimmed access token and optional account ID.
- The official models request includes `chatgpt-account-id` when present and omits it when absent.

## Scope

No UI changes, network retry changes, provider routing changes, or database schema changes are included.
