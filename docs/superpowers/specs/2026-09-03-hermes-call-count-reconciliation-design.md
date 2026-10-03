# Hermes call-count reconciliation design

## Context

Hermes persists main-loop call totals in `sessions.api_call_count` and
per-route/per-task cumulative usage in `session_model_usage`. Incremental main
loop writes normally update both tables in one transaction, while absolute
gateway writes and legacy data can leave a positive main-loop remainder only
in `sessions`. Auxiliary tasks are recorded only in `session_model_usage`.

CC Switch currently imports only `session_model_usage`, so it can undercount
main-loop calls that exist only in the session aggregate.

## Goal

Import that positive main-loop remainder without double-counting auxiliary
calls or inventing a model/provider attribution.

This change does not attempt exact per-call time attribution. Hermes still
exposes cumulative rows rather than timestamped usage events, so CC Switch
continues to assign observed deltas to adjacent sync windows.

## Source contract

For each Hermes session:

```text
attributed_main_calls = SUM(session_model_usage.api_call_count WHERE task = '')
main_residual = MAX(0, sessions.api_call_count - attributed_main_calls)
```

Rows with a non-empty `task` are auxiliary usage and must not reduce the main
residual. Existing `session_model_usage` rows remain unchanged. When
`main_residual` is positive, the reader appends one cumulative synthetic row
with:

- the same `session_id`;
- `model = 'unknown'`;
- empty billing-provider, endpoint, and billing-mode values;
- `task = 'unattributed_main'`;
- `api_call_count = main_residual`;
- zero tokens and zero cost.

The explicit unknown route prevents a legacy last-write-wins session route
from being presented as proven attribution.

## Compatibility and failure handling

The importer first reads `session_model_usage`, as it does today. It performs
reconciliation only when the source database also has a `sessions` table with
an `api_call_count` column. Older or partial databases that lack that source
contract continue importing their detail rows without residuals.

Negative counters remain invalid. A session total below its attributed main
sum produces no residual rather than a negative delta. Existing snapshot
regression handling remains authoritative if a source row is reset or rebuilt.

No CC Switch database migration is required because the synthetic row uses the
existing snapshot and delta schema.

## Testing

Focused tests will establish that:

1. a positive session residual becomes one `unattributed_main` row;
2. auxiliary task calls do not reduce the main residual;
3. no row is added when detail rows fully cover the session total;
4. a source without the compatible `sessions.api_call_count` contract keeps
   the previous behavior;
5. the second sync emits only the residual increase and remains idempotent.

The existing Hermes importer test group, formatting, Clippy, and the GitHub
backend matrix remain required gates after implementation. Because the PR is
currently conflicting with upstream `main`, the implementation branch must be
updated and conflicts resolved before final CI evidence is accepted.
