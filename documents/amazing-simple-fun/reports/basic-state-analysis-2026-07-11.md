# Amazing Simple Fun - Basic State Analysis

Date: 2026-07-11
Analyst: Oto, COO
Project path: `/home/growthhacker/projects/amazing-simple-fun`

## Executive Summary

Current status: not release-clean, but less chaotic than the previous audit snapshot.

The repository is currently clean locally on branch `agent/hermes/affiliate-register-fix`, with 5 commits ahead of `origin/main`. TypeScript passes and locked design tokens pass. The main blocking risks remain security hygiene and i18n drift.

## Current Git State

- Branch: `agent/hermes/affiliate-register-fix`
- Ahead/behind vs `origin/main`: `5 / 0`
- Working tree: clean
- Recent branch work:
  - `docs: document CSS lead workflow`
  - `fix(affiliate): use partner-specific commission rate/currency instead of hardcoded 30 USD`
  - `fix(affiliate): remove approval notification from admin status update (self-serve)`
  - `fix(affiliate): add partner_approved email template to notify-affiliate-partner`
  - `fix(affiliate): centralized post-auth redirect + i18n cleanup for register flow`

## Project Shape

- React 18 + TypeScript + Vite app.
- Large Supabase-backed Growili product surface.
- Approximate current size:
  - 3,649 TS/TSX files
  - 206 page files
  - 172 feature directories at `src/features` depth 2
  - 485 component files at `src/components` depth 2
  - 251 hook files at `src/hooks` depth 2
  - 32 store files at `src/store` depth 2
  - 136 Supabase edge functions
  - 175 Supabase migrations
  - 754 MB local footprint

## Validation Run

Passed:
- `npx tsc --noEmit --pretty false`
- `npm run lint:tokens`

Failed:
- `npm run i18n:check`

i18n failure details:
- 234 missing EN translations.
- 94 extra EN keys not present in CS.
- 3,248 suspicious identical CS/EN values remain under the quality check.
- New quality regression in `conversions.ts`: 32 suspicious keys, previously 25, increase of 7.

Not run in this basic pass:
- Full `npm run test`
- Full `npm run lint`
- Full `npm run build`
- `npm audit`

## Security Hygiene

`.env` is present and still tracked by Git:

- `git ls-files .env` returns `.env`
- local repo `.git-credentials`: absent

I did not read or print any secret values. The tracked `.env` should remain a P0 check before deploy or sharing.

## Operational Risks

P0 - Secret hygiene:
- `.env` is tracked in Git. If it contains real credentials, rotate affected secrets and remove it from tracked history using an approved process.

P1 - i18n gate red:
- The i18n gate is worse than the 2026-07-10 audit snapshot: missing EN keys increased from 211 to 234, and `conversions.ts` regression increased from +6 to +7.
- This blocks reliable release gating and can break user-facing flows in CZ/EN.

P1 - Affiliate/CSS flow is active branch scope:
- The current branch contains affiliate registration and CSS lead workflow changes.
- This is business-critical surface: signup, partner onboarding, commission emails, and checkout/customer journey.

P2 - Large app surface:
- The project has many modules, hooks, stores, edge functions, and migrations.
- Without strict gates, small changes can create regressions across onboarding, billing, i18n, or auth.

## Recommended Next Priorities

1. P0 security hygiene:
   - Confirm whether tracked `.env` contains real secrets.
   - If yes, rotate affected credentials and remove tracked `.env` through an approved cleanup plan.

2. Fix i18n gate:
   - Run `npm run i18n:sync`.
   - Review generated translation changes instead of blindly accepting bad copy.
   - Fix the `conversions.ts` quality regression with real Czech values or update baseline only if intentionally accepted.

3. Run release-quality checks:
   - `npm run test`
   - `npm run lint`
   - `npm run build`
   - `npm audit --audit-level=moderate --omit=dev`

4. Validate affiliate/CSS business flow:
   - Affiliate registration.
   - Partner approval notification.
   - Commission currency/rate behavior.
   - CSS lead workflow from acquisition to onboarding handoff.

## Escalation Threshold

Escalate immediately if:
- `.env` contains production secrets.
- Affiliate registration or CSS onboarding fails in production.
- Stripe/CSS checkout tests are still red before a deploy decision.

## Confidence

Medium-high for repository state and immediate technical gates.
Medium for release readiness, because full lint/test/build were not run in this basic pass.
