# CSS Lead Flow SOP

**Version:** 1.0 (draft)  
**Effective Date:** 2026-07-13  
**Prepared by:** Oto (COO)  
**Status:** Draft – awaiting §10 completion & priority approval  

## 1. Purpose
Define a repeatable, measurable process for handling CSS leads from capture to activation, ensuring:
- **First touch ≤ 2 h**
- **Onboarding completed within 48 h**
- **No orphaned leads**
- **Clear hand‑off between CSO, delivery, and founder**

## 2. Scope
Applies to all inbound CSS leads (form, email, Telegram, partner) entering the Growili pipeline before they become paying customers.

## 3. Goals & SLAs

| Metric | Target | Owner |
|--------|--------|-------|
| Lead response time (capture → first touch) | < 2 h | CSO |
| Onboarding completion | ≤ 48 h from activation | CSO + Delivery |
| Onboarding completion rate | ≥ 90 % | COO |
| Orphaned leads (no owner) | 0 | CSO |

## 4. Roles (RACI)

| Phase | CSO | COO | CTO | Founder |
|-------|-----|-----|-----|---------|
| Capture / validation | **R** | **A** | **C** | **I** |
| First touch (<2 h) | **R** | **A** | **I** | **I** |
| Qualification | **R** | **C** | **I** | **I** |
| Handoff to onboarding | **R** | **A** | **C** | **I** |
| Activation | **C** | **R** | **C** | **I** |
| Monitoring / escalation | **A** | **R** | **I** | **I** |

R = Responsible, A = Accountable, C = Consulted, I = Informed  

## 5. Workflow (step‑by‑step)

### 5.1 Capture
1. Lead arrives via **Google Form → Supabase** or **Telegram bot**.  
2. Mandatory fields: `Name`, `E‑mail`, `Source`, `Timestamp`.  
3. Incomplete records → flag `incomplete`, auto‑escalate to CSO within 30 min.

### 5.2 First Touch (<2 h)
1. Automated acknowledgement (Zapier → Gmail/Telegram).  
2. CSO manually contacts lead (email/Telegram).  
3. Log `first_touch` timestamp → calculate `response_time`.

### 5.3 Qualification
1. Verify: functional e‑shop, minimum annual turnover, genuine interest in CSS.  
2. Outcome: `qualified` / `nurture` / `reject` (record rationale).

### 5.4 Onboarding Handoff
1. CSO moves qualified lead to **onboarding queue** in **Google Sheets**.  
2. Handoff record includes: `lead_id`, `contact`, `context`, `scheduled_start`.  
3. Delivery acknowledges receipt within **24 h**.

### 5.5 Activation
1. Delivery performs initial setup (tracking, campaign config).  
2. First measurable result delivered within **48 h**.  
3. Mark lead as `activated`; if not activated by then → flag in heartbeat queue.

### 5.6 Monitoring
1. COO runs heartbeat checks:  
   - Leads without touch > 2 h → alert.  
   - Onboarding > 48 h → alert.  
2. Weekly report to Steve with metrics: response time, completion rate, orphan leads.

## 6. Verification Checkpoints
| Checkpoint | Expected Result | How to Verify |
|------------|----------------|---------------|
| Capture fields | All mandatory fields filled | Google Form validation |
| First touch | `< 2 h` | Timestamp diff → log |
| Handoff | Received within 24 h | Queue timestamp |
| Activation | Completed ≤ 48 h | Delivery status flag |

## 7. Break‑Glass
| Symptom | Immediate Step | Escalation |
|---------|----------------|------------|
| Lead > 2 h without touch | CSO manually contacts lead within 30 min | If unresolved > 4 h → COO → Steve |
| Onboarding > 48 h | COO flags delivery queue | If > 72 h → Steve |
| Lead orphaned (no owner) | CSO re‑assign owner within 1 h | COO escalates if not fixed |

> **Rule:** No customer‑impacting promise without approval. No financial changes without explicit sign‑off.

## 8. Metrics & Review
- **Weekly**: response time, completion rate, orphan count (shared with Steve).  
- **Monthly**: deeper drill‑down with CTO & CSO.  
- **Quarterly**: formal SOP review, update via `skill_workshop action=apply`.

## 9. Checklist (per lead activation)

- [ ] Lead captured with all required fields  
- [ ] First touch logged < 2 h  
- [ ] Qualification status recorded  
- [ ] Handoff to onboarding queue logged & delivery acknowledged  
- [ ] Activation completed ≤ 48 h  
- [ ] Monitoring metrics updated  

## 10. Otevřené otázky (k doplnění před schválením)

1. **Nástroj pro sběr leadů:** Google Form + Supabase  
2. **Nástroj pro onboarding queue:** Google Sheets  
3. **Kdo je delivery (Jiří/Jiří?):** Jiří (founder)  

---

*Tento SOP je připraven k revizi. Po doplnění §10 a odsouhlasení priority bude nasazen a integrován do heartbeat workflowu.*

#### Připraven/sestaven:
**Oto, COO**