# Codex Desktop Model Whitelist Phase 2 Implementation Plan

**Goal:** Add a one-shot, best-effort synchronization of the active provider's model ids into Codex Desktop's cached Statsig `available_models` list.

## Scope

- Read model ids from the cc-switch `modelCatalog` database field.
- Preserve Chromium LocalStorage value prefixes and UTF-8/UTF-16LE encodings.
- Update only Statsig cached-evaluations records and only the `107580212.value.available_models` array.
- Discover both traditional Codex data paths and Microsoft Store/MSIX package paths.
- Run once after a Codex provider is written live; log lock/errors without failing the provider switch.

## Explicit exclusions

- No background retry thread or periodic loop.
- No Chromium remote-debugging port.
- No DevTools WebSocket or JavaScript injection into the Codex renderer.
- No deletion of previously whitelisted model ids.

## Test sequence

1. Verify model ids are appended without duplicates.
2. Verify UTF-8/UTF-16LE LocalStorage values round-trip.
3. Verify a temporary LevelDB cache is updated in place.
4. Verify Windows Store/MSIX candidate paths are discovered.
5. Run focused Codex config tests, provider integration tests, formatting, and cargo check.

## Operational behavior

The one-shot write succeeds when Codex Desktop is closed. If its LevelDB is locked, cc-switch logs a warning and leaves the provider switch successful; the user can close Codex Desktop and re-save/switch the provider.
