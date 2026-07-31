---
status: accepted
---

# Preserve Upstream Codex Unified History per Target

CC Switch will preserve upstream's stable Codex `custom` session bucket instead of encoding each CC Switch Provider identity into `model_provider`. Provider identity remains in CC Switch, while Codex uses `custom` as a compatibility route and history category; the existing unified-history backup and migration behavior therefore remains usable. This behavior is applied independently inside each Managed Target, so Windows and every WSL environment keep separate configuration, authentication, session files, state databases, backups, and migration records.

## Considered Options

- Generate a distinct `cc_switch_<name>_<id>` Codex route for every Provider: rejected because it fragments native history, bypasses upstream's migration feature, and confuses an internal Provider identity with a Codex compatibility category.
- Aggregate sessions only in CC Switch: retained as future target-aware session management, but insufficient by itself because users also need Codex's native history and resume surfaces.
- Copy or merge history between Managed Targets: rejected because Windows, WSL, and future remote environments own independent state and may contain non-portable encrypted content.

## Consequences

- Changing Provider inside one Managed Target does not move or copy its history to another Target.
- Existing generated `cc_switch_*` routes are compatibility inputs to migration, not the canonical shape for newly projected configuration.
- Cross-Provider resume remains best-effort when a backend cannot consume encrypted content created by another backend.
