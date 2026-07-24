# Changelog

All notable changes to CC Switch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.18.0] - 2026-07-21

Development since v3.17.0 is headlined by Grok Build joining as the eighth managed app — full provider switching, proxy takeover on its own route namespace, MCP/Skills/prompts sync, a curated preset list, and a Grok Official entry with official-login import (schema v14/v15) — and by xAI Grok account sign-in over an OAuth device flow for Claude Code, Claude Desktop, and Codex, including a strict-gateway compatibility layer that lets codex 0.142+ drive a Grok subscription over native Responses. A usage-accounting repair wave fixes the v3.17.0 fork/sub-agent double count with a one-time automatic rebuild (schema v16) plus a manual rebuild action, makes proxy usage logging idempotent, and stops the usage page freezing during large imports. Diagnostics mature: logs persist across restarts under size rotation, every log egress redacts secrets, and renderer crashes are captured to disk behind an error boundary with a reload screen. The Codex conversion layer gets four correctness fixes — tool schemas normalized to object type, reasoning attached forward across turns, streamed tool-call identity and order preserved, and parser-required catalog fields backfilled so codex 0.144.5+ starts — while managed-OAuth providers are now reliably flagged as routing-required and Windows provider switches no longer flash a console window or freeze the UI. Rounded out by Kimi K3 presets and pricing, corrected OpenClaw preset costs, SudoCode.us restored beside SudoCode.chat, sponsor-grouped preset ordering, first-run tray language detection, and permanently deletable default Skill repositories.

**Stats**: 52 commits | 217 files changed | +21,452 insertions | -6,285 deletions

### Added

- **Grok Build Becomes the Eighth Managed App**: xAI's Grok CLI ("Grok Build", live config `~/.grok/config.toml`) is now a first-class managed app alongside Claude Code, Claude Desktop, Codex, Gemini CLI, OpenCode, OpenClaw, and Hermes: full provider add/import/switch (with a "restart Grok Build to apply" toast), app visibility and config-directory override settings, session manager and usage dashboard coverage, prompts with first-launch auto-import, deep-link (`ccswitch://`) provider import, and proxy takeover with its own `/grokbuild/v1/responses` route namespace, failover queue, and per-app proxy settings row — it reuses the Codex Responses forwarding path but never shares Codex's provider namespace or circuit-breaker state. MCP servers sync bidirectionally into Grok's `[mcp_servers]` table with the dialect differences handled (Grok infers transport from `command`/`url` and uses `headers`, so exports strip the explicit `type` and rename `http_headers` to `headers`, while imports infer the transport back), and Skills gain a Grok Build enable flag. Instead of borrowing the Codex preset list (an early cut leaked China-official direct providers and Codex default models into the Grok form), Grok Build ships its own curated preset module: aggregators and relays that actually carry Grok models, default model normalized to `grok-4.5` (`x-ai/grok-4.5` on namespaced routers), with no `cn_official` category. The tools panel installs Grok via the official xAI installer (`x.ai/cli/install.sh` / `install.ps1`) with npm `@xai-official/grok` as fallback; installs detected as native (launcher in `~/.grok/bin` or `$GROK_BIN_DIR`, real target under `~/.grok/downloads/grok-*`) self-update via `grok update`, while npm installs keep npm-anchored updates — the self-update is gated on positive native detection so it can never mutate a different install. Localized in all four locales (zh/en/ja/zh-TW). (#5453)
- **Grok Official Provider With Official-Login Import**: A "Grok Official" entry lets Grok Build fall back to the CLI's own built-in xAI OAuth login: selecting it hides the connection fields and writes an empty `~/.grok/config.toml`, and CC Switch never stores or touches those credentials. Live-config reads, backups, and official-state writes now use syntax-only TOML validation, so an official-login (empty) config round-trips instead of failing; importing the live config while Grok is in official login state yields "Grok Official set as current" instead of an error, matching Codex behavior. Official-state detection is deliberately wired only into the manual import command — the startup auto-importer still rejects official-state configs, so a deleted Grok Official entry is never resurrected on launch. Proxy takeover of an official-login Grok config is skipped automatically and rejected with a clear message on the manual path, consistent with the existing ban on proxying official providers.
- **Sign In With Your xAI Grok Account (OAuth) in Claude Code and Claude Desktop**: New "xAI (Grok)" presets for Claude Code and Claude Desktop authenticate with an OAuth device-code login instead of an API key, routing requests through the local proxy which transforms Anthropic Messages traffic to the xAI Responses API and injects the access token per request (default model `grok-4.5` on every role; the Claude Desktop preset maps the branded `claude-*` role IDs to `grok-4.5` upstream so it passes Desktop's third-party model validation). A new xAI section in Settings → OAuth Auth Center handles device-code login (user code with copy button, verification link, waiting/cancel/retry), multiple accounts with a default-account picker, per-account removal, and re-auth badges — accounts whose refresh token was revoked stay visible as "expired" instead of disappearing, and auth status refetches every 15 s so server-side revocation surfaces on its own. The integration is pinned closed: upstream is always `https://api.x.ai/v1/responses` in Responses format regardless of what the editable endpoint/format fields say, OAuth endpoints are resolved via OIDC discovery but validated to `auth.x.ai` over https only, refresh tokens live in `~/.cc-switch/xai_oauth_auth.json` (`0600` on Unix; access tokens are memory-only), and OAuth error bodies are never embedded in error messages or logs. `grok-4.5` is also seeded into the built-in pricing table ($2 input / $6 output / $0.50 cache-read per 1M tokens), fixing the $0 cost the dashboard would otherwise record; existing databases pick the row up automatically on next launch. Localized in all four locales (zh/en/ja/zh-TW). See the upgrade notes for the client-identity disclosure.
- **xAI (Grok) Codex Presets: OAuth Managed and Native-Responses API Key**: Codex gains two ways to talk to xAI directly. The "xAI (Grok) OAuth" managed preset drives Codex against a Grok subscription through the local proxy — the form hides the key/endpoint/format fields and shows the account picker, "fetch models" uses the signed-in account, and the provider is pinned to native Responses with the base URL and per-request token enforced by the proxy (edited base URLs or formats are ignored, so the managed route cannot be redirected). Because codex 0.142+ emits ChatGPT-backend-private shapes that xAI's strict parser rejects (`type:"namespace"` tool declarations → 422, plus private fields like `prompt_cache_retention`, `safety_identifier`, `external_web_access`, the `additional_tools` carrier, and grok-4.5-unsupported sampling knobs), the OAuth route gets a compatibility layer on the native passthrough: namespace tools are flattened to top-level function tools (using the same sha256-truncated naming as the Chat path) and restored to namespaced form in both streaming and non-streaming responses, and unsupported fields are stripped — all deterministic field-removal/structural lifts, never semantic rewrites, so prompt-cache prefixes stay stable. The layer is gated exclusively on the xAI OAuth provider type: no other provider's traffic is touched, including the second new preset, a plain "xAI (Grok)" API-key preset (native Responses against `api.x.ai/v1` with a 500K-context `grok-4.5` catalog entry) that does not receive these xAI-specific compatibility transforms — API-key users on codex 0.142+ can still hit xAI's strict parser, and the OAuth preset is the fully-compatible path. xAI OAuth token failures are classified non-retryable so failover never silently moves a conversation onto a different Grok account.
- **Renderer Crash Capture With On-Disk Reports and a Reload Screen**: A React error boundary now wraps the UI (including the database-recovery screen), so a renderer crash shows a "Something went wrong in the interface" card with a Reload button instead of a blank white window, and global `error`/`unhandledrejection` handlers persist renderer errors to disk via the new `log:default` capability — previously a JS crash left zero on-disk evidence. Everything logged from the frontend passes two redaction layers: a structured serializer that redacts by sensitive property name (normalized so `tokens`/`apiKeys`/`credentials` variants all match, hiding the entire value including nested objects) and by value shape (token prefixes, PEM headers, high-entropy opaque strings), then a single text egress (`redactFrontendLogText`) whose ordered regex chain covers URL query values and credentials, auth headers and schemes, and named secret containers even in double-encoded JSON. JSON that arrives as a string is re-parsed and redacted structurally, and oversized structured input is dropped entirely rather than truncated — a truncated JSON string would fall back to the weaker text regexes and could leak. The settings toggles were also relabeled to say what they actually control: "Application Diagnostic Logs" (cc-switch.log) versus the proxy's "Record Request Usage" (the stats database, which never was a text log). Localized in all four locales (zh/en/ja/zh-TW).
- **"Rebuild Codex Usage" Maintenance Action**: The usage dashboard's maintenance section gains a "Rebuild Codex Usage" button that backs up the database, wipes only `codex_session`-sourced detail rows, their `_codex_session` daily rollups, and the Codex sync cursors, then re-imports every rollout file from scratch with the corrected parser — the recovery path for databases already inflated by the fork double-count bug below, and the retry path for deferred fork files whose parent log has since been restored. The manual rebuild fails hard when the pre-rebuild backup cannot be written (unlike the automatic migration variant, which only warns, since blocking startup on an unwritable backup directory would leave the app unable to launch after upgrade), holds the session-sync lock across the whole backup → reset → reimport sequence so the 60-second background sync cannot interleave with the wipe, and always sends exactly one frontend refresh notification on completion — including when the reimport returns zero rows or fails after the reset — so the dashboard never keeps showing pre-reset numbers (`finish_codex_rebuild` in `commands/usage.rs`, pinned by two regression tests). Cursor deletion matches rollout paths purely by shape (`rollout-{uuid}` filename under a `sessions`/`archived_sessions` segment), so cursors recorded under an old `CODEX_HOME` are still cleaned. Localized in all four locales (zh/en/ja/zh-TW).
- **Session Import Observability: Deferred Files and Suspected Duplicates**: Session sync results now report `filesScanned`, `deferredFiles` — fork rollouts whose parent log is missing or whose parent markers conflict, held back without writing a cursor so a later sync or manual rebuild retries them, instead of being imported on a guess — and `suspectedDuplicates`, backed by a post-insert probe that checks each imported row for a pre-existing same-fingerprint row (via the `idx_request_logs_dedup_lookup_expr` expression index) and logs a warning per hit, so any future regression of the double-count bug announces itself in the logs instead of silently inflating totals.
- **Kimi K3 in Presets and Built-In Pricing**: The Kimi open-platform presets for Codex, Hermes, OpenClaw, and OpenCode gain the Kimi K3 model (1M context window), appended after K2.7 Code so existing default-model behavior is unchanged, and the built-in pricing table gains `kimi-k3` at the official list price ($3 input / $15 output / $0.30 cache-read per 1M tokens) plus a bare `k3` alias — the Kimi For Coding subscription reports its model under the short id `k3`, which would otherwise match no pricing row (same precedent as the existing `hunyuan-hy3`/`hy3` pair). Existing databases pick both rows up automatically on next launch without touching user-edited pricing.
- **SudoCode.us Restored Alongside SudoCode.chat**: The two unrelated companies sharing the "SudoCode" name are now separate presets: the sponsor is renamed "SudoCode.chat", and the legacy "SudoCode.us" (which an earlier release had replaced in place) returns with its original endpoints and models, its own icon, and a distinct Hermes slug so both can coexist in the additive `~/.hermes/config.yaml`. Counting the new Grok Build preset list, SudoCode.chat now ships in seven apps and SudoCode.us in all eight.

### Changed

- **Diagnostic Logs Persist Across Restarts, Are Size-Rotated, and Never Record Secrets**: `cc-switch.log` is no longer wiped on every startup — the log that explained a crash used to be gone by the time the app reopened — and instead rotates at 20 MB with 4 archives kept (~100 MB cap, versus a single 1 GB file before); `crash.log`, previously unbounded, rotates at 5 MB with 2 archives, with the size-check/rotate/append sequence under one lock so concurrent panics cannot lose an archive. Because persistent logs make verbatim secrets a real exposure (users attach these files to public issues), every backend log egress was scrubbed in the same change: upstream URLs are logged with userinfo, query, and fragment stripped (origin-only when no known secret is available to scrub, since a credential could be embedded in the path), request and response bodies are never logged — replaced by byte counts, a short hash, or a safe classification (`sse`/`html`/`json-like`/`binary-or-encoded`/…) that preserves the transform-debugging signal without the content — response headers go through an allowlist (unlisted headers log name-only), the exact secret values in use (API key, access token) are scrubbed out of any logged URL that carries them, and MCP custom-field values are omitted. The log plugin also registers earlier (so updater/startup failures are diagnosable), the persisted log level applies as soon as the database opens and fails closed to Info, and the "Enable Diagnostic Logs" switch now gates frontend-originated log writes too. Pre-upgrade log files are not retroactively scrubbed — see upgrade notes.
- **Preset Picker Groups Sponsors and Alphabetizes the Rest**: The picker's default ordering now has four tiers — official first, then the prime partner, then sponsor presets (in the same order as the README sponsor table, which the preset files were physically reordered to match), then all remaining presets sorted alphabetically by display name instead of file order. An entry matching multiple tiers lands only in the earliest, so nothing is listed twice.
- **Preset "Get API Key" Links Updated to Current Referral URLs**: The key-application links on the RunAPI, ClaudeCN, ZetaAPI, and APINebula presets now open each provider's current registration/referral page (ClaudeCN also moved domains, claudecn.top → claudecn.ai). Referral tags remain confined to these links and the README — website links and API endpoints stay untouched.

### Fixed

- **Codex Fork/Sub-Agent Rollouts No Longer Import Replayed Parent History**: Fixes the v3.17.0 usage inflation where forking a Codex task or spawning a sub-agent in copy mode re-counted the parent conversation's token history as new usage — reports showed single days jumping by billions of tokens, duplicated parent/child rows that were byte-identical, and empty forks carrying usage they never consumed. Fork and sub-agent rollout files begin by replaying the parent thread's history, and the previous parser located the takeover boundary heuristically (first `thread_settings_applied` event, object-shaped `subagent` source markers): the boundary landed too early when the parent's own settings changes appeared in the replay, and was missed entirely for the current string-shaped source format, importing the whole replayed parent history. The parser now identifies a fork only by explicit parent identity — `forked_from_id` on the child's `session_meta` or `source.subagent.thread_spawn.parent_thread_id`, with a conflict between the two deferring the file — anchors each thread's identity to the rollout filename UUID, loads the parent rollout's own pre-fork token-count sequence, and strips the child's replayed prefix by aligning token signatures against it, so replayed events only restore the cumulative baseline and are never inserted as rows. Sub-agent logs that carry no replayed history are now counted as live usage, fixing the opposite-direction undercount where real sub-agent consumption was skipped as suspected replay. (#5335, #5433, #5381)
- **Proxy Usage Logging Is Now Idempotent With Stable Response-Scoped Keys**: When a terminal usage event carried no message id — the norm for Codex `/responses` traffic through the local proxy — the dedup key fell back to a random UUID, so every retry or replay of the same upstream response minted a fresh key and `INSERT OR REPLACE` stacked a new row each time; one reporter's database held the same usage combination 2,078 times. The parser now derives the key from the response envelope itself — the Codex `response.completed` event's `response.id` (`response.created` ids are discarded), Chat Completions `chatcmpl` ids, and the Gemini `responseId` — scoped as `session:{app_type}:{provider_id}:{id}` so the same response replayed against a different provider during failover still bills once per provider without cross-provider collisions (Claude keeps the bare `session:{id}` shape so proxy rows continue to converge with session-log imports). When no envelope id exists at all, the fallback is a deterministic SHA-256 over the response's usage semantics rather than a random UUID — an identical replay must collide into the same key for dedup to work — and the final database write is a guarded insert-if-absent within the dedup window instead of an unconditional REPLACE. (#5496)
- **Usage Page No Longer Freezes During Large Session Imports**: Opening the usage page while a big import ran could lock up the UI: every inserted row fired a refresh notification, each notification made the frontend re-run all ~10 usage queries, and those queries contended with the importer for the single database connection while it parsed rollout files tens of megabytes large line by line — on duplicate-inflated databases the row storm compounded all three. Session sync now notifies the frontend once per completed sync pass instead of once per row, all session importers are serialized behind a single-flight lock (a manual "sync now" queues behind the running pass instead of racing it), the blocking parse work runs on a dedicated blocking thread so it no longer starves the async runtime driving the UI's commands, and the 60-second background tick skips missed runs instead of bursting to catch up.
- **Codex 0.144.5+ No Longer Fails to Start on CC Switch-Generated Model Catalogs**: codex ≥ 0.144.5 parses external model catalogs strictly and rejects the whole file when an entry is missing `supports_reasoning_summaries` — so both the Codex CLI and desktop app failed to launch, and deleting the generated catalog didn't help because any provider save regenerated it the same way. The root cause is that CC Switch clones its catalog template from the machine-shared `models_cache.json`, whose field set is whatever the last-writing codex process produced — a coexisting older codex build kept rewriting the cache without the field the newer parser requires. Generated catalogs now backfill parser-required fields from the bundled static template, only when absent (dynamic values always win), and deliberately do not backfill optional capability fields whose "missing = parser default" semantics must survive.
- **Windows: No More Console Flash or UI Freeze When Switching Providers**: Switching providers or toggling takeover on Windows flashed a transient console window and froze the UI for up to ~2 seconds. Three causes, three fixes: the `codex debug models --bundled` probe launches `codex.cmd` through `cmd.exe`, which in a GUI-subsystem app spawns its own console — the child process is now created with `CREATE_NO_WINDOW`; the model-catalog template was regenerated on every switch — it is now cached process-wide after the first successful load (failures stay retryable so a bad first probe cannot poison the cache), so the Codex CLI starts at most once per app run; and `switch_provider` was a synchronous command running on the main thread — it is now async with the real work on a blocking thread, still serialized by the per-app switch lock. The freeze fix benefits all platforms; the console-flash fix is Windows-specific.
- **Codex Tool Schemas With Null, Missing, or Union Parameter Types Are Accepted by Strict Upstreams**: Built-in Codex tools such as `codex_app__automation_update` declare `parameters: null` (or `type: null`), which strict OpenAI-compatible upstreams like DeepSeek reject with a 400 for the entire request, killing tool-using sessions routed through the proxy. The Responses→Chat bridge now normalizes every tool's parameters to a `type:"object"` schema — null or missing parameters (including the nested-form missing case) become `{"type":"object","properties":{}}`, a non-object `type` (including `type: null`) is coerced to `"object"` in place, and top-level `oneOf` union schemas get a root `type:"object"` added while their branches are preserved untouched. The same object-type guarantee was extended to the Codex→Anthropic tool path's `input_schema`. Existing `properties`/`required` are never dropped. (#4706, #5315; issues #4705, #4783)
- **Reasoning Models Keep Their Thinking Across Multi-Turn Codex Chat Conversations**: With a reasoning model (e.g. kimi-k2-thinking) behind the proxy's Responses→Chat bridge, multi-turn history mangled the thinking: each turn's `reasoning` item was glued onto the tail of the _previous_ assistant message, leaving the following assistant turn with no `reasoning_content` — models would visibly break off mid-conversation. Responses semantics place reasoning _before_ the message it belongs to, so the bridge now attaches reasoning forward to the assistant message or tool call that follows it; genuine trailing reasoning back-attaches only at the true tail (end of input, or a turn boundary such as an incoming user message — where it was previously silently discarded), appending to any embedded reasoning already present, and pending reasoning is always consumed at boundaries so it can never leak across a user turn into a later assistant message. (#5508)
- **Streamed Parallel Tool Calls Keep Their IDs and Their Order**: Two bugs in the Chat→Responses streaming bridge corrupted parallel tool calls from upstreams that split identity across chunks: a continuation delta carrying an empty `id` overwrote the real `call_id` (the Codex client then saw `call_id:""` and couldn't match tool results to calls), and tool calls were emitted the moment they individually became ready, so a later index whose name arrived early could jump ahead of an earlier one — reordering parallel calls. Empty ids are now ignored, and emission goes through a consecutive-index gate that releases tool calls strictly in Chat `index` order, waiting on any not-yet-identified earlier index; no fake call id is ever synthesized mid-stream (only as a last resort at stream finalization, which also skips nameless calls defensively and still emits sparse indexes). (#5310)
- **Managed-OAuth Providers Are Reliably Flagged as Needing Local Routing**: The "needs routing" badge and switch-time warning were derived from the provider's API format, which is the wrong signal for managed-OAuth providers (Copilot, Codex OAuth, xAI) whose credential is injected by the proxy regardless of upstream format — a managed provider on a native format got no warning and failed silently without takeover. Routing need is now decided by a single shared predicate: official providers never need routing, managed-OAuth providers always do, and format-based rules apply only to the remaining cases per app. The switch-time gate also checks the right readiness signal per app: per-app takeover for most apps (the old gate looked only at a global proxy-running flag, missing the case where the proxy runs but the current app is not taken over), while Claude Desktop keeps watching the proxy process itself — the backend's takeover status has no Claude Desktop field, so a uniform per-app gate would have left Desktop warning forever. Claude Desktop provider forms now force proxy mode and lock the model-mapping toggle for every managed-OAuth type, not just xAI. Localized in all four locales (zh/en/ja/zh-TW).
- **Tool Updates Work When Node Lives in nvm/fnm/mise Under a GUI Launch**: Anchored npm update and repair commands invoked npm by absolute path, but npm's launcher resolves `node` via its `#!/usr/bin/env node` shebang against PATH — and a GUI-launched app inherits only the system PATH, not the user's version-manager directories, so updates silently failed for tools installed via nvm/fnm/mise. Every anchored npm invocation now prefixes PATH with npm's own sibling `bin` directory, so both npm and its shebang resolve to the same Node install; the Codex self-repair (uninstall + reinstall) path is covered too.
- **Deleted Default Skill Repositories Stay Deleted**: Default Skill repositories were re-seeded on every startup by a "supplement missing defaults" pass, so a default repo the user deleted silently returned on the next launch. Seeding is now one-time per database, tracked by a settings flag; databases that already contain repositories at upgrade time get the flag set without any re-seeding, so existing selections are untouched. (#5356)
- **First-Run Tray Language Follows the System Locale**: Before any language was chosen in settings, the tray menu was hardcoded to Simplified Chinese even on English/Japanese/Traditional-Chinese systems where the main UI correctly followed the OS locale — tray and UI disagreed until the user switched language once. The tray now derives its first-run language from the OS locale with the same precedence rules as the frontend (including `zh-TW`/`zh-HK`/`zh-Hant` → Traditional Chinese); an explicitly chosen language always wins, and unreadable locales fall back to Chinese as before. (#4355)
- **Failed Live-Config Imports Show the Real Error and Refresh the List**: Every failed "import from live config" produced an empty error toast, because Tauri's `invoke` rejects with the backend's error _string_ while the handler read `.message` off it; the actual backend message is now shown (with a localized generic fallback), and the provider list is refreshed even on failure so side effects committed before the error are visible immediately.
- **OpenClaw Preset Model Costs Corrected to Official List Prices**: Fifteen OpenClaw preset entries carried cost values in the wrong unit or unconverted currency — the `cost` field is USD per million tokens, but e.g. `glm-5.1` was listed at `0.001/0.001` (≈1000× undervalued, so its usage showed near-zero cost) while `deepseek-v4-pro` carried unconverted CNY values (overvalued). All entries now carry official list prices in $/M; subscription-plan and free-tier endpoints deliberately show list prices too, so plan users see the standard value of their usage. Providers created from these presets going forward get the corrected values; previously added providers keep the config they were created with.
- **AiHubMix Icon on the Codex Preset**: The Codex app's AiHubMix preset was the only one missing its brand icon fields and rendered a generic icon; it now matches the other apps.
- **Two Missing Locale Keys Backfilled in All Four Locales**: The Codex "needs routing because it uses Anthropic Messages format" toast rendered its reason fragment in Chinese inside an otherwise-localized sentence because `proxyReasonAnthropicMessages` existed in no locale file, and the provider-form key-status loading label had shipped only as a hardcoded default since April; both now exist in zh/en/ja/zh-TW.

### Docs

- **Codex + Claude Local Routing Guide (Three Languages)**: New guide in Chinese, English, and Japanese on pointing Codex at a Claude-family `/v1/messages` gateway via the native Anthropic Messages upstream format introduced in v3.17.0, with screenshots; the v3.17.0 release notes now link to it.
- **Claude Code via Codex Providers Guide (Chinese)**: New Chinese guide on using Responses-speaking providers (a gateway API key, or a ChatGPT subscription's Codex service) from Claude Code: Claude Code keeps talking Anthropic Messages to the local `/v1/messages` route, and the proxy converts each request to the upstream's Responses protocol. With screenshots.
- **README Sponsor Updates**: SubRouter added as a sponsor across the four README languages; the pinned Kimi sponsor copy refreshed to K3 with banners served from the Moonshot CDN; RunAPI benefit copy refreshed and sponsor rows reordered to match the in-app preset order.

### Internal

- **Backend CI Now Covers Linux, Windows, and macOS**: Backend checks previously ran only on Ubuntu, so Windows/macOS-gated code paths were never compiled or tested in CI — the matrix now spans all three platforms, with the platform-gated tests repaired to pass everywhere (TOML literal-string path escapes, stale anchored-update expectations, and skill tests honoring the test home-directory override). No shipped behavior changes. (#5138)

### Upgrade notes

- Upgrading from v3.17.0 runs three schema migrations (v13 → v16): v14 rebuilds the `proxy_config` table to admit Grok Build (existing per-app proxy settings are carried over and a `grokbuild` row is added), v15 adds Grok Build enablement columns to the MCP-server and Skills tables, and v16 triggers the Codex usage rebuild described next.
- Schema v16 performs a one-time automatic rebuild of Codex session usage on first launch after upgrading: the database is backed up under `backups/`, `codex_session` data and cursors are reset, and the normal startup sync re-imports everything with the corrected parser. Expect seconds for typical data; the heaviest dataset measured (1,801 rollout files / 1.5 GB) took about 65 seconds. Later launches are incremental as before.
- The rebuild recomputes usage from the rollout JSONL files, so history whose source log was already deleted cannot be reconstructed.
- Fork files whose parent rollout is missing are deferred and reported instead of guessed; restoring the parent log and running “Rebuild Codex Usage” imports them later.
- Historical proxy-source duplicate rows are permanently retained — the migration rebuilds only session-sourced data, and no cleanup pass for past proxy inflation exists; the idempotent logger prevents new duplicates from this point on.
- The xAI Grok OAuth integration reuses the official Grok CLI's public OAuth client identity and scopes (`client_id b1a00492-073a-47ea-816f-4c329264a828`, scope including `grok-cli:access`) rather than a CC Switch-registered application. xAI may not support this use, and it could lead to account restriction or suspension — use at your own risk. The feature is entirely opt-in; nothing changes unless you add an xAI provider. On first login it creates `~/.cc-switch/xai_oauth_auth.json` (refresh tokens only, `0600` on Unix; access tokens are held in memory) and contacts `auth.x.ai` and `api.x.ai` through the configured outbound proxy, with no local callback port.
- Diagnostic logs are no longer cleared at startup and now persist across restarts (up to ~100 MB of rotated runtime logs plus ~15 MB of crash logs). Log files written by earlier versions are not retroactively scrubbed and may contain API keys, tokens, or URLs with credentials — review pre-upgrade logs before sharing them publicly.
- Installing or reinstalling Grok Build now prefers the official xAI installer, fetching `x.ai/cli/install.sh` (or `install.ps1` on Windows) at install time, with npm as fallback; existing npm installs keep updating via npm.
- New built-in pricing rows (`grok-4.5`, `kimi-k3`, `k3`) are appended automatically on next launch via insert-if-absent; user-edited pricing rows are never overwritten.

## [3.17.0] - 2026-07-13

Development since v3.16.5 is headlined by project profiles — named snapshots of provider/MCP/Skills/prompt state, switchable per scope from a new header switcher or the tray (schema v12) — and a deep Codex push: official ChatGPT-subscription sessions can now route through the local proxy takeover with a corrected client identity, gpt-5.6 lands across context-window injection and Sol/Terra/Luna pricing with 1.25× cache-write rates, and a native Anthropic Messages upstream joins the Codex format options. A proxy-correctness wave makes the Responses↔Anthropic bridges fail closed and round-trip reasoning/tool results losslessly, strengthens prompt-cache breakpoint injection, and fixes cache-write accounting across historical token semantics (schema v13); a config.toml hardening batch stops deleted MCP servers from resurrecting, fails MCP sync closed on unparseable files, extends switch-time common-config autosync to Codex, and moves the common-config merge to backend toml_edit. Usage tooling gains Zhipu team-plan quota queries, Codex sub-agent and free-plan accounting, and transient-failure retry, while Kimi For Coding's 256K window finally takes effect — rounded out by a Codex default-model form field, renamed-session titles, OpenCode form and live-sync fixes, and preset updates (SudoCode sponsorship, LongCat-2.0, GPT-5.6 defaults, Hunyuan Hy3 pricing).

**Stats**: 69 commits | 172 files changed | +21,067 insertions | -2,464 deletions

### Added

- **Project Profiles for One-Click, Snapshot-Based Config Switching**: You can now save the current provider, MCP, Skills, and prompt state as a named "project" and re-apply it in one click from a new header switcher or the tray Projects submenu, so moving between per-project setups no longer means toggling each dimension by hand; it covers Claude Code, Claude Desktop, and Codex (Claude Desktop's only cc-switch-managed dimension is its provider, so its snapshots capture just that and leave the other dimensions untouched on apply). Projects are global shared entities, but capture and switching are per scope — Claude Code, Claude Desktop, and Codex each keep their own `current_profile_id_<scope>` marker and only touch their own payload slots, so picking a project on the Codex tab never disturbs the Claude config (slots use `Option` to distinguish "never captured" from "captured empty", preventing cross-side accidental disable). Applying is best-effort: `ProfileService::apply` (in `services/profile.rs`) reuses the existing switch primitives — provider switch, then minimal MCP/Skills toggle diffs, then prompt enable — and any dangling reference or per-item failure becomes a warning instead of rolling back. Switching first autosaves the leaving project's current state for the active scope, so a project always holds the configuration you last left it in, which is why there is no manual "update snapshot from current state" button. Before applying, proxy takeover is unconditionally disabled per app in the scope, the frontend invalidates takeover status, and the proxy server is stopped when the switch leaves no takeovers active so Claude Desktop's local-route master toggle reads off. Backed by a new `profiles` table introduced by the v11→v12 schema migration, wired into `commands/profile.rs`, `database/dao/profiles.rs`, the tray, and `components/profiles/`, localized in all four locales (zh/en/ja/zh-TW), with integration tests covering roundtrip, dangling refs, bidirectional autosave, and proxy-takeover teardown.
- **Setting to Show or Hide the Project Switcher on the Main Page**: The header project switcher can now be turned off from a new "Show project switcher" toggle under the Homepage Display section of Settings, for users who don't use projects and prefer a cleaner header. It defaults to on (`show_profile_switcher` / `showProfileSwitcher`) so existing users keep the current behavior, and disabling it only hides the header control on every tab — projects and the tray Projects submenu remain fully usable. Localized in all four locales.
- **Route Codex Official ChatGPT Sessions Through Proxy Takeover**: CC Switch can now route a Codex session authenticated with a ChatGPT subscription (native OAuth or API-key login, no stored API key) through the local proxy, so official-account traffic gets the same proxy routing, format conversion, and usage accounting that third-party providers already receive. The built-in `codex-official` "OpenAI Official" seed is restored if it was deleted (idempotent `ensure_codex_official_provider` command wired into the add-provider flow) and is now selectable during takeover from the provider panel and tray, with the provider card badge reading "Official Account Routing" while routing and "Codex Sign-in" otherwise; only that fixed seed qualifies (`is_codex_official_provider` / `official_provider_supports_proxy_takeover`, mirrored in the frontend `supportsOfficialProxyTakeover`), so copied UUID-based official entries stay blocked and still show "No Routing Support". Instead of writing an `OPENAI_API_KEY = PROXY_MANAGED` placeholder into `auth.json`, the official route projects a dedicated `cc-switch-official` `model_provider` into `config.toml` with `requires_openai_auth = true`, `wire_api = "responses"`, `supports_websockets = false`, and `base_url` pointing at the local proxy (`apply_codex_official_proxy_route`), so Codex forwards its own ChatGPT authorization to the proxy's `/responses` endpoint and the `codex-official` row never stores a credential (its `auth` stays `{}`). The forwarder passes the client's `Authorization` header through unchanged to the fixed first-party endpoint (`CHATGPT_CODEX_BASE_URL`, now shared by the Codex and Claude adapters), rejects a missing header or a stale `PROXY_MANAGED` placeholder with an actionable error (finish the ChatGPT login / restart Codex or start a new session), and treats official `401`/`403` and auth errors as non-retryable so failover never silently moves the conversation to another account or pollutes the circuit breaker. The native login is never overwritten — takeover preserves OAuth or API-key material into the backup (falling back to live `auth.json` when the backup is missing), config transforms in `codex_config.rs`/`services/proxy.rs` now fail closed instead of swallowing errors, and stale managed placeholders left in other provider tables are cleaned; the `preserveCodexOfficialAuthOnSwitch` setting copy was updated to clarify that takeover routing always preserves the official login and the toggle now only governs direct (non-routing) third-party switches.
- **GPT-5.6 Context Window for Claude Code Codex Takeover**: When Claude Code is routed to a ChatGPT Codex (Codex OAuth) backend via proxy takeover, CC Switch now injects `CLAUDE_CODE_MAX_CONTEXT_TOKENS` and `CLAUDE_CODE_AUTO_COMPACT_WINDOW` set to `372000` into the effective live `settings.json`, so Claude Code stops assuming its default 200K window for the unrecognized GPT model id and auto-compacts before the upstream rejects an oversized prompt. 372000 matches the ChatGPT Codex catalog window for gpt-5.6 — the catalog lists a ~353K effective budget, not the 1.05M API spec — and Claude Code's built-in output reserve and compact buffer then keep the actual compact trigger below that budget. The defaults are gated on every configured model env key (`ANTHROPIC_MODEL`, the haiku/sonnet/opus/fable defaults, and `CLAUDE_CODE_SUBAGENT_MODEL`) starting with `gpt-5.6`: gpt-5.5's upstream catalog oscillates between 272K and 372K and must not inherit them; any non-gpt-5.6 or mixed mapping is skipped (and any value a legacy shared snippet dragged in is stripped), while an explicit user value always wins. The Codex OAuth presets for both Claude Code and Claude Desktop bump their default routes to the gpt-5.6 family (haiku → `gpt-5.6-luna`, main/sonnet/opus → `gpt-5.6`), the custom Codex `config.toml` template default model moves to gpt-5.6, and the Claude Code preset ships both context env keys pinned at 372000 (see the corresponding Changed entry on preset-pinned context windows). On switch-away backfill the injected values are stripped as the mirror-inverse of the injection conditions so program defaults never harden into per-provider explicit values, and both keys are held out of the shared Claude common-config snippet (`services/provider/mod.rs` strip list plus a guard in `live.rs`) because context limits must follow the actual upstream model; six Rust integration tests cover injection, user overrides, legacy-snippet strip, non-gpt5.6 skip, and the backfill round-trip. (openai/codex#31860)
- **GPT-5.6 Sol/Terra/Luna Pricing With 1.25x Cache-Write Rate**: The usage dashboard now costs GPT-5.6 traffic using seeded pricing for the three tiers — Sol at 5 / 30 / 0.50, Terra at 2.50 / 15 / 0.25, and Luna at 1 / 6 / 0.10 USD per million input / output / cache-read tokens — cross-checked against OpenAI's pricing page and OpenRouter. Unlike GPT-5.5 and earlier, whose cache writes are seeded at zero, the 5.6 family bills prompt-cache writes at 1.25x the uncached input rate, so cache-write is seeded at 6.25 / 3.125 / 1.25 for Sol / Terra / Luna. The seed set also adds the bare `gpt-5.6` id as the official Sol alias plus effort-suffix variants (`-low`, `-medium`, `-high`, `-xhigh`, `-minimal`), all priced at Sol rates and mirroring the gpt-5.5 accounting shape. Rows are applied through the seed+repair dual-write path in `src-tauri/src/database/schema.rs`: a `repair_current_model_pricing` branch corrects the earlier zero cache-write seeds for the three tiers in existing databases, matching only rows still holding the exact old input/output/cache-read/cache-write values so user-customized prices survive, and no `SCHEMA_VERSION` bump is required.
- **Native Anthropic Messages Protocol as Codex Upstream**: Codex providers can now target a gateway that only exposes the native Anthropic Messages protocol (`/v1/messages`) via a new `anthropic` value on the upstream-format selector in the Codex form; the local proxy performs bidirectional Responses↔Anthropic request, response, and streaming conversion (new `transform_codex_anthropic` / `streaming_codex_anthropic` modules). The form adds an auth-field selector — `ANTHROPIC_AUTH_TOKEN` sends `Authorization: Bearer` (default) and `ANTHROPIC_API_KEY` sends `x-api-key`, mutually exclusive — an optional Claude Code client-impersonation toggle (`impersonateClaudeCode`, off unless explicitly enabled, spoofing User-Agent / `anthropic-beta` / `x-app` / system-prompt first line and dropping Codex/OpenAI fingerprint headers), and a per-provider `maxOutputTokens` override (Codex omits `model_max_output_tokens`, so without it the path falls back to a conservative 8192 that can truncate long or thinking-heavy replies). The bridge injects standard 5-minute ephemeral `cache_control` so system/tools/history are cached rather than resent at full price, defers stripping the `[1m]` long-context marker until after catalog matching and re-emits the `context-1m-2025-08-07` beta header, gates extended thinking on the trailing turn only so history-resending sessions don't permanently lose it after the first tool call, disables native `web_search` for this profile, drops `tool_choice` when no tools survive filtering, treats a base URL already ending in `/v1/messages` as a full endpoint, and reports truncated streams as incomplete/failed instead of completed. (#5071)
- **Default Model Field for Codex Provider Form**: The Codex provider form now exposes the top-level `model` key of `config.toml` as an editable field, so users can point an existing provider at a newly released model (e.g. `gpt-5.6`) without waiting for a preset update — preset changes only affect newly added providers, while existing ones keep their saved TOML. The field syncs bidirectionally with the TOML editor (mirroring the base-URL pattern), suggests models from the mapping catalog unioned with the provider's `/models` endpoint, offers a one-click "add to mapping" action when the value falls outside a non-empty catalog, and is hidden for official providers. Save-time catalog sync now backfills the first mapping row into the top-level `model` only when the field was left empty, so an explicit value always wins over the implicit row-0 fallback; model names and `base_url` are written with TOML basic-string escaping and control characters are stripped from field input, since `/models` ids are remote data and unescaped interpolation could inject `config.toml` lines such as a forged `[mcp_servers.*]`. The strict model-line matcher (`src/utils/providerConfigUtils.ts`) recognizes escaped output, empty strings, and single-quoted literals to keep extract/set round-trips stable, and the fetched model list is invalidated (with an in-flight sequence guard) whenever request identity changes — base URL, full-URL toggle, API key, or custom User-Agent — so the dropdown never shows a previous provider's models.
- **Switch-Time Common-Config Autosync Now Covers Codex**: When switching away from a Codex provider with `common_config_enabled`, the service now re-extracts the shareable portion of its live `config.toml` into the stored common-config snippet — the same live-to-snippet sync that previously ran only for Claude — so preferences you tweak directly in the running Codex config propagate to every opted-in provider and a removed key isn't silently re-injected on the next switch. Codex was gated out before because its TOML extractor leaked provider-specific and injected content; `extract_codex_common_config` now strips top-level `model`, `model_provider`, `base_url`, and `wire_api`, the entire `[model_providers]` table, `mcp_servers` and the legacy `[mcp.servers]` form, the top-level `experimental_bearer_token` fallback (which would otherwise leak the API key into the shared snippet), the `model_catalog_json` catalog pointer, and the injected `web_search = "disabled"` sentinel, while keeping a user-set `web_search` value as a shareable preference. The sync in `services/provider/mod.rs` is scoped strictly to Claude and Codex (Gemini is still excluded), skipped when the snippet was explicitly cleared, and all failures are warn-only and never block the switch; the autosync-before-strip ordering also self-heals stale snippet values previously baked into provider snapshots, since the re-extracted snippet matches the live values and the value-match strip removes them on the same switch.
- **Claude Subagent Model Configuration**: Claude providers can now pin a dedicated sub-agent model through a new "Subagent" row in the provider form, writing the `CLAUDE_CODE_SUBAGENT_MODEL` env key so Claude Code's spawned sub-agents run on a chosen (typically cheaper or faster) model. The row supports the `[1M]` marker but has no display-name field — it renders a "Not shown in /model" placeholder since the sub-agent model never appears in the `/model` menu — and the "quick set" button now fills it alongside the other tiers. On the proxy takeover path (`services/proxy.rs`), `CLAUDE_CODE_SUBAGENT_MODEL` is added to `CLAUDE_MODEL_OVERRIDE_ENV_KEYS` so it is written when a provider configures it and stale values are cleared when a provider omits it, and the model mapper (`proxy/model_mapper.rs`) gains a `subagent_model` field that passes a request through unchanged when its model (comparing with the 1M suffix stripped) matches the configured sub-agent model, instead of collapsing it to the default-model fallback. The key is also excluded from the shared Claude common-config snippet in `services/provider/mod.rs` so it can't leak across providers, and `modelRoleSubagent` / `modelNoDisplayName` labels were added in all four locales (zh/en/ja/zh-TW). (#4830)
- **1M Context Checkbox on the Claude Fallback Model Field**: The Claude provider form's fallback model field (`ANTHROPIC_MODEL`, the "default/fallback model") now carries the same 1M checkbox that the role-specific Sonnet/Opus/Fable rows already had, so a fallback model backed by a 1M-token window can declare it instead of silently being treated as 200K. Checking the box appends the `[1M]` marker to the fallback model id and unchecking strips it, reusing the shared `hasClaudeOneMMarker` / `setClaudeOneMMarker` / `stripClaudeOneMMarker` helpers, and the checkbox is a no-op when the model field is empty. The marker is written in the documented uppercase form (`[1M]`), which Claude Code parses case-insensitively. This is a frontend-only change to `ClaudeFormFields.tsx`. (#5124, fixes #3679)
- **Zhipu (智谱) Team Plan Quota Query Support**: Added quota-query support for Zhipu's team plan (团队套餐), which the personal-plan query could not reach: it hits the same `open.bigmodel.cn` quota endpoint with `?type=2` and requires two extra request headers, `bigmodel-organization` and `bigmodel-project`, so `detect_provider` cannot tell it apart from the personal plan by base URL alone. The usage-script modal gains a "Zhipu GLM Team" template with organization-ID and project-ID inputs (preserved when switching templates), and the backend routes on an explicit `coding_plan_provider == "zhipu_team"` identifier threaded through the usage script, IPC command, and background query path; the new `query_zhipu_team` in `services/coding_plan.rs` pins the CN site, appends `?type=2`, and shares response parsing (`zhipu_quota_from_body` / `parse_zhipu_token_tiers`) with the personal plan. All three of API key + organization ID + project ID are required — missing any one returns a NotFound that prompts the user to complete them (identifier match is case-insensitive) — and new copy was added across all four locales (zh/en/ja/zh-TW). (#5128)
- **OpenCode Provider Form Gains Headers and Per-Model Token-Limit Editors**: The OpenCode provider form now exposes two previously unreachable config fields as structured editors: a Headers section for the provider's `options.headers` (arbitrary HTTP headers like `HTTP-Referer` or `X-Title` sent with provider requests) with add/remove rows and case-insensitive duplicate-name rejection that reverts the input on conflict, and per-model Token Limits (Context and Output number inputs writing `model.limit.context` / `model.limit.output`, where clearing the field removes it and negative or non-finite values are rejected while valid ones are truncated to integers). The Extra Options block was reworked into a collapsible section (its i18n label stays "Extra Options") that auto-expands when options already exist. Both the headers and extra-options editors switched their placeholder draft keys to colon-bearing prefixes (`draft-header:` / `draft-option:`) — a code comment notes a colon is invalid in an HTTP field name so a draft key cannot collide with a legitimate header, and the same technique now guards option keys — and those drafts are stripped on save, fixing a bug where the old filter discarded any real option key literally starting with `option-` (the former placeholder prefix). New i18n keys were added across all four locales (zh/en/ja/zh-TW), and `aria-label`s were added for the header add/remove and model-details toggle controls in `OpenCodeFormFields.tsx`; the headers state and draft-stripping logic live in `useOpencodeFormState.ts`. (#2907)
- **Tencent Hunyuan Hy3 Model Pricing**: Seeded pricing for Tencent's Hunyuan Hy3 (released 2026-07-06, 256K context) so its usage is billed instead of showing $0. The rows are added to `seed_model_pricing` in `schema.rs` under both the `hunyuan-hy3` and `hy3` ids, since the upstream billing id isn't yet confirmed and one of the two should match logged usage. Prices follow Tencent's launch-day list rate (CNY 1 / 4 / 0.25 per Mtok input / output / cache-hit), converted at 1 USD ≈ 7.14 to $0.14 / $0.56 / $0.035, with `cache_write` left at `0` because no write rate is published. Hy3 actually uses input-length tiered billing (<16K / 16-32K / ≥32K) but the single-price table holds the lowest tier, so long-context requests are under-billed until this is revisited against the official billing page. Being a brand-new model it is seed-only, applied on next app start via `ensure_model_pricing_seeded` with no `SCHEMA_VERSION` bump.

### Changed

- **Codex Chat Prompt-Cache Routing**: Added provider-aware `prompt_cache_key` injection on the Codex Responses→Chat Completions bridge. Kimi Coding and OpenAI official endpoints are enabled automatically, Kimi's preset opts in explicitly, and unknown OpenAI-compatible gateways remain off to avoid strict-schema 400s. The key is taken from an explicit client value or a real client-provided session ID—never a generated per-request UUID—with an advanced Auto/Enabled/Disabled override in all four UI locales.
- **Provider Connectivity Configuration Simplified**: Removed the obsolete per-provider `testConfig` override (timeout, retry count, and degraded-latency threshold) from provider forms, frontend/backend provider metadata, and reachability-check merging. The lightweight `base_url` probe now always uses the single global connectivity-check configuration, while automatic failover remains driven exclusively by its separate proxy timeout and circuit-breaker settings. Also renamed the remaining settings UI and API modules from model-test terminology to connectivity-check terminology, removed the retired first-use confirmation flag, and deleted stale model/prompt/error copy across all four locales.
- **Codex Image Capabilities Are Inferred Without a User Toggle**: Generated Codex model catalogs now advertise only models in CC Switch's confirmed, exact text-only registry as `input_modalities = ["text"]`; GPT, aliases, new suffix variants, and all unknown models fail open to `["text", "image"]`. The rectifier's “Text-Only Model Preflight” switch continues to control only proactive proxy request-body replacement and does not change Codex's catalog declaration. Live catalog reverse-import also collapses inferred modalities instead of persisting them as hidden row overrides, so a later registry correction or a model's multimodal upgrade takes effect automatically; only declarations that differ from inference survive a DB-missing/import round-trip.
- **Context Window Values Pinned in Presets Instead of Editable Form Fields**: The `Codex` (ChatGPT/GPT-5.6) and `Kimi For Coding` presets no longer surface the "Max Context Tokens" and "Auto Compact Window" inputs in the provider form; the numbers are now hardcoded directly in the preset env (`372000`/`372000` for Codex, `262144`/`262144` for Kimi For Coding) instead of being exposed as editable `templateValues`. This drops two fields most users never need to touch while keeping both env keys present on purpose: since Claude Code resolves the compact window as `min(model window, value)`, setting it equal to the declared context window is behavior-neutral today, but pinning it explicitly shields the local compaction trigger against remote-config experiments that dial it down. The rare user who wants different numbers can still edit `CLAUDE_CODE_MAX_CONTEXT_TOKENS` / `CLAUDE_CODE_AUTO_COMPACT_WINDOW` directly in the provider's JSON editor. The change is confined to `claudeProviderPresets.ts` and its tests.
- **Universal Provider Auto-Syncs to Live Configs After Being Added**: Adding a universal (multi-app) provider through the Add Provider dialog now immediately pushes it to its live target configs instead of only saving it to the database, so the cross-app config lands in Claude/Codex/Gemini without a separate manual sync step. After `universalProvidersApi.upsert` succeeds, `AddProviderDialog` calls `universalProvidersApi.sync(provider.id)` and reports the combined outcome: a success toast (`universalProvider.addedAndSynced`) when the sync lands, or a non-blocking warning toast (`universalProvider.addedButSyncFailed`) when the provider was saved but the sync failed — while a save failure still aborts early with the existing error toast (`universalProvider.addFailed`) and never attempts the sync. The now-unused `universalProvider.addSuccess` key was removed from all four locales (zh/en/ja/zh-TW) and a new `universalProvider.addedButSyncFailed` key was added alongside the pre-existing `addedAndSynced` key. (#2811)
- **SudoCode Promoted to Paid Sponsor Across Six Clients**: The existing SudoCode preset — previously a `sudocode.us` provider that collided by name with an unrelated service — is replaced in place by the new paid sponsor SudoCode on `sudocode.chat`, now marked `isPartner` (gold star) with `partnerPromotionKey: "sudocode"` and a four-locale promo blurb (zh/en/ja/zh-TW). The preset spans six clients: Claude Code, Claude Desktop, Codex, OpenCode, OpenClaw, and Hermes; the Gemini entry was removed as outside sponsor scope. All endpoints move to `api.sudocode.chat` (the presets that carry an `endpointCandidates` list — Claude Code, Claude Desktop, Codex — collapse it to that single host, dropping the old `sudocode.run` fallback), and the `apiKeyUrl` points to the attributed `sudocode.chat/register?utm_source=ccswitch&utm_medium=partner` signup while `websiteUrl` moves to `sudocode.chat`. Claude Code and Claude Desktop use direct Anthropic passthrough with no model mapping, whereas Codex/OpenCode/OpenClaw/Hermes default to `gpt-5.6-sol` (replacing the old `gpt-5.5`) — Codex/Hermes/OpenClaw over the native Responses format and OpenCode via `@ai-sdk/openai`. The provider icon is refreshed to the new brand PNG, and the promo copy surfaces that one key drives Claude Opus 4.8 on the Claude apps and GPT-5.6 on Codex plus a CNY ¥10 trial-credit offer.
- **LongCat Presets Updated to LongCat-2.0**: The LongCat presets across Claude Code, Claude Desktop, Codex, Hermes, OpenClaw, and OpenCode now default to `LongCat-2.0` in place of the retired `LongCat-Flash-Chat` / `LongCat-2.0-Preview`, advertising the model's real 1M (1048576) context window and — for the Claude Code preset — a raised `CLAUDE_CODE_MAX_OUTPUT_TOKENS` of 131072 plus an added `ANTHROPIC_SMALL_FAST_MODEL` pin. OpenClaw's and OpenCode's base URL move to the `.../openai/v1` path (Codex and Hermes already used it), and the OpenClaw model entry gains `reasoning: false`, `input: ["text"]`, `maxTokens: 131072`, a bumped `contextWindow` of 1048576, and a new `compat.maxTokensField` (`max_tokens`) hint backed by a new optional `compat` field on the `OpenClawModel` type. Because LongCat-2.0 is text-only, the proxy's media sanitizer allowlist now classifies `longcat-2.0` (case-insensitively, keeping the retired `longcat-flash-chat` name for saved configs) so images pasted into a LongCat-2.0 session are replaced with the unsupported-image marker instead of being forwarded upstream and hard-rejected; a regression test covers the classification. (#4838)
- **Volcengine/Doubao/BytePlus Website Links Restored to Invite Pages**: Reverts the v3.16.5 change (`56248087`) that had pointed the `火山Agentplan`, `DouBaoSeed`, and `BytePlus` presets' `websiteUrl` at their product homepages. Per the commit, the `websiteUrl` for these three presets is intentionally the same ccswitch-attributed campaign/invite link as `apiKeyUrl`, so it is restored across all six app preset files — `火山Agentplan` back to its `activity/codingplan` link, `BytePlus` to its `product/modelark` link, and `DouBaoSeed` to the Ark console API-key page, each carrying the `utm_*` attribution params. The `apiKeyUrl` links were already left intact by the reverted commit and are unchanged.
- **Code0.ai Invite Link Updated to Agent Register URL**: The Code0.ai sponsor's `apiKeyUrl` moves from the `?source=ccswitch` referral to the new agent-register invite link `https://code0.ai/agent/register/B2XHxGjGmRvqgznY`, updated across all seven app presets and the Code0 sponsor rows in all four READMEs (`README.md`, `README_ZH.md`, `README_JA.md`, `README_DE.md`). The bare `code0.ai` API endpoint stays untouched.
- **Dropped the Redundant OpenAI Compatible Preset**: Removed the `OpenAI Compatible` custom-template preset from the OpenCode and OpenClaw preset lists so the picker no longer shows two entries pointing at the same place. It was a duplicate entry point — the built-in `custom` provider flow already exposes an interface-format selector whose default (`@ai-sdk/openai-compatible` / `openai-completions`) seeds a byte-identical starting config. Existing providers are unaffected, since presets only seed form defaults on creation and saved providers store concrete `npm`/`baseUrl`/`apiKey`/`models` values; the `@ai-sdk/openai-compatible` dropdown label is deliberately kept as the format option users pick in the custom flow, not the deleted preset.

### Fixed

- **Align Codex OAuth Client Identity to Fix 404s on Newest ChatGPT Models**: The newest ChatGPT-gated subscription models (e.g. `gpt-5.6-luna`) now resolve correctly when routed through the local proxy takeover with an official Codex OAuth account, instead of returning a misleading `404 Model not found` even though the account had access. ChatGPT's Codex backend performs model cohort routing by the `originator`+`version` header pair, and cc-switch previously identified takeover requests as `originator: cc-switch` with no version, landing them in a cohort where `gpt-5.6-luna` resolved to an undeployed internal engine. The `CodexOAuth` auth strategy in `proxy/providers/claude.rs` now sends `originator: codex_cli_rs` paired with `version: 0.144.1`, matching the real Codex CLI and satisfying luna's `minimal_client_version` requirement of `0.144.0`. Both headers must be sent together (dropping either re-triggers the 404), and the `CODEX_OAUTH_CLIENT_VERSION` constant must be bumped whenever a newer target model raises the minimum. A direct HTTP A/B test against the live backend confirmed the old identity 404ed while the aligned identity completed successfully, so no WebSocket transport workaround is required.
- **Fail Closed on Responses Upstream Failures Instead of Returning Empty Replies**: When the proxy bridges an Anthropic-format client (Claude Code / Claude Desktop pointed at a Responses gateway) to an OpenAI Responses upstream, a semantic upstream failure that arrives inside an HTTP 2xx body — a `{status:"failed"|"cancelled"}` object, an `error` envelope, or a `response.failed`/`error` SSE event before any output — is now surfaced as a real error so failover can select a different provider, instead of being converted into a silent, empty `end_turn`. The forwarder validates buffered/JSON bodies (`validate_responses_success_response`) and primes streaming responses up to 256 KB (`validate_responses_stream_start`) while still inside the retry loop, and the streaming converter emits an Anthropic `error` event then ignores every event after a terminal so a late delta cannot synthesize a spurious `message_start`. Streams that end cleanly after partial text now finalize as an explicit `max_tokens`-style incomplete turn with content blocks closed in protocol order, whereas a stream cut off mid tool-call or mid-reasoning (partial JSON or a missing thinking signature) yields a `stream_truncated` error rather than a corrupt turn, and on the Codex→Anthropic direction a truncated tool call is reported `incomplete` instead of `completed`. Gateways that ignore `stream:true` and return a single whole JSON document (even without a JSON content-type) are now recognized and expanded into a full Anthropic SSE lifecycle, and malformed client-supplied history is raised as an `InvalidRequest` — a `NonRetryable` category — so it fails fast instead of retrying across every provider. Touches `forwarder.rs`, `streaming_responses.rs`, `streaming_codex_anthropic.rs`, `transform_responses.rs`, and `transform_codex_anthropic.rs`.
- **Preserve Reasoning, Tool Results, and System Roles Across the Responses/Anthropic Bridge**: Content and token accounting that crosses the Responses↔Anthropic bridge in multi-turn tool loops is now preserved instead of being dropped or corrupted. Encrypted Responses `reasoning` items round-trip losslessly by being carried inside a versioned bridge-owned Anthropic thinking `signature` (or a `redacted_thinking` `data` field, prefixed `ccswitch-openai-reasoning-v1:`) and restored on replay, orphaned reasoning-only assistant turns are discarded so they cannot brick the next request with a missing-following-item error, and the streaming converter now consumes the official `response.reasoning_summary_text.*` / `response.reasoning_text.*` event vocabulary (keeping `response.reasoning.*` as a compatibility alias), tracks concurrent items by stable id/output-index, emits the signature before closing the block, and recovers tool arguments from `*.done` / `output_item.done` events when a gateway skips deltas. Non-streaming Responses→Anthropic tool calls with empty arguments normalize to `{}`, invalid arguments in an incomplete turn degrade to `{}` with a warning, and invalid arguments on a completed response are rejected, while on the Codex→Anthropic request path malformed or non-object arguments raise a non-retryable `InvalidRequest`, incomplete historical tool calls are dropped, and only signed thinking blocks are re-encoded as encrypted reasoning. Structured tool results now keep their `is_error` flag, text, base64/URL images, and PDF/`input_file` documents in both directions instead of collapsing to a canonical JSON string, and historical `system`/`developer` messages are hoisted into Anthropic `system` rather than being silently demoted to user turns. For accounting, usage from a successful upstream request is recorded even when the subsequent conversion fails, and the Codex→Anthropic bridge keeps standard 5-minute prompt caching on by default while honoring the dedicated `cache_injection` sub-switch. Touches `reasoning_bridge.rs`, `transform_responses.rs`, `transform_codex_anthropic.rs`, `streaming_responses.rs`, `handlers.rs`, and `forwarder.rs`.
- **Account for Cache-Write Tokens in Proxy Usage and Cost**: Prompt-cache write tokens are now billed correctly on the proxy usage dashboard. The parser reads cache writes from OpenAI/Codex-style usage details (`input_tokens_details.cache_write_tokens` and `prompt_tokens_details.cache_write_tokens`) and preserves them through every response-usage conversion path — Chat→Responses, Responses→Anthropic, Anthropic→Responses, and OpenAI/Chat→Anthropic (`transform_codex_chat.rs`, `transform_responses.rs`, `transform_codex_anthropic.rs`, `transform.rs`, `streaming.rs`) — so cache creation is no longer dropped when a request crosses a format boundary. Because a Codex/Gemini provider's reported `input_tokens` is inclusive of both cache reads and cache writes, the cost calculator now subtracts both before applying the input rate; previously only reads were removed, so cache-write tokens were double-charged at both the input rate and the cache-creation rate. To keep historical rows accurate across this shift, a new `input_token_semantics` column (schema v13; `0` legacy = subtract reads only, `1` total-inclusive = subtract reads and writes, `2` fresh = no subtraction) records how each row's `input_tokens` was stored, so the cost backfill subtracts reads only for pre-existing rows and reads-plus-writes for new total-inclusive rows, while daily rollups are normalized to fresh input and stamped `2`. v12 databases gain the column on both `proxy_request_logs` and `usage_daily_rollups` via `migrate_v12_to_v13`, and Claude-style rows stay fresh input with no subtraction.
- **Stronger Prompt-Cache Breakpoint Injection on the Proxy Bridge**: On the proxy paths that inject Anthropic `cache_control` breakpoints — the `codex_responses_to_anthropic` takeover bridge and the Bedrock native optimizer — the injector now spends its four-breakpoint budget more effectively so long, tool-heavy conversations keep hitting the prompt cache instead of re-sending system, tools, and history at full price every turn. Beyond marking the tools tail, system tail, and the latest cacheable message, it now adds a second older user anchor (`msgs-prior-user`) when at least four messages exist and budget remains, keeping the stable prefix inside Anthropic's 20-block lookback from the newest breakpoint. Injection is short-circuited unless both the optimizer master switch (`enabled`) and the `cache_injection` sub-switch are on, `thinking`/`redacted_thinking` blocks are never chosen as cache targets, and injected markers use Anthropic's standard 5-minute TTL. Caller-owned breakpoints are preserved verbatim — never deleted, reordered, or rewritten — and when a caller already ships more than the supported four, the injector logs a warning, adds no automatic breakpoints, and leaves the markers in place (`cache_injector.rs`, `forwarder.rs`).
- **Kimi For Coding's 256K Context Window Now Takes Effect**: The Kimi For Coding preset's standalone `CLAUDE_CODE_AUTO_COMPACT_WINDOW=262144` (added in #4401 and shipped in 3.16.4) never actually applied, because Claude Code caps unrecognized non-Claude model ids at a 200K window and resolves the compact window as `min(model window, value)`, clamping 262144 back down to 200K. The preset now pairs it with `CLAUDE_CODE_MAX_CONTEXT_TOKENS` (both pinned to 262144) and explicitly routes the endpoint's `kimi-for-coding` alias across `ANTHROPIC_MODEL` and all three tier keys (`ANTHROPIC_DEFAULT_HAIKU/SONNET/OPUS_MODEL`), since Claude Code ignores both context envs for `claude-`-prefixed ids — the non-Claude alias is what unlocks the enlarged window. For already-saved providers, `build_effective_settings_with_common_config` in `services/provider/live.rs` injects the same two context defaults (262144) into the effective live settings at switch time for any provider whose `ANTHROPIC_BASE_URL` is the Kimi coding endpoint (`https://api.kimi.com/coding`), with a mirror-inverse strip on backfill so an injected default never hardens into stored provider config and an explicit user value always wins. Because the alias routing lives only in the preset and not in the injection, a provider saved from the old preset — which set neither the alias nor `CLAUDE_CODE_MAX_CONTEXT_TOKENS` — stays effectively at 200K until the preset is re-applied. Three tests cover backfill injection, user-override preservation, and injected-only strip.
- **Deleted Codex MCP Servers No Longer Resurrected on Provider Activation**: MCP servers are owned by the database `mcp_servers` table and the `[mcp_servers]` section in Codex's live `config.toml` is only a projection re-synced after every write, but switching away used to bake that projection into the stored provider snapshot — so a server you deleted in the app came back to life the next time you activated that provider, and per-entry reconcile (which only knows rows still in the DB) could never clean up the orphan. Switch-away backfill now strips `[mcp_servers]` (and the legacy `[mcp.servers]` form) from the stored settings via `strip_codex_mcp_servers_from_settings`, and any snapshot already polluted self-heals the next time you switch away from it. One user-visible consequence: a hand-written `[mcp_servers.*]` section in a Codex provider's config is stripped out of the stored snapshot the first time you switch away, so going forward define Codex MCP servers through the MCP manager (the DB `mcp_servers` table) rather than by hand-editing the provider config.
- **Codex MCP Sync Fails Closed on an Unparseable config.toml**: When writing a single MCP server into Codex, `sync_single_server_to_codex` silently fell back to an empty `toml_edit` document if the existing `config.toml` failed to parse and then wrote the whole file back — wiping every other section (`model`, `model_providers`, comments) and leaving only the one synced `[mcp_servers.<id>]` entry, a destructive fallback left behind when an earlier fix (`3a548152`) hardened only the removal path. The sync path now returns an `McpValidation` error and leaves the file untouched, matching the fail-closed behavior already used on the read/validate path, so a temporarily malformed config can no longer be silently truncated to a single MCP table.
- **MCP Operations Now Report Per-App Failures Instead of Silently Blocking Others**: Importing MCP servers from apps used to swallow every importer error with `unwrap_or(0)`, so a corrupt Codex `config.toml` surfaced only as "imported 0 servers" with no hint anything went wrong; `import_from_all_apps` now imports each app (claude/codex/gemini/opencode/hermes) best-effort and, when any fail, reports an aggregated error naming the failing apps alongside the count that did import, with the frontend refreshing the server list on settle so partially-imported servers still appear. On the projection side `sync_all_enabled` iterated every app with `?`, so one app's unparseable live file (e.g. a broken `~/.claude.json` that passes the existence gate but fails to parse) blocked every app behind it in iteration order and bubbled a false "switch failed" back even though the target app's live file and the database were already updated; switch and save now re-project only the target app through the new `sync_enabled_for_app` and degrade projection failure to a warning, while config-import and cloud-restore keep the all-apps sweep but collect failures best-effort. Toggling `unify_codex_session_history`, which rewrites the current official provider's live `config.toml` in full and drops the `[mcp_servers]` projection, now re-projects Codex MCP through that same target-only path so enabled servers no longer silently vanish until the next provider switch. In every degraded case the projection self-heals on the next switch or MCP toggle.
- **Codex Common-Config Form Preserves Comments and Key Order**: Toggling "use common config" in the Codex provider form used to route the merge through the frontend `smol-toml` implementation, which re-serialized the whole document (parse → deep-merge → stringify): comments were dropped, keys were reordered, and empty parent table headers like `[model_providers]` were synthesized — the long-standing "config.toml keeps getting reordered" symptom. The merge now runs on a backend command backed by the same `merge_toml_table_like` / `remove_toml_table_like` used to write live configs, so the form preview and the live write share one merge semantic and hand-written formatting survives edit-time merges, and the frontend helper was deleted outright so the pattern can't come back. Because the operations are now async, a landing result is discarded unless it is still current: a per-hook sequence number covers rapid toggle/save races so an earlier merge resolving after a later removal can't flip the switch back (last operation wins), and a config-baseline check keeps a stale merge from clobbering a TOML the user hand-edited while the merge was in flight, with the checkbox self-healing via the existing inference effect.
- **Inject a Single Auth Placeholder on Managed Claude Takeover**: Switching the managed Claude provider from a third-party endpoint (DeepSeek/MiMo/…) to a Codex-managed provider wrote both `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` as `PROXY_MANAGED` into `~/.claude/settings.json`, so Claude Code warned "Both ANTHROPIC_AUTH_TOKEN and ANTHROPIC_API_KEY set - auth may not work as expected" on every run — an accretion artifact of the original API-key insert (#1049, Copilot) and the later `ANTHROPIC_AUTH_TOKEN` preservation that avoided a login prompt (#3784), which added the token without removing the key. The `ManagedAccount` branch of `apply_claude_takeover_fields_for_provider` now clears all token keys and injects exactly one placeholder: `ANTHROPIC_AUTH_TOKEN` for Codex-managed providers and `ANTHROPIC_API_KEY` for Copilot; local-proxy auth is unaffected because the forwarder resolves the `PROXY_MANAGED` placeholder from either header. Because enabling takeover short-circuits (`return Ok(())`) when the live config already matches the current proxy, users upgrading with an existing double-key `settings.json` may need to toggle Claude routing off and back on once (or switch providers) to trigger the rewrite. (#4919, #5095)
- **Skip Reachability Probes for Official Providers**: Health/connectivity checks no longer derive an unauthenticated probe target from an official provider's runtime adapter defaults. Batch `stream_check_all_providers` now skips any entry whose `category` is `official`, and `StreamCheckService::resolve_base_url` returns an explicit error for official providers instead of falling back to a first-party endpoint (e.g. `https://chatgpt.com/backend-api/codex`) and probing it without credentials — a request that is meaningless and always fails. The connectivity-check button is already hidden for these providers in `ProviderCard.tsx`, so this is defense-in-depth that also matters now that the built-in Codex official provider participates in takeover; a regression test covers the Codex official provider so a future adapter change cannot silently reintroduce the probe.
- **Usage and Quota Queries Reject Transient Transport Failures So Retry and Keep-Last-Good Work**: Usage and quota queries frequently showed spurious "query failed" states that a manual refresh could not clear, because every transport-level failure — including read-timeouts mid-body — was folded into `Ok(success:false)`, so react-query's retry never fired and the failure body poisoned the cache as if it were real data. The `balance`, `coding_plan`, and `subscription` services now return `Err` for send failures and body-read failures, reading the body via `bytes()` before `serde_json::from_slice` (reqwest's `json()` wraps read errors as `Decode`, which made error-kind checks on it dead code), while auth/4xx/parse errors stay `Ok(success:false)` and surface immediately; the script path maps transient `AppError` keys (`request_failed` / `read_response_failed`) to `Err`, Volcengine gains a `Transient` call variant, HTTP 429 is now classified transient alongside 5xx, and expired-credential retries propagate the transient error instead of rewriting it as "OAuth token has expired". The command layer skips snapshot persistence, the `usage-cache-updated` emit, and tray refresh on `Err` so the cache bridge cannot overwrite retained data. On the frontend, `resolveDisplayUsage` (generalized to subscription quotas via a new options object carrying a `rejected` flag) re-anchors stale success data retained by react-query across rejections to `dataUpdatedAt` so it expires through the same 10-minute keep-last-good window instead of masking a longer outage indefinitely, `useSubscriptionQuota` / `useCodexOauthQuota` gain keep-last-good with per-scope snapshot reset, rejected queries with no displayable value synthesize a failure placeholder carrying the real error message so the footer keeps a retry entry point, and `UsageScriptModal` surfaces string rejections via `extractErrorMessage`. (#3820)
- **Codex Sub-Agent Session Usage Now Counted in Local Statistics**: Token usage from Codex sub-agent (spawned agent) sessions was missing from local usage statistics because the parser keyed each session's synthetic `request_id` on `session_meta.session_id`, but a sub-agent's log carries the parent thread's `session_id` while its own unique identity lives in the `id` / `thread_id` field — so multiple sub-agents under one parent collapsed onto colliding request IDs and were dropped as duplicates. `session_usage_codex.rs` now extracts a unique `thread_id` per file (preferring `id` / `thread_id` over `session_id`) and prefixes request IDs with `codex_session:thread-v1:<thread_id>:<index>`, giving each sub-agent distinct rows. Because fork/sub-agent logs first replay the parent thread's history before the takeover event, the parser detects a history-snapshot boundary (via `forked_from_id` / `subagent` source markers plus a `thread_settings_applied` or `inter_agent_communication` event) and uses the replayed `token_count` events only to restore the cumulative baseline instead of double-counting them as new usage. Archived logs under `archived_sessions/` inherit the sync cursor from their original path by filename suffix, so re-parsing an archived copy imports only appended usage rather than re-importing the whole thread; the fix ships with unit tests covering identity extraction, replay-baseline accounting, distinct request IDs, and archived-cursor inheritance. (#5187)
- **Codex Free-Plan 30-Day Quota Window Now Renders in Footer and Tray**: Codex free accounts are metered on a rolling 30-day secondary window (`limit_window_seconds = 2,592,000`) instead of the weekly window paid plans use, and although the backend already mapped this to the tier name `30_day`, neither the frontend `TIER_I18N_KEYS` whitelist nor the tray's `TIER_LABEL_GROUPS` month group recognized it — so `SubscriptionQuotaView` filtered the tier out and, since it is the only surviving tier for a free account, the quota footer rendered nothing while the tray's `format_subscription_summary` returned `None` and showed no quota at all. A single-sourced `TIER_THIRTY_DAY` ("30_day") constant now ties together the backend `window_seconds_to_tier_name` mapping (which maps `2_592_000` explicitly), the tray's month ("m") group, and the frontend whitelist (`30_day → subscription.thirtyDay`), with the `thirtyDay` label added to all four locales (zh/en/ja/zh-TW). Two regression tests lock in the window-seconds-to-tier mappings and assert that a 30-day-only Codex account still renders in the tray, guarding the invariant that any tier visible in the footer must not leave the tray blank. (#3651, #4886)
- **Usage Dashboard Refresh Interval Persists Across Restarts**: The usage dashboard's auto-refresh cadence was component-local state that reset to 30s on every restart, so a user who chose a different interval (off, 5s, 10s, or 60s) had to re-select it each session. The dashboard now reads and writes the choice through a new `usageDashboardRefreshIntervalMs` app setting: `UsageDashboard` takes `refreshIntervalMs` / `onRefreshIntervalChange` props wired from `SettingsPage` to the persisted settings, initializes from the saved value (normalized to a known option, falling back to the 30s default for unrecognized values), and applies changes optimistically — invalidating the usage queries immediately — while rolling back the selection if the save fails. The field is added to the Rust `AppSettings` struct, the TypeScript `Settings` type, and the zod settings schema, and three component tests cover mount-from-saved, persist-on-change, and rollback-on-failure. (#5057)
- **Exclude Fable Tier Model Env Keys From the Shared Claude Common Config**: Fable is Claude's fourth model-mapping tier (added in v3.16.3), but its `ANTHROPIC_DEFAULT_FABLE_MODEL` / `ANTHROPIC_DEFAULT_FABLE_MODEL_NAME` env keys were missing from the provider-specific exclusion list, so a provider's Fable model pin could leak into the shared common-config snippet and then be injected into other providers on the next switch. Both keys are now stripped in `extract_claude_common_config` alongside the haiku/sonnet/opus pins, guarded by a regression test. The same commit also completes Fable's proxy-takeover support: the two keys are added to `CLAUDE_MODEL_OVERRIDE_ENV_KEYS` (now 12 entries) and the takeover writes a stable `claude-fable-5` role alias (`CLAUDE_TAKEOVER_FABLE_MODEL`), reapplying it when the provider configures Fable and clearing stale values otherwise, mirroring the other three tiers; when Fable is unconfigured no alias is written and the mapper falls back fable→opus. (#5206, fixes #4272)
- **Default Missing Tool Schema Types and Restore the API Key Field for Uncategorized Providers**: Two provider-facing fixes. First, when a client sends a tool whose Anthropic `input_schema` omits the top-level `type` or is an empty `{}`, the proxy passed the schema through the shared `clean_schema` helper without filling in the missing type, producing an OpenAI `parameters` object that strict gateways reject. `clean_schema` (defined once in `transform.rs` and reused by the Responses converter via `super::transform::clean_schema`, so both the Anthropic→OpenAI Chat and Responses paths pick up the change) now defaults a missing root `type` to `"object"` — adding an empty `properties: {}` only when properties is also absent — and applies the defaulting only to the root schema via a new `is_root` flag threaded through a `clean_schema_inner` helper, so nested sub-schemas (an `anyOf` string/null union, an `items`-only array) are preserved unchanged rather than having a spurious `type: "object"` forced onto them. Second, editing a provider with no category (a historically imported or hand-built custom provider) hid the Claude API key input, because `useApiKeyState` only showed and auto-created the field in add mode with a defined non-official category; the gate now keys off category alone, showing and populating the field for any provider that isn't `official` or `cloud_provider` — including the undefined-category edit case — while `official` (OAuth-only, input disabled) and `cloud_provider` (which use dedicated auth fields rather than an Anthropic key) stay conservative. New unit and hook tests cover both converters and the uncategorized/official/cloud-provider key paths. (#5069)
- **Media Fallback for Volcano GLM 5.2 Text-Only Image 400s**: When the local proxy takes over a Volcano Coding Plan provider running GLM 5.2, image blocks in a request no longer produce a dead 400 — the rectifier's media fallback in `src-tauri/src/proxy/media_sanitizer.rs` now covers GLM 5.2 on both its preventive and reactive paths. Preventively, the text-only model registry gains `glm-5.2` as an exact tail match (deliberately not a prefix, so a future multimodal `glm-5.2v` variant following Zhipu's 4v/5v naming keeps its images), stripping image blocks before they reach the text-only endpoint. Reactively, because the gateway's error `Model only support text input` never mentions image/vision/media and drops the third-person `s`, a new self-evident phrase list (`only support text` / `only supports text`) now asserts a modality rejection on its own and bypasses the `mentions_image` gate that previously discarded the error before the fallback hints could run; the old `only supports text` hint is folded into that list. Regression tests exercise the verbatim #5025 error body, the `glm-5.2[1M]` mapped-model form, and a `glm-5.2v` negative assertion. (#5025)
- **Display Renamed Codex Session Titles**: Sessions renamed inside Codex now show their renamed titles in the session manager instead of silently falling back to the original first-message text. The scanner (`src-tauri/src/session_manager/providers/codex.rs`) loads thread titles from Codex's `session_index.jsonl` and `state_5.sqlite` — resolving the DB path from `sqlite_home` / `CODEX_SQLITE_HOME` through a new shared `codex_state_db` module that deduplicates path resolution previously copy-pasted with the history-migration code — and prefers a stored thread title over the first user message when building each session's display title. The read-only title query adds a `busy_timeout` so a lookup during a concurrent Codex write no longer fails immediately with `SQLITE_BUSY` and drops back to the first message, and the `title == first_user_message` check is pushed into a NULL-safe SQL `WHERE` clause (matching Codex's `distinct_thread_metadata_title` semantics) so the potentially large `first_user_message` column no longer crosses into Rust. (#4927)
- **Live Config Edits for OpenCode, OpenClaw, and Hermes Now Sync Into the Database on Startup**: The startup auto-import for the live-config-managed apps previously skipped any provider whose id already existed in the database, so edits made directly to the OpenCode, OpenClaw, or Hermes (`~/.hermes/config.yaml`) live files — a changed base URL, an added or renamed model — were never picked up after the first import and silently diverged from the app's stored copy. The `import_{opencode,openclaw,hermes}_providers_from_live` functions in `services/provider/live.rs` now look up each existing provider and, when its live `settings_config` differs, rewrite the stored provider from the live file (OpenCode additionally refreshes the display name from the live `name`, falling back to the existing name rather than the id); the returned count now covers both new imports and updates, and the startup log line changed from "Imported N" to "Synced N". The sync runs on every launch from `lib.rs::run()`, is a no-op when nothing changed, and is fully non-fatal — a provider that vanishes mid-import or a DB lookup error is warned and skipped, and users with no live file still take the quiet `Ok(0)` path. (#4712, #5098)
- **OpenCode Session Resume Command Updated to Current CLI Syntax**: The Session Manager displayed and copied `opencode session resume <id>` as the resume command for OpenCode sessions, which is not the OpenCode CLI's current resume syntax, so pasting the copied command would not resume the session. Both the SQLite-scanned (`scan_sessions_sqlite`) and JSON-parsed (`parse_session`) session paths in `session_manager/providers/opencode.rs` now emit `opencode -s <id>` to match the CLI's actual resume flag. (#2359)

### Docs

- **Codex + Kimi Local Routing Guides**: New step-by-step guides explain how to run Kimi inside the Codex CLI through CC Switch's local routing, motivated by a protocol mismatch: the newer Codex CLI targets the OpenAI Responses API while both Kimi Open Platform and Kimi For Coding expose the OpenAI Chat Completions shape (`/chat/completions`), so pointing a Kimi endpoint straight at Codex typically 404s on `/responses` or yields streams Codex cannot parse. The guides walk through adding a Codex provider from the built-in `Kimi` (Open Platform, pay-as-you-go, `kimi-k2.7-code`) or `Kimi For Coding` (membership, `kimi-for-coding`) preset, and lay out the four-step conversion chain: Codex is written to talk to `http://127.0.0.1:15721/v1` with `wire_api = "responses"` kept in place, the provider's `meta.apiFormat = "openai_chat"` flags the real upstream as Chat, the route rewrites `/responses` to `/chat/completions` and converts the body, then converts the upstream Chat JSON/SSE back into the Responses shape Codex expects. Added under `docs/guides/` in English, Japanese, and Chinese with accompanying UI screenshots.
- **new-api Sponsor Row in READMEs**: The open-source AI infrastructure project `new-api` is appended as the newest entry in the sponsor table across all four localized READMEs (`README.md`, `README_ZH.md`, `README_JA.md`, `README_DE.md`), along with its banner asset.

### Internal

- **Harden Release Supply Chain**: Added a `.github/CODEOWNERS` that, together with branch protection's Code Owners rule, requires owner review before any PR merges to `main` (with `/.github/` and `/src-tauri/` pinned explicitly alongside the global `*` fallback, all to `@farion1231`), gated the release job behind a `release` environment in `release.yml` so signing secrets unlock only after manual approval, and removed `.github` from `.gitignore` so the previously untracked `labeler.yml` and `workflows/labeler.yml` are now versioned.
- **Settle pnpm Build-Script Approvals**: Approved `esbuild` (via `onlyBuiltDependencies`) and ignored `msw` (via `ignoredBuiltDependencies`) in `pnpm-workspace.yaml` so pnpm 10.13+ stops appending `allowBuilds` placeholders to the file on every install.
- **Platform-Gate the Desktop-Scope Assertion in the Profile Roundtrip Test**: The `profile_roundtrip` integration test's Claude Desktop assertion is now `cfg`-gated to macOS/Windows to match the already cfg-gated desktop switch, so on Linux CI (where desktop live writes error) it expects the seeded `d1` instead of `d2` and no longer panics, poisons the shared test mutex, and cascades into two more failures.

## [3.16.5] - 2026-07-01

Development since v3.16.4 reworks the Codex native-Responses path — restoring a generated model catalog for proxy-less direct-connect, decoupling model mapping from the local-routing toggle, and adding a host/model-prefix blacklist that disables Codex's built-in web_search on gateways that reject it — alongside a broad wave of new provider presets (Qiniu and Code0.ai across all seven apps, the FennoAI/ZetaAPI/TeamoRouter/NekoCode partners, and the non-partner Amux), Claude Sonnet 5 pricing plus a default-tier bump to it, a categorized two-level session view with group-level batch selection, live auto-sync of the shared Claude common config on switch, and a run of credential-safety, tool-detection, Doubao model-id, branding, and icon-size fixes.

**Stats**: 36 commits | 93 files changed | +5,678 insertions | -2,804 deletions

### Added

- **Codex Native Responses Direct-Connect Regenerates a Model Catalog**: Reverses the v3.16.4 direction that dropped the catalog — Codex providers now run in two explicit modes, and native direct-connect once again ships a catalog file. Providers with `apiFormat: "openai_responses"` run with no proxy, and cc-switch generates `~/.codex/cc-switch-model-catalog.json` (`CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME`, provider id `custom`) so Codex Desktop actually shows the custom models and tools work without the freeform `apply_patch` (`type=custom`) tool that native gateways like MiMo reject — editing falls back to `shell_command`. Catalog generation is keyed on `apiFormat` via `CodexCatalogToolProfile` and decoupled from the route-takeover toggle, so a native provider persists a catalog without enabling local route mapping, while `openai_chat` keeps the existing Responses↔Chat proxy conversion unchanged. Codex's parser requires `base_instructions` on every entry, so the native template (`codex_native_responses_template.json`, slug `gpt-5.5`) carries a neutral default that per-vendor official text overrides (MiMo, MiniMax); synthesized catalogs for Qwen/Doubao/LongCat use the neutral default. Existing native providers must be re-saved once to regenerate a valid catalog (no DB migration).
- **Categorized Session View With Two-Level Grouping**: The Session Manager gains a grouped view mode alongside the flat list, toggled via a List/ListTree Select in the toolbar and persisted to `localStorage` under `cc-switch.sessionManager.listViewMode`. The new `groupSessionsByProviderAndDirectory` helper builds a two-level hierarchy (provider group → project-directory group): directory groups are keyed by `getSessionDirectoryGroupKey(providerId, projectDir)` and labeled with `getBaseName(projectDir)`, and sessions lacking a `projectDir` fall into an "unknown directory" bucket (`UNKNOWN_PROJECT_DIR_KEY = __unknown_project_dir__`). Both levels are Radix Collapsible sections whose expansion state is serialized to `cc-switch.sessionManager.groupExpansionState` (pruned to valid keys after load), with a "collapse all" button; in batch mode each group header gains a tri-state checkbox that selects/deselects every selectable session (those with a `sourcePath`) at once, backed by a `Minus`-glyph indeterminate state added to `ui/checkbox.tsx` and a selected/selectable count badge. New keys were added across all four locales (zh/en/ja/zh-TW). The change is entirely frontend — no Tauri commands or DAO changes. (#4776)
- **Qiniu (七牛云) Provider Preset Across All Seven Apps**: Added the Qiniu Cloud AI gateway as a partner aggregator spanning Claude Code, Claude Desktop, Codex, Gemini, OpenCode, OpenClaw, and Hermes — the widest coverage of the batch and the only one carrying a Gemini preset. Because Qiniu relays native Claude/GPT/Gemini rather than remapping to domestic models, Claude Code/Desktop use the Anthropic-compatible bare host with native passthrough (no model pinning), Codex hits native Responses at `https://api.qnaigc.com/bypass/openai/v1` defaulting to `gpt-5.5`, and Gemini uses `gemini-3.1-pro-preview` via `https://api.qnaigc.com/bypass/vertex`; OpenCode/OpenClaw/Hermes default to `gpt-5.5` over the OpenAI-compatible `/v1`. Each type carries `endpointCandidates` listing the `api.qnaigc.com` primary plus the `api.modelink.ai` overseas mirror for failover. It is marked `isPartner` with referral `https://s.qiniu.com/nMvAvy`, a localized display name via `nameKey` (`providerForm.presets.qiniu`) and a partner-promotion blurb in all four locales (zh/en/ja/zh-TW), and registers the `qiniu.png` icon through URL import plus `iconUrls` and metadata.
- **FennoAI, ZetaAPI, and TeamoRouter Partner Presets**: Added three partner aggregators — FennoAI (`api.fenno.ai`), ZetaAPI (`api.zetaapi.ai`), and TeamoRouter (`api.teamorouter.com`) — each covering Claude Code, Claude Desktop, Codex, OpenCode, OpenClaw, and Hermes with no Gemini preset, since these relay native Claude/GPT only. In every case Claude Code/Desktop use the Anthropic-compatible bare host with native passthrough, Codex uses native Responses (FennoAI at `api.fenno.ai`, ZetaAPI and TeamoRouter at their `/v1` paths) defaulting to `gpt-5.5`, and OpenCode/OpenClaw/Hermes default to `gpt-5.5` over the OpenAI-compatible `/v1`. All three are `isPartner` with referral `apiKeyUrls` (FennoAI's ¥9.9 Coding Plan link, ZetaAPI's `zetaapi.ai/go/ccs`, TeamoRouter's `teamorouter.com` with `utm_source=cc_switch` — its in-app link stays English while README links are per-language), each ships a localized promo blurb in zh/en/ja/zh-TW plus a sponsor row in all four READMEs, and each registers its own icon (`fenno-icon.webp`, `zetaapi-icon.png`, `teamorouter`). Two carry surfaced discount codes: ZetaAPI gives a first-recharge 10% discount with promo code `CC-SWITCH`, and TeamoRouter gives new users 10% off their first top-up.
- **Amux Aggregator Preset**: Added Amux (`api.amux.ai`) as a non-partner aggregator across Claude Code, Claude Desktop, Codex, OpenCode, OpenClaw, and Hermes, with no Gemini preset. Claude Code/Desktop use the bare Anthropic-compatible host, Codex uses native Responses over the `/v1` path, and the remaining three apps default to `gpt-5.5` over the OpenAI-compatible endpoint. Unlike the partner presets in this batch it carries no `isPartner` flag, referral link, or promo copy; the `amuxapi-icon.svg` is registered as an inline `currentColor` icon in `src/icons/extracted/index.ts` and `metadata.ts` rather than a raster URL import.
- **Code0.ai Partner Preset Across All Seven Apps**: Added Code0.ai (`code0.ai`) as a partner aggregator spanning all seven apps — Claude Code, Claude Desktop, Codex, Gemini, OpenCode, OpenClaw, and Hermes. Claude Code, Claude Desktop, and Gemini use the bare `https://code0.ai` host (Anthropic-native passthrough with no model pinning for the two Claude apps, `gemini-3.1-pro-preview` for Gemini), Codex hits native Responses at `https://code0.ai/v1`, and OpenCode/OpenClaw/Hermes default to `gpt-5.5` over the OpenAI-compatible `/v1`. It is marked `isPartner` with a `?source=ccswitch` referral on the API-key signup link, a partner-promotion blurb surfacing the CC Switch test-credit offer in all four locales (zh/en/ja/zh-TW), a sponsor row in all four READMEs, and the `code0.png` icon registered via URL import.
- **NekoCode Partner Preset Across Six Apps**: Added NekoCode (`nekocode.ai`) as a partner aggregator spanning six apps — Claude Code, Claude Desktop, Codex, OpenCode, OpenClaw, and Hermes (no Gemini preset). Claude Code and Claude Desktop use the bare `https://nekocode.ai` host (Anthropic-native passthrough with no model pinning), Codex hits native Responses at `https://nekocode.ai/v1`, and OpenCode/OpenClaw/Hermes default to `gpt-5.5` over the OpenAI-compatible `/v1`. It is marked `isPartner` with a `?aff=CCSWITCH` referral on the API-key signup link (kept off the actual request endpoints), a partner-promotion blurb in all four locales (zh/en/ja/zh-TW), and a sponsor row in all four READMEs, both surfacing the CC Switch offer — 10% off top-ups with promo code `cc-switch` at recharge. The `nekocode-icon.png` icon is registered via URL import.
- **Claude Sonnet 5 Model Pricing**: Seeded a `claude-sonnet-5` pricing row in `schema.rs` at Anthropic list price — $3/$15 per Mtok input/output and $0.30/$3.75 cache read/write, identical to the Sonnet 4.6 rates. The introductory $2/$10 promo (valid through 2026-08-31) is deliberately not seeded, so accounting reflects steady-state list pricing rather than a temporary discount. The row is applied on next app start via `ensure_model_pricing_seeded` and requires no `SCHEMA_VERSION` bump.

### Changed

- **Decoupled Codex Model Mapping From the Local-Routing Toggle**: Aligns the Codex provider form with Claude Code — the model-mapping catalog is now independent of route takeover, since native Responses providers (MiMo, Doubao, MiniMax) need it for proxy-less direct connect while Chat providers use the proxy regardless of any per-provider flag. The "Needs Local Routing" toggle is removed: it had no backend field and only gated catalog/reasoning persistence, which is equivalent to whether the mapping is filled. Model mapping is now always shown for non-official providers and persisted whenever the list is non-empty (the backend already keys off `modelCatalog.models`), while reasoning visibility/persistence is gated on the Chat format instead of the toggle. The Chat upstream-format option is marked "routing required" with a refreshed advanced-section hint across all four locales (zh/en/ja/zh-TW), dead `localRouting*` keys are dropped, and the model-mapping block moves above the custom User-Agent block. Also fixes `useCodexConfigState` dropping `supportsParallelToolCalls`/`inputModalities`/`baseInstructions` when loading a saved provider — which silently lost parallel tools, image input, and the official base instructions on edit — by preserving them on load (camelCase and snake_case) and comparing them in the row sync.
- **Auto-Sync Claude Common Config From Live settings.json on Switch**: When switching away from a Claude provider that has `common_config_enabled`, the service now re-extracts the shareable portion of its live `settings.json` and replaces the stored common-config snippet, instead of only writing the snippet one way. This captures config the user added directly in the running app (`enabledPlugins`, `hooks`, `env`, `theme`, shared prefs) so it isn't silently lost on the next switch, and it propagates deletions so a removed key isn't re-injected later. The sync in `services/provider/mod.rs` is scoped strictly to Claude providers with `common_config_enabled`, is skipped when the snippet was explicitly cleared, and all failures are non-fatal (warn-only) and never block the switch. Four integration tests cover capture / delete-sync / opt-out / cleared.
- **Default Sonnet Tier Now Pins to Claude Sonnet 5**: Bumped every default Sonnet pin from `claude-sonnet-4-6`/`claude-sonnet-4.6` to `claude-sonnet-5` across all provider presets (`claudeProviderPresets`, `claudeDesktopProviderPresets`, `hermesProviderPresets`, `openclawProviderPresets`, `opencodeProviderPresets`, and universal `NEWAPI_DEFAULT_MODELS`), covering the `ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_SONNET_MODEL` / `ANTHROPIC_DEFAULT_OPUS_MODEL` env keys and prefixed variants like `anthropic/claude-sonnet-5` and `global.anthropic.claude-sonnet-5`. The Claude Desktop `DEFAULT_PROXY_ROUTES` sonnet `route_id` in `claude_desktop_config.rs` also moves to `claude-sonnet-5` (`supports_1m` preserved), and the route-derivation test expectations were updated to match. Non-Anthropic pins (gpt/gemini/glm/sonnet-4-5) were left untouched.
- **Doubao Dated Model Id and YYMMDD Pricing Normalization**: Switched the Doubao (DouBaoSeed) preset model id to the dated form `doubao-seed-2-1-pro-260628` across every app surface (config default, generated Codex catalog, and OpenClaw namespaced refs), because Volcengine Ark rejects the bare `doubao-seed-2-1-pro` with a 404 ("model does not exist or you do not have access to it") even after activation and only accepts the full dated id. Because real usage now arrives with a date suffix, `strip_model_date_suffix` in `usage_stats.rs` was extended to also strip Volcengine's 6-digit YYMMDD form (`-260628`, `-250615`) in addition to the existing 8-digit YYYYMMDD/ISO handling; the 6-digit branch validates month 01-12 and day 01-31 to avoid eating non-date version suffixes like `-123456`, then falls back to `None` for exact matching. This keeps the bare-name seed row as the canonical pricing identity, fixing the $0-cost display for every Volcengine Doubao model. Unit and end-to-end regression tests were added.
- **"Write Common Config" Renamed to "Apply Common Config"**: The original label "Write Common Config" (写入通用配置) was ambiguous about data-flow direction, reading as "write the current config INTO the common config" when the actual behavior is the reverse — the saved common-config snippet is merged INTO this provider's config. The checkbox is renamed to "Apply Common Config" across all four locales (zh/en/ja/zh-TW), including every hint/guide/notice reference and the `CommonConfigEditor` / `GeminiConfigSections` defaultValue fallbacks, and the Gemini hint wording is aligned with the claude/codex "when checked" phrasing. The Japanese user manual (`docs/user-manual/ja/2-providers/2.1-add.md`) and `README_JA.md` were also synced to 共通設定を適用 / 共有設定を適用. (#4829)
- **OpenClaw Doubao Context Window Aligned to 262144**: The OpenClaw DouBaoSeed preset hard-coded `contextWindow` 128000 while the Codex preset/catalog used 262144 for the same model, giving OpenClaw users a too-small window that could compress or truncate long context prematurely. Bumped `openclawProviderPresets.ts` to 262144 and added a cross-preset consistency test asserting the OpenClaw and Codex Doubao context windows stay equal so the two sides cannot silently drift apart again.
- **Volcengine/Doubao/BytePlus Website Links Restored to Official Homepages**: The `websiteUrl` for the 火山Agentplan, BytePlus, and DouBaoSeed presets had accidentally been set to the same invite/console link as `apiKeyUrl`, so the "visit official site" button routed users to the referral/API-key page instead of the product homepage. Restored clean official homepages across all six app preset files — 火山Agentplan to `https://www.volcengine.com/product/ark`, DouBaoSeed to `https://www.volcengine.com/product/doubao`, and BytePlus to `https://www.byteplus.com/en/product/modelark` (dropping the utm params) — while leaving the `apiKeyUrl` referral links intact.
- **Kimi and SiliconFlow Referral Links Refreshed**: Updated two sponsor referral sets across presets and READMEs. Following Moonshot's console rebrand to Kimi, the Kimi preset referral links and the domestic (ZH) README banner moved to the new `platform.kimi.com` domain — the domestic console `platform.moonshot.cn/console` → `platform.kimi.com`, the API-key page → `platform.kimi.com/console/api-keys`, and the Coding Plan link `www.kimi.com/code/docs/` → `www.kimi.com/code/` — and the missing `?aff=cc-switch` attribution param was added to the codex and openclaw entries. API endpoints (`api.moonshot.cn`, `api.kimi.com/coding`) were left unchanged, and the presets have no overseas variant (the separate EN/DE/JA README switch to `platform.kimi.ai` is covered under Docs below). Separately, the SiliconFlow invite code was rotated from `drGuwc9k` to `YflgU2Ve` across all README locales and the claude, claude-desktop, codex, hermes, and openclaw presets.
- **Downscaled Oversized Provider Icons to 256px**: Shrank a batch of bundled provider icons that shipped far larger than their ~32px on-screen render size in `ProviderIcon`, cutting bundle weight with no code, filename, or import changes (aspect ratio preserved, rendered via `<img>` object-contain). Raster PNGs: ZetaAPI 1254×1254/940KB→40KB, relaxcode 1462×1076/1.16MB→42KB, sudocode 2048×2048/432KB→37KB, hermes 512×512/125KB→38KB, claudecn 512×512/109KB→46KB, atlascloud 3525×3300/105KB→9KB. Two "fake vector" SVGs wrapping a single oversized base64 PNG were re-embedded at a 256px max dimension keeping the SVG wrapper: ccsub 1.16MB→60KB and shengsuanyun 212KB→51KB. Also removed `dds.svg`, a 1.4MB genuine 2222-path vector that was never imported in `index.ts` nor referenced under `src/`, so it only bloated the repo without ever shipping.

### Fixed

- **Disable web_search for Native Codex Gateways That Reject It**: Some native `/responses` gateways whose first-party models lack OpenAI's hosted `web_search` tool reject it with "tool type 'web_search' is not supported by this gateway phase" (`responses_feature_not_supported`), and Codex sends the tool by default (config-driven, not gated by the catalog's `supports_search_tool`), producing hard 400s. cc-switch now writes the top-level TOML line `web_search = "disabled"` for those vendors via `set_codex_native_web_search_field`, injected alongside `model_catalog_json` at switch time. Scope is a blacklist (default-on): only providers matched by `base_url` host — `CODEX_WEB_SEARCH_REJECT_HOSTS = xiaomimimo.com, longcat.chat, minimax.io, minimaxi.com` — or by model brand prefix — `CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES = mimo, longcat, minimax, qwen3-coder` — are disabled, so relays serving real GPT, DouBao, general Qwen, and any unknown provider keep Codex's default. The `qwen3-coder` prefix suppresses the tool for the native `qwen3-coder-plus` direct-connect preset (百炼/DashScope marks built-in tools unsupported for the coder series) while general Qwen models sharing the DashScope host stay enabled — matching is on the model axis (after stripping any aggregator `vendor/` path segment like `MiniMaxAI/MiniMax-M3` or `qwen/qwen3-coder-plus`), so it also catches aggregators such as SiliconFlow fronting a reject vendor's model. A blacklist was chosen over a fuzzy "is this GPT?" whitelist because wrongly keeping `web_search` ON fails with a hard 400, and an ownership sentinel means cc-switch only ever removes a `web_search` key whose value equals its own `disabled`, so existing providers need no re-save and switching back re-enables web search. Also corrects the LongCat-2.0-Preview preset context window from 131072 (128K) to its real 1048576 (1M), aligning with the MiMo/Qwen 2^20 convention, and tightens the native Responses preset tests to assert exact model→contextWindow catalogs instead of only checking catalog presence.
- **Strip All Credential-Like Keys From the Shared Claude Common-Config Snippet**: `extract_claude_common_config` previously only redacted `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN`, but Claude providers legitimately carry other credentials (`OPENROUTER_API_KEY`, `GOOGLE_API_KEY`, and possibly OpenAI/Gemini/AWS Bedrock/Vertex secrets), which could leak into the shared snippet and then be injected into other providers. Extraction now pattern-matches and strips any credential-shaped env key (`*_API_KEY` / `*_AUTH_TOKEN` / `*secret*` / `*token*`, etc.), while preserving plural `*_TOKENS` values like `MAX_OUTPUT_TOKENS` as legitimately shareable. This also closes the same pre-existing leak in the manual Extract and one-time auto-extract paths, and is covered by a dedicated credential-stripping unit test.
- **Usage-Script Credentials Persisted Only as Explicit Overrides**: Provider usage scripts store optional `api_key`/`base_url` fields that override the provider's live credentials when querying quota, but these were silently mirroring the provider's own credentials — so copying a provider or editing its main API key/base URL left the usage script pinned to the old endpoint and key, and quota queries kept hitting the stale target. `ProviderService::add`/`update` now run `normalize_usage_script_credential_overrides` before persisting: if the script's trimmed `api_key` or slash-normalized `base_url` matches the provider's resolved usage credentials (or is blank) it is cleared to `None` so the query falls back to `resolve_usage_credentials` from the live config, while genuinely distinct overrides are kept and `template_type == "token_plan"` scripts are left untouched. The deeplink import path gained matching `normalize_deeplink_api_key`/`base_url` helpers, and the frontend now invalidates `usageKeys.script(id, appId)` (including the `originalId` when a provider is renamed) on update so the homepage re-queries with the corrected config instead of the test-time cache. (#4654)
- **Hermes Config Dir Now Resolves Correctly on Windows**: CC Switch hardcoded `~/.hermes` as the Hermes config directory, but Hermes itself resolves it via `get_hermes_home()` — the `HERMES_HOME` env var, then a platform default of `%LOCALAPPDATA%\hermes` on Windows (`~/.hermes` on mac/Linux). On Windows this meant CC Switch wrote provider configs to a path Hermes never reads, so provider switches had no effect. `get_hermes_dir()` now mirrors Hermes' own resolution order — `settings.hermes_config_dir` explicit override, then `HERMES_HOME` taken verbatim (trimmed, non-empty, no `~` expansion, matching Hermes' `Path(val)`), then the platform default reading the actual `LOCALAPPDATA` env var (falling back to `~\AppData\Local\hermes`) on Windows and `~/.hermes` elsewhere. This deliberately re-honors the `HERMES_HOME` that #3470 had dropped, since unlike Codex/Claude the Hermes Windows installer sets `HERMES_HOME` as a first-class mechanism for relocated installs. Config-dir tests were also isolated from ambient `HERMES_HOME`/`LOCALAPPDATA`. (#4680, refs #3178, #3470)
- **Linux Wayland: Override the AppImage's Forced `GDK_BACKEND=x11`**: The AppImage's GTK launch hook (`linuxdeploy-plugin-gtk.sh`) unconditionally exports `GDK_BACKEND=x11` to dodge a historical native-Wayland crash (tauri-apps/tauri#8541). On newer Wayland + NVIDIA setups this forced XWayland leaves the WebKitGTK web content unable to receive pointer events — the GTK title bar stays clickable but the page is dead — and black-screens on resize; the existing `WEBKIT_DISABLE_*` mitigations don't help because the root cause is the forced window backend, not rendering. Since the hook also overrides any user-set `GDK_BACKEND`, there was no way to switch back without unpacking the AppImage. `main.rs` now reads an opt-in `CC_SWITCH_GDK_BACKEND` escape hatch (which the hook never touches) before GTK init: leaving it unset keeps the current x11 behavior unchanged (zero regression), while `CC_SWITCH_GDK_BACKEND=wayland` forces native Wayland. The override is generic, so users on tiling Wayland compositors hitting the inverse input bug can set `CC_SWITCH_GDK_BACKEND=x11`. (#4351, fixes #4350)
- **Get API Key Link Now Shows in Claude Desktop, OpenClaw, and Hermes Forms**: The "Get API Key" link and partner-promotion block below the API key input was only wired for claude/codex/gemini/opencode. Claude Desktop rendered a bare Input that never showed it, and OpenClaw/Hermes were blocked by two gaps: `useApiKeyLink` whitelisted only those four appIds (so the link was suppressed even when a preset carried `apiKeyUrl`), and `useProviderCategory` only parsed the `/^(claude|codex|gemini|opencode)-(\d+)$/` preset-id pattern (so OpenClaw/Hermes category stayed undefined and the cn_official/aggregator/third_party link condition never held). `ClaudeDesktopProviderForm` now calls `useApiKeyLink` and uses the shared `ApiKeySection`; the `useApiKeyLink` whitelist and its `PresetEntry` union add claude-desktop/openclaw/hermes; and `useProviderCategory` now resolves openclaw/hermes preset categories. Additionally, `HermesFormFields` and `OpenClawFormFields` no longer let an "official" category disable the key input, since these apps have no OAuth-only official providers (e.g. Hermes' Nous Research is official but still needs a user-supplied key).
- **Deduplicated Windows Codex npm Shims in Tool Detection**: On Windows, npm installs a tool as three sibling files — `codex.cmd`, `codex.exe`, and an extensionless Unix shim named `codex` — and CC Switch's `tool_executable_candidates` listed all three, so the extensionless shim (which Windows cannot execute directly) was probed as a redundant/failing candidate. `tool_executable_candidates` now appends the extensionless path only when `windows_runnable_sibling_for_extensionless_tool` finds no adjacent `.cmd`/`.exe` sibling, and `resolve_path_default` likewise prefers a runnable `.cmd`/`.exe` sibling before canonicalizing a bare extensionless PATH hit. This keeps version detection and launching anchored to the actually-runnable Windows shim instead of the shadowed Unix one. (#4782)
- **Scroll Bounds for Long Select Dropdowns**: The `SelectContent` popover used `overflow-hidden` with no height cap, so dropdowns with many options (e.g. long model or provider lists) rendered taller than the viewport and clipped their overflowing items with no way to reach them. It now sets `max-h-[min(24rem,var(--radix-select-content-available-height))]` and `overflow-y-auto overflow-x-hidden`, bounding the content to 24rem or the Radix-computed available height and letting the list scroll vertically while still clipping horizontally. (#4798)
- **Date-Range Picker Calendar Stays On-Screen in Narrow Popovers**: The custom date-range picker switched to its two-column layout (date fields | calendar) based on the viewport width via Tailwind's `sm:` (640px) breakpoint, but the popover is clamped to `100vw - 2rem` and anchored to its trigger with `align="end"`, so its real available width is narrower than the viewport. On narrow windows the two-column layout could activate while the popover only had room for one column, pushing the calendar column off the right edge where it was clipped — the month header and 4 of 7 weekday columns cut off and unreachable. The layout now keys off the popover's own inline size via a CSS container query (`w-[620px] max-w-[calc(100vw-2rem)]`) instead of the viewport, so it collapses to one column exactly when the popover itself is narrow, keeping the calendar fully visible at any window width. (#4860)

### Docs

- **Documented the `CC_SWITCH_GDK_BACKEND` Escape Hatch in README and User Manual**: Added an FAQ entry for the opt-in `CC_SWITCH_GDK_BACKEND` environment variable (see the corresponding Fixed entry) across all four README locales (`README.md`, `README_ZH.md`, `README_JA.md`, `README_DE.md`) and the zh/en/ja user-manual troubleshooting pages (`docs/user-manual/{zh,en,ja}/5-faq/5.2-questions.md`), explaining how Wayland + NVIDIA users can switch back to native Wayland when the webview goes click-dead and black-screens on resize, and how tiling-Wayland users can set it to `x11` for the inverse input bug.
- **Overseas Kimi READMEs Point to platform.kimi.ai With a Coding Plan Link**: Updated the Kimi K2.7 Code partner section across the English, German, and Japanese READMEs so the banner image and both inline CTAs point to `https://platform.kimi.ai?aff=cc-switch` instead of the old `platform.kimi.com` host, keeping the `aff=cc-switch` referral tag intact. All four localized READMEs (`README.md`, `README_DE.md`, `README_JA.md`, `README_ZH.md`) also gained a new line promoting the Kimi For Coding subscription plan linked to `https://www.kimi.com/code/?aff=cc-switch`; note the ZH README kept its existing `platform.kimi.com` link and only received the Coding Plan addition.
- **GitHub Global Top-100 Milestone Banner in v3.16.4 Release Notes**: Prepended a celebratory callout blockquote to all three localized v3.16.4 release notes (`docs/release-notes/v3.16.4-en.md`, `-ja.md`, `-zh.md`) announcing that CC Switch has entered the global top 100 GitHub projects by star count, thanking users, contributors, and stargazers. This is a docs-only addition to the release-notes header and does not change any product behavior.

## [3.16.4] - 2026-06-27

Development since v3.16.3 focuses on tightening the Codex proxy path — native OpenAI Responses migration for the major Chinese providers, a decoupled upstream-format selector, zstd request/error-body decompression, and a run of tool-call and OAuth-over-proxy fixes — alongside richer usage and pricing tooling (models.dev pricing import, Volcengine Ark coding/agent-plan quotas, live-tracking date ranges, GLM-5.2/Doubao Seed 2.1 pricing), new proxy and resilience capabilities (custom request header/body overrides, an in-app recovery screen for too-new databases, native Windows ARM64 builds), and a broad wave of preset and branding updates (the SubRouter and OpenCode Go subscriptions, the CTok→ETok rename, Kimi rebranding and prime-partner badges, and a Kimi K2.7 Code sponsor banner).

**Stats**: 53 commits | 126 files changed | +8,149 insertions | -1,016 deletions

### Added

- **In-App Recovery Screen for a Too-New Database**: When the SQLite `user_version` is newer than the app supports (`SCHEMA_VERSION`) — e.g. after a downgrade or because a third-party client wrote the file — startup used to dead-end in a native Retry/Exit dialog where Retry just failed again. The app now boots a dedicated recovery screen offering an in-place "Upgrade app" button (download + install + restart with a progress bar) when an update is available, or a warning that even the latest build can't read the database when none is. The too-new check runs before any schema writes so the app never runs DDL against a database it can't understand, and native-close quits cleanly in recovery mode where no tray exists. (#4575)
- **Local Proxy Request Overrides (Custom Headers and Body)**: Provider configs can now define custom request headers and request-body overrides that the local proxy applies when forwarding, exposed via a new field in the Claude and Codex provider forms. Inputs are validated, including a protected-header-name list that blocks overriding security-sensitive headers. (#4589)
- **Volcengine Ark Coding/Agent Plan Usage Query**: Usage panels can now query coding-plan and agent-plan quota for Volcengine Ark. Because the Ark control-plane OpenAPI (`open.volcengineapi.com`) requires account-level AccessKey signing rather than the inference API key, the usage script gains a dedicated AK/SK input block, with a clickable link straight to the Volcengine IAM key-management console (`https://console.volcengine.com/iam/keymanage`), and the proxy implements Volcengine Signature V4 (an AWS SigV4 variant with fixed canonical header order, `HMAC-SHA256` algorithm, and `ark` service scope). It auto-detects the plan by probing `GetAFPUsage` (Agent Plan five-hour/weekly/monthly quotas) before falling back to `GetCodingPlanUsage`, parses the window label from the `Level` field (guarding `ResetTimestamp <= 0`), and adds a `monthly` tier label across the footer, tray menu, and all four locales.
- **Import Model Pricing from models.dev**: The Add Pricing panel gains an "Import from models.dev" button that fetches `https://models.dev/api.json`, lets users full-text search the catalog, and imports the selected entry through the same `update_model_pricing` path as manual entry. Imported model IDs are normalized to match the backend's `clean_model_id_for_pricing` rules (strip vendor prefix, lowercase, drop `:` suffix, map `@` to `-`, drop the `[1m]` marker) so stored rows actually match cost-attribution lookups. A companion fix makes the scoped zero-cost backfill match raw model aliases (route prefixes, `:free` variants, date suffixes) in Rust instead of by exact SQL string, so newly priced alias rows get costed immediately rather than waiting for the next startup backfill (Fixes #4017). (#4079)
- **Windows ARM64 Release Builds**: Releases now include native Windows ARM64 artifacts so ARM-based Windows devices get a matching build instead of relying on x64 emulation. The release matrix also runs each platform independently (fail-fast disabled) so a missing-secret failure on one job — e.g. macOS signing in forks — no longer cancels its siblings before they finish. (#3950)
- **Live End Time for Custom Date Ranges**: The custom date-range picker gains an "End time follows current time" checkbox; when enabled the end time becomes read-only and tracks the current moment, so usage data always reflects up-to-the-second consumption from the chosen start. This is especially useful for watching real-time token use within a Coding Plan 5-hour quota window. `liveEndTime` is included in the React Query cache keys so a live range and a fixed range with the same stored endpoints no longer collide on a stale cache entry. (#4438)
- **Source File Name in Session Detail Header**: The session detail header now shows the session log's file name (with the full path on hover and click-to-copy) alongside the project directory, so users can locate and open the underlying JSONL file directly from the UI. Long, space-less basenames such as ~70-char Codex rollout files are truncated at `max-w-[200px]` to keep them from overflowing into the action-button area on narrow windows. (#4113)
- **Unmanaged-Skill Indicator on Import Button**: The top-bar Skills Import button now shows a green dot with a tooltip when local unmanaged skills are available to import, so you can tell at a glance that on-disk skills aren't tracked yet. The scan runs once on mount and is shared across navigations (30s `staleTime` + `keepPreviousData`) to avoid repeated disk IO.
- **OpenCode Go Subscription Presets**: New OpenCode Go (`opencode.ai/zen/go`) presets for Claude, Codex, and OpenCode, authenticated with a plain pasteable API key (no OAuth). The Codex preset uses `openai_chat` conversion with a GLM/Kimi/DeepSeek/MiMo model catalog (and no static `codexChatReasoning`, so per-model capability is inferred), while OpenCode targets `/zen/go/v1` via `@ai-sdk/openai-compatible`. All four OpenCode Go presets — Claude, Claude Desktop, Codex, and OpenCode — carry a referral link and an in-app promo phrase; the promo banner is now gated on `partnerPromotionKey` alone rather than `isPartner`, so a preset can show a referral promo without earning the gold paid-partner star (this also re-surfaces the existing MiniMax promos).
- **SubRouter Partner Provider**: Added SubRouter (`subrouter.ai`), an AI relay aggregator that exposes many models and providers behind a single key, as a preset across all seven managed apps — the Anthropic-format endpoint for Claude Code / Claude Desktop / OpenClaw / Hermes, the OpenAI-compatible `/v1` endpoint with `gpt-5.5` for Codex and OpenCode, and the Gemini-compatible `/v1beta` endpoint with `gemini-3.5-flash` for Gemini CLI — carrying its own brand icon, the gold partner star, four-locale promotion copy, and the affiliate registration link (`?aff=l3ri`) prefilled as the API-key signup URL. (#4522)
- **Prime-Partner Preset Badge and Ordering**: First-party Moonshot Kimi presets (Kimi / Kimi For Coding / Kimi K2.7 Code) are now flagged as prime partners: instead of the gold star, they render a solid gold heart with no badge frame, and in the default (Original) sort they float to the top right after official-category presets, ahead of the rest. Grouping is a three-way partition so each group keeps its internal order and an official preset also flagged prime-partner stays only in the official group.
- **Pricing for GLM-5.2 and Doubao Seed 2.1**: Seed model pricing now includes GLM-5.2 (#4385) and Doubao Seed 2.1 Pro/Turbo, so usage from these models is cost-attributed correctly instead of recording zero cost. Doubao prices use Volcengine's official list price (CNY converted at ~7.14); `cache_creation` is kept at 0 because Doubao bills cache storage by time rather than per-token writes, and the existing 2.0 rows are retained for historical accounting.
- **Kimi For Coding Auto-Compact Window**: The Kimi For Coding preset now sets `CLAUDE_CODE_AUTO_COMPACT_WINDOW` to a default of 262144 to match the official Kimi docs, exposed via `templateValues` so users can customize the value for future models or performance tuning. (#4401)

### Changed

- **Native Responses API for CN Codex Providers**: Several Chinese providers (Qwen/DashScope Bailian, Xiaomi MiMo, Volcengine Doubao, Meituan LongCat, MiniMax CN/intl) now expose a native OpenAI Responses endpoint, so their Codex presets switch to `apiFormat: "openai_responses"` and reach the upstream directly instead of going through the Responses->Chat route-takeover conversion. Dropping the now-unused `codexChatReasoning` and `modelCatalog` also keeps the "local route mapping" toggle unchecked by default. SiliconFlow-hosted MiniMax stays `openai_chat` since it is a third-party endpoint rather than MiniMax's own base_url. Stale model ids on the remaining chat-only providers were refreshed as well (GLM 5.1->5.2, StepFun 3.5-flash-2603->3.7-flash, Ling 2.5-1T->2.6-1T).
- **Decoupled Upstream Format Selector from Model-Mapping Toggle**: The Codex provider form used to tie Chat-format conversion and route takeover (model mapping) to a single toggle, so a provider serving a native Responses API could not use model mapping without forcing Chat Completions conversion. The upstream format (Chat Completions / Responses) is now an independent, always-visible selector, while the local-routing toggle solely gates the advanced sub-sections (model mapping catalog, plus reasoning capability when the format is Chat). Its initial state is derived from saved catalog presence with no new persisted field, and the `codexConfig` i18n strings were reworded across all four locales (zh/en/ja/zh-TW).
- **Doubao Seed 2.1 Pro Preset**: The DouBaoSeed preset now targets `doubao-seed-2-1-pro` (replacing `doubao-seed-2-0-code-preview-latest`) across all six clients (claude, claude-desktop, codex, opencode, openclaw, hermes), with display names updated to "Doubao Seed 2.1 Pro" and the OpenClaw cost field corrected from 0.002/0.006 to 0.84/4.2 USD per 1M tokens to match the new model.
- **CTok Rebranded to ETok**: Following the vendor's domain, endpoint, and trademark rename, all user-facing branding moves from CTok to ETok (`ctok.ai` -> `etok.ai`, `api.ctok.ai` -> `api.etok.ai`, internal id, display name, icons, and README partner banners) across every client preset. The Codex history-migration whitelist keeps `ctok` as a legacy id alongside the new `etok` so existing users' local session history stays correctly bucketed after the rename.
- **Consistent Kimi Preset Naming**: The OpenCode and OpenClaw Kimi presets, previously labeled "Kimi K2.7 Code", are renamed to plain "Kimi" (along with the OpenCode provider display name) to match the other apps; the model label stays "Kimi K2.7 Code" since it describes the actual model.
- **Dark Mode for JSON Editors**: The CodeMirror `JsonEditor` in the usage-script modal, provider form, and universal provider form now follows the app theme via `useDarkMode()`, switching to the `oneDark` editor theme instead of staying light while the rest of the app is dark. (#4556)
- **Tighter Add Provider Header With Footer Hint**: The Add Provider dialog reduces the title-to-tabs and tabs-to-card vertical gaps from 24px to 12px and adds an always-visible pinned footer hint guiding users to fill in the fields below after choosing a preset. `FullScreenPanel` gains an optional `contentClassName` prop so the padding override is scoped to this panel without affecting others that share it.
- **Theme-Adaptive Kimi Logo**: The inline Kimi placeholder mark is replaced with the vendor's refreshed logo. The K glyph uses `currentColor` so it follows the theme text color (dark in light mode, white in dark mode), while the brand accent dot is pinned to the new `#1783FF`, with the metadata fallback color aligned accordingly.
- **Fable 5 Verified Banner Removed**: The Settings About page no longer shows the Fable 5 Verified commemorative banner that 3.16.3 added beside the app name to mark the special build; the banner image and its markup are dropped, returning the About panel to its standard version-badge layout.

### Fixed

- **Copilot/Codex OAuth Requests Now Honor the Global Proxy**: `CopilotAuthManager` and `CodexOAuthManager` hardcoded `Client::new()` at construction, so their auth flows (token exchange, `/models` listing, model-vendor checks, device-code and OAuth-refresh requests) ignored the configured global proxy and connected directly. With Copilot the direct connection returned zero Claude models, breaking live model resolution and causing the upstream to reject requests with `400 model_not_supported`. Both managers now fetch the shared client per request via `crate::proxy::http_client::get()`, so they follow the global proxy URL and pick up runtime proxy changes. Fixes #2016, #2931. (#4583)
- **Compressed Request and Error Body Decompression**: Codex Desktop sends zstd-compressed request bodies when authenticated against the Codex backend, which broke local proxy routing because the handlers parsed the raw compressed bytes with `serde_json` directly. The proxy now decompresses request bodies (gzip/br/deflate plus new zstd support, including stacked codings like `gzip, zstd`) before JSON parsing across the three Codex handlers and strips the stale `content-encoding`/`content-length`/`transfer-encoding` headers so the forwarder regenerates them. Upstream non-2xx error bodies are decompressed the same way, so compressed rate-limit and auth details are no longer dropped and hidden from the client. Fixes #3764, #3696. (#3817)
- **DeepSeek Endpoint `thinking: disabled` 400 Errors**: DeepSeek's Anthropic-compatible endpoint rejects requests where `thinking.type=disabled` coexists with effort parameters, returning HTTP 400, which broke Claude Code 2.1.166+ sub-agents (Workflow/Dynamic Workflow) that hardcode `thinking: disabled`. Rather than overriding the client's intent, the proxy now strips the conflicting `output_config.effort` / `reasoning_effort` parameters for the official DeepSeek endpoint, since sub-agents don't need to display reasoning. (#4239)
- **Reverted Anthropic System-Message Hoisting**: Reverts #3775's hoisting of `role=system` messages out of `messages[]` into the top-level `system` field for Anthropic-compatible providers. DeepSeek's endpoint accepts inline system messages natively, and the rewrite altered the request prefix; leaving the messages in place preserves the prompt prefix and avoids a suspected cache hit-rate regression (refs #4297). The unrelated Windows test fixes and the tool-thinking-history normalization from #3775 are kept. (#3775)
- **Chat Tool Calls with Missing Function Names**: Some upstreams send empty or absent function names in streaming tool-call deltas, which previously produced invalid Codex Chat output items (or an `unknown_tool` fallback). Accumulated tool-call state is no longer overwritten by empty deltas, and tool calls that never receive both a `call_id` and a valid name are skipped at finalization across the streaming, non-streaming, and legacy `function_call` paths. (#4159)
- **Restored Cached Codex Tool Call Fields**: When Codex sends a follow-up Chat request referencing `previous_response_id`, its `function_call` items can arrive carrying only `call_id`. The history enrichment previously refilled only `reasoning`/`reasoning_content`, leaving the function `name`, `arguments`, `status`, and related fields empty; it now restores all cached tool-call fields from history so the call is reconstructed correctly for the Chat upstream. (#4160)
- **Duplicate Codex base_url Entries in config.toml**: Writing the Codex `base_url` into `config.toml` only replaced or removed a single matching assignment per section, so a section that already contained multiple `base_url` lines kept the extras and accumulated duplicates. `setCodexBaseUrl` now collapses all matches in the target section or top level (replacing the first and removing the rest), and the TOML `base_url` regex handles escaped quotes. (#4316)
- **CODEX_SQLITE_HOME State DB Probing for History Migration**: The Codex session-history migration only scanned `~/.codex/state_5.sqlite` and the `config.toml` `sqlite_home` location, so when Codex's SQLite state was relocated via the `CODEX_SQLITE_HOME` env var the state DB was never scanned and its threads kept their old provider bucket. The shared `codex_state_db_paths` helper used by both the third-party and unified-session migrations now falls back to `CODEX_SQLITE_HOME` (config `sqlite_home` still takes precedence).
- **Provider Terminals Respect the User's Shell**: Launching a provider terminal on macOS/Linux hardcoded `bash`, so zsh/fish users' rc files never loaded. The launchers now detect the user's default shell from `$SHELL` (falling back to `/bin/zsh` on macOS, `/bin/bash` on Linux) and exec into it with clean-start flags, while the launch scripts themselves run through POSIX `sh` for portability (e.g. fish, NixOS where `/bin/sh` may not exist). (#4140, fixes #1546)
- **Claude MCP Path Honors Custom Config Dir**: When a custom Claude config directory is configured, MCP server reads and writes now resolve to that directory's MCP file instead of the default location, keeping MCP state isolated per profile. The previous copy-on-access migration of the legacy file was removed in favor of resolving the override path directly. (#3431)
- **Preset Search Results Clickable After Searching**: After searching in the Add Provider preset selector, results could no longer be clicked or selected. The `requestAnimationFrame` `select()` that raced with typing (and ate the first character, e.g. "gateway" -> "ateway") is removed, input autofocus is restored for the open-by-click path, and refocus is wired up for the Ctrl/Cmd+F shortcut while the box is open. The provider-list typing guard is also scoped to the Ctrl/Cmd+F branch so Escape still closes the search panel. (#4315)
- **Skills Browser and Provider Card Display Fixes**: Fixed several display and interaction issues: the repo-manager action stays available while browsing skills.sh and Refresh stays available even when a repo returns no results; long provider names and website URLs on the provider card now truncate instead of overflowing; the OMO model-variant dropdown truncates its selected label with a full-text tooltip; and Select menu items show a checkmark on the active option. (#4323)
- **Settings Scroll Resets on Tab Switch**: Switching tabs in the Settings dialog kept the previous tab's scroll position, sometimes landing partway down the new tab; the scroll container now resets to the top whenever the active tab changes. (#4165)

### Docs

- **Kimi Pinned Sponsor Banner**: The pinned sponsor banner at the top of all four README locales (en/zh/ja/de) now features Kimi K2.7 Code in place of the previous MiniMax M2.7 banner. The copy reflects the K2.7 Code release (a coding-focused agentic model that reduces thinking-token usage roughly 30% versus K2.6), the banner is served from in-repo assets (`assets/partners/banners/kimi-banner-en.png` / `kimi-banner-zh.png`) instead of the Moonshot CDN, and a clickable call-to-action links to the `aff=cc-switch` Moonshot console.
- **Codex Unified Session-History Guide**: New trilingual (zh/en/ja) guide for the unified Codex session-history toggle, explaining what opt-in migration (on enable) and ledger-based restore (on disable) actually do, why session data is never truly deleted (tag-only rewrite plus automatic backups), and how to verify files on disk versus merely being filed under another provider drawer. It includes a symptom reference table for the common "my sessions are gone" misunderstanding plus on-disk verification commands for macOS/Linux/Windows, and is linked as the lead item in the v3.16.3 "Usage Guides" release notes.
- **Simplified Homebrew Install Instructions**: The installation guide no longer instructs users to run `brew tap farion1231/ccswitch` before `brew install --cask cc-switch`; the deprecated tap step was removed from the en/ja/zh user manuals so the cask installs directly. (#4319)
- **Star-History Global Rank Badge**: Added a star-history global rank badge next to the existing Trendshift badge in all four README locales, with light/dark theme variants.
- **Volcengine Coding Plan Campaign Link**: The "中国大陆地区的开发者请点击这里" link in the ByteDance/Volcengine sponsor entry now points to the Volcengine `ai618` campaign page instead of the previous `codingplan` referral URL, updated across all four README locales.
- **CCSub Sponsor Banner Vector Asset**: Replaced the low-res `ccsub.jpg` sponsor logo with a vector `ccsub.svg`, letterboxed from 2046x648 to 2046x850 (~2.406:1) so it matches the other sponsor-table banners and renders at the same 62px height. All four README locales point at the new asset.

## [3.16.3] - 2026-06-14

Development since v3.16.2 focuses on getting usage accounting right end-to-end — billing route-takeover and format-conversion traffic by the real upstream model and pricing basis (schema v11), counting Claude Code Workflow sub-agent sessions, folding Claude Desktop into the Claude view, refreshing the model pricing seed, and reworking the usage dashboard with global provider/model filters, brand-icon toolbars, and far more resilient quota queries — while hardening the proxy (mislabeled SSE bodies, Codex image rectification, OAuth token and takeover-residue recovery, Hermes duplicate YAML keys), reworking provider configuration (a custom User-Agent override, a unified Codex advanced section, searchable preset selection, a Fable 5 tier, and refreshed Kimi/Unity2/Volcengine/MiniMax presets), and smoothing the update, About-panel, and provider-health experiences.

**Stats**: 59 commits | 130 files changed | +10,223 insertions | -4,232 deletions

### Added

- **Custom User-Agent Override**: Provider configs can now set a custom User-Agent that the proxy applies consistently across request forwarding, stream check, and model listing (`GET /v1/models`), so coding-plan upstreams that gate on UA no longer fail detection or return 403 while the proxy itself works. The Claude and Codex forms expose it in advanced settings with a curated presets dropdown (Claude Code / Kilo Code families that pass UA whitelists) and live non-blocking validation; stale custom UAs are dropped when switching to an official preset to avoid silently altering headers (#3671).
- **Unified Codex Session History**: Official Codex sessions can now share a single resume-history bucket with cc-switch third-party sessions via an opt-in toggle under Settings → Codex App Enhancements, so the resume picker no longer hides them from each other. When enabled, the live `config.toml` routes official runs through a shared `custom` model_provider that mirrors the built-in OpenAI provider (`auth.json` is untouched); the toggle is forward-only by default but the enable dialog offers a checkbox to migrate existing official sessions (with per-generation backups), and the disable dialog offers a precise ledger-based restore that only reverts sessions originally recorded as `openai` while leaving sessions created during the toggle untouched.
- **Dashboard-Wide Provider/Model Filters**: The provider and model filters move from inside the request-log table up to the top bar, applying globally to the hero summary, trend chart, request logs, and both stats tabs so you can scope the whole dashboard to a given source and model. Sources match by exact display name (so session placeholder rows like "Claude (Session)" are selectable) and models match by effective pricing model, with the model dropdown cascading from the selected source and both lists showing only options that have data in the current range.
- **Refreshed Model Pricing Seed**: Added pricing for 9 models including Claude Fable 5, Grok 4.3, Mistral Medium 3.5 / Small 4, and Qwen 3.7 Max/Plus, and corrected 28 existing prices against current official vendor list pricing (GLM, Grok, MiMo, Doubao, Kimi, MiniMax, Mistral, Qwen) so usage cost estimates are accurate. Each change updates the seed for fresh installs and adds a guarded repair for existing databases without clobbering user-edited rows.
- **Claude Fable 5 Model Tier**: Provider forms now expose `claude-fable-5` as a fourth model-mapping tier on both the Claude Code and Claude Desktop proxy paths, with a fable → opus → default fallback mirroring the official downgrade and the `fable-` prefix whitelisted for the Desktop 1.12603.1+ validator. A clarified four-language fallback hint warns that leaving a tier blank on third-party endpoints forwards the literal model name and 404s (#3980, #4026, #4049).
- **Unity2.ai Partner Provider**: Added Unity2.ai, an AI API relay partner, as a preset across all seven managed apps (Claude Code, Codex, Gemini, OpenCode, OpenClaw, Claude Desktop, Hermes), each carrying the referral signup link and partner promotion copy in all four locales. Codex uses the bare base URL (the gateway exposes `/responses` at root) while OpenCode / OpenClaw / Hermes use the `/v1` chat-completions endpoint with `gpt-5.5`.
- **Kimi K2.7 Code Model**: Added the `kimi-k2.7-code` model (in $0.95 / out $4.00 / cache-read $0.19 per 1M tokens, 256K context) and pointed all six official Moonshot Kimi presets (Claude Code, Codex, Claude Desktop, Hermes, OpenCode, OpenClaw) at it, renaming the OpenCode / OpenClaw presets to "Kimi K2.7 Code". The pricing seed applies on startup via the idempotent insert path, so existing users pick up the new pricing without a migration.
- **Codex "Kimi For Coding" Preset Restored**: Re-added the Codex "Kimi For Coding" preset (`openai_chat`, `kimi-for-coding`, 256K context) with thinking mode enabled by default; it was previously removed because the coding endpoint rejects Codex's default `codex-cli` User-Agent with 403. It now works via proxy takeover combined with the custom User-Agent override (set to a whitelisted UA such as `claude-cli/*`).
- **Pricing-Model Audit in Request Detail**: The request detail panel now shows the requested model and the pricing model when they differ from the response model, making route-takeover bills auditable directly from the usage UI.
- **Preset Provider Search & Sorting**: The provider preset selector gains a searchable, sorted list with an inline search box (toggled via a magnifier icon, dismissed on ESC or outside click). Buttons use a responsive grid with consistent sizing and default icons, and search matches only provider display/raw names so URL fragments and shared category labels no longer produce noisy matches (#3975, #4183).
- **Claude Mythos 5 Pricing**: Registered the `claude-mythos-5` model in the bundled model/pricing table (in $10 / out $50 per 1M tokens, cache read $1.00, cache write $12.50), so usage metering prices and displays it correctly (#4077).
- **Fable 5 Verified Banner**: The Settings About page now displays a Fable 5 Verified banner beside the app name and version, marking this as a special build, with the version badge centered under the app name.

### Changed

- **Claude Desktop Usage Folded Into Claude**: The dashboard no longer shows a standalone "Claude Desktop" bucket, which only ever displayed a partial number (Desktop chat usage never passes through the proxy and its Code-tab sessions write into the shared `~/.claude/projects` tree). Desktop proxy traffic is now folded into the `claude` view for display while still recorded under its own `app_type` for route-takeover billing audit, with the real value visible in the request detail panel.
- **Lightweight Provider Health Check**: The provider health check no longer sends a real streaming model request (which many third-party providers blocked with 401/403/WAF, causing false negatives); it now performs a lightweight HTTP reachability probe of the provider `base_url`, treating any HTTP response as reachable and counting only DNS/connect/TLS/timeout as failure. The connectivity button is hidden for official providers (which use OAuth with an empty base URL and no reliable reachability target), the real-request confirmation dialog and test model/prompt fields are removed, and the degraded-latency threshold is set to 6s with an 8s timeout. The reachability check never resets the circuit breaker, so failover detection stays driven solely by real proxy traffic.
- **Codex Advanced Options Section**: The Codex provider form now folds local routing, model mapping, reasoning overrides, and custom User-Agent into a single collapsible advanced section mirroring the Claude form (auto-expanding when a UA is set or local routing is on). Custom User-Agent is now also configurable for native Responses providers, where it was previously reachable only with `openai_chat` routing enabled.
- **Usage Toolbar Refresh and Layout**: The app filter now renders brand icons (via ProviderIcon, with a grid icon for "All") instead of text tabs that wrapped awkwardly in narrow windows, and the usage hero shows the selected app's brand icon with Codex recolored to a neutral gray matching OpenAI's monochrome branding. The click-to-cycle refresh button becomes a Select with a localized "off" label, and the top-bar controls are compacted and aligned into consistent width groups with truncated long date-range labels.
- **Faster About Panel Loading**: The Settings About panel now loads progressively: the app version badge appears the instant it resolves instead of waiting for tool probes, each tool card updates the moment its own version check finishes (probes run concurrently rather than sequentially), and results are cached for the app session with a 10-minute TTL so reopening the About tab reuses cached values and revalidates stale ones in the background instead of re-probing all six tools every time.
- **Volcengine Ark Coding Plan Promo**: Updated the Volcengine Ark preset across all six apps with the new Coding Plan invite link (replacing the old Agent Plan / activity links) and refreshed the partner promotion copy in all four locales (two-month 75% off plus invite code 6J6FV5N2), correcting the product name from Agent Plan to Coding Plan.
- **MiniMax Demoted to Regular Provider**: Removed the gold partner star badge and the API-key promotion banner for MiniMax by dropping the `isPartner` flag from all its presets; it stays as a regular `cn_official` provider keeping its icon and theme. The promotion copy is kept dormant so the partnership can be re-enabled with a single line.
- **LemonData Removed, SudoCode Demoted**: Removed the LemonData provider preset entirely from all apps along with its promotion copy, icons, and sponsor listings, and demoted SudoCode from a partner to a regular `third_party` provider by dropping its `isPartner` flag and promotion copy (it keeps its icon).
- **AtlasCloud Codex GLM 5.1 Context Window**: Declared the 200,000-token context window for the `zai-org/glm-5.1` model in the AtlasCloud Codex preset, matching the other GLM 5.1 preset entries.

### Fixed

- **Route-Takeover Traffic Billed by the Real Upstream Model**: When a request was routed to a different upstream (env model mapping, Claude Desktop routes, Copilot normalization, Codex chat override), the proxy used to attribute and price usage by whatever model the upstream echoed back, recording kimi/glm tokens as `claude-*` and overstating cost roughly 5–25×. The forwarder now captures the real outbound model, attributes usage by upstream-echo then outbound then client alias, persists the actual pricing basis on every row (schema v11), and keeps that basis through cost backfill and 30-day rollup pruning; Claude Desktop traffic is now logged under its own `app_type` so its pricing overrides apply.
- **Usage Metering on Format-Conversion Proxy Paths**: Audited and fixed token/cache accounting across the proxy's format-conversion paths (Chat, Responses, and Gemini converted to Anthropic). The proxy now records the actually returned model, injects `stream_options.include_usage` so OpenAI-compatible upstreams emit usage in streaming, excludes `cache_read` and `cache_creation` from input on Claude←OpenAI paths to stop double-billing cache tokens, subtracts cached Gemini prompt tokens, still records fully-cached requests, and skips synthetic all-zero usage that previously inflated request counts (#2774).
- **In-App Update No Longer Hangs on Restart**: Installing an update from within the app no longer freezes on the "restarting" screen, leaving the new version installed but requiring a manual force-quit. The download-install-restart chain now runs entirely in the backend (a new `install_update_and_restart` command) with platform-aware install ordering and single-instance-lock teardown before re-exec, instead of depending on the old WebView to keep running JS after the app bundle was already swapped; exit requests are also classified so restart requests fall through to Tauri's default flow rather than deadlocking on the window-state plugin mutex (#4069, #4074).
- **Codex Upgrade No Longer Breaks the Install**: Upgrading Codex from the Settings "About" tab no longer leaves it throwing "Missing optional dependency @openai/codex-…" errors. The upgrade chain previously ran `codex update` first, which on an npm install is a bare reinstall that reports success even when the per-platform binary fails to land; Codex is now removed from the self-update-first path and a runnable check triggers an uninstall+reinstall self-heal (scoped to npm-managed installs) that actually re-lands the missing platform binary.
- **Codex OAuth Auth Token Preserved on Proxy Takeover**: Enabling proxy takeover for a Codex provider no longer strips the `ANTHROPIC_AUTH_TOKEN` placeholder, which previously broke Claude Code's login on hot-switches, fresh installs, and configs already stripped by older releases. The placeholder is now injected unconditionally for managed (non-Copilot) Codex providers, including URL-only ones; GitHub Copilot behavior (API_KEY only) is unchanged (#3789, #3784).
- **Takeover-Residue Recovery Across Config-Dir Switches**: Restarting the app after changing the config directory while proxy takeover is active no longer leaves Claude/Codex/Gemini pointed at a dead local proxy. The old instance now restores the taken-over live files before restarting, the first-run import refuses to persist a takeover placeholder as a provider, and SSOT restore validates that the current provider's config is free of placeholders before writing it back (#4076).
- **Mislabeled SSE Bodies in Format-Transform Fallback**: Requests routed through Claude/Codex format conversion no longer fail with an opaque 422 "Failed to parse upstream response" when a MaaS gateway force-streams a `stream:false` request and returns an SSE body under a non-SSE Content-Type. The proxy now sniffs for SSE on parse failure, aggregates the chunks into a single JSON, and runs the existing converter so clients still get a valid non-stream response; remaining parse failures are enriched with content-type, encoding, and body-snippet diagnostics, and deflate decoding now tries zlib before raw (#2234).
- **Duplicate YAML Keys in Hermes Config**: Hermes config writes no longer accumulate duplicate top-level keys (e.g. `mcp_servers`) that caused "Failed to parse Hermes config as YAML: duplicate entry with key" errors. Section replacement now strips all stale occurrences from the remainder instead of degrading into appends, the dedup safety net handles both LF and CRLF line endings, and healing keeps the last (newest) occurrence to match Hermes's own last-wins PyYAML semantics (#3267, #3633, #2973, #2529, #3310, #3762).
- **Usage Query Resilience and Error Clarity**: Usage cards no longer flip to red on a single transient blip: queries now retry once and keep showing the last successful result for up to 10 minutes on network/timeout/5xx failures, while deterministic failures (auth, empty key, unknown provider, 4xx) surface immediately and clear the snapshot so a stale quota can't resurface after credentials change. Native balance/coding-plan/subscription timeouts were raised from 10s to 15s for slow cross-border endpoints, and coding-plan now returns explicit "API key is empty" / "Unknown coding plan provider" errors instead of a blank failure.
- **Usage Script Provider Credential Resolution**: Custom JS-script usage queries resolved `{{apiKey}}` / `{{baseUrl}}` by guessing env fields only, so providers that store credentials elsewhere (e.g. Codex's `auth.OPENAI_API_KEY` plus `config.toml` base_url) always got empty values and failed despite being fully configured. Script queries and the test/preview now reuse the same per-app credential resolver as the native balance path, with explicit non-empty script values still taking precedence (#1479).
- **Claude Code Workflow Sub-Agent Usage Counted**: Local (no-proxy) session-log usage accounting missed Claude Code Workflow sub-agent traffic, under-counting overall usage by roughly 4.1% (concentrated in workflow/subagent transcripts). The scanner now descends into the deeper `subagents/workflows/wf_*/` transcript directories, and the parser no longer drops billable assistant messages that lack a `stop_reason` but already incurred input/cache token cost; dedup is unchanged so no usage is double-counted.
- **Codex Image Rectifier for /responses Text-Only Upstreams**: Codex `/responses` requests carrying images and routed to text-only OpenAI-chat models (e.g. DeepSeek `deepseek-v4-flash`) no longer fail with HTTP 400 "unknown variant `image_url`". The media rectifier now also covers the Codex adapter, scanning the responses `input` for `input_image` blocks so it can proactively strip images for known text-only models and reactively retry with images replaced on upstream image-unsupported errors.
- **Zhipu Coding-Plan Quota Window Mislabeling**: The Zhipu coding-plan view no longer swaps the 5-hour and weekly quota buckets in the final hours of each weekly cycle. The two windows are now classified by the explicit `unit` field (3 = 5-hour, 6 = weekly) instead of by sorting reset-time ascending, which mislabeled them exactly when users check their weekly quota most; the old reset-time heuristic remains as a fallback (#3036).
- **Duplicate Provider Terminal Sessions on macOS**: Launching a provider terminal on macOS no longer opens an extra empty window alongside the command session; Terminal.app uses `launch` (not `activate`) on cold start and Ghostty uses an initial-command so a single session opens, with a fallback retained if the AppleScript path fails (#4156).
- **Claude Desktop Model-Mapping Placeholders**: The Claude Desktop model-mapping form previously showed mismatched example brands across the menu display name and request model columns (DeepSeek vs Kimi), implying a display name maps to an unrelated model. Both placeholders are now derived from each row's role so they stay brand-consistent, with the lightweight Haiku tier using a flash example.
- **Popovers Behind Fullscreen Panels**: Popovers and tooltips such as the provider preset search no longer render behind fullscreen panels and appear unresponsive on click; their z-index is raised above the fullscreen overlay while staying below modal dialogs.
- **ToggleRow Icon Shrinking**: Toggle row icons no longer shrink or distort when paired with long descriptions, keeping the icon at a fixed size next to multi-line text.

### Docs

- **Release Notes Contributor Mentions**: Restored contributor mentions in the v3.16.1 and v3.16.2 release notes across all three locales.

## [3.16.2] - 2026-06-07

Development since v3.16.1 focuses on broadening data portability and usage observability — S3-compatible cloud sync, OpenCode session usage import, and an opt-in official-subscription quota template — while hardening Codex Chat Completions routing (stream truncation, `tool_choice` / custom-tool / reasoning-token edge cases, file and audio attachments, and a Codex CLI models endpoint), strengthening proxy robustness (ephemeral ports, takeover/placeholder restore, system-message normalization, clearer upstream errors, and a text-only image fallback), fixing coding-plan quota lookups (Zhipu, MiniMax) and several Windows/macOS issues, adding the CherryIN and ZenMux providers, and refreshing the user manual.

**Stats**: 41 commits | 132 files changed | +11,116 insertions | -1,636 deletions

### Added

- **S3-Compatible Cloud Sync**: Cloud Sync now supports S3-compatible object storage as a second backend alongside WebDAV, using hand-rolled AWS Signature V4 signing for broad compatibility. The settings panel offers one-click presets for AWS S3, MinIO, Cloudflare R2, Alibaba Cloud OSS, Tencent Cloud COS, and Huawei OBS plus a custom endpoint, with connection testing, manual upload/download, and auto-sync on configuration changes (provider, endpoint, MCP, prompt, skill, settings, and proxy tables — not high-frequency data like usage logs); enabling S3 sync disables active WebDAV sync and vice versa (#1351).
- **OpenCode Session Usage Sync**: Added OpenCode as a usage-statistics source that imports per-message token, cost, and model data from OpenCode's local SQLite database, with a new "OpenCode" app filter tab and an "OpenCode Session" data-source label. The database path respects `OPENCODE_DB` and `XDG_DATA_HOME` (defaulting to `~/.local/share/opencode` on all platforms), only finalized messages are imported, and the freshness check accounts for the WAL file so newly written sessions are not skipped (#3215).
- **Official Subscription Quota Template**: Added an explicit, opt-in "official subscription" usage template for Claude, Codex, and Gemini official providers that queries plan quota via CLI/OAuth credentials, replacing the previous implicit auto-query. It is disabled by default and enabled from the usage-script modal with a configurable refresh interval.
- **Unsupported Image Fallback Rectifier**: Added a proxy rectifier that replaces Anthropic image blocks with an `[Unsupported Image]` marker when the routed model is text-only (declared, or detected via a built-in model-name heuristic) or when the upstream rejects image input, so conversations are not interrupted. A new Settings toggle controls the fallback, with a separate toggle for the heuristic detection.
- **ZenMux Token Plan Provider**: Added ZenMux as a Token Plan coding-plan provider that accepts a manually entered API key and base URL in the usage-script modal and renders its quota with USD-denominated used / limit values (#2709).
- **CherryIN Preset**: Added the CherryIN aggregator gateway as a quick-config preset across all seven supported apps — Anthropic-format endpoint for Claude Code / Claude Desktop / OpenClaw / Hermes, `@ai-sdk/anthropic` for OpenCode, the OpenAI-compatible endpoint for Codex, and the Gemini-compatible endpoint for Gemini CLI — with the official brand icon, placed next to AiHubMix (#3643).
- **CCSub Preset**: Added CCSub, a multi-model aggregator partner, as a quick-config preset across six apps — Claude Code, Claude Desktop, Codex, OpenCode, OpenClaw, and Hermes — with the official brand icon and the partner referral link prefilled as the API-key signup URL (`gpt-5.5` for the OpenAI-compatible Codex and OpenCode endpoints).
- **Codex CLI Models Endpoint**: The local proxy now answers `GET /v1/models`, which Codex CLI probes at startup, returning the cc-switch-managed Codex model catalog. A stale-catalog guard parses the live `config.toml` and only serves the catalog when `model_catalog_json` still references the cc-switch-owned file, so a leftover catalog from a previous provider is not advertised (#3818).
- **Codex Chat File and Audio Attachments**: The Codex Responses-to-Chat converter now maps `input_file` parts (carrying `file_id` or inline `file_data`) and `input_audio` parts into their Chat Completions equivalents, and emits top-level `input_*` items that were previously dropped, so file and audio attachments reach Chat-only Codex upstreams.

### Changed

- **Usage Dashboard Hero Redesign**: Restructured the Usage Dashboard hero and summary cards into a more compact layout, consolidating the real-token total, request count, and cost into a single top row (#3426).
- **SSSAiCode Endpoint Refresh**: Updated the SSSAiCode preset's website, signup, and API base URLs to the `sssaicodeapi.com` domain and refreshed its endpoint nodes (default `node-hk.sssaicodeapi.com`, plus `node-hk.sssaiapi.com` and `node-cf.sssaicodeapi.com`) across all seven app presets.

### Fixed

- **Codex Chat Truncated Stream Detection**: When a Chat Completions upstream ends a stream without a `finish_reason` or `[DONE]`, CC Switch no longer reports it as a normal completion — it finalizes normally only when the stream truly finished, emits an incomplete (`max_output_tokens`) response when partial output was produced, and emits a failed `stream_truncated` event when nothing was produced. Late-arriving reasoning is also attached to still-active streamed tool calls.
- **Codex Chat `tool_choice` Without Tools**: The Responses-to-Chat converter now drops `tool_choice` and `parallel_tool_calls` whenever the resulting tools array is absent or empty, so strict OpenAI-compatible upstreams (vLLM, enterprise gateways) no longer reject the request with "When using `tool_choice`, `tools` must be set." (#3640).
- **Codex Custom Tool Metadata Over Chat Routing**: Custom Codex tools (such as the freeform `apply_patch` tool) now preserve their full original definition — including format and grammar metadata — as a compact, order-stable JSON block in the generated Chat function description instead of a generic placeholder, keeping them usable on Chat Completions upstreams (#3644).
- **Codex Chat `reasoning_tokens` in Usage**: The Chat-to-Responses usage conversion now always includes `output_tokens_details.reasoning_tokens` (defaulting to 0), even when a provider omits `completion_tokens_details` or returns it as a non-object, satisfying the Codex CLI's strict requirement and avoiding repeated parse failures and retries (#3514).
- **Codex Cross-Turn Reasoning for Custom and Search Tools**: The cross-turn reasoning cache in Codex Chat history now covers the full tool-call set (`function_call`, `custom_tool_call`, `tool_search_call`) and their outputs, so `apply_patch` and tool-search calls keep their `reasoning_content` when restored via `previous_response_id`.
- **Ephemeral Proxy Port Resolution**: When the proxy listens on port 0 (OS-assigned), takeover now starts the proxy first to learn the real port and writes it into the Live configs and database, so client URLs no longer point at a broken `:0` address; the Claude Desktop gateway URL is rejected if no concrete port has been resolved.
- **Proxy Placeholder Backup/Restore Loop**: If a previous proxy stop left the proxy placeholders in Live, taking over again no longer overwrites a good backup with the proxy config, and restore no longer writes the placeholder back to Live — both paths detect the placeholder state and rebuild Live from the current provider, fixing cases where the proxy toggle became a no-op and clients stayed pinned to the local proxy (#3689).
- **Official Provider Block Under Proxy Takeover**: While Local Routing takeover is active, only providers explicitly categorized as official are blocked from switching, instead of also disabling custom providers whose endpoint lives in metadata or whose fields are unfilled. The disabled Enable button now shows a lighter hint tooltip in place of the red "Blocked" badge.
- **Localhost Listen Address Normalization**: Saving the proxy with a listen address of `localhost` now normalizes it to `127.0.0.1` before persisting, avoiding binding inconsistencies (#3016).
- **Anthropic System Message Normalization**: For Anthropic-format providers, system-role entries inside the `messages` array are collapsed and merged into the top-level `system` field (preserving order and any existing top-level system), preventing strict upstreams from rejecting non-leading system messages; OpenAI Chat routing is untouched (#3775).
- **Claude Desktop 1M-Context Model Routing**: Claude Desktop appends a `[1m]` marker to the model name when the 1M-context beta is active (e.g. `claude-opus-4-8[1m]`). The proxy now strips that suffix before route lookup so exact, alias, legacy, and role-keyword matching resolve correctly, fixing `route_unknown` (HTTP 400) failures when switching to a 1M-capable model mid-conversation.
- **Codex 413 Error Clarity**: When a Codex upstream gateway rejects an oversized request with HTTP 413, the proxy now returns a dedicated message identifying it as the provider's server-side body-size limit (not a CC Switch limit) with recovery steps (run `/compact`, drop large logs or inline images, or ask the provider to raise its limit), instead of echoing the raw upstream HTML page.
- **Proxy Panel Error Detail**: When toggling proxy takeover fails, the proxy panel toast now includes the underlying backend error detail instead of only a generic failure message (#3656).
- **Copilot Infinite-Whitespace Threshold**: Raised the streaming infinite-whitespace abort threshold from 20 to 500 consecutive whitespace characters, so legitimate tool calls with deeply indented code arguments are no longer falsely aborted while still catching the real Copilot infinite-whitespace bug (#2647).
- **Subscription Tier Tray Rendering**: Fixed tray and quota rendering for official subscription tiers via a unified tier-to-label mapping: Claude/Codex no longer drop the seven-day window, Gemini Pro/Flash/Flash-Lite tiers no longer leak raw machine names, and multi-window plans (e.g. Opus + Sonnet) now display the worst utilization instead of the first match.
- **Inflated Claude Stream Input Tokens**: Some Anthropic-compatible streaming providers (e.g. Qwen, MiniMax) report the full context as `input_tokens` in `message_start`, double-counting the cached portion and artificially lowering the displayed cache hit rate. The parser now prefers a smaller positive `input_tokens` from `message_delta` and adopts the paired cache counts from the same usage block; native Claude and OpenRouter-converted paths are unchanged.
- **Zhipu Quota Query Endpoint Routing**: The Zhipu coding-plan quota lookup was hard-coded to `api.z.ai`, so users on the mainland China preset (`open.bigmodel.cn`) could not retrieve usage when the international endpoint was unreachable. The quota request now routes to the host matching the user's configured base URL (#3702).
- **MiniMax Balance API and Pricing**: Adapted MiniMax coding-plan quota to its new balance API (which returns remaining-percent fields instead of usage counts that broke the old parser and left the tray blank), filtered out non-coding models (e.g. video), handled plans without a weekly limit, and seeded default pricing for MiniMax M3 (#3518).
- **GLM Coding Plan Endpoints and Model Fetch**: Corrected the ZhiPu / Z.AI GLM Coding Plan presets to the `/api/coding/paas/v4` endpoints across Codex, OpenCode, OpenClaw, and Hermes, and taught the model-list probe to query `{base}/models` for base URLs that already end in a `/v{N}` segment (keeping `/v1/models` as a fallback), so the Fetch Models button no longer 404s on versioned endpoints (#3524).
- **Codex Model Catalog Path Portability**: Codex now writes only the relative filename `cc-switch-model-catalog.json` to `config.toml` instead of an absolute path (Codex CLI resolves it from the config directory), fixing the model catalog breaking on WSL and symlinked setups where the absolute path could not be translated (#3614).
- **APINebula OpenCode SDK**: The APINebula OpenCode preset now loads `@ai-sdk/openai-compatible` instead of `@ai-sdk/openai`, so requests use the OpenAI Chat Completions format the relay expects rather than the Responses API.
- **Windows Tray Icon Residue on Exit**: Quitting CC Switch on Windows could leave a dead tray icon until hovered; the app now removes the tray icon before exiting so it disappears cleanly (#3797).
- **Windows Taskbar Icon**: Set an explicit Windows AppUserModelID at runtime and stamped the installer's desktop and start-menu shortcuts with the same ID and product icon, so CC Switch shows the correct icon and groups properly in the taskbar (#3457).
- **Windows Subdirectory Skill Updates**: Normalized backslash path separators to forward slashes when scanning installed skills on Windows, so skills nested in subdirectories (e.g. `skills/my-skill`) are matched by the update check instead of being silently skipped (#3430).
- **macOS Input Auto-Capitalization**: Disabled autocomplete, autocorrect, autocapitalize, and spellcheck on the shared text Input component so macOS no longer auto-capitalizes or auto-corrects the first letter typed into configuration fields (#3626).
- **Codex VS Code Session Previews**: Codex session previews for requests sent from VS Code could show selection or open-file content instead of the prompt when a markdown heading preceded the injected request. Both the backend title and frontend preview now match the last "## My request for Codex:" heading (the IDE injects the real request as the final section) (#3593).
- **VS Code Wording in Chinese UI**: Corrected the "Apply to Claude Code plugin" description in the Simplified and Traditional Chinese locales to write "VS Code" properly instead of "Vscode", aligning with the English and Japanese strings (#3228).

### Docs

- **User Manual Refresh**: Refreshed the README locales and the en / zh / ja user manuals to reflect all seven supported apps (adding Claude Desktop and Hermes), corrected the OpenCode config path to `~/.config/opencode/` (`opencode.json`), documented Hermes configuration files, updated the language docs to four languages, revised per-app MCP / Prompts / Skills availability, noted that export produces a timestamped SQL backup including usage logs, and documented the pricing model-ID matching rules (#3411).
- **Codex Official Auth Preservation Guide**: Added a trilingual (en / zh / ja) guide explaining how to keep Codex official remote control and plugins working while routing model traffic to third-party APIs, and linked it from the v3.16.1 release notes.
- **README Release-Note Links and Sponsor Markup**: Updated the Release Notes links in all README locales to point at v3.16.1 and fixed broken smart-quote characters in the README_ZH sponsor blocks so their HTML attributes render correctly (#3772).

## [3.16.1] - 2026-06-01

Development since v3.16.0 focuses on hardening Codex provider switching and Local Routing takeover: preserving official OAuth auth and model catalogs across normal switches, hot-switches, backup restore, and edit flows; restoring Codex Chat tool/plugin compatibility over Chat Completions upstreams; improving Codex proxy diagnostics and CLI discovery; and documenting DeepSeek routing.

**Stats**: 23 commits | 62 files changed | +5,603 insertions | -1,113 deletions

### Added

- **Codex Official Auth Preservation Setting**: Added an opt-in setting that keeps the official ChatGPT / Codex OAuth login in `auth.json` when switching third-party Codex providers, while moving third-party provider tokens into `config.toml` when enabled.
- **Codex DeepSeek Routing Guides**: Added localized DeepSeek routing guides for Codex in English, Chinese, and Japanese, with screenshots covering provider routing requirements, Codex provider setup, and Local Routing takeover.

### Changed

- **Codex Auth Preservation Is Opt-In**: The new official-auth preservation setting now defaults to off, so third-party Codex switches keep the legacy behavior of writing the active provider auth unless users explicitly enable preservation.
- **Codex Provider Switch Restart Hint**: Successful Codex provider switches now tell users to restart the Codex client so catalog and config changes take effect.
- **Codex Proxy Takeover Switching Is Serialized**: Provider switches and takeover toggles now share a per-app lock and use backup / live placeholder ownership signals instead of lagging `enabled` or server-running flags, preventing normal live writes from racing a just-activated or temporarily stopped takeover.
- **Codex Takeover Hot-Switch Display Refresh**: Hot-switching a Codex provider while Local Routing owns Live now refreshes the proxy-safe live provider id, model, and display name while keeping endpoints pointed at the local proxy.
- **Sponsor Ordering**: Swapped the Shengsuanyun and AICodeMirror sponsor blocks across README locales.
- **Docs Organization**: Moved the Chinese proxy guide under `docs/guides/` and removed the obsolete working-directory plan document.

### Fixed

- **Codex Provider Edit Dialog Under Takeover**: The Codex provider edit form now shows an explicit notice and storage-aware auth / config hints clarifying that it displays the stored provider config (not the proxy-managed live `auth.json` / `config.toml`), so the official OAuth token is no longer mistaken as lost while takeover is active. The dialog also treats takeover as active regardless of whether the proxy server is currently running.
- **Codex OAuth Auth During Proxy Takeover**: Fixed multiple preserve-mode takeover paths that could clear or overwrite the official ChatGPT / Codex OAuth `auth.json`. Takeover detection now recognizes `PROXY_MANAGED` in `config.toml`, cleanup only removes placeholder bearer tokens, config-only takeover writes are used consistently, and mis-categorized third-party providers no longer trigger the official-provider auth overwrite path. Provider sync and switching now treat the restore backup and live placeholders as the takeover-ownership signal (instead of the lagging `enabled` / proxy-running flags) and serialize switch/takeover per app, so a just-activated or proxy-stopped takeover can no longer be overwritten by a normal live write.
- **Codex Model Catalog Data Loss**: Fixed cases where Codex `modelCatalog` could be wiped by live-config backfill, active-provider edit dialogs, provider switches, or proxy takeover-off restore. Snapshot backups now keep existing `model_catalog_json` pointers, provider-rebuilt backups regenerate the catalog projection from the database source of truth, active edit dialogs prefer the DB catalog over lossy Live reconstruction, and provider switches always refresh the generated catalog JSON.
- **Codex Chat Tools Over Chat Completions Routing**: Restored Codex `tool_search`, loaded namespace tools, custom tools, and tool outputs when third-party Codex providers are routed through Chat Completions. Non-streaming and streaming Chat responses now map back to the right Responses item types, including native `response.custom_tool_call_input.*` events for custom-tool streaming.
- **Codex Proxy Error Diagnostics**: Codex forwarding failures now return richer JSON errors with provider, model, endpoint, upstream status, stable `cc_switch_*` codes, and HTTP statuses aligned with the canonical `ProxyError` response mapping.
- **Codex Native Balance / Coding-Plan Queries**: Fixed native usage and plan lookups so each app resolves the correct provider credentials instead of leaking assumptions from another app surface.
- **Codex CLI Discovery and Catalog Projection**: Fixed third-party Codex catalog projection that could fail when the Codex CLI was not reachable through one narrow PATH lookup, by adding multi-platform CLI discovery plus a bundled GPT-5.5 model-catalog template fallback.
- **Claude Desktop Official Provider Creation**: Fixed adding the Claude Desktop Official provider when the official category/config path was selected.
- **Anthropic Tool Thinking History for Kimi / Moonshot**: Added Kimi and Moonshot to the Anthropic-compatible tool-thinking history normalizer so later turns can replay reasoning and tool-call context correctly.
- **Windows Tool Version Probing**: Fixed Windows version checks that could misquote `.cmd` / `.bat` commands and decode localized command output as mojibake, causing working tools to appear as "installed but not runnable".

## [3.16.0] - 2026-05-29

Development since v3.15.0 focuses on making third-party Codex providers work like first-class citizens through Chat Completions routing, stabilizing Codex provider identity and history, adding an in-app managed CLI tool lifecycle, expanding the partner preset catalog, refreshing the default model / pricing matrix around GPT-5.5 and Claude Opus 4.8, and improving usage observability, localization, docs, and proxy robustness.

**Stats**: 101 commits | 221 files changed | +27,063 insertions | -3,052 deletions

### Highlights

- **Codex Chat Completions routing**: Codex providers can now be served by OpenAI-compatible Chat Completions upstreams. CC Switch converts Codex Responses requests into Chat Completions, rebuilds JSON and SSE responses back into Responses shape, preserves reasoning / `<think>` / tool-call state, normalizes error envelopes, and probes Chat-format providers correctly in Stream Check.
- **Codex third-party provider state is unified and safer**: third-party Codex providers now share the stable `custom` model-provider bucket, with a one-shot migration for historical JSONL sessions and `state_5.sqlite` threads, plus fixes that preserve OAuth login state, user-selected catalog models, and user-authored provider ids during live reads / switches.
- **Managed CLI tool management**: the About page is now a tool management panel for Claude, Codex, Gemini, OpenCode, OpenClaw, and Hermes, with install / update actions, update-all, conflict diagnostics, source-aware anchored upgrades, WSL handling, and visible "installed but not runnable" states.
- **Provider ecosystem and model matrix refresh**: added APIKEY.FUN, APINebula, AtlasCloud, SudoCode, Xiaomi MiMo Token Plan, and Claude Desktop Official presets; refreshed partner links and default models / pricing across apps; upgraded the default Claude Opus model line to 4.8 and GPT defaults to 5.5 where applicable.
- **Usage and docs polish**: Usage Dashboard updates now react immediately when logs are written, custom usage-script summaries and subagent session-log accounting were fixed, Traditional Chinese UI localization landed, and a German README plus expanded Claude Desktop / Codex Chat / tool-management manuals were added.
- **Proxy and conversion hardening**: fixed Codex Chat reasoning / cache / usage edge cases, DeepSeek Anthropic tool-thinking history, Claude-compatible empty `tool_calls` streams, managed-account takeover auth, MiMo reasoning output, Gemini Native tool-call replay, and several panic-prone proxy paths.

### Added

- **Codex Chat Completions Routing**: Codex providers can now be served by upstreams that only speak the OpenAI Chat Completions API. CC Switch's local proxy converts Codex's outgoing Responses requests into Chat Completions and rebuilds the Chat response (both JSON and SSE) back into Responses shape, preserving `reasoning_content`, inline `<think>` blocks, streamed reasoning summaries, tool calls, and `previous_response_id` follow-ups. A bounded Codex Chat history cache restores tool calls before their tool outputs.
- **22 Codex Third-Party Provider Presets with Chat Routing**: Enabled Chat Completions routing with explicit model catalogs for major Chinese/Asian providers — DeepSeek, Zhipu GLM (+ en), Kimi, MiniMax (+ en), StepFun (+ en), Baidu Qianfan Coding Plan, Bailian, ModelScope, Longcat, BaiLing, Xiaomi MiMo (+ Token Plan), Volcengine Agentplan, BytePlus, DouBao Seed, SiliconFlow (+ en), Novita AI, and Nvidia. Each preset declares its context window so the UI can size the model-mapping rows.
- **Codex Model Mapping Table**: Codex provider forms now expose a model catalog (model + display name + context window per row) that is the single source of truth for the upstream model list, projected to `~/.codex/cc-switch-model-catalog.json`.
- **Codex Chat Providers in Stream Check**: Stream Check now probes Chat-format Codex providers against `/chat/completions` with a Chat-shaped body instead of `/v1/responses`, and aligns its URL fallback order with the production `CodexAdapter` (origin-only base URLs hit `/v1/<endpoint>` first) so a non-404 error on the bare path no longer flags a working provider as down.
- **Codex Chat Reasoning Auto-Detection**: When a Codex provider is served through Chat Completions routing, CC Switch now auto-detects the upstream's reasoning interface from its name, base URL, and model — injecting the correct thinking parameter (`thinking:{type}`, `enable_thinking`, `reasoning_split`, top-level `reasoning_effort`, or OpenRouter's native `reasoning:{effort}` object) with no manual setup. Aggregator/hosting platforms (OpenRouter, SiliconFlow) are matched platform-first, since the same model can expose different reasoning controls on different platforms. Providers that only expose a thinking on/off switch (Kimi, GLM, Qwen, MiniMax, MiMo, SiliconFlow) drop the effort *level* instead of forwarding an unsupported field — so changing Codex's reasoning effort has no effect for them — while providers with real effort tiers (DeepSeek, OpenRouter, and StepFun's `step-3.5-flash-2603` only) pass the level through. OpenRouter specifically uses the native `reasoning:{effort}` object, clamps `max` to `xhigh` (its enum has no `max`), and forwards an explicit `effort:"none"` so reasoning can be turned off.
- **Codex Goal Mode and Remote Compaction Controls**: Codex config editing now exposes a Goal Mode toggle and a Remote Compaction toggle for third-party providers; new Codex templates default to `disable_response_storage = true` while still allowing explicit goal support.
- **Xiaomi MiMo Token Plan Presets**: Added Xiaomi MiMo Token Plan presets with specs aligned to the official documentation (#2803).
- **Claude Desktop Official Preset**: Added a Claude Desktop Official preset that restores the native Claude Desktop login, plus a localized Claude Desktop user guide (en / zh / ja).
- **Managed CLI Tool Lifecycle**: Added silent install / update commands for managed CLI tools, latest-version checks, per-tool and batch actions, update-all, and diagnostics for multiple installations across PATH, Homebrew, npm, pnpm, bun, volta, fnm, nvm, scoop, WinGet, Windows native paths, and WSL.
- **Source-Aware Tool Diagnostics**: The Settings / About surface can now diagnose conflicting tool installations, show the concrete install source and version for each path, and generate backend-planned upgrade commands anchored to the actual installation source.
- **Real-Time Usage Refresh**: The backend now emits `usage-log-recorded` when proxy logs, session-log syncs, or rollups write usage data; Usage Dashboard listens for that event and invalidates its queries immediately instead of waiting for the next polling interval (#3027).
- **Traditional Chinese Localization**: Added `zh-TW` UI localization and a settings language option (#3093).
- **German README**: Added `README_DE.md` and linked it from the existing README language switchers (#2994).
- **New Partner Presets**: Added APIKEY.FUN, APINebula, AtlasCloud, and SudoCode partner presets across the supported app surfaces, with partner copy, icons, and README entries.

### Changed

- **Codex Third-Party Providers Unified into a "custom" History Bucket**: Codex filters resume history by `model_provider`, so switching between provider-specific ids made past sessions appear to vanish. All third-party providers now normalize to a single stable `custom` bucket (reserved built-in ids like `openai` / `ollama` are preserved), with a one-shot device migration that rewrites historical JSONL sessions and the `state_5.sqlite` threads table and backs up originals under `~/.cc-switch/backups/codex-history-provider-migration-v1/`.
- **Codex Provider Form Simplified**: Removed the API Format selector from the Codex form (`wire_api` is always `responses`, so the selector misleadingly implied a protocol change); the model mapping table is now the only source of truth with no hidden default entries, and the form notes that a Codex restart is required after catalog changes since `model_catalog_json` is loaded at startup. Only the "Needs Local Routing" toggle remains.
- **Codex Local Routing Toggle Hints Rewritten**: Reframed the OFF / ON hints as action guidance (when to enable) rather than scenario descriptions, synced across zh / en / ja.
- **Codex Live Config Preservation**: Live Codex config reads no longer force-rewrite a user's `model_provider` field, and provider-scoped `experimental_bearer_token` handling now preserves OAuth login state when switching between third-party providers.
- **Tool Install / Upgrade Strategy**: Managed tool installation now prefers official native installers where available, falls back to package managers when appropriate, runs self-update first for compatible tools, anchors upgrades to the detected install source, and locks duplicate batch actions while work is in flight.
- **About Page Becomes Tool Management**: The About settings page now presents installed / latest versions, install and update actions, conflict diagnostics, WSL shell preferences, and clearer status for broken or unrunnable tools.
- **Default Models and Pricing Refreshed**: Upgraded the default Claude Opus model to 4.8, moved GPT-based presets and templates to GPT-5.5 where applicable, refreshed pricing seeds, aligned Claude Desktop model mapping with Claude Code's three-role tiers, and renamed the OpenCode Go preset to drop a stale model suffix.
- **Partner Links Refreshed**: Updated ShengSuanYun referral links, Atlas Cloud UTM links, and partner copy across README locales and provider metadata.
- **Homebrew Official Cask Installation**: Installation simplified to `brew install --cask cc-switch` now that CC Switch is in the official Homebrew repository; the personal-tap requirement was removed from all READMEs.
- **Shared Frontend Utilities**: Replaced JSON stringify / parse deep-copy patterns with a shared `deepClone` helper and extracted a shared `useTauriEvent` hook (#3140).

### Fixed

- **Codex Chat Error Responses Converted to Responses Envelope**: The Codex Chat-to-Responses bridge previously passed upstream error bodies through untouched, leaving Codex clients unable to recognize MiniMax `base_resp`, raw OpenAI Chat errors, or plain-text / HTML error pages. Errors are now regularized into the standard `{error: {message, type, code, param}}` envelope with the original HTTP status preserved; non-JSON bodies are wrapped and truncated to 1KB at a UTF-8 char boundary. Also fixed a pre-existing append-vs-insert bug that emitted a duplicate `Content-Type` header on rewritten JSON bodies.
- **Codex Mid-Stream System Messages Collapsed**: MiniMax's OpenAI-compatible endpoint strict-rejects any non-leading `system` message (error 2013). All `system` fragments are now collapsed into a single leading message (joined in original order), losslessly for permissive backends too.
- **Codex Model Catalog Wiped After Restart**: Editing the active Codex provider triggered a live read that omitted `modelCatalog`, so a subsequent save silently destroyed user-configured model mappings. Live reads now reverse-parse the on-disk catalog projection to round-trip the same shape the save path writes.
- **Codex Model Catalog Infinite Render Loop**: Broke a bidirectional sync cycle between the catalog table and its parent state that caused severe UI jittering when adding or editing entries.
- **Codex Chat Preserves User-Selected Catalog Model**: A model the client selects from the catalog (e.g. via `/model`) is no longer overwritten by `config.toml`'s default model.
- **Codex Chat Reasoning and Cache Stability**: Restored a unique call-id fallback when Codex omits or rewrites `previous_response_id`, stopped deriving cache identity from `previous_response_id`, and canonicalized parseable JSON string payloads in tool conversions for stable prefix-cache reuse.
- **Codex Chat Streaming Usage Recovered**: The Responses-to-Chat conversion now injects `stream_options.include_usage` (merging into any client-provided `stream_options`) when a request is streaming, so OpenAI-compatible upstreams like Kimi and MiniMax emit the trailing usage chunk again. Previously their streamed token / cost / cache stats were recorded as zero on the Codex Chat path.
- **Codex Chat Tool-Call Reasoning Backfill**: Thinking models like Kimi/Moonshot and DeepSeek reject an assistant message that carries `tool_calls` without a non-empty `reasoning_content`. When cross-turn history recovery misses (proxy restart, ambiguous `call_id`, or a turn with no upstream reasoning), a placeholder `reasoning_content` is now backfilled in a final pass — genuine trailing reasoning still attaches first — so the request no longer fails with `reasoning_content is missing in assistant tool call message`.
- **Managed-Account Claude Takeover Auth**: Managed-account providers (GitHub Copilot / Codex OAuth) now drop token env keys and write only the `ANTHROPIC_API_KEY` placeholder when taking over Claude Live config, with an outbound guard that refuses to send the `PROXY_MANAGED` placeholder upstream.
- **Claude Desktop Profile Sync During Takeover**: Claude Desktop profile data is now synced during proxy takeover, model routes align with the Claude Code three-role tiers, and the Cowork egress profile has been corrected (#3157, #3172).
- **Managed-Account Takeover Model Fields**: Local Routing now sources takeover model fields from the target provider on managed accounts instead of carrying stale model values.
- **DeepSeek Anthropic Tool Thinking History**: Normalized DeepSeek Anthropic-compatible tool-thinking history so later turns can replay reasoning / tool-call context without malformed messages (#3203).
- **Claude-Compatible Empty Tool Calls in Streams**: Fixed a Claude-compatible streaming edge case where an empty `tool_calls` array reset block state and broke streamed responses (#2915).
- **MiMo Reasoning for Claude Code Proxy**: Added MiMo `reasoning_content` support on the Claude Code proxy path (#2990).
- **Gemini Native Tool-Call Robustness**: Fixed `functionResponse.name` resolution (422) and `thought_signature` replay (400) for synthesized tool-call IDs in long multi-turn sessions (#2814).
- **Session Log Subagent Token Accounting**: `collect_jsonl_files()` now scans subagent JSONL logs that were previously missed, so subagent token usage is counted in session cost (session-log mode only) (#2821).
- **Usage Dashboard / Sync Stability**: Fixed a Codex usage-sync panic on non-ASCII model names, custom usage-script summaries, and missing real-time refresh after usage rollups (#3027, #3129).
- **ZhiPu Coding-Plan Quota Tier Ordering**: When the 5-hour bucket is at 0% utilization, ZhiPu's API omits `nextResetTime`; the old `i64::MAX` sentinel sorted those entries last, letting the weekly bucket incorrectly claim the five-hour slot. Tiers now sort so a missing `nextResetTime` maps to the five-hour bucket, so tray and usage quota display stays correct for ZhiPu coding plans.
- **Skills Install by Key**: Installing from skills.sh search results now uses the unique key instead of the directory name, so skills that share a directory name install the correct one (#2784); also fixed a skill sync copy fallback (#2791).
- **Usage Price Input Precision**: Reduced the price input step to 0.0001 so sub-cent costs like DeepSeek cache reads can be entered (#2793, closes #2503).
- **Ghostty Clean Window Launch**: Ghostty now opens a single clean window instead of cloning existing tabs, and other terminals open a new window via `open -na` (#2801, closes #2798).
- **Tool Version and Update Reliability**: Version probing no longer masks unrunnable installs, prerelease tools are handled correctly in version checks, batch updates run per tool, install / update buttons stay locked during preflight, anchored upgrade branches enforce absolute paths, and WSL installer paths use native Unix installers when needed.
- **Codex mise Detection**: Fixed Codex mise environment detection (#2822).
- **Codex Archived Sessions**: Codex archived sessions are now included in session discovery (#2861).
- **Codex Chat Empty Tool Arguments**: Empty tool-call argument payloads are coerced to `{}` during Codex Chat conversion so upstreams and clients receive valid JSON.
- **Claude Provider Deeplink Imports**: Importing Claude providers through deeplinks now preserves custom environment fields (#2928).
- **OMO Recommended Models**: Synced OMO recommended models with upstream defaults and improved Fill Recommended feedback.
- **ShengSuanYun Model IDs Prefixed for Routing**: ShengSuanYun (胜算云) presets now carry the vendor prefixes the upstream gateway requires — `anthropic/…`, `google/…`, and `openai/…` (e.g. `anthropic/claude-sonnet-4.6`, `google/gemini-3.1-pro-preview`) — across the Claude Code, Claude Desktop, Codex, Gemini, OpenCode, and OpenClaw presets, including the Claude Code routing env (`ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_{HAIKU,SONNET,OPUS}_MODEL`), so they resolve to valid upstream models instead of failing to route.
- **ClaudeAPI Model Test Re-Enabled**: Reclassified the ClaudeAPI preset (Claude Code and Claude Desktop) from `third_party` to `aggregator` so its model test button is no longer disabled by the third-party Claude gate; the partner star is unaffected since it is driven by `isPartner`, not category.
- **About Version Check**: Version checks now handle prerelease tool versions without misclassifying update state.
- **App Switcher Text Clipping**: Removed a fixed width constraint that clipped app-switcher text (#3161).
- **useEffect Race Condition**: Added an active-flag pattern to App.tsx effects to prevent listener leaks on unmount, and guarded against storing `undefined` language in localStorage (#2827).

### Removed

- **LionCC Sponsor and Presets**: Removed the LionCC sponsor entry and LionCCAPI presets across READMEs, provider configs, and locales (icon asset retained).
- **AICoding Partner Entry**: Removed the AICoding partner from README sponsor listings, provider presets, and i18n metadata.
- **Kimi For Coding Codex Preset**: Removed the Kimi For Coding preset from the Codex preset catalog.
- **CLI Uninstall Command Hints**: Dropped generated CLI uninstall command hints from the tool-management UI while keeping conflict diagnostics visible.

### Docs

- **Codex Chat Provider Support**: Documented Chat Completions routing, provider support, reasoning auto-detection, and Local Routing guidance in the changelog and user manual.
- **Settings Manual Refresh**: Updated settings documentation for the new managed tool lifecycle and Hermes installer behavior.
- **Claude Desktop Guide**: Added localized Claude Desktop guide pages and screenshots for provider setup, import, model mapping, and Local Routing context.
- **Installation Docs**: Updated installation docs and READMEs to recommend the official Homebrew cask and refreshed the v3.15.0 release-note imposter-site warning wording across locales.

## [3.15.0] - 2026-05-16

Development since v3.14.1 focuses on a dedicated Claude Desktop surface with third-party provider switching through a proxy gateway, a large reverse-proxy hardening pass (reliability, retries, cache, takeover, Gemini/Vertex/Codex paths), expansion of the third-party provider preset catalog (BytePlus / Volcengine / ClaudeAPI / ClaudeCN / RunAPI / RelaxyCode / PatewayAI / Baidu Qianfan), role-based model mapping with a 1M context flag, Codex OAuth live model discovery, and a long tail of usage, OAuth, Codex, and session quality-of-life fixes.

**Stats**: 127 commits | 211 files changed | +17,980 insertions | -2,748 deletions

### Highlights

- **Claude Desktop becomes a first-class managed surface** with third-party provider switching through an in-app proxy gateway, role-based model mapping (sonnet / opus / haiku) with a 1M context flag, Copilot/Codex OAuth provider reuse, and 44 imported provider presets translated from the Claude Code catalog. Note: 20 Claude Desktop presets now default to direct mode instead of routing through the proxy — verify connectivity if you previously relied on proxy routing for these presets.
- **Major reverse-proxy hardening**: P0–P3 lifecycle, retry, failover, and rectifier patches; pooled HTTPS reuse for non-Anthropic backends; Codex/Responses cache hit-rate improvements; correct Anthropic ↔ OpenAI `tool_choice` mapping; Vertex AI URL preservation; Gemini path-based model extraction; takeover detection refinement; IPv6 listen address support.
- **Provider Ecosystem Expansion**: Added BytePlus, Volcengine Agentplan, ClaudeAPI, ClaudeCN, RunAPI, RelaxyCode, PatewayAI, and Baidu Qianfan Coding Plan partner presets; promoted DouBao Seed to partner status; routing-support badges now surface on provider cards.
- **Role-Based Model Mapping for Claude Code**: Display-name-aware sonnet / opus / haiku route mapping with a `supports1m` flag replaces the legacy `[1M]` suffix and decouples routing from raw model IDs.
- **Codex OAuth Live Model Discovery**: ChatGPT Codex providers now fetch the live model list from the ChatGPT backend on demand instead of relying on a static list.
- **Usage Dashboard Filter-Driven Hero**: A new filter-driven Hero card with cache-normalized totals replaces the legacy summary block, paired with cache-cost-semantics fixes that silence a noisy pricing warning storm.
- **DeepSeek tool-call reasoning and zero-usage final deltas**: DeepSeek tool calls now return `reasoning_content` alongside `tool_calls` (#2543), and the final `message_delta` event always includes a usage block (even when zero) so strict Anthropic clients no longer crash on `null` (#2485).

### Added

- **Claude Desktop Third-Party Provider Switching via Proxy Gateway**: Added a dedicated Claude Desktop surface that brokers third-party Claude providers through CC Switch's in-app proxy, with a routing-support badge for providers that need it, role-based model route mapping locked to `sonnet` / `opus` / `haiku`, Copilot/Codex OAuth provider reuse, a redesigned Claude Code import flow, app-switcher differentiation between "Claude Code" and "Claude Desktop", and 44 provider presets translated from the Claude Code catalog.
- **Routing Support Badges on Provider Cards**: Provider cards in both Claude Code and Codex panels now show a routing-support badge so users can tell at a glance which providers can be served through Local Routing.
- **Codex OAuth Live Model List**: ChatGPT Codex providers now fetch the current model list from the ChatGPT backend on demand, replacing the previously hardcoded selection.
- **Role-Based Model Mapping with 1M Flag**: Claude Code model mapping is now role-based (`sonnet` / `opus` / `haiku`) with display names and a `supports1m` flag, replacing the legacy `[1M]` suffix to decouple routing from raw model IDs.
- **Filter-Driven Usage Hero**: The usage dashboard's Hero summary is now filter-driven with cache-normalized totals so the figures line up with the active date range and provider filters.
- **Provider Form "Save Anyway" Prompt**: Softened provider form validation with a "save anyway" prompt so non-blocking input issues no longer prevent saving (#2307).
- **Universal Provider Duplicate Action**: Added a duplicate action for universal providers from the provider list (#2416).
- **Persisted Tauri Window State**: Window position and size now persist across launches (#2377).
- **Tray Icon Tooltip**: The system tray icon now shows a tooltip on hover for clearer at-a-glance state (#2417).
- **Warp Terminal Session Launch**: Added support for launching Warp and executing a saved session inside it (#2466).
- **DeepSeek `reasoning_content` for Tool Calls**: DeepSeek tool-calling responses now return `reasoning_content` together with `tool_calls` so callers can render both (#2543).
- **Baidu Qianfan Coding Plan for Claude Code**: Added a Baidu Qianfan Coding Plan preset for Claude Code (#2322).
- **Compshare Coding Plan Preset (Cross-App)**: Added Compshare Coding Plan preset across claude / codex / hermes / openclaw.
- **Partner Provider Presets**: Added BytePlus, Volcengine Agentplan, ClaudeAPI, ClaudeCN, RunAPI, RelaxyCode, and PatewayAI provider presets; promoted DouBao Seed to partner status with refreshed endpoint and links.
- **44 Claude Desktop Provider Presets**: Translated 44 provider presets from the Claude Code catalog into the new Claude Desktop surface.

### Changed

- **20 Claude Desktop Presets Switched from Proxy to Direct Mode**: 20 Claude Desktop presets now ship in direct mode instead of routing through the proxy by default, reducing setup friction for users who don't need proxy-specific compatibility shims. Verify connectivity if you previously relied on proxy routing for these presets.
- **Claude Desktop Operational Notes**: Switching a Claude Desktop provider now writes CC Switch's managed 3P profile and requires restarting Claude Desktop to take effect; proxy-mode providers require CC Switch Local Routing to stay running.
- **Failover / Local Routing Guardrails**: Failover controls now require the target app's Local Routing takeover to be enabled, and stopping only the proxy service is blocked while any app still depends on takeover state.
- **Usage Accounting Semantics**: Usage summaries now report cache-normalized real total tokens and cache hit rate; historical token and cost totals may shift after deduplication and pricing recalculation, but should be more accurate.
- **Provider Preset Rendering Order**: Provider preset lists now render in author-defined array order with partners prioritized at the top, replacing the previous implicit sort.
- **Model Mapping Hint Copy Simplified**: `modelMappingOffHint` was rewritten as action-oriented copy across zh / en / ja.
- **CC Switch Brand Surface Unified to ccswitch.io**: All in-app and README references now point at ccswitch.io as the sole official website; the release notes template also surfaces ccswitch.io.
- **Theme Switch Simplified**: Removed the circular reveal animation; theme changes are now an instant cross-fade.
- **Claude Code App Switcher Differentiation**: The app switcher now visually distinguishes "Claude Code" from "Claude Desktop" and uses the "Claude Code" label in the app visibility settings.
- **CI: Claude Review on Opus 4.7**: Upgraded the Claude review GitHub Action to Opus 4.7, tuned the prompt to reduce nitpick noise, added an `@claude` review-only Code Action, pinned PR head SHA for checkout, and dropped a `--max-turns 5` limit.
- **Dependency Bumps**: `actions/checkout` 4 → 6 (#2517), `pnpm/action-setup` 5 → 6 (#2518), `softprops/action-gh-release` 2 → 3 (#2519), `actions/stale` 9 → 10 (#2520).
- **DeepSeek Presets Switched to V4**: DeepSeek presets now ship V4 (flash / pro) with refreshed pricing seeds.
- **Codex 1M Context Toggle Hidden in Provider Edit Form**: The 1M context-window toggle is no longer surfaced in the Codex provider edit form to reduce knob count for a setting that has no effect in current Codex deployments.
- **OpenClaudeCode Migrated to MicuAPI Domain**: Updated the OpenClaudeCode preset to the MicuAPI domain; refreshed Micu API links to `micuapi.ai`.
- **CrazyRouter Endpoints Switched to `cn` Subdomain**: Updated CrazyRouter preset endpoints to the `cn` subdomain.
- **RelaxyCode Custom Icon**: Switched RelaxyCode preset icon to a custom `relaxcode.png` asset.
- **Kimi For Coding Doc URL**: Updated Kimi For Coding website URL to the `/code/docs/` path.
- **SiliconFlow International Site Shows USD**: Balance display now correctly shows USD for the SiliconFlow international site (was incorrectly displaying CNY).

### Fixed

- **OpenAI Responses API Usage Parsing Robustness**: Hardened `build_anthropic_usage_from_responses()` and the Responses → Anthropic SSE translator so a missing or malformed upstream `usage` no longer produces `"usage": null` in `message_delta`. This unblocks strict Anthropic clients (notably the VSCode Claude Code extension) that crashed with "Cannot read properties of null (reading 'output_tokens')" against providers such as Codex OAuth and DashScope's `compatible-mode/v1/responses` endpoint. Added OpenAI field-name fallbacks (`prompt_tokens` / `completion_tokens`), null/empty/partial object handling, and preserved cache token fields even when input/output tokens are missing (#2422).
- **Proxy Reliability Patches (P0–P3)**: Multiple rounds of routing, lifecycle, retry, and rectifier patches across the request-forwarder paths; extracted a shared `handle_rectifier_retry_failure` helper and a shared `auth_header_value` helper across provider adapters.
- **Proxy: Pooled HTTPS Connection Reuse**: Non-Anthropic backends now reuse pooled HTTPS connections instead of opening a fresh TLS session per request, materially reducing per-request latency.
- **Proxy: Forward Client HTTP Method**: The proxy forwards the client's actual HTTP method instead of hard-coding `POST`, so non-POST upstream endpoints (e.g. GET `/v1/models`) work correctly.
- **Proxy: Per-Attempt Counters and `max_retries` Wiring**: Client-request counters moved out of the per-attempt loop, and `AppProxyConfig.max_retries` is now correctly wired into the request forwarder.
- **Proxy: Failover Decision Refinements**: Refined failover decision logic in the forwarder so retryable / unretryable errors are classified more accurately.
- **Proxy: Takeover Detection Tightening**: Tightened takeover detection and use fallback restore when disabling takeover so leftover state no longer strands a provider.
- **Proxy: Anthropic ↔ OpenAI `tool_choice` Mapping**: Anthropic `tool_choice` is now correctly mapped to the OpenAI Chat nested form during format conversion.
- **Proxy: Gemini Request Model Extraction**: Gemini request model is now correctly extracted from the URI path (not the body) so transformed traffic reports the right model.
- **Proxy: Auth Header Error Handling**: `get_auth_headers` now returns `Result` instead of panicking on bad credentials.
- **Proxy: IPv6 Listen Address Validation**: The Proxy panel now accepts IPv6 listen addresses.
- **Proxy: Codex / Responses Cache Hit Rate**: Improved cache hit rate for Codex and OpenAI Responses requests by stabilizing cache key derivation; only emit `prompt_cache_key` when a real client-provided session identity is available so unrelated conversations no longer collapse onto a single key; canonicalize (sort) JSON keys in outgoing request bodies and `tool_call` arguments / `tool_result` content for byte-identical prefix-cache reuse; thread `session_id` into the usage logger for request correlation.
- **Proxy: JSON Schema Underscore Fields Preserved**: Private-parameter filtering now preserves underscore-prefixed field names inside JSON Schema name maps such as `properties`, `patternProperties`, `definitions`, and `$defs`, so user-defined schema keys like `_id` and `_meta` survive the filter.
- **Proxy: Read Tool Empty Pages**: Drop empty pages from `Read` tool inputs so providers don't reject the request (#2472).
- **Proxy: Per-Request Hot-Path Trim**: Trimmed per-request hot-path work and database wait time.
- **Proxy: Real Provider Model Names Under Takeover**: The Claude Code menu now exposes the real provider model names when running under takeover, instead of a stale alias.
- **Proxy: Zero Usage in Final Message Delta**: The final `message_delta` event always includes a usage block (even when zero) so strict Anthropic clients no longer crash on `null` (#2485).
- **Proxy: Streaming `message_delta` Deduplication**: Deduplicated streaming `message_delta` events that some upstreams emit twice (#2366).
- **Proxy: Scoped `reasoning_content` Preserved for Tool Calls**: Tool-call paths now correctly preserve the scoped `reasoning_content` field during transformation; Kimi / Moonshot OpenAI Chat compatibility paths keep the field while generic OpenAI-compatible requests stay free of it (#2367).
- **Proxy: Vertex AI Full URL Preserved**: Full Vertex AI URLs are no longer truncated during proxy forwarding (#2415).
- **Proxy: Leading Billing Header Stripped from System Content**: Strips the leading billing-header content that some upstreams prepend to the system message (#2350).
- **Proxy: Claude Auth Strategy from `ANTHROPIC_*` Env Var**: The Claude auth strategy is now derived from the actual `ANTHROPIC_*` env variable name rather than an opaque heuristic.
- **Third-Party Claude Providers: Disable Model Test**: Model probing is now disabled for third-party Claude providers where the gateway doesn't implement `/v1/models` consistently.
- **Model-Fetch: `/models` Subpath for Anthropic-Compatible Providers**: `/models` discovery now works for Anthropic-compatible subpath providers.
- **Copilot: Claude Model ID Resolution Against Live `/models`**: Copilot-backed providers now resolve Claude model IDs against the live `/models` list to avoid stale ID mismatches.
- **Codex: Skip `environment_context` Injection When Extracting Session Title**: Session title extraction no longer pulls in `environment_context` noise (#2439).
- **Codex: Hide Subagent Sessions**: Codex subagent sessions are now hidden from the main session list (#2445).
- **Codex Startup Live Import Duplication**: Fixed a duplicate-import bug in the Codex startup live-import path (#2590).
- **Codex Provider Switch History Drift**: Switching the active Codex provider no longer changes existing session history (#2349).
- **Codex Usage Log Message**: Corrected a misleading log message for Codex session usage (#2473).
- **Claude: Persist Max Effort via Env**: `max` effort now correctly persists via the env variable on restart (#2493).
- **Claude Desktop: Match Proxy Model Route Without `[1M]` Suffix**: Route matching no longer requires the legacy `[1M]` suffix.
- **Claude Desktop: Provider Form Focus Loss**: Fixed an input that lost focus while editing in the Claude Desktop provider form.
- **Claude Desktop: Spurious Proxy-Stopped Status Alert**: Removed an alert that fired spuriously when the proxy was intentionally stopped.
- **Claude Desktop: Empty Toolbar Capsule Hidden**: Hides the empty toolbar capsule when Claude Desktop is the active app.
- **UI: Monitor Badge Icon Centering**: Centered the Monitor badge icon in the app switcher.
- **Linux: Theme Selection Segfault**: Prevented selecting a theme from causing a segfault on Linux (#2502).
- **Terminal: iTerm Fallback on Cold Launch**: Prevented iTerm from being selected as a fallback on cold launch when not actually present (#2448).
- **Config: Sort JSON Keys Alphabetically**: Config writes now sort JSON keys alphabetically for deterministic output (#2469).
- **Import Existing Side-Effect Free**: Made "import existing" side-effect free (#2429).
- **Coding Plan: Zhipu Weekly Tier by Reset Time**: Corrected the Zhipu weekly tier name to match the actual reset time (#2420).
- **DashScope: Usage Parsing Robustness**: Hardened DashScope usage parsing so a malformed payload no longer crashes the VSCode Claude Code extension (#2425).
- **Usage: Prevent Double-Counting Between Proxy and Session-Log Sources**: Deduplicated usage records sourced from both the proxy and session logs.
- **Usage: Cache Cost Semantics + Pricing Warn Storm**: Corrected cache-cost semantics and silenced a noisy pricing warning storm that fired on every request.
- **CI: Frontend Formatting and Linux Clippy Restored**: Restored frontend formatting and Linux clippy checks in CI.
- **Proxy Test Helper Clippy Warning**: Fixed a clippy warning in the proxy test helper.

### Removed

- **Hermes Agent Usage Tracking Integration**: Removed the in-cycle Hermes Agent usage tracking integration after upstream behavior changes made it impractical to keep in sync. The integration was never enabled in any released version — a zero-cost rendering bug found during its development was fixed before the integration was rolled back.
- **Theme Switch Circular Reveal Animation**: Removed the circular reveal animation used during theme switching; the animation caused jank on slower compositors and added little visible value.
- **DDSHub Partner Integration**: Removed DDSHub as a partner preset and dropped the cross-link blurbs across READMEs.

### Docs

- **README Sponsor Refresh (zh / en / ja)**: Added BytePlus, ClaudeCN, RunAPI, and PatewayAI sponsor entries; cross-linked BytePlus and Volcengine entries; refreshed the Crazyrouter $2 credit claim flow, the Compshare blurb, the Right Code blurb, and other sponsor logos and listings; flattened the LionCC logo onto a white background; switched the Chinese README's sponsor logo to the Volcengine artwork; added Hermes Agent to the README subtitles.
- **Release Notes Template**: Surfaces `ccswitch.io` in the release notes template.
- **Brand Surface**: Documented `ccswitch.io` as the sole official website across READMEs and in-app references.

## [3.14.1] - 2026-04-23

Development since v3.14.0 focuses on Codex OAuth stability, tray usage visibility, Skills import/install reliability, Gemini session restore paths, and simplifying Hermes configuration health handling.

**Stats**: 13 commits | 48 files changed | +1,883 insertions | -808 deletions

### Added

- **Tray Usage Visibility**: System tray submenus now show cached usage for the current Claude / Codex / Gemini provider, including subscription and script-based usage summaries with utilization color markers. Tray-triggered refreshes are throttled, limited to visible apps, and synchronized back into React Query so the main window and tray share fresh usage data (#2184).
- **Tray Coding-Plan Usage (Kimi / Zhipu / MiniMax)**: System tray now renders 5-hour + weekly window usage for Chinese coding-plan providers using the same `🟢 h12% w80%` two-window layout as official subscription badges (worst utilization drives the emoji). Creating a Claude provider whose `ANTHROPIC_BASE_URL` matches a known coding-plan host now auto-injects `meta.usage_script`, so the tray lights up without opening the Usage Script modal. Existing `usage_script` values are preserved on update.
- **Codex OAuth FAST Mode**: Added an explicit FAST mode toggle for Codex OAuth-backed Claude providers. When enabled, converted Responses requests send `service_tier="priority"` for lower latency; the toggle stays off by default to avoid unexpectedly increasing ChatGPT quota consumption (#2210).

### Changed

- **Session and Settings Layout Polish**: Hardened the scroll-area viewport with width containment to fix horizontal overflow, and tightened app bottom spacing plus settings footer spacing so long session/settings views fit more cleanly (#2201).

### Removed

- **Hermes Config Health Scanner**: Removed the in-app Hermes config health scanner, warning banner, `scan_hermes_config_health` command, `HermesHealthWarning` type, and `HermesWriteOutcome.warnings` payload. CC Switch now keeps the Hermes surface focused on active provider display, provider switching defaults, memory editing, and launching the Hermes Web UI for deep configuration.

### Fixed

- **Codex OAuth Cache Routing**: Stabilized ChatGPT Codex reverse-proxy cache identity by using client-provided session IDs for `prompt_cache_key` and Codex session headers, preserving explicit cache keys, and avoiding generated UUID cache churn (#2218).
- **Codex OAuth Responses SSE Aggregation**: Non-streaming Anthropic clients now receive JSON even when the ChatGPT Codex upstream forces OpenAI Responses SSE; CC Switch aggregates the upstream SSE events before running the non-streaming transform (#2235).
- **Codex OAuth Stream Check Parity**: Stream checks now build Codex OAuth test requests with the same `store: false`, encrypted reasoning include, and provider FAST mode setting as production proxy requests (#2210).
- **Codex Model Extraction**: Replaced first-line regex matching with TOML parsing when reading Codex config models, so multiline TOML is handled correctly (#2227).
- **Model Quick-Set / One-Click Config**: Model quick-set updates now apply against the latest provider form config, preventing stale state from making one-click configuration fail (#2249).
- **Skills Import Duplicates**: The Skills import dialog disables actions while import is pending and the installed-skills cache deduplicates imported results by ID, preventing double-clicks from adding duplicate installed entries (#2139, #2211).
- **Root-Level Skill Repos**: Skill install and update flows now consistently resolve three source patterns: direct nested paths, install-name recursive search, and repository-root `SKILL.md` sources (#2231).
- **Gemini Session Restore Paths**: Gemini session scanning now reads `.project_root` metadata so restore flows can pass the original project directory when available (#2240).
- **Provider Hover Names**: Provider icons now expose the provider name on hover for inline SVG, image URL, and fallback initials render paths (#2237).

## [3.14.0] - 2026-04-21

Development since v3.13.0 focuses on onboarding Hermes Agent as a first-class managed app, rolling out Claude Opus 4.7 across the preset matrix, adding a Gemini Native API proxy, and sharpening session, usage, and proxy workflows.

**Stats**: 100 commits | 219 files changed | +20,548 insertions | -3,569 deletions

### Added

- **Hermes Agent Support (6th Managed App)**: Added Hermes Agent as a first-class managed app with database migration v9→v10, full Rust command surface, YAML-backed `~/.hermes/config.yaml` read/write with atomic backups, MCP sync, Skills sync, session manager with SQLite + JSONL support, and dedicated frontend panels. Supports four API protocols (`chat_completions`, `anthropic_messages`, `codex_responses`, `bedrock_converse`) aligned with Hermes Agent 0.10.0 schema. Read-only rendering for providers owned by the user-authored `providers:` dict, with deep configuration delegated to the Hermes Web UI.
- **Hermes Memory Panel**: Added a Memory panel for editing `MEMORY.md` and `USER.md` directly from CC Switch, with an enable switch, character-count limits, and a live save flow. Replaces the Prompts entry for Hermes.
- **Hermes Provider Presets**: Added ~50 Hermes provider presets spanning Nous Research, Shengsuanyun, OpenRouter, DeepSeek, Together AI, StepFun, Zhipu GLM, Bailian, Kimi, MiniMax, DouBao, BaiLing, ModelScope, KAT-Coder, PackyCode, Cubence, AIGoCode, RightCode, AICodeMirror, AICoding, CrazyRouter, SSSAiCode, Micu, CTok.ai, DDSHub, E-FlowCode, LionCCAPI, PIPELLM, Compshare, SiliconFlow, AiHubMix, DMXAPI, TheRouter, Novita, Nvidia, and Xiaomi MiMo.
- **Claude Opus 4.7 Support**: Added Claude Opus 4.7 with adaptive thinking whitelisting, per-million pricing seed, and Bedrock SKU (`anthropic.claude-opus-4-7` / `global.anthropic.claude-opus-4-7`, dropping the legacy `-v1` suffix). Migrated all aggregator and Bedrock presets to Opus 4.7 as the default Opus model.
- **Claude `max` Effort Tier**: Upgraded the Claude effort dropdown from `high` to `max` for extended reasoning capacity.
- **Gemini Native API Proxy**: Added `api_format = "gemini_native"` so the proxy can forward to Google's `generateContent` API with full streaming, schema conversion, and shadow request support. Adds `gemini_url.rs`, `gemini_schema.rs`, `gemini_shadow.rs`, `streaming_gemini.rs`, and `transform_gemini.rs` under the proxy providers module.
- **GitHub Copilot Enterprise Server**: Added GHES authentication and endpoint configuration for Copilot-backed Claude providers, plus thinking-block stripping before upstream to preserve premium interaction quota.
- **Session List Virtualization**: Virtualized the session list via `@tanstack/react-virtual` so long conversations (thousands of records) scroll smoothly; long session messages are now collapsed by default to reduce text layout cost.
- **Codex / OpenClaw Session Title Extraction**: Added meaningful title auto-extraction for Codex and OpenClaw sessions with 2-line display; strips OpenClaw `message_id` suffix noise.
- **Usage Date Range Picker**: Added a date range selector to the usage dashboard with preset tabs (Today / 1d / 7d / 14d / 30d), a custom date + time calendar picker, and a page-jump input on paginated lists.
- **Model Mapping Quick-Set**: Added a quick-set button next to model mapping fields in provider forms for faster edits.
- **Stream Check Error Classification**: Classified Stream Check errors and surfaced them as color-coded toasts; refreshed default probe models and added explicit detection for "model not found" responses.
- **Block Official Provider Switching During Local Routing**: Blocks switching to official providers while Local Routing is active, since routing official API traffic through the local proxy carries account-suspension risk. A warning toast surfaces the block.
- **Pricing Database Refresh (v8 → v9)**: Added ~50 new model pricing entries and corrected stale prices via a reseed-on-migration step, including Claude 4.7, Opus 4.7 Adaptive Thinking, Grok 4, Qwen 3.5/3.6, MiniMax M2.5/M2.7, Doubao Seed 2.0 series, and GLM-5/5.1. DeepSeek and Kimi K2.5 prices updated.
- **Application-Level Window Controls**: Added an opt-in setting to render CC Switch's own minimize / toggle-maximize / close buttons instead of the system decorations, materially improving the experience on Linux Wayland where compositor-drawn buttons can become inert.
- **Hermes in Unified Skills Management**: Added Hermes to the unified Skills surface; skill install, enable, and filter now cover the Hermes app alongside Claude / Codex / Gemini / OpenCode / OpenClaw.
- **OpenClaw Config Directory Override**: Added a settings option to point CC Switch at a custom `openclaw.json` location.
- **Hermes Config Directory Override**: Added a settings option to point CC Switch at a custom `~/.hermes/config.yaml` location, backed by data-driven dispatch.
- **StepFun Step Plan Preset**: Added StepFun Step Plan (EN/ZH) provider presets.
- **New API Usage Script Template**: Added a User-Agent header to the New API usage script template for better upstream compatibility.
- **Launch Hermes Dashboard from Toolbar**: When the Hermes Web UI probe fails, the toolbar entry now offers to run `hermes dashboard` in the user's preferred terminal via a temp bash/batch script. `hermes dashboard` opens the browser itself once ready, so no polling is required. Also corrects the stale `hermes web` hint in the offline toast (the real command is `hermes dashboard`) and reorders Linux terminal detection to try `which` before stat'ing `/usr/bin`, `/bin`, `/usr/local/bin`.
- **LemonData Provider Preset (All Six Apps)**: Registered LemonData as a third-party partner preset across Claude, Codex, Gemini, OpenCode, OpenClaw, and Hermes, with icon assets and zh/en/ja partner-promotion copy. Claude uses `ANTHROPIC_API_KEY` auth; OpenAI-compatible apps target `gpt-5.4`.
- **DDSHub Codex Preset**: Added a Codex-compatible endpoint for DDSHub at the same host as its Claude service; base URL omits the `/v1` suffix because the gateway auto-routes OpenAI SDK paths.

### Changed

- **"Local Proxy Takeover" → "Local Routing"**: Unified terminology across UI copy, README, and docs in all three locales. Functional behavior is unchanged.
- **Hermes `Auto` api_mode Removed**: Users must now pick an explicit protocol; new deeplinks default to `chat_completions`. Eliminates URL-based heuristic surprises.
- **Hermes Provider Form**: Added an API mode dropdown and per-provider model editor; bound per-provider models to the top-level `model:` when switching active providers.
- **Hermes Deep Config Delegation**: Deep YAML knobs are now delegated to the Hermes Web UI via a direct launch action, rather than duplicated in the CC Switch form.
- **`ANTHROPIC_REASONING_MODEL` Removed from Claude Quick-Set**: Decoupled the reasoning capability from model selection; the legacy field is no longer surfaced in the quick-set form.
- **Per-Provider Proxy Config Removed**: Consolidated into global Local Routing; the provider-level proxy toggle and associated storage are gone.
- **Unified Toolbar Icon Button Width**: Normalized icon-button widths across Claude / Codex / Gemini / OpenCode / OpenClaw / Hermes panels for a consistent header look.
- **Rust Toolchain Pinned to 1.95**: Adopted clippy 1.95 suggestions across the workspace and pinned the toolchain to prevent nightly drift.
- **Tray Menu ID Constant**: The tray identifier moved from the hardcoded string `"main"` to a `TRAY_ID` constant (`"cc-switch"`) across all call sites.
- **Copilot Request Classification**: Refined request routing inside the Copilot optimizer to further reduce unnecessary premium interaction consumption.
- **Usage Script Intranet Support**: Removed private-IP / suspicious-hostname blocking from usage scripts, unblocking enterprise intranet, Docker, and self-hosted API endpoints. Built-in templates still enforce HTTPS (except localhost) and same-origin checks; custom templates remain user-controlled with those request-URL checks skipped.
- **Failover Queue Notes**: Provider notes now appear in failover queue selectors and queue rows for easier identification across multi-provider queues.
- **Hermes Toolbar Layout**: Swapped the Hermes Web UI button from `ExternalLink` to `LayoutDashboard` (clicking may spawn `hermes dashboard` rather than just opening a URL), and moved MCP to the final toolbar slot so Hermes matches the Claude / Codex / Gemini / OpenCode layout.

### Fixed

- **Header Auto-Compact Latching After Maximize**: The toolbar no longer stays compacted after maximize/restore; compaction now reevaluates on size changes.
- **Hermes YAML Pollution & OAuth MCP Auth Drop**: Round-tripping through CC Switch no longer drops OAuth MCP `auth` blocks or pollutes unrelated YAML keys; guard tests added via `tests/hermes_roundtrip.rs`.
- **Hermes Active Provider Display**: Hermes UI now correctly surfaces the active provider and wires add / enable / remove actions.
- **Hermes Provider Persistence**: Providers persist under `custom_providers:` so `api_mode` and `model` survive restarts and config reloads.
- **Codex `cache_control` Preservation**: Preserve `cache_control` when merging system prompts during Codex format conversion (#1946).
- **Claude Prompt Cache Key Leak**: Stopped sending prompt cache keys during Claude chat conversions (#2003).
- **Proxy Hop-by-Hop Header Stripping**: Strip hop-by-hop response headers (Connection, Keep-Alive, Transfer-Encoding, etc.) per RFC 7230.
- **Permissive Proxy CORS Removed**: Removed the permissive CORS layer from the proxy (#1915).
- **Copilot Premium Consumption**: Further reduced unnecessary Copilot premium interaction consumption during pass-through traffic.
- **Backend Error Details in Proxy Toast**: Surface backend error payload details in proxy-related toast messages instead of a generic failure string.
- **Usage Log Deduplication**: Deduplicated proxy and session-log usage records so the same request is no longer double-counted; synced the request log time range with the dashboard's 1d / 7d / 30d selector.
- **Common Config Checkbox Persistence**: Checkbox state for Claude / Codex / Gemini common-config toggles now persists correctly across reopens.
- **Claude Plugin `settings.json` Sync**: Editing the current provider now syncs back to `settings.json` for the Claude plugin path.
- **Google Official Gemini Env Preservation**: Saving the Google Official Gemini provider no longer clobbers the `env` block.
- **OpenCode JSON5 Parser for Trailing Commas**: OpenCode config reads now tolerate trailing commas via a JSON5 parser.
- **Preset Refreshes**: Refreshed stale context windows for DeepSeek and Claude 1M; refreshed stale model IDs; backfilled Hermes model lists; fixed the Nous endpoint and replaced the Hermes placeholder icon with Nous brand artwork; pruned unused official Hermes presets.
- **Auto-Expand Collapsed Messages on Search Hit**: Collapsed messages now auto-expand when a search match lands inside hidden content.
- **Unknown Subscription Quota Tiers Hidden**: Provider cards no longer render unknown subscription quota tiers.
- **Weekly Limit Label Unified**: Aligned the weekly_limit tier label with the official 7-day naming across locales.
- **Root-Level Skill Repo Install**: Fixed skill installation when the repository root itself is a skill.
- **Session ID Parsing Clippy**: Removed a redundant closure in session ID parsing (clippy warning).
- **Usage Log Stat Dedup**: Deduplicated proxy-sourced and session-log-sourced usage records for accurate totals.
- **Stream Check Default Models Refresh**: Updated stream-check default probe models to match each vendor's current lineup.
- **Skills Import Sync**: Imported Skills are now immediately synced into enabled app directories instead of only being recorded in the database, so the UI no longer shows "installed" while the target app directory is missing the skill.
- **Ghostty Session Restore**: Fixed Ghostty session restore launch by using shell execution with `--working-directory`, avoiding `cwd` escaping issues when the path contains spaces or special characters.
- **Hermes Health Check Borrowing OpenClaw Schema**: Hermes providers were routed through `check_additive_app_stream` (the OpenClaw dispatcher), which reads camelCase `baseUrl` / `apiKey` / `api` and surfaced "OpenClaw provider is missing baseUrl" even when every Hermes field was filled. Introduced `check_hermes_stream` with Hermes-specific extractors that map `api_mode` (`chat_completions` / `anthropic_messages` / `codex_responses`) to the matching `check_claude_stream` `api_format`, and returns `bedrock_converse` as unsupported. `api_mode` is now resolved before URL / API key extraction, so `bedrock_converse` users see the real cause rather than a misleading "missing base_url".
- **Usage Query Modal for Hermes & OpenClaw**: `getProviderCredentials` now reads flat `settingsConfig` fields for Hermes (snake_case `base_url` / `api_key`) and OpenClaw (camelCase `baseUrl` / `apiKey`), so the "official balance" template auto-selects for matching providers like SiliconFlow. Also refactored the BALANCE and TOKEN_PLAN test paths to reuse the precomputed `providerCredentials` instead of re-reading `env.ANTHROPIC_*` directly, fixing the "empty key" error for non-Claude apps even when the key was configured.

### Docs

- **README Sponsor Updates**: Updated SiliconFlow signup bonus to ¥16, trimmed the SSSAiCode sponsor blurb, updated partner logos, and added LemonData as a new sponsor.
- **Global Proxy Hint Clarified**: Clarified the global proxy hint about local routing across all three locales.
- **Takeover → Routing Rename**: Renamed takeover docs to routing and updated anchors across all languages.
- **PIPELLM Website URL**: Updated the PIPELLM sponsor website URL to `code.pipellm.ai`.

### Breaking

- **Hermes requires explicit `api_mode`**: The `Auto` mode is gone; imported or deeplinked providers default to `chat_completions`. Users with prior `Auto` configs will be prompted to pick a protocol.
- **`ANTHROPIC_REASONING_MODEL` removed from Claude quick-set**: The legacy field is no longer exposed; existing settings are cleaned up automatically.
- **Per-provider proxy configuration removed**: Migrate to the global Local Routing setting. Existing per-provider proxy values are ignored.
- **Database schema bumped v9 → v10**: Adds `enabled_hermes` columns to `mcp_servers` and `skills` (auto-migrated with `DEFAULT 0`; no data loss).
- **Pricing table reseeded (v8 → v9)**: The `model_pricing` table is cleared and reseeded on first launch to pick up new models and corrected prices.
- **XCodeAPI preset removed**: Users of the XCodeAPI preset should switch to another provider.

---

## [3.13.0] - 2026-04-10

Development since v3.12.3 focuses on quota visibility, provider workflow upgrades, stronger proxy compatibility, and lower-overhead tray / session workflows.

### Added

- **Lightweight Mode**: Added a tray-only mode that destroys the main window and keeps CC Switch running from the system tray, with the window recreated when users reopen it.
- **Provider Model Auto-Fetch**: Added OpenAI-compatible `/v1/models` discovery for Claude, Codex, Gemini, OpenCode, and OpenClaw provider forms, including grouped dropdown selection and failure-specific error messages.
- **Quota & Balance Visibility**: Added inline quota or balance display for official Claude / Codex / Gemini providers, GitHub Copilot premium interactions, Codex OAuth providers, Token Plan providers (Kimi / Zhipu GLM / MiniMax), and official balance queries for DeepSeek, StepFun, SiliconFlow, OpenRouter, and Novita AI. Copilot / ChatGPT OAuth and CLI subscription quota now only auto-poll for the currently active provider, preventing unnecessary API calls and misleading displays on non-current cards.
- **Skills Discovery & Batch Updates**: Added SHA-256 based skill update detection, per-skill and batch update actions, a storage-location toggle between CC Switch and `~/.agents/skills`, and public `skills.sh` search integration.
- **Session Workflow Upgrades**: Added batch delete in Session Manager, a directory picker before launching Claude terminal restore commands, usage import from Claude / Codex / Gemini session logs without requiring proxy interception, and per-app usage filtering for Claude / Codex / Gemini dashboards.
- **Codex OAuth Reverse Proxy**: Added ChatGPT Plus / Pro based Codex OAuth reverse proxy support for Claude provider cards, including managed OAuth login and inline subscription quota display.
- **OpenCode / OpenClaw Stream Check Coverage**: Added OpenCode npm package mapping plus support for OpenClaw `openai-completions` and the remaining OpenClaw protocol variants in Stream Check.
- **Full URL Endpoint Mode**: Added a provider option that treats `base_url` as a complete upstream endpoint so proxy forwarding and stream checks can work with vendors that require nonstandard URL layouts.
- **OpenCode StepFun Step Plan Preset**: Added a StepFun Step Plan provider preset for OpenCode.
- **Copilot Interaction Optimizer**: Added request classification and routing logic to reduce unnecessary GitHub Copilot premium interaction consumption.
- **First-Run Welcome Dialog**: Added a one-time welcome dialog on fresh installs explaining how existing configuration is preserved as a default provider and how the bundled official preset enables one-click revert. Upgrade users are excluded.
- **Official Provider Seeding**: Added automatic seeding of Claude Official, OpenAI Official, and Google Official provider entries on startup, giving every user a one-click path back to the official endpoint.
- **OpenCode / OpenClaw Auto-Import**: Added automatic startup import of live OpenCode and OpenClaw provider configurations, matching the auto-import behavior already present for Claude, Codex, and Gemini.
- **Common Config Editor Guidance**: Added an informational guide and empty-state prompt to the Common Config snippet editor modal for Claude, Codex, and Gemini, with i18n support.
- **Common Config First-Run Notice**: Added a one-time informational dialog explaining Common Config Snippets when users first open the provider add/edit form.
- **Claude Session Titles**: Added meaningful title extraction for Claude sessions using a priority chain: custom-title metadata, first real user message, then directory basename fallback.
- **Session Search Highlighting**: Added keyword highlighting in session titles and messages during Session Manager search.
- **URL-Based Provider Icons**: Added a dual rendering mode to the icon system supporting Vite URL imports for large SVGs and raster images (PNG, JPG, WebP), keeping small SVGs inlined.
- **Kaku Terminal Support**: Added Kaku as a selectable terminal for session launch on macOS, reusing the WezTerm-compatible launch path.
- **OMO Slim Council Support**: Restored first-class council support as a built-in oh-my-opencode-slim agent with updated metadata and UI copy.
- **TheRouter Provider Preset**: Added TheRouter provider presets across Claude, Codex, Gemini, OpenCode, and OpenClaw.
- **DDSHub Provider Preset**: Added DDSHub as a third-party partner provider for Claude with icon and partner promotion text.
- **LionCCAPI Provider Preset**: Added LionCCAPI as a third-party partner provider across all five apps with anthropic-messages protocol for OpenCode and OpenClaw.
- **Shengsuanyun Provider Preset**: Added Shengsuanyun (胜算云) as an aggregator partner provider across all five apps with URL-based icon and localized display name.
- **PIPELLM Provider Preset**: Added PIPELLM provider preset across Claude, Codex, OpenCode, and OpenClaw with full model definitions and icon.
- **E-FlowCode Provider Preset**: Added E-FlowCode provider preset across all five apps with per-app protocol configuration.

### Changed

- **Tray Menu Organization**: Reworked the tray menu into per-app submenus to prevent overflow and make background provider switching scale better with larger provider lists.
- **Proxy Forwarding Stack**: Refactored proxy forwarding onto a Hyper-based client with transparent header forwarding, improved endpoint rewriting, and better support for dynamic upstream endpoints.
- **OAuth Auth Center UI Polish**: Tightened the Auth Center copy, layout, and icon presentation so the Codex OAuth login flow feels cleaner and less cluttered.
- **Provider Key Lifecycle & Live Sync**: Reworked additive provider create / rename / duplicate flows so live config writes, cleanup, and rollback stay consistent across OpenCode / OpenClaw and takeover scenarios.
- **Codex OAuth Defaults**: Updated the Codex OAuth preset to the GPT-5.4 model family.

### Fixed

- **Copilot Authentication & Proxy Compatibility**: Fixed GitHub Copilot authentication regressions, corrected enterprise / dynamic endpoint handling, repaired clipboard verification-code copying on macOS and Linux, and fixed Responses routing when Copilot-backed Claude providers target OpenAI models.
- **Streaming Parser Compatibility**: Fixed SSE parsing to accept fields with optional spaces, improving compatibility with non-strict streaming implementations.
- **UTF-8 Stream Chunk Boundaries**: Fixed intermittent garbled output (U+FFFD replacement characters) in Claude Code when multi-byte UTF-8 sequences such as Chinese characters or emoji were split across TCP stream chunks via the Copilot reverse proxy, by preserving incomplete trailing bytes across chunks in all four SSE streaming paths instead of lossy decoding.
- **Fragmented System Prompt Normalization**: Fixed strict OpenAI-compatible chat backends (Nvidia, Qwen-style) rejecting requests when converted Claude payloads contained multiple system messages, by merging system content into a single leading system message during the Anthropic → OpenAI chat transformation.
- **Provider Switch State Corruption**: Serialized per-app provider switches to prevent concurrent failover or hot-switch operations from leaving `is_current`, settings state, and live backup state out of sync.
- **Claude Takeover Live Config Drift**: Fixed provider edits while Claude takeover is active so live settings remain aligned with the latest provider state without breaking takeover restore behavior.
- **WebDAV Password Retention & Validation**: Fixed the WebDAV password field so saved credentials remain visible after refresh and treated `MKCOL 405` responses correctly during connection validation.
- **Provider Card Action States**: Fixed additive-mode highlight behavior, aligned usage display layout across provider cards, replaced hard proxy-switch blocking with a warning path, and disabled unsupported test / usage actions for Copilot and Codex OAuth cards.
- **Usage Accuracy & Pricing**: Fixed MiniMax quota math and 0%→100% progression, corrected CNY→USD pricing plus missing model definitions, improved Gemini session-log syncing, and resolved session-based usage entries being shown as unknown providers.
- **Usage Editor & Skills UI Regressions**: Fixed usage query fields being reset while editing extractor code, corrected broken `skills.sh` links and empty descriptions, and fixed auto-query defaults plus number-input clearing in usage configuration.
- **Chinese Skills Terminology**: Unified Skills-related labels across settings panels in the `zh` locale so storage and sync options use consistent wording.
- **Environment & Preset Compatibility**: Added Bun global bin detection in CLI scan, adapted to the oh-my-openagent rename with backward compatibility, corrected the OpenCode `kimi-for-coding` preset, gated Gemini keychain parsing to macOS, and fixed an OpenClaw serializer panic on empty collections.
- **Linux UI Unresponsive on Startup**: Fixed a bug where the window UI (including native title bar buttons) couldn't receive clicks on Linux until the user manually maximized and restored the window. Root causes: (1) Tauri webview did not acquire keyboard focus after `show()` on Linux, so the first click was consumed by X11/Wayland click-to-activate (Tauri #10746, wry #637); (2) GTK surface's input region failed to renegotiate on the `visible:false → show()` path under some WebKitGTK/compositor combinations, leaving the entire window unresponsive. Mitigations: set `WEBKIT_DISABLE_COMPOSITING_MODE=1` at startup, and added a new `linux_fix::nudge_main_window` helper that performs `set_focus` + a ±1px no-op resize ~200ms after show, equivalent to a visually invisible "maximize-and-restore". Wired into all window-re-show paths (normal startup, deeplink, single_instance, tray `show_main`, lightweight exit).
- **Linux Drag Region on Header**: Removed `data-tauri-drag-region` from the top header bar on Linux to avoid triggering `gtk_window_begin_move_drag` paths affected by Tauri #13440 under Wayland. macOS drag behavior is preserved.
- **OpenCode / OpenClaw Stream Check Edge Cases**: Fixed custom-header passthrough, OpenClaw custom auth-header detection, Bedrock error messaging, and OpenCode default `baseURL` fallback handling in Stream Check.
- **Duplicate Toast on Provider Switch**: Fixed double toast notifications (proxy-required warning followed by switch-success) when switching to Copilot, ChatGPT, or OpenAI-format providers with the proxy not running.
- **Session Search Accuracy & Chinese Support**: Fixed session search result truncation across providers and switched FlexSearch tokenizer to full mode for proper Chinese substring matching.
- **Adaptive Thinking Reasoning Effort**: Fixed `resolve_reasoning_effort()` mapping adaptive thinking to `xhigh` instead of incorrectly using `high` in OpenAI format conversions.
- **Thinking Model Fallback Display**: Fixed the Claude provider form showing an empty Thinking model field after saving only a main model by applying read-only fallback to ANTHROPIC_MODEL.
- **Auth Tab Localization**: Fixed missing i18n translation keys for the settings auth tab label across all locale bundles.
- **Schema Migration Guard**: Fixed database migrations failing when skills or model_pricing tables did not exist by adding table-existence checks before ALTER and UPDATE operations.

### Docs

- **User Manual Refresh**: Updated the EN / ZH / JA manuals for tray submenus, lightweight mode, provider model fetching, session management, workspace files, WebDAV v2 behavior, OpenCode / OpenClaw activation, and other provider workflow improvements.
- **Community & Contribution Docs**: Added `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, bilingual issue / PR templates, Dependabot config, and CI quality checks.
- **Release Notes Risk Notice**: Added a Copilot reverse proxy risk notice and anchored highlight links in the v3.12.3 release notes across all three languages.
- **Sponsor Partners**: Added Shengsuanyun, LionCC, and DDS as sponsor partners in README across all languages.

---

## [3.12.3] - 2026-03-24

Major release adding GitHub Copilot reverse proxy support, macOS code signing & Apple notarization, intelligent reasoning effort mapping for o-series models, skill backup/restore lifecycle, proxy gzip compression, and critical fixes for WebDAV password safety, tool message parsing, and dark mode.

**Stats**: 36 commits | 107 files changed | +9,124 insertions | -802 deletions

### Added

- **GitHub Copilot Reverse Proxy**: Full GitHub Copilot integration as a Claude Code provider via OAuth Device Code flow; includes multi-account management, automatic token refresh, Anthropic ↔ OpenAI format conversion, real-time model list fetching, and usage statistics (#930)
- **Copilot Auth Center**: New Auth Center panel in Settings for managing GitHub accounts globally, with per-provider account binding via `meta.authBinding`
- **Tool Search Toggle**: Added `ENABLE_TOOL_SEARCH` env var support for Claude 2.1.76+; exposed as a checkbox in the provider Common Config editor (#930)
- **Reasoning Effort Mapping**: Two-tier `resolve_reasoning_effort()` for OpenAI o-series and GPT-5+ models — explicit `output_config.effort` takes priority, falling back to thinking `budget_tokens` thresholds (<4 000→low, 4 000–16 000→medium, ≥16 000→high); covers both Chat Completions and Responses API paths with 17 unit tests
- **OpenCode SQLite Backend**: Added SQLite session storage support for OpenCode alongside existing JSON backend; dual-backend scan with SQLite priority on ID conflicts, atomic session deletion, and path validation (#1401)
- **Skill Auto-Backup**: Skill files are automatically backed up to `~/.cc-switch/skill-backups/` before uninstall, with metadata preserved in `meta.json`; old backups pruned to keep at most 20
- **Skill Backup Restore & Delete**: Added list/restore/delete commands for skill backups; restore copies files back to SSOT, saves the DB record, and syncs to the current app with rollback on failure
- **macOS Code Signing & Notarization**: CI now imports an Apple Developer ID certificate, signs the universal binary, submits for Apple notarization, and staples the ticket to both `.app` and `.dmg`; a hard-fail verification step (`codesign --verify` + `spctl -a` + `stapler validate`) gates the release for both artifacts
- **Codex 1M Context Window Toggle**: One-click checkbox in Codex config editor to set `model_context_window = 1000000` with auto-populated `model_auto_compact_token_limit = 900000`; unchecking removes both fields
- **Disable Auto-Upgrade Toggle**: Added `DISABLE_AUTOUPDATER` env var checkbox in the Claude Common Config editor to prevent Claude Code from auto-upgrading

### Changed

- **Skills Cache Strategy**: Replaced `invalidateQueries` with direct `setQueryData` updates for skill install/uninstall/import operations; added `staleTime: Infinity` with `keepPreviousData` to eliminate loading flicker (#1573)
- **Proxy Gzip Compression**: Non-streaming proxy requests now auto-negotiate gzip compression instead of forcing `identity`; streaming requests conservatively keep `identity` to avoid SSE decompression errors
- **o1/o3 Model Compatibility**: Chat Completions proxy forwarding now correctly uses `max_completion_tokens` instead of `max_tokens` for OpenAI o-series models such as o1/o3/o4-mini (#1451)
- **OpenCode Model Variants**: Placed OpenCode model variants at top level instead of inside options for better discoverability (#1317)
- **Skills Import Flow**: Replaced implicit filesystem-based app inference with explicit `ImportSkillSelection` to prevent incorrect multi-app activation; added reconciliation to remove disabled/orphaned symlinks and MCP servers from live config
- **Claude 4.6 Context Window**: Updated Claude Opus 4.6 and Sonnet 4.6 context window from 200K to 1M across OpenClaw and OpenCode presets (GA release)
- **MiniMax Model Upgrade**: Updated MiniMax presets from M2.5 to M2.7 across Claude, OpenClaw, and OpenCode configurations with updated partner descriptions in all three locales
- **Xiaomi MiMo Model Upgrade**: Updated MiMo presets from mimo-v2-flash to mimo-v2-pro across all supported applications
- **AddProviderDialog Simplification**: Removed redundant OAuth tab, reducing dialog from 3 tabs to 2 (app-specific + universal)
- **Provider Form Advanced Options Collapse**: Model mapping, API format, and other advanced fields in the Claude provider form now auto-collapse when empty; auto-expands when any value is set or when a preset fills them in

### Fixed

- **WebDAV Password Silent Clear**: Fixed WebDAV password being silently wiped when ProviderList or UsageScriptModal saved settings by stripping `webdavSync` from frontend payloads and adding backend backfill logic in `merge_settings_for_save()` to preserve existing passwords
- **Tool Message Parsing**: Fixed tool_use/tool_result message classification across Claude (tool_result content blocks), Codex (function_call/function_call_output payloads), and Gemini (array content + toolCalls extraction) session providers (#1401)
- **Dark Mode Selector**: Changed Tailwind `darkMode` from `["selector", "class"]` to `["selector", ".dark"]` to ensure correct dark mode activation (#1596)
- **Copilot Request Fingerprint**: Unified Copilot request fingerprint headers across all API call sites to prevent User-Agent leakage and stream check mismatches
- **o-series Responses API Tokens**: Kept Responses API on the correct `max_output_tokens` field for o-series models instead of incorrectly injecting `max_completion_tokens`
- **Provider Form Double Submit**: Prevented duplicate submissions on rapid button clicks in provider add/edit forms (#1352)
- **Ghostty Session Restore**: Fixed Claude session restore in Ghostty terminal (#1506)
- **Skill ZIP Import Extension**: Added `.skill` file extension support in ZIP import dialog (#1240, #1455)
- **Skill ZIP Install Target App**: ZIP skill installs now use the currently active app instead of always defaulting to Claude
- **OpenClaw Active Card Highlight**: Fixed active OpenClaw provider card not being highlighted (#1419)
- **Responsive Layout with TOC**: Improved responsive design when TOC title exists (#1491)
- **Import Skills Dialog White Screen**: Added missing TooltipProvider in ImportSkillsDialog to prevent runtime crash when opening the dialog
- **Panel Bottom Blank Area**: Replaced hardcoded `h-[calc(100vh-8rem)]` with `flex-1 min-h-0` across all content panels to eliminate bottom gap caused by mismatched offset values

### Docs

- **Pricing Model ID Normalization**: Added documentation section explaining model ID normalization rules (prefix stripping, suffix trimming, `@`→`-` replacement) in EN/ZH/JA user manuals (#1591)
- **macOS Signed & Notarized**: Removed all `xattr` workaround instructions and "unidentified developer" warnings from README, README_ZH, installation guides (EN/ZH/JA), and FAQ pages (EN/ZH/JA); replaced with "signed and notarized by Apple" messaging

---

## [3.12.2] - 2026-03-12

Post-v3.12.1 work focuses on Common Config safety during proxy takeover and more reliable Codex TOML editing.

**Stats**: 5 commits | 22 files changed | +1,716 insertions | -288 deletions

### Added

- **Empty State Guidance**: Improved first-run experience with detailed import instructions and a conditional Common Config snippet hint for Claude/Codex/Gemini providers

### Changed

- **Proxy Takeover Restore Flow**: Proxy takeover hot-switch and provider sync now refresh the restore backup instead of overwriting live config files, rebuilding effective provider settings with Common Config applied so rollback preserves the real user configuration
- **Codex TOML Editing Engine**: Refactored Codex `config.toml` updates onto shared section-aware TOML helpers in Rust and TypeScript, covering `base_url` and `model` field edits across provider forms and takeover cleanup
- **Common Config Initialization Lifecycle**: Startup now auto-extracts Common Config snippets from clean live configs before takeover restoration, tracks explicit "snippet cleared" state, and persists a one-time legacy migration flag to avoid repeated backfills

### Fixed

- **Common Config Loss During Takeover**: Fixed cases where proxy takeover could drop Common Config changes, overwrite live configs during sync, or produce incomplete restore snapshots when switching providers
- **Codex Restore Snapshot Preservation**: Fixed Codex takeover restore backups so existing `mcp_servers` blocks survive provider hot-switches instead of being discarded; changed MCP backup preservation from wholesale table replacement to per-server-id merge so provider/common-config MCP updates win on conflict while live-only servers are retained
- **Cleared Snippet Resurrection**: Fixed startup auto-extraction recreating Common Config snippets that users had intentionally cleared
- **Codex `base_url` Misplacement**: Fixed Codex `base_url` extraction and editing to target the active `[model_providers.<name>]` section instead of appending to the file tail or confusing `mcp_servers.*.base_url` entries for provider endpoints

---

## [3.12.1] - 2026-03-12

### Patch Release

Stability-focused patch release fixing the Common Config modal infinite reopen loop, a WebDAV sync foreign key constraint failure, several i18n interpolation issues, and a Windows toolbar compact mode bug. Also adds **StepFun** provider presets, **OpenClaw input type selection** and **authHeader** support, upgrades Gemini to **3.1-pro**, and welcomes four new sponsor partners.

**Stats**: 19 commits | 56 files changed | +1,429 insertions | -396 deletions

### Added

#### Provider Presets

- **StepFun**: Added StepFun (阶跃星辰) provider presets including the step-3.5-flash model across supported applications (#1369, thanks @hengm3467)

#### OpenClaw Enhancements

- **Input Type Selection**: Added input type selection dropdown for model Advanced Options in OpenClaw configuration form (#1368, thanks @liuxxxu)
- **authHeader Field**: Added optional `authHeader` boolean to OpenClawProviderConfig for vendor-specific auth header support (e.g. Longcat), and refactored form state to reuse the shared type

#### Sponsor Partners

- **Micu API**: Added Micu API as sponsor partner with affiliate links
- **XCodeAPI**: Added XCodeAPI as sponsor partner
- **SiliconFlow**: Added SiliconFlow (硅基流动) as sponsor partner with affiliate links
- **CTok**: Added CTok as sponsor partner

### Changed

- **UCloud → Compshare**: Renamed UCloud provider to Compshare (优云智算) with full i18n support across all three locales (EN/ZH/JA)
- **Compshare Links**: Updated Compshare sponsor registration links to coding-plan page
- **Gemini Model Upgrade**: Upgraded default Gemini model from 2.5-pro to 3.1-pro in provider presets

### Fixed

#### Common Config & UI

- **Common Config Modal Loop**: Fixed an infinite reopen loop in the Common Config modal and added draft editing support to prevent data loss during edits
- **Toolbar Compact Mode (Windows)**: Fixed toolbar compact mode not triggering on Windows due to left-side overflow (#1375, thanks @zuoliangyu)
- **Session Search Index**: Fixed session search index not syncing with query data, causing stale list display after session deletion

#### Sync & Data

- **WebDAV Provider Health FK**: Fixed foreign key constraint failure when restoring `provider_health` table during WebDAV sync

#### Provider & Preset

- **Longcat authHeader**: Added missing `authHeader: true` to Longcat provider preset (#1377, thanks @wavever)
- **OpenClaw Tool Permissions**: Aligned OpenClaw tool permission profiles with upstream schema (#1355, thanks @bigsongeth)
- **X-Code API URL**: Corrected X-Code API URL from `www.x-code.cn` to `x-code.cc`

#### i18n & Localization

- **Stream Check Toast**: Fixed stream check toast i18n interpolation keys not matching translation placeholders
- **Proxy Startup Toast**: Fixed proxy startup toast not interpolating address and port values (#1399, thanks @Mason-mengze)
- **OpenCode API Format Label**: Renamed OpenCode API format label from "OpenAI" to "OpenAI Responses" for accuracy

---

## [3.12.0] - 2026-03-09

### Feature Release

This release restores the **Model Health Check (Stream Check)** UI, adds **OpenAI Responses API** format conversion, introduces the **Bedrock Optimizer** for thinking + cache injection, expands provider presets (Ucloud, Micu, X-Code API, Novita, Bailian For Coding), overhauls **OpenClaw config panels** with a JSON5 round-trip write engine, enhances **WebDAV sync** with dual-layer versioning, and delivers a comprehensive **i18n audit** fixing 69 missing keys alongside 20+ bug fixes.

**Stats**: 56 commits | 221 files changed | +20,582 insertions | -8,026 deletions

### Added

#### Stream Check (Model Health Check)

- **Restore Stream Check UI**: Brought back the model health check (Stream Check) panel for testing provider endpoint availability with live streaming validation
- **First-Run Confirmation**: Added a confirmation dialog on first use of Stream Check to inform users about the feature's purpose and network requests
- **OpenAI Chat Format Support**: Stream Check now supports `openai_chat` api_format, enabling health checks for providers using OpenAI-compatible endpoints

#### OpenAI Responses API

- **Responses API Format Conversion**: New `api_format = "openai_responses"` option enabling Anthropic Messages ↔ OpenAI Responses API bidirectional conversion for providers that implement the Responses API
- **Responses API Deduplication**: Deduplicated and improved the Responses API conversion logic, consolidating shared transformation code

#### Bedrock Optimizer

- **Bedrock Request Optimizer**: PRE-SEND optimizer that injects thinking parameters and cache control blocks into AWS Bedrock requests, enabling extended thinking and prompt caching on Bedrock endpoints (#1301)

#### OpenClaw Enhancements

- **JSON5 Round-Trip Write Engine**: Overhauled OpenClaw config panels with a JSON5 round-trip write engine that preserves comments, formatting, and ordering when saving configuration changes
- **Config Panel Improvements**: Redesigned EnvPanel as a full JSON editor, added `tools.profile` selection to ToolsPanel, introduced OpenClawHealthBanner for config validation warnings, and added legacy timeout migration support in Agents Defaults
- **Agent Model Dropdown**: Replaced text inputs with dropdown selects for OpenClaw agent model configuration, offering a curated list of available models
- **User-Agent Toggle**: Added a User-Agent header toggle for OpenClaw, defaulting to off to avoid potential compatibility issues with certain providers

#### Provider Presets

- **Ucloud**: Added Ucloud partner provider preset for Claude, Codex, and OpenClaw with endpointCandidates, unified apiKeyUrl, refreshed model defaults, and OpenClaw `templateValues` / `suggestedDefaults`
- **Micu**: Added Micu partner provider preset for Claude, Codex, OpenClaw, and OpenCode with OpenClaw `templateValues` / `suggestedDefaults`
- **X-Code API**: Added X-Code API partner provider preset for Claude, Codex, and OpenCode with endpointCandidates
- **Novita**: Added Novita provider presets and icon across all supported apps (#1192)
- **Bailian For Coding**: Added Bailian For Coding preset configuration (#1263)
- **SiliconFlow Partner Badge**: Added partner badge designation for SiliconFlow provider presets
- **Model Role Badges**: Added model role badges (e.g., Opus, Sonnet) to provider presets and reordered presets to prioritize Opus models

#### WebDAV Sync

- **Dual-Layer Versioning**: Added protocol v2 + db-v6 dual-layer versioning to WebDAV sync, enabling backward-compatible sync format evolution and automatic migration detection
- **Auto-Sync Confirmation**: Added a confirmation dialog when toggling WebDAV auto-sync on/off to prevent accidental changes

#### Usage & Data

- **Daily Rollups & Auto-Vacuum**: Added usage daily rollups for aggregated statistics, incremental auto-vacuum for storage management, and sync-aware backup that coordinates with WebDAV sync cycles
- **UsageFooter Extra Fields**: Added extra field display in UsageFooter component for normal mode, showing additional usage metadata (#1137)

#### Session Management

- **Session Deletion**: Added session deletion with per-provider cleanup and path safety validation, allowing users to remove individual conversation sessions

#### UI & Config

- **Auth Field Selector**: Restored Claude provider auth field selector supporting both AUTH_TOKEN and API_KEY authentication modes
- **Failover Toggle**: Moved failover toggle to display independently on the main page with a confirmation dialog for enabling/disabling
- **Common Config Auto-Extract**: Auto-extract Common Config Snippets from live configuration files on first run, seeding initial common config without manual setup
- **New Provider Page Improvements**: Improved the new provider page with API endpoint and model name fields (#1155)

### Changed

#### Architecture

- **Common Config Runtime Overlay**: Common Config is now applied as a runtime overlay during provider switching instead of being materialized (merged) into each provider's stored config. This preserves the original provider config in the database and applies common settings dynamically at write time
- **First-Run Auto-Extract**: On first run, Common Config Snippets are automatically extracted from the current live configuration files, eliminating the need for manual initial setup

### Fixed

#### Proxy & Streaming

- **OpenAI Streaming Conversion**: Fixed OpenAI ChatCompletion → Anthropic Messages streaming conversion that could produce malformed events under certain response structures
- **Codex /responses/compact Route**: Added support for Codex `/responses/compact` route in proxy forwarding (#1194)
- **Codex Common Config TOML Merge**: Fixed Codex Common Config to use structural TOML merge/subset instead of raw string comparison, correctly handling key ordering and formatting differences
- **Proxy Forwarder Failure Logs**: Improved proxy forwarder failure logging with more descriptive error messages

#### Provider & Preset

- **X-Code Rename**: Renamed "X-Code" provider to "X-Code API" for consistency with the official branding
- **SSSAiCode Missing /v1**: Added missing `/v1` path to SSSAiCode default endpoint for Codex and OpenCode
- **AICoding URL Fix**: Removed `www` prefix from aicoding.sh provider URLs to match the correct domain
- **New Provider Page Input Handling**: Fixed the new provider page so API endpoint / model fields handle line-break deletion correctly and added the missing `codexConfig.modelNameHint` i18n key for zh/en/ja

#### Platform

- **Cache Hit Token Statistics**: Fixed missing token statistics for cache hits in streaming responses (#1244)
- **Minimize-to-Tray Auto Exit**: Fixed issue where the application would automatically exit after being minimized to the system tray for a period of time (#1245)

#### i18n & Localization

- **Comprehensive i18n Audit**: Added 69 missing i18n keys and fixed hardcoded Chinese strings across the application, improving localization coverage for all three languages (zh/en/ja)
- **Model Test Panel i18n**: Corrected i18n key paths for model test panel title and description
- **JSON5 Slash Escaping**: Normalized JSON5 slash escaping and added i18n support for OpenClaw panel labels

#### UI

- **Skills Count Display**: Fixed skills count not displaying correctly when adding new skills (#1295)
- **Endpoint Speed Test**: Removed HTTP status code display from endpoint speed test results to reduce visual noise
- **Outline Button Text Tone**: Aligned outline button text color tone with usage refresh control for visual consistency (#1222)

### Performance

- **OpenClaw Config Write Skip**: Skip backup and atomic write when OpenClaw configuration content is unchanged, avoiding unnecessary I/O operations

### Documentation

- **User Manual i18n**: Restructured user manual for internationalization and added complete EN/JA translations alongside the existing ZH documentation
- **User Manual OpenClaw**: Added OpenClaw coverage and completed settings documentation for the user manual
- **UCloud CompShare Sponsor**: Added UCloud CompShare as a sponsor partner
- **Docs Directory Reorganization**: Reorganized docs directory structure, added user manual links to all three README files, removed cross-language links from user manual sections, and synced README features across EN/ZH/JA

### Maintenance

- **Periodic Maintenance Timer**: Consolidated periodic maintenance timers into a unified scheduler, combining vacuum and rollup operations into a single timer
- **OpenClaw Save Toast**: Removed backup path display from OpenClaw save toasts for cleaner notification messages

---

## [3.11.1] - 2026-02-28

### Hotfix Release

This release reverts the Partial Key-Field Merging architecture introduced in v3.11.0, restoring the proven "full config overwrite + Common Config Snippet" mechanism, and fixes several UI and platform compatibility issues.

**Stats**: 8 commits | 52 files changed | +3,948 insertions | -1,411 deletions

### Reverted

- **Restore Full Config Overwrite + Common Config Snippet** (revert 992dda5c): Reverted the partial key-field merging refactoring from v3.11.0 due to critical issues — non-whitelisted custom fields were lost during provider switching, backfill permanently stripped non-key fields from the database, and the whitelist required constant maintenance. Restores full config snapshot write, Common Config Snippet UI and backend commands, and 6 frontend components/hooks

### Changed

- **Proxy Panel Layout**: Moved proxy on/off toggle from accordion header into panel content area, placed directly above app takeover options, ensuring users see takeover configuration immediately after enabling the proxy
- **Manual Import for OpenCode/OpenClaw**: Removed auto-import on startup; empty state now shows an "Import Current Config" button, consistent with Claude/Codex/Gemini behavior

### Fixed

- **"Follow System" Theme Not Auto-Updating**: Delegated to Tauri's native theme tracking (`set_window_theme(None)`) so the WebView's `prefers-color-scheme` media query stays in sync with OS theme changes
- **Compact Mode Cannot Exit**: Restored `flex-1` on `toolbarRef` so `useAutoCompact`'s exit condition triggers correctly based on available width instead of content width
- **Proxy Takeover Toast Shows {{app}}**: Added missing `app` interpolation parameter to i18next `t()` calls for proxy takeover enabled/disabled messages
- **Windows Protocol Handler Side Effects**: Disabled environment check and one-click install on Windows to prevent unintended protocol handler registration

---

## [3.11.0] - 2026-02-26

### Feature Release

This release introduces **OpenClaw** as the fifth supported application, a full **Session Manager** for browsing conversation history across all apps, an independent **Backup Management** panel, **Oh My OpenCode (OMO)** integration, and 50+ other features, fixes, and improvements across 147 commits.

**Stats**: 147 commits | 274 files changed | +32,179 insertions | -5,467 deletions

### Added

#### OpenClaw Support (New Application)

- **OpenClaw Integration**: Full management support for OpenClaw as the fifth application in CC Switch, including provider switching, configuration panels (Env / Tools / Agents Defaults), Workspace file management (HEARTBEAT / BOOTSTRAP / BOOT), daily memory files, and additive overlay mode
- **OpenClaw Provider Presets**: 13+ built-in provider presets with brand icon and complete i18n (zh/en/ja)
- **OpenClaw Form Fields**: Dedicated provider form with providerKey input, model allowlist auto-registration, and default model button
- **OpenClaw Config Panels**: Env editor, Tools editor, and Agents Defaults editor backed by JSON5 read/write (`openclaw_config.rs`)

#### Session Manager

- **Session Manager**: Browse and search conversation history for Claude Code, Codex, Gemini CLI, OpenCode, and OpenClaw with table-of-contents navigation and in-session search
- **Session App Filter**: Auto-filter sessions by current app when entering the session page
- **Session Performance**: Parallel directory scanning and head-tail JSONL reading for faster session list loading

#### Backup Management

- **Backup Panel**: Independent backup management panel with configurable backup policy (max count, auto-cleanup) and backup rename support
- **Periodic Backup**: Hourly automatic backup timer during runtime
- **Pre-Migration Backup**: Automatic backup before database schema migrations with backfill warning
- **Delete Backup**: Delete individual backup files with confirmation dialog
- **Backup Time Fix**: Use local time instead of UTC for backup file names

#### Oh My OpenCode (OMO)

- **OMO Integration**: Full Oh My OpenCode config file management with agent model selection, category configuration, and recommended model fill
- **OMO Slim**: Lightweight oh-my-opencode-slim mode support with OmoVariant parameterization
- **OMO Cross-Exclusion**: Enforce OMO ↔ OMO Slim mutual exclusion at the database level

#### Workspace

- **Daily Memory Search**: Full-text search across daily memory files with date-sorted display
- **Clickable Paths**: Directory paths in workspace panels are now clickable; renamed “Today's Note” to “Add Memory”
- **Workspace Files Panel**: Manage bootstrap markdown files for OpenClaw (HEARTBEAT / BOOTSTRAP / BOOT types)

#### Provider Presets

- **AWS Bedrock**: Support for AKSK and API Key authentication modes (Claude and OpenCode)
- **SSAI Code**: Partner provider preset across all five apps
- **CrazyRouter**: Partner provider preset with custom icon
- **AICoding**: Partner provider preset with i18n promotion text
- **Bailian**: Renamed from Qwen Coder with new icon; updated domestic model providers to latest versions

#### Proxy & Network

- **Thinking Budget Rectifier**: New rectifier for thinking budget parameters with dedicated module (`thinking_budget_rectifier.rs`)
- **WebDAV Auto Sync**: Automatic periodic sync with large file protection mechanism

#### UI & UX

- **Theme Animation**: Circular reveal animation when toggling between light and dark themes
- **Claude Quick Toggles**: Quick toggle switches in the Claude config JSON editor for common settings
- **Dynamic Endpoint Hint**: Context-aware hint text in endpoint input based on API format selection
- **AppSwitcher Auto Compact**: Automatically collapse to compact mode based on available width, with smooth transition animation
- **App Transition**: Fade-in/fade-out animation when switching between OpenClaw and other apps
- **Silent Startup Conditional**: Show silent startup option only when launch-on-startup is enabled

#### Settings & Environment

- **First-Run Confirmation**: Confirmation dialogs for proxy and usage features on first use
- **Local Proxy Toggle**: `enableLocalProxy` setting to control proxy UI visibility on the home page
- **Environment Check**: More granular local environment detection (installed CLI tool versions, Volta path detection)

#### Usage & Pricing

- **Usage Dashboard Enhancement**: Auto-refresh control, robust formatting, and request log table improvements
- **New Model Pricing**: Added pricing data for claude-opus-4-6 and gpt-5.3-codex with incremental data seeding

### Changed

#### Architecture

- **Partial Key-Field Merging (⚠️ Breaking, reverted in v3.11.1)**: Provider switching now uses partial key-field merging instead of full config overwrite, preserving user's non-provider settings (plugins, MCP, permissions). The "Common Config Snippet" feature has been removed as it is no longer needed. Removes 6 frontend files and ~150 lines of backend dead code (#1098)
- **Manual Import**: Replaced auto-import on startup with manual “Import Current Config” button in empty state, reducing ~47 lines of startup code
- **OMO Variant Parameterization**: Eliminated ~250 lines of OMO/OMO Slim code duplication via `OmoVariant` struct with STANDARD/SLIM constants
- **OMO Common Config Removal**: Removed the two-layer merge system for OMO common config (-1,733 lines across 21 files)

#### Code Quality

- **ProviderForm Decomposition**: Extracted ProviderForm.tsx from 2,227 lines to 1,526 lines by splitting into 5 focused modules (opencodeFormUtils, useOmoModelSource, useOpencodeFormState, useOmoDraftState, useOpenclawFormState)
- **Shared MCP/Skills Components**: Extracted AppCountBar, AppToggleGroup, and ListItemRow shared components to eliminate duplication across MCP and Skills panels
- **OpenClaw TanStack Query Migration**: Migrated Env, Tools, and AgentsDefaults panels from manual useState/useEffect to centralized TanStack Query hooks

#### Settings Layout

- **Proxy Tab**: Split Advanced tab into dedicated Proxy tab (local proxy, failover, rectifiers, global outbound proxy); moved pricing config to Usage dashboard as collapsible accordion. SettingsPage reduced from ~716 to ~426 lines with 5-tab layout: General | Proxy | Advanced | Usage | About
- **Data Section Split**: Split data accordion into Import/Export and Cloud Sync sections for better discoverability

#### Terminal & Config

- **Unified Terminal Selection**: Consolidated terminal preference to global settings; added WezTerm support and terminal name mapping (iterm2 → iterm)
- **OpenClaw Agents Panel**: Primary model field set to read-only; detailed model fields (context window, max tokens, reasoning, cost) moved to advanced options
- **Claude Model Update**: Updated Claude model references from 4.5 to 4.6 across all provider presets

### Fixed

#### Critical

- **Windows Home Dir Regression**: Restored default home directory resolution on Windows to prevent providers/settings “disappearing” when `HOME` env var differs from the real user profile directory (Git/MSYS environments); auto-detects v3.10.3 legacy database location
- **Linux White Screen**: Disabled WebKitGTK hardware acceleration on AMD GPUs (Cezanne/Radeon Vega) to prevent EGL initialization failure causing blank screen on startup
- **OpenAI Beta Parameter**: Stopped appending `?beta=true` to OpenAI Chat Completions endpoints, fixing request failures for Nvidia and other `apiFormat=”openai_chat”` providers
- **Health Check Auth Mode**: Health check now respects provider's auth_mode setting instead of always using x-api-key header

#### Provider & Preset

- **OpenClaw /v1 Prefix**: Removed /v1 prefix from OpenClaw anthropic-messages presets to prevent double path (/v1/v1/messages) with Anthropic SDK auto-append
- **Opus Pricing**: Corrected Opus pricing from $15/$75 to $5/$25 and upgraded model ID to claude-opus-4-6
- **AIGoCode URLs**: Unified API base URL to https://api.aigocode.com across all apps; removed trailing /v1 suffix
- **Zhipu GLM**: Removed outdated partner status from Claude, OpenCode, and OpenClaw presets
- **API Key Visibility**: Restored API Key input field when creating new Claude providers (was incorrectly hidden for non-cloud_provider categories)

#### OMO / OMO Slim

- **OMO Slim Category Checks**: Added missing omo-slim category checks across add/form/mutation paths
- **OMO Slim Cache Invalidation**: Invalidate OMO Slim query cache after provider mutations to prevent stale UI state
- **OMO Recommended Models**: Synced agent/category recommended models with upstream sources; fixed provider/model format to pure model IDs
- **OMO Fill Feedback**: Added toast feedback when “Fill Recommended” button silently fails
- **OMO Last-Provider Restriction**: Removed last-provider deletion restriction for OMO/OMO Slim plugins
- **OpenCode Model Validation**: Reject saving OpenCode providers without at least one configured model

#### OpenClaw

- **OpenClaw P0-P3 Fixes**: Fixed 25 missing i18n keys, replaced key={index} with stable crypto.randomUUID(), excluded openclaw from ProxyToggle/FailoverToggle, added deep link merge_additive_config(), unified serde(flatten) naming, added directory existence checks, removed dead code, added duplicate key validation
- **OpenClaw Robustness**: Fixed EnvPanel visibleKeys using entry key names instead of array indices; added NaN guards; validated provider ID and model before import
- **OpenClaw i18n Dedup**: Merged duplicate openclaw i18n keys to restore provider form translations

#### Platform

- **Window Flash**: Prevented window flicker on silent startup (Windows)
- **Title Bar Theme**: Title bar now follows dark/light mode theme changes
- **Skills Path Separator**: Fixed path separator matching for skill installation status on Windows (supports both `/` and `\`)
- **WSL Conditional Compilation**: Added `#[cfg(target_os = “windows”)]` to WSL helper functions to eliminate dead_code warnings on non-Windows platforms

#### UI

- **Toolbar Clipping**: Removed toolbar height limit that was clipping AppSwitcher
- **Update Badge**: Show update badge instead of green check when a newer version is available
- **Session Button Visibility**: Only show Session Manager button for Claude and Codex apps
- **Directory Spacing**: Added vertical spacing between directory setting sections
- **Dark Mode Cards**: Unified SQL import/export card styling in dark mode
- **OpenClaw Scroll**: Enabled scrolling for OpenClaw configuration panel content

#### i18n & Localization

- **Session Manager i18n**: Replaced hardcoded Chinese strings with i18n keys for relative time, role labels, and UI elements
- **OpenClaw Default Model Label**: Renamed “Enable/Default” to “Set as Default / Current Default” with wider button
- **Daily Memory Sort**: Sort daily memory files by filename date (YYYY-MM-DD.md) instead of modification time
- **Backup Name i18n**: Use local time for backup file names

#### Other

- **Skill Doc URL**: Use actual branch from download_repo for documentation URL; switched from /tree/ to /blob/ pointing to SKILL.md
- **OpenCode Install Detection**: Added install.sh priority paths (OPENCODE_INSTALL_DIR > XDG_BIN_DIR > ~/bin > ~/.opencode/bin) with path dedup and cross-platform executable candidates
- **Provider Auto-Import**: Removed auto-import side effect from useProvidersQuery queryFn; users now trigger import manually via empty state button
- **Manual Backup Validation**: Treat missing database file as error during manual backup to prevent false success toast

### Performance

- **Session Panel Loading**: Parallel directory scanning and head-tail JSONL reading for Codex, OpenClaw, and OpenCode session providers
- **Query Cache Cleanup**: Removed unnecessary TanStack Query cache overhead for Tauri local IPC calls

### Documentation

- **Sponsors**: Added/updated SSSAiCode, Crazyrouter, AICoding, Right Code, and MiniMax sponsor entries across all README languages
- **User Manual**: Added user manual documentation (#979)

### Maintenance

- **Pre-Release Cleanup**: Removed debug logs, fixed clippy warnings, added missing Japanese translations, and formatted code
- **UI Exclusions**: Hidden MCP, Skills, proxy/pricing, stream check, and model test panels for OpenClaw where not applicable

---

## [3.10.3] - 2026-01-30

### Feature Release

This release introduces a generic API format selector, pricing configuration enhancements, and multiple UX improvements.

### Added

- **API Key Link for OpenCode**: API key link support for OpenCode provider form, enabling quick access to provider key management pages
- **AICodeMirror Partner Preset**: Added AICodeMirror partner preset for all apps (Claude, Codex, Gemini, OpenCode)
- **API Format Selector**: Generic API format chooser for Claude providers, replacing the OpenRouter-specific toggle. Supports Anthropic Messages (native) and OpenAI Chat Completions format
- **API Format Presets**: Allow preset providers to specify API format (anthropic or openai_chat) for third-party proxy services
- **Proxy Hint**: Display info toast when switching to OpenAI Chat format provider, reminding users to enable proxy
- **Pricing Config Enhancement**: Per-provider cost multiplier, pricing model source (request/response), request model logging, and enriched usage UI (#781)
- **Skills ZIP Install**: Install skills directly from local ZIP files with recursive scanning support
- **Preferred Terminal**: Choose preferred terminal app per platform (macOS: Terminal.app/iTerm2/Alacritty/Kitty/Ghostty; Windows: cmd/PowerShell/Windows Terminal; Linux: GNOME Terminal/Konsole/Xfce4/Alacritty/Kitty/Ghostty)
- **Silent Startup**: Option to prevent window popup on launch (#713)
- **OpenCode Environment Check**: Version detection with Go path scanning and one-click install from GitHub Releases
- **OpenCode Directory Sync**: Auto-sync all providers to live config on directory change with additive mode support
- **NVIDIA NIM Preset**: New provider preset for Claude and OpenCode with nvidia.svg icon
- **n1n.ai Preset**: New provider preset (#667)
- **Update Badge Icon**: Replace update badge dot with ArrowUpCircle icon
- **Linux ARM64**: CI build support for Linux ARM64 architecture

### Changed

- **API Format Migration**: Migrate api_format from settings_config to ProviderMeta to prevent polluting ~/.claude/settings.json
- **DeepSeek max_tokens**: Remove max_tokens clamp from proxy transform layer
- **Terminal Functions**: Consolidate redundant terminal launch functions
- **Home Dir Utility**: Consolidate get_home_dir into single public function
- **Kimi/Moonshot**: Upgrade provider presets to k2.5 model

### Fixed

- **Codex 404 & Timeout**: Fix 404 errors and connection timeout with custom base_url; improve /v1 prefix handling and system proxy detection (#760)
- **Proxy URL Building**: Fix duplicate /v1/v1 in URL; extend ?beta=true to /v1/chat/completions endpoint
- **OpenRouter Compat Mode**: Improve backward compatibility supporting number and string types
- **Gemini Visibility**: Correct Gemini default visibility to true (#818)
- **Footer Layout**: Correct footer layout in advanced settings tab
- **Claude Code Detection**: Prioritize native install path for detection
- **Tray Menu**: Simplify title labels and optimize menu separators (#796)
- **Duplicate Skills**: Prevent duplicate skill installation from different repos (#778)
- **Windows Tests**: Stabilize test environment (#644)
- **i18n**: Update apiFormatOpenAIChat label to mention proxy requirement
- **Error Display**: Use extractErrorMessage for complete error display in mutations
- **Sponsors**: Add AICodeMirror and reorder sponsor list

---

## [3.10.2] - 2026-01-24

### Patch Release

This maintenance release adds skill sync options and includes important bug fixes.

### Added

- **Skills**: Add skill sync method setting with symlink/copy options
- **Partners**: Add RightCode as official partner

### Fixed

- **Prompts**: Clear prompt file when all prompts are disabled
- **OpenCode**: Preserve extra model fields during serialization
- **Provider Form**: Backfill model fields when editing Claude provider

---

## [3.10.1] - 2026-01-23

### Patch Release

This maintenance release includes important bug fixes for Windows platform, UI improvements, and code quality enhancements.

### Added

- **Provider Icons**: Updated RightCode provider icon with improved visual design

### Changed

- **Proxy Rectifier**: Changed rectifier default state to disabled for better stability
- **Window Settings**: Reordered window settings and updated default values for improved UX
- **UI Layout**: Increased app icon collapse threshold from 3 to 4 icons
- **Code Quality**: Simplified `RectifierConfig` implementation using `#[derive(Default)]`

### Fixed

- **Windows Platform**:
  - Fixed terminal window closing immediately after execution on Windows
  - Corrected OpenCode config path resolution on Windows
- **UI Improvements**:
  - Fixed ProviderIcon color validation to prevent black icons from appearing
  - Unified layout padding across all panels for consistent spacing
  - Fixed panel content alignment with header constraints
- **Code Quality**: Resolved Rust Clippy warnings and applied consistent formatting

---

## [3.10.0] - 2026-01-21

### Feature Release

This release introduces OpenCode support and brings improvements across proxy, usage tracking, and overall UX.

### Added

- **OpenCode Support** - Manage OpenCode providers, MCP servers, and Skills, with first-launch import and full internationalization (#695)
- **Global Proxy** - Add global proxy settings for outbound network requests (#596)
- **Claude Rectifier** - Add thinking signature rectifier for Claude API (#595)
- **Health Check Enhancements** - Configurable prompt and CLI-compatible requests for stream health check (#623)
- **Per-Provider Config** - Support provider-specific configuration and persistence (#663)
- **App Visibility Controls** - Show/hide apps and keep tray menu in sync (Gemini hidden by default)
- **Takeover Compact Mode** - Use a compact AppSwitcher layout when showing 3+ visible apps
- **Keyboard Shortcut** - Press `ESC` to quickly go back/close panels (#670)
- **Terminal Improvements** - Provider-specific terminal button, `fnm` path support, and safer cross-platform launching (#564)
- **WSL Tool Detection** - Detect tool versions in WSL with additional security hardening (#627)
- **Skills Presets** - Add `baoyu-skills` preset repo and auto-supplement missing default repos

### Changed

- **Proxy Logging** - Simplify proxy log output (#585)
- **Pricing Editor UX** - Unify pricing edit modal with `FullScreenPanel`
- **Advanced Settings Layout** - Move rectifier section below failover for better flow
- **OpenRouter Compat Mode** - Disable OpenRouter compatibility mode by default and hide UI toggle

### Fixed

- **Auto Failover** - Switch to P1 immediately when enabling auto failover
- **Provider Edit Dialog** - Fix stale data when reopening provider editor after save (#654)
- **Deeplink** - Support multiple endpoints and prioritize `GOOGLE_GEMINI_BASE_URL` over `GEMINI_BASE_URL` (#597)
- **MCP (WSL)** - Skip `cmd /c` wrapper for WSL target paths (#592)
- **Usage Templates** - Add variable hints and validation fixes; prevent config leaking between providers (#628)
- **Gemini Timeout Format** - Convert timeout params to Gemini CLI format (#580)
- **UI** - Fix Select dropdown rendering in `FullScreenPanel`; auto-apply default icon color when unset
- **Usage UI** - Auto-adapt usage block offset based on action buttons width (#613)
- **Provider Endpoint** - Persist endpoint auto-select state (#611)
- **Provider Form** - Reset baseUrl and apiKey states when switching presets

---

## [3.9.1] - 2026-01-09

### Bug Fix Release

This release focuses on stability improvements and crash prevention.

### Added

- **Crash Logging** - Panic hook captures crash info to `~/.cc-switch/crash.log` with full stack traces (#562)
- **Release Logging** - Enable logging for release builds with automatic rotation (keeps 2 most recent files)
- **AIGoCode Icon** - Added colored icon for AIGoCode provider preset

### Fixed

- **Proxy Panic Prevention** - Graceful degradation when HTTP client initialization fails due to invalid proxy settings; falls back to no_proxy mode (#560)
- **UTF-8 Safety** - Fix potential panic when masking API keys or truncating logs containing multi-byte characters (Chinese, emoji, etc.) (#560)
- **Default Proxy Port** - Change default port from 5000 to 15721 to avoid conflict with macOS AirPlay Receiver (#560)
- **Windows Title** - Display "CC Switch" instead of default "Tauri app" in window title
- **Windows/Linux Spacing** - Remove extra 28px blank space below native titlebar introduced in v3.9.0
- **Flatpak Tray Icon** - Bundle libayatana-appindicator for tray icon support on Flatpak (#556)
- **Provider Preset** - Correct casing from "AiGoCode" to "AIGoCode" to match official branding

---

## [3.9.0] - 2026-01-07

### Stable Release

This stable release includes all changes from `3.9.0-1`, `3.9.0-2`, and `3.9.0-3`.

### Added

- **Local API Proxy** - High-performance local HTTP proxy for Claude Code, Codex, and Gemini CLI (Axum-based)
- **Per-App Takeover** - Independently route each app through the proxy with automatic live-config backup/redirect
- **Auto Failover** - Circuit breaker + smart failover with independent queues and health tracking per app
- **Universal Provider** - Shared provider configurations that can sync to Claude/Codex/Gemini (ideal for API gateways like NewAPI)
- **Provider Search Filter** - Quick filter to find providers by name (#435)
- **Keyboard Shortcut** - Open settings with Command+comma / Ctrl+comma (#436)
- **Deeplink Usage Config** - Import usage query config via deeplink (#400)
- **Provider Icon Colors** - Customize provider icon colors (#385)
- **Skills Multi-App Support** - Skills now support both Claude Code and Codex (#365)
- **Closable Toasts** - Close button for switch toast and all success toasts (#350)
- **Skip First-Run Confirmation** - Option to skip Claude Code first-run confirmation dialog
- **MCP Import** - Import MCP servers from installed apps
- **Common Config Snippet Extraction** - Extract reusable common config snippets from the current provider or editor content (Claude/Codex/Gemini)
- **Usage Enhancements** - Model extraction, request logging improvements, cache hit/creation metrics, and auto-refresh (#455, #508)
- **Error Request Logging** - Detailed logging for proxy requests (#401)
- **Linux Packaging** - Added RPM and Flatpak packaging targets
- **Provider Presets & Icons** - Added/updated partner presets and icons (e.g., MiMo, DMXAPI, Cubence)

### Changed

- **Usage Terminology** - Rename "Cache Read/Write" to "Cache Hit/Creation" across all languages (#508)
- **Model Pricing Data** - Refresh built-in model pricing table (Claude full version IDs, GPT-5 series, Gemini ID formats, and Chinese models) (#508)
- **Proxy Header Forwarding** - Switch to a blacklist approach and improve header passthrough compatibility (#508)
- **Failover Behavior** - Bypass timeout/retry configs when failover is disabled; update default failover timeout and circuit breaker values (#508, #521)
- **Provider Presets** - Update default model versions and change the default Qwen base URL (#517)
- **Skills Management** - Unify Skills management architecture with SSOT + React Query; improve caching for discoverable skills
- **Settings UX** - Reorder items in the Advanced tab for better discoverability
- **Proxy Active Theme** - Apply emerald theme when proxy takeover is active

### Fixed

- **Security** - Security fixes for JavaScript executor and usage script (#151)
- **Usage Timezone & Parsing** - Fix datetime picker timezone handling; improve token parsing/billing for Gemini and Codex formats (#508)
- **Windows Compatibility** - Improve MCP export and version check behavior to avoid terminal popups
- **Windows Startup** - Use system titlebar to prevent black screen on startup
- **WebView Compatibility** - Add fallback for crypto.randomUUID() on older WebViews
- **macOS Autostart** - Use `.app` bundle path to prevent terminal window popups
- **Database** - Add missing schema migrations; show an error dialog on initialization failure with a retry option
- **Import/Export** - Restrict SQL import to CC Switch exported backups only; refresh providers immediately after import
- **Prompts** - Allow saving prompts with empty content
- **MCP Sync** - Skip sync when the target CLI app is not installed
- **Common Config (Codex)** - Preserve MCP server `base_url` during extraction and remove provider-specific `model_providers` blocks
- **Proxy** - Improve takeover detection and stability; clean up model override env vars when switching providers in takeover mode (#508)
- **Skills** - Skip hidden directories during discovery; fix wrong skill repo branch
- **Settings Navigation** - Navigate to About tab when clicking update badge
- **UI** - Fix dialogs not opening on first click and improve window dragging area in `FullScreenPanel`

---

## [3.9.0-3] - 2025-12-29

### Beta Release

Third beta release with important bug fixes for Windows compatibility, UI improvements, and new features.

### Added

- **Universal Provider** - Support for universal provider configurations (#348)
- **Provider Search Filter** - Quick filter to find providers by name (#435)
- **Keyboard Shortcut** - Open settings with Command+comma / Ctrl+comma (#436)
- **Xiaomi MiMo Icon** - Added MiMo icon and Claude provider configuration (#470)
- **Usage Model Extraction** - Extract model info from usage statistics (#455)
- **Skip First-Run Confirmation** - Option to skip Claude Code first-run confirmation dialog
- **Exit Animations** - Added exit animation to FullScreenPanel dialogs
- **Fade Transitions** - Smooth fade transitions for app/view/panel switching

### Fixed

#### Windows
- Wrap npx/npm commands with `cmd /c` for MCP export
- Prevent terminal windows from appearing during version check

#### macOS
- Use .app bundle path for autostart to prevent terminal window popup

#### UI
- Resolve Dialog/Modal not opening on first click (#492)
- Improve dark mode text contrast for form labels
- Reduce header spacing and fix layout shift on view switch
- Prevent header layout shift when switching views

#### Database & Schema
- Add missing base columns migration for proxy_config
- Add backward compatibility check for proxy_config seed insert

#### Other
- Use local timezone and robust DST handling in usage stats (#500)
- Remove deprecated `sync_enabled_to_codex` call
- Gracefully handle invalid Codex config.toml during MCP sync
- Add missing translations for reasoning model and OpenRouter compat mode

### Improved

- **macOS Tray** - Use macOS tray template icon
- **Header Alignment** - Remove macOS titlebar tint, align custom header
- **Shadow Removal** - Cleaner UI by removing shadow styles
- **Code Inspector** - Added code-inspector-plugin for development
- **i18n** - Complete internationalization for usage panel and settings
- **Sponsor Logos** - Made sponsor logos clickable

### Stats

- 35 commits since v3.9.0-2
- 5 files changed in test/lint fixes

---

## [3.9.0-2] - 2025-12-20

### Beta Release

Second beta release focusing on proxy stability, import safety, and provider preset polish.

### Added

- **DMXAPI Partner** - Added DMXAPI as an official partner provider preset
- **Provider Icons** - Added provider icons for OpenRouter, LongCat, ModelScope, and AiHubMix

### Changed

- **Proxy (OpenRouter)** - Switched OpenRouter to passthrough mode for native Claude API

### Fixed

- **Import/Export** - Restrict SQL import to CC Switch exported backups only; refresh providers immediately after import
- **Proxy** - Respect existing Claude token when syncing; add fallback recovery for orphaned takeover state; remove global auto-start flag
- **Windows** - Add minimum window size to Windows platform config
- **UI** - Improve About section UI (#419) and unify header toolbar styling

### Stats

- 13 commits since v3.9.0-1

---

## [3.9.0-1] - 2025-12-18

### Beta Release

This beta release introduces the **Local API Proxy** feature, along with Skills multi-app support, UI improvements, and numerous bug fixes.

### Major Features

#### Local Proxy Server
- **Local HTTP Proxy** - High-performance proxy server built on Axum framework
- **Multi-app Support** - Unified proxy for Claude Code, Codex, and Gemini CLI API requests
- **Per-app Takeover** - Independent control over which apps route through the proxy
- **Live Config Takeover** - Automatically backs up and redirects CLI configurations to local proxy

#### Auto Failover
- **Circuit Breaker** - Automatically detects provider failures and triggers protection
- **Smart Failover** - Automatically switches to backup provider when current one is unavailable
- **Health Tracking** - Real-time monitoring of provider availability
- **Independent Failover Queues** - Each app maintains its own failover queue

#### Monitoring
- **Request Logging** - Detailed logging of all proxy requests
- **Usage Statistics** - Token consumption, latency, success rate metrics
- **Real-time Status** - Frontend displays proxy status and statistics

#### Skills Multi-App Support
- **Multi-app Support** - Skills now support both Claude and Codex (#365)
- **Multi-app Migration** - Existing Skills auto-migrate to multi-app structure (#378)
- **Installation Path Fix** - Use directory basename for skill installation path (#358)

### Added
- **Provider Icon Colors** - Customize provider icon colors (#385)
- **Deeplink Usage Config** - Import usage query config via deeplink (#400)
- **Error Request Logging** - Detailed logging for proxy requests (#401)
- **Closable Toast** - Added close button to switch notification toast (#350)
- **Icon Color Component** - ProviderIcon component supports color prop (#384)

### Fixed

#### Proxy Related
- Takeover Codex base_url via model_provider
- Harden crash recovery with fallback detection
- Sync UI when active provider differs from current setting
- Resolve circuit breaker race condition and error classification
- Stabilize live takeover and provider editing
- Reset health badges when proxy stops
- Retry failover for all HTTP errors including 4xx
- Fix HalfOpen counter underflow and config field inconsistencies
- Resolve circuit breaker state persistence and HalfOpen deadlock
- Auto-recover live config after abnormal exit
- Update live backup when hot-switching provider in proxy mode
- Wait for server shutdown before exiting app
- Disable auto-start on app launch by resetting enabled flag on stop
- Sync live config tokens to database before takeover
- Resolve 404 error and auto-setup proxy targets

#### MCP Related
- Skip sync when target CLI app is not installed
- Improve upsert and import robustness
- Use browser-compatible platform detection for MCP presets

#### UI Related
- Restore fade transition for Skills button
- Add close button to all success toasts
- Prevent card jitter when health badge appears
- Update SettingsPage tab styles (#342)

#### Other
- Fix Azure website link (#407)
- Add fallback to provider config for usage credentials (#360)
- Fix Windows black screen on startup (use system titlebar)
- Add fallback for crypto.randomUUID() on older WebViews
- Use correct npm package for Codex CLI version check
- Security fixes for JavaScript executor and usage script (#151)

### Improved
- **Proxy Active Theme** - Apply emerald theme when proxy takeover is active
- **Card Animation** - Improved provider card hover animation
- **Remove Restart Prompt** - No longer prompts restart when switching providers

### Technical
- Implement per-app takeover mode
- Proxy module contains 20+ Rust files with complete layered architecture
- Add 5 new database tables for proxy functionality
- Modularize handlers.rs to reduce code duplication
- Remove is_proxy_target in favor of failover_queue

### Stats
- 55 commits since v3.8.2
- 164 files changed
- +22,164 / -570 lines

---

## [3.8.0] - 2025-11-28

### Major Updates

- **Persistence architecture upgrade** - Moved from single JSON storage to SQLite + JSON dual-layer; added schema versioning, transactions, and SQL import/export; first launch auto-migrates `config.json` to SQLite while keeping originals safe.
- **Brand new UI** - Full layout redesign, unified component/ConfirmDialog styles, smoother animations, overscroll disabled; Tailwind CSS downgraded to v3.4 for compatibility.
- **Japanese language support** - UI now localized in Chinese/English/Japanese.

### Added

- **Skills recursive scanning** - Discovers nested `SKILL.md` files across multi-level directories; same-name skills allowed by full-path dedup.
- **Provider icons** - Presets ship with default icons; custom icon colors; icons retained when duplicating providers.
- **Auto launch on startup** - One-click enable/disable using Registry/LaunchAgent/XDG autostart.
- **Provider preset** - Added MiniMax partner preset.
- **Form validation** - Required fields get real-time validation and unified toast messaging.

### Fixed

- **Custom endpoints loss** - Switched provider updates to `UPDATE` to avoid cascade deletes from `INSERT OR REPLACE`.
- **Gemini config writing** - Correctly writes custom env vars to `.env` and keeps auth configs isolated.
- **Provider validation** - Handles missing current provider IDs and preserves icon fields on duplicate.
- **Linux rendering** - Fixed WebKitGTK DMA-BUF rendering and preserved user `.desktop` customizations.
- **Misc** - Removed redundant usage queries; corrected DMXAPI auth token field; restored missing deeplink translations; fixed usage script template init.

### Technical

- **Database modules** - Added `schema`, `backup`, `migration`, and DAO layers for providers/MCP/prompts/skills/settings.
- **Service modularization** - Split provider service into live/auth/endpoints/usage modules; deeplink parsing/import logic modularized.
- **Code cleanup** - Removed legacy JSON-era import/export, unused MCP types; unified error handling; tests migrated to SQLite backend and MSW handlers updated.

### Migration Notes

- First launch auto-migrates data from `config.json` to SQLite and device settings to `settings.json`; originals kept; error dialog on failure; dry-run supported.

### Stats

- 51 commits since v3.7.1; 207 files changed; +17,297 / -6,870 lines. See [release-note-v3.8.0](docs/release-notes/v3.8.0-en.md) for details.

---

## [3.7.1] - 2025-11-22

### Fixed

- **Skills third-party repository installation** (#268) - Fixed installation failure for skills repositories with custom subdirectories (e.g., `ComposioHQ/awesome-claude-skills`)
- **Gemini configuration persistence** - Resolved issue where settings.json edits were lost when switching providers
- **Dialog overlay click protection** - Prevented dialogs from closing when clicking outside, avoiding accidental form data loss (affects 11 dialog components)

### Added

- **Gemini configuration directory support** (#255) - Added custom configuration directory option for Gemini in settings
- **ArchLinux installation support** (#259) - Added AUR installation via `paru -S cc-switch-bin`

### Improved

- **Skills error messages i18n** - Added 28+ detailed error messages (English & Chinese) with specific resolution suggestions
- **Download timeout** - Extended from 15s to 60s to reduce network-related false positives
- **Code formatting** - Applied unified Rust (`cargo fmt`) and TypeScript (`prettier`) formatting standards

### Reverted

- **Auto-launch on system startup** - Temporarily reverted feature pending further testing and optimization

---

## [3.7.0] - 2025-11-19

### Major Features

#### Gemini CLI Integration

- **Complete Gemini CLI support** - Third major application added alongside Claude Code and Codex
- **Dual-file configuration** - Support for both `.env` and `settings.json` file formats
- **Environment variable detection** - Auto-detect `GOOGLE_GEMINI_BASE_URL`, `GEMINI_MODEL`, etc.
- **MCP management** - Full MCP configuration capabilities for Gemini
- **Provider presets**
  - Google Official (OAuth authentication)
  - PackyCode (partner integration)
  - Custom endpoint support
- **Deep link support** - Import Gemini providers via `ccswitch://` protocol
- **System tray integration** - Quick-switch Gemini providers from tray menu
- **Backend modules** - New `gemini_config.rs` (20KB) and `gemini_mcp.rs`

#### MCP v3.7.0 Unified Architecture

- **Unified management panel** - Single interface for Claude/Codex/Gemini MCP servers
- **SSE transport type** - New Server-Sent Events support alongside stdio/http
- **Smart JSON parser** - Fault-tolerant parsing of various MCP config formats
- **Extended field support** - Preserve custom fields in Codex TOML conversion
- **Codex format correction** - Proper `[mcp_servers]` format (auto-cleanup of incorrect `[mcp.servers]`)
- **Import/export system** - Unified import from Claude/Codex/Gemini live configs
- **UX improvements**
  - Default app selection in forms
  - JSON formatter for config validation
  - Improved layout and visual hierarchy
  - Better validation error messages

#### Claude Skills Management System

- **GitHub repository integration** - Auto-scan and discover skills from GitHub repos
- **Pre-configured repositories**
  - `ComposioHQ/awesome-claude-skills` (curated collection)
  - `anthropics/skills` (official Anthropic skills)
  - `cexll/myclaude` (community, with subdirectory scanning)
- **Lifecycle management**
  - One-click install to `~/.claude/skills/`
  - Safe uninstall with state tracking
  - Update checking (infrastructure ready)
- **Custom repository support** - Add any GitHub repo as a skill source
- **Subdirectory scanning** - Optional `skillsPath` for repos with nested skill directories
- **Backend architecture** - `SkillService` (526 lines) with GitHub API integration
- **Frontend interface**
  - SkillsPage: Browse and manage skills
  - SkillCard: Visual skill presentation
  - RepoManager: Repository management dialog
- **State persistence** - Installation state stored in `skills.json`
- **Full i18n support** - Complete Chinese/English translations (47+ keys)

#### Prompts (System Prompts) Management

- **Multi-preset management** - Create, edit, and switch between multiple system prompts
- **Cross-app support**
  - Claude: `~/.claude/CLAUDE.md`
  - Codex: `~/.codex/AGENTS.md`
  - Gemini: `~/.gemini/GEMINI.md`
- **Markdown editor** - Full-featured CodeMirror 6 editor with syntax highlighting
- **Smart synchronization**
  - Auto-write to live files on enable
  - Content backfill protection (save current before switching)
  - First-launch auto-import from live files
- **Single-active enforcement** - Only one prompt can be active at a time
- **Delete protection** - Cannot delete active prompts
- **Backend service** - `PromptService` (213 lines) with CRUD operations
- **Frontend components**
  - PromptPanel: Main management interface (177 lines)
  - PromptFormModal: Edit dialog with validation (160 lines)
  - MarkdownEditor: CodeMirror integration (159 lines)
  - usePromptActions: Business logic hook (152 lines)
- **Full i18n support** - Complete Chinese/English translations (41+ keys)

#### Deep Link Protocol (ccswitch://)

- **Protocol registration** - `ccswitch://` URL scheme for one-click imports
- **Provider import** - Import provider configurations from URLs or shared links
- **Lifecycle integration** - Deep link handling integrated into app startup
- **Cross-platform support** - Works on Windows, macOS, and Linux

#### Environment Variable Conflict Detection

- **Claude & Codex detection** - Identify conflicting environment variables
- **Gemini auto-detection** - Automatic environment variable discovery
- **Conflict management** - UI for resolving configuration conflicts
- **Prevention system** - Warn before overwriting existing configurations

### New Features

#### Provider Management

- **DouBaoSeed preset** - Added ByteDance's DouBao provider
- **Kimi For Coding** - Moonshot AI coding assistant
- **BaiLing preset** - BaiLing AI integration
- **Removed AnyRouter preset** - Discontinued provider
- **Model configuration** - Support for custom model names in Codex and Gemini
- **Provider notes field** - Add custom notes to providers for better organization

#### Configuration Management

- **Common config migration** - Moved Claude common config snippets from localStorage to `config.json`
- **Unified persistence** - Common config snippets now shared across all apps
- **Auto-import on first launch** - Automatically import configs from live files on first run
- **Backfill priority fix** - Correct priority handling when enabling prompts

#### UI/UX Improvements

- **macOS native design** - Migrated color scheme to macOS native design system
- **Window centering** - Default window position centered on screen
- **Password input fixes** - Disabled Edge/IE reveal and clear buttons
- **URL overflow prevention** - Fixed overflow in provider cards
- **Error notification enhancement** - Copy-to-clipboard for error messages
- **Tray menu sync** - Real-time sync after drag-and-drop sorting

### Improvements

#### Architecture

- **MCP v3.7.0 cleanup** - Removed legacy code and warnings
- **Unified structure** - Default initialization with v3.7.0 unified structure
- **Backward compatibility** - Compilation fixes for older configs
- **Code formatting** - Applied consistent formatting across backend and frontend

#### Platform Compatibility

- **Windows fix** - Resolved winreg API compatibility issue (v0.52)
- **Safe pattern matching** - Replaced `unwrap()` with safe patterns in tray menu

#### Configuration

- **MCP sync on switch** - Sync MCP configs for all apps when switching providers
- **Gemini form sync** - Fixed form fields syncing with environment editor
- **Gemini config reading** - Read from both `.env` and `settings.json`
- **Validation improvements** - Enhanced input validation and boundary checks

#### Internationalization

- **JSON syntax fixes** - Resolved syntax errors in locale files
- **App name i18n** - Added internationalization support for app names
- **Deduplicated labels** - Reused providerForm keys to reduce duplication
- **Gemini MCP title** - Added missing Gemini MCP panel title

### Bug Fixes

#### Critical Fixes

- **Usage script validation** - Added input validation and boundary checks
- **Gemini validation** - Relaxed validation when adding providers
- **TOML quote normalization** - Handle CJK quotes to prevent parsing errors
- **MCP field preservation** - Preserve custom fields in Codex TOML editor
- **Password input** - Fixed white screen crash (FormLabel → Label)

#### Stability

- **Tray menu safety** - Replaced unwrap with safe pattern matching
- **Error isolation** - Tray menu update failures don't block main operations
- **Import classification** - Set category to custom for imported default configs

#### UI Fixes

- **Model placeholders** - Removed misleading model input placeholders
- **Base URL population** - Auto-fill base URL for non-official providers
- **Drag sort sync** - Fixed tray menu order after drag-and-drop

### Technical Improvements

#### Code Quality

- **Type safety** - Complete TypeScript type coverage across codebase
- **Test improvements** - Simplified boolean assertions in tests
- **Clippy warnings** - Fixed `uninlined_format_args` warnings
- **Code refactoring** - Extracted templates, optimized logic flows

#### Dependencies

- **Tauri** - Updated to 2.8.x series
- **Rust dependencies** - Added `anyhow`, `zip`, `serde_yaml`, `tempfile` for Skills
- **Frontend dependencies** - Added CodeMirror 6 packages for Markdown editor
- **winreg** - Updated to v0.52 (Windows compatibility)

#### Performance

- **Startup optimization** - Removed legacy migration scanning
- **Lock management** - Improved RwLock usage to prevent deadlocks
- **Background query** - Enabled background mode for usage polling

### Statistics

- **Total commits**: 85 commits from v3.6.0 to v3.7.0
- **Code changes**: 152 files changed, 18,104 insertions(+), 3,732 deletions(-)
- **New modules**:
  - Skills: 2,034 lines (21 files)
  - Prompts: 1,302 lines (20 files)
  - Gemini: ~1,000 lines (multiple files)
  - MCP refactor: ~3,000 lines (refactored)

### Strategic Positioning

v3.7.0 represents a major evolution from "Provider Switcher" to **"All-in-One AI CLI Management Platform"**:

1. **Capability Extension** - Skills provide external ability integration
2. **Behavior Customization** - Prompts enable AI personality presets
3. **Configuration Unification** - MCP v3.7.0 eliminates app silos
4. **Ecosystem Openness** - Deep links enable community sharing
5. **Multi-AI Support** - Claude/Codex/Gemini trinity
6. **Intelligent Detection** - Auto-discovery of environment conflicts

### Notes

- Users upgrading from v3.1.0 or earlier should first upgrade to v3.2.x for one-time migration
- Skills and Prompts management are new features requiring no migration
- Gemini CLI support requires Gemini CLI to be installed separately
- MCP v3.7.0 unified structure is backward compatible with previous configs

## [3.6.0] - 2025-11-07

### ✨ New Features

- **Provider Duplicate** - Quick duplicate existing provider configurations for easy variant creation
- **Edit Mode Toggle** - Show/hide drag handles to optimize editing experience
- **Custom Endpoint Management** - Support multi-endpoint configuration for aggregator providers
- **Usage Query Enhancements**
  - Auto-refresh interval: Support periodic automatic usage query
  - Test Script API: Validate JavaScript scripts before execution
  - Template system expansion: Custom blank template, support for access token and user ID parameters
- **Configuration Editor Improvements**
  - Add JSON format button
  - Real-time TOML syntax validation for Codex configuration
- **Auto-sync on Directory Change** - When switching Claude/Codex config directories (e.g., WSL environment), automatically sync current provider to new directory without manual operation
- **Load Live Config When Editing Active Provider** - When editing the currently active provider, prioritize displaying the actual effective configuration to protect user manual modifications
- **New Provider Presets** - DMXAPI, Azure Codex, AnyRouter, AiHubMix, MiniMax
- **Partner Promotion Mechanism** - Support ecosystem partner promotion (e.g., Zhipu GLM Z.ai)

### 🔧 Improvements

- **Configuration Directory Switching**
  - Introduced unified post-change sync utility (`postChangeSync.ts`)
  - Auto-sync current providers to new directory when changing Claude/Codex config directories
  - Perfect support for WSL environment switching
  - Auto-sync after config import to ensure immediate effectiveness
  - Use Result pattern for graceful error handling without blocking main flow
  - Distinguish "fully successful" and "partially successful" states for precise user feedback
- **UI/UX Enhancements**
  - Provider cards: Unique icons and color identification
  - Unified border design system across all components
  - Drag interaction optimization: Push effect animation, improved handle icons
  - Enhanced current provider visual feedback
  - Dialog size standardization and layout consistency
  - Form experience: Optimized model placeholders, simplified provider hints, category-specific hints
- **Complete Internationalization Coverage**
  - Error messages internationalization
  - Tray menu internationalization
  - All UI components internationalization
- **Usage Display Moved Inline** - Usage display moved next to enable button

### 🐛 Bug Fixes

- **Configuration Sync**
  - Fixed `apiKeyUrl` priority issue
  - Fixed MCP sync-to-other-side functionality failure
  - Fixed sync issues after config import
  - Prevent silent fallback and data loss on config error
- **Usage Query**
  - Fixed auto-query interval timing issue
  - Ensure refresh button shows loading animation on click
- **UI Issues**
  - Fixed name collision error (`get_init_error` command)
  - Fixed language setting rollback after successful save
  - Fixed language switch state reset (dependency cycle)
  - Fixed edit mode button alignment
- **Configuration Management**
  - Fixed Codex API Key auto-sync
  - Fixed endpoint speed test functionality
  - Fixed provider duplicate insertion position (next to original provider)
  - Fixed custom endpoint preservation in edit mode
- **Startup Issues**
  - Force exit on config error (no silent fallback)
  - Eliminate code duplication causing initialization errors

### 🏗️ Technical Improvements (For Developers)

**Backend Refactoring (Rust)** - Completed 5-phase refactoring:

- **Phase 1**: Unified error handling (`AppError` + i18n error messages)
- **Phase 2**: Command layer split by domain (`commands/{provider,mcp,config,settings,plugin,misc}.rs`)
- **Phase 3**: Integration tests and transaction mechanism (config snapshot + failure rollback)
- **Phase 4**: Extracted Service layer (`services/{provider,mcp,config,speedtest}.rs`)
- **Phase 5**: Concurrency optimization (`RwLock` instead of `Mutex`, scoped guard to avoid deadlock)

**Frontend Refactoring (React + TypeScript)** - Completed 4-stage refactoring:

- **Stage 1**: Test infrastructure (vitest + MSW + @testing-library/react)
- **Stage 2**: Extracted custom hooks (`useProviderActions`, `useMcpActions`, `useSettings`, `useImportExport`, etc.)
- **Stage 3**: Component splitting and business logic extraction
- **Stage 4**: Code cleanup and formatting unification

**Testing System**:

- Hooks unit tests 100% coverage
- Integration tests covering key processes (App, SettingsDialog, MCP Panel)
- MSW mocking backend API to ensure test independence

**Code Quality**:

- Unified parameter format: All Tauri commands migrated to camelCase (Tauri 2 specification)
- `AppType` renamed to `AppId`: Semantically clearer
- Unified parsing with `FromStr` trait: Centralized `app` parameter parsing
- Eliminate code duplication: DRY violations cleanup
- Remove unused code: `missing_param` helper function, deprecated `tauri-api.ts`, redundant `KimiModelSelector` component

**Internal Optimizations**:

- **Removed Legacy Migration Logic**: v3.6 removed v1 config auto-migration and copy file scanning logic
  - ✅ **Impact**: Improved startup performance, cleaner code
  - ✅ **Compatibility**: v2 format configs fully compatible, no action required
  - ⚠️ **Note**: Users upgrading from v3.1.0 or earlier should first upgrade to v3.2.x or v3.5.x for one-time migration, then upgrade to v3.6
- **Command Parameter Standardization**: Backend unified to use `app` parameter (values: `claude` or `codex`)
  - ✅ **Impact**: More standardized code, friendlier error prompts
  - ✅ **Compatibility**: Frontend fully adapted, users don't need to care about this change

### 📦 Dependencies

- Updated to Tauri 2.8.x
- Updated to TailwindCSS 4.x
- Updated to TanStack Query v5.90.x
- Maintained React 18.2.x and TypeScript 5.3.x

## [3.5.0] - 2025-01-15

### ⚠ Breaking Changes

- Tauri commands only accept the `app` parameter (`claude`/`codex`); removed `app_type`/`appType` compatibility.
- Frontend types are standardized to `AppId` (removed `AppType` export); variable naming is standardized to `appId`.

### ✨ New Features

- **MCP (Model Context Protocol) Management** - Complete MCP server configuration management system
  - Add, edit, delete, and toggle MCP servers in `~/.claude.json`
  - Support for stdio and http server types with command validation
  - Built-in templates for popular MCP servers (mcp-fetch, etc.)
  - Real-time enable/disable toggle for MCP servers
  - Atomic file writing to prevent configuration corruption
- **Configuration Import/Export** - Backup and restore your provider configurations
  - Export all configurations to JSON file with one click
  - Import configurations with validation and automatic backup
  - Automatic backup rotation (keeps 10 most recent backups)
  - Progress modal with detailed status feedback
- **Endpoint Speed Testing** - Test API endpoint response times
  - Measure latency to different provider endpoints
  - Visual indicators for connection quality
  - Help users choose the fastest provider

### 🔧 Improvements

- Complete internationalization (i18n) coverage for all UI components
- Enhanced error handling and user feedback throughout the application
- Improved configuration file management with better validation
- Added new provider presets: Longcat, kat-coder
- Updated GLM provider configurations with latest models
- Refined UI/UX with better spacing, icons, and visual feedback
- Enhanced tray menu functionality and responsiveness
- **Standardized release artifact naming** - All platform releases now use consistent version-tagged filenames:
  - macOS: `CC-Switch-v{version}-macOS.tar.gz` / `.zip`
  - Windows: `CC-Switch-v{version}-Windows.msi` / `-Portable.zip`
  - Linux: `CC-Switch-v{version}-Linux.AppImage` / `.deb`

### 🐛 Bug Fixes

- Fixed layout shifts during provider switching
- Improved config file path handling across different platforms
- Better error messages for configuration validation failures
- Fixed various edge cases in configuration import/export

### 📦 Technical Details

- Enhanced `import_export.rs` module with backup management
- New `claude_mcp.rs` module for MCP configuration handling
- Improved state management and lock handling in Rust backend
- Better TypeScript type safety across the codebase

## [3.4.0] - 2025-10-01

### ✨ Features

- Enable internationalization via i18next with a Chinese default and English fallback, plus an in-app language switcher
- Add Claude plugin sync while retiring the legacy VS Code integration controls (Codex no longer requires settings.json edits)
- Extend provider presets with optional API key URLs and updated models, including DeepSeek-V3.1-Terminus and Qwen3-Max
- Support portable mode launches and enforce a single running instance to avoid conflicts

### 🔧 Improvements

- Allow minimizing the window to the system tray and add macOS Dock visibility management for tray workflows
- Refresh the Settings modal with a scrollable layout, save icon, and cleaner language section
- Smooth provider toggle states with consistent button widths/icons and prevent layout shifts when switching between Claude and Codex
- Adjust the Windows MSI installer to target per-user LocalAppData and improve component tracking reliability

### 🐛 Fixes

- Remove the unnecessary OpenAI auth requirement from third-party provider configurations
- Fix layout shifts while switching app types with Claude plugin sync enabled
- Align Enable/In Use button states to avoid visual jank across app views

## [3.3.0] - 2025-09-22

### ✨ Features

- Add “Apply to VS Code / Remove from VS Code” actions on provider cards, writing settings for Code/Insiders/VSCodium variants _(Removed in 3.4.x)_
- Enable VS Code auto-sync by default with window broadcast and tray hooks so Codex switches sync silently _(Removed in 3.4.x)_
- Extend the Codex provider wizard with display name, dedicated API key URL, and clearer guidance
- Introduce shared common config snippets with JSON/TOML reuse, validation, and consistent error surfaces

### 🔧 Improvements

- Keep the tray menu responsive when the window is hidden and standardize button styling and copy
- Disable modal backdrop blur on Linux (WebKitGTK/Wayland) to avoid freezes; restore the window when clicking the macOS Dock icon
- Support overriding config directories on WSL, refine placeholders/descriptions, and fix VS Code button wrapping on Windows
- Add a `created_at` timestamp to provider records for future sorting and analytics

### 🐛 Fixes

- Correct regex escapes and common snippet trimming in the Codex wizard to prevent validation issues
- Harden the VS Code sync flow with more reliable TOML/JSON parsing while reducing layout jank
- Bundle `@codemirror/lint` to reinstate live linting in config editors

## [3.2.0] - 2025-09-13

### ✨ New Features

- System tray provider switching with dynamic menu for Claude/Codex
- Frontend receives `provider-switched` events and refreshes active app
- Built-in update flow via Tauri Updater plugin with dismissible UpdateBadge

### 🔧 Improvements

- Single source of truth for provider configs; no duplicate copy files
- One-time migration imports existing copies into `config.json` and archives originals
- Duplicate provider de-duplication by name + API key at startup
- Atomic writes for Codex `auth.json` + `config.toml` with rollback on failure
- Logging standardized (Rust): use `log::{info,warn,error}` instead of stdout prints
- Tailwind v4 integration and refined dark mode handling

### 🐛 Fixes

- Remove/minimize debug console logs in production builds
- Fix CSS minifier warnings for scrollbar pseudo-elements
- Prettier formatting across codebase for consistent style

### 📦 Dependencies

- Tauri: 2.8.x (core, updater, process, opener, log plugins)
- React: 18.2.x · TypeScript: 5.3.x · Vite: 5.x

### 🔄 Notes

- `connect-src` CSP remains permissive for compatibility; can be tightened later as needed

## [3.1.1] - 2025-09-03

### 🐛 Bug Fixes

- Fixed the default codex config.toml to match the latest modifications
- Improved provider configuration UX with custom option

### 📝 Documentation

- Updated README with latest information

## [3.1.0] - 2025-09-01

### ✨ New Features

- **Added Codex application support** - Now supports both Claude Code and Codex configuration management
  - Manage auth.json and config.toml for Codex
  - Support for backup and restore operations
  - Preset providers for Codex (Official, PackyCode)
  - API Key auto-write to auth.json when using presets
- **New UI components**
  - App switcher with segmented control design
  - Dual editor form for Codex configuration
  - Pills-style app switcher with consistent button widths
- **Enhanced configuration management**
  - Multi-app config v2 structure (claude/codex)
  - Automatic v1→v2 migration with backup
  - OPENAI_API_KEY validation for non-official presets
  - TOML syntax validation for config.toml

### 🔧 Technical Improvements

- Unified Tauri command API with app_type parameter
- Backward compatibility for app/appType parameters
- Added get_config_status/open_config_folder/open_external commands
- Improved error handling for empty config.toml

### 🐛 Bug Fixes

- Fixed config path reporting and folder opening for Codex
- Corrected default import behavior when main config is missing
- Fixed non_snake_case warnings in commands.rs

## [3.0.0] - 2025-08-27

### 🚀 Major Changes

- **Complete migration from Electron to Tauri 2.0** - The application has been completely rewritten using Tauri, resulting in:
  - **90% reduction in bundle size** (from ~150MB to ~15MB)
  - **Significantly improved startup performance**
  - **Native system integration** without Chromium overhead
  - **Enhanced security** with Rust backend

### ✨ New Features

- **Native window controls** with transparent title bar on macOS
- **Improved file system operations** using Rust for better performance
- **Enhanced security model** with explicit permission declarations
- **Better platform detection** using Tauri's native APIs

### 🔧 Technical Improvements

- Migrated from Electron IPC to Tauri command system
- Replaced Node.js file operations with Rust implementations
- Implemented proper CSP (Content Security Policy) for enhanced security
- Added TypeScript strict mode for better type safety
- Integrated Rust cargo fmt and clippy for code quality

### 🐛 Bug Fixes

- Fixed bundle identifier conflict on macOS (changed from .app to .desktop)
- Resolved platform detection issues
- Improved error handling in configuration management

### 📦 Dependencies

- **Tauri**: 2.8.2
- **React**: 18.2.0
- **TypeScript**: 5.3.0
- **Vite**: 5.0.0

### 🔄 Migration Notes

For users upgrading from v2.x (Electron version):

- Configuration files remain compatible - no action required
- The app will automatically migrate your existing provider configurations
- Window position and size preferences have been reset to defaults

#### Backup on v1→v2 Migration (cc-switch internal config)

- When the app detects an old v1 config structure at `~/.cc-switch/config.json`, it now creates a timestamped backup before writing the new v2 structure.
- Backup location: `~/.cc-switch/config.v1.backup.<timestamp>.json`
- This only concerns cc-switch's own metadata file; your actual provider files under `~/.claude/` and `~/.codex/` are untouched.

### 🛠️ Development

- Added `pnpm typecheck` command for TypeScript validation
- Added `pnpm format` and `pnpm format:check` for code formatting
- Rust code now uses cargo fmt for consistent formatting

## [2.0.0] - Previous Electron Release

### Features

- Multi-provider configuration management
- Quick provider switching
- Import/export configurations
- Preset provider templates

---

## [1.0.0] - Initial Release

### Features

- Basic provider management
- Claude Code integration
- Configuration file handling
