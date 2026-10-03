# Hermes request capture and historical estimate

## Contract

- Hermes owns a per-profile, opt-in plugin. It records only hook metadata in a separate SQLite ledger, never prompts, responses, API keys, or base URLs.
- `post_api_request` and `api_request_error` cover main-loop attempts; `post_auxiliary_call` covers auxiliary attempts. A request ID plus outcome/retry number is the idempotency key, because Hermes can reuse its main-loop request ID across retries. Unknown/streaming usage stays unknown, not zero.
- The optional capture plugin requires Hermes **0.21.5 / release `v2026.9.24` or newer**, the first official release containing the auxiliary hooks introduced by upstream commit `0e5809566f8e98b1ff4316eafeafc58a263dd884` (2026-09-22). The manifest declares this floor and all three hooks. Registration also checks actual `VALID_HOOKS` before creating the ledger; missing hooks reject capture with an upgrade message instead of silently omitting auxiliary calls. This requirement does not apply to the separate cumulative importer. Approval-prompt/decision hooks are not model API calls and must not replace auxiliary hooks.
- Main-loop `started_at` is shared across retries: the stored difference to `ended_at` is **cumulative main-call elapsed time, including retries and waits**, not an independent attempt's latency. Auxiliary events carry attempt-scoped timestamps and are labeled **auxiliary attempt elapsed time**. Existing numeric values are preserved, including historical ledger rows; we neither subtract prior events nor estimate provider-only latency.
- The plugin freezes a cumulative `session_model_usage` baseline plus any session-level main-call residual when its ledger is first created. A manual replay copies that baseline to CC Switch as one *estimated historical total*, with no synthetic request rows or daily buckets. Missing/incompatible baselines are reported, not guessed. A replacement ledger can add new exact events, but replay never adds a second potentially overlapping historical estimate for the same profile.
- CC Switch auto-imports exact events read-only from each profile ledger with a durable, bounded cursor keyed by ledger incarnation. Exact events drive a paginated request list only. Existing aggregate deltas remain the sole source of dashboard totals/trends, so events and aggregates are never added together.
- Collection begins only after the plugin is installed, enabled in the relevant Hermes profile and loaded on restart. Hook write failures, unhooked paths (including current Codex app-server), or unknown usage are explicitly caveated; no claim of universal completeness.
- Historical estimates can overlap aggregate deltas already imported before activation. They are displayed separately and never added to dashboard totals or trends.
- One-click activation refuses a `plugins:` config block with comments or YAML anchors rather than dropping them; those configurations need a manual plugin-enable edit. It validates the resulting YAML before writing and preserves a pre-change backup for ordinary configs.

## Verification

1. Test plugin event sanitization, deduplication, baseline freezing, and missing-usage behavior with a temporary Hermes home.
2. Test CC Switch source import, repeat import, multiple profiles, and manual baseline replay against temporary SQLite fixtures.
3. Run focused Rust tests, plugin Python tests, frontend tests/typecheck, formatting, then live PR CI after pushing.
