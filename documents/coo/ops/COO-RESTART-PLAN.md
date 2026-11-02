# COO Restart & Operational Restoration Plan
**Date:** 2026-07-24 | **Prepared by:** COO | **Status:** DRAFT for Steve's review

## Executive Summary
The COO workspace is functional but **data sources are disconnected**. All 6 HEARTBEAT checks show null timestamps because we lack live connections to CRM, support desk, FIO bank, and capacity tracker. The CSO pipeline is blocked (missing `pipeline-integrity-check.md` since 2026-07-13). This plan restores operational visibility within 24h.

---

## Current State Assessment

| Check | Last Run | Status | Blocker |
|-------|----------|--------|---------|
| Lead Response Time | Never | ❌ No data | No CRM connection |
| Onboarding Queue | Never | ❌ No data | No onboarding tracker |
| Support Backlog | Never | ❌ No data | No support desk |
| Capacity vs Commitments | Never | ❌ No data | No capacity tracker |
| Finance/Cashflow | Never | ❌ No data | No FIO bank feed |
| Process Gaps | 2026-07-23 | ⚠️ Partial | Documented SOPs exist but no live validation |

**Key finding from prior heartbeats:** "No connected data sources (CRM, support desk, FIO bank feed, capacity tracker) in this environment, so all six operational metrics remain N/A."

---

## Root Cause: Three Missing Connections

1. **CSO Pipeline** — `pipeline-integrity-check.md` deleted/lost on 2026-07-13; Jiří (CSO) needs to re-pair nodes or provide alternative data source
2. **Finance Feed** — FIO bank integration not configured; no cashflow visibility
3. **Support Desk** — No ticketing system connected; support backlog unmonitorable

---

## Phase 1: Immediate (Next 4 Hours) — COO Owned

| Action | Owner | Output Path | Success Criterion |
|--------|-------|-------------|-------------------|
| 1.1 Create minimal `pipeline-integrity-check.md` template | COO | `/documents/coo/ops/pipeline-integrity-check.md` | File exists with required fields; CSO can populate |
| 1.2 Document FIO bank connection requirements | COO | `/documents/coo/finance/fio-setup-requirements.md` | Spec ready for CTO handoff |
| 1.3 Document support desk connection requirements | COO | `/documents/coo/support/desk-setup-requirements.md` | Spec ready for CTO handoff |
| 1.4 Update `heartbeat-state.json` with current timestamp | COO | `/memory/heartbeat-state.json` | All 6 checks show today's timestamp (even if N/A) |
| 1.5 Send status report to Steve | COO | via `sessions_send` | Steve has visibility |

---

## Phase 2: CSO Dependency (Next 8 Hours) — Requires Jiří

| Action | Owner | Dependency | Output |
|--------|-------|------------|--------|
| 2.1 Jiří re-pairs CRM nodes or provides CSV export | CSO (Jiří) | Steve approval if new OAuth needed | Live lead data or manual import |
| 2.2 Populate `pipeline-integrity-check.md` with current deals | CSO | 2.1 complete | All 6 CSO priorities trackable |
| 2.3 Validate lead response times from live data | CSO | 2.2 complete | HEARTBEAT check #1 functional |

---

## Phase 3: CTO Dependency (Next 24 Hours) — Requires CTO

| Action | Owner | Dependency | Output |
|--------|-------|------------|--------|
| 3.1 Configure FIO bank read-only API | CTO | Finance credentials from Steve | Live cashflow feed |
| 3.2 Configure support desk webhook/API | CTO | Support desk credentials | Live ticket feed |
| 3.3 Connect capacity tracker (Notion/Sheets) | CTO | Access from Steve | Live capacity feed |
| 3.4 Verify all 6 HEARTBEAT checks return data | CTO + COO | 3.1-3.3 complete | Operational monitoring restored |

---

## Escalation Triggers (Per AGENTS.md)

| Trigger | Escalate To | Timeline |
|---------|-------------|----------|
| `pipeline-integrity-check.md` not recreated by EOD today | Steve (CEO) | 5 hours from now |
| FIO credentials not provided by Steve | Steve | 24 hours |
| Support desk credentials not provided | Steve | 24 hours |
| Any customer-impact incident detected | Steve | Immediate |

---

## Deliverables for Steve's Review

1. **This plan** — `/documents/coo/ops/COO-RESTART-PLAN.md`
2. **Pipeline template** — `/documents/coo/ops/pipeline-integrity-check.md` (Phase 1.1)
3. **FIO requirements** — `/documents/coo/finance/fio-setup-requirements.md` (Phase 1.2)
4. **Support desk requirements** — `/documents/coo/support/desk-setup-requirements.md` (Phase 1.3)

---

## Report Back to Steve (AGENTS.md Format)

```
TO: Steve
FROM: Oto, COO 🧭
TASK: COO Operational Restoration Plan
STATUS: draft
OUTPUT: /home/growthhacker/.openclaw/workspaces/coo/documents/coo/ops/COO-RESTART-PLAN.md
RISK / BOTTLENECK: CSO pipeline blocked since 2026-07-13 — requires Jiří's explicit node pairing or data export
ESCALATION TRIGGER: If pipeline-integrity-check.md not populated by EOD today → escalate to CEO
CONFIDENCE: medium (plan is solid; execution depends on CSO/CTO dependencies)
NEXT ACTION: Await Steve approval to execute Phase 1; notify Jiří for Phase 2
```

---

## Approval Requested

Steve: Please reply with **APPROVE** to proceed with Phase 1, or provide modifications. I'll begin execution immediately upon approval.