# CSS Lead Onboarding Flow — Growili

**Status:** Draft v1  
**Owner:** COO  
**Created:** 2026-09-16  
**Priority:** High  
**Context:** Growili, malý tým, founder bandwidth limit  
**Cíl:** Zkrátit time-to-first-value  
**Omezení:** Bez nového headcountu, co nejvíce automatizace

---

## CEO Brief — Doporučení

Navrhni onboarding jako **5fázový, automatizovaný, low-touch flow** s jediným cílem:  
**dovést lead co nejrychleji k prvnímu měřitelnému výsledku** = „shop je připojený, data tečou, další krok je jasný“.

Neoptimalizuj na „perfektní onboarding“, ale na:
1. **Rychlou kvalifikaci**
2. **Rychlé připojení účtu / podkladů**
3. **Automatické odstranění mrtvých míst**
4. **Jasný owner + SLA**
5. **První value milestone do 24–48 h**

### Cíle flow
- Zkrátit time-to-first-response
- Zkrátit time-to-data-connected
- Zkrátit time-to-first-recommendation / first action
- Founder handholding na minimum

### Princip návrhu
Každý nový CSS lead musí projít jednou z **3 větví**:

| Větev | Popis | Akce |
|-------|-------|------|
| **Hot lead** | Připraven jednat hned | Fast-lane activation |
| **Warm but incomplete** | Má zájem, chybí podklady / přístup / rozhodnutí | Self-serve intake + missing-info follow-up |
| **Low intent / not ready** | Zatím neaktivovat founder time | Nurture / soft follow-up |

> To je zásadní. Největší ztráta času u malého týmu bývá, že se všem leadům věnuje stejná energie.

---

## Navržený onboarding flow

### Fáze 1 — Capture + Instant Qualification
**Cíl:** Do 5 minut od leadu rozhodnout, kam spadá.

**Co se má stát automaticky:**
- Lead se uloží do jednoho pipeline stavu: `new_css_lead`
- Automaticky se pošle:
  - Potvrzení přijetí
  - Krátké vysvětlení dalšího kroku
  - Link na onboarding form / checklist
- Systém vytvoří interní score / tagy:
  - Shop URL
  - Market / country
  - Platforma
  - Měsíční spend / order volume
  - Má / nemá GMC, feed, přístupy
  - Urgentnost

**Výstup:** Lead se automaticky rozdělí do:
- `qualified-fast-lane`
- `needs-info`
- `nurture`

---

### Fáze 2 — Self-Serve Intake
**Cíl:** Získat vše nutné bez ručního chase.

**Lead vyplní / dodá (minimum nutné pro aktivaci):**
- E-shop URL
- Země / cílový trh
- Kontakt
- Používaná platforma
- Stav GMC / Merchant Center
- Stav feedu
- Reklamní účet / přístupy
- Očekávání / cíl
- Měsíční spend / obrat range

**Automatizace po odeslání:**
- Validace povinných polí
- Automatické zjištění chybějících podkladů
- Systém vygeneruje **personalized next-step email / message**
- Interně vytvoří checklist stavu onboarding readiness

> **Doporučení:** Drž intake krátký — **max 3–5 minut vyplnění**. Všechno ostatní až ve druhém kroku po potvrzeném zájmu.

---

### Fáze 3 — Readiness Check + Branch Logic
**Cíl:** Nezastavit se na nekompletním leadu a zároveň neblokovat hot lead.

#### Branch A — Ready (`activation_ready`)
Pokud má: validní shop + relevantní fit + dostupné podklady / přístupy + základní intent

**Automaticky:**
- Odejde onboarding confirmation
- Vznikne interní task checklist
- Nastaví se SLA na first review
- Pošle se „co bude následovat do 24 h“

#### Branch B — Missing Info (`waiting_for_client`)
Pokud chybí část podkladů:
- Systém pošle přesně jednu zprávu s checklistem chybějících věcí
- Interně stav `waiting_for_client`
- Po X hodinách / dnech automatický reminder
- Po neaktivitě přesun do nurture

#### Branch C — Poor Fit / Low Intent (`nurture`)
- Neeskalovat founderovi
- Poslat soft follow-up nebo resource
- Zařadit do pozdějšího nurture

> Tímto odstraníš největší bottleneck: **ruční rozhodování nad každým leadem od nuly**.

---

### Fáze 4 — Activation Sprint
**Cíl:** První skutečná hodnota do 24–48 h od complete intake.

**Sekvence (bez čekání):**
1. Interní kontrola readiness
2. Připojení / ověření účtů a podkladů
3. Kontrola feed / GMC / CSS prerequisites
4. Identifikace prvního quick win
5. Výstup pro klienta:
   - Co je hotové
   - Co chybí
   - Co je první doporučený krok
   - Kdy bude další checkpoint

**First Value Milestone (definice pro CSS):**
- Účet/podklady jsou validně připravené **NEBO**
- Klient dostane první jasný akční audit / readiness verdict

> I když ještě není plná aktivace, klient musí rychle vidět: **„něco se stalo, jsme posunutí, další krok je jasný.“**

---

### Fáze 5 — Handoff do Operating Rhythm
**Cíl:** Onboarding nekončí tichým přechodem do chaosu.

**Po aktivaci:**
- Lead přejde do stavu `live_css_onboarding` nebo `active`
- Odejde shrnutí:
  - Co bylo dokončeno
  - Co se bude dít dál
  - Kdo je owner
  - Kdy je další checkpoint
- Interně vznikne standardizovaný handoff record

**Nutné minimum handoffu:**
- Stav přístupů
- Readiness status
- Known blockers
- Next action
- Owner
- SLA / checkpoint date

---

## Doporučený Operating Model

### 1. Jediný Canonical Pipeline
```
New → Auto-qualified → Needs info → Activation ready → Waiting for client → In activation → Active
                                                    ↘ Nurture → Closed / poor fit
```

### 2. Jediný Onboarding Checklist (interní)
- Fit check
- Obchodní readiness
- Technická readiness
- Access readiness
- Activation next step

### 3. Jedna Klientská Truth Message
Každý lead má po každé změně dostat jednoduchou odpověď:
- Kde jsme
- Co chybí
- Co uděláme my
- Co má udělat klient
- Do kdy

### 4. Founder Jen na Exception Path
Founder vstupuje jen když:
- High-value lead
- Pricing / offer exception
- Blocker na rozhodnutí
- Reputační / obchodní riziko
- VIP onboarding

**Všechno ostatní má projet standard flow.**

---

## Největší Rizika
1. **Příliš dlouhý intake form** → drop-off
2. **Ruční dohledávání chybějících podkladů** → pomalý onboarding
3. **Nejasný owner dalšího kroku** → lead visí
4. **Žádný first-value milestone** → klient má pocit, že se nic neděje
5. **Founder je fallback pro všechno** → flow se škáluje špatně

---

## Praktické Doporučení: V1 Architektura
Začni **simple v1**, ne enterprise workflow.

| Komponenta | Popis |
|------------|-------|
| 1 vstupní formulář | Minimum polí pro kvalifikaci |
| 1 automatická confirmační zpráva | Po odeslání formuláře |
| 1 interní scoring / tagging vrstva | Auto-řiď větve |
| 1 missing-info follow-up šablona | Pro Branch B |
| 1 activation checklist | Krok za krokem |
| 1 handoff template | Pro fázi 5 |

---

## Checklist — Implementační Kroky

### A. Intake
- [ ] Definovat povinná pole pro CSS lead intake
- [ ] Zkrátit formulář na minimum nutné pro kvalifikaci
- [ ] Nastavit automatickou potvrzovací zprávu po odeslání
- [ ] Přidat tagy: fit / readiness / urgency / source

### B. Qualification
- [ ] Zavést 3 větve: fast-lane / needs-info / nurture
- [ ] Definovat jasná pravidla pro přesun mezi větvemi
- [ ] Nastavit poor-fit pravidla, aby nebral founder time

### C. Automation
- [ ] Automaticky posílat missing-info checklist
- [ ] Nastavit reminder při čekání na klienta
- [ ] Nastavit interní stavové změny bez ručního přepisování
- [ ] Vygenerovat jednotné interní onboarding tasky

### D. Activation
- [ ] Definovat „first value milestone“
- [ ] Zapsat activation checklist krok za krokem
- [ ] Nastavit SLA: first review / first response / next step
- [ ] Připravit klientské status message šablony

### E. Handoff
- [ ] Zavést jednotný handoff template
- [ ] U každého leadu evidovat ownera, blocker, next action
- [ ] Po aktivaci automaticky posílat shrnutí dalšího postupu

### F. Founder Protection
- [ ] Sepsat exception path, kdy vstupuje Jiří
- [ ] Vše ostatní držet mimo founder inbox
- [ ] Měřit, kolik leadů skutečně vyžaduje manuální zásah

---

## 3 KPI pro Sledování Od Začátku
- **Lead → first response**
- **Lead → complete intake**
- **Lead → first value milestone**

---

## Next Steps (pro další iteraci)
1. Přetavit do konkrétního **SOP** s detailními kroky
2. Definovat **pipeline stavy** v CRM / toolu
3. Napsat **automatické zprávy** pro každou fázi
4. Vytvořit **activation checklist** jako šablonu
5. Sepsat **handoff template**
6. Nastavit **exception path pravidla** pro Jiřího

---

*Poznámka: Tento dokument je living draft. Iterujeme podle reálných dat z prvních leadů.*