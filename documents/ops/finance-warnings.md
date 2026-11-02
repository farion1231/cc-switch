# Finance Risks

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

## Current Status
- FIO Bank monitoring active but FIO API token not configured
- Last check: 2026-08-09 (2 weeks ago)
- Status: WARNING (Cashflow verification incomplete)

## Impact
- Missing FIO token prevents real-time cashflow anomaly detection
- Overdue invoices cannot be automatically flagged
- Requires manual verification of financial records

## Escalation Path
1. Immediate: Configure FIO API token
2. 24h: Escalate to CFO if token still missing
3. 48h: Risk of cashflow reporting inaccuracies

## Action Required
-----------------------
- [ ] Configure FIO API token in `config.schema.lookup('finance.integration.fio')`
- [ ] Schedule weekly finance check (`cron.add(weekly_finance_check)`)
- [ ] Verify overdue invoices in `documents/finance/overdue-invoices.md`
