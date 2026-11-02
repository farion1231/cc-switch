# H2 Nadpisy Stabilizační Plán
**Cíl:** Zajistit konzistentní použití H2 nadpisů v celé aplikaci zgodně s design systémem a předejít vizuální i přístupnostní regresi.

**Souvislost:** Tato činnost podporuje prioritu **P0-RELEASE-GATES** (kvalita kódu, konzistence UI) a může být zařazena jako podúkol před finální release validací.

## 1. Výchozí stav audit
- Poslední audit: `docs/audits/2026-10-04-inapp-h2-stabilization.md`
- Existující lint pravidla:
  - `eslint-rules/enforce-heading-page-for-h2.cjs` – vyžaduje H2 jako první nadpis na stránce
  - `eslint-rules/enforce-page-header.cjs` – obecná hlavičková kontrola
  - `eslint-rules/enforce-locked-section-header.cjs` – zamčené sekční hlavičky
- Aktuální větve s probíhající prací:
  - `chore/unify-inapp-h2-20261010` – nejnovější snaha o sjednocení
  - `chore/h2-lint-error-20261011` – branch s opravami lint chyb

## 2. Kroky stabilizace

### 2.1. Shromáždit aktuální stav
- Spustit ESLint s výše uvedenými pravidly a uložit výsledek do `reports/h2-lint-<timestamp>.md`
- Rozdělit chyby podle souboru a závažnosti (poměrně jednoduché: missing H2, wrong level, duplicated H2)

### 2.2. Prioritizace oprav
| Priorita | Kritérium | Příklady |
|----------|-----------|----------|
| **P0** | Stránky přístupné veřejnosti (landing, pricing, demo) | `/apps/*`, `/pricing`, `/demo` |
| **P1** | Stránky uvnitř aplikace po přihlášení (dashboard, nastavení) | `/dashboard/*`, `/settings/*` |
| **P2** | Interní nástroje, admin, méně využívané stránky | `/admin/*`, `/internal/*` |

### 2.3. Opravy v dávkách
- Každá dávka = max 15 souborů, aby se dalo snadno reviewovat.
- Opravy provádět pomocí **coding-agent** subagentů s cílem:
  - Přidat chybějící H2 nadpisy tam, kde je vhodné (podle designu).
  - Snížit nadpisy (H3→H2) pouze pokud to dává sémantický smysl a není v rozporu s sekční strukturou.
  - Odstranit duplicitní H2 na jedné stránce (ponechat pouze jeden jako hlavní nadpis sekce).
  - Aktualizovat případné overridy v Tailwind nebo v komponentách, které explicitně nastavují úroveň nadpisu.

### 2.4. Kontrola po opravách
- Znovu spustit lint a ověřit, že počet chyb klesl na nulu v dané dávce.
- Vytvořit náhledový build (`npm run build`) a zkontrolovat, že nedošlo k broken UI.
- Spustit dostupné regresní testy (Playwright snapshoty, pokud existují) – porovnat s baseline.

### 2.5. Dokumentace
- Aktualizovat `docs/design/TYPOGRAPHY_RULES.md` s konkrétními pokyny pro H2 použití.
- Přidat krátkou sekci do `docs/plans/2026-09-02-homepage-h2-unification.md` o tom, jak se pravidla rozšířila na celou aplikaci.
- Pokud se změní výchozí chování komponent, aktualizovat příslušné `.md` soubory v `docs/components/`.

### 2.6. Schválení a merge
- Po dokončení všech dávek vytvořit pull request s názvem:
  `chore/stabilize-h2-headings-<yyyy-mm-dd>`
- Popis PR obsahuje:
  - Souhrn počtu opravených souborů.
  - Odkaz na finální lint report s nulovými chybami.
  - Poznámka o testování (build success, vizuální regrese žádné).
- Požádat o review od CTO (či určeného ownnera UI) a po schválení merge do `main`.

## 3. Odpovědnosti a navržení subagentů
Členové týmu AI agentů mohou být přiděleni následovně:

| Subagent / roli | Název úkolu | Popis |
|-----------------|------------|-------|
| **H2-Auditor** | `agents_list` → vybrat `coding-agent` s kontextem "audit" | Spustí lint, vygeneruje report, navrhne prioritní seznam souborů. |
| **H2-Fixer‑Batch1** | `coding-agent` | Opraví první dávku (P0 stránky). |
| **H2-Fixer‑Batch2** | `coding-agent` | Opraví druhá dávka (P1 stránky). |
| **H2-Fixer‑Batch3** | `coding-agent` | Opraví třetí dávka (P2 a zbytek). |
| **H2-Verifier** | `coding-agent` + `taskflow` | Spustí build, testy, vizuální regrese a připraví finální report. |
| **H2-DocuWriter** | `coding-agent` | Aktualizuje dokumentaci a připraví popis PR. |

Každý subagent dostane jasně definovaný vstupní adresář (např. `src/pages/`, `src/components/`, `apps/`) a výstupní umístění pro své raporty (např. `reports/h2-audit-*.md`).

## 4. Kritéria úspěchu
- Nulové chyby od eslint pravidel souvisejících s H2 na celé aplikaci.
- Žádné negativní dopady na přístupnost (zkontrolováno přes axe-core pokud je součástí testovací sady).
- Vizuální regrese nepotvrzené (snapshot testy pasují).
- Dokumentace odráží nový stav.
- PR úspěšně prošel code review a je sloučen do `main`.

## 5. Odhad úsilí (hrubý)
- Audit: 0,5 h
- Opravy (3 dávky po ~15 souborech): 2–3 h
- Verifikace + dokumentace: 1 h
- Celkem: přibližně **4–5 hodin** čistého času, paralelně proveditelné více agenty.

---
*Připraveno COO Oto pro distribuci mezi AI agenty. Datum: 2026-10-11.*