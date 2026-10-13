# Add an "Import to CC Switch" Button to Your Site: Integration Guide for Site Owners

[中文](deeplink-site-integration-guide-zh.md) | English | [日本語](deeplink-site-integration-guide-ja.md)

If you run a site, CC Switch lets your users import your provider into Claude Code, Codex and other agent apps with one click.

> Applies to CC Switch v4.0.10 and later. Behavior marked "since v4.0.10" works differently on earlier versions; the differences are listed in "Compatibility with older versions" at the end. This guide is based on the current code. All domains and keys in the examples are fake.

## Who this is for

You run an API service (a relay, an aggregator or a self-hosted gateway all count) and want your users to click one button in your dashboard to add your endpoint, key, models and balance query to CC Switch, without filling in a form by hand.

This is done with a `ccswitch://` deep link. Once CC Switch is installed, clicking a `ccswitch://...` link in the browser launches CC Switch and opens an import confirmation dialog. The user checks it and clicks "Import".

The [Deep Link Protocol](../user-manual/en/5-faq/5.3-deeplink.md) page in the user manual is the parameter reference. This guide covers the decisions a site owner actually has to make: how to generate the link, what it can carry, what gets rejected, and how to show users their balance right after import.

## Quickest path

> Is your site built on New API or Sub2API? Both already have an "Import to CC Switch" button in their key list, so it works out of the box with no development needed. The difference is the balance: New API links carry no usage query script, so users don't see a balance on the card after import; Sub2API links do. Follow this guide if you want to add the balance, include your icon, or support more apps.

Most site owners only need to do three things. Reading the sections below in order is enough:

1. Read "Key points", then "The minimal link", "Supported apps" and "Common parameters" under "Provider links", and decide which apps to generate links for and how to write the endpoint and format. You can fill in the form in the [online generator](https://ccswitch.io/deplink.html) to produce a link, click it on your own computer, and see what the confirmation dialog looks like.
2. If you want users to see their balance after import, read "Show the balance right after import: usage query scripts". NewAPI / OneAPI and Sub2API style sites have ready-made scripts you can copy as is.
3. Read "Code for generating links" and "Putting the link behind a button": the backend generates the link per user, and the frontend shows the button.

The collapsed sections (embedded config, the summary of rejected links, MCP / prompts / Skills) can be expanded when you need them.

## Key points

1. **Generate links per user**. An API key is required when importing a provider, and a link without a key fails to import. So the "one-click import" button belongs in the user's dashboard after login, and your backend builds the link with that user's own key. Don't put it on a public page. See "Putting the link behind a button" for how to build the button.
2. **Declare your API format with `apiFormat`**. Values are `anthropic`, `openai_chat`, `openai_responses` and `gemini_native`; which ones each app supports is listed below. Without it, the app's default format is used: Anthropic Messages for Claude Code, OpenAI Responses for Codex and Grok Build, and OpenAI Chat Completions for OpenCode / OpenClaw / Hermes / Pi.
3. **Set model names only when needed**. If your relay serves genuine Claude models, leave all Claude Code models empty and Claude Code uses its built-in model list. Only non-native models such as DeepSeek or Kimi need `model`, `haikuModel`, `sonnetModel` and `opusModel`. OpenCode, OpenClaw and Hermes take their model list from `model`, so without it there are no models to choose from. Pi requires it.
4. **Include a usage query script if you want users to see their balance**. Since v4.0.10, a link with `usageScript` enables it by default. To stay compatible with older versions, also set `usageEnabled=true`. The script can only request HTTPS endpoints with the same origin as the provider endpoint; details below.
5. **The user decides whether to "switch after import"**. The confirmation dialog has a switch for it, and `enabled` in the link only sets its initial value. You usually don't need to set it.
6. **URL-encode Base64 parameters again**. Build the link with `URLSearchParams` or an equivalent function, not by concatenating strings by hand.

## Link structure

```
ccswitch://v1/import?resource=<type>&<params>...
```

- `v1` is the protocol version and `/import` is a fixed path. Getting either wrong gets the link rejected.
- `resource` is one of `provider`, `mcp`, `prompt` and `skill`.
- Parameter names are case-sensitive and use camelCase: `apiKey`, `usageScript`, not `api_key`.

### Encoding rules

There are two layers of encoding, and the order matters:

1. Parameters that carry a whole block of content, such as `config`, `usageScript` and `content`, are first encoded as **Base64** from their UTF-8 text. Both standard and URL-safe Base64 are accepted, and the trailing `=` is optional.
2. When building the link, **every parameter value** is then URL-encoded (`application/x-www-form-urlencoded` rules). The `+` in standard Base64 must become `%2B`, otherwise it is decoded as a space. CC Switch tries to turn spaces back into `+`, but don't rely on that.

`URLSearchParams` does step 2 for you. Forgetting this step is the most common mistake in hand-written links.

Boolean parameters take `true` / `false`. Any other value is treated as if the parameter were missing.

## What happens when the user clicks the link

1. The browser asks whether to open CC Switch. Once the user agrees, CC Switch comes up (it starts first if it isn't running).
2. CC Switch opens the import confirmation dialog, showing everything in the link: the endpoint, the masked key, the models, the embedded config, and the full usage query script. Suspicious content is highlighted in yellow, for example endpoints pointing at localhost or a private network, environment variables that change how processes load code (`NODE_OPTIONS`, `LD_PRELOAD` and the like), and MCP servers that run a whole command line through a shell.
3. After the user clicks "Import":
   - **Provider**: added to the provider list of the target app. The bottom of the dialog has a switch whose initial value comes from `enabled` in the link, and the user can change it:
     - Claude Code, Codex, Gemini CLI, Grok Build: the switch is "Switch to this provider after import", off by default. When it is on, the dialog warns that "client requests will go to the endpoint above".
     - OpenCode, OpenClaw, Hermes, Pi: these apps can hold several providers at once. The switch is "Add to the <app> config after import" (for Pi, "Enable in Pi after import"), on by default. When it is off, the provider is only saved in CC Switch's list.
   - **MCP**: written immediately into the config file of every app listed in `apps`.
   - **Prompt**: added to the prompt list. The dialog has an "Enable after import" switch whose initial value comes from `enabled` in the link. When it is on, it replaces the app's current prompt file, and the previously enabled prompt is disabled.
   - **Skill**: what gets added is a Skill repository. The user still has to go to the "Skills" page and choose which Skills to install.

A few details site owners tend to trip over:

- The link can only be triggered by a user click. Most browsers block an automatic redirect to `ccswitch://` on page load.
- CC Switch handles one link at a time. If the user clicks a new link before finishing the previous one, only the newest one is kept. Don't design "one button that fires several links in a row". Give providers, MCP and prompts a button each and let the user click them one by one.
- Clicking the same provider link twice imports two copies. The provider ID is generated from the name plus a timestamp, with no deduplication.
- A web page cannot reliably tell whether the user has CC Switch installed. Put a hint and a download link next to the button; see "Putting the link behind a button" for the wording.

## Provider links

### The minimal link

```
ccswitch://v1/import?resource=provider&app=claude&name=Example%20AI&endpoint=https%3A%2F%2Fapi.example.com&apiKey=sk-xxxx
```

The only required items are `app`, `name`, the endpoint and the key (the last two can be URL parameters or go in the embedded `config`). Pi additionally requires `model`.

### Supported apps

| `app` | App | Default format | Allowed `apiFormat` | What `config` can carry |
|-------|------|---------|--------------------|-------------------|
| `claude` | Claude Code | `anthropic` | All four | `{"env": {...}}`, all variables in `env` are kept |
| `codex` | Codex | `openai_responses` | `openai_responses`, `openai_chat`, `anthropic` | All or part of a `config.toml`, see below |
| `gemini` | Gemini CLI | `gemini_native` | Only `gemini_native` | The three keys `GEMINI_API_KEY`, `GOOGLE_GEMINI_BASE_URL` and `GEMINI_MODEL`; anything else is ignored |
| `grokbuild` | Grok Build | `openai_responses` | `openai_responses`, `openai_chat`, `anthropic` | One `[model."<name>"]` table, see below |
| `opencode` | OpenCode | `openai_chat` | All four | Only the endpoint and key |
| `openclaw` | OpenClaw | `openai_chat` | All four | Only the endpoint and key |
| `hermes` | Hermes | `openai_chat` | `openai_chat`, `openai_responses`, `anthropic` | Only the endpoint and key |
| `pi` | Pi | `openai_chat` | All four | Only the endpoint and key |

Deep link import of providers is not yet supported for Claude Desktop or MiniMax Code; such links are rejected.

`apiFormat` lands in different places depending on the app:

- **Claude Code, Codex, Grok Build**: these three clients only speak their own native format (Anthropic for Claude Code, Responses for the other two). With a non-native format, the imported card carries a "Needs routing" tag, and when the user switches to it, local routing has to be on so CC Switch can convert the request format.
- **OpenCode, OpenClaw, Hermes, Pi**: these clients support several formats themselves. CC Switch maps `apiFormat` to the protocol field in their config (`npm` for OpenCode, `api` for OpenClaw / Pi, `api_mode` for Hermes), and the client calls your API in that format directly, without local routing.

Write the endpoint following the convention of the chosen format: for the Anthropic format in Claude Code, leave out `/v1` (e.g. `https://api.example.com`); the two OpenAI formats usually include `/v1`. For OpenCode, OpenClaw, Hermes and Pi, the endpoint is the base URL of each one's SDK, and they don't build paths the same way: for the same Anthropic format, the OpenCode endpoint in CC Switch's built-in presets includes `/v1`, while Hermes and Pi don't. Generate a separate link for each app, and use the endpoint form you have actually tested to work in that app.

### Common parameters

| Parameter | Description |
|------|------|
| `name` | Display name, required |
| `endpoint` | API endpoint. Several can be separated by commas: the first is the primary endpoint, and the rest are added to the provider's endpoint list ("API Endpoint Management") for the user to speed-test and switch between |
| `apiKey` | API key. When it is both a URL parameter and in `config`, the URL parameter wins |
| `apiFormat` | API format, see the table above. Without it, the app's default format is used |
| `model` | Default model. Leave it empty for Claude Code with genuine Claude models; Codex uses `gpt-5-codex` and Grok Build uses `grok-4.5` when it is missing; required for Pi |
| `haikuModel` / `sonnetModel` / `opusModel` | Claude Code only: the real model names behind the three model aliases, needed only for non-native models |
| `homepage` | Website. Inferred from the endpoint when missing (with the `api.` prefix removed) |
| `icon` | Name of a CC Switch built-in icon. Only lowercase letters, digits and `. _ -` are allowed; anything else is dropped. An unknown name shows the first letter of the provider name. Leave it out if there is no built-in icon for your brand |
| `iconUrl` | URL of your own icon image, see "Adding your own icon" below. Falls back to `icon` if the download fails |
| `notes` | Notes, shown on the provider card |
| `enabled` | Initial value of the switch-after-import / add-after-import switch in the confirmation dialog; the user can change it. You usually don't need to set it |
| `config` + `configFormat` | Embedded config, Base64. `configFormat` is `json` (default) or `toml` |

`configUrl` (remote config) is not implemented yet. Links that carry this parameter are rejected outright.

### Adding your own icon: iconUrl

If CC Switch has no built-in icon for your brand, give an image URL with `iconUrl`. As soon as the confirmation dialog opens, CC Switch downloads the image and shows it at the top. After import it becomes the card icon, with nothing more for the user to click.

The image is redrawn as a 96×96 PNG and stored in CC Switch, and the URL is not fetched again after that. If you change your icon later, users who already imported won't get the new one.

The URL and the image must meet all of the conditions below. If any one fails, the dialog shows no error; the icon falls back to the built-in icon named by `icon`, or to the first letter of the provider name if there is no `icon`:

- It starts with `https://`, the host is a domain name (not an IP), the port is the default one, and there is no username or password. A malformed URL is dropped when the link is parsed, and the rest of the link imports as usual.
- It is publicly accessible: CC Switch downloads it without cookies or Referer, and does not go through the user's system proxy. URLs that require login or have hotlink protection will fail.
- The domain must resolve to a public address. Anything pointing at a private network or localhost is never downloaded.
- The download finishes within 5 seconds and follows at most 3 redirects, and every redirect target must also meet these conditions.
- It is no larger than 256 KB.
- The `Content-Type` response header is one of `image/png`, `image/jpeg`, `image/webp` or `image/svg+xml`, and matches the actual file format. Generic types such as `application/octet-stream` don't work.
- Bitmap width and height are no more than 4096 pixels.

Suggestions for the image itself:

- Use a square image. A square image with opaque corners fills the whole icon frame; a logo with transparent corners is shrunk to the center with padding around it, matching the look of the built-in icons.
- SVG is displayed only as an image: scripts inside it don't run, and external fonts and images it references aren't loaded. Convert text to paths.
- Every user requests this URL once each time they open the confirmation dialog. Serving it from a CDN is fine.

### Claude Code: embedded environment variables

<details>
<summary>Expand if you need extra environment variables (custom request headers, timeouts, etc.)</summary>

Sites that need extra environment variables (custom request headers, timeouts, turning off experimental features, etc.) put them in `config`:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-xxxx",
    "ANTHROPIC_MODEL": "example-sonnet",
    "API_TIMEOUT_MS": "600000",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
  }
}
```

Variables in `env` are kept as they are. For the endpoint, key and model names, the URL parameters win if they are also set. The confirmation dialog lists every variable, with sensitive values masked.

</details>

### Codex: embedded config.toml

<details>
<summary>Only needed for extra settings such as <code>wire_api</code>, request headers or reasoning effort. Expand to see which keys are accepted and which forms are rejected</summary>

Two forms:

- `configFormat=json`, with content `{"auth": {"OPENAI_API_KEY": "sk-xxxx"}, "config": "<config.toml contents>"}`;
- `configFormat=toml`, with the `config.toml` contents directly.

CC Switch does not copy the whole file into the user's `~/.codex/config.toml`. It only takes the parts that belong to this provider:

- **The selected `[model_providers.<id>]` table**: the one `model_provider` points to; without `model_provider`, the file may contain only one such table. These keys are taken from the table: `name`, `base_url`, `wire_api`, `requires_openai_auth`, `experimental_bearer_token`, `model_catalog_url`, `capabilities`, `query_params`, `http_headers`, `request_max_retries`, `stream_max_retries`, `stream_idle_timeout_ms`, `websocket_connect_timeout_ms`, `supports_websockets`, `supports_standalone_web_search`.
- **Top-level model fields**: `model`, `review_model`, `model_reasoning_effort`, `plan_mode_reasoning_effort`, `disable_response_storage`, `web_search`, `model_context_window`, `model_auto_compact_token_limit`, `model_supports_reasoning_summaries`, `model_verbosity`, plus the subagent models in `[agents]` and the memory model in `[memories]`.

Nothing else is taken: `[features]`, `[tools]`, MCP, sandbox, `notify`, other provider tables and so on are the user's own global settings.

After import, the table is renamed to `custom` (all Codex providers managed by CC Switch are written to `[model_providers.custom]`), and `base_url` and `experimental_bearer_token` use the merged endpoint and key. `model_catalog_url` must have the same origin as the provider endpoint, otherwise the import is rejected.

**Forms that are rejected**: a table containing `auth` (makes Codex run a command to get the key), `aws`, `gateway_oauth` (reads cloud credentials on the machine), or `env_http_headers` (reads environment variables). `env_key` is dropped; the key must be given by the link itself. A link that only has `env_key` and no key is rejected.

Example (`configFormat=toml`):

```toml
model_provider = "example"
model = "example-coder"
model_reasoning_effort = "high"

[model_providers.example]
name = "Example AI"
base_url = "https://api.example.com/v1"
wire_api = "responses"
http_headers = { "X-Client" = "cc-switch" }
stream_idle_timeout_ms = 300000
```

</details>

### Grok Build: embedded model table

<details>
<summary>Only needed for extra settings such as the context window or sampling parameters</summary>

Same forms as Codex (`{"config": "<config.toml contents>"}` or `configFormat=toml`).

Only the `[model."<name>"]` table that `[models] default` points to is imported. Without `default`, the file may contain only one table; several tables without `default` are rejected. A Grok Build provider in CC Switch maps to exactly one model table, so importing several models from one link is not supported.

The table keeps the display name, `description`, `api_backend`, `context_window`, `max_completion_tokens`, sampling and retry parameters, reasoning effort fields and so on. The endpoint, key and model name come from the link parameters (URL parameters first). If the table has no `api_backend` or `context_window`, defaults are filled in.

Links whose table contains `auth_provider`, `env_http_headers` or `mtls_cert_dir` are rejected. `env_key` is handled the same way as in Codex.

</details>

### Rejected links: summary

<details>
<summary>Expand when the confirmation dialog shows an error, and look up the cause by the error text</summary>

| Situation | Error in the confirmation dialog (original text, excerpt) |
|------|-------------------------------|
| No key | `API key is required` |
| No endpoint | `Endpoint is required` |
| Endpoint is not http / https | `Invalid URL scheme for 'endpoint[0]'` |
| Unsupported `app` value | `Invalid provider app type` |
| Invalid `apiFormat` value | `Unsupported apiFormat` |
| The app doesn't support that `apiFormat` | `is not supported for app` |
| Pi without a model | `Pi providers need a model` |
| The name, endpoint, key or model for OpenCode / OpenClaw / Hermes / Pi contains syntax the client would expand (see below) | `uses syntax that … would expand` |
| `configUrl` is present | `'configUrl' is not supported yet` |
| Base64 can't be decoded | `config 参数 Base64 解码失败` (Base64 decoding of the config parameter failed) |
| Codex / Grok gives the key only through `env_key` | `supplies its API key indirectly` |
| Codex table contains `auth` or similar | `cannot be imported from a link` |
| Codex catalog URL has a different origin from the provider endpoint | `model_catalog_url` |
| Grok has several tables but no `default` | `several [model.*] tables` |

OpenCode, OpenClaw, Hermes and Pi expand certain syntax when reading their config: in Pi, a key starting with `!` is run as a command, and `$NAME` / `${NAME}` are replaced with environment variables; in OpenCode, `{env:…}` reads an environment variable and `{file:…}` reads a local file; in OpenClaw and Hermes, `${…}` reads an environment variable. All of these are rejected when importing from a link. Your key and endpoint are plain literal values, so they are not affected.

</details>

## Show the balance right after import: usage query scripts

The CC Switch provider card can show a balance or plan usage, and the data comes from a usage query script. Put the script in the link and users see it right after import.

### What a script looks like

A script is a JavaScript expression that returns an object with `request` (what request to send) and `extractor` (how to pull the numbers out of the response):

```javascript
({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  extractor: function (response) {
    return {
      remaining: response.balance,
      unit: "USD"
    };
  }
})
```

Before running it, CC Switch replaces the placeholders in the script with actual values:

| Placeholder | Replaced with |
|--------|--------|
| `{{apiKey}}` | `usageApiKey`; the provider's API key if not given |
| `{{baseUrl}}` | `usageBaseUrl`; the provider's primary endpoint (trailing `/` removed) if not given |
| `{{accessToken}}` | `usageAccessToken` |
| `{{userId}}` | `usageUserId` |

Note that `{{baseUrl}}` is the provider endpoint as is. If it includes `/v1`, the result is `https://api.example.com/v1/user/balance`. If your balance endpoint is not under `/v1`, just write the full URL in the script, as long as it has the same origin as the endpoint.

`extractor` receives the parsed response JSON and returns an object, or an array of objects (for several plans). All fields are optional:

| Field | Type | Description |
|------|------|------|
| `remaining` | number | Remaining quota |
| `used` | number | Used |
| `total` | number | Total quota |
| `unit` | string | Unit, such as `USD`, `CNY`, or a count unit like "times" |
| `planName` | string | Plan name |
| `isValid` | boolean | Whether the key is valid; when `false`, the card shows it as invalid |
| `invalidMessage` | string | Reason it is invalid |
| `extra` | string | Additional note |

### Runtime limits

The script is untrusted input, and CC Switch puts hard limits on it:

- It runs in a QuickJS sandbox with no `fetch`, files, network or any other capability. The script only "describes the request" and "parses the response"; CC Switch sends the request on its behalf.
- **Each query sends only one request**, so you can't log in first and then query.
- **The request must be HTTPS and have the same origin as `{{baseUrl}}`** (same domain, same port). Requests to a different origin are rejected. If your balance endpoint is on another domain (for example the dashboard is at `console.example.com` and the API at `api.example.com`), point `usageBaseUrl` at the dashboard domain.
- The response must be JSON.
- CC Switch sends the request directly from the user's computer, not through a browser, so your balance endpoint doesn't need CORS. The endpoint must accept API key authentication. If it can only be queried with a login session or a system access token, put the token in the link with `usageAccessToken` (see the NewAPI section below).
- Script execution is limited to 5 seconds and 16 MiB of memory; the HTTP request times out after 10 seconds.

### Enabled or not

| How the link is written | After import |
|----------|--------|
| `usageScript` present, no `usageEnabled` | Enabled (since v4.0.10; not enabled on earlier versions) |
| `usageScript` present, `usageEnabled=true` | Enabled |
| `usageScript` present, `usageEnabled=false` | Not enabled; the user can turn it on in the app |
| No script | Not enabled |

The confirmation dialog shows the full script and an "Enabled" / "Disabled" tag.

`usageAutoInterval` sets the automatic query interval in minutes. `0` or leaving it out means no scheduled queries. Don't make it too short: every user will hit your endpoint at that rate.

### NewAPI / OneAPI style sites

The balance endpoint on these sites uses a system access token and a user ID, not the API key. Include `usageAccessToken` and `usageUserId` in the link:

```javascript
({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{accessToken}}",
      "New-Api-User": "{{userId}}"
    }
  },
  extractor: function (response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group,
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        unit: "USD"
      };
    }
    return { isValid: false, invalidMessage: response.message };
  }
})
```

`500000` is NewAPI's default quota conversion ratio. If you changed it, use your own value. A system access token has more power than an API key, so only put it in links generated after the user has logged in.

### Sub2API style sites

Sub2API ships a `GET /v1/usage` endpoint meant for CC Switch. It authenticates with the API key, so no `usageAccessToken` is needed. Its top-level fields are exactly the `isValid`, `planName`, `remaining` and `unit` that CC Switch understands, so the script is mostly a direct copy:

```javascript
({
  request: {
    url: "{{baseUrl}}".replace(/\/+$/, "").replace(/\/v1$/, "") + "/v1/usage",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    }
  },
  extractor: function (response) {
    var quota = response.quota || {};
    var result = {
      isValid: response.isValid !== false,
      unit: response.unit || quota.unit || "USD"
    };
    if (typeof response.planName === "string") result.planName = response.planName;
    if (typeof response.remaining === "number" && response.remaining >= 0) {
      result.remaining = response.remaining;
    }
    if (quota.limit > 0) {
      result.used = quota.used;
      result.total = quota.limit;
    }
    if (!result.isValid) {
      result.invalidMessage = response.status
        ? String(response.status)
        : "Query failed";
    }
    return result;
  },
})
```

- Codex and Grok Build provider endpoints include `/v1`; Claude Code's don't. The script strips a trailing `/v1` before appending the path, so it works for every app and you don't need `usageBaseUrl`.
- `remaining` is present in all three cases: wallet balance (`planName` is Sub2API's wallet balance label), a subscription group (`planName` is the group name, and `remaining` is whichever of the daily / weekly / monthly limits has the least left), and a key with its own total quota (with `quota.used` and `quota.limit` as well). When a subscription group has no limits at all, `remaining` is `-1`, meaning unlimited, and the script doesn't show that number.
- The response also has `rate_limits` (the key's 5-hour / 1-day / 7-day windows) and `subscription` (the group's daily / weekly / monthly usage and limits). To show these on separate lines on the card, have `extractor` return an array with one element per line.
- This script is identical to the Sub2API template under "Configure usage query" in the CC Switch app (since v4.0.10), so users adding your site by hand can pick it with one click too.
- The link generated by the "Import to CC Switch" button in the Sub2API admin panel comes with its own query script. It uses `?.` and `??`, which CC Switch can run, but it shows `-1` when a subscription group is unlimited.

## MCP, prompt and Skill links

<details>
<summary>Not closely related to an API service. Expand if you also distribute companion tools</summary>

These three types have little to do with your API service, but some sites also distribute companion tools, so here are the key points.

**MCP** (`resource=mcp`)

- `apps` is required, comma-separated: `claude`, `codex`, `gemini`, `grokbuild`, `opencode`, `hermes`, `pi`, `mcode`. `openclaw` is ignored.
- `config` is a Base64 `{"mcpServers": {"<ID>": {...}}}`. One link can carry several servers.
- If the user already has a server with the same ID, only the new apps are checked for it; its config is not overwritten by the link. Use a different ID if you want to update the config.
- Importing writes to the client config files right away. The `enabled` parameter currently has no effect.

**Prompt** (`resource=prompt`)

- `app` is one of `claude`, `codex`, `gemini`, `grokbuild`, `opencode`, `pi`, `mcode`. In CC Switch, `openclaw` and `hermes` are handled by the workspace editor and memory management, so distributing prompts to them via deep link is not recommended.
- `name` and `content` (Base64 Markdown) are required; `description` is optional.
- `enabled` is the initial value of the "Enable after import" switch in the confirmation dialog. Enabling replaces the user's current prompt file (such as `CLAUDE.md`) with this content, so usually don't set it.

**Skill** (`resource=skill`)

- `repo` is required, in the form `owner/name`, and must be a public GitHub repository; `branch` defaults to `main`.
- What gets added is the repository; users still choose which Skills to install themselves. The `directory` parameter is currently ignored.

</details>

## Code for generating links

### Backend (Node.js)

```javascript
function b64(text) {
  return Buffer.from(text, "utf8").toString("base64");
}

const usageScript = `({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: { "Authorization": "Bearer {{apiKey}}" }
  },
  extractor: function (r) {
    return { remaining: r.balance, unit: "USD" };
  }
})`;

function claudeImportLink(user) {
  const params = new URLSearchParams({
    resource: "provider",
    app: "claude",
    name: "Example AI",
    endpoint: "https://api.example.com",
    apiKey: user.apiKey,
    // Leave all models empty when serving genuine Claude models; only set
    // model / haikuModel / sonnetModel / opusModel for non-native models like DeepSeek or Kimi
    homepage: "https://example.com",
    // If there is no built-in icon for your brand, give your own icon URL; see "Adding your own icon"
    iconUrl: "https://example.com/static/logo.png",
    usageScript: b64(usageScript),
    usageEnabled: "true",
  });
  return `ccswitch://v1/import?${params}`;
}
```

A link for OpenCode in the Anthropic format only changes `app` and `apiFormat`, and adds the model:

```javascript
const params = new URLSearchParams({
  resource: "provider",
  app: "opencode",
  name: "Example AI",
  endpoint: "https://api.example.com/v1",
  apiKey: user.apiKey,
  apiFormat: "anthropic",
  model: "claude-sonnet-5-5",
});
```

### Backend (Python)

```python
import base64
from urllib.parse import urlencode

def b64(text: str) -> str:
    return base64.b64encode(text.encode("utf-8")).decode("ascii")

codex_toml = """\
model_provider = "example"
model = "example-coder"

[model_providers.example]
name = "Example AI"
base_url = "https://api.example.com/v1"
wire_api = "responses"
"""

def codex_import_link(api_key: str) -> str:
    params = {
        "resource": "provider",
        "app": "codex",
        "name": "Example AI",
        "apiKey": api_key,
        "configFormat": "toml",
        "config": b64(codex_toml),
    }
    return "ccswitch://v1/import?" + urlencode(params)
```

By default `urlencode` turns spaces into `+` and `+` into `%2B`, which matches how CC Switch parses links.

### Generating in the browser

The browser's `btoa` doesn't handle non-ASCII characters such as Chinese, so convert to UTF-8 first:

```javascript
function b64(text) {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  bytes.forEach((b) => (binary += String.fromCharCode(b)));
  return btoa(binary);
}
```

If you can generate the link on the backend, do it there. It's better if the key never passes through frontend JavaScript.

## Putting the link behind a button

Recommended flow:

1. The button itself carries no link. When the user clicks it, the frontend asks your backend for this user's link.
2. The backend checks that the user is logged in, generates the link with this user's own key (using the generator function from the previous section) and returns it.
3. Once the frontend has the link, it runs `window.location.href = link`. The browser asks whether to open CC Switch, and after the user agrees, the import confirmation dialog opens.

This way the link never appears in the page source and is generated only once, when the user actually clicks the button.

### Backend: return the link per user

Using Node.js (Express) as an example. `claudeImportLink` is the generator function from the previous section; write `codexImportLink` the same way:

```javascript
const builders = {
  claude: claudeImportLink,
  codex: codexImportLink,
};

app.post("/api/ccswitch-link", requireLogin, (req, res) => {
  const build = builders[req.body.app];
  if (!build) return res.status(400).json({ error: "unsupported app" });
  // The response contains the user's key: don't cache it, and keep it out of logs
  res.set("Cache-Control", "no-store");
  res.json({ link: build(req.user) });
});
```

Use POST, not GET: GET requests may be prefetched or cached by the browser, or show up in access logs with their parameters.

### Frontend: button and hint

```html
<button type="button" onclick="importToCcSwitch('claude')">Import to Claude Code</button>
<button type="button" onclick="importToCcSwitch('codex')">Import to Codex</button>
<p class="hint">
  Your browser will ask whether to open CC Switch. Choose "Open". Nothing happened? CC Switch may not be installed yet:
  <a href="https://ccswitch.io" target="_blank" rel="noopener">download CC Switch</a>, install it, then click again.
</p>

<script>
  async function importToCcSwitch(app) {
    const res = await fetch("/api/ccswitch-link", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ app }),
    });
    if (!res.ok) {
      alert("Failed to generate the import link. Please try again later.");
      return;
    }
    const { link } = await res.json();
    window.location.href = link;
  }
</script>
```

Notes:

- **Redirect right after the click**. Browsers only allow launching an external app when it is triggered by a user click, and some browsers stop treating it as click-triggered if too much time passes after the click. Your endpoint should respond within a second or two. Don't show another confirmation dialog after getting the link and make the user click again.
- **One link imports into one app only**. If you support several apps, give each app its own button, or let the user pick the client first and then click a single "Import" button. The endpoint and format may differ per app; see "Supported apps".
- **When CC Switch isn't installed, the browser usually shows nothing**, and the web page can't reliably detect it. So keep the hint next to the button visible at all times instead of waiting for a detection result.
- Clicking the same button twice imports two copies. After the user clicks once, you can change the button to "Sent to CC Switch" or disable it for a few seconds.

## Security and privacy

- **A link with a key is as good as the key itself**. A link written directly into a button's `href` appears in the page source and may be read by browser extensions. Following "Putting the link behind a button", where the link is requested from the backend only when the user clicks, avoids this. Only generate it on pages behind login. Never put it on public pages, in mass emails or in community announcements, and keep it out of your access logs and third-party analytics.
- Keep links as short as possible. With a long full `config`, the command line passed to CC Switch on Windows has a limit of about 32,000 characters, and longer links won't open.
- CC Switch applies allowlist filtering and prominent warnings to link content; it does not vouch for you to the user. Users see every item shown in the confirmation dialog. Don't put anything unrelated to your service in there, or users will stop trusting the button.

## Debugging

1. First click the link on your own computer, check that every item in the confirmation dialog is what you expect, then import, switch, and send a message.
2. If the confirmation dialog shows an error, the message is the cause. Check it against "Rejected links" above.
3. Import succeeded but no balance shows: open "Configure usage query" on the provider card and click "Test script" to see the error. The most common causes are a request URL with a different origin, a balance endpoint that doesn't return JSON, and wrong fields in `extractor`.
4. Import succeeded but requests fail after switching: most likely the API format is wrong (see "Supported apps") or the model name is wrong.
5. The `iconUrl` icon doesn't show: download failures don't produce an error; the reason is in the CC Switch log (`~/.cc-switch/logs/cc-switch.log`, search for `Skipped deep link icon`). The most common causes are a wrong `Content-Type`, an image over 256 KB, or a URL that requires login or has hotlink protection. Check the response headers with `curl -I`.

## Compatibility with older versions

Not all of your users have upgraded to the latest version. These behaviors differ on older versions:

| Behavior | v4.0.10 and later | v3.19.0 – v4.0.9 |
|------|-----------|------------------|
| Script present, no `usageEnabled` | Enabled | Not enabled |
| `config.toml` embedded for Codex | Provider table and model fields taken per the allowlist | Only the endpoint, key and model name are taken; the rest is dropped and the config is rebuilt from a fixed template |
| Model table embedded for Grok Build | Fields of the selected table are taken | Only the endpoint, key and model name are taken |
| Clicking a link while CC Switch isn't running | The confirmation dialog opens after startup | Occasionally lost; the user has to click again |
| `apiFormat` | Mapped per app | Ignored; the default format is always used |
| `app=pi` | Supported | Import rejected |
| `iconUrl` | Downloaded and used as the card icon | Ignored; shows `icon` or the first letter of the name |
| Switch / enable after import | A switch in the confirmation dialog, with `enabled` as its initial value | Follows `enabled` in the link directly; OpenCode / OpenClaw / Hermes always write to the app config |

To get consistent behavior across versions: set `usageEnabled=true` explicitly for the usage query, and for Codex / Grok Build put the key information (endpoint, key, model) in URL parameters too, not only in `config`. If your API is not the app's default format, cards imported on older versions will have the wrong format; you can add a hint next to the button asking users to upgrade CC Switch.
