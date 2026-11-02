# Ops Source-of-Truth Map Brief — 2026-08-02

> **Status:** HISTORICAL / SUPERSEDED  
> Nepoužívat jako aktuální provozní stav; viz `README.md`.

## Context
This follow-up addresses a **P1 operational reliability gap**: heartbeat quality is limited by missing or stale live operations sources. The immediate problem is not only capacity visibility, but the reliability of the underlying operating signals.

## Decision
Treat missing live ops sources as **reliability debt requiring cleanup now**.

This is **not a red-alert escalation yet**.

## Escalate only if one of these triggers trips
- capacity reaches **>=85%**
- a **customer-impact incident** occurs
- required live sources are **still unavailable next cycle**

## Risk
Stale snapshots can hide a real onboarding or service issue until it becomes customer-visible.

## Required deliverable
Build a source-of-truth map for these operational lanes:
1. lead queue
2. onboarding queue
3. support backlog
4. capacity planning
5. finance

For each lane, record:
- **authoritative source**
- **owner**
- **refresh cadence**
- **current status**

## Recommended template

| Lane | Authoritative source | Owner | Refresh cadence | Current status | Reliability gap | Escalation threshold |
|---|---|---|---|---|---|---|
| Lead queue | [fill] | [fill] | [fill] | [green/yellow/red + note] | [fill] | [fill] |
| Onboarding queue | [fill] | [fill] | [fill] | [green/yellow/red + note] | [fill] | [fill] |
| Support backlog | [fill] | [fill] | [fill] | [green/yellow/red + note] | [fill] | [fill] |
| Capacity planning | [fill] | [fill] | [fill] | [green/yellow/red + note] | [fill] | [fill] |
| Finance | [fill] | [fill] | [fill] | [green/yellow/red + note] | [fill] | [fill] |

## COO interpretation
- This work fits current COO scope because it strengthens acceptance evidence and operational readiness without changing production.
- It should be treated as an operational reliability artifact supporting:
  - `P1-ONBOARDING-ACCEPTANCE`
  - heartbeat quality and escalation discipline
- Do not infer health from stale snapshots once a lane is marked source-degraded.

## Next action
Create the first populated version of the source-of-truth map and use it to determine whether heartbeat checks are evidence-backed or source-degraded.
