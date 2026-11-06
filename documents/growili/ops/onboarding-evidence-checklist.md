# Onboarding Evidence Checklist — P1 Blockers Unblock (A–F)

**Purpose**: Close P1-ONBOARDING-ACCEPTANCE by documenting exact evidence requirements for Phase 1 go/no-go.

## Required Artifacts (A–F)

### A. User State Capture
- **Local user session recording** with authenticated browser session (video/webm, 30s+)
- **User profile state** snapshot: registered data, preferences, shop context
- **Session consistency proof**: same user data across browser refreshes

### B. Shop/Platform State
- **Platform instance state** screenshot showing current shop state at artifact capture moment
- **Data integrity proof**: verified database consistency (user + shop fields)
- **Deployment state**: confirmed running from main branch, not stale cache

### C. Console Logs
- **Browser devtools console** log capture with no critical errors during user flow
- **Error rate baseline**: <1% critical error rate during the session
- **Console completeness**: all relevant interactions recorded

### D. Network Traffic
- **Network request capture** showing full API call chain (auth → shop → submission)
- **Request/response validation**: successful HTTP status codes (2xx/3xx)
- **Performance metrics**: request times <3s average, 95th percentile <5s

### E. Query Volume
- **Server load metrics** during the capture window (CPU, memory, concurrent sessions)
- **Database query analysis** showing acceptable volume for the handled traffic
- **Resource utilization proof**: system within healthy thresholds

### F. Screenshots
- **Full page screenshots** of user interactions: login → onboarding form → submission
- **Error state screenshots** if any occur during the flow
- **Final state screenshot** confirming successful submission/completion

## Phase 1 Go/No-Go Criteria

**✅ GO** if ALL of:
- A (User state) ✅ Authenticated session captured
- B (Platform state) ✅ Current shop state visible, consistent
- C (Console logs) ✅ No critical errors (<1% error rate)
- D (Network) ✅ All API calls successful, performance acceptable
- E (Query volume) ✅ Resource usage within healthy thresholds
- F (Screenshots) ✅ Complete visual audit trail

**❌ NO-GO** if ANY of:
- Critical errors in console logs
- Failed API calls or network issues
- User state inconsistency
- Platform data mismatches
- Resource constraints exceeded

## Evidence Delivery Requirements

- **Format**: Markdown structured report with embedded evidence paths
- **Completeness**: All 6 artifact categories fully documented
- **Verification**: Local QA team or CTO review approved
- **Storage**: Save under `/documents/growili/ops/onboarding/` as `p1-evidence-[timestamp].md`

## Impact Assessment

**If P1 remains blocked**:
- Phase 1 cannot start - deployment freeze
- Customer lead acquisition delayed
- Revenue projections impacted

**Unblocking benefit**:
- Enables Phase 1 automated QA
- Opens path to D1/D2 decision unblock
- Resets acquisition pipeline for Goili

---

**Action Item**: Prepare next sprint decision with Jiří:
1. **Current artifacts** ready for immediate capture
2. **Resource allocation** for evidence collection (1-2 developers, 4 hours)
3. **QA timeline** for Phase 1 verification (24-48 hours)

**Owner**: COO (prepared this checklist)
**Next decision point**: Jiří's D1 + D2 decisions (canonical offer + lead owner/SLA)