Aktuální stav ověřený 2026-10-01 — číst před historickým zadáním

Jiří přímo zadal nové ověření a aktualizaci vlastních MD podkladů. Výsledek: `documents/growili/reports/2026-10-01-ops-claims-recheck.md`.

- Ověřen čistý main `c29ffaffc16d86f5233fbb9f0f77fc8211dfdf6a`, shodný s živým origin/main při kontrole.
- Demo formulář volá `submit-demo-request`; hidden attribution pole nejsou povinná na klientu ani serveru. Backend ukládá do demo_requests, migrace existuje, notifikace je implementovaná. Nezadávat tyto opravy znovu.
- Onboarding ukládá completion flag. Webhook retry worker i plánovací migrace existují.
- Dokumentační výstupy NIGHT-2026-09-28-03 a -07 existují. Jejich staré technické závěry jsou překonané; dokončený dokument není produkční acceptance.
- DEC-2026-08-08-01 a DEC-2026-08-09-01 připouštějí manuální acceptance s auditní stopou. Browser artefakty nejsou jedinou schválenou cestou. Finální uzavření celé P1 zde není potvrzeno.
- Produkční deployment/E2E, aktuální FIO/CRM připojení, živé fronty, SLA a pricing rozhodnutí: NEOVĚŘENO. Neoznačovat jako neexistující, rozbité nebo aktuálně blokované jen podle starých zpráv.
- CSO-2026-07-31-01 je ve sdíleném inboxu proposed, ne nově schválená kritická práce.
- Další práce pouze podle aktuálního přímého zadání / schválených priorit. Tato oprava sama nespouští implementaci, deployment ani změnu zákaznického procesu.