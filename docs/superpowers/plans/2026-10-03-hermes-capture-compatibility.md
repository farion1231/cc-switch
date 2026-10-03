# Hermes Capture Compatibility Implementation Plan

> **For agentic workers:** Execute inline in the existing dedicated PR checkout.

**Goal:** Fix plugin admission and misleading elapsed-time display without changing totals.

**Architecture:** Declare the supported release floor and check actual hook availability
before capture starts. Keep stored timestamps unchanged and explain timing by event kind
in the existing request table. No schema changes or new statistics.

**Tech Stack:** Python unittest, React/i18next, Vitest, existing Rust usage importer.

## Task 1: Plugin compatibility

- [x] Add manifest tests requiring `requires_hermes: ">=0.21.5"` and declarations for
  `post_api_request`, `api_request_error`, `post_auxiliary_call`.
- [x] Add a registration test with `VALID_HOOKS` lacking `post_auxiliary_call`;
  require an upgrade error, zero registrations, and no created ledger.
- [x] Run `python -B src-tauri/resources/hermes-usage-plugin/test_collector.py` and
  confirm the new assertions fail before implementation.
- [x] Add the manifest declarations and an early `VALID_HOOKS` subset check raising
  `RuntimeError` listing missing hooks and the supported release.
- [x] Rerun unittest and isolated Hermes Doctor/admission validation.

## Task 2: Timing and guidance

- [x] Add frontend assertions for main cumulative and auxiliary attempt timing labels,
  including null duration, and the capture compatibility notice.
- [x] Run `pnpm exec vitest run tests/components/HermesCapturePanel.test.tsx` and
  confirm the new label/notice assertions fail.
- [x] Add existing-table labels selected by `event.kind` and localized compatibility
  guidance in all four locales; preserve the numeric duration and null placeholder.
- [x] Rerun the focused tests, `pnpm typecheck`, and focused Prettier checks.

## Task 3: Regression verification and handoff

- [x] Characterize three same-start retry events: durations remain 1063/4186/9488 ms.
- [x] Run aggregate and capture Rust tests; confirm no changes to source identity/totals.
- [x] Update the capture contract with the release floor, runtime gate, and timing semantics.
- [x] Inspect `git diff --check` and final scope. Leave changes uncommitted/unpushed unless
  the user explicitly requests publication; preserve their runtime configuration.

## Verification results (2026-10-03)

- Red: two new Python assertions and two new UI assertions failed for their intended reasons.
- Green: Python 8/8, capture panel + dashboard frontend 15/15, Rust aggregate/capture 20/20.
- Typecheck, focused Prettier, renderer build, and whitespace validation passed. Existing
  browser-data, bundle-size and mixed-import build warnings remain; no dependency updates.
- The local Hermes source reports 0.20.0 and the real loader rejects it under the new floor.
  With only the host-version function controlled to 0.21.5 in an isolated test, real
  Doctor and full admission validation pass without findings/warnings, using actual hook
  registration. This is a compatibility fixture, not verification of an activated
  0.21.5 installation. Version comparison rejects 0.21.2 and accepts 0.21.5.
- Official release source inspection confirms auxiliary hooks absent in v2026.9.21 and
  present in v2026.9.24 (project/CLI version 0.21.5).
- At local verification time, no installed plugin, runtime configuration, remote PR
  description, or comment had changed. Publication is a separately authorized step.
