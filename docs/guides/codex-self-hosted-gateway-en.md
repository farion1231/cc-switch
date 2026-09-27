# Using Codex with a self-hosted LLM gateway (llm-gateway-go): keep official login + multi-model side-by-side

> Version: CC Switch v3.20.4+. This guide is based on end-to-end verification logs from 2026-09-28 (CODEX_HOME = `~/.codex`, real instance). Screenshots TBD.

## What this solves

Many teams deploy their own OpenAI-compatible gateways (built on [llm-gateway-go](https://github.com/kaixuan/llm-gateway-go) and similar) that fan-out to multiple upstream models (Anthropic, GLM, Kimi, MiniMax, DeepSeek, …) under a single base_url. They want Codex to talk to this gateway **without** losing the ChatGPT official login state — the Codex desktop app, remote control, and bundled plugins all depend on it.

CC Switch v3.20.1+ already keeps third-party API keys in `config.toml` only (never `auth.json`). This guide focuses on the gateway case:

1. Expose multiple non-OpenAI models in a single Codex provider, all selectable in the TUI model picker.
2. Switch back to OpenAI Official at any time, with no side effects on the gateway state.
3. Don't accidentally strip the common config (approval_policy, sandbox_mode, agents, …) when switching providers.

## TL;DR

1. Upgrade to CC Switch v3.20.4+ (older versions don't properly round-trip `[model_providers.custom]`).
2. In the Codex tab, pick the "开轩 LLM 网关" or "本地 LLM 网关 (8782)" preset. (If missing, add it via `Settings → Custom Presets` or open a PR against `src/config/codexProviderPresets.ts`.)
3. Enter the gateway API key. CC Switch writes the key into `config.toml`, **not** `auth.json`, so your ChatGPT OAuth cache is preserved.
4. Launch Codex once so it ingests `~/.codex/cc-switch-model-catalog.json` and the models appear in the picker.
5. The 4 providers (`default` / `OpenAI Official` / kxpms-gateway / local-gateway-8782) coexist as independent rows in CC Switch — switching is a UI click, never destructive.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│ CC Switch maintains 4 independent codex providers:        │
│  • default          → ChatGPT OAuth (official)            │
│  • OpenAI Official  → API-key placeholder                 │
│  • 开轩 LLM 网关     → https://llm.kxpms.cn/v1  (multi-model) │
│  • 本地 LLM 网关(8782)→ http://localhost:8782/v1 (multi-model) │
│                                                          │
│ UI switch: DB is_current flip → live config rewrite      │
│      model_provider / model / model_catalog_json / auth  │
└──────────────────────────────────────────────────────────┘
```

## End-to-end verification (2026-09-28)

Pass criterion: `codex exec --json` event stream must contain `item.completed(type=agent_message)` with text exactly equal to `TOKEN-<model>`, plus a `turn.completed`.

| Gateway | Model | Result |
|---|---|---|
| kxpms | claude-opus-5 | **PASS** |
| kxpms | claude-opus-4-8 | **PASS** |
| 8782 | claude-opus-5 | **PASS** |
| 8782 | claude-sonnet-5 | **PASS** |

**Known limits (gateway-side, not config)**:

- `glm-5.2` / `kimi-k3` got rate-limited in the capture window (8782: 264× `rate_limit_exceeded` in 6 min; kxpms: 140s tail latency with no response).
- `claude-sonnet-5` / `claude-opus-5-5` hang on kxpms (curl 100s, zero bytes) — upstream credentials issue.
- `deepseek-v4-pro` / `grok-4.7` → 503 provider_unavailable; `qwen*.max` / `gemini-3.x` → 503 no_candidate.

`[profiles.kx-*]` syntax is **rejected by codex 0.158** — it has moved to `~/.codex/<name>.config.toml`. If you have a legacy `[profiles.xx]` block in config.toml, `codex --profile xx` will hard-fail:

```
Error loading config.toml: --profile `xx` cannot be used while
config.toml contains legacy `profile = "xx"` or `[profiles.xx]`
```

This contract is pinned by `codexProviderPresets.gateways.test.ts`.

## Prerequisites

1. CC Switch v3.20.4+.
2. Codex CLI 0.158+ (`codex --version`).
3. Your gateway URL + API Key. **Don't paste real keys into tickets.**
4. The gateway exposes OpenAI Responses API (`/v1/responses`), not just Chat Completions.

## Step 1: log into ChatGPT first

Switch to `OpenAI Official` (or `default`, whichever carries your OAuth), launch Codex once, complete the official login (Free tier is fine). The OAuth lives in `auth.json`; switching to a gateway later won't touch it.

## Step 2: add the gateway provider

In the Codex tab, pick "开轩 LLM 网关" or "本地 LLM 网关 (8782)" → enter API Key → Save. This writes the key into `config.toml` only.

## Step 3: prime Codex with the model catalog

`codex exec --skip-git-repo-check -m claude-opus-5 "ping"` once, so Codex parses and caches `~/.codex/cc-switch-model-catalog.json`. The `slug` values become your `--model` candidates.

## Step 4: switch back to OpenAI to verify nothing was lost

Click `OpenAI Official` in CC Switch → `codex exec -m gpt-5.6-sol "ping"` should reach `chatgpt.com/backend-api` normally (verified: all 4 official models returned 401/usage-limit responses within 12s, no config damage). Click back to your gateway provider when done.

## Troubleshooting

| Symptom | Check |
|---|---|
| ChatGPT login lost after switching | Your cc-switch is < v3.20.1 — third-party keys used to overwrite `auth.json`; upgrade |
| `[profiles.xx]` legacy warning | Move `[profiles.xx]` into `~/.codex/xx.config.toml`; see codex 0.158 release notes |
| Gateway: `--model glm-5.2` returns 503 | Upstream credential exhausted or rate-limited; verify with `curl -m 10 -X POST <base>/v1/responses` |
| `Reconnecting 1/5...5/5` on every gateway call | Don't put `request_max_retries=2` under `[model_providers.custom]` — codex 0.158 ignores it (verified via bogus-host probe) |

## Engineering contract (for preset authors)

When adding a self-hosted gateway preset to `codexProviderPresets.ts`:

- `apiFormat: "openai_responses"` (direct — don't route through cc-switch's Responses→Chat transform).
- `modelCatalog` must list **every** model explicitly with `contextWindow` / `inputModalities` / `reasoningLevels` (don't let the backend guess).
- Default `model = "<first row of catalog>"`.
- No `[profiles.kx-*]` (rejected by 0.158).
- No `request_max_retries` / `stream_max_retries` / `stream_idle_timeout_ms` (0.158 ignores them at `[model_providers.custom]` — confirmed by probe; it's a fake feature).

Regression coverage: `src/config/codexProviderPresets.gateways.test.ts` (static assertions) + `src-tauri/src/codex_config.rs::tests::self_hosted_gateway_presets_round_trip_through_catalog_pipeline` (real catalog write + reverse-parse round-trip).