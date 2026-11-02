# Current Task Assignment — COO (Oto)

**Generated:** 2026-10-09  
**Source:** `CURRENT-PRIORITIES.md` (shared, approved by Jiří 2026-07-27) + `CURRENT-WORK.md` (COO workspace, verified 2026-10-01)  
**Branch:** `feat/acquisition-review-ui` pushed to origin

---

## Active Priorities (from CURRENT-PRIORITIES.md)



---

### P2-ACQUISITION-TO-LEAD — **Owner: CMO** | Support: CSO, CTO
**Status:** active — **blocked on Jiří decisions (D1, D2)**

**Demo form code is FIXED** (client `34844e032` + server `e8429adf3` on main; hidden fields no longer required; `submit-demo-request` Edge Function + migration `20260927120000` exist).

**What needs to happen (independent of CTO deployment):**
| Blocker | Owner | Action |
|---|---|---|
| D1: Ratify canonical 20% CSS offer | **Jiří** | Decide pricing — independent of deployment |
| D2: Name demo lead owner + define SLA | **Jiří** | Assign owner so SLA is defensible when traffic resumes |
| D4: Cleanup 7 dead 2025 deadline keys in `css.ts` | — | At leisure; zero rendering call sites |

**Success criteria (before any landing code):**
- Approved product/route inventory
- Canonical 4–6-pillar message
- Proof-asset list
- Pricing/offer decision request
- Real lead-capture + ownership + SLA contract

**Constraint:** No public copy, campaign, pricing change, or customer promise without Jiří.

---

### P0-RELEASE-GATES — **Owner: CTO** | Support: COO
**Status:** active — CTO work

**Baseline (stale — 2026-10-01):**
- 46 failed tests (32 merchant-center)
- 61 ESLint errors / 772 warnings
- 3 high + 3 moderate production dependency findings
- 12 TypeScript errors; 4 missing i18n keys per locale
- CI absent

**What COO does here:** Support CTO; no direct repair work unless explicitly assigned.

---

## Current Blockers Summary

| Blocker | Owner | Status |
|---|---|---|
| D1: Canonical 20% CSS offer ratification | **Jiří** | **OPEN — decision needed** |
| D2: Demo lead owner + SLA definition | **Jiří** | **OPEN — decision needed** |
| `submit-demo-request` Edge Function deployment confirmation | CTO | Pending confirmation |
| CTO repair batch approval for P0 | Steve | Pending CTO proposal |

---

## COO Next Actions (this session)

### Immediate (today)
1. ✅ **Done:** Fix Shoptet Import Wizard logo → merged to `feat/acquisition-review-ui` and pushed
2. ✅ **Done:** Fix "New Test" corruption in `en/admin.ts` → committed and pushed
3. ✅ **Done:** Merge `origin/main` into feature branch → pushed
4. **Create this assignment document** → push to GitHub
5. **Send report to Steve (CEO)** via `sessions_send` with summary

### Pending Jiří decisions (escalate via Steve → Jiří Telegram)
- P2-D1: Ratify canonical 20% CSS offer
- P2-D2: Name demo lead owner + define SLA

### Ongoing
- Monitor P0-RELEASE-GATES progress (CTO)
- Track D4 cleanup (leisure)
- Maintain operational hygiene docs under `documents/growili/ops/`

---

## Files Changed in This Session

| File | Change |
|---|---|
| `src/components/imports/Import/ShoptetImportWizard.tsx` | Replace generic `ShoppingCart` with Shoptet logo SVG |
| `src/lib/i18n/translations/en/admin.ts` | Remove corrupted `New Test` text from A/B Testing section header |
| Merge: `origin/main` → `feat/acquisition-review-ui` | Synced latest main including all recent docs/commandcode changes |

---

## Reference Documents

- **Priorities:** `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/CURRENT-PRIORITIES.md`
- **COO Work Context:** `/home/growthhacker/.openclaw/workspaces/coo/CURRENT-WORK.md`
- **Evidence Baseline:** `PRODUCT-AUDIT-2026-07-27.md`
- **Acquisition Audit:** `docs/audits/2026-07-26-apps-landing-conversion-audit.md`
- **Landing Redesign Plan:** `docs/plans/MODULE_LANDING_SKILLPLATE_REDESIGN.md`

---

## Escalation Path

```
COO (Oto) → Steve (CEO, via sessions_send) → Jiří (Telegram)
```

**Use for:** Customer impact, financial exception, delivery capacity risk, serious incident, data risk, cross-functional conflict.

---

*This document reflects the current assignment as of 2026-10-09. It does not authorize new work — only documents what is already approved in CURRENT-PRIORITIES.md and CURRENT-WORK.md.*