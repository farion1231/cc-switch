# VS Code Copilot

## Behavior

CC Switch exposes VS Code Copilot as a first-level application and manages its Custom Endpoint BYOK catalog as portable provider data, similar to the OpenCode integration:

- Each CC Switch BYOK provider is written as one `customendpoint` Group whose name is the configured provider name.
- A Group owns one protocol mode, endpoint URL, API key, and request-header set, and can expose multiple models that all share that connection.
- All managed models remain available in the VS Code Copilot model picker at the same time.
- After fetching a provider's model list, CC Switch enriches exact or unambiguous model matches from the same models.dev capability catalog used by OpenCode. Tool calling, vision, reasoning, context, and input/output limits are filled automatically for common model families. Model-specific request contracts that models.dev cannot express, such as fixed sampling parameters, are matched by model ID and do not depend on the provider label or endpoint. Unknown or ambiguous models default only `toolCalling` to `true` so VS Code keeps them visible for agent use; other unknown capabilities remain automatic, and tool calling can still be disabled explicitly when an endpoint does not support it.
- Before the user makes an explicit target selection, CC Switch manages the VS Code Stable default profile (or the first detected default profile when Stable is unavailable).
- Selecting or editing a model in CC Switch does **not** change the model selected in VS Code.
- Provider/model CRUD, enable/disable changes, and profile selection synchronize automatically.
- Stopping management removes CC Switch-owned groups and clears the selected profiles so later catalog edits do not add them back.
- The manual sync action is retained as a repair/reconciliation operation.

The app switcher opens a provider-only catalog as a primary page, matching the other managed applications instead of exposing profile and file-management controls there. Its toolbar uses VS Code's real configuration locations for Skills, Prompt Files, Sessions, and MCP, exposes Sync Targets with the same Agents-configuration icon used by OpenClaw, retains the add-provider action, and links directly to Usage Statistics filtered to VS Code Copilot. The add action opens the shared full-screen provider form with an expandable multi-model editor. VS Code Copilot is shown by default in Settings > General > Apps on Main Page and can be hidden or shown again there.

The Sync Targets toolbar action opens VS Code profile selection together with import, restore, resync, and stop-management actions. Advanced Settings > Configuration Directories remains limited to application directory overrides. Provider cards and provider editing remain on the first-level application page.

## Managed files

Detected targets include VS Code Stable and Insiders default profiles and named profiles declared in VS Code's profile metadata. Language models, prompts, and MCP are resolved independently according to the corresponding VS Code Profile inheritance flags, and each resource is deduplicated by physical path before synchronization. A custom absolute path to `chatLanguageModels.json` can also be added. Canonical path identity prevents the same physical file from being managed twice through aliases such as `..`, symlinks, or Windows case differences.

New managed groups carry CC Switch ownership metadata without altering the provider-facing Group name. Groups created by older branch builds with the `CC Switch:` prefix are also recognized and migrated. Other Custom Endpoint provider groups are preserved.

Before the first write to an existing target, CC Switch creates:

```text
chatLanguageModels.json.cc-switch.bak
```

The portable provider catalog is stored in the normal CC Switch provider database under its own `copilot-byok-catalog` namespace, so database export, WebDAV, and S3 synchronization include it without colliding with the normalized `VSCode Copilot` usage provider. Selected VS Code editions/profiles and custom absolute paths remain in the device-local `~/.cc-switch/copilot-byok.json` store and are intentionally excluded from portable catalog data. The store always uses the default local home directory, independent of any portable application configuration override; a matching legacy copy in the override directory is migrated and removed automatically. Catalog and profile updates are preflighted and rollback-protected. If profile synchronization fails, CC Switch restores the previous catalog and target snapshots. Startup synchronization reconciles the catalog and selected targets after an interrupted process.

## Existing configuration import

`Import and manage` reads non-managed `vendor: "customendpoint"` groups from a selected VS Code profile.

- A group is only removed from its original position after every model in the group can be represented.
- Unknown group/model fields and future edit-tool values are retained so importing and re-rendering does not silently downgrade newer VS Code schemas.
- Incomplete or incompatible groups are skipped as a whole.
- Groups are reused only when their full provider/model semantics are equivalent; internal CC Switch IDs and display metadata are ignored for this comparison.
- Provider-name conflicts are namespaced with an `imported` suffix.
- VS Code SecretStorage `${input:...}` references are preserved, together with any `${apiKey}` header placeholders. Another profile may require entering the referenced secret again.

## Security model

Credentials entered directly in CC Switch are materialized into each model's request headers in `chatLanguageModels.json`, because an external application cannot create the corresponding VS Code SecretStorage entry. Imported `${input:...}` SecretStorage references remain references and are not converted to plaintext. Protect both the VS Code configuration file and the CC Switch database/backup files as credential-bearing files.

The CC Switch store file is created and atomically replaced with mode `0600` on Unix platforms.

## Scope

This only manages Copilot Chat/utility models exposed through VS Code Custom Endpoint BYOK. It does not replace:

- inline completions,
- Next Edit Suggestions,
- embeddings,
- GitHub Copilot account authentication.

## Usage statistics

Statistics are reconstructed from VS Code Copilot's local conversation history and therefore do not require local proxy takeover. The importer is limited to GitHub Copilot and Custom Endpoint requests; language-model providers registered by unrelated VS Code extensions are ignored. Every imported row uses the provider name `VSCode Copilot`, regardless of the upstream model vendor. When the selected model is `copilot/auto`, the recorded billing/display model prefers the request result's `resolvedModel` and `resolvedModelName`, while retaining `copilot/auto` as the requested model. Incomplete requests are skipped, canceled/error requests keep their state, cached-input tokens are separated when VS Code records them, and JSONL mutations are replayed so deleted or truncated requests do not remain in statistics. VS Code session rows retain their stable request IDs instead of entering the generic 30-day detail rollup, preventing catalog replays from counting the same historical request again. Cache hit rate is exact only for requests whose persisted result includes cache-token details; a missing field is not evidence that the upstream cache was not used. GitHub Copilot subscription requests are not assigned an estimated USD cost; Custom Endpoint costs appear only when matching pricing is configured.

## Main review files

- `src-tauri/src/copilot_byok.rs`
- `src-tauri/src/copilot_byok/model.rs`
- `src-tauri/src/copilot_byok/store.rs`
- `src-tauri/src/copilot_byok/sync.rs`
- `src-tauri/src/copilot_byok/import.rs`
- `src-tauri/src/copilot_byok/vscode.rs`
- `src-tauri/src/commands/copilot_byok.rs`
- `src/lib/api/copilotByok.ts`
- `src/components/settings/CopilotByokSettings.tsx`
- `src/components/settings/CopilotByokGroupPanel.tsx`
- `src/components/AppSwitcher.tsx`

## Suggested local review

```bash
pnpm install --frozen-lockfile
pnpm typecheck
pnpm format:check
pnpm test:unit
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml copilot_byok --lib
```

The repository root intentionally has no `Cargo.toml`; always pass `--manifest-path src-tauri/Cargo.toml` to Cargo commands.

Also manually verify that first launch selects the Stable default profile, then verify explicit Stable/Insiders default and named-profile selection, independent language-model/prompt/MCP inheritance, preservation of user-owned groups, import conflict behavior, stop-management deselection, backup restore, explicit empty target selection, and repeated idempotent synchronization.
