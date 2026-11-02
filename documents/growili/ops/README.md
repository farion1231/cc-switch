# Growili Ops documents — index a pravidla aktuálnosti

> **Aktualizováno:** 2026-09-10  
> Tento soubor určuje, které dokumenty jsou kanonické. Dated reporty jsou historické snapshoty a nesmí přepisovat aktuální stav.

## Aktivní kanonické dokumenty

| Oblast | Kanonický soubor | Stav |
|---|---|---|
| P1 onboarding acceptance | `onboarding-phase0-acceptance-plan.md` | BLOCKED — čeká na schválenou důkazní cestu |
| GMC landing page | `gmc-landing-page-spec.md` | READY FOR REVIEW, bez autorizace k deployi |

Aktivní priority se vždy ověřují také v:

- `/home/growthhacker/.openclaw/workspaces/coo/CURRENT-WORK.md`
- `/home/growthhacker/.openclaw/workspaces/main/documents/strategy/growili/CURRENT-PRIORITIES.md`

Při rozporu má přednost novější přímý pokyn Jiřího, schválené priority a poté kanonický dokument z této tabulky.

## Nahrazené dokumenty

Tyto soubory zůstávají jako krátké přesměrování kvůli existujícím odkazům:

- `gmc-landing-copy.md` → `gmc-landing-page-spec.md`
- `onboarding-alternative-acceptance-protocol.md` → `onboarding-phase0-acceptance-plan.md`
- `onboarding-block-status.md` → `onboarding-phase0-acceptance-plan.md`
- `onboarding-phase0-acceptance-plain.md` → budoucí evidence package
- `onboarding-acceptance-script-2026-07-30.md` → historický testovací návrh

## Historické snapshoty

Následující kategorie slouží jen pro auditní stopu:

- `heartbeat-*.md`
- `daily-*.md`
- `onboarding-queue-check-*.md`
- `onboarding-block-status-*.md`
- `p1-proof-path-decision-brief-*.md`
- `release-readiness-*.md`
- `release-readiness-board-*.md`
- `application-state-analysis-*.md`
- `ops-source-of-truth-map-*-2026-08-02.md`

Snapshot se nikdy průběžně „neaktualizuje“. Nový stav patří do kanonického dokumentu; nový časový report musí mít datum a zdroje.

## Pravidla pro nové Markdowny

1. Každý provozní dokument má `Status`, `Owner`, datum aktualizace a odkaz na aktivní priority.
2. Pro jednu oblast existuje právě jeden kanonický dokument.
3. Nová verze buď aktualizuje kanonický dokument, nebo starý dokument označí `SUPERSEDED` s odkazem.
4. Claim bez runtime důkazu se označí `UNVERIFIED`; nesmí se změnit na veřejný slib.
5. Evidence patří do samostatného adresáře a nesmí obsahovat tokeny, cookies ani zbytečné osobní údaje.
6. Markdown nesmí sám vytvářet schválení, změnu priority ani go-live autorizaci.

## Další úklid

Fyzické přesunutí historických snapshotů do `archive/` je volitelné. Není nutné pro správné rozhodování a mělo by proběhnout až po kontrole externích odkazů.
