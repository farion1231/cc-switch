# Denní provozní plán - Pondělí 16. srpna 2026

> HISTORICKÝ PODKLAD — oprava 2026-10-01: Není důkazem aktuálních blockerů. Demo backend, schema, notifikace a opravy attribution polí existují; P1 připouští manuální acceptance. Aktuální FIO/CRM, fronty, SLA a deployment nejsou tímto historickým záznamem ověřeny. Viz documents/growili/reports/2026-10-01-ops-claims-recheck.md v COO workspace. Níže zachována historie; nepřebírat jako současný stav.

**Generováno:** 2026-08-16 07:05 UTC  
**COO:** Oto  
**Cíль:** Připravit Jiřího na denní operační priorities

---

## Aktuální stav priority (k 07:05 UTC)

1. **P0-RELEASE-GATES** — Blokované deterministickou validací (CTO owner)
   - TypeScript chyby: 12 chyb
   - i18n chybějící klíče: 4 v obou CS/EN
   - Vitest neúspěchy: 18 selhání
   - ESLint chyby: 61 + 772 varování
   - Production dependency audit: 3 high + 3 moderate
   - CI gate: žádné workflows

2. **P1-ONBOARDING-ACCEPTANCE** — Zablokováno missing authenticated browser session (COO owner)
   - No authenticated browser evidence exists
   - Phase 1 cannot start until browser session approved
   - Acceptance plan existuje, ale bez proof

3. **P2-ACQUISITION-TO-LEAD** — Čeká na P1 completion (CMO owner)
   - Demo form odhazuje leads po falešném success
   - /apps obsahuje broken destinations

---

## Navržené úkoly pro dnešek (3-5 prioritizovaných)

### Úkol 1: P0 Release Gates — fresh evidence rerun (HIGH priority, ~2h)
- **Cíl:** Spustit fresh TypeScript, i18n, Vitest, ESLint rerun a zachytit výsledky
- **Akce:**
  - Rerun audit příkazem a zachytit aktuální čísla (namísto zastaralých 39/658)
  - Zkontrolovat TypeScript errors count fresh
  - Evidence uložit do release-readiness board
- **Výstup:** Deníka fresh evidence numbers (ne stale counts)

### Úkol 2: P1 Onboarding — Request browser session approval from Jiří (HIGH priority, ~1h)
- **Cíl:** Vyřešit hlavní blocker pro Phase 1
- **Akce:**
  - Poslat eskalaci Jiřímu: "P1 onboarding acceptance is still blocked. No authenticated browser evidence package exists."
  - Požádat o approval authenticated browser session nebo alternativní proof path
  - Dokumentovat decision a next steps
- **Výstup:** Schválený browser session request nebo explicitní zamítnutí

### Úkol 3: P2 Acquisition-to-Lead — Review status after P1 resolution (MEDIUM priority, ~1h)
- **Cíl:** Posoudit stav acquisition funnel po P1 rozhodnutí
- **Akce:**
  - Prozkoumat docs/audits/2026-07-26-apps-landing-conversion-audit.md
  - Zkontrolovat /apps destinations a navigation coverage
  - Připravit joint CMO+CSO recommendation na canonical offer/pricing a lead destination
- **Výstup:** Status report ready for Jiří decision

### Úkol 4: CSO Pipeline Data Source Unblock (MEDIUM priority, ~1h)
- **Cíl:** Dokončit CRM/deal feed source schvalování
- **Akce:**
  - Prozkoumat CSO-2026-07-31-01 task z TODO-INBOX.md
  - Posoudit options: feed, node pairing, or exception
  - Dokumentovat decision a dependencies
- **Výstup:** Pipeline integrity check ready

### Úkol 5: Daily coordination report to Steve (LOW priority, ~30min)
- **Cíl:** Poslat shrnutí denního pokroku
- **Akce:**
  - Vytvořit report formát dle SOUL.md operational situation
  - Vyzdvořit risk/bottleneck, doporučený proces, owner/checklist
  - Poslat přes sessions_send na main session
- **Výstup:** Report doručen Stephenu

---

## Odhad času celkem: ~5h

**Sequence:** 1 → 2 → 3 → 4 → 5  
**Blokery:** Úkol 2 je critical path — bez browser session approval nelze pokročit s Phase 1  
**Doporučení:** Začít úlohou 1 (release gates evidence) zatímco čekáme na Jiřího odpověď k úloze 2

---

*Tento plán je založen na aktuálních prioritách z CURRENT-PRIORITIES.md, TODO-INBOX.md a stavu CURRENT-WORK.md. Dení plan je navržen tak, abych maximalizoval progres i přes existing blockery.*
