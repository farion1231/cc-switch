# Growili release readiness — P0 support view

> **Status:** ARCHIVED BASELINE  
> Aktuální stav neurčuje; viz `CURRENT-WORK.md` a `README.md`.

**Priority:** `P0-RELEASE-GATES`  
**Owner:** CTO  
**Support:** COO  
**Last refreshed:** 2026-08-08 UTC  
**Evidence baseline:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`  
**Canonical repo/source:** `/home/growthhacker/projects/amazing-simple-fun` at audited SHA `7e500dffc87901e8caa7f519b8f40c0a79479b5e`

## Executive status

**Release is not ready.** The current baseline still shows deterministic release gates failing, and there is no newer evidence in the approved source set proving those failures were resolved.

## Current blockers (exact, evidence-backed)

### 1. TypeScript gate — blocker
- **Status:** FAIL
- **Measured result:** `12 errors`
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Evidence detail:** glossary model drift (`src/data/glossary.ts` vs `src/types/glossary-types.ts`) and inconsistent `notify_onboarding_emails` shapes in generated Supabase/profile types.
- **Operational implication:** no reliable typed validation baseline for release signoff.
- **Primary owner:** CTO
- **COO support:** maintain blocker ledger, ensure exact failing cluster is tracked, require fresh validation evidence before blocker is removed.

### 2. i18n integrity gate — blocker
- **Status:** FAIL
- **Measured result:** `4 missing keys in both CS and EN`; audit notes one may be scanner noise from a regression test.
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Operational implication:** release text parity is not trustworthy; signoff cannot assume locale completeness.
- **Primary owner:** CTO
- **COO support:** require exact missing-key list and rerun evidence before readiness status changes.

### 3. Vitest gate — blocker
- **Status:** FAIL
- **Measured result:** `18 failed tests`; `1,720 passed`, `2 skipped`
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Evidence detail:** failures cluster in i18n hook semantics, `TransformationSqlDialog`, `SimpleConnectDialog.sklik`, `SEODashboard`, `DatabasePage`, `NavModuleStrip`, dashboard widget layout, and Google My Business account ID mapper.
- **Operational implication:** regression surface remains open; no stable automated acceptance baseline.
- **Primary owner:** CTO
- **COO support:** translate failing clusters into acceptance gates and track whether reruns cover the same files/components.

### 4. ESLint gate — blocker
- **Status:** FAIL
- **Measured result:** `61 errors + 772 warnings in 383 files`
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Evidence detail:** the audit explicitly says the consolidated plan citing `39/658` is already stale.
- **Operational implication:** any readiness board using the smaller count is stale and invalid.
- **Primary owner:** CTO
- **COO support:** prevent stale counts from appearing in release reporting; require current lint output to replace the audited baseline.

### 5. Production dependency audit — blocker
- **Status:** FAIL
- **Measured result:** `3 high + 3 moderate` production dependency findings
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Evidence detail:** direct packages named in the audit include `dompurify` and `react-router-dom`; fixes were reported available.
- **Operational implication:** known production package risk remains open until patched and re-audited.
- **Primary owner:** CTO
- **COO support:** keep this as a ship blocker until exact remediation evidence exists.

### 6. CI gate — blocker
- **Status:** FAIL
- **Measured result:** no active `.github/workflows/*.yml` or `*.yaml`
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Operational implication:** there is no enforced repeatable validation path for release gates.
- **Primary owner:** CTO
- **COO support:** define required evidence package for signoff only after CI or equivalent deterministic rerun path exists.

## Not blockers but still relevant

### Production build — currently positive but insufficient alone
- **Status:** PASS
- **Measured result:** `NODE_OPTIONS=--max-old-space-size=4096 npm run build` passed in `45.06 s`
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Why not enough:** the audit explicitly concludes the repository is still not release-gated despite build success.

### Bundle size warning
- **Status:** WARN
- **Measured result:** `index-*.js` `1,765.36 kB` minified / `453.35 kB` gzip
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Operational implication:** monitor, but do not let this obscure the harder blocking failures above.

### Migration ledger
- **Status:** PASS
- **Measured result:** remote up to date; migration reconciliation already complete
- **Source:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/PRODUCT-AUDIT-2026-07-27.md`
- **Last verified:** `2026-07-27 13:57 UTC`
- **Important note:** this must **not** remain listed as an open blocker.

## What is stale or unknown

### Stale
1. The previous COO file at this path was stale and unusable: it listed generic placeholders, incorrect counts, and no source paths.
2. Any release summary still using `39 ESLint errors / 658 warnings` is stale; audited baseline is `61 / 772`.
3. Any onboarding/release note implying migration reconciliation is still open is stale; audit says it is complete.

### Unknown / not yet evidenced
1. Whether any of the audited TypeScript, i18n, Vitest, ESLint, dependency, or CI blockers were fixed after `2026-07-27 13:57 UTC`.
2. Exact current failing file list from a fresh rerun in the canonical repo.
3. Whether build warning size changed after the audited SHA.
4. Whether a CI proposal using `tsconfig.app.json` and increased Node heap has been prepared/approved, as required by current priorities.

## Minimum evidence required to clear P0 blockers

A blocker may be removed from this board only when a newer evidence package exists with:
- exact command/check rerun,
- timestamp,
- repo SHA,
- result summary,
- failing file/package list when non-zero,
- source artifact path in workspace documentation.

Required checks to refresh:
1. TypeScript
2. i18n check
3. Vitest
4. ESLint
5. production dependency audit
6. CI presence and working validation path

## Ownership split

### CTO owns
- code fixes,
- dependency updates,
- test fixes,
- lint remediation,
- CI implementation,
- validation reruns in the canonical codebase,
- any branch/worktree execution tied to code changes.

### COO can support
- maintain the release-blocker board,
- verify that evidence packages are current and sourced,
- prevent stale blocker counts from being reported upward,
- define release signoff checklist requirements,
- distinguish resolved vs unresolved vs unverified items,
- make escalation visible when no fresh evidence exists.

## COO recommendation

**No release signoff and no “ready” framing until the six failing gates above are rerun with fresh evidence and either pass or are explicitly waived by Jiří.** Right now the only evidence-backed position is that P0 remains blocked by deterministic validation failures.
