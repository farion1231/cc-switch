# Escalation Action Plan – P1 Onboarding Acceptance (Deadline: 2 hours)

## Timeline

| Time | Action |
|------|---------|
| Now | Template sent to Jiří (this file) |
| +30 min | Check for Jiří response |
| +60 min | If no response, send follow-up reminder |
| +90 min | If still no response, initiate intrusive escalation |
| +120 min | Final escalation if still pending |

## Option 1: Follow-up Reminder (if no response in 30 min)

**Message to Jiří:**

> Hi Jiří,
> 
> Following up on my earlier message (ID: 4665) regarding **P1 Onboarding Acceptance** – the Phase 1 gate is still blocked because we lack an **authenticated browser session** for real user/shop verification.
> 
> As confirmed by COO/CTO alignment, there is **no verification package** in place. Without this evidence, Phase 0 cannot be marked passed and Phase 1 cannot start.
> 
> **Request:** Please approve the authenticated browser session or provide an approved alternative proof path so we can meet the 2-hour deadline.
> 
> This is a **critical path dependency** – P1 acceptance is required before any Phase 1 work can proceed.
> 
> Let me know ASAP so we can unblock the pipeline.
> 
> Best,
> Otto (COO)

## Option 2: Intrusive Escalation (if no response in 60 min)

**Method:** Direct executive escalation via OpenClaw governance

1. **Send immediate alert** to COO/CTO leadership (via `sessions_send` to main channel or direct message)
2. **Attach** the escalation template (`jiřis-escalation-template.md`)
3. **Include**:
   - Current blocker status (P1-ONBOARDING-ACCEPTANCE BLOCKED)
   - Deadline (2 hours from message ID 4665)
   - Impact on P0-RELEASE-GATES
   - Reference to `onboarding-phase0-acceptance-plan.md` and `CURRENT-WORK.md`
4. **Notify** the COO directly that the critical path is stalled

**Escalation message draft:**

> **URGENT – P1 ONBOARDING ACCEPTANCE BLOCKED**
> 
> Phase 1 onboarding acceptance is **BLOCKED** awaiting authenticated browser verification. 
> 
> - **Blocker:** No approved authenticated browser session for real user/shop state
> - **Impact:** P1 cannot start; P0 release gates remain blocked
> - **Deadline:** 2 hours from message ID 4665
> 
> Please approve the authentication path or provide an alternative proof method immediately. This is a **critical path dependency** for Phase 1.
> 
> [Attachment: jiřis-escalation-template.md]
> 
> @Jiří @COO
> 
> *Sent by Otto (COO)*

## Option 3: Fallback – Alternative Proof Path

If Jiří cannot provide authenticated browser access quickly, explore:

1. **Pre-generated browser session** – Can we spin up a temporary authenticated session for testing?
2. **Recorded session capture** – If a recorded browser session exists with real user interactions, can it be used as proxy evidence?
3. **Manual verification** – If the team can simulate the onboarding flows without live browser access (limited but possible for certain checks)

**Note:** Any alternative path must still satisfy the onboarding plan's requirement for **real authenticated browser evidence** – screenshots alone are insufficient.

## Decision Points

- **✅ Approved** – Mark `onboarding-phase0-acceptance-plan.md` as PASSED, clear blocker, proceed to Phase 1
- **❌ Blocked** – Continue escalation, consider alternative proof path
- **🚨 Escalated** – If no resolution within 2 hours, involve senior leadership

## Owner
- **Primary:** Otto (COO) – decision authority
- **Support:** CTO (Jan) – technical validation
- **Monitor:** Jiří – execution responsibility

---

*Action Items:*
1. Send follow-up to Jiří now (using `message` tool)
2. Set 60-minute timer for response
3. If no response, trigger intrusive escalation via `sessions_send` to main channel
4. Document outcome in `onboarding-block-status.md`
