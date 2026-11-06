# Amazing Simple Fun — cílená oprava design driftu

**Přímé zadání Jiřího:** „Oprav to prosím“ (navazuje na audit `2026-11-05-amazing-simple-fun-design-drift.md`).  
**Repo:** `/home/growthhacker/projects/amazing-simple-fun`  
**Větev:** `fix/design-manual-drift-20261105`  
**Commit:** `8b1c1e116` (`fix(design): align audited UI with canonical manual`)  
**Stav:** lokálně commitnuto, čistá větev; nepushnuto, nemergnuto, nedeployováno.

## Hotovo

- `BRAND.md` a `BRAND_DNA.md` výslovně označují historickou modrou/Playfair paletu jako neplatnou pro současné Growili UI a odkazují na `docs/design/DESIGN_SYSTEM.md` / `src/styles/main.css`.
- `ForecastDashboard.tsx` používá sdílené `Table`, `TableHead`, `TableCell` a další části místo ručně typograficky stylované native tabulky. Sémantické stavové barvy trendu zůstaly zachované.
- Odstraněny konkrétní přepisy váhy `<Button>` v `AIProductListingHero.tsx`, `CostsDashboard.tsx` a `OrdersAnalyticsPage.tsx`.
- V nevyužitém `AIProductListingHero.tsx` nahrazena lokální karta/inline gradient sdílenými tokeny `rounded-card`, `shadow-card`, `bg-card`.
- Opravena zastaralá nápověda radiusů v `eslint-rules/no-hardcoded-border-radius.cjs` (karta a dialog 10 px, nikoli 20/24 px).
- Přidáno ESLint pravidlo `enforce-button-font-weight`; prozatím `warn`, protože AST průchod nad `src/**/*.tsx` našel ještě 57 existujících `<Button>` call sites s explicitní vahou. Přepnutí na `error` bez migrace by vytvořilo nově blokující baseline.

## Ověření

- Cílený ESLint nad čtyřmi změněnými TSX soubory, configem a pravidly: exit 0.
- `bash scripts/lint-design-tokens.sh`: exit 0, zamčené tokeny odpovídají specifikaci.
- RuleTester nového pravidla: 5 valid/invalid případů úspěšně (včetně `cn(...)`).
- Pre-commit: gitleaks bez nálezu; lint-staged ESLint úspěšný; TypeScript kontrola 4 změněných souborů a 1 importéra bez chyb.
- `git diff --check`: bez chyby; pracovní větev po commitu čistá.
- Vizuální QA v prohlížeči a produkční deployment: **neprovedeno**. Oprava Forecast tabulky může změnit vzhled záměrně směrem ke sdílenému manuálu; vyžaduje vizuální acceptance.

## Zbývající hranice

- Výsledek není globální redesign ani důkaz shody všech obrazovek. 57 starších přepisů váhy tlačítka je potřeba migrovat po komponentách; pak lze pravidlo povýšit na `error`.
- CI nemá samostatný lint gate. Nepřidával jsem jej naslepo přes známý existující lint baseline; je to navazující technická práce pro CTO.
- `AIProductListingHero.tsx` nemá nalezeného importéra; jeho úprava je hygienická a sama o sobě nevysvětluje živý vzhled homepage.
- Další merge/push/deploy nebyl součástí provedeného kroku. Bez vizuální acceptance nelze tvrdit, že produkční odchylky jsou odstraněné.

## Další krok

CTO/frontend ověřit Forecast v light/dark + mobil/desktop a potvrdit případné konkrétní další obrazovky od Jiřího. Poté řešit zbylé přepisy a CI gate po malých dávkách.
