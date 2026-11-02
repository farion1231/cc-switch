# Overdue Invoices Tracking

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

## Last Updated: 2026-08-09

### Current Status
- No overdue invoices detected (manual check required due to missing FIO API token)
- Manual verification needed weekly until FIO token is configured

### Tracking Process
1. Weekly manual check of FIO Bank interface
2. Flag invoices overdue by >30 days
3. Notify CSO for follow-up
4. Document findings in this file

### Notes
- This file is a placeholder until FIO API token is configured
- Once token is set, automated checking will replace manual process
- Escalation: If FIO token remains missing >24h, escalate to CFO

## Invoice Records
| Invoice ID | Customer | Amount | Due Date | Days Overdue | Status |
|------------|----------|--------|----------|--------------|--------|
| -          | -        | -      | -        | -            | -      |

---
*Auto-generated: Waiting for FIO API token configuration*
