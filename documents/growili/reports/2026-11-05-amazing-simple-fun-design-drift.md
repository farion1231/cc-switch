# Amazing Simple Fun — proč se UI rozchází s design manuálem

**Datum:** 2026-11-05  
**Zadání:** přímá instrukce Jiřího (mimo pořadí aktivních priorit)  
**Rozsah:** read-only kontrola lokálního repozitáře `/home/growthhacker/projects/amazing-simple-fun`, čistý `main` na `ad6a6236182119a62f41bd8267b9b45a01fa2782`. Nejde o úplný vizuální audit ani o ověření produkce.

## Verdikt

Odchylky mají více příčin. Nejdůležitější je **rozpor mezi dvěma dokumentovanými brand autoritami**; dále manuál pokrývá více pravidel, než vynucují kontroly, a ve zdrojovém kódu zůstaly lokální styly. Zamčené tokeny v CSS jsou naopak podle samostatné kontroly v pořádku. Bez konkrétních obrazovek nelze určit příčinu každé pozorované vizuální odchylky.

## Doložené příčiny a příklady

1. **Konflikt dokumentace (vysoká priorita).** `BRAND.md:15` a `BRAND_DNA.md:35-36` přikazují modrou `#2563EB`, jantarovou, zelenou a fonty Inter/Playfair. `docs/design/DESIGN_SYSTEM.md:1-10,42-53,273-276` se pro UI výslovně označuje za jediný aktivní manuál; CSS `src/styles/main.css:65-79` používá fialovou `#5835d8`, TikTok Sans/Onest. Kdo pracuje podle `BRAND.md`, vytvoří vizuál odporující skutečnému design systému. Neprokazuje to, že současné UI používá staré barvy; prokazuje to nejednoznačné zadání a pravděpodobný zdroj budoucího driftu.

2. **Lokální styly obcházejí sdílené varianty (vysoká priorita).** Manuál `docs/design/DESIGN_SYSTEM.md:20-24,110,1103-1111,1133-1138` požaduje tokeny a sdílené komponenty. `src/components/homepage/hero/AIProductListingHero.tsx:78` přidává `font-semibold` na `<Button>`, ačkoli manuál `:125-137` i `:1108` váhu buttonu zakazuje přepisovat. Stejný soubor `:102-105` skládá kartu z `rounded-xl`, `shadow-lg` a inline gradientu místo varianty/tokenu. `:109-111` vkládá lokální barvy přes inline CSS proměnné. Tato cesta způsobuje, že změna centrální varianty nemusí změnit dané místo.

3. **Kontroly nejsou úplným převodem manuálu (vysoká priorita).** `scripts/lint-design-tokens.sh:1-9,20,252-263` hlavně porovnává zamčené hodnoty `main.css`; při této kontrole prošel. To neověřuje vzhled ani všechny call sites. ESLint souboru `AIProductListingHero.tsx` i `ForecastDashboard.tsx` hlásil **0 aktivních chyb a varování**, přesto výše uvedené příklady zůstávají. `eslint-rules/no-hardcoded-typography.cjs:183-188` má kontrolu malých velikostí a vah defaultně vypnutou; `eslint.config.js:777-787` ji zapíná jen ve vybraných migrovaných adresářích. Neexistuje zjevné pravidlo pro váhu `<Button>` analogické pravidlu pro radius (`eslint.config.js:449`). `eslint.config.js:422,474` nechává jiné typografické kontroly pouze na `warn`.

4. **CI nekontroluje celou design shodu (střední priorita).** `package.json:15,20,42-47` má lint a token script, ale `.github/workflows/ci.yml` definuje typecheck, test a build, bez samostatného lint/design jobu (`:23-80`). Pre-commit spouští ESLint jen nad staged TS/TSX a token script při změně `main.css`; nové lokální styly, které neodpovídají pravidlům, projdou. Toto je procesní mezera, nikoli důkaz, že každý merge obchází hook.

5. **Část nápovědy v samotných kontrolách je zastaralá (střední priorita).** Manuál i CSS uvádějí `--radius-card: 10px` (`DESIGN_SYSTEM.md:112-123`, `main.css:528`), ale `eslint-rules/no-hardcoded-border-radius.cjs:51-54,68-71` doporučuje `rounded-card (20px)` a `rounded-dialog (24px)`. To mate při opravách, i když lint může technicky projít.

6. **Konkrétní in-app odchylka typografie (střední priorita).** `src/pages/forecast/ForecastDashboard.tsx:103-110` ručně styluje hlavičky tabulky `text-sm font-semibold`, zatímco `DESIGN_SYSTEM.md:326-337` říká, že typografie buněk patří sdílené `<Table>` a call site ji nepřepisuje. Tato stránka používá native `<table>`/`<th>`, čímž pravidlo pro sdílenou komponentu není účinné. Úspěšný cílený ESLint běh to potvrdil jako mezeru pokrytí, nikoli jako schválenou výjimku.

## Důležité rozlišení

- `src/components/homepage/hero/AIProductListingHero.tsx:1` potlačuje i18n/hierarchii, nikoli zde uvedený button styl; jeho čistý ESLint výsledek proto nelze vysvětlit jen direktivou `eslint-disable`.
- Starý audit `documents/growili/reports/2026-10-03-homepage-typography.md:16-31` zjistil v produkci chybné doručování fontů. Stav k dnešku nebyl znovu ověřen; může být dalším vizuálním faktorem, ale nesmí být vydáván za aktuálně potvrzenou příčinu.
- Některé `font-semibold` na běžném textu nejsou automaticky porušením: in-app strop je 600 (`DESIGN_SYSTEM.md:352-359`). Výše jsou uváděny jen příklady s konkrétním konfliktem vůči pravidlu nebo komponentě.

## Doporučené pořadí nápravy — návrh, bez změn

1. Jiří/brand owner: určit autoritu `BRAND.md` a `BRAND_DNA.md` vůči současnému `DESIGN_SYSTEM.md`; staré barevné a fontové příkazy označit jako historické nebo schválit záměrnou výjimku. Žádnou paletu neměnit implicitně.
2. CTO/frontend: opravit reprezentativní call sites (button váha, lokální karta/gradient, forecast tabulka) v malé větvi a porovnat screenshoty light/dark + mobil/desktop.
3. CTO: doplnit cílené ESLint pravidlo/test pro button váhu a pro native tabulkové styly, aktualizovat nesprávné popisy radiusů; teprve pak nastavit lint jako CI gate.
4. Pro stránky, které Jiří konkrétně vidí jako chybné, udělat vizuální acceptance proti renderovanému CSS a ověřit font assety. Tento audit je diagnóza repozitáře, nikoli podpis produkční shody.

**Ověření:** `bash scripts/lint-design-tokens.sh` → úspěch. Cílený ESLint nad dvěma uvedenými TSX soubory → 0 aktivních chyb / 0 varování. Bez změny souborů projektu, bez deploymentu.
