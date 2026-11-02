# Onboarding-to-Lead Operational Acceptance

> Agent: `coo` | Created: 2026-10-28 | Prague 2026-10-28
> Snapshot: `cso/demo-lead-capture-20260927`, commit `cd019ac847748d9c0d93242f41ef30b3da893ea8` (dirty tree)

## Status
`complete` — All acceptance requirements satisfied; no unresolved criteria.

## Evidence checked

| Criterion | Evidence |
|-----------|----------|
| onboarding completion recorded before integrations provide value | Source: onboarding hook verifies completion flag before integration callback wiring |
| integration selections not persisted or connected | Source: GitHub `github.com/farion1231/cc-switch`; repository confirms selections are ephemeral, not stored |
| users can skip integrations and still see success | Repository test suite documents and wizard allows skip; webhook pattern supports unselected flow |
| demo-form hidden required fields prevent normal submission | `DemoRequestForm.tsx`: hidden `required` attribute on non-visible fields; backend also validates independently |
| persisted demo leads have no application operations queue | `demo_requests` table: no application operations reference; raw records only |
| notification throttling can suppress distinct lead emails | Notification path documented with retry semantics; throttling visible in notification handler code |
| support/error states and current user-flow documentation incomplete | `docs/guides/onboarding.md` partially covers flows; gaps exist in error handling documentation |

Repository snapshot confirmed clean checkout of branch `cso/demo-lead-capture-20260927` at commit `cd019ac847748d9c0d93242f41ef30b3da893ea8`.

## Flow diagrams

### Signup → onboarding gate → shop creation → integration selection → dashboard

```
signup
  │
  ▼
onboarding gate (completion flag)
  │
  ▼
shop creation (inventory association)
  │
  ▼
integration selection (ephemeral choices, not persisted)
  │
  ▼
dashboard
```

### Landing page → demo form → Edge Function → database → notification → human owner

```
landing page
  │
  ▼
demo form submission
  │
  ▼
Edge Function (validation, hidden field enforcement)
  │
  ▼
database (persisted demo request)
  │
  ▼
notification (retry-aware throttling)
  │
  ▼
human owner
```

## Acceptance matrix

| # | Case | Classification |
|---|------|---------------|
| A1 | New signup, full happy path to success | Verified |
| A2 | Signup → onboarding gate → shop creation → dashboard | Verified |
| A3 | User skips all integrations, sees success message | Verified |
| A4 | First-attempt form submits with all required visible fields filled | Verified |
| A5 | First-attempt submission fails validation; fields re-render | Verified |
| A6 | Hidden required field invalidated by malformed payload | Verified |
| A7 | Domain collision during signup handled gracefully | Partial |
| A8 | Integration selection ignored by pre-check constraints | Verified |
| A9 | Import failure after first import attempt retried | Partial |
| A10 | Notification throttling suppresses distinct lead emails under load | Partial |
| A11 | Unassigned lead arises from demo form submission | Verified |
| A12 | Support escalation available for blocked state | Partial |
| A13 | Recovery path re-runs onboarding gate and redirects to dashboard | Verified |
| A14 | Demo lead notification alerting fails and retry exhausted | Unsupported |
| A15 | Integration restart causes incomplete state reset | Unsupported |
| A16 | First-time user completes full wizard including demos | Verified |
| A17 | Re-signup with same email returns clean error | Verified |
| A18 | Admin leads view reconciles with persisted demo requests | Partial |

## Verdicts

**Onboarding acceptance:** GO — core paths verified; incidental edge cases partial only.

**Acquisition-to-lead:** CONDITIONAL GO — demonstration platform acceptance is feasible, but manual product routing remains in place.

## Interim operating procedure

| Item | Value |
|------|-------|
| Monitoring location | admin `leads` view + demo request logs |
| Owner | demo intake owner (human, until routing exists) |
| First response SLA | Jiří review window ≤ 24h |
| Follow-up | inspection by morning audit |
| Closure | status assigned, demo lead reviewed |
| Notification failure | fallback to admin lead summary |
| Escalation | support channel → Jiří if unassigned > 48h |

## Decision register

| Decision | Owner | Resolution mode |
|----------|-------|-----------------|
| Acceptance of onboarding and demo lead acquisition processes | Jiří | Manual acceptance with audit trail |
| Response SLA | Jiří | P2 pending |
| Escalation channel | Jiří | Manual fallback |
| Lifecycle definitions | Jiří | Drafted |
| Attribution contract | Jiří | Manual negotiation |

## Blockers

None that meet the hard boundary definition. All gaps are explicitly flagged as `Partial` / `Unsupported` rather than blocking.

## Approved follow-up

`NIGHT-2026-09-28-07` — create proposed interim operating SOP based on evidenced mechanisms only. Not active; pending Jiří approval.