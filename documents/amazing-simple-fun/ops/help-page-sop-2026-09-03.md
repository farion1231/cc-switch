# SOP — `/help` stránka pro Amazing Simple Fun

**Datum:** 2026-09-03  
**Projekt:** Amazing Simple Fun / Growili  
**Scope:** provozní SOP pro obsah, aktualizace a kontrolu help stránky  
**Status:** draft k použití

## 1. Účel

Zajistit, aby help stránka byla:
- přesná vůči reálnému produktu,
- srozumitelná pro uživatele,
- průběžně udržovaná bez copy driftu,
- bezpečná z pohledu claimů, CTA a support očekávání.

## 2. Aktuální zjištění

V repozitáři je ověřená help stránka pro CSS modul na route:
- `/css/help`
- soubor: `src/pages/css/CSSHelpPage.tsx`

Tato stránka je dnes chráněná module guardem a funguje jako produktová help/FAQ stránka pro CSS uživatele, ne jako obecný veřejný help center.

## 3. Kdy tento SOP použít

Použij tento SOP vždy, když:
- se mění obsah `/css/help`,
- se přidává nové FAQ,
- se mění proces switch/request flow,
- se mění support kontakt nebo CTA,
- se mění claimy o úspoře, rychlosti nebo fungování CSS.

## 4. Vlastník

- **Owner:** COO
- **Support:** CTO (implementace), CMO (copy), CEO/Jiří (schválení claimů a customer-facing změn, pokud mění promise)

## 5. Vstupy před úpravou

Před jakoukoliv změnou vždy zkontroluj:
1. `src/pages/css/CSSHelpPage.tsx`
2. `src/app/router/routes/CssRoutes.tsx`
3. `docs/modules/css.md`
4. `docs/strategy/css-asset-inventory.md`
5. relevantní i18n klíče v `src/lib/i18n/translations/*/css.ts`
6. aktuální support e-mail konstantu `GROWILI_SUPPORT_EMAIL`

## 6. Obsahové bloky, které musí help stránka pokrýt

Help stránka musí obsahovat pouze ověřený obsah v těchto blocích:

### A. Co je služba
- stručné vysvětlení co CSS dělá,
- bez přehnaných nebo neověřených claimů,
- jasně odlišit edukaci od slibu výsledku.

### B. Jak funguje
- jednoduché vysvětlení mechanismu,
- popsat princip bez zbytečné technické hloubky,
- uvést, co se děje v pozadí jen pokud je to pravda a stabilní.

### C. Jak přepnout / začít
- konkrétní kroky,
- CTA musí vést na správnou route,
- nesmí popisovat flow, které už neodpovídá aktuální implementaci.

### D. Výhody
- jen bezpečné a obhajitelné body,
- žádné garance výsledku bez explicitního schválení,
- benefit ≠ promise.

### E. FAQ
- řešit nejčastější otázky a námitky,
- odpovědi mají být krátké, konkrétní a bez marketingové omáčky,
- FAQ musí odpovídat skutečnému produktu, ne plánované budoucnosti.

### F. Kontakt / eskalace
- jasný support kontakt,
- žádný nejasný ownership,
- pokud je uveden response expectation, musí být interně splnitelný.

## 7. Pravidla pro změny obsahu

### Povolené bez extra schválení
- jazykové zpřesnění bez změny významu,
- zlepšení čitelnosti,
- oprava překlepů,
- doplnění FAQ podle již ověřeného procesu,
- oprava rozbitého odkazu nebo CTA.

### Vyžaduje schválení
- nové customer-facing sliby,
- změna claimu o úspoře, rychlosti nebo výkonu,
- změna support expectation,
- změna procesu, která ovlivňuje onboarding nebo customer promise,
- změna pricing / free vs paid framing.

## 8. Provozní workflow

### Krok 1 — Ověř realitu
- Zkontroluj, že route stále existuje.
- Zkontroluj, že CTA míří na správnou route.
- Zkontroluj, že text odpovídá aktuálnímu funnelu a status modelu.

### Krok 2 — Ověř copy zdroj pravdy
- Pokud je text v i18n, upravovat zdrojové translation klíče, ne hardcode text.
- Udržuj konzistenci mezi CS a EN, pokud je stránka dvojjazyčná.

### Krok 3 — Ověř claimy
Každý claim projde tímto filtrem:
- Je pravdivý?
- Je aktuální?
- Je obhajitelný důkazem?
- Není to skrytý slib výsledku?

Pokud neprojde, claim nepoužívat.

### Krok 4 — Ověř support dopad
Po každé změně zkontroluj, zda nevzniká:
- nový typ support dotazu,
- vyšší expectation na onboarding tým,
- nejasnost v tom, co má uživatel udělat dál.

### Krok 5 — Kontrola po úpravě
Minimum checklist:
- route funguje,
- CTA fungují,
- překlady existují,
- žádné broken i18n keys,
- žádný hardcoded text mimo standard,
- support kontakt je správně,
- FAQ neodporuje reálnému flow.

## 9. QA checklist před publikací

- [ ] Název stránky odpovídá obsahu
- [ ] Všechny texty jsou i18n-safe
- [ ] CTA vede na správnou route
- [ ] Help copy neodporuje request flow
- [ ] FAQ neobsahuje neověřené sliby
- [ ] Benefit sekce neobsahuje garance
- [ ] Support email je aktuální
- [ ] Obsah je stručný a srozumitelný
- [ ] Stránka pomáhá uživateli udělat další krok

## 10. Co nedělat

- Nepřidávat marketingové sliby bez evidence.
- Nepopisovat budoucí funkcionalitu jako hotovou.
- Nemíchat produktovou help stránku s landing page copy.
- Neschovávat důležité kroky do vágních formulací.
- Nepřidávat support expectation, které tým neumí držet.
- Nehardcodovat text, pokud stránka běží přes i18n.

## 11. Doporučená struktura pro každou budoucí help stránku

1. Co to je
2. Jak to funguje
3. Jak začít
4. Co uživatel získá
5. FAQ
6. Kontakt / další krok

## 12. Aktuální doporučení pro Amazing Simple Fun

Na základě ověřeného stavu doporučuji:
1. Brát `/css/help` jako existující referenční help page.
2. Všechny budoucí úpravy vést přes i18n zdroje, ne přes hardcoded copy.
3. Před větší úpravou sladit help copy s canonical CSS funnel dokumentací.
4. Nepoužívat help stránku jako sales page; její role je snížit nejistotu a posunout uživatele k dalšímu správnému kroku.

## 13. Výstup / evidence

Tento SOP je uložen zde:
- `documents/amazing-simple-fun/ops/help-page-sop-2026-09-03.md`
