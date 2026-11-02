# Audit větví — 2026-09-30

Přímé zadání Jiřího: fetch a doporučení pro merge/ukončení větví. Bez merge, push, mazání, stashování či změn pracovních souborů repozitáře.

Repo: `/home/growthhacker/projects/amazing-simple-fun`
Ověřený origin/main i lokální main: `a5baee751`; hlavní checkout čistý.
`git fetch origin` úspěšný. Následný projektový cleanup dry-run také provedl fetch/prune tracking refů; žádné vzdálené větve nebyly smazány.

## Doporučené rozhodnutí

### Ukončit bez merge

- `fix/seo-contentplan-missing-import` — předek origin/main, čistý worktree.
- `fix/shop-dropdown-first-load` — předek origin/main, čistý worktree.
- `integrate/tier2` — předek origin/main, bez připojeného worktree.

Záložní `backup/local-wip-ai-agents-20260930` je také předek main, ale zálohu doporučuji ponechat do vyřešení necommitované AI Agents práce a stashů.
`origin/claude/bulk-product-rating-automation-91f04a` je předek main; konečné ukončení závisí na potvrzení vlastníka, že nemá další lokální práci.

### Malí kandidáti

- `origin/docs/doc-drift-guard`, `d46010bb9`: jediný nový markdown plán, 177 řádků, stage proposal. `git apply --check` vůči aktuálnímu čistému main prošel. Lze začlenit jako NÁVRH, nikoli označit jeho implementaci za hotovou. Faktická tvrzení o produkční DB v dokumentu nebyla tímto auditem ověřena.
- PR #44, `origin/claude/beautiful-ramanujan-4hiwy5`, `2311691ab`: pouze CLAUDE.md, +12/-3; apply-check prošel. Doporučuje přesun plného typecheck do CI. Draft, historické typecheck/test selhaly, build skipped. Neoznačovat za zelený release; sladit s novým scoped typecheck na main a ověřit CI. Stejný patch se objevuje v integračních větvích, nezačleňovat vícekrát.
- `d046dff2c` z `integrate/coo-safe-sweep-20260929`: přímé importy v 9 zdrojových souborech. Samostatný commit projde apply-check na main. Kandidát na izolovaný cherry-pick + cílený lint/build, nikoli doporučení okamžitého merge celé větve. Její worktree má navíc necommitovanou Labelizer/XML práci.

### Neuzavírat: necommitovaná práce

Přestože HEAD je v main:
- `feat/ai-agents-sidebar-cto-20260930`: změněné initializeNavigation.ts a aiAgentsService.ts.
- `feat/web-photos-checklist-20260930`: překlady, nové web-photos soubory a stránky.
- `fix/ai-agent-f1-review-recovered`: chat/stream/proxy/rate-limit/migrace a nové testy.
- `feat/hermes-scanner-step1-20260929`: router, i18n, scanner, Vite, nový server a testy.

Dále dirty `integrate/coo-safe-sweep-20260929`: CS/EN labelizer, XMLDashboard a nový LabelizerHeroVisual.
Celkem 32 stash záznamů. Jejich obsah nebyl auditován; nic nemažte hromadně.

### Nemergovat celé v současném stavu

- PR #39: GitHub `OPEN`, `CONFLICTING`, `DIRTY`, head `36158ee68`; staré 4 CI joby selhaly. Branch delta proti společnému předku nyní 167 souborů, nikoli pouze původní profilová oprava.
- `feat/ai-agents-theme-polish-20260929`: textové konflikty AgentChat.tsx a AgentList.tsx; musí zachovat novější streaming/progressive reveal.
- `integrate/branch-sweep-20260929`: konflikty plán/roadmapa; 2 ze 3 nemerge commitů patch-ekvivalentní main.
- `feat/competitor-ads-bigquery-live`: konflikty backendu, mapování, testů a CSS stránky/překladů.
- `feat/growili-about-recovered`: konflikty ve čtyřech About komponentách.
- `feat/meta-ad-library-edge-functions`: konflikt plánu; search potomek navíc konflikt edge funkce.
- `fix/growthlink-rpc-tenant-guard`: konflikt CHANGELOG; bezpečnostní SQL vyžaduje zvláštní věcnou revizi, merge kódu není deploy DB.
- `fix/main-shell-boot2`: konflikty hooku, rout a překladových indexů.
- `fix/merchant-center-real-upload`: konflikt FileUploadPage.test.tsx.
- `lovable-sync-1790746493`: konflikt XMLDashboard; obsahuje generované DB typy a migraci, ne jen grafiku.
- Staré široké větve jako PR #34/#40/#41/#42/#43: stovky odlišných historických commitů. Nelze schválit hromadným merge; přenášet pouze doložené chybějící části.

## Další patch-ekvivalentní větve

`git cherry` neukazuje nové nemerge patche pro:
`feat/ai-agents-sidebar-module`, `origin/feat/merchant-api-v1-product-writes`,
`origin/fix/css-paid-workflow-null-fields`, `origin/fix/css-pricing-route`,
`origin/fix/google-ads-setup-race-guards`, `origin/fix/header-academy-nav-size`.
Jsou kandidáti na úklid po kontrole výsledného chování a vlastníka; patch-ekvivalence není totéž co ancestry ani důkaz nepřítomnosti pozdějšího revertu.

## Rozsah a omezení ověření

Inventura všech lokálních/remote refů, historie, ahead/behind, patch-ekvivalence, worktree status a stash list. Detailní textová simulace 13 kandidátů přes tříargumentový `git merge-tree`; žádný skutečný merge.
Git 2.34.1 nepodporuje `merge-tree --write-tree` (`fatal: unknown rev --write-tree`); počáteční neúspěchy této varianty NEJSOU konflikty. Konflikty uvedené výše pocházejí až z podporované tříargumentové varianty.
`graft map` nedostupný: `graft: command not found`.
Projektový `cleanup-verified-branches.sh` dry-run skončil exit 141 bez stdout/stderr, neposkytl seznam k úklidu. Nic s --apply nebylo spuštěno.
Žádný nový build, lint, test ani UI QA neběžel; bezkonfliktní aplikace patchů není ověření funkčnosti. GitHub vrácené main CI běhy jsou historické na jiných SHA, nikoli důkaz CI pro a5baee751.
Aktuální workspace priority nebyly dostupným souborovým read nástrojem načteny; audit je autorizován přímým zadáním Jiřího, nikoli odvozenou prioritou.

## Další krok

Ukončit dva čisté hotfix worktrees a integrate/tier2 po souhlasu vlastníka. Sloučit dokumentační návrh jako proposal. Importní commit připravit izolovaně na čerstvém main a ověřit. Necommitované práce nejprve nechat vlastníky dokončit/zachovat; nepřekrývat se s aktivními úpravami.
