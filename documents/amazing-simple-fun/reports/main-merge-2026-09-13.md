# Amazing Simple Fun — stabilized main merge

- **Direct instruction:** Jiří Franěk, 2026-09-13
- **Priority:** P0-RELEASE-GATES reopened by Jiří's direct instruction
- **Repository:** `/home/growthhacker/projects/amazing-simple-fun`
- **Remote:** `growthhackercz/amazing-simple-fun`
- **Previous remote main:** `9663df78d`
- **Pushed remote main:** `ac35c76d38954898b84ebd793d5da0f7842828c0`
- **Push mode:** normal fast-forward (`HEAD:main`), no force push

## Integration safety

1. `git pull --ff-only` fetched 23 intervening commits and then safely aborted because five staged files overlapped.
2. The original ten staged changes were committed on `coo/stabilize-20260913`.
3. Integration was performed in an isolated worktree based on the newest `origin/main`.
4. Two obsolete Merchant dashboard import changes conflicted with newer rewrites and were intentionally dropped in favor of the newer remote implementations.
5. Eight functional changes were retained; `postcss-nested@6.2.0` was added as a direct dev dependency, including a minimal lockfile update.
6. Concurrent unstaged edits in three backend task documents and `DemoRequestForm.tsx` were not staged, modified, or included.

## Verification

- `git diff --check`: pass
- targeted ESLint on changed source/config files: pass
- route/import reference inspection: pass
- `npm ci --ignore-scripts --dry-run`: pass
- relevant Vitest set: 86 pass, 4 fail, 2 skipped
- clean `origin/main` router baseline: same 4 failures (`Navigate is not defined` in `GrowiliRoutes.tsx`)
- production build: fails on both the integration branch and clean `origin/main`; baseline contains stale imports after component renames (`OptimizedImage` / `OptimizedGrowthLinkOverview`) and duplicate i18n keys
- i18n gate: fails on existing missing keys from current `origin/main`; none of the reported missing keys were introduced by this commit
- full TypeScript pre-commit: stopped after 15 minutes of sustained CPU/RAM use; no diagnostic result was produced

## Operational risk

The requested commit is on `main`, but the repository-wide release gates remain red due to pre-existing post-refactor breakage on the fetched baseline. Main should not be treated as release-ready until stale imports, the missing `Navigate` import, duplicate/missing i18n keys, and the TypeScript gate runtime are resolved.

