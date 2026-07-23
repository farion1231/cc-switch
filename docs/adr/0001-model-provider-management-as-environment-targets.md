---
status: accepted
---

# Model Provider Management as Environment Targets

CC Switch will model each Application installation as a Managed Target in an Environment rather than extending the existing single-directory override with additional platform-specific paths. Providers remain reusable definitions, while Target Overrides, authentication, local configuration, current Provider state, and session provenance remain device-local; switching uses field-level Projection and multi-target rollback instead of copying complete configuration files. This supports independent or explicit linked switching across Windows and WSL without mixing platform paths, and creates a stable seam for later Application and SSH adapters.

## Considered Options

- Add a second `codexConfigDir` for WSL: rejected because it hard-codes exactly two Codex locations and repeats the same problem for other Applications and future remote hosts.
- Synchronize the complete Windows configuration into WSL: rejected because paths, authentication, MCP commands, workspace trust, and other Local Fields have different ownership and semantics.
- Merge session storage between Environments: rejected because Windows, WSL, and future remote Targets own independent state. Within one Managed Target, Codex's upstream unified-history bucket is preserved as refined by ADR-0002; cross-Provider resume remains best-effort.

## Consequences

- The first supported vertical slice is Codex on a Windows host managing Windows and one or more WSL users.
- Unknown configuration keys are Local Fields by default; an Application adapter must explicitly claim Managed Fields.
- Existing automatic live-config backfill cannot write a shared Provider in a multi-target flow. Drift requires an explicit user decision and may only become a Target Override unless the user deliberately edits the shared Provider.
- WSL proxy takeover, other Applications, and SSH Targets require additional adapters and are subsequent phases, not implicit first-release support.

## Follow-up Work

- Make Session Manager target-aware. The desktop build currently scans the Windows Codex home even when a WSL Managed Target is selected, so the page shows Windows sessions only.
- Add an Environment/Target selector to Session Manager and include the target ID in query keys, message loading, resume commands, and deletion scope.
- Read WSL sessions without copying or rewriting them, preserving their Environment, source path, and Provider provenance. Provider filtering must distinguish API Providers from the existing Application filter; legacy `custom` sessions whose original Provider cannot be proven should be labeled unknown rather than guessed.
- Keep cross-Target session aggregation read-only. Resuming a session should route through its recorded Target; within that Target, upstream's unified Codex history behavior applies. Cross-Provider resume remains best-effort because encrypted reasoning content may not be portable between backends.
