# Amazing Simple Fun / Growili - COO Project Audit

Date: 2026-07-10
Auditor: COO
Audited paths:
- `/home/growthhacker/projects/amazing-simple-fun`
- `/home/growthhacker/.openclaw/workspace/documents/brand/amazing-simple-fun`
- `/home/growthhacker/.openclaw/workspace/documents/sites/amazing-simple-fun`

## Executive Verdict

Status: partial operationally ready, not clean enough for confident release.

The project is a large production-grade React/Vite/Supabase Growili application with strong architecture rules, broad documentation, CI, custom linting, i18n gates, and substantial test coverage. The main operational weakness is not lack of system design; it is quality-gate drift and uncontrolled surface area.

Highest-risk blockers:
1. Security hygiene: `.env` is tracked in the project Git index, and the OpenClaw mirror contains `.git-credentials`.
2. Quality gates are red: `npm run i18n:check` fails, and `npm run test` showed multiple failing suites.
3. Current working tree is dirty with in-progress affiliate registration/auth changes.
4. Project scale is high: about 3,475 TS/TSX files, 134 Supabase edge functions, 174 migrations, and 722 MB local footprint.

## Operational Situation

Project type:
- React 18 + TypeScript + Vite frontend.
- Supabase backend with many edge functions and migrations.
- Strong e-commerce analytics / POAS / CSS / affiliate / onboarding scope.

Branch and workspace:
- Branch: `agent/hermes/affiliate-register-fix`
- Ahead/behind origin/main: `0 0`
- Dirty files:
  - `docs/todo/project_backlog.md`
  - `src/hooks/auth/useAuthSession.ts`
  - `src/pages/affiliate/AffiliateRegisterPage.tsx`

Relevant project rules:
- Do not push directly to `main` unless explicitly asked.
- Use dedicated branch per task.
- Stop before editing when unrelated local changes exist.
- CI expects lint, typecheck, build, and tests.
- User-facing strings must go through i18n.
- Design-token lint is part of `npm run lint`.

## Validation Results

Completed:
- `npx tsc --noEmit`: passed.
- `npm run i18n:check`: failed.
- `npm run test`: interrupted by gateway restart, but before interruption it already showed failing suites.

Observed test failures before interruption:
- `src/hooks/core/__tests__/useOnboarding.test.ts`
  - expected `projects` call count mismatch.
  - Supabase mock missing `.order()` in existing-shop path.
- `src/hooks/payments/__tests__/useStripeCheckout.test.ts`
  - CSS checkout expected payload no longer matches actual `returnPath`.
  - customer portal expected call no longer matches actual body payload.
- `src/lib/i18n/__tests__/translations.test.ts`
  - CS/EN key parity failures.
  - equal key count failure.
  - copy-paste quality threshold exceeded.
- `src/__tests__/i18n/hooks.test.tsx`
  - language change/current language and memoization expectations failing.

`npm run i18n:check` details:
- 211 missing EN translations.
- 94 extra EN keys not in CS.
- New quality regression in `conversions.ts`: +6 suspicious copy-paste keys versus baseline.
- Quality debt still large: 3,250 suspicious keys versus baseline 3,366.

Not completed:
- `npm run build`: interrupted before terminal result.
- `npm run lint`: interrupted before terminal result.
- `npm audit`: not completed due repeated gateway restarts.

No long-running `vite build`, `vitest`, `eslint`, `npm audit`, or `tsc` processes remained after cleanup check.

## Risk / Bottleneck

P0 - Secret exposure risk:
- `.env` is tracked by Git in `/home/growthhacker/projects/amazing-simple-fun`.
- The OpenClaw site mirror contains `.env` and `.git-credentials`.
- I did not read or print secret values. This must be verified by the owner immediately. If real credentials are present, rotate them and remove from tracked/mirrored locations.

P1 - Release confidence risk:
- CI quality gates are expected to run lint, typecheck, build, and test.
- Typecheck passes, but i18n and tests are already red.
- Any release from current state risks broken onboarding/auth/checkout/i18n behavior.

P1 - Customer-impact workflow risk:
- Current branch is `affiliate-register-fix`.
- Dirty changes touch affiliate registration and auth session redirect logic.
- Backlog mentions duplicate registration emails.
- These areas directly affect signup, onboarding, affiliate partner flow, and customer trust.

P2 - Operational complexity risk:
- Large app surface area with many pages, features, edge functions, migrations, docs, and previous audits.
- Without a strict release checklist, fixes can solve one funnel while breaking another.

P2 - Documentation drift:
- README claims some older dependency versions while `package.json` has newer versions.
- README references `CHANGELOG.md`, but it was not present in the file listing I observed.
- There are many audit/todo/status documents, which increases risk of stale priorities.

## Recommended Process

Immediate release gate:
1. Freeze deploys until P0 secret hygiene is checked.
2. Do not merge or deploy `agent/hermes/affiliate-register-fix` until tests are green or failures are explicitly accepted.
3. Run and record:
   - `npm run i18n:sync`
   - `npm run i18n:check`
   - `npm run test`
   - `npm run lint`
   - `npm run build -- --mode development`
   - `npm audit --audit-level=moderate --omit=dev`

Fix order:
1. Security cleanup: `.env`, `.git-credentials`, rotation decision.
2. i18n sync/parity, because it is a shared gate and contaminates many tests.
3. Update failing tests or implementation for affiliate/onboarding/Stripe behavior.
4. Re-run full CI command set.
5. Only then continue feature work.

Owner checklist:
- CTO: verify and rotate secrets if needed; decide `.env` removal history strategy.
- CTO/engineering agent: fix i18n parity and translation quality regression.
- CSO/COO: validate affiliate signup/onboarding/customer email behavior.
- CEO/Steve: approve deploy only after green gate report.

Escalation threshold:
- Immediate escalation if `.env` or `.git-credentials` contains production secrets.
- Immediate escalation if duplicate registration emails are confirmed in production.
- Escalate before launch if any signup, checkout, onboarding, or CSS purchase test remains red.

## Next Action

Recommended next task:
Run a focused P0 security hygiene task for `.env` / `.git-credentials` without printing secrets, then fix the i18n gate before touching affiliate flow further.

Confidence: medium-high.

Limitations:
- Memory search was unavailable because the memory index metadata is missing.
- Gateway restarts interrupted long-running build, lint, test, and audit commands.
- Audit is based on local repository/docs/config and partial command output, not production telemetry.
