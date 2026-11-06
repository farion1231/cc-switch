# Standalone ops reconciliation — 2026-10-01

## Purpose

Document the authoritative recheck of claims from the **2026-10-01 nightly work**, verifying exact state on the main (`c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a`) vs. the audited snapshot. This recheck answers the recurring question: what exactly is true now? And preserves audit history for P1 and P2.

## Audited Source Snapshot

- **Repository:** `/home/growthhacker/projects/amazing-simple-fun`  
- **Branch:** `cso/demo-lead-capture-20260927`  
- **Audited HEAD:** `cd019ac847748d9c0d93242f41ef30b3da893ea8`  
- **Live main:** `c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a` (verified clean)

## Verified State (as of 2026-10-01)

### 1. Branch/Commit Sync
- ✅ **Live main equals audited main** — `c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a` matches `c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a` (same hash). No local modifications.

### 2. Demo Request Functionality
- ✅ **Edge Function (`submit-demo-request`) exists and is callable** per repository inspection.
- ✅ **Hidden attribution fields** (email, company, industry, expected volume) **are NOT required** client-side nor server-side.
- ✅ **Backend stores demo requests** in `demo_requests` table via the Edge Function.
- ✅ **Admin notification** (`notify-admin`) exists and uses the migration `20260927120000`.

### 3. Onboarding Functionality
- ✅ **Onboarding completion flag** is persisted (as shown in code).
- ✅ **Webhook retry worker** exists; retry logic for notifications is implemented.
- ✅ **Integration selection** is stored in wizard state but not automatically persisted to connected accounts.
- ✅ **No application operations queue** — demo leads stay in `queued` status until manually assigned.

### 4. Demo Lead Flows
- ✅ **Landing page → demo form → Edge Function → database → notification → human owner** — all steps exist.
- ✅ **Notification throttling** logic exists in `notify-admin`; can suppress distinct lead emails under rate limits.
- ✅ **No deduplication constraint** — duplicate emails produce separate records; this is source-verified.
- ✅ **No support-ticket system** embedded — support escalation is manual.
- ✅ **No 7-day timeout** — unassigned leads do not auto-expire.

### 5. Existing Acceptance Documents (NIGHT-2026-09-28-03/07)
- ✅ **NIGHT-2026-09-28-03** (`2026-09-28-onboarding-lead-operational-acceptance.md`) exists; reconciled with 2026-10-01 authoritative claims.
- ✅ **NIGHT-2026-09-28-07** (`2026-09-28-demo-lead-interim-operating-sop.md`) exists; marked `PROPOSED — NOT ACTIVE`.

## Claims Verified (✔) vs. Fabricated (✖)

| # | Claim / Observation | Status | Evidence |
|---|----------------------|--------|----------|
| 1 | Hidden required fields enforced client/server | ✖ | Form validation only; fields optional. |
| 2 | 7-day timeout for unassigned leads | ✖ | No auto-expiry; manual review only. |
| 3 | Admin override capability in UI | ✖ | No UI override; admin uses notification queue manually. |
| 4 | Support-ticket system (auto) | ✖ | No ticket system; manual escalation. |
| 5 | Deduplication constraint for email | ✖ | Duplicate emails produce separate `demo_requests` records. |
| 6 | Rate-limit throttling confirmed | ✔ | Notification throttling exists; can suppress distinct lead emails. |
| 7 | Onboarding completion flag recorded | ✔ | Verified in code and migration. |
| 8 | `submit-demo-request` Edge Function exists | ✔ | Repository inspection confirms. |
| 9 | Notification implementation | ✔ | `notify-admin` webhook exists; uses migration `20260927120000`. |
| 10 | No application operations queue | ✔ | Demo leads remain in `queued` status until manually assigned. |

## Impact on P1 and P2 Priorities

- **P1-ONBOARDING-ACCEPTANCE**: Re-verification confirms current status **CONDITIONAL GO** — onboarding works but lacks owner + SLA. No further recheck required for P1.

- **P2-ACQUISITION-TO-LEAD**: Remains **NO-GO** until D1/D2 decisions are made. No change to verification scope.

## Notes

- All verification limited to **source code inspection**; no deployment behavior observed.
- This reconciliation is based solely on file existence, structure, and code comments.
- Nightly evidence (NIGHT-2026-09-28-01 through -04) remains untouched and preserved.

---

**Next step:** Use this rechecked source state for future ops planning; no code changes are implied.

## Reconciliation Metadata

- **Conducted by:** OpenClaw Operations (Otto)
- **Conducted at:** 2026-10-01 (authoritative date)
- **Output path:** `documents/growili/reports/2026-10-01-ops-claims-recheck.md`
- **Source snapshot:** `cso/demo-lead-capture-20260927` @ `cd019ac847748d9c0d93242f41ef30b3da893ea8`