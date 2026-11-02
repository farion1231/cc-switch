# P1 onboarding acceptance — decision brief for Jiří

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

> **Status:** HISTORICAL / SUPERSEDED  
> Aktuální rozhodovací bránu určuje `onboarding-phase0-acceptance-plan.md`.

**Priority:** `P1-ONBOARDING-ACCEPTANCE`  
**Status:** **BLOCKED**  
**Decision needed:** approve authenticated browser access for acceptance, or approve an alternative proof path  
**Owner:** COO  
**Date:** 2026-08-17 UTC

## 1. Required proof
To clear P1, we need a **real authenticated acceptance run** for both states:
- **no purchased module**
- **purchased analytics module**

For each run, the minimum valid evidence is:
- user/shop state
- tested route
- visual capture
- console evidence
- network evidence
- pass/fail result with timestamp

Phase 1 can start only if `/get-started`, sidebar auto-completion, overview next step, console warnings, and network repetition all pass in a real authenticated session.

## 2. Why current evidence is insufficient
Current evidence is **procedure only, not proof**.

What we have:
- onboarding acceptance plan
- source-plan requirements
- audit notes saying browser verification is still required

What we do **not** have:
- authenticated browser run artifact
- proof for free/no-module state
- proof for paid analytics state
- console capture proving no loop warnings
- network capture proving queries settle without repetition

So the current safe status is:
**P1 remains blocked. Anything stronger would be false confidence.**

## 3. Safest alternative proof path
**Best path:** approve COO/QA use of an authenticated browser session against an approved test or real account with both required module states.

**If browser access is not approved, safest alternative:**
approve an owner-run authenticated proof pack captured by a trusted operator using the exact COO checklist.

That alternative proof pack must include:
- same two states: no-module + purchased analytics
- route captures for `/get-started` and overview
- sidebar before/after deferred sync
- console export/screenshot showing absence or presence of named warnings
- network capture/export showing query count and whether requests settle
- account/shop label, timestamp, and tester identity

This is safer than accepting screenshots alone, because screenshots cannot prove console stability or request-loop behavior.

## 4. Minimum artifacts needed to restore ops visibility
To restore minimum COO heartbeat visibility, we need one compact evidence pack covering:

### A. Onboarding proof artifact
- authenticated acceptance pack for P1
- pass/fail by scenario
- blocker notes if any

### B. Lead heartbeat
- current lead intake status
- capture path/owner
- latest known failure/no-failure note

### C. Support heartbeat
- open support load or explicit "no active queue" evidence
- escalation threshold

### D. Capacity heartbeat
- current delivery/onboarding capacity status
- constraint or explicit green status

### E. Finance heartbeat
- owner/status for FIO verification
- whether finance-risk visibility is current or stale

## Decision requested from Jiří
Choose one:
1. **Approve authenticated browser access** for COO acceptance.
2. **Approve owner-run alternative proof pack** as the accepted evidence path.
3. **Hold P1 blocked** and defer Phase 1 until one of the above is available.

## COO recommendation
**Recommendation: option 1.** It is the shortest and most reliable path to real acceptance proof and removes the biggest readiness blind spot with the least ambiguity.
