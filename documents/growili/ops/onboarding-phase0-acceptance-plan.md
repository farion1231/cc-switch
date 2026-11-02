# Onboarding Phase 0 — kanonický acceptance plán

> **Status:** IN PROGRESS — schválena lokální důkazní cesta; čeká se na artefakty  
> **Owner:** COO (Oto)  
> **Priority:** `P1-ONBOARDING-ACCEPTANCE`  
> **Aktualizováno:** 2026-09-10  
> **Nahrazuje:** starší acceptance skript, plain evidence draft, block-status report a neověřený alternative-path protokol

## 1. Rozhodnutí a hranice

Phase 0 nelze označit jako přijatou bez reprodukovatelného důkazu z reálné přihlášené relace. Platí jedna z těchto cest:

1. **Preferovaná cesta:** Jiří schválí použití již přihlášené browser session.
2. **Schválená alternativní cesta (2026-09-11):** Jiří nebo CTO provede lokální read-only capture podle `onboarding-phase0-evidence/LOCAL-CAPTURE-RUNBOOK.md` a předá sanitizované screenshoty, konzoli a HAR bez vzdáleného přístupu.

Samotný Markdown, statický code review ani tvrzení o dřívějším schválení nejsou acceptance evidence. Phase 1 nezačne před písemným go/no-go po kontrole balíčku.

## 2. Scope testu

Ověřit v přihlášené relaci:

- `/get-started` pro uživatele bez placeného modulu;
- `/get-started` pro uživatele s aktivním modulem;
- stav a chování sekce „Začínáme“ v sidebaru;
- shodu dalšího kroku mezi overview a `/get-started`;
- paid/no-module stav bez změny produkčních dat;
- absenci render/update loopů v konzoli;
- ustálení síťových požadavků bez nekonečného opakování.

Payment flow, FIO, lead queue a obecný monitoring nejsou součástí této acceptance.

## 3. Předpoklady

- [x] Schválená důkazní cesta je zaznamenaná s datem a schvalovatelem.
- [ ] Existuje bezpečný testovací uživatel a shop nebo read-only produkční relace.
- [ ] Jsou známé očekávané module states.
- [ ] Test nemění billing, zákaznická data ani procesní pravidla.
- [ ] DevTools umí zachytit console log a sanitizovaný HAR.

Pokud některý předpoklad chybí, test se zastaví a stav zůstává `BLOCKED`.

## 4. Testovací scénáře

| ID | Scénář | Pass kritérium | Povinný důkaz |
|---|---|---|---|
| A | Bez zakoupeného modulu | `/get-started` ukáže správnou nabídku a globální kroky | screenshot + stav účtu |
| B | Aktivní modul | příslušný modul a jeho postup jsou viditelné | screenshot + stav účtu |
| C | Sidebar | stav odpovídá skutečně splněným krokům; nic se falešně nedokončí | před/po screenshot + console |
| D | Overview → další krok | doporučený krok je konzistentní s `/get-started` | dvojice screenshotů |
| E | Konzole | žádný `Maximum update depth exceeded`, `getSnapshot should be cached` ani opakovaná runtime chyba | export konzole |
| F | Síť | požadavky se po dokončení obrazovky ustálí; žádný nekonečný loop | sanitizovaný HAR + souhrn počtů |

Číselný limit requestů se stanoví až z ověřeného baseline. Staré limity `<10` a `<100` nebyly doložené a nejsou acceptance kritériem.

## 5. Provedení

1. Zaznamenej čas, prostředí, commit/deploy identifikátor a anonymizovaný účet/shop.
2. Otevři DevTools ještě před vstupem na první testovanou route.
3. Vyčisti console a network log; zapni zachování logů.
4. Proveď scénáře A–F bez zásahu do zákaznických dat.
5. U každého scénáře ulož důkazy a verdikt `PASS`, `FAIL` nebo `BLOCKED`.
6. Odstraň z HAR tokeny, cookies, e-maily a jiné osobní údaje.
7. Zapiš souhrn do `onboarding-phase0-evidence.md`.
8. COO zkontroluje úplnost; CTO může technická zjištění připomínkovat.
9. Steve obdrží doporučení `GO` nebo `NO-GO`; bez výslovného rozhodnutí platí `NO-GO`.

## 6. Evidence package

Výstupní adresář:

`documents/growili/ops/onboarding-phase0-evidence/`

Minimální obsah:

- `onboarding-phase0-evidence.md` — matice výsledků a rozhodnutí;
- screenshoty pro A–D;
- sanitizovaný console export pro C–E;
- sanitizovaný HAR a souhrn síťových požadavků pro F;
- metadata: čas, prostředí, deploy/commit, anonymizovaný account/shop, verifier.

Do důkazů nikdy neukládat přístupové tokeny, cookies, hesla ani celé osobní údaje.

## 7. Go/no-go

**GO doporučit pouze tehdy, když:**

- A–F mají `PASS`;
- balíček je úplný a sanitizovaný;
- nebyl proveden zakázaný zápis do produkce;
- zjištění neukazují zákaznický, datový nebo bezpečnostní incident.

**NO-GO při:**

- libovolném `FAIL` nebo `BLOCKED`;
- chybějícím důkazu;
- nekonečné konzolové/síťové smyčce;
- nesouladu onboardingových stavů;
- nejasném účtu, shopu nebo deployi.

## 8. Aktuální další krok

Jiří nebo CTO provede lokální sběr podle `onboarding-phase0-evidence/LOCAL-CAPTURE-RUNBOOK.md` a předá sanitizované artefakty. Do jejich úplného přezkoumání zůstává celkové doporučení `NO-GO`.
