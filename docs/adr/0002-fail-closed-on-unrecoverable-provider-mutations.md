---
status: accepted
---

# Fail closed on unrecoverable provider mutations

Provider credential operations span SQLite state and client-owned configuration files, so they use compensating snapshots rather than pretending to be a single atomic transaction. If an operation fails and compensation cannot restore agreement, CC Switch marks only the affected client as having a Configuration Inconsistency and blocks further configuration writes there until explicit recovery succeeds; unrelated clients and read-only features remain available.
