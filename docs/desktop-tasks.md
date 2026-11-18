# Local Claude Desktop task inventory and recovery

On macOS, open **Desktop tasks** from the Sessions menu. The inventory keeps each existing account/organization registry separate and shows the recorded schedule and enabled/paused state. Saved prompts come from `scheduled-tasks/<id>/SKILL.md` in the configured Claude directory. Unreferenced prompts can be viewed/exported but cannot supply a missing schedule or enabled state; orphan classification is withheld if a registry cannot be read.

Choose another existing profile, select tasks, and preview before applying with Desktop closed. Recovery preserves enabled/paused state, cron expressions or future one-time timestamps, working folders, model/permission settings, and other definition fields. It reuses the existing shared prompt on this Mac, updates `filePath`, and removes notification/subscription links to source-account sessions. Source definitions and prompts remain unchanged. Existing destination task IDs are skipped.

For recurring tasks, `missedRunScanFloor` moves to recovery time (or stays later if already later). `lastRunAt` and `lastScheduledFor` remain unchanged. This excludes runs missed before recovery without inventing run history. Expired one-time tasks are blocked even when paused. Source retry/history registries are not imported, and destination retries for restored IDs are removed. Other destination entries and registry fields remain intact.

Desktop owns subsequent execution, including its normal dispatch delay and catch-up after later sleeps. Recovery does not run tasks, install a scheduler, or disable Desktop's future catch-up behavior. It does not cover Cowork/cloud tasks, CLI session cron jobs, watchers, remote-migrated/space-bound tasks, T3 imports, or cross-machine migration. Unsupported schemas, cron expressions, missing prompts, and unavailable working folders are blocked.

## Format gate and publication

Writes require Claude Desktop **1.46388.4**, whose missed-run boundary and persisted retries were inspected. Other builds remain available for inventory. Expand the gate only with evidence for the scheduler's handling of these fields. The reference inventory alone did not establish recovery semantics.

Preview fingerprints the source/target registry and selected prompt contents. Apply rechecks them, preserves a mode-0600 backup when a target registry exists, atomically publishes the updated registry, and verifies the result by reading it back. New registries are created exclusively. Receipts disclose completed writes and backups even if a later step fails. CC Switch serializes its own recoveries and checks for Desktop before/during writes; external programs do not share that lock.

## Validation

Inspection ran on macOS arm64 with Claude Desktop 1.46388.4. This machine had no saved local tasks, so recovery is validated with fictional registry/prompt fixtures only. Tests cover enabled/paused preservation, missed-run boundaries, expired/future one-time tasks, pending retries, malformed/duplicate definitions, stale previews, missing inputs, backup/read-back, and running-app rejection. No live sessions or schedules were changed. Native Desktop visibility/execution still requires acceptance in disposable accounts; Windows/Linux behavior is not claimed.

Format sources and license notices are in [desktop-format-provenance.md](desktop-format-provenance.md).
