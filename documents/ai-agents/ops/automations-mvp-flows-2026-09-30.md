# AI Agents — automatizace: user flows a MVP
Datum: 2026-09-30  
Vlastník návrhu: Oto, COO  
Zadání: přímý pokyn Jiřího, Telegram zpráva 1297 (pracovní reference DIRECT-1297; nejde o ID schválené roadmapy).  
Deadline dokumentu: dnes 10:30 dle zadání; časové pásmo zadání neurčeno.  
Stav: návrh připravený k implementačnímu posouzení, nikoli implementace nebo schválená změna zákaznického procesu.

## 1. Rozhodnutí a provozní situace

**Doporučení: jeden trigger → jedna chat akce, jeden společný editor, žádný workflow builder. Žádná nová backendová tabulka v základním rozsahu.**

- Trigger: `manual` nebo `schedule` s cron výrazem.
- Akce: `chat`, povinný prompt, volitelný textový kontext, konkrétní agent.
- Čtyři hlavní plochy: seznam, tlačítko vytvoření, společný dialog tvorby/úprav, detail s historií.
- Uložení a aktivace jsou oddělené. Nová automatizace se uloží jako pozastavená.
- První ověřitelná hodnota: vytvořit → uložit → ručně spustit → otevřít skutečný výsledek v historii.
- Plánování nesmí uživateli tvrdit, že funguje na pozadí, pokud je uložené jen v prohlížeči.

### Dvě realizovatelné varianty

| Varianta | Co skutečně funguje | Uložení | Omezení |
|---|---|---|---|
| A: lokální MVP / pilot | Tvorba, úprava, aktivace manual, pauza, ruční chat běh, lokální historie; schedule lze nastavit a uložit | localStorage pro definice a minimální metadata běhů; skutečné výstupy přes existující chat, je-li dostupný | Aktivace schedule není povolena; UI vysvětlí nedostupné automatické spouštění. Není to plnohodnotná časová automatizace |
| B: funkční MVP včetně schedule | Vše z A + cron spouští chat i při zavřeném prohlížeči | Již existující perzistentní úložiště/job konfigurace a historie + scheduler | Pouze pokud CTO ověří potřebné existující schopnosti. Bez nových tabulek, ale integrační práce je nutná |

**Preferovat B, pokud infrastruktura existuje. Jinak dodat A jako transparentně označený pilot.** B nelze slíbit pouze na základě tohoto návrhu. Nová jednoduchá tabulka je až výslovně schválená alternativa, není součástí základního odhadu a sama o sobě nenahradí scheduler.

## 2. Přesná definice MVP

### Automatizace

- Název: povinný, návrh limitu 80 znaků.
- Agent: povinný `agentId`; předvyplněný při vstupu z detailu agenta. Seznam pouze agentů přístupných uživateli.
- Trigger:
  - `manual`: spouští uživatel přes „Spustit nyní“.
  - `schedule`: standardní pětisložkový cron (minuta, hodina, den měsíce, měsíc, den týdne). Bez sekund a rozšířených syntaxí v MVP.
  - Jednoduché předvolby „Denně v 9:00“, „Pracovní dny v 9:00“ + „Vlastní cron“; interně vždy cron výraz.
  - Časové pásmo je povinné, předvyplněné z profilu/prohlížeče a viditelné. Pro CZ typicky Europe/Prague; nepoužívat pevné GMT+2 jako náhradu pásma.
  - Validace a náhled dalších tří termínů používají stejnou cron interpretaci jako runner. Nepodporovaný výraz odmítnout s konkrétní chybou.
- Akce: jediný typ `chat`.
  - Prompt: povinný po odstranění okrajových mezer.
  - Kontext: volitelný prostý text, ne soubory ani automaticky připojená historie.
  - Limity promptu a kontextu převzít z existujícího chat API a zobrazit v editoru, nevymýšlet další modelová nastavení.
  - Každý běh vytváří novou chat konverzaci, aby se kontext mezi běhy neakumuloval.
  - Výsledek zůstane v chatu; žádné automatické publikování, e-maily nebo externí změny. Použít režim bez vedlejších efektů; není-li dostupný, je to podmínka pro CTO před aktivací automatického běhu.
- Stav definice: `active` / `paused`; chyby běhu nejsou třetí stav automatizace.
- Ruční „Spustit nyní“ lze použít i u pozastavené nebo schedule automatizace; nemění její stav ani příští cron termín.
- Rozpracovaný neuložený formulář je pouze lokální stav editoru, nepotřebuje samostatný backendový draft.

### Běh a historie

Minimální záznam: ID běhu, ID automatizace, čas zahájení/ukončení, zdroj manual/schedule, stav, ID konverzace nebo bezpečná chyba. Uložit snapshot promptu/kontextu v již existující chat zprávě; nebudovat plné verzování definic.

Stavy: „Běží“, „Dokončeno“, „Chyba“, „Stav neověřen“. Poslední stav použít při ztrátě spojení, pokud nelze výsledek ověřit; nezaměňovat ho s chybou ani úspěchem.

Ve variantě A uchovávat lokálně nejvýše posledních 50 metadat běhů na automatizaci. Předem označit omezenou historii; neslibovat auditní log. Plné odpovědi neduplikovat do localStorage. Pokud chat API nemá trvalou historii, výstup dostupný jen v aktuální relaci a tato degradace musí být viditelná.

## 3. User flows

### F1 — Vytvoření

1. AI Agents → Automatizace → „Vytvořit automatizaci“.
2. Dialog: název, agent, „Ruční / Podle plánu“, případně předvolba/cron + pásmo, prompt, sbalitelný kontext.
3. U schedule zobrazit lidské shrnutí a tři příští termíny. Ve variantě A trvale „Plán lze uložit, automatické spouštění zatím není dostupné“.
4. „Uložit“ zvaliduje pole a uloží definici jako pozastavenou. Chyba uložiště zachová vyplněný formulář a nabídne opakovat uložení.
5. Otevře se detail s akcemi „Spustit nyní“, „Aktivovat“, „Upravit“.
6. Zrušení beze změn zavře dialog; při neuložených změnách nabídne „Zahodit změny / Pokračovat v úpravě“.

Výsledek: jedna uložená definice, žádný nevyžádaný chat běh. Prázdný prompt, chybějící agent a neplatný cron blokují uložení.

### F2 — Úprava

1. Seznam nebo detail → „Upravit“ → stejný předvyplněný dialog.
2. Upravit název, trigger, čas/pásmo, prompt či kontext.
3. U aktivní automatizace upozornit „Změny se použijí od dalšího běhu“.
4. „Uložit změny“ zachová dosavadní stav active/paused, pokud je nová konfigurace platná a podporovaná.
5. Při neúspěšném uložení/aktualizaci scheduleru platí stará konfigurace; UI nehlásí úspěch. U varianty B vyžaduje potvrzení runneru.
6. Ve variantě A při změně active manual → schedule explicitně zobrazit „Uložit a pozastavit“.
7. Právě běžící chat dokončí původní zadání; historie se nepřepisuje.

Výsledek: detail ukazuje skutečně uloženou konfiguraci a aktualizovaný další termín.

### F3 — Aktivace

1. Pozastavená automatizace → „Aktivovat“.
2. Ověřit existenci/přístup k agentovi, validitu zadání a u schedule dostupnost runneru a perzistence.
3. U schedule stručné potvrzení: shrnutí času, pásmo a první běh; zmínit, že se využívá stávající AI limit účtu.
4. Až po úspěšném potvrzení uložení/runneru zobrazit „Aktivní“.
5. Aktivace sama nespustí chat. Manual čeká na ruční kliknutí; schedule na nejbližší budoucí termín.
6. Varianta A: schedule „Aktivovat“ disabled s viditelným vysvětlením. Nezaměnit za tiché selhání.

Výsledek: uživatel chápe, kdy a jak proběhne další běh.

### F4 — Pauza a opětovná aktivace

1. Aktivní automatizace → „Pozastavit“, bez zbytečného potvrzovacího dialogu.
2. Po potvrzení runneru/uložení změnit stav na „Pozastaveno“ a u schedule skrýt příští běh.
3. Zpráva „Další plánované běhy jsou pozastavené. Probíhající běh se dokončí.“
4. Neodeslaná spuštění musí respektovat nový stav před startem. Ruční spuštění zůstává možné.
5. Opětovná aktivace použije nejbližší budoucí termín; žádné dohánění zmeškaných termínů.
6. Při chybě pauzy ponechat původní stav a výslovně říct „Pozastavení se nepodařilo; automatizace může dál běžet“.

Výsledek: pauza není zaměněná se zrušením probíhajícího běhu.

### F5 — Ruční spuštění

1. Detail → „Spustit nyní“ použije uloženou, nikoli neuloženou verzi.
2. Ověřit přístup, vstup a existující AI limit. Přidat běh a během požadavku zakázat opakovaný klik.
3. Chat ukáže průběh; dokončení zpřístupní výsledek a záznam historie.
4. Selhání ukáže srozumitelnou chybu. „Spustit znovu“ vytvoří nový běh až po vědomém kliknutí.
5. Žádný automatický retry. Při nejasném výsledku nejprve ověřit běh dle jeho ID, nevytvářet automaticky druhý placený požadavek.

Výsledek: jeden klik vytvoří nejvýše jeden zamýšlený běh; limity a chyby nejsou skryté.

### F6 — Zobrazení běhů

1. Seznam → klik na název → detail.
2. Shrnutí konfigurace, stav a případný příští termín; pod tím běhy od nejnovějšího.
3. Řádek: čas, manual/schedule, stav, délka pokud známá, „Otevřít výsledek“.
4. Klik otevře existující chat. Chyba ukáže důvod a možnost nového ručního běhu.
5. Prázdný stav „Zatím žádné běhy“ + „Spustit nyní“.
6. Nedostupný historický chat nesmí vypadat jako prázdný úspěšný výstup; zobrazit „Výsledek již není dostupný“.

Výsledek: uživatel vidí, zda běh proběhl a co vytvořil, bez dalšího analytického dashboardu.

## 4. Potřebné UI komponenty

| Komponenta | Minimální obsah a chování |
|---|---|
| Seznam automatizací | Název, agent, trigger/srozumitelný plán, stav, poslední běh, další běh pouze pokud skutečně naplánován; klik do detailu; menu Upravit / Aktivovat či Pozastavit |
| Tlačítko „Vytvořit automatizaci“ | V záhlaví a prázdném stavu; vždy tentýž dialog |
| Dialog vytvoření/úprav | Sdílená pole a validace, dvě varianty triggeru, jednoduché předvolby + vlastní cron, pásmo, prompt a sbalitelný kontext; Uložit / Zrušit |
| Detail automatizace | Shrnutí, badge stavu, Upravit / Aktivovat nebo Pozastavit / Spustit nyní |
| Historie běhů uvnitř detailu | Seznam skutečných běhů, stav a otevření výsledku/chyby, nejnovější nahoře |
| Sdílené stavy | Loading, prázdný seznam/historie, chyba čtení či zápisu, disabled a pending akce, neuložené změny |
| Označení lokálního režimu | „Uloženo jen v tomto prohlížeči. Data se nesynchronizují a mohou být smazána při vyčištění prohlížeče.“ |

Použít existující design systém a chat view, bez nového grafického builderu. Dialog ovladatelný klávesnicí, popisky polí, chyby u pole a focus po zavření zpět na spouštěč. Potvrzení jen při ztrátě změn a první aktivaci plánovaného spouštění.

## 5. Uložení a minimální technický kontrakt

Nejde o návrh nové databázové tabulky. Jde o tvar dat pro lokální stav nebo již existující úložiště:

- Definition: `id, name, agentId, trigger {type, cron?, timezone?}, action {type:"chat", prompt, context?}, status, createdAt, updatedAt`.
- Run: `id, automationId, source, startedAt, finishedAt?, status, conversationId?, error?`.
- Tenant/user scope musí vycházet ze stávající identity; localStorage oddělit dle účtu a workspace. Po přepnutí účtu nesmí UI načíst data jiného účtu.
- Do lokálních dat neukládat přístupové tokeny ani API klíče.
- Oddělit funkce načtení/uložení/spuštění od UI, aby pozdější změna úložiště nevyžadovala přepis obrazovek. Žádný obecný plugin framework.
- Před chat požadavkem ověřit dostupnost lokálního zápisu. Pokud selže, nevydávat tichý příslib uchování historie.
- Pokud existující chat runner neumí dohledat běh po reloadu, lokální běh označit „Stav neověřen“, nikoli automaticky „Chyba“.

### Podmínky varianty B, které ověří CTO

1. Existuje perzistentní definice jobu, tenant-scoped serverové spouštění a dohledatelný výsledek bez nové tabulky.
2. Naplánovaná práce funguje se zavřeným browserem; lze ji potvrzeně aktualizovat a pozastavit.
3. Jeden job nemá souběžné běhy. Překrytý termín se přeskočí, nedohání se; důvod je viditelný v dostupném job logu.
4. Existující runner zabrání duplicitě stejného plánovaného termínu a opakovaného požadavku.
5. Cron náhled a runner mají stejná pravidla časového pásma/DST. Návrh: neexistující místní čas přeskočit, opakovaný místní termín spustit jednou; pokud existující runner toto nepodporuje, před aktivací upravit specifikaci a popisek podle ověřeného chování.
6. Stávající oprávnění a AI limity se kontrolují i při běhu na pozadí; žádné privilegované obcházení. Bezpečný chat bez externích vedlejších efektů.
7. Chyba běhu je viditelná v historii i seznamu. Automatický retry a notifikace nejsou nutné pro MVP.

Pokud podmínky neplatí, varianta B není pokrytá odhadem pouhé integrace. Neskrývat nový scheduler nebo oprávnění jako „malou frontend úpravu“.

## 6. Co odložit

- Webhooky, event triggery, podmínky, větvení, několik akcí, drag-and-drop canvas.
- Import souborů jako kontext, retrieval konektory, proměnné, sdílené šablony.
- E-mail/Slack notifikace a jakékoli externí publikování.
- Automatické retry, backfill zmeškaných běhů, složité fronty a nastavitelná paralelizace.
- Verzování a rollback definic, export historie, dlouhodobý auditní log.
- Nová týmová oprávnění; použít existující přístupy, bez sdílení lokálních definic.
- Hromadné operace, duplikace, mazání a archivace definic, pokročilé filtry a vyhledávání.
- Vlastní model/budget editor, analýzy nákladů, další action typy.
- Nové backendové tabulky bez samostatného schválení.

## 7. Odhad úsilí — člověkohodiny

Orientační implementační odhad, nikoli naměřená pracnost ani závazek dodání. Jeden vývojář znalý aplikace, existující autentizace, design systém, navigace a funkční chat API. Zahrnuje základní responzivitu a chybové stavy; nezahrnuje tvorbu chat backendu. Návrh nebyl porovnán s repozitářem.

| Prvek | Hodiny | Obsah / hranice |
|---|---:|---|
| Ověření existujícího chat/storage/scheduler kontraktu | 2–3 | Read-only technické ověření, rozhodnutí A/B |
| Seznam automatizací | 3–4 | Řádky, badge, navigace, prázdný a chybový stav |
| CTA Vytvořit automatizaci | 0,5–1 | Záhlaví + prázdný stav |
| Společný dialog tvorby/úprav | 4–6 | Pole, agent, prompt, kontext, validace, neuložené změny |
| Schedule/cron editor | 3–4 | Předvolby, cron validace, pásmo, náhled termínů, stav nedostupné aktivace |
| Aktivace/pauza | 1,5–2 | Přechody stavů, pending/error, dostupnost akcí |
| Detail automatizace | 2–3 | Shrnutí a akce; historie oceněna zvlášť |
| Historie běhů | 3–4 | Řádky, stavy, otevření výsledku/chyby |
| Lokální persistence | 2–3 | Definice, metadata běhů, oddělení účtů, chyba zápisu |
| Napojení ručního chat běhu | 3–5 | Request, ID běhu, skutečný výsledek, chyba, ochrana proti dvojkliku |
| Integrační QA a akceptace | 4–6 | Kritické flows, reload, chybové scénáře, klávesnice |
| **Varianta A celkem** | **28–41** | Lokální pilot, schedule pouze konfigurace |
| Připojení existujícího scheduleru a perzistence | +4–6 | Náhrada lokální autority existujícím serverovým úložištěm; create/update/pause |
| Background historie a mapování chat výsledků | +2–3 | Jen reuse již dostupného run logu/API |
| QA plánování / pauzy / restartu / duplicity | +2–3 | Ověření se zavřeným prohlížečem a známým cron termínem |
| **Varianta B celkem** | **36–53** | A + integrace, jen při splnění podmínek výše |

Plánovací rezerva 20 % nad rozpis: A přibližně **34–50 h**, B **44–64 h**. Rozsahy A/B se nesčítají. B používá základ A jako odhad UI práce; nejde o požadavek nejdříve dodat a pak zahodit lokální pilot. Při výběru B po technickém ověření realizovat přímo serverový adaptér.

Jednoduchá nová tabulka: pouze podmíněná alternativa po schválení, orientačně dalších 4–8 h pro migraci, oprávnění a CRUD nad hotovou infrastrukturou; není to cena vývoje scheduleru. Při chybějícím runneru nový odhad až podle zjištěného kontraktu.

Deadline 10:30 se vztahuje k tomuto návrhu, ne k implementaci v desítkách hodin.

## 8. Akceptační checklist a vlastníci

### COO — kontrola zadání

- [x] Popsány vytvoření, úprava, aktivace, pauza a historie; doplněno ruční spuštění.
- [x] MVP obsahuje manual/schedule cron, chat prompt a volitelný kontext.
- [x] Vyjmenované povinné UI komponenty.
- [x] Hodinový rozpis, součty, předpoklady a odložený rozsah.
- [x] Bez povinnosti nové tabulky; odlišeno uložení konfigurace od skutečného background běhu.

### CTO / implementátor — před označením implementace za hotovou

- [ ] Ověřit A/B a existující API; první krok 2–3 h z odhadu.
- [ ] Nová definice se uloží pozastavená a přežije reload v deklarovaném režimu.
- [ ] Neplatné pole zabrání uložení a zůstane opravitelně vyplněné.
- [ ] Ruční běh vytvoří jeden skutečný chat výsledek a dohledatelný záznam.
- [ ] Úprava nemění historické výsledky ani zadání již běžícího chatu.
- [ ] Selhání save/pause neukáže falešný nový stav.
- [ ] Přepnutí účtu neodhalí cizí definice ani historii.
- [ ] Chyba AI, nedostupný agent, ztráta spojení a plné localStorage mají srozumitelný výsledek.
- [ ] Varianta A nikdy netvrdí, že cron běží automaticky.
- [ ] Varianta B prokazatelně spustí úlohu se zavřeným browserem, respektuje pauzu, neduplikuje termín a ukazuje skutečné výsledky.
- [ ] Cron náhled, pásmo a chování změny letního času odpovídají runneru.

### Jiří / Steve — prioritizace

Vybrat rozsah realizace podle technického ověření. Tento dokument sám nemění roadmapu, neaktivuje žádnou automatizaci a nezakládá novou tabulku.

## 9. Riziko, eskalace a další krok

**Hlavní riziko:** zaměnit lokálně uložený cron za funkční plánování. Dopad: uživatel očekává výstup, který nikdy nevznikne.

**Eskalační práh:** pokud do konce úvodních 2–3 h technického ověření není potvrzen scheduler, persistence, tenant oprávnění a dostupná historie, neslibovat B; předat Jiřímu/Stevovi rozhodnutí A versus samostatně naceněná infrastruktura. Stejně eskalovat, pokud existující chat vykonává externí akce, které nelze pro automatizaci omezit.

**Další krok:** CTO ověří výše uvedený kontrakt a potvrdí A/B. Poté realizovat vertikální řez „vytvořit → ručně spustit → výsledek“, následně plánování, pokud je dostupné. Jde o doporučený handoff, ne tvrzení, že CTO úkol převzal.

## 10. Evidence a omezení podkladů

Primární zdroj: přímé zadání Jiřího, zpráva 1297 z 2026-09-30. Jde o produktový návrh, nikoli popis ověřeného současného backendu.

Paměť a wiki byly prohledány, ale neposkytly relevantní specifikaci tohoto modulu. CURRENT-WORK.md a sdílený CURRENT-PRIORITIES.md se nepodařilo načíst: dostupný memory_get vrátil MEMORY_PATH_NOT_ALLOWED; obecný read nástroj v tomto běhu není dostupný. Shell pro čtení nebyl použit v souladu s workspace instrukcí. Přímé nové zadání opravňuje zpracování dokumentu bez předstírání ověřené aktivní priority.

Nástroj update_goal není v tomto běhu dostupný; dokončení dokumentu lze oznámit, nikoli tvrdit systémovou změnu aktivního cíle.

