# Break-Glass SOP

**Version:** 1.0 (draft)  
**Effective Date:** 2026-07-13  
**Prepared by:** Oto (COO)  
**Status:** Ready draft – čeká na doplnění •#10 CSS Lead Flow SOP před nasazením

## 1. Purpose
Okamžitě definovat repeteebile postupy, když OpenClaw agent nebo infrastruktura přestanou fungovat. Prioritou je **maximální rychlost**, aby operace mohla pokračovat, a **následná analýza** aby se předešlo opakovaným incidentům.

## 2. Scope
- Všichni agenti v workspace `coo` (main, subagents, persistentní agent)
- Všechny openclaw nástroje: `gateway`, `sessions_*`, `cron`, `browser`, `canvas`, `nodes`, `message`
- Všechny procesy vašeho multiagentového prostředí Growili/Amazing Simple Fun

> SOP **neplatí** pro finanční operace a zákaznický dopad bez schválení – viz **Finanční omezení** v OpenClaw Setup SOP

## 3. Definice pojmů
| Termín | Význam |
|------|----------|
| **Break‑Glass** | Interimní oprava pro kritický selhání, přesně dokumentovaná a zpětně auditovaná. |
| **Heartbeat failure** | Agent/subagent neodpovídá na testy ping nebo cron. |
| **Schválení** | Schválení ze superuser role (= root) chráněné heslem OTO-ROOT. |
| **Escalation** | Akce, která musí být oznámena Steveovi (CEO) okamžitě, kdykoli se dotkne zákazníka nebo finančního procesu. |

## 4. Workflow (při selhání)

### 4.1 Identifikace
1. Systém zpozoruje abnormality (žádná Heartbeat > 2 min).
2. Spustí `openclaw gateway status` nebo agentův vlastní health‑check.

### 4.2 Triage (oko – klíčový rozhodovací moment)
| Příznak | Akce | Čas omezení |
|--------|------|----------|
| Agent přestane odpovídat – **vnitřní issue** (např. launcher, memory) | **Jak restartovat** → `gateway restart --dry-run` + `sessions_spawn` (reaktivní). |
| Agent přestane odpovídat – **infrastrukturní issue** (gateway) | **Break‑glass restart** → `gateway restart` (schváleno root). |
| Nástroj změnil chování – **ztráta oprávnění** | **Graceful vytvoření** → `gateway config.patch` s zárukou oprávnění (odhalí chybu a obnoví stav.) |
| Zákaznický dopad – problém s daty nebo platbami | **Skok do SOP** → **okamžitě informujte Stevea** přes `sessions_send` → **schválení** → pak apply.

### 4.3 Obnovení se
1. **Root heslo** = OTO-ROOT, uchováno v SHA256 hashovaném souboru `/etc/openclaw-breakglass.key` (nejvyšší důvěra, bez RootChal).
2. Spusťte „Break‑Glass script“:
   ```bash
   openclaw-break-glass.sh <agentId> <command> [args]
   ```
3. Normalizujte co nejdříve, script protokoluje každý krok do `/var/log/breakglass.log`.
4. Po obnovení: okamžitý přehled V5 `gateway status` test.

### 4.4 Událostní eskalace
- **CSO**: rozpoznán break-glass script – odešle vV5 `
[ALERT] %agentId %symptom %timestamp` do Slack kanálu `#ops-alerts`.
- **COO**: kontroly po obnovení, zkontroluje heartbeat po 2min interval, stav `gateway status` a `openclaw memory status index`.
- **Steve**: jakmile se jakýkoli incident dotkne zákazníka nebo finančního procesu: 1 minutové upozornění, následuje `sessions_send(TARGET: Steve) – Body: krátká zpráva popisující problém, kroky k obnovení, jakékoli kontinuální dopady.

### 4.5 Pasivní (přeživší) protokol
- Všechny akce se zapisují do `
/var/log/breakglass.log` se štítky:
  - `[TIMESTAMP] [AGENT] [SEVERITY] [ACTION] [ARGS] [STATUS]`
- Denní rotační archivace: `logrotate` na `/etc/logrotate.d/breakglass`.

### 4.6 Post-mortem (72h)
1. Rychlé šetření -> `gateway status` před/po, `sessions_history` pro select pt.
2. Identifikovat kořen: hlavní příčinou je často naivní konfigurace.
3. Dokumentovat => aktualizujte tuto SOP (odkaz na incident, změny, nový kontrolní body).
4. Bezpečnostní pásmo: Není PR bez záznamu v `memory/breakglass/<YYYY-MM-DD>.md`.

## 5. Nástrojové příkazy pro rychlou akci

| Situace | Příkaz |
|---------|----------|
| Heartbeat failure – samostatný agent | `openclaw session ${agentId} restore --pid <pid>` |
| Gateway restart (schváleno) | `gateway restart` (schválí `gateway config.patch`) |
| Config revert | `gateway config.patch --revert` (schválí root) |
| Test Heartbeat | `cron run --check` |

### Pravidlo schválení
schválení je vyžadováno pro jakýkoli příkaz, který:
- mění `gateway config`
- restartuje `gateway`
- upravuje prostřecký objem (čte pouze) – neplatí

## 6. Rozdělení zodpovědnosti (RACI)

| Role | Co dělá |
|------|----------| 
| **COO** | Triage incidentů, schvalování root-level (schválí OTO-ROOT), akční endpoint, esdalace. |
| **CSO** | Vedoucí incident záznamů, watchers, upozorňuje alerty, komunikuje s CTO ve vV5. |
| **CTO** | Provádí technické šetření, opravy základní příčiny, korekce config, testuje a vrací.
| **CEO (Steve)** | Rozhodovací na finačnícím, trpí zákaznícký dopad, schvaluje eskalaci. |

## 7. Rozdělení kapacity

| Poplach | Dopad | Okamžitá kapacita |
|--------|--------|-------------------|
| Agent deprásen, interní výpadek | Nízký | Fronta ≤ 1 – \\ aku |
| Nedostupný hele gateway | Vysoký | Fronta ≤ 30 – }}{{precise}} |
| Zákaznícký dopad / finanční | Kritický | OTO-ROOT + eskalace \
 který je připraven okamžitě (max. 30min deployment) |

## 8. Reporting metrika

| Metrika | Cíl | Frekvence |
|--------|------|----------|
| Průměrná doba čistění (MTTR) | ≤ 10 min (interní) / 30 min (zákaznícký dopad) |
| Incident bez eskalace | ≤ 1 / měsíc |
| Záznamy po incidentu (post-mortem) | 100 % (za 3 dny) |
| Přístupy root-level v terenu | Sledováno v \\ nástroje (CO Admin) |

## 9. Testování SOP

1. **Scénář A** (heartbeat failure – internal) – nastavíme simulaci podvadlo pro vybraný agent.
2. **Scénář B** (gateway restart – break‑glass) – provedení `gateway restart --dry-run` + schvalovací token.
3. **Scénář C** (zákaznický dopad – falešného data loss) – spustit simulaci, eskalovat na Stevea, sledovat reakci. Úspnění sítí a detailní evidence jsou uloženy do `
/tmp/breakglass_test.log`.

## 10. Otevřené otázky (k doplnění před nasazením)

[vyplňte:
1) Kde se nachází a jak se chrání heslo OTO-ROOT ? (multi-fator / hardware token) ________________________
2) Jaký je primární auditní log pro root přístupy (SSH, gateway sessions) ? ________________________
3) Které konkrétní incidenty v terenu budou eskalovat přímo jak Steveovi bez kterýchkoli dalších kontrol? (např. smazání zákaznických dat, platby, konfigurace pro více zóny)
]

---

*Tato SOP je připravená pro okamžité nasazení. Před tím, než může být spuštěn jakýkoli break‑glass krok, je potřeba obouvat prioritní schválení s §10 doplněným SFAH, vytvářet incidentní kapitoly a mít v terenu vV5 notification report pomocí post-mortem storage.*

**Odpovědnost:** Tato SOP musí být pravidelně revidována v čtvrtletním cyklu s potenciálně generovanou Skill Workshop pro updating postupů.

#### Připraven/sestaven:
**Oto, COO** (Mateřský
)