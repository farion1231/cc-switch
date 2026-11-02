# Ops Source-of-Truth Map — V1

> **Status:** HISTORICAL / SUPERSEDED BY `ops-source-of-truth-map-v2-2026-08-02.md`  
> Nepoužívat jako aktuální provozní stav; viz `README.md`.

**Date:** 2026-08-02  
**Prepared by:** Oto, COO 🧭  
**Purpose:** Establish the first explicit source-of-truth map for key operational lanes and make heartbeat reliability gaps visible instead of inferred.

## Executive read
Current operational monitoring is **source-degraded**. We have partial design-time documentation for several lanes, but we do **not** have confirmed live connected systems for reliable heartbeat validation across lead queue, onboarding queue, support backlog, capacity planning, and finance.

This is a **P1 reliability gap**, not yet a red-alert escalation.

## Escalate immediately if
- capacity reaches **>=85%**
- a **customer-impact incident** appears
- any **critical source remains unavailable next cycle**

## Status scale
- **Live** = connected, current, and usable for routine heartbeat checks
- **Stale** = documented or previously checked, but not verified as live now
- **Unknown** = source not confirmed
- **Source-degraded** = no authoritative live source currently available to trust

---

## V1 Source-of-Truth Map

| Metric / queue name | Authoritative system | Exact location | Owner | Refresh cadence | Live vs stale | Last verified timestamp | Fallback manual source | Current blocker | Escalation threshold |
|---|---|---|---|---|---|---|---|---|---|
| Lead queue / first-touch SLA | **Target design:** Google Form -> Supabase; **operational destination still TBD** | SOP names capture path in `coo/documents/coo/ops/css-lead-flow-sop.md` §5.1; contract still says destination TBD in `main/documents/growili/sales/acquisition-to-lead-contract-2026-07-27.md` | CSO primary; COO monitors SLA | Intended near-real-time; SLA threshold <2h first touch | **Source-degraded** | 2026-07-27 05:17 UTC heartbeat confirmed no live lead feed | `main/documents/strategy/growili/pipeline-integrity-check.md` if restored; manual CSO review | No paired/live pipeline source; internal CRM destination not finalized; pipeline monitoring blocked | Escalate if any lead feed remains unavailable next cycle or first-touch breaches >2h cannot be checked |
| Onboarding queue / stuck >48h | **Target design:** Google Sheets onboarding queue | Named in `coo/documents/coo/ops/css-lead-flow-sop.md` §5.4; current local status file: `coo/documents/ops/onboarding-queue-status.md` | CSO + Delivery operationally; COO monitors | Intended daily / event-driven during active onboarding | **Stale / source-degraded** | 2026-07-31T13:19:00Z noted in local status file, but source itself remains unverified | Manual onboarding acceptance evidence in `coo/documents/growili/ops/onboarding-phase0-acceptance-plan.md` and direct queue review when source is identified | Queue system documented but not connected/verified; no current stuck-customer count available | Escalate if customers can be in onboarding without a verified queue source next cycle, or any case appears >48h |
| Support backlog / SLA breaches | **Unknown live support desk**; requirements mention Zendesk/Freshdesk/Intercom class system | Requirements doc: `main/documents/coo/support/desk-setup-requirements.md`; current local status file: `coo/documents/support/backlog-status.md` | COO for monitoring; support owner not yet explicitly assigned in live system | Intended every 30 min polling once connected | **Source-degraded** | 2026-07-31T13:19:00Z noted in local status file, but no live desk verified | Manual review of customer issue threads / support requests if surfaced through sessions or shared docs | No support desk integration credentials/source confirmed; no live ticket feed | Escalate on any customer-impact support incident or if backlog source remains unavailable next cycle |
| Capacity planning / commitments load | **Unknown authoritative planning board** | Current placeholder status file: `coo/documents/ops/capacity-check.md`; daily planning artifact: `coo/documents/growili/daily-ops/daily-ops-2026-07-23.md` | COO | Daily / weekly planning cycle | **Source-degraded** | 2026-07-31T13:19:00Z noted in local status file; no live capacity board verified | Daily ops plans and active priorities in `CURRENT-WORK.md` + `CURRENT-PRIORITIES.md` | No current capacity board, staffing model, or delivery-load source connected | Escalate immediately if estimated capacity reaches >=85% or overcommit becomes visible |
| Finance / cashflow signal | **Target design:** FIO Bank read-only integration | Requirements doc: `main/documents/coo/finance/fio-setup-requirements.md`; current local status file: `coo/documents/finance/signal-status.md` | COO monitors; Steve/CTO required for credentials/integration | Intended every 15 min polling once connected | **Source-degraded** | 2026-07-31T13:19:00Z noted in local status file; no live bank data verified | Manual invoice / obligation review if shared externally; finance notes in workspace docs | FIO credentials not provided; integration not activated; no paired finance/bank source | Escalate on any cashflow risk, overdue-obligation signal, or if finance source remains unavailable next cycle |

---

## Lane-by-lane notes

### 1) Lead queue
**Current read:** The process design is more mature than the live source. The SOP identifies the intended capture path as Google Form -> Supabase, but the acquisition-to-lead contract still says the actual internal CRM / destination is **to be determined**.

**Operational implication:** We do not yet have a single confirmed authoritative live queue for heartbeat checks.

**Evidence:**
- `coo/documents/coo/ops/css-lead-flow-sop.md`
- `main/documents/growili/sales/acquisition-to-lead-contract-2026-07-27.md`
- `coo/documents/reports/heartbeat-2026-07-27-ops-check.md`

### 2) Onboarding queue
**Current read:** The intended queue is Google Sheets, but the current status doc explicitly says the source is unknown and requires verification.

**Operational implication:** The queue concept exists, but not as a confirmed live source of truth.

**Evidence:**
- `coo/documents/coo/ops/css-lead-flow-sop.md`
- `coo/documents/ops/onboarding-queue-status.md`

### 3) Support backlog
**Current read:** We have setup requirements for a support desk integration, but no confirmed desk system or live feed.

**Operational implication:** Support SLA monitoring is currently not evidence-backed.

**Evidence:**
- `main/documents/coo/support/desk-setup-requirements.md`
- `coo/documents/support/backlog-status.md`

### 4) Capacity planning
**Current read:** We have planning artifacts and priorities, but not an authoritative live capacity board.

**Operational implication:** Capacity risk can only be estimated manually, not monitored reliably.

**Evidence:**
- `coo/documents/ops/capacity-check.md`
- `coo/documents/growili/daily-ops/daily-ops-2026-07-23.md`
- `CURRENT-WORK.md`
- `main/documents/strategy/growili/CURRENT-PRIORITIES.md`

### 5) Finance
**Current read:** Requirements exist for FIO integration, but activation and credentials are still missing.

**Operational implication:** Cashflow monitoring remains blind unless checked manually.

**Evidence:**
- `main/documents/coo/finance/fio-setup-requirements.md`
- `coo/documents/finance/signal-status.md`

---

## Reliability conclusion
The main problem is **not lack of documents**. The main problem is that the documents describe intended sources, while heartbeat needs **verified live authoritative systems**.

Today’s V1 map therefore confirms:
- the lanes we need,
- the intended system per lane where known,
- the current owner assumptions,
- and the fact that most lanes remain **source-degraded**.

## Recommended next action
Create **V2** only after confirming, for each lane, one of the following:
1. a real live system is connected and usable, or
2. a temporary manual authoritative fallback is explicitly approved.

## Immediate COO action list
1. Confirm the actual live system for the **lead queue**.
2. Confirm whether **Google Sheets** is the real live onboarding queue or only an intended design.
3. Identify the chosen **support desk** system or mark support as manually tracked.
4. Define the authoritative **capacity board** or manual weekly load sheet.
5. Confirm whether **FIO integration** will be activated or finance remains manual for now.
