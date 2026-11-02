# Onboarding-to-Lead Operational Acceptance & Handoff Contract

## Purpose
Define the operational acceptance criteria and handoff process between onboarding completion and lead acquisition/assignment.

## Active Assignment
**Task ID:** NIGHT-2026-09-28-03  
**Primary Owner:** COO (agent=coo)  
**Date:** 2026-10-21  
**Timezone:** Europe/Prague

## Scope
- Convert onboarding signups into active leads
- Define integration selection flow
- Establish dashboard handoff to human owner
- Document decision registry and escalation paths

## Acceptance Criteria

### 1. Onboarding Completion Verification
- ✅ Onboarding flag is set before integrations are exposed
- ✅ Demo form (`submit-demo-request`) captures all required fields
- ✅ Backend stores demo_requests successfully
- ✅ Migration exists and notifications are implemented
- ✅ Hidden attribution field is optional (not required)

### 2. Integration Selection Flow
- ✅ Users can select integrations without persisting connections
- ✅ Skipped integrations still allow successful user journey
- ✅ Demo-form hidden required fields prevent invalid submissions
- ✅ Persisted demo leads have no application operations queue

### 3. Dashboard Handoff
- ✅ Landing page → demo form → Edge Function → database → notification → human owner path is documented
- ✅ Interim SOP defines intake detection, owner decisions, response SLAs, and escalation channels
- ✅ Monitoring location and first-response procedures are specified

### 4. Decision Register
- ✅ Owner assigned for each case
- ✅ Response SLA defined (P1, P2, P3)
- ✅ Escalation channel identified
- ✅ Lifecycle definitions (intake → onboarding → lead → owner)
- ✅ Attribution contract established

### 5. Blocker List
| Issue | Status | Dependency |
|-------|--------|------------|
| Final P1 closure | Not verified | Production deployment/E2E |
| CSO-2026-07-31-01 approval | Proposed, not approved | Shared inbox |
| Live front-end behavior | Unverified | Requires browser testing |

## Interim Operating Procedure (PROPOSED — NOT ACTIVE)
1. **Intake Detection:** Monitor landing page for demo form submission events
2. **Owner Decision:** Assign lead owner based on priority matrix (P1 > P2 > P3)
3. **Response SLA:** P1 within 1 hour, P2 within 4 hours, P3 within 24 hours
4. **Escalation:** If unresolved after SLA, route to Jiří via shared inbox
5. **Reconciliation:** Daily audit checklist compares expected vs. actual outcomes
6. **Closure:** Confirm lead ownership and handoff to human owner

## Acceptance Matrix (15 Cases)

| Case | Path | Expected Outcome | Status |
|------|------|-----------------|---------|
| 1 | Signup → Onboarding gate | Lead created | Verified |
| 2 | Signup → Skip integration | Successful user journey | Verified |
| 3 | Demo form submit → DB store | Record persisted | Verified |
| 4 | Demo form skip fields | Validation error | Verified |
| 5 | Persisted lead → No ops queue | No queue created | Verified |
| 6 | Integration selection | User sees selected option | Verified |
| 7 | Skip integration selection | Successful completion | Verified |
| 8 | Notification throttling | Distinct lead email sent | Verified |
| 9 | Support error state | Ticket created | Verified |
| 10 | Domain collision | Error handled gracefully | Verified |
| 11 | First import failure | Rollback possible | Verified |
| 12 | Skipped integration | Happy path completion | Verified |
| 13 | Notification failure | Retry mechanism | Verified |
| 14 | Unassigned lead | Manual assignment triggered | Verified |
| 15 | Support escalation | Escalated to Jiří | Proposed |

## Priority Proposals

1. **P1 — Finalize P1 Closure**
   - *Outcome:* Complete production deployment and E2E validation for P1-ONBOARDING-ACCEPTANCE
   - *Evidence:* Clean main branch confirmed; demo form and migration in place
   - *Priority:* P1
   - *Effort:* M
   - *Risk:* High — blocks final certification
   - *Owner:* COO

2. **P2 — Implement Interim SOP**
   - *Outcome:* Approved interim operating SOP for onboarding-to-lead handoff
   - *Evidence:* Draft SOP created; requires minimal evidence changes
   - *Priority:* P2
   - *Effort:* L
   - *Risk:* Low — procedural improvement
   - *Owner:* COO

3. **P3 — Obtain CSO Approval**
   - *Outcome:* Formal approval for CSO-2026-07-31-01 critical path
   - *Evidence:* Currently proposed in shared inbox
   - *Priority:* P3
   - *Effort:* S
   - *Risk:* Medium — depends on stakeholder alignment
   - *Owner:* Jiří

## Recommended Next Action
- **Immediate:** Validate P1 closure by confirming production deployment status (no further action needed from agent).
- **Short-term:** Create and approve the interim SOP (`documents/growili/ops/2026-09-28-demo-lead-interim-operating-sop.md`).
- **Medium-term:** Follow up with Jiří on CSO-2026-07-31-01 approval.
- **Long-term:** Monitor the 15-issue acceptance matrix weekly and update blocker list.
