# Typografie veřejné homepage Growili — 2026-10-03

## Závěr

Homepage nyní není typograficky sjednocená. Nejprve opravit doručování fontů; poté sjednotit sekční nadpisy a zopakovat kontrolu zalamování.

Zadání: přímý požadavek Jiřího, veřejná https://growili.com/, především homepage. Read-only kontrola živého webu, bez změn kódu, nasazení nebo zákaznických procesů.

## Ověřený rozsah

- Chromium, český obsah, světlý režim, viewporty 1440×900, 390×900 a 320×900.
- Průchod celé homepage, computed styles, FontFaceSet, screenshoty, kontrola rozměrů.
- Opakovaná kontrola síťových odpovědí fontů a skutečně renderovaného fontu přes Chrome DevTools Protocol.
- Nejde o kontrolu všech stránek, všech interaktivních stavů, Safari či Firefoxu. Anglická verze nebyla plně auditována.

## 1. Vysoká priorita: zamýšlené fonty se nenačtou

Všechny deklarované řezy Outfit a Onest 400/500/600/700 měly `document.fonts` status `error`. Konzole: `Failed to decode downloaded font`, `OTS parsing error: invalid sfntVersion: 1008813135`.

Přímo ověřené odpovědi:

| URL | HTTP / deklarovaný typ | Skutečný obsah |
|---|---|---|
| https://growili.com/fonts/Outfit-700.woff2 | 200 / font/woff2 | HTML začínající `<!DOCTYPE html>`, 1610 B |
| https://growili.com/fonts/Onest-400.woff2 | 200 / font/woff2 | HTML začínající `<!DOCTYPE html>`, 1621 B |

CDP na H1 potvrdilo `Liberation Sans`, `LiberationSans-Bold`, `isCustomFont: false`. To je konkrétní fallback testovacího systému; jiná zařízení mohou vykreslovat jiné náhradní písmo.

Dopad: návštěvník nedostává zamýšlené fonty, mění se šířky znaků, zalamování i vizuální dojem. HTTP 200 samo nestačí jako kontrola správnosti fontu.

Doporučení pro frontend/deployment ownera: ověřit skutečné fontové soubory a routování `/fonts/`, případně upstream assety/cache. Přesná příčina v konfiguraci není tímto auditem určena. Acceptance: validní WOFF2 obsah, FontFaceSet loaded, žádné decode chyby, CDP potvrzuje zamýšlené webfonty.

## 2. Vysoká priorita: dva rozdílné styly sekčních H2

| Skupina | Desktop 1440 | Mobil 390 | Mobil 320 |
|---|---|---|---|
| Zisk / Automatizace / Péče / FAQ / závěrečná CTA | Outfit 700, 52/57,2 px | cca 34/37,35 px | 32/35,2 px |
| Reference / Kruh / Funkce / Integrace / Kroky / Ceník | Onest 500, 52/71,5 px | 52/71,5 px | 52/71,5 px |

Fontové rodiny v tabulce jsou deklarace CSS, ne úspěšně načtené fonty.

Konkrétní sekce druhé skupiny:

1. Vaše konkurence už roste s Growili o desítky procent
2. Growili Kruh úspěchu
3. Všechny funkce na jednom místě
4. Propojte všechny vaše nástroje na jednom místě
5. Začněte okamžitě ve 3 jednoduchých krocích.
6. Škálovatelné cenové plány pro váš růst!

Na mobilu je tak šest H2 větších než hlavní H1 (48 px). Nadpis reference má při 320 px výšku 429 px, tedy šest řádků po 71,5 px. Rozdíl v rodině, řezu a řádkování existuje i na desktopu.

Doporučení: sjednotit těchto šest sekcí na společný responsivní styl sekčního nadpisu, pokud nejsou výslovně zamýšlenou výjimkou. Stávající škála ostatních H2 (32–34 px mobil / 52 px desktop) je konkrétní výchozí kandidát, nikoli zde ověřená závazná brand specifikace.

## 3. Střední priorita: hero zabírá na mobilu mnoho prostoru

H1 má 48/48 px na obou mobilních šířkách, na desktopu 100/100 px. Při zachyceném mobilním stavu má výšku 288 px; úvodní odstavec na 390 px má 21/32 px a výšku 192 px. Rotující text může výšku měnit, proto tyto hodnoty nejsou garantované pro všechny animační fáze.

Není to samo o sobě potvrzená technická chyba. Po opravě fontů znovu vyhodnotit délku hero a polohu CTA; případné zmenšování písma dělat až na správných fontech.

## Důkazy

- [Desktop 1440](homepage-1440.png)
- [Mobil 390](homepage-390.png)
- [Mobil 320](homepage-320.png)
- [Reprodukční skript](typography-audit.cjs)
- [Detail mobilního hero včetně cookie panelu](mobile-hero.png)

Celostránkový screenshot 390 px a detail hero byly také vizuálně prohlédnuty. Dodatečný pokus o samostatné výřezy dvou níže položených sekcí skončil timeoutem při hledání H2; tyto výřezy nebyly vytvořeny. Kompletní screenshoty všech tří viewportů z původního průchodu jsou zachované. Závěry o hodnotách stylů vycházejí z úspěšného měření DOM, nikoli z neúspěšného pokusu o výřezy.

Šířka dokumentu odpovídala viewportům (1440, 390, 320). To samo nedokazuje absenci lokálně oříznutých prvků; některé carousely jsou záměrně mimo viewport. Při 320 px měl nadpis sekce kroků box široký 305 px při levém odsazení 16 px, tedy přesah do pravého okraje; po opravě fontů a škály znovu ověřit.

## Owner / další postup

- Doporučený owner oprav: frontend/deployment (CTO).
- Pořadí: validní fontové assety → společný styl H2 → vizuální retest hero, CTA a zalamování.
- Eskalace: potvrzené oříznutí klíčového textu nebo nepřístupné CTA by vyžadovalo přednostní opravu; tento audit takový dopad neprokázal.
- Aktuální stav: kontrola dokončena, opravy neprovedeny.

## Omezení obnovy

Původní agent po restartu nebyl aktivní, jeho uložený výsledek nebyl nalezen. Měření byla provedena znovu. Čtení CURRENT-WORK.md přes dostupný memory_get selhalo s `MEMORY_PATH_NOT_ALLOWED`; nativní read nástroj nebyl dostupný. Kontrola vychází z přímého zadání uživatele, nikoli z tvrzení o aktualizovaném pořadí sdílených priorit.
