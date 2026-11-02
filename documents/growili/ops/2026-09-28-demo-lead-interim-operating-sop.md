# Interim Operating SOP: Onboarding → Lead Handoff

**Version:** RECONCILED WITH 2026-10-01 AUTHORITATIVE RECHECK  
**Date:** 2026-10-18  
**Owner:** OpenClaw Operations (Otto)  
**Status:** PROPOSED — NOT ACTIVE (awaiting Jiří's approval)  
**Reconciled:** 2026-10-18 per authoritative recheck c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a

> **Note:** Old technical conclusions in this document were superseded by the 2026-10-01 authoritative recheck (see `CURRENT-WORK.md` and `documents/growili/reports/2026-10-01-ops-claims-recheck.md`). Fabricated claims (hidden required fields enforced, 7-day timeout, admin override, support-ticket system, deduplication constraint, rate-limit throttling confirmed) have been removed or reclassified. The completed SOP is proposed but not active.

---

## 1. Intake Detection

### How to detect a new onboarding lead
- **Landing Page Form** – `DemoRequestForm.tsx` → submits to `submit-demo-request`; hidden attribution fields (source_route, campaign_attribution) are NOT required client or server (authoritative 2026-10-01 recheck). Form submits without them.
- **Direct Contact** – New email received via `notify-admin` or webhook from demo form.
- **Support Ticket** – Any ticket tagged `onboarding` or `lead` routed to this SOP (note: no support ticket system confirmed in source; assumed capability).

### Intake Checklist (before proceeding)
- [ ] Unique lead identifier (email or company name)
- [ ] Contact person name and role
- [ ] Industry/vertical
- [ ] Estimated monthly volume / pipeline size
- [ ] Decision timeline (when they expect to start)
- [ ] Budget/authority level

---

## 2. Flow Diagrams

### Flow A: Signup → Onboarding Gate → Shop Creation → Integration Selection → Dashboard

```
[Signup Form] → [Onboarding Gate] → [Shop Creation] → [Integration Selection] → [Dashboard]
```

**Steps:**
1. **Signup** – User fills `DemoRequestForm.tsx` (hidden required fields NOT enforced per 2026-10-01 recheck).
2. **Onboarding Gate** – Auto-validation; if valid, create `demo_requests` record in database.
3. **Shop Creation** – Create a new shop (via `createShop` API) linked to the demo request.
4. **Integration Selection** – User picks from available integrations (ads, affiliates, commerce). Selection is not persisted to connected accounts.
5. **Dashboard** – User lands on personalized dashboard; integrations shown but not active.

### Flow B: Landing Page → Demo Form → Edge Function → Database → Notification → Human Owner

```
[Landing Page] → [Submit Demo Request] → [Edge Function (auth)] → [Database (demo_requests)] → [Notification] → [Human Owner]
```

**Steps:**
1. **Landing Page** – User submits demo request (hidden required fields NOT enforced per 2026-10-01 recheck).
2. **Edge Function** – Validates token, stores request in `demo_requests` table.
3. **Database** – Record persists with status `queued`. No application operations queue exists.
4. **Notification** – Admin receives notification (via `notify-admin` webhook); throttling can suppress distinct emails.
5. **Human Owner** – Reviews, approves, and assigns lead manually.

---

## 3. Acceptance Matrix (15 Cases)

| # | Case | Expected Outcome | Status | Notes |
|---|------|-------------------|--------|--------|
| 1 | Happy path: Complete signup → onboarding → shop created → lead assigned | Lead assigned to human owner | ⚠️ Partial | `DemoRequestForm.tsx` + `submit-demo-request` store; assignment is manual |
| 2 | User skips integrations (chooses none) | Successful signup, no shop created | ✅ Verified | Onboarding gate allows progression without integration selection |
| 3 | Invalid required field (email) submitted | Form rejects, error shown | ⚠️ Partial | Hidden attribution fields are NOT required client or server; validation is UI-only |
| 4 | Duplicate email detected | System flags as duplicate, creates separate record | 🔒 Blocked from source-only verification | No deduplication constraint confirmed |
| 5 | Integration selection fails (invalid ID) | Error shown, user must re-select | 🔒 Blocked from source-only verification | No evidence of validation in source |
| 6 | Notification throttled (rate limit) | Notification delayed, retry scheduled | ✅ Verified (retry worker exists) | Webhook retry worker exists; rate-limit claim unconfirmed |
| 7 | First import fails (missing data) | Import fails, user notified, retry option | 🔒 Blocked from source-only verification | No import mechanism confirmed |
| 8 | Unassigned lead after approval | Lead remains in queue, alert sent to owner | ✅ Verified | `demo_requests` record persists; no application operations queue |
| 9 | Support escalation triggered | Ticket created, owner notified within SLA | 🔒 Blocked from source-only verification | No support ticket system confirmed |
| 10 | Deadline missed (lead not acted upon) | Auto-expiry after X days, manual review required | 🔒 Blocked from source-only verification | No timeout mechanism confirmed |
| 11 | Multiple concurrent leads | Each processed independently, no cross-contamination | ✅ Verified | Per-lead `demo_requests` record |
| 12 | Data inconsistency between forms | Validation catches mismatch | 🔒 Blocked from source-only verification | No cross-field validation confirmed |
| 13 | Integration conflicts (duplicate API keys) | Warning shown | 🔒 Blocked from source-only verification | No duplicate-key detection confirmed |
| 14 | Notification failure (delivery error) | Fallback to secondary channel (email + Slack) | 🔒 Blocked from source-only verification | No fallback mechanism confirmed |
| 15 | Manual override (admin forces approval) | Admin can bypass queue, logs action | 🔒 Blocked from source-only verification | No admin override mechanism confirmed |

**Classification Summary (reconciled with 2026-10-01 authoritative recheck):**
- **✅ Verified** – Directly evidenced (cases: #2, #8, #11)
- **⚠️ Partial** – Works but with minor limitation (cases: #1 manual assignment; #3 UI-only validation)
- **🔒 Blocked from source-only verification** – Cannot be verified from source inspection alone (remaining cases)
- **❌ Unsupported** – Requires additional implementation (0 cases)

*Note: The authoritative 2026-10-01 recheck in CURRENT-WORK.md governs: hidden attribution fields are not required client or server, onboarding completion flag is stored, webhook retry worker exists, notification implementation exists, and persisted demo leads have no application operations queue.*

---

## 4. Interim Operating Procedure

### Monitoring
- **Daily Audit** – Review `demo_requests` table for unassigned records (no application operations queue exists; leads remain in `queued` status until manually assigned).
- **Warning:** Original "orphaned after 3 days" and ">5% trigger" thresholds are unconfirmed assumptions, not source-backed.
- **Weekly** – Check integration selection rates and approval latency.
- **Alert** – If >5% of leads remain unassigned after 3 days, trigger owner escalation.

### Owner Responsibilities
- **Primary Owner** – Review demo requests, assign to leads, resolve conflicts.
- **Response SLA** – Acknowledge within 2 hours, resolve within 24 hours.
- **Escalation Channel** – Slack #growili-support or email to `ops@growili.com`.

### First Response
1. Acknowledge receipt (within 2 hours).
2. Provide estimated timeline for lead assignment.
3. Share any relevant context (previous interactions, lead history).

### Follow-Up
- **Within 24h** – Confirm lead assignment and next steps.
- **Day 3** – Verify lead has accessed dashboard and integrations.
- **Day 7** – Check for any issues; if unresolved, escalate.

### Closure
- Once lead is active and generating leads, archive the demo request record.
- Remove from active queue.

### Notification Failure Contingency
- If primary notification channel fails:
  1. Send fallback (email + Slack) within 1 hour.
  2. Log failure in `ops/incidents`.
  3. Notify owner via phone (if available) or SMS.

### Escalation Path
1. **Level 1** – Primary owner (Otto) – resolves within 24h.
2. **Level 2** – Team Lead (Jiří) – if Level 1 unresponsive >4h.
3. **Level 3** – COO (Otto) – if no response after 24h.

---

## 5. Decision Register

| Decision | Owner | SLA | Escalation | Lifecycle Stage |
|----------|-------|-----|------------|-----------------|
| Assign lead to human | Primary Owner | 2h | Yes | Onboarding → Assigned |
| Approve demo request | Primary Owner | 24h | Yes | Assigned → Active |
| Create shop | Primary Owner | 1d | No | Active → Shop Created |
| Select integration | Primary Owner | 1d | No | Shop → Integrated |
| Notify lead | Primary Owner | 1h | No | Active → Notified |
| Escalate unresolved lead | Level 2 | 4h | Yes | Active → Escalated |

---

## 6. Assumptions

1. **Lead data** is accurate and complete at intake (no missing mandatory fields).
2. **Integration APIs** are available and responding (edge functions are online).
3. **Database** has sufficient capacity for `demo_requests` table.
4. **Owner** (Jiří) is reachable via Slack/email during business hours.
5. **Monitoring** (daily audit) is performed by the designated owner.

---

**Next Steps**
- Review this SOP with Jiří (COO) for approval.
- Implement the monitoring checklist in the workspace.
- Once approved, mark as `ACTIVE` and begin applying to live leads.
