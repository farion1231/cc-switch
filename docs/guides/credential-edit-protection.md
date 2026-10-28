# Credential editing protection

This opt-in guard helps users avoid accidentally changing an imported gateway credential while editing a provider. It is not encryption, authorization enforcement or proof of a provisioning service's identity.

## Import and user choice

A provider deep link can include `credentialEditProtected=true`. The import confirmation shows an **unchecked** protection checkbox alongside the existing gateway/provider details. The value sent to the native import command reflects the user's choice, not merely the URL's suggestion. Omitted/false values keep existing behavior; malformed or repeated query parameters are rejected.

```js
const link = new URL("ccswitch://v1/import");
link.searchParams.set("resource", "provider");
link.searchParams.set("app", "claude");
link.searchParams.set("name", "Team gateway");
link.searchParams.set("endpoint", "https://gateway.example");
link.searchParams.set("apiKey", "public-example-key");
link.searchParams.set("credentialEditProtected", "true");
```

The example uses a dummy credential. This proposal does not change existing credential delivery or make deep-link keys confidential. The displayed source is provided by the link; it is not a verified issuer identity.

The accepted choice is stored in `ProviderMeta.credentialEditProtected`, never in the native client's settings. Initial custom endpoints are recorded with the protected provider before activation. Existing public deep-link app support is unchanged: this patch does not add Claude Desktop or MiniMax Code deep-link imports.

## Editing and removing protection

Protected providers show a notice in the editor, and the shared API key input is read-only. Revealing/copying the value remains unchanged. Raw configuration can still be edited, but native validation rejects changes to the recognized model-request credential configuration, its sources and its destinations. Model/name edits and unrelated settings remain editable.

The comparison handles the standard inline-key shapes for Claude Code/Desktop, Codex, Gemini CLI, Grok Build, OpenCode V1/V2, OpenClaw, Hermes, Pi and MiniMax Code. JSON and the active Codex/Grok TOML profile are compared semantically, so TOML comments/formatting do not constitute a credential change. Credentials are never resolved from environment variables, files or commands for this check. Official or managed OAuth accounts and external credential sources are not supported by this guard.

Kimi Code, DeepSeek Harness, Antigravity and Oh My Pi remain outside this guard's supported configuration shapes. Their ordinary import/edit paths keep working where supported upstream. The import dialog disables the protection option with an explanation, and native writes reject a protected record for these apps instead of claiming to protect unrecognized credential fields.

Protected fields include recognized keys, auth helpers/selectors, provider auth headers and gateway addresses. Provider ID/auth binding changes and silently dropping the protection flag are rejected. Custom endpoint membership is guarded separately; usage timestamps are not locked. Usage-script configuration is outside the model-request credential guard.

Checks happen under the database connection/transaction used for saving, including direct settings writes and ID replacement. The Pi writer checks before changing its live file, and universal sync preflights all child updates before projecting any of them.

**Remove protection** is an explicit local action with confirmation. It changes only the metadata flag, preserves the current credential/settings, and closes the editor without applying unsaved edits. Reopen the editor to make a deliberate manual credential change. Cancel/failure leaves the editor open. A late result from a closed editor does not close another editor, and an old protected save cannot silently re-enable protection after removal. Re-enabling an existing row through an ordinary save is not supported; import a new protected profile instead.

Deleting a provider remains available. A machine owner can also edit files or use a key outside CC Switch; this guard does not prevent that.

## Authorization and compatibility boundaries

No browser authorization, SSO, renewal exchange or remote credential-write API is introduced. Future renewal support needs its own validated operation bound to the app/provider/account and expected state. Do not implement renewal by silently dropping the guard through an ordinary save. Until that integration exists, intentional manual replacement requires explicit removal of protection.

This is a reference implementation for design discussion. Older binaries can ignore/drop the metadata, and whole-database restoration/import or external SQL edits can replace it. It is not a cross-version policy-enforcement mechanism. The metadata/ownership model should be agreed with the cross-app provider design before release.

Server-side authorization still controls model access, credential expiry and revocation. No gateway/vendor/group names are hardcoded, and the patch is independent of the application-allowlist proposal.
