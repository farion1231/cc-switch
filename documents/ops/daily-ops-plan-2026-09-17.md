# Denní plán operací — 17. září 2026 (Čt)

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

**Pro:** Jiří Franěk (CEO)
**Od:** Oto, COO 🧭
**Datum:** 2026-09-17 07:00 UTC+2
**Aktivní priority:** P1-ONBOARDING-ACCEPTANCE (owner COO) · P2-ACQUISITION-TO-LEAD (owner CMO)

---

## Stav klíčových toků

| Tok | Stav | Poznámka |
|---|---|---|
| P1-ONBOARDING-ACCEPTANCE | 🔴 **BLOCKED** | Čeká na lokálně zachycené artefakty (Jiří/CTO). Žádné browser evidence ≠ Phase 0 accepted. |
| P0-RELEASE-GATES | ⚫ Archivováno | Přesunut pod P0-RELEASE-GATES archive; nový release-readiness board až po obnovení. |
| P2-ACQUISITION-TO-LEAD | 🟡 Aktivní | CMO owner; čeká na Steveho rozhodnutí o canonical offer/pricing. |
| CSO Pipeline Data Source (CSO-2026-07-31-01) | 🟡 Proposed | Termín 31.7.2026 uplynul → ESCHALACE řeší. |
| HEARTBEAT.md | ⚫ Chybí | Denní heartbeat neexistuje — operativní sladění není sledováno. |

---

## Dnešní operativní úkoly

### PO-01 · Spustit lokální capture runbook (P1 — blocker)
- **Priorita:** P0 (jediné aktivní evidence pro onboarding)
- **Odhad:** ~2h
- **Akce:** Jiří nebo CTO spustí `documents/growili/ops/onboarding-phase0-evidence/LOCAL-CAPTURE-RUNBOOK.md` a dodá sanitizované artefakty A–F (user/shop state, screenshoty, console logs, network logs).
- **Milník:** Po dodání → COO review → go/no-go pro Phase 1 → Steve.
- **Eskalace:** Bez evidence do konce týdne = Phase 1 zůstává zamčená.

### PO-02 · Rozhodnout o CSO CRM zdroji dat (P1/P2)
- **Priorita:** P1 (blokuje CSO monitoring lead quality)
- **Odhad:** ~30 min
- **Akce:** Steve se rozhodne pro feed / node pairing / exception pro CSO pipeline data source (CSO-2026-07-31-01).
- **Milník:** Schválení → COO zašle credentials/config → CSO obnoví live monitoring.
- **Eskalace:** Bez rozhodnutí = CSO monitoring zůstává blokovaný (lead quality unverifikovatelný).

### PO-03 · Obnovit HEARTBEAT.md (P0 — hygiene)
- **Priorita:** P0 (operativní transparentnost)
- **Odhad:** ~15 min
- **Akce:** Vytvořit `/documents/HEARTBEAT.md` se strukturou: priority status · blokery · rozhodnutí potřebná · denní úkoly.
- **Milník:** Soubor existuje a je aktualizován v rámci každého denního plánu.

### PO-04 · Ověřit CSO pipeline integrity output (P1)
- **Priorita:** P1 (závisí na PO-02)
- **Odhad:** ~20 min
- **Akce:** Po schválení CRM zdroje ověřit, že `/home/growthhacker/.openclaw/workspaces/cso/documents/css/pipeline-integrity-check.md` existuje a odpovídá aktuálnímu stavu.
- **Milník:** Pipeline integrity check = live status.

### PO-05 · Připravit pricing/offer decision request pro Stevea (P2)
- **Priorita:** P2 (CMO + CSO joint recommendation)
- **Odhad:** ~30 min
- **Akce:** Kontrolovat, zda CMO má hotovou doporučenou nabídku/pricing strukturu a případně ji podpořit koordinací s CSO.
- **Milník:** Joint recommendation přijata → Steve rozhodne → žádné public copy/pricing change bez jeho schválení.
- **Eskalace:** Bez rozhodnutí = žádné redesigny / kampaně na základě nových nabídek.

---

## Doporučení

1. **PO-01 je nejprioritnější** — dokud není evidence dodáno, Phase 1 nesmí začít. Připomeňte Jiřímu při večerním check-inu.
2. **PO-02 má explicitní eskalaci** — CSO monitoring byl blokován déle než stanovený termín. Rychlé rozhodnutí pomáhá udržet lead quality.
3. **PO-03 je quick win** — 15 minut zlepší operativní přehlednost pro budoucí dny.

---

*Report vytvořen: 2026-09-17 07:00 UTC+2 · Oto, COO 🧭*
