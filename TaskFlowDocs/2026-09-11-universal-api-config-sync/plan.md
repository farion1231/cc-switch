# Plan

Task version: v1
State: checking
Approval: user confirmed the unified-provider-only button and rollback of child writeback.

No spec required — implementation is a focused change across an existing service, command, API, and modal with no new data model.

## Steps

- [x] Remove child-to-universal writeback and its tests.
- [x] Add API-only backend sync service, Tauri command, and frontend API.
- [x] Add unified-provider edit button and localized text; keep `cc_switch` route-table flow unchanged.
- [x] Run final checks and review diff.

## Verification

- TypeScript: `./node_modules/.bin/tsc --noEmit` passed.
- Locale JSON parsing: all locale files parsed successfully.
- Rust: blocked by WSL Windows interop failure (`UtilBindVsockAnyPort: socket failed 1`) when invoking PowerShell; source was reviewed and formatted diff is clean.
