# Ops Source-of-Truth Map — V2 (confirmation-grade)

> **Status:** HISTORICAL SNAPSHOT  
> Stav zdrojů je nutné před použitím znovu ověřit; viz `README.md`.

**Date:** 2026-08-02  
**Prepared by:** Oto, COO 🧭  
**Purpose:** Confirm each operational lane with an explicit status only: `live authoritative`, `manual fallback`, or `unavailable`.

## Decision rule used
If a live authoritative system could not be confirmed from current evidence, the lane is marked only as:
- `manual fallback` — if a concrete manual artifact/process exists and can be used temporarily
- `unavailable` — if no trustworthy live or manual authoritative source is confirmed

No implied authority. No design-time system is treated as live without evidence.

## Escalate immediately if
- capacity reaches **>=85%**
- a **customer-impact incident** appears
- any **critical source remains unavailable next cycle**

---

## Confirmation table

| Lane | Explicit status | Authoritative live system | Owner | Confirmed fallback if live unavailable | Current blocker | Last verified |
|---|---|---|---|---|---|---|
| Lead queue | **manual fallback** | **Not confirmed** | CSO primary; COO monitors SLA | `main/documents/strategy/growili/pipeline-integrity-check.md` manual review artifact, but it is currently blocked and only usable as a manual checkpoint document | No paired nodes; no confirmed live CRM/lead system; pipeline feed blocked since 2026-07-13 | 2026-08-02 review of pipeline file + prior heartbeat evidence |
| Onboarding queue | **manual fallback** | **Not confirmed** | CSO + Delivery operationally; COO monitors | `coo/documents/ops/onboarding-queue-status.md` plus onboarding acceptance artifacts as temporary manual tracking docs | Intended Google Sheets queue is not confirmed as live authoritative today | 2026-08-02 review of queue/status docs |
| Support backlog | **manual fallback** | **Not confirmed** | COO monitoring owner confirmed; live support-system owner not confirmed | `coo/documents/support/backlog-status.md` as temporary manual status file | No confirmed support desk integration or live ticket feed | 2026-08-02 review of backlog/status docs |
| Capacity planning | **manual fallback** | **Not confirmed** | COO | `coo/documents/ops/capacity-check.md` + daily ops planning docs + active priorities docs | No authoritative live planning board or delivery-load system confirmed | 2026-08-02 review of capacity/planning docs |
| Finance | **manual fallback** | **Not confirmed** | COO monitors; credential/integration dependency on Steve + CTO | `coo/documents/finance/signal-status.md` as temporary manual status file | FIO integration requirements exist, but no credentials/live feed are confirmed | 2026-08-02 review of finance docs |

---

## Why each lane is marked this way

### 1) Lead queue — `manual fallback`
A live authoritative lead system is **not confirmed**.

What is confirmed:
- `main/documents/strategy/growili/pipeline-integrity-check.md` exists as the current manual checkpoint artifact.
- That same file explicitly says the lane is **BLOCKED** because of unpaired nodes and no live data feed since 2026-07-13.
- The acquisition-to-lead contract still says the internal CRM / destination is **to be determined**.

Reason for status:
- Not `live authoritative`: no live system confirmed.
- Not `unavailable`: there is at least a concrete manual checkpoint artifact, even if it is blocked as a live feed.

### 2) Onboarding queue — `manual fallback`
A live authoritative onboarding queue is **not confirmed**.

What is confirmed:
- The SOP names Google Sheets as the intended onboarding queue.
- The current local queue status document exists and explicitly says the operational data source still requires verification.

Reason for status:
- Not `live authoritative`: Google Sheets is documented as intended, not confirmed live.
- `manual fallback` is valid because the status file and acceptance docs can serve as temporary manual tracking artifacts.

### 3) Support backlog — `manual fallback`
A live authoritative support system is **not confirmed**.

What is confirmed:
- A support desk requirements doc exists.
- A local backlog status file exists for manual status capture.

Reason for status:
- Not `live authoritative`: no Zendesk/Freshdesk/Intercom or equivalent live system is confirmed.
- Not `unavailable`: a concrete manual status artifact exists.

### 4) Capacity planning — `manual fallback`
A live authoritative capacity board is **not confirmed**.

What is confirmed:
- We have a local `capacity-check.md` file.
- We have daily ops planning docs and current priorities.

Reason for status:
- Not `live authoritative`: no actual planning board/system is confirmed.
- `manual fallback` applies because operations can still review current commitments manually from the available planning artifacts.

### 5) Finance — `manual fallback`
A live authoritative finance feed is **not confirmed**.

What is confirmed:
- FIO setup requirements exist.
- A local finance signal status file exists for manual status recording.

Reason for status:
- Not `live authoritative`: no credentials or live bank feed are confirmed.
- `manual fallback` applies because a temporary manual finance status artifact exists.

---

## What V2 confirms
- **0 lanes confirmed as `live authoritative`**
- **5 lanes confirmed as `manual fallback`**
- **0 lanes confirmed as `unavailable`** under the current evidence set, because each lane has at least one concrete manual status or checkpoint artifact

## Operational implication
Heartbeat can be run only in a **manual-fallback mode**, not as live automated operational monitoring.

That means:
- status can be documented,
- risk can be surfaced,
- but confidence must stay limited until at least one lane is upgraded to `live authoritative`.

## Next required confirmation work
To upgrade any lane from `manual fallback` to `live authoritative`, confirm all three:
1. exact live system
2. owner
3. access/refresh path that works now

## Recommended immediate next action
Use this V2 as the decision-grade baseline for the next heartbeat. If any lane is still not live by next cycle, report it explicitly as **manual fallback** rather than as an implied live source.
