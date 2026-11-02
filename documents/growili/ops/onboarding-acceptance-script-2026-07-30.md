# Authenticated Onboarding Acceptance Script

> **Status:** HISTORICAL / SUPERSEDED  
> **Kanonický zdroj:** `onboarding-phase0-acceptance-plan.md`  
> Tento snapshot obsahuje rozporné a nedoložené limity. Nepoužívat pro aktuální go/no-go.

## Scenarios Tested

1. **No Purchased Module**
   - User starts onboarding without any modules purchased
   - Expected: Module selection interface presented
   - Evidence: User can browse and select modules

2. **Purchased Analytics Module**
   - User has analytics module in their account
   - Expected: Auto-select analytics configuration
   - Evidence: Setup wizard skips generic steps, shows analytics-specific flow

3. **Sidebar Auto-Completion**
   - User completes initial sidebar setup
   - Expected: All required fields auto-populated with defaults
   - Evidence: Sidebar shows completed state without manual input

4. **Overview Next Step**
   - User reaches overview page after setup
   - Expected: Clear next-step indicator visible
   - Evidence: "Continue to Dashboard" or similar CTA visible and functional

5. **Console Loop Warnings**
   - User triggers setup errors
   - Expected: Descriptive error messages in console
   - Evidence: Specific guidance provided, not generic "error occurred"

6. **Network Query Count/Repetition**
   - User completes onboarding
   - Expected: Reasonable number of network requests (<10)
   - Evidence: Network tab shows <= 10 unique requests, no duplicate calls

## Evidence Requirements

- **Console**: Error messages must include specific remediation steps
- **Network**: All requests must include proper authentication headers
- **User State**: Current page, route, and form values must be verifiable
- **Screenshots**: Forbidden for acceptance - must use automated validation

## Go/No-Go Criteria for Phase 1

**GO** if:
- All 6 scenarios pass automated validation
- User state properly captured at each step
- Network traffic within acceptable limits
- Console error handling tested and working

**BLOCKED** if:
- Any scenario fails automated validation
- Evidence collection incomplete
- Console errors lack specific guidance
- Network traffic exceeds limits
- User experience inconsistent across modules
