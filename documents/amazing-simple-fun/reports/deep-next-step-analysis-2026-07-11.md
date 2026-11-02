# Amazing Simple Fun - Deep Next-Step Analysis

Date: 2026-07-11 12:54 UTC
Owner: Oto, COO
Project path: `/home/growthhacker/projects/amazing-simple-fun`
Current remote main: `06b3c2843 fix: stabilize lint cleanup after main rebase`

## Executive Decision

Continue with release-gate stabilization before more broad lint cleanup.

The next best work item is not another random largest lint file. The project now has larger release blockers:

1. `npm test` fails: 18 failed tests across 12 failed test files.
2. `.env` is tracked in Git.
3. `npm audit` reports 4 vulnerabilities, including 2 high.
4. `npm run build` passes, but emits duplicate i18n key warnings and many chunk/circular dependency warnings.
5. Full ESLint still fails: 331 hard errors across 226 files.

## Current Verified State

Passed:
- `npx tsc --noEmit --pretty false`
- `npm run i18n:check`
- `npm run build` exits successfully

Failed:
- `npm test`
- `npx eslint . --quiet`
- `npm audit --json`

Warnings:
- Build warns about duplicate keys in `src/lib/i18n/translations/en/affiliate.ts`:
  - `affiliate.emptyState.pendingApproval.tag`
  - `affiliate.clicks.title`
- Build warns about circular chunk dependencies through barrel re-exports.
- Build warns about one CSS syntax issue: `Expected percentage but found "{"`.

## Highest-Impact Blockers

### P0 - Security Hygiene

`.env` is tracked by Git.

Observed:
- `git ls-files .env .env.*` returns `.env` and `.env.example`.

Action:
- Remove `.env` from Git tracking while preserving local file.
- Ensure `.env` is ignored.
- Rotate any secrets that were committed or pushed.

Release impact:
- Do not treat the project as release-clean while secrets may be in repo history.

### P0 - Test Suite Red

`npm test` result:
- 98 test files total
- 86 passed
- 12 failed
- 1,392 tests total
- 1,372 passed
- 18 failed
- 2 skipped
- 1 unhandled error

Main failing areas:
- `src/hooks/payments/__tests__/useStripeCheckout.test.ts`
  - tests do not expect new `returnPath` payloads for CSS checkout and customer portal.
- `src/hooks/core/__tests__/useOnboarding.test.ts`
  - mock chain does not support current `.order().limit()` flow.
  - expected project call count is stale.
- `src/features/affiliate/hooks/useAffiliatePartnerGate.test.ts`
  - expected exported function is missing or renamed.
  - redirect expectation for null partner is stale.
- `src/lib/i18n/__tests__/translations.test.ts`
  - test methodology disagrees with `npm run i18n:check`.
  - reports 244 missing EN keys and 1,465 identical values, while dedicated i18n check is green with baseline support.
- `src/pages/seo/__tests__/SEODashboard.test.tsx`
  - missing `QueryClientProvider` in test setup.

Action:
- Fix tests in focused batches, starting with payment + onboarding + affiliate gate.
- Align i18n unit test with the production i18n check/baseline or retire duplicate logic.

### P1 - Build Warning Cleanup

`npm run build` exits successfully, but warnings are not harmless:

1. Duplicate i18n keys in `en/affiliate.ts`.
   - Risk: later values overwrite earlier values silently.
2. Circular chunk dependency warnings:
   - AffiliateLayout barrel export.
   - Facebook hooks barrel export.
   - import hooks barrel export.
   - shop hooks barrel export.
3. CSS minifier warning:
   - `Expected percentage but found "{"`.

Action:
- Fix duplicate affiliate keys first.
- Then replace barrel imports with direct imports in the warned files.
- Locate CSS syntax warning with generated CSS/source search.

### P1 - Dependency Audit

`npm audit --json` reports:
- `vite`: high, direct, fix points to major upgrade.
- `form-data`: high, transitive, fix available.
- `dompurify`: moderate, direct, fix available.
- `esbuild`: moderate, transitive through Vite/tsx.

Action:
- First try targeted safe upgrades:
  - `dompurify`
  - transitive `form-data` via lockfile-compatible update if possible
- Treat Vite major upgrade as a separate controlled branch because it may affect build/dev tooling.

### P2 - Lint Burn-Down

Current ESLint:
- 331 errors
- 226 files

Top rules:
- 84 `custom/file-naming-convention`
- 31 `no-restricted-syntax`
- 28 `custom/no-hardcoded-spacing`
- 22 `custom/no-hardcoded-typography`
- 21 `custom/enforce-page-header`
- 20 `jsx-a11y/click-events-have-key-events`

Top files:
- `src/components/poas-kurz/ProfessionalMockupSection.tsx` - 6
- `src/components/showcase/DesignSystemShowcase.tsx` - 6
- `src/types/manual-tags.ts` - 6
- `src/components/google/GoogleTextLogo.tsx` - 5
- `src/components/homepage/hero/RecentTransactionsList.tsx` - 5
- `src/components/imports/Import/DragDropFileUploader.tsx` - 5
- `src/components/rfm-landing/RFMHeroSection.tsx` - 5
- `src/components/ui/unified-alert.tsx` - 5

Action:
- After P0/P1 gates, attack lint by rule family:
  1. file naming convention
  2. a11y keyboard handlers / anchors
  3. design-system tokens
  4. Supabase `exec_sql` wrappers

## Existing Stash

There is a stash:
- `pre-main-push-uncommitted-lint-work`

It contains broad lint-related edits across 79 files. Do not pop it wholesale onto main. Review and apply selectively after release-gate blockers are handled.

## Recommended Next Work Order

1. Create a short branch from `main`: `fix/release-gate-stabilization`.
2. Fix tracked `.env` hygiene and document required secret rotation.
3. Fix duplicate `en/affiliate.ts` keys and rerun build.
4. Fix failing tests in three small batches:
   - payments checkout tests
   - onboarding mocks
   - affiliate gate exports/redirects
5. Align i18n translation unit tests with `scripts/i18n-check-all.cjs`.
6. Patch safe audit items (`dompurify`, `form-data`) if lockfile allows.
7. Only then resume lint burn-down.

## Escalation Threshold

Do not deploy until:
- `.env` is no longer tracked and secrets are rotated if exposed.
- `npm test` is green or specific failing tests are explicitly waived.
- `npm run build` is green without duplicate-key warnings.
- `npm audit` high vulnerabilities are resolved or accepted with rationale.
- full lint is either green or has an explicit temporary waiver.

