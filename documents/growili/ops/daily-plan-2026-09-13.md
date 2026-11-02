# Denní operační plán — 13. září 2026 (COO Oto)

## Aktivní priority (podle CURRENT-PRIORITIES.md)

1. **P0-RELEASE-GATES** — vlastník CTO, podpora COO
   - Status: aktivní — build projde, ale TS/i18n/testy/ESLint a production dependency audit selžou; CI chybí
   - Měřitelný baseline: 12 TS chyb; 4 chybějící i18n klíče/locale; 18 selhávajících testů; 61 ESLint chyb / 772 warningů; 3 high + 3 moderate production dependency findings
   - Úspěch: evidence-backed opravní sekvence existuje; schválené opravy jsou izolované a validované; CI/pre-commit návrh používá `tsconfig.app.json` a dostatek Node heap; žádný přímý `main` push ani deploy bez Jiřího

2. **P1-ONBOARDING-ACCEPTANCE** — vlastník COO (mě)
   - Status: aktivní — Phase 0 je na `main` ale neauthenticated-browser ověřeno; čtyři onboarding plochy se liší
   - Úspěch: reprodukovatelné browser QA pokrývá `/get-started`, sidebar, overview, paid/no-module stavy, console loops a query objem; zjištění určují go/no-go pro Phase 1
   - Constraint: žádná Phase 1 implementace dokud nebude přezkoumána Phase 0 acceptanční evidence

3. **P2-ACQUISITION-TO-LEAD** — vlastník CMO
   - Status: aktivní — demo formulář ztrácí leady po falešném úspěchu; `/apps` obsahuje broken/mismatched destinace a chybí z hlavní navigace
   - Úspěch: schválený product/route inventář, kanoniční 4-6-pillar message, proof-asset seznam, pricing/offer rozhodnutí request, skutečný lead-capture + ownership + SLA smlouva existuje před kódem
   - Constraint: žádná veřejná kopie, kampaň, cenová změna nebo customer promise bez Jiřího

## Pending návrhy (z TODO-INBOX.md)

- **CSO-2026-07-31-01** — Approve CRM/deal feed source for CSO pipeline monitoring
  - Priority P1 (podporuje P2)
  - Status: proposed
  - Owner: coo
  - Dependency: Steve's rozhodnutí o CRM zdroji (možnosti: feed, node pairing, exception)
  - Escalation trigger: nevyřešeno do EOD 2026-07-31 → CSO monitoring zůstává blokován

## Konkrétní operační úkoly na dnešek

### 1. P1: Shromažďování lokální evidence pro onboarding acceptance (aktuální blokátor)
- **Popis:** Jiří schválil alternativní local capture runbook (2026-09-11) kvůli postrahu pairovaného počítače. COO potřebuje od Jiřího nebo CTO:
  - lokálně zachycený balíček A–F dle `LOCAL-CAPTURE-RUNBOOK.md`
  - sanitizované artiky (uživatelský/shop stav, screenshots, console logs, network logs)
- **Odhad času:** 60–90 min (revize při přijetí artifactů)
- **Priorita:** 🔴 Kritická — bez evidence nezačíná Phase 1
- **Blokátor:** čeká se na lokální artifact od Jiřího/CTO

### 2. P0: Revize a priorizace TS chyb a ESLint warningů před schválením opravního batch
- **Popis:** CTO potřebuje od COO analýzu produkčního auditního baseline (12 TS chyb, 61 ESLint chyby / 772 warningů). Cíl: rozdělit na „safe isolated fixes" vs „breaking risk". Podporovat schválení první opravního batch od Jiřího.
- **Odhad času:** 45 min
- **Priorita:** 🟠 Vysoká — blokuje P0 RELEASE GATES
- **Blokátor:** kontext k auditu je v `PRODUCT-AUDIT-2026-07-27.md`

### 3. CSO-2026-07-31-01: Iniciovat rozhodování o CRM zdroji pro pipeline monitoring
- **Popis:** CMO/CSO monitorování zůstává blokován dokud není schvářen datový zdroj. COO připraví krátký brief pro Steveho: srovnání možností (feed vs node pairing vs exception) — včetně rizik, implementačního náročnosti a dopadu na lead quality verification.
- **Odhad času:** 30 min (příprava briefu) + 15 min (deliver)
- **Priorita:** 🟡 Střední — podpora P2
- **Escalation:** pokud nevyřešeno do EOD 2026-07-31

### 4. P0: Příprava CI/pre-commit návrhu s `tsconfig.app.json` a Node heap nastavením
- **Popis:** Audit ukazuje chybějící CI. COO zkotroří `tsconfig.app.json` nastavení a Node heap parameter pro pre-commit hook. Návrh směřovaný ke schválení.
- **Odhad času:** 45 min
- **Priorita:** 🟠 Vysoká — klíčové pro P0 Release Gates
- **Constraint:** žádný přímý `main` push ani deploy bez Jiřího

## Doporučený pořadí dnes

1. CSO-2026-07-31-01 brief (ráno — krátký, odemknout CMO pipeline)
2. P1 onboarding evidence shromažďování (dle artifactů od Jiřího/CTO)
3. P0 TS/ESLint priorizace
4. CI/pre-commit návrh

## Rizika / bottleneck

- 🔴 P1 onboarding acceptance je blokován — čeká se na lokální artifact od Jiřího/CTO. Bez evidence nelze přejít na Phase 1.
- 🟠 P0 anemický — 12 TS chyb a 772 ESLint warningů vyžadují rychlé rozhodnutí o opravním batchi od Jiřího.
