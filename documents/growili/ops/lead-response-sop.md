# Lead Response SOP — Growili CSS Leads

**Status:** Draft v1  
**Owner:** COO  
**Created:** 2026-09-16  
**Priority:** High  
**Context:** Growili, malý tým, founder bandwidth limit  
**Cíl:** Zkrátit time-to-first-response na leady  
**Omezení:** Bez nového headcountu, co nejvíce automatizace

---

## Executive Summary

Největší ztráta leadů nastává v **prvních 15 minutách** po příchodu. Tento SOP definuje:
- **Kdo** odpovídá
- **Za jak dlouho**
- **Jaký obsah** zprávy
- **Kdy eskalovat**
- **Co měřit**

Princip: **automatizace nahradí ruční práci, ruční zásah jen u exception path.**

---

## 1. SLA — Response Time Targets

| Scenario | Target Response Time | Owner |
|----------|---------------------|-------|
| Working hours (9–18 CET, Mon–Fri) | **≤15 min** | Auto + human fallback |
| Off-hours (venku pracovní doby) | **≤1 h** next business hour | Auto + morning review |
| High-value lead (hot lane) | **≤5 min** | Human (CMO or designated) |
| Weekend / holiday | Auto only, review next business day | — |

> **Definition of "working hours":** CET/CEST, Jiřího pracovní den. Upravit, pokud je jiný schedule.

---

## 2. Triage — 3 Sekundy na Tag

Každý nový lead dostane **automatický tag** do jedné z 3 větví:

### Branch A — `hot-lead` (fast-lane)
**Trigger (alespoň 2 z 3):**
- Shop URL present + valid
- Monthly spend / revenue range vyplněno
- CSS request již submitted nebo preview existuje

**Akce:**
- Okamžitá auto-zpráva (template: "Přijato, zpracováváme do 5 min")
- Notification na CMO / designated handler
- Human follow-up do 5 min

### Branch B — `needs-info` (warm)
**Trigger:**
- Email present, ale chybí shop URL / spend / podklady

**Akce:**
- Auto-zpráva s **missing-info checklist** (precisely 1 message)
- Interní stav `waiting_for_client`
- Reminder po 24h (automatický)
- Po 72h neaktivity → přesun do `nurture`

### Branch C — `nurture` (low intent)
**Trigger:**
- Low intent signals, poor fit, no budget range

**Akce:**
- Soft follow-up (resource / blog / case study)
- Není eskalace na founder
- Monthly nurture batch (1x měsíc)

---

## 3. Auto-Response Templates

### Template 1 — Instant Confirmation (all leads)
```
Hi [name],

Děkujeme za zájem o Growili CSS. Přijali jsme tvou žádost.

Co se stane dál:
1. Zkontrolujeme tvůj e-shop a podklady (do 15 min)
2. Ozveme se s konkrétním plánem
3. Připojíme CSS a měříme POAS

Team Growili
```

### Template 2 — Missing Info (needs-info branch)
```
Hi [name],

Chceme ti pomoci co nejrychleji. Pro aktivaci CSS potřebujeme:

☐ E-shop URL
☐ Přibližný měsíční reklamní spend
☐ Google Merchant Center přístup (ano/ne)

Odpověz na tento email s doplněnými údaji a ozveme se do 24 h.

Team Growili
```

### Template 3 — Hot Lead Follow-up (human, ≤5 min)
```
Hi [name],

Vidím zájem o CSS. Mám 2 otázky pro rychlé kvalifikování:
1. Jaký měsíční obrat z reklam?
2. Jaký CSS provider používáte nyní?

Odpovědí mi pomůžeš urychlit proces.

[Name]
Growili
```

### Template 4 — Nurture (low intent)
```
Hi [name],

Pro zajímavost: přečti si, jak jeden e-commerce shop zvýšil POAS o 3x pomocí CSS:
[link to case study]

Když budeš připraven, klidně napiš.

Team Growili
```

---

## 4. Manual Response Rules

### Who responds when

| Lead type | Responder | Escalation rule |
|-----------|-----------|-----------------|
| `hot-lead` | CMO or designated handler | If no response in 5 min → notify founder |
| `needs-info` | COO (backoffice) | If no response in 24h → notify CMO |
| `nurture` | No manual response | Batch nurture only |

### Response format rules
- **Always** reference lead ID / source
- **Always** state next step + timeline
- **Never** send generic "we received your request" without next action
- **Always** include owner name + contact

---

## 5. Escalation Path

### Level 1 — Auto
- Instant confirmation (all leads)
- Missing-info reminder (24h, 72h)
- Nurture batch (monthly)

### Level 2 — Human
- Hot lead follow-up (≤5 min)
- Needs-info qualification (≤24h)

### Level 3 — Founder (Jiří)
- High-value lead (>€X monthly spend threshold — TBD)
- Pricing / offer exception
- Blocker na rozhodnutí
- Reputace / obchodní riziko
- VIP onboarding

> **Rule:** Founder vstupuje pouze na Level 3. Vše ostatní jde přes standard flow.

---

## 6. Tools & Process

### Required minimum (V1)
| Tool | Purpose |
|------|---------|
| Form / landing page | Lead capture |
| Email/CRM | Auto-response + tagging |
| Sheet / kanban | Internal tracking (lead ID, branch, status, owner, SLA deadline) |
| Telegram/Slack | Human notifications (hot lead alert) |

### Process steps (per lead)
1. Lead arrives → auto-tag (A/B/C)
2. Auto-response fires immediately
3. Internal tracking record created
4. Human follows up per SLA
5. Status update at each stage change
6. Lead moves to onboarding flow (Phase 2+) when `activation_ready`

---

## 7. Monitoring & KPI

### Daily metrics
- Počet nových leadů (celkem / branch A / B / C)
- Průměrná response time
- % leadů odpovězeno během SLA
- % hot leads → fast-lane activation

### Weekly review
- CPL (cost per lead) — pokud platí
- Lead quality score (manual, COO review)
- Top performing Context Hints / channels
- Bottleneck analysis (kde lead visí)

### Monthly report
- SLA compliance %
- Lead → first value milestone time
- Founder exception count (should be low)
- Recommendations for SOP refinement

---

## 8. Implementation Checklist

### A. Setup (Day 1)
- [ ] Vytvořit auto-response template (confirmation + missing-info)
- [ ] Nastavit tagging pravidla (hot / needs-info / nurture)
- [ ] Připravit interní tracking sheet / kanban
- [ ] Nastavit notification channel (Telegram / Slack) pro hot leads

### B. Team Alignment (Day 1–2)
- [ ] Představit SOP týmu (CMO, COO, CSO)
- [ ] Delegovat: kdo odpovídá na hot lead
- [ ] Delegovat: kdo zpracovvuje needs-info
- [ ] Sepsat exception path (kdy Jiří vstupuje)

### C. Launch (Day 3)
- [ ] Live: auto-response active
- [ ] Live: tagging active
- [ ] Live: human follow-up per SLA
- [ ] Daily check: response time, SLA compliance

### D. Optimize (Week 2–4)
- [ ] Review response time data
- [ ] Adjust SLA thresholds if needed
- [ ] Refine triage rules based on actual lead patterns
- [ ] Add/remove templates based on feedback

---

## 9. Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Auto-response fails | Lead lost | Fallback: manual check every 30 min during working hours |
| Human no response within SLA | Lead goes cold | Escalation: notify CMO after 5 min (hot) / 24h (needs-info) |
| Wrong triage tag | Wrong branch, wasted effort | Weekly review: audit 10% of tags for accuracy |
| Founder overload (Level 3 too frequent) | Bandwidth wasted | Tighten exception criteria; COO filters before escalation |
| No tracking tool | Invisible pipeline | V1 = Google Sheet minimum; upgrade when volume justifies |

---

## 10. Next Steps

1. **Schválením SOP** — Jiří potvrdí draft
2. **Setup V1** — auto-response + tagging + tracking sheet (1 den)
3. **Team alignment** — delegace + exception path (1 den)
4. **Live launch** — aktivace (den 3)
5. **First weekly review** — data + adjustments (týden 1)

---

## Appendix A: Success Criteria

- **Lead → first response:** ≤15 min (working hours), ≤1 h (off-hours)
- **SLA compliance:** ≥90%
- **Hot lead → human follow-up:** ≤5 min
- **Founder exceptions:** ≤10% of total leads
- **Lead drop-off (no response):** ≤5%

## Appendix B: Integration Points

Tento SOP se napojuje na:
- **CSS Lead Onboarding Flow** (fáze 1–2: capture + qualification)
- **CSS Lead Onboarding Flow checklist** (sekce C: Automation)
- **P1-ONBOARDING-ACCEPTANCE** (po Phase 0 go/no-go)
- **ChatGPT Ads SEOP** (lead capture → auto-triage)

---

*Document is a living draft. Iterace na základě reálných dat z prvních leadů.*