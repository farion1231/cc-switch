# API Key refresh script template

This directory is a complete, dependency-free Node.js 20+ refresh script for CC Switch. `index.mjs` imports configuration and helper modules so provider-specific logic stays isolated.

## Quick test (offline)

1. Extract the ZIP and select the extracted `api-key-refresh-template` directory in **Settings → API Key variables → Script directory**.
2. Run `node index.mjs` in that directory. The default `demo` mode prints one JSON object with a clearly fake `demo-not-a-real-key-...` value.
3. In CC Switch, add `MY_KEY`, choose this directory, then use **Test script**. Reference the saved value in a provider field with `{{MY_KEY}}`.

The demo key cannot authenticate with any API. It only verifies the script contract and multi-file imports.

## Supported API Key fields

Use `{{MY_KEY}}` as the complete value of any supported API Key field. CC Switch resolves it for Claude, Claude Desktop, Codex, Gemini, Grok Build, OpenCode, OpenClaw, Hermes, Pi, MiniMax Code, and universal providers. This also includes model-level `apiKey` and `api_key` fields in native configuration editors. It does not interpolate references into `Bearer` strings, arbitrary headers, MCP/tool credentials, usage settings, or OAuth credentials.

## Connect an HTTP provider

1. Edit `config.mjs` or start CC Switch with `API_KEY_TEMPLATE_MODE=http` and `API_KEY_REFRESH_URL=https://your-provider.example/refresh`. The default `example.invalid` URL is intentionally rejected.
2. Adapt the POST JSON body in `config.mjs` and the response mapping comment in `lib/provider.mjs`.
3. Before starting CC Switch, set `API_KEY_REFRESH_TOKEN` in the environment that launches it. Do not put the token in this directory or print it. Restart CC Switch after changing its environment.
4. Run `node index.mjs` manually, then use **Test script** again.

The HTTP example sends `POST` JSON with `Authorization: Bearer $API_KEY_REFRESH_TOKEN`. It expects `{ "apiKey": "...", "expiresAt"?: "...", "refreshAfter"?: "...", "message"?: "..." }`. Map your provider response to this shape in `lib/provider.mjs`.

PowerShell (start CC Switch from this same terminal):

```powershell
$env:API_KEY_TEMPLATE_MODE = "http"
$env:API_KEY_REFRESH_URL = "https://provider.example/v1/refresh"
$env:API_KEY_REFRESH_TOKEN = "replace-with-a-secret"
node index.mjs
# Start or restart CC Switch from this terminal after the manual test succeeds.
```

POSIX shells:

```sh
export API_KEY_TEMPLATE_MODE=http
export API_KEY_REFRESH_URL=https://provider.example/v1/refresh
export API_KEY_REFRESH_TOKEN='replace-with-a-secret'
node index.mjs
```

A successful manual run prints exactly one line, for example:

```json
{
  "apiKey": "provider-key-without-spaces",
  "expiresAt": "2030-01-01T00:00:00.000Z",
  "refreshAfter": 1893454200,
  "message": "refreshed"
}
```

`expiresAt` and `refreshAfter` accept RFC 3339 / ISO-8601 timestamps with a timezone (for example `2030-01-01T00:00:00Z`) or Unix **seconds**. Never use Unix milliseconds. An `expiresAt` value must be in the future.

## Runtime contract

CC Switch executes `index.mjs` from the selected directory with Node 20+ (configure the Node executable under **Script runtime** when discovery fails). On success, stdout must contain only one JSON object no larger than 64 KiB. Put diagnostics on stderr; never log credentials, keys, authorization headers, or raw provider responses.

CC Switch supplies these environment variables:

- `CC_SWITCH_CONTRACT_VERSION=2`
- `CC_SWITCH_VAR_NAME` — variable being refreshed
- `CC_SWITCH_REASON` — `startup`, `scheduled`, `switch`, `manual`, `test`, or `retry`
- `CC_SWITCH_CURRENT_EXPIRES_AT` — optional current expiry as an ISO-8601 UTC timestamp

Exit with `2` for configuration, authentication, or invalid-response problems that require attention. Exit with `1` for network, timeout, HTTP 408/429, or 5xx failures so CC Switch retries with backoff. The supplied files implement these rules.

CC Switch uses a 30-second default script timeout (configurable from 5 to 300 seconds) and a five-minute default refresh lead time. It runs enabled scripts at startup and on its schedule, and may also run them before a switch when the current key is near expiry. Scheduling uses `refreshAfter` first; otherwise it uses `expiresAt` minus the lead time, then falls back to one hour after a successful run. If testing fails, first verify the selected directory contains `index.mjs`, run the same command manually, check the Node path under **Script runtime**, then review stderr without adding secrets to logs.
