# Growili onboarding/blocking status — 2026-08-12

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

**Priority:** `P1-ONBOARDING-ACCEPTANCE`  
**Owner:** COO  
**Status:** BLOCKED  
**Related priority:** `P0-RELEASE-GATES` remains visibility-limited on live operational reporting  

## Decision
- **P1 onboarding acceptance remains blocked.**
- **P0/P1 operational reporting is still partially blind.**

## Why
1. No authenticated browser evidence package exists for P1.
2. The current onboarding acceptance file is a procedure, not proof.
3. Finance, support, and lead-response sources are still placeholders or unverified, so operational reporting is not yet dependable end to end.

## Risk
- Onboarding reliability cannot be proven end to end.
- Reporting may hide real operational failures.
- Escalate immediately if a customer-impact onboarding delay happens.

## Evidence base
- `CURRENT-WORK.md` states Phase 1 cannot start until authenticated browser evidence exists or Jiří approves an alternative proof path.
- `documents/growili/ops/onboarding-phase0-acceptance-plan.md` defines the required proof package and explicitly says the current gap is missing authenticated browser evidence.
- `documents/growili/ops/release-readiness-2026-07-27.md` confirms release reporting must stay evidence-backed and stale or unverified signals must not be treated as cleared.

## Required next action
Jiří approval is needed for one of these paths:
1. **Authenticated browser session** for acceptance verification, or
2. **Alternative approved proof path** for P1.

Live owners also need to be assigned or confirmed for restoring verified source-of-truth feeds for:
- lead response,
- support,
- finance.

## Escalation threshold
Escalate immediately if:
- any customer onboarding is delayed and proof of root cause is missing,
- any support/lead/finance incident appears in a feed that is still placeholder-based,
- anyone claims P1 is verified without the required evidence package.

## COO recommendation
Keep `P1-ONBOARDING-ACCEPTANCE` blocked and treat current ops reporting as partial until authenticated proof and live source ownership are both in place.
