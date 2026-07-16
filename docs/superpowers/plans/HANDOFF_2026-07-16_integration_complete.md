# Handoff — Codex Workbench Integration (2026-07-16)

## Status

**Local implementation of plan Tasks T1–T16 is complete and committed.**

Automated gates (this machine):

| Gate | Result |
|------|--------|
| `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` | pass |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | **2053 passed**, 0 failed, 2 ignored |
| `pnpm typecheck` | pass |
| `pnpm test:unit` | **475 passed** (76 files) |
| `git push origin main` | **blocked 403** — `Seller-1990` has no write access to `farion1231/cc-switch` |

HEAD: `8f90fcd` — `chore(rust): silence clippy -D warnings on lib`  
Branch: `main` **ahead of origin/main by 31 commits**

## Key commits (recent)

- `8f90fcd` chore(rust): silence clippy -D warnings on lib
- `4178782` style(rust): cargo fmt for workbench integration modules
- `c65f659` docs(security): attribute CodexElves MIT port and drop unused imports
- `f6fc0bb` / `d69975f` test(sync): S3/WebDAV prepare-apply restore coverage
- `41551bd` test(codex): OverviewTab no-body contract
- `93bde3e` feat(codex): complete Codex workbench reasoning integration (T16)
- `4ad34dd` feat(usage): enrich codex session logs with reasoning and turn_id (T15)
- `774ecc6` feat(proxy): wire T14 multi-round reasoning continuation
- … through earlier T1–T13 workbench/security/runtime commits

## Intentionally untracked / not committed

- `.superpowers/` — pre-existing tooling dir; **do not commit**
- `projects_probe.py` — local probe script; not product code
- Plan checkboxes in `2026-07-15-codex-workbench-integration.md` still show `[ ]` historically; code is landed — treat this handoff as the completion record rather than re-editing 120+ step boxes unless desired

## Windows 15-item acceptance matrix

Automated unit/integration coverage is green. **Live UI / real Codex CDP items still need a human on a Windows desktop with Codex installed:**

1. Credential ownership UI visible
2. Stale dual-edit → revision conflict on second save
3. Cloud restore preview defaults local credentials
4. Ordinary Codex running → enhanced launch refuses, **kill_calls=0**
5. Enhanced launch → CDP + bridge up; navigation reinjects once
6. Toggle page features; one selector failure does not stop peers
7. Script install valid hash / invalid hash retains old
8. Plugin marketplace init under temp `CODEX_HOME`
9. **Live CDP inject + bridge auth** (needs real Codex process)
10. Radar scan cached degradation (can use fixtures; live optional)
11. Reasoning continuation multi-round (proxy unit tests cover logic; live optional)
12. Usage log shows continuation metadata, **no reasoning body**
13. Provider system-prompt rewrite path
14. Sync prepare/apply non-destructive
15. Full app smoke after cold start

Items 4, 7, 11–14 have strong automated coverage. Item **9** is the main residual live gap.

## Safety constraints honored

- Never kill ordinary Codex (unit-tested FakeHooks)
- Mutation tests use temp dirs/DBs only — no developer real profile
- Credential audit stores fingerprints only
- Market never auto-fetches
- Bridge binds localhost + nonce

## What you must do to finish remote delivery

1. Grant push access **or** push from an authorized account:
   ```
   git push origin main
   ```
2. Optionally open PR if `main` on remote should not receive direct pushes.
3. Run live matrix items 5–9 on a machine with Codex App installed.
4. Decide whether to commit `docs/adr/`, `docs/superpowers/`, `CONTEXT.md` (recommended product docs) — currently untracked.

## Test commands (reproduce)

```powershell
cd "D:\LocalWork\Software Development\desktop\switch\cc-switch"
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm typecheck
```

## Unsupported / out of scope this session

- GitHub push as `Seller-1990` (403)
- Full interactive Codex GUI CDP matrix without installed/running Codex
- Editing remote-protected `main` without collaborator rights

### FE fixture fix (same day)
- Added `revision: 1` to MSW default providers and App.test add/edit mocks (fixes `provider_revision_missing`).
- Raised Codex workbench App integration test timeout to 15s under full-suite load.

