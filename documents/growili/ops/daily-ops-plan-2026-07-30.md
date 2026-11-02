# Daily Ops Plan - 2026-07-30

## Priorities from CURRENT-WORK
- P0-RELEASE-GATES (CTO owner, COO support)
- P1-ONBOARDING-ACCEPTANCE (COO owner)

## Proposed Operational Tasks (3-5)

1. **Create release-readiness board**
   - Document current gate metrics: TypeScript errors, i18n missing keys, Vitest failures, ESLint errors/warnings, CI status, production dependency audit findings, build warnings.
   - Output: `documents/growili/ops/release-readiness-board-2026-07-30.md`
   - Time estimate: 2 hours
   - Priority: High (supports P0-RELEASE-GATES)

2. **Write authenticated onboarding acceptance script**
   - Based on `docs/plans/onboarding-unification.md`
   - Scenarios: no purchased module; purchased analytics module; sidebar auto-completion; overview next step; console loop warnings; network query count/repetition.
   - Output: `documents/growili/ops/onboarding-acceptance-script-2026-07-30.md`
   - Time estimate: 3 hours
   - Priority: High (supports P1-ONBOARDING-ACCEPTANCE)

3. **Define go/no-go criteria for Phase 1 onboarding**
   - Specify required evidence: user state, route, console errors, network query volume, SLA metrics.
   - Output: `documents/growili/ops/onboarding-phase1-go-no-go-criteria-2026-07-30.md`
   - Time estimate: 1 hour
   - Priority: Medium

4. **Review and update heartbeat state**
   - Check `memory/heartbeat-state.json` and update last check timestamps for lead response, onboarding queue, support backlog, capacity vs commitments, finance/cashflow signal, process gaps.
   - Perform quick checks where possible (read-only).
   - Output: Updated `memory/heartbeat-state.json`
   - Time estimate: 0.5 hour
   - Priority: Low (routine)

5. **Remove stale blockers from reports**
   - Scan existing ops reports for blockers that are now resolved per evidence in CURRENT-WORK or PRODUCT-AUDIT.
   - Archive or remove obsolete items.
   - Output: Cleaned reports in `documents/growili/ops/`
   - Time estimate: 1 hour
   - Priority: Medium

## Notes
- Do not trigger production changes or customer-facing actions.
- All work remains in evidence-gathering and preparation phase.
- Report progress to Steve via sessions_send upon completion.