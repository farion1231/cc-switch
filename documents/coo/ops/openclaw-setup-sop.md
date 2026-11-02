# OpenClaw Multi-Agent Setup SOP

**Version:** 1.1  
**Effective Date:** 2026-07-13  
**Prepared by:** Oto (COO)

## 1. Purpose
Establish a single source of truth for configurations, agent roles, workspace layout, channel access, and break-glass procedures that enable stable, auditable, and scalable OpenClaw deployments.

## 2. Scope
All agents running inside the `coo` workspace, including:
- Core agents (COO, CMO, CSO, CTO)
- Support agents (email, calendar, finance, etc.)
- Persistent and isolated sub-agents created during multi-agent experiments

## 3. Prerequisites
| Item | Location | Why Needed |
|------|----------|------------|
| Workspace root | `/home/growthhacker/.openclaw/workspaces/coo` | All configuration lives here |
| AGENTS.md | `/home/growthhacker/.openclaw/workspaces/coo/AGENTS.md` | Defines role rules, permission hierarchy |
| SOUL.md | `/home/growthhacker/.openclaw/workspaces/coo/SOUL.md` | Operative priorities, communication style |
| TOOLS.md | `/home/growthhacker/.openclaw/workspaces/coo/TOOLS.md` | Tool availability and safety rules |

## 4. Roles & Permissions
| Agent | Role | Key Permissions | Config Schema |
|-------|------|-----------------|---------------|
| COO | Operations, capacity, escalation | full workspace control, `gateway` admin, `sessions_send` → `main` | `agents.roles.coo` |
| CMO | Campaign timing | email, calendar, social-post-writer | `agents.roles.cmo` |
| CSO | Onboarding handoff | support, email-draft-orchestrator, ticket-scout | `agents.roles.cso` |
| CTO | Integration & platform stability | coding-agent, node-connect, fireflies-meeting-intelligence | `agents.roles.cto` |

> **Policy:** All agents must be listed in `AGENTS.md` before activation. Adding an agent without an entry blocks deployment.

## 5. Workspace Layout
```
/home/growthhacker/.openclaw/workspaces/coo/
├── AGENTS.md
├── SOUL.md
├── TOOLS.md
└── documents/
    ├── coo/ops/        # SOPs & guidelines
    ├── coo/support/    # Assist-agent code
    ├── coo/finance/    # FIO-bank queries, reports
    └── …               # Other domains
```

## 6. Agent Activation Procedure

### Step-by-Step
1. **Add agent entry** to `AGENTS.md` (use template below)
2. **Grant workspace access** via `gateway config.patch` if elevated tooling needed
3. **Apply config:** `gateway config.apply` → verify "OK" in logs
4. **Verify heartbeat:**
   - Trigger test cron or ping agent
   - Confirm `sessions_send` works to `main`
   - Log success threshold: response <30 s
5. **Document activation** in `documents/coo/ops/setup-activation-log.md` with ISO timestamp

### Verification Checkpoints
| Checkpoint | Expected Result | How to Confirm |
|------------|-----------------|----------------|
| Config apply | "OK" in stdout | `gateway config.apply` output |
| Heartbeat test | Response <30 s | `cron status` shows recent run |
| sessions_send | ACK received | REPLY visible in sessions history |

## 7. Break-Glass / Recovery

| Symptom | Threshold | Automated Action | Manual Action | Escalation |
|---------|-----------|------------------|--------------|------------|
| Agent no heartbeat | >2 min | Run `gateway restart` | Verify after restart | Inform Steve if still down |
| Permission escalation needed | Always | Audit log | Re-apply via `gateway config.patch` | Requires approval for finance/Customer rules |
| Channel/Integration failure | Any error | `gateway config.patch <integration>` | Apply + `gateway restart` | Alert CTO for integration bugs |

> **Rule:** Never change financial or customer-impacting rules without explicit approval.

## 8. SOP Auditing & Updates

- **Quarterly review** led by COO + CTO
- **Metrics:**
  - Setup success rate ≥ 95 %
  - Break-glass incidents ≤ 1 per quarter
- **Change management:** Proposal → Review → `skill_workshop action=apply` → Bump version

## 9. Checklist for Activation (v1.1)
- [ ] Agent entry added to `AGENTS.md`
- [ ] `gateway config.apply` returned OK
- [ ] Heartbeat test passed (<30 s)
- [ ] `sessions_send` to `main` confirmed
- [ ] Activation logged in `setup-activation-log.md`

## 10. Agent Entry Template (for AGENTS.md)
```
## <agent-name>
- **Role:** <brief-role>
- **Primary Duties:** <3 bullets>
- **Allowed Tools:** <from TOOLS.md>
- **Approval Boundaries:** <what requires explicit approval>
```

---

*This SOP is the foundational procedure for any future agent setup or workspace modification within Growili.*