# Application restrictions for provider imports

An authorization service may issue different provider profiles (credentials, endpoints and model catalogs) for different clients. CC Switch can retain an optional application restriction on each provider to catch accidental retargeting. The service still chooses the profile and the gateway still enforces credential permissions.

## Import contract

Add `allowedApps` to an existing provider deep link. Its value is a URL-encoded JSON array of canonical app IDs. For example, this JavaScript constructs a provider link intended for Claude Code or Claude Desktop:

```js
const link = new URL("ccswitch://v1/import");
link.searchParams.set("resource", "provider");
link.searchParams.set("app", "claude");
link.searchParams.set("name", "Team profile");
link.searchParams.set("homepage", "https://gateway.example");
link.searchParams.set("endpoint", "https://gateway.example");
link.searchParams.set("apiKey", "public-example-key");
link.searchParams.set("allowedApps", JSON.stringify(["claude", "claude-desktop"]));
```

This is a synthetic example. Follow your existing secure credential-delivery process for real keys; this feature does not add encryption or an authorization-code exchange.

- Omitted: existing behavior, no declared restriction.
- Present: a nonempty, duplicate-free array of supported IDs. Unknown IDs, aliases, empty strings, malformed JSON and repeated `allowedApps` parameters are rejected.
- The selected `app` must be included. CC Switch checks this in the native import path, including direct IPC calls, before saving the provider or writing a client file.
- The confirmation dialog shows the allowed applications. The value is retained in `provider.meta.allowedApps`, not in the client's native settings.
- An inline `config` supplies native client settings; it cannot override this top-level restriction.

Canonical IDs are `claude`, `claude-desktop`, `codex`, `gemini`, `grokbuild`, `opencode`, `openclaw`, `hermes`, `pi`, and `mcode`. This list identifies applications, not new deep-link support: the public import parser's existing supported-app list still applies. In particular, this change does not enable Claude Desktop or MiniMax Code deep-link imports. Existing Desktop/MiniMax provider records can carry the metadata through the native provider API.

## Subsequent operations

Normal provider editing preserves the declaration. Native provider saves, activation and live configuration writes validate it. Local route selection and aggregation eligibility also check it. Universal-provider save/sync validates all enabled destinations before changing children; a declared parent restriction is propagated while unrelated child metadata stays intact. When the parent has no declaration, an existing child's declaration is retained.

A credential update must preserve the provider's metadata and owning app. This patch does not add a browser login or credential-renewal flow. Such an integration should separately bind the operation to its app, provider and account, and update only the authorized credential fields. Gateway-specific model grouping and renewal policy belong to that integration, not to CC Switch.

## Compatibility and trust

This is a configuration guardrail, not proof of the requesting program's identity. It does not prevent the machine owner from changing files, deleting/reimporting a provider without the declaration, or copying a bearer key. No credential encryption or editing lock is introduced.

Older versions may ignore `allowedApps` or drop metadata on save. Integrators must set a minimum supported CC Switch version and must enforce actual model/group permissions on the server. Do not use the declaration to protect a key that the gateway would otherwise accept for the wrong group.
