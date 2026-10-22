# Keep Codex Remote Control and Official Plugins While Using Third-Party APIs: CC Switch Setup Guide

> Applies to CC Switch v3.20.1 and later; UI text follows v3.20.4. This guide is based on the current code, user manual, and the v3.16.1 / v3.20.1 release notes. Screenshots use de-identified sample data and do not include real Access Tokens or API keys. Some screenshots were taken on older versions; where a switch name differs, the text of this guide takes precedence.

## What this guide solves

Many Codex users want both of these at the same time:

1. Use models from DeepSeek, Kimi, GLM, MiniMax, SiliconFlow, or other third-party APIs, or use GPT models through an aggregator.
2. Keep Codex official-app capabilities such as mobile remote control and official plugins.

Previously, when switching to a third-party provider, the old behavior wrote the third-party API key into Codex `auth.json`, which could overwrite the original official ChatGPT / Codex login cache. The third-party model worked, but features that depend on the official login state disappeared.

The **Codex App Enhancements** switch added in v3.16.1 solves this conflict: the official Access Token stays in `auth.json`, while third-party provider information is written to `config.toml`. Codex App can still see an official account, but actual model requests follow the third-party provider currently selected in CC Switch.

This behavior already existed in v3.16.0 and was enabled by default. After some users reported that they did not want this behavior, v3.16.1 turned it into an explicit switch.

Since v3.20.1, the switching logic has been simplified one step further: third-party API keys are only ever written to `config.toml` and never to `auth.json`. This switch now decides just one thing: when you switch directly to a third-party provider, whether the official login in `auth.json` is kept or deleted.

## Quick answer

Recommended order:

1. In the CC Switch Codex panel, switch to `OpenAI Official`.
2. Start Codex and log in once with an official ChatGPT / Codex account. A Free subscription is enough.
3. Return to CC Switch and enable `Settings -> General -> Codex App Enhancements -> Keep official login for direct switches`.
4. Add or switch to a third-party Codex provider.
5. If the provider uses the Chat Completions protocol (its card shows the `Needs Routing` badge, for example SiliconFlow or ModelScope), also enable local routing and route Codex through it. Presets such as DeepSeek, Kimi, MiniMax, and Zhipu GLM already connect directly, so they do not need this step.
6. Restart Codex so `config.toml` and the model catalog are reloaded.

![Codex App Enhancements switch in Settings](../images/codex-official-auth-preservation/01-codex-app-enhancement-setting.png)

## Prerequisites

Prepare the following:

- CC Switch v3.20.1 or later.
- Codex installed and able to start. Installing both the app and CLI is recommended.
- An official ChatGPT / Codex account that can log in to Codex. A Free subscription is enough.
- A third-party API key, such as DeepSeek, Kimi, GLM, MiniMax, OpenRouter, SiliconFlow, or similar.

Do not manually copy or share the contents of `~/.codex/auth.json`. It stores official login cache and Access Tokens, so it is sensitive.

## Step 1: Switch back to OpenAI Official and complete official login

Open CC Switch and switch to the top-level `Codex` tab. First select the `OpenAI Official` provider, or add it from the preset providers if it is missing, and make it the current provider.

![OpenAI Official and third-party providers in the Codex provider list](../images/codex-deepseek-routing/01-codex-providers-require-routing.png)

Then start Codex, preferably the CLI, and follow the official login flow to sign in with your ChatGPT / Codex account. This account can be on the Free plan. In this setup, it mainly preserves the official identity required by Codex App, and does not pay for third-party model usage.

After login, Codex stores the official login cache in `~/.codex/auth.json`. The key point for the following steps is: do not let third-party provider switching overwrite this file again.

## Step 2: Enable Codex App Enhancements

Return to CC Switch and open:

```text
Settings -> General -> Codex App Enhancements
```

Enable:

```text
Keep official login for direct switches
```

This switch is off by default because some users do not want this behavior. Enable it only when you explicitly want "third-party API + official remote control / official plugins" at the same time.

After it is enabled, when you switch to a third-party provider without routing takeover:

- `auth.json`: keeps the official ChatGPT / Codex login cache.
- `config.toml`: stores the active third-party provider's model, endpoint, `model_provider`, and provider-scoped `experimental_bearer_token`.

The `config.toml` half has nothing to do with the switch: it is written the same way when the switch is off. The switch only controls whether `auth.json` is kept or removed. While routing takeover is enabled, the official login is always preserved, regardless of this switch.

## Step 3: Add a third-party Codex provider

Return to the Codex panel and click the plus button in the upper-right corner to add a provider. Prefer built-in presets such as DeepSeek, Kimi, MiniMax, GLM, or SiliconFlow. After selecting a preset, you only need to enter the API key; the preset automatically configures the base URL, default model, model catalog, and upstream format.

After saving, check the badge on the provider card:

- **No badge**: the upstream natively supports the OpenAI Responses API and can connect directly, without local routing. The DeepSeek, Kimi, MiniMax, Zhipu GLM, and 千问AI平台 presets, as well as aggregators that offer GPT models, all fall into this group.
- **Has the `Needs Routing` badge**: the upstream format is `Chat Completions (routing required)`, for example SiliconFlow or ModelScope. Local routing must be enabled for these providers so CC Switch can convert Codex Responses requests into Chat Completions requests.

For a custom provider, choose the upstream format under `Advanced Options -> Upstream Format` at the bottom of the form.

## Step 4: Enable local routing and route Codex when needed

Open:

```text
Settings -> Routing -> Local Routing
```

Complete two actions:

1. Turn on the `Routing Master Switch` to start the local service. The default address is usually `127.0.0.1:15721`.
2. Under `Routing Enabled`, turn on `Codex`.

![Enabling Codex takeover on the local routing page](../images/codex-deepseek-routing/03-local-route-codex-takeover.png)

After takeover, Codex's live `config.toml` temporarily points to the CC Switch local route, and the token field holds the placeholder `PROXY_MANAGED`. The real third-party API key remains in the CC Switch provider configuration and is injected by the local route when it forwards requests. During takeover, the official login in `auth.json` is always preserved.

## Step 5: Switch to the third-party provider and restart Codex

Return to the Codex provider list and enable the third-party provider you just added. After switching, restarting Codex is recommended for two reasons:

- Codex reads `config.toml` at startup.
- The Codex `/model` menu usually needs a restart before it reloads `model_catalog_json`.

After restart, you can run a quick verification:

- In Codex App, the account information still shows the official account. This is expected.
- In CC Switch, the current Codex provider is the third-party provider.
- If local routing is enabled, request logs or routing stats show Codex requests going through the local route.
- The third-party provider dashboard or balance records show actual model requests.

## How it works

Codex mainly uses two configuration files:

```text
~/.codex/auth.json
~/.codex/config.toml
```

They have different responsibilities:

- `auth.json` stores the official ChatGPT / Codex login cache, which Codex App needs to identify the official account and enable remote control and official plugins.
- `config.toml` stores runtime configuration such as the current model provider, base URL, model, model catalog, and provider-scoped token.

When you switch directly to a third-party provider, CC Switch takes the third-party provider API key from the provider configuration and writes it under the current provider in `config.toml` (this step does not depend on the switch):

```toml
model_provider = "custom"

[model_providers.custom]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
experimental_bearer_token = "sk-..."
```

With `Keep official login for direct switches` enabled, `auth.json` keeps the official login cache unchanged. Codex App can still identify the official account, while model requests follow the current provider and base URL in `config.toml`.

If the provider uses the Chat Completions protocol, CC Switch local routing adds another conversion layer:

```text
Codex Responses request
        |
CC Switch local route
        |
Third-party Chat Completions API
        |
Converted back to Codex Responses response
```

This is why you can keep using official plugins / mobile remote control while moving model traffic to a third-party API.

## Side effects to understand

### Codex still shows the official account

This is the easiest part to misunderstand. After this capability is enabled, Codex App reads the official login state from `auth.json`, so it continues to display the official account.

That does not mean model requests are still going to official OpenAI. Actual traffic is determined by the current Codex provider in CC Switch, `config.toml`, and local routing logs.

### Do not use the Codex account display to judge billing

If you switch to DeepSeek, Codex can still display the official account, while model requests go to the DeepSeek API. Billing, quota, error codes, and data policy should all be understood according to the third-party provider. You can inspect specific request details in the usage panel.

### Restart Codex after changing model mappings

Codex reads the model catalog at startup. Even if CC Switch has generated a new model catalog, a running Codex process may not hot-load it, so restart Codex after editing model mappings.

### With the switch off, direct switches delete the official login

If `Keep official login for direct switches` is turned off, switching to a third-party provider without routing takeover makes CC Switch delete `auth.json`. Codex App then loses its official login state, so official remote control and official plugins stop working. The third-party API key is still written only to `config.toml`, never to `auth.json`.

Routing takeover is not affected by this switch: while routing takeover is enabled, the official login is always preserved. If your goal is to keep official remote control and official plugins long term, keep this switch enabled.

## FAQ

**I switched to a third-party API. Why does Codex still show the official account?**

This is expected. Official account information comes from `auth.json`; the actual model provider comes from `config.toml` and the current provider in CC Switch.

**Is a Free subscription really enough?**

Yes. The official account is mainly used to obtain and preserve the official login state required by Codex App. Third-party model requests use the third-party API key configured in CC Switch.

**What should I do if official plugins or mobile remote control still do not work?**

Switch back to `OpenAI Official`, restart Codex, and complete official login once. Then confirm `Settings -> General -> Codex App Enhancements -> Keep official login for direct switches` is enabled in CC Switch before switching back to the third-party provider.

**What if third-party requests return 404, the model list is wrong, or streaming responses are broken?**

If the provider uses Chat Completions, confirm that `Upstream Format` under `Advanced Options` in the provider form is set to `Chat Completions (routing required)`, and that `Settings -> Routing` has both the `Routing Master Switch` and Codex takeover turned on.

**Can I switch back to OpenAI Official while local routing is enabled?**

Not recommended. CC Switch tries to prevent switching to official providers while local routing takeover is active, because accessing official APIs through a proxy may create account risk. Use official login only to preserve `auth.json`, and route model traffic to third-party providers.

**Why is this flow so complex? Can it be simplified?**

Because Codex App Enhancements and routing takeover can create unnecessary trouble for users who do not need them, these features are explicit switches instead of always-on behavior.

## References

- [Can't see custom models in the Codex desktop app? (FAQ)](./codex-desktop-custom-model-visibility-en.md)
- [Using Chat-Format APIs in Codex: Local Routing Guide](./codex-deepseek-routing-guide-en.md)
- [Add a Codex provider: Chat Completions routing and model mapping](../user-manual/en/2-providers/2.1-add.md)
- [Local Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [Local Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [CC Switch v3.16.1 Release Note](../release-notes/v3.16.1-en.md)
- [CC Switch v3.20.1 Release Note](../release-notes/v3.20.1-en.md)
