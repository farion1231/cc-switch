# Growili Merchant Center — landing page specification

> **Status:** READY FOR REVIEW, NOT AUTHORIZED FOR DEPLOY  
> **Owner:** COO / product handoff to CTO  
> **Priority context:** direct instruction from Jiří; implementation must remain isolated from `P1-ONBOARDING-ACCEPTANCE`  
> **Aktualizováno:** 2026-09-10  
> **Route:** `/growili/merchant-feed-optimizer`

## 1. Product decision

Landing page může být veřejná před dokončením backendu. Musí ale prodávat **early access**, ne okamžitě použitelný produkt.

- Veřejná stránka je dostupná bez feature flagu.
- Feature flag `early_access` řídí pouze CTA a přístup do modulu.
- Dokud není ověřený backend, primární CTA sbírá zájem nebo žádost o demo.
- Po production acceptance lze stejnou stránku přepnout na Google OAuth bez přepisování layoutu.
- Neověřené funkce se nesmí prezentovat jako hotové.

## 2. Positioning

### Cílový uživatel

Majitel nebo marketingový správce e-shopu, který používá Google Merchant Center, řeší zamítnuté produkty a nechce ručně procházet více obrazovek.

### Hlavní problém

„Nevím rychle, které produkty mají problém a co musím řešit jako první.“

### Hodnota early-access verze

Jedno místo pro připojení GMC a přehled feedů, produktových stavů a validačních problémů — pouze v rozsahu ověřeném na cílovém deployi.

## 3. Claims gate

Před implementací vytvoř matici `claim → důkaz → stav`. Claim lze publikovat jen se stavem `VERIFIED`.

| Claim | Výchozí stav | Potřebný důkaz |
|---|---|---|
| Google OAuth připojení | CODE-VERIFIED | funkční production/staging flow |
| Přehled feedů | CODE-VERIFIED | screenshot z cílového deploye |
| Stav produktů | CODE-VERIFIED | screenshot + API response bez citlivých dat |
| Validace feedu | CODE-VERIFIED | test + UI důkaz |
| Hromadný upload | CODE-VERIFIED | end-to-end důkaz |
| Reporting | CODE-VERIFIED | screenshot + definice metrik |
| Připojení za 2 minuty | UNVERIFIED | měření alespoň 10 dokončených flow |
| Hodinová synchronizace | UNVERIFIED | scheduler konfigurace + runtime log |
| 14 dní zdarma / bez karty | UNAPPROVED | schválený pricing a billing flow |
| AES-256 / EU hosting | UNVERIFIED | security a infrastruktura evidence |
| AI optimalizace / automatické opravy | NOT IMPLEMENTED | nesmí se publikovat |
| Výkonnostní nebo revenue uplift | NO EVIDENCE | case study s metodikou |

## 4. Kanonická copy pro pre-launch

### Hero

**Eyebrow:** Nový modul Growili · Early access

**H1:** Mějte problémy v Google Merchant Center na jednom místě

**Subheadline:** Připojte svůj účet a získejte přehled feedů, produktových stavů a chyb. Přístup otevíráme postupně po technickém ověření.

**Primární CTA:** `Chci early access`

**Sekundární CTA:** `Podívat se, jak modul funguje`

**Mikrocopy:** Nejdřív ověříme kompatibilitu vašeho účtu. Nic automaticky neúčtujeme.

Pokud ještě neexistuje funkční lead destination, CTA nesmí simulovat úspěch. V takovém případě stránku nedeployovat.

### Sekce v pořadí

1. **Hero** — problém, hodnota, jedna hlavní konverze.
2. **Jak to funguje** — Připojení přes Google → načtení dostupných dat → přehled problémů.
3. **Co modul ukáže** — pouze verified capabilities s reálnými screenshoty.
4. **Pro koho je** — e-shopy s aktivním GMC a potřebou rychlejší kontroly.
5. **Co early access znamená** — omezená dostupnost, možné ruční onboarding kroky, žádný příslib termínu.
6. **FAQ** — námitky, data, cena, dostupnost, vztah k GMC.
7. **Final CTA** — zopakovat `Chci early access` se stejným cílem.

### FAQ

**Nahrazuje Growili Google Merchant Center?**  
Ne. Growili pracuje s daty z vašeho účtu a zpřehledňuje jejich kontrolu.

**Je modul už veřejně dostupný?**  
Zatím jej otevíráme postupně. Po žádosti ověříme, zda je váš účet vhodný pro early access.

**Co potřebuji k připojení?**  
Aktivní Google účet s oprávněním k příslušnému Merchant Center účtu.

**Budete měnit produkty automaticky?**  
Ne bez výslovného potvrzení. Automatické opravy nejsou součástí současného veřejného slibu.

**Kolik modul stojí?**  
Cenu zveřejníme až po schválení nabídky. Early-access formulář nesmí naznačovat trial ani placené pokračování, dokud není pricing rozhodnutý.

**Jak pracujete s daty?**  
Stránka musí odkázat na aktuální zásady ochrany osobních údajů. Konkrétní tvrzení o šifrování nebo lokaci dat publikovat jen po technickém ověření.

### Final CTA

**H2:** Chcete mít stav Merchant Center pod kontrolou z Growili?

**Text:** Pošlete žádost o early access. Ozveme se s dalšími kroky po ověření kompatibility.

**CTA:** `Chci early access`

## 5. Technická architektura

### Routing

- Přidej lazy-loaded veřejnou route `/growili/merchant-feed-optimizer`.
- Stránka se musí načíst i při `early_access = false`.
- Přístup do chráněného modulu zůstane samostatná route za autentizací a feature flagem.
- Nezaváděj novou alternativní marketingovou URL; případné starší URL přesměruj 301 na kanonickou.

### Feature flag

Použij jeden flag s explicitním režimem:

```ts
type MerchantCenterLaunchMode = 'waitlist' | 'demo' | 'oauth';
```

- `waitlist`: CTA vede na skutečný lead form a potvrzení vzniká až po úspěšném uložení.
- `demo`: CTA vede na existující rezervaci/demonstraci.
- `oauth`: povolit až po end-to-end acceptance Google OAuth a GMC backendu.
- Neznámá nebo chybějící hodnota musí bezpečně spadnout do `waitlist`.

Samotný boolean `early_access` může řídit oprávnění uživatele v produktu, ale nestačí pro tři odlišné CTA režimy.

### Doporučené komponenty

```text
MerchantFeedOptimizerLandingPage
├── SEOHead
├── MerchantHero
├── HowItWorks
├── VerifiedCapabilities
├── EarlyAccessDisclosure
├── LandingFAQSection
└── FinalCTA
```

Nejdřív znovu použij existující design-system komponenty a `LandingFAQSection`. Nevytvářej druhý accordion, button systém ani samostatné tokeny.

## 6. Implementační postup pro AI coding agenta

### Fáze 0 — ověření vstupů

1. Otevři `AGENTS.md`, package scripts a routovací konfiguraci projektu.
2. Najdi existující landing-page template, CTA komponentu, FAQ, SEO a analytics helper.
3. Najdi skutečnou implementaci Google OAuth, GMC typů, validace a API handlerů.
4. Vyplň claims matrix důkazy z cílové branche/deploye.
5. Ověř skutečný lead destination. Pokud neukládá data, zastav implementaci CTA a eskaluj.

### Fáze 1 — minimální veřejná stránka

1. Vytvoř jednu page komponentu a malé sekční komponenty jen tam, kde zlepší testovatelnost.
2. Přidej route a lazy import.
3. Použij kanonickou pre-launch copy z tohoto dokumentu.
4. Přidej reálné screenshoty pouze po odstranění osobních údajů.
5. Přidej FAQ přes sdílenou komponentu.
6. Obě primární CTA napoj na stejný launch-mode resolver.

### Fáze 2 — lead flow

1. Formulář sbírá pouze schválené minimum polí.
2. Validuj data na klientu i serveru.
3. Zobraz úspěch až po potvrzeném zápisu.
4. Zapiš ownera, zdroj stránky, timestamp a souhlas podle platných pravidel.
5. Zajisti viditelné chybové stavy a možnost opakovat odeslání.
6. Nevytvářej falešný `setTimeout` success stav.

### Fáze 3 — SEO, analytika a přístupnost

1. Nastav unikátní title, description, canonical a OG metadata.
2. Použij právě jedno H1 a logickou hierarchii nadpisů.
3. Přidej události `gmc_lp_view`, `gmc_lp_primary_cta`, `gmc_lp_secondary_cta`, `gmc_lp_lead_success` a `gmc_lp_lead_error`.
4. U událostí neposílej e-mail, Merchant ID ani jiné osobní údaje.
5. Ověř klávesnici, focus states, reduced motion a kontrast WCAG AA.
6. Optimalizuj hero media a zabraň layout shiftu.

### Fáze 4 — přepnutí na OAuth

Provést až po production acceptance:

1. Ověř consent screen, scopes, callback, error/cancel stav a token refresh.
2. Ověř účet bez Merchant Center i uživatele bez potřebných oprávnění.
3. Nastav launch mode na `oauth` bez změny copy, která není podložená.
4. Po měření reálného času lze samostatně rozhodnout o časovém claimu.

## 7. Testy a acceptance

### Automatické

- route renderuje bez přihlášení;
- každý launch mode vede na správný cíl;
- neznámý režim padá do `waitlist`;
- lead success vznikne pouze po úspěšném persistence response;
- FAQ je ovladatelné klávesnicí;
- není přítomen žádný zakázaný claim;
- chybějící screenshot nerozbije layout;
- CS i EN varianta nemá chybějící i18n klíče.

### Manuální QA

- 360 px, 768 px, 1280 px a 1440 px;
- Chrome, Safari a Firefox;
- keyboard-only a screen-reader smoke test;
- pomalá síť a selhání formuláře;
- správná analytics událost bez PII;
- CTA v hero a final sekci mají stejný cíl;
- veřejná stránka funguje, i když je produktový `early_access` vypnutý.

### Release gate

Deploy je `NO-GO`, pokud:

- lead destination ztrácí nebo simuluje lead;
- copy obsahuje `UNVERIFIED`, `UNAPPROVED` nebo `NOT IMPLEMENTED` claim;
- CTA vede do nefunkčního OAuth;
- není dostupná privacy informace;
- build, typecheck nebo relevantní testy selžou;
- nelze provést rollback.

## 8. Release a rollback

1. Implementuj v izolované branch/worktree.
2. Přilož screenshoty desktop/mobile a výsledky relevantních testů.
3. Vyžádej product/copy review a technické review.
4. Deploy nejdřív na preview/staging.
5. Po schválení zveřejni pouze marketingovou route v režimu `waitlist` nebo `demo`.
6. Rollback: změň launch mode na `waitlist`; při chybě stránky vrať route na poslední stabilní build.
7. Po 7 a 30 dnech vyhodnoť návštěvy, CTA click rate, validní lead rate a chyby formuláře.

## 9. Definition of done

- [ ] Veřejná route existuje a je dostupná bez produktu.
- [ ] Copy prošla claims gate.
- [ ] Jeden skutečný lead destination má vlastníka a SLA.
- [ ] CTA nikdy nevede na nedokončený backend.
- [ ] Analytics měří funnel bez PII.
- [ ] Accessibility a responsive QA prošly.
- [ ] Build, typecheck a relevantní testy prošly.
- [ ] Rollback byl popsán a vyzkoušen.
- [ ] Jiří schválil nabídku/copy; CTO schválil implementaci/deploy.

## 10. Otevřená rozhodnutí

1. Kam přesně se má early-access lead ukládat a kdo jej přebírá?
2. Bude CTA dočasně waitlist, nebo rezervace dema?
3. Jaká je schválená cena nebo bude cena zatím úplně vynechána?
4. Které capabilities mají runtime důkaz z cílového deploye?
