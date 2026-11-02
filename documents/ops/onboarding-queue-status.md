# Onboarding Queue Status

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

## Overview
Need operational data source for onboarding queue tracking

## Current Status
- **Last Check**: 2026-09-02T06:18:02Z
- **Data Source**: No live queue source identified in workspace; manual fallback only
- **Queue State**: Unknown — no authoritative live onboarding queue metrics in workspace
- **Flow State**: Acceptance path is not blocked by browser-session approval; Jiří approved a manual verification alternative on 2026-08-09
- **Decision Note**: Older docs referring to a browser-session-only onboarding blocker are stale and should not drive escalation.

## Action Items
- [ ] Identify onboarding queue tracking system
- [ ] Confirm whether any customers are currently waiting on manual verification evidence
- [ ] Update with current metrics once a live source exists

## Notes
- Reviewed `CURRENT-WORK.md`, `documents/growili/ops/onboarding-queue-check-2026-08-30-1319utc.md`, and `documents/ops/onboarding_block.md`.
- `documents/growili/ops/onboarding-queue-check-2026-08-30-1319utc.md` still contains stale language claiming P1 is blocked by browser evidence; CURRENT-WORK and onboarding_block confirm that was superseded by DEC-2026-08-09-01.
- No actual queue system or current user-count source is documented in workspace.
