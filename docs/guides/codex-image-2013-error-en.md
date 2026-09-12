# Misleading "context window exceeds limit (2013)" when attaching images > 10 MB to Codex

> Applies to CC Switch v3.16.4 and earlier. Filed as upstream issue #4820.

## What this guide solves

Codex CLI users who route through CC Switch to MiniMax-family providers
(`MiniMax-M3`, `MiniMax-M2.7`, etc.) sometimes see this error when they
attach an image to a Codex conversation:

```
CC Switch local proxy failed while handling Codex endpoint /responses.
Provider: MiniMax; model: MiniMax-M3;
upstream_status: HTTP 400;
cause: invalid params, context window exceeds limit (2013)
```

The error message says **"context window"**, which sends users down the
wrong debugging path (clearing conversation history, switching to a
model with a bigger context window). The real cause is different.

## Root cause

MiniMax API returns error code **2013** for multiple distinct failure
modes. The upstream payload looks like one of:

```json
// Real context window overflow (200K tokens exceeded)
{"error":{"message":"prompt is too long (2013)",
          "code":"invalid_prompt"}}

// Single image attachment > 10 MB
{"error":{"message":"media exceeds size limit: max 10485760 bytes (2013)",
          "code":"invalid_prompt"}}
```

CC Switch's upstream-error wrapper currently translates **every** `2013`
into the human-readable string `"context window exceeds limit"`, with no
distinction between the two cases. As a result, the user sees the same
message regardless of whether their problem is context length or media
size.

Verified by direct `curl` against `https://api.minimaxi.com/v1/responses`:

```bash
$ curl .../v1/responses -d '{ "model": "MiniMax-M3",
                              "input": [...large base64 image > 10 MB...] }'
{"error":{"message":"media exceeds size limit: max 10485760 bytes (2013)",
          "code":"invalid_prompt"}}
```

`MiniMax-M3` actually supports 200K+ tokens of context. A request that
is small in tokens but big in image bytes will fail with `2013` /
`media exceeds size limit`, not because of context.

## How to identify which case you're in

| Symptom | Likely cause |
|---|---|
| Long conversation history + image | Real context overflow (rare) |
| Single image > 10 MB (Retina screenshot of a detail-rich page, PDF page) | Media size limit (common) |
| Conversation has < 1K tokens + image fails | Almost certainly media size |

The fastest way to confirm: `curl` the same payload directly against the
upstream API (`api.minimaxi.com`) and read the `error.message` field
verbatim.

## Recommended upstream fix (proposed in issue #4820)

1. When CC Switch receives a `2013` from a MiniMax-family provider,
   forward the upstream `error.message` string to the Codex UI instead
   of substituting a generic one. The message already says "media" vs
   "prompt" — no translation is needed.
2. Optionally, classify the upstream error in the same place and pick
   from a small lookup table (`2013` + `"media"` → `"Image exceeds 10 MB
   limit"`, `2013` + `"prompt"` → `"Conversation too long"`).

This is a small change in the forwarder's error-mapping layer; no
provider-side changes are needed.

## Workaround for end users (today)

Until the upstream fix ships, the
[Konsn666/codex-img-prep](https://github.com/Konsn666/codex-img-prep)
companion tool pre-processes images client-side so they always fit
under MiniMax's 10 MB media cap. One-line install:

```bash
curl -fsSL https://raw.githubusercontent.com/Konsn666/codex-img-prep/main/install-remote.sh | bash
```

After install:

- Screenshots copied to the clipboard are auto-resized within 2 seconds
  (max 1920 px, JPEG q85, ≤ 8 MB).
- Finder right-click any image → **Quick Actions → Compress for Codex**.
- Shell aliases: `prep-clip`, `prep-img`, `prep-watch`.

Additionally, switching the Codex provider's `meta.apiFormat` in
`cc-switch.db` from `openai_chat` to `openai_responses` (also done by
the installer) avoids a lossy `/responses → /chat/completions`
translation in the forwarder and makes logs match the actual request
path.

## Environment

- CC Switch: v3.16.4
- Codex CLI: latest, `wire_api = "responses"`
- MiniMax-M3 via `api.minimaxi.com`
- macOS 15.4 (Apple Silicon) — the workaround is macOS-only because it
  uses the built-in `sips` tool; Linux/Windows users would need to
  swap `sips` for `ImageMagick` or similar.

## Related

- Upstream issue: https://github.com/farion1231/cc-switch/issues/4820
- Workaround repo: https://github.com/Konsn666/codex-img-prep
- End-to-end verification: 38.64 MB screenshot → 1.04 MB after the
  pipeline → CC Switch → MiniMax → 200 OK with correct vision answer.
---

## Chinese (简体中文) version

A fully translated Chinese version of this guide is available at
[Konsn666/codex-img-prep/blob/main/README.zh-CN.md](https://github.com/Konsn666/codex-img-prep/blob/main/README.zh-CN.md)
along with the upstream bug report in Chinese at
[ISSUE.zh-CN.md](https://github.com/Konsn666/codex-img-prep/blob/main/ISSUE.zh-CN.md).
