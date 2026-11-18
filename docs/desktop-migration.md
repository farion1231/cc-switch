# Same-Mac Claude Desktop session migration

On macOS, open **Migrate Desktop sessions** in the authorization center. Select existing source and target account/organization profiles, select records, and preview before applying. This implements the copy-only scope of issue #5231 and adapts the closed PR #5232 to the current authorization center.

Migration copies missing `local_*.json` Code records between normal Claude and managed Claude-3p roots on the same Mac. It leaves source records and existing destination files unchanged. Archived records are selectable through an archive filter and remain archived, including records whose archived state exists only in the legacy index. Deleted/unverified source records and destination deletion/archive conflicts are blocked.

This copies Desktop metadata. It does not copy missing transcripts or project folders, move credentials, switch the Desktop login, or migrate data between machines. A successful file receipt is not proof of native sidebar visibility or successful native resume.

## Publication and validation

Quit Claude Desktop before applying. A preview fingerprints the selection, source/target records, and archive/deletion state; apply rejects a changed snapshot. Descriptor-relative operations validate identifiers and reject linked components. Session files use exclusive publication, so existing destinations cannot be overwritten by a racing copy. Read-back verifies bytes. Partial failures report completed copies without deleting source data or silently rolling back completed writes.

CC Switch serializes its own recovery operations and checks for Desktop before and during publication. Desktop and external editors do not share that lock, so the process check cannot prevent an external restart or write during a copy.

The installed format was inspected read-only on Claude Desktop 1.46388.4, macOS arm64. Synthetic tests cover legacy archive preservation, account tombstones, stale previews, path traversal/symlinks, concurrent exclusive copies, idempotency, running-app rejection, and partial receipts. No live account migration was performed; native sidebar and resume acceptance remain to be exercised in disposable Desktop accounts. Other platforms are not claimed as tested.

Format sources and license notices are in [desktop-format-provenance.md](desktop-format-provenance.md).
