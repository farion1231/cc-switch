# CURRENT-WORK — COO Oto

## Aktuální stav ověřený 2026-10-01 — číst před historickým zadáním

Jiří přímo zadal nové ověření a aktualizaci vlastních MD podkladů. Výsledek: `documents/growili/reports/2026-10-01-ops-claims-recheck.md`.

- Ověřen čistý main `c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a`, shodný s živým origin/main při kontrole.
- Demo formulář volá `submit-demo-request`; hidden attribution pole nejsou povinná na klientu ani serveru. Backend ukládá do demo_requests, migrace existuje, notifikace je implementovaná. Nezadávat tyto opravy znovu.
- Onboarding ukládá completion flag. Webhook retry worker i plánovací migrace existují.
- Dokumentační výstupy NIGHT-2026-09-28-03 a -07 existují. Jejich staré technické závěry jsou překonané; dokončený dokument není produkční acceptance.
- DEC-2026-08-08-01 a DEC-2026-08-09-01 připouštějí manuální acceptance s auditní stopou. Browser artefakty nejsou jedinou schválenou cestou. Finální uzavření celé P1 zde není potvrzeno.
- Produkční deployment/E2E, aktuální FIO/CRM připojení, živé fronty, SLA a pricing rozhodnutí: NEOVĚŘENO. Neoznačovat jako neexistující, rozbité nebo aktuálně blokované jen podle starých zpráv.
- CSO-2026-07-31-01 je ve sdíleném inboxu proposed, ne nově schválená kritická práce.
- Další práce pouze podle aktuálního přímého zadání / schválených priorit. Tato oprava sama nespouští implementaci, deployment ani změnu zákaznického procesu.

## Historické zadání z 2026-09-28 — není aktuální technická diagnóza

Níže uvedené Key evidence to verify byly hypotézy tehdejšího zadání; některé jsou dnes vyvráceny. Při rozporu použít ověřený stav výše, nikoli starý snapshot.

**ACTIVE:** `P1-ONBOARDING-ACCEPTANCE` + support for `P2-ACQUISITION-TO-LEAD` by direct Jiří assignment  
**TASK ID:** `NIGHT-2026-09-28-03`  
**DIRECT APPROVAL:** Jiří, 2026-09-28  
**REPOSITORY SNAPSHOT:** `/home/growthhacker/projects/amazing-simple-fun`, branch `cso/demo-lead-capture-20260927`, audited HEAD `cd019ac847748d9c0d93242f41ef30b3da893ea8`  
**OUTPUT:** `documents/growili/ops/2026-09-28-onboarding-lead-operational-acceptance.md`

## Assignment

Produce one onboarding-to-lead operational acceptance and handoff contract. Determine whether the current signup/onboarding and demo-lead flows are operationally acceptable, identify explicit no-go conditions, and define the temporary human handoff required until product routing is implemented.

## Required scope

1. Record the exact branch/commit snapshot and dirty-tree warning.
2. Create two source-backed flow diagrams:
   - signup → onboarding gate → shop creation → integration selection → dashboard;
   - landing page → demo form → Edge Function → database → notification → human owner.
3. Build an acceptance matrix with at least 15 cases covering happy paths, validation/server errors, domain collision, first-import failure, skipped/selected integrations, notification failure/throttling, unassigned leads, support escalation, and recovery.
4. Classify every case as `Verified`, `Partial`, `Unsupported`, or `Blocked from source-only verification`.
5. Give separate `GO`, `CONDITIONAL GO`, or `NO-GO` conclusions for onboarding acceptance and acquisition-to-lead.
6. Define an interim operating procedure: monitoring location, owner, first response, follow-up, closure, notification failure, and escalation.
7. Create a decision register for owner, response SLA, escalation channel, lifecycle definitions, and attribution contract.
8. Produce a prioritized blocker list without proposing or implementing code.

## Allowed sources

Read-only:

- onboarding guard, onboarding hook, wizard, integration/completion steps, onboarding page and layout;
- `DemoRequestForm.tsx` and its landing-page call sites;
- `submit-demo-request`, `notify-admin`, and the demo-request migration;
- lead service/hooks/admin leads page;
- onboarding tests;
- `docs/guides/onboarding.md`, `docs/modules/leady.md`, and repository roadmap;
- Git status/diff metadata only.

## Key evidence to verify

- onboarding completion is recorded before integrations provide value;
- integration selections are not persisted or connected;
- users can skip integrations and still see success;
- demo-form hidden required fields prevent normal submission;
- persisted demo leads have no application operations queue;
- notification throttling can suppress distinct lead emails;
- support/error states and current user-flow documentation are incomplete.

## Success

- exactly one Markdown document at the output path;
- at least 15 evidence-backed acceptance cases;
- both priority areas receive an unambiguous verdict;
- the interim operating process identifies every unresolved owner/SLA decision;
- deployment state and live behavior remain explicitly unverified.

## Hard boundaries

Documentation only. No code, tests, migrations, deployment, Git, browser login, production, credentials, live lead data, customer communication, or process changes with external impact. Write only the specified internal document and the cron-generated daily review under this workspace.

## Approved follow-up after primary output is verified complete

**FOLLOW-UP TASK:** `NIGHT-2026-09-28-07`  
**FOLLOW-UP OUTPUT:** `documents/growili/ops/2026-09-28-demo-lead-interim-operating-sop.md`

Create a proposed interim manual operating SOP using only mechanisms actually evidenced in the repository. Include intake detection options, owner-decision placeholders, response and escalation options, reconciliation with the existing admin lead view, notification-failure contingency, daily audit checklist, and explicit assumptions. Mark the SOP `PROPOSED — NOT ACTIVE`.

Follow-up succeeds only when it does not invent dashboards, queues, Slack alerts, SMS, owners, SLAs, monitoring, or implemented integrations, and every unresolved operating choice is left for Jiří's approval.
