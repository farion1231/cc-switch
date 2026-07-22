---
status: accepted
---

# Model Provider Management as Environment Targets

CC Switch will model each Application installation as a Managed Target in an Environment rather than extending the existing single-directory override with additional platform-specific paths. Providers remain reusable definitions, while Target Overrides, authentication, local configuration, current Provider state, and session provenance remain device-local; switching uses field-level Projection and multi-target rollback instead of copying complete configuration files. This supports independent or explicit linked switching across Windows and WSL without mixing platform paths, and creates a stable seam for later Application and SSH adapters.

## Considered Options

- Add a second `codexConfigDir` for WSL: rejected because it hard-codes exactly two Codex locations and repeats the same problem for other Applications and future remote hosts.
- Synchronize the complete Windows configuration into WSL: rejected because paths, authentication, MCP commands, workspace trust, and other Local Fields have different ownership and semantics.
- Merge Codex's native session buckets: rejected as a core behavior because visibility would improve while cross-Provider resume can still fail; CC Switch will aggregate sessions read-only and restore them through their Session Provenance instead.

## Consequences

- The first supported vertical slice is Codex on a Windows host managing Windows and one or more WSL users.
- Unknown configuration keys are Local Fields by default; an Application adapter must explicitly claim Managed Fields.
- Existing automatic live-config backfill cannot write a shared Provider in a multi-target flow. Drift requires an explicit user decision and may only become a Target Override unless the user deliberately edits the shared Provider.
- WSL proxy takeover, other Applications, and SSH Targets require additional adapters and are subsequent phases, not implicit first-release support.
