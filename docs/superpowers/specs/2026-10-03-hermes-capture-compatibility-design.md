# Hermes capture compatibility and elapsed-time correction

## Approved minimal scope

Correct plugin admission metadata, report unsupported Hermes installations explicitly,
and distinguish cumulative main-call elapsed time from auxiliary attempt duration.
Do not change aggregate totals, six-column source identity, database schemas, historical
replay, or add metrics panels. Do not enable or restart a user's Hermes runtime.

## Compatibility

The supported release floor for the optional capture plugin is Hermes 0.21.5
(release tag `v2026.9.24`, commit `f97608f178d1ffeca59860195ab7da295f7c8e5f`).
The preceding release `v2026.9.21` does not declare `post_auxiliary_call`.
Auxiliary hooks were introduced by `0e5809566f8e98b1ff4316eafeafc58a263dd884`.
Declare `requires_hermes` and all three registered hooks in the manifest.
Check the actual `VALID_HOOKS` before creating a ledger or registering callbacks;
missing hooks must reject capture with an upgrade message, not silently drop auxiliary
traffic. This requirement does not gate the separate read-only aggregate importer.
Approval UI hooks must not substitute for auxiliary model-call hooks.

## Timing

Preserve existing source timestamps and duration values. Main-loop hooks share the
logical request start across retries, so label main rows as cumulative elapsed time
including retries/waits. Auxiliary hooks wrap a provider attempt, so label those rows
as attempt elapsed time. Do not subtract prior failures or claim exact provider latency.
This also makes previously imported main records understandable without migration.

## Verification

Add tests before implementation for manifest declarations, rejection before ledger
creation on a missing auxiliary hook, main/auxiliary timing labels, and compatibility
guidance. Characterize cumulative retry payloads without changing their values.
Run plugin tests, isolated real Hermes admission checks, focused frontend tests,
typecheck/formatting, and aggregate/capture Rust tests. Report only freshly verified
scope; no push, PR comment, or runtime activation without explicit authorization.
