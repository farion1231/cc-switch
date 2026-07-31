# Can't See Custom Models in the Codex Desktop App? (FAQ)

> Applies to CC Switch v3.16.1 and later. This article explains "why the Codex desktop app can't see custom models" and the available mitigation; for the detailed step-by-step setup with screenshots, see [Keep Codex Remote Control and Official Plugins While Using Third-Party APIs](./codex-official-auth-preservation-guide-en.md).

## Symptom

After you switch Codex to a third-party / custom model in CC Switch (DeepSeek, Kimi, GLM, MiniMax, an aggregator, etc.):

- The model picker in the **Codex desktop app** doesn't show these custom models — often only the official default model remains, and the reasoning level falls back to the official default;
- but everything works fine in the **command-line `codex`** `/model` menu.

Many users have run into this. Here's why, and what you can do about it.

## Why this happens

This is **not a CC Switch local-config problem and not a CC Switch bug** — it is the **Codex desktop app's (the upstream closed-source client's) own model-gating behavior**.

The Codex desktop app's model picker decides which models to allow based on your **current login identity**: when it can't detect an official ChatGPT / Codex login state, it forces the picker back to the official default model and hides the custom models you configured through `config.toml` (the reasoning level falls back to the official default too). The upstream has marked "exposing custom-provider models in the desktop GUI" as not planned, so CC Switch cannot fully fix this at the desktop-GUI level.

The command-line `codex` `/model` menu and request routing both recognize the custom providers in `config.toml` correctly — **only the desktop GUI picker is constrained by this gating layer**.

## Mitigation: keep the official login

The workaround is to **keep the official login state** so the desktop app's gating allows your custom models through. The key points are below (the full step-by-step setup with screenshots is in the linked guide):

1. Log in once with an official ChatGPT / Codex account in Codex (a Free subscription is enough) to keep the official login state.
2. In CC Switch, enable `Settings -> General -> Codex App Enhancements -> Keep official login when switching third-party providers` (**off by default**).
3. Enable local routing and route Codex through it for this third-party provider (required for Chat Completions providers such as DeepSeek / Kimi / MiniMax).
4. Fully quit and restart Codex.

Once enabled, CC Switch preserves the official login state in `~/.codex/auth.json` when switching to a third-party provider and writes the third-party key into `config.toml`, so the desktop app still recognizes the official login identity, the gating lets your models through, and the custom models you configured reappear in the picker. **The preserved official token is never sent to the third party** — third-party model requests still use the key you configured, forwarded through the local route.

> 📖 Detailed step-by-step setup: [Keep Codex Remote Control and Official Plugins While Using Third-Party APIs](./codex-official-auth-preservation-guide-en.md)

## Still can't see them?

- **Confirm the toggle is on**: this toggle is off by default, and many people overwrite the official login state the first time they switch to a third-party provider, which is exactly why the models disappear — enable it as above.
- **The official login state expires**: if you haven't used the official login for several days, the picker may go empty again once the token expires — log in to the official account once more to restore it.
- **Command-line fallback diagnosis**: run `codex debug models` to list the models actually available on the CLI side and confirm the model itself is configured correctly (the CLI is unaffected by this gating).
- Individual Codex desktop versions may behave slightly differently; this is in the upstream client's domain, and no CC Switch version can fully fix it at the desktop-GUI level.

## References

- [Keep Codex Remote Control and Official Plugins While Using Third-Party APIs](./codex-official-auth-preservation-guide-en.md)
- [Codex DeepSeek local routing hands-on guide](./codex-deepseek-routing-guide-en.md)
- [Local Routing](../user-manual/en/4-proxy/4.2-routing.md)
