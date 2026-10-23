# DSH Native Architecture

## Purpose

This change makes DeepSeek Harness a first-class CC Switch application while retaining DSH as the source of truth for providers, credentials, and the current model. The first milestone covers provider switching, default-model selection, and API-key management; session browsing, usage import, and environment detection/install are planned as follow-up pull requests.

## Product Reference

The DSH experience follows CC Switch's Codex design:

- explicit application identity on the provider page;
- structured provider fields rather than a raw JSON-only form;
- API key, endpoint, protocol, default model, model discovery, and model catalog;
- provider cards with endpoint, model count, current provider, and current model.

DSH does not inherit Codex OAuth, proxy takeover, failover routing, or protocol transformation because the DSH runtime owns those concerns differently.

The upstream submission follows the official v4 UI, version, and update mechanism.
DSH appears in the shared sidebar and provider page. Provider cards use the shared
additive presentation (added/available sections, native default-model state, and
current-provider removal protection). The official API-key preset belongs to the
vendor group, not the OAuth login group.

## Data Flow

Since `@deepseek-ai/dsh` 0.2.1-rc.2, `$DSH_HOME/settings.yaml` is a legacy
document: DSH renames it to `settings.yaml.imported` at startup and layers each
section into the active profile once per process. Persisted edits belong to the
profile's patch layer. Reloading depends on DSH's enabled `dsh-hmr` plugin;
writing a file is not proof that a running process has reconciled the edit.
Without HMR, restart DSH after changing the configuration.

This milestone manages **DSH Desktop's `desktop` profile only**, not whichever
CLI profile or process happens to be running. The Electron app owns this profile
exclusively (`dsh --profile desktop` refuses to launch it). CC Switch does not
infer launch-time CLI overlays or claim to control those processes:

```text
~/.dsh/profiles/desktop/cordis.patch.yml   (top-level YAML sequence)
  - id: llm-deepseek         config.<fields> ------------> deepseek-official card
  - id: llm-pi-ai            config.providers.<route> ----> custom DSH cards
  - id: agent-default-model  config.provider/model -------> current provider/model badges

~/.dsh/.credentials.yaml
  version: 1
  refs.<NAME> ----------------------> masked API key form value
  records.* ------------------------> preserved, never exposed

Native adapter -> DSH provider service -> providers table -> React query -> cards/forms
```

Writes preserve unrelated patch entries and manage only the three entries above.
The official `llm-deepseek` config is replaced as a complete config, with only
schema-supported fields; the historical `profile` field is never emitted.
Custom routes replace only their key in the composed `llm-pi-ai.providers` config.
Deleting the last route retains an explicit empty override, and removing the
official/default entry retains a disable tombstone, so an older layer cannot
silently resurrect it. Enabling writes an explicit enabled state.

The adapter is a bounded, non-executing projection for persisted Desktop files,
not DSH's runtime loader. It honors literal managed-entry patches, their names
and disabled flags, and the home patch's precedence. Unsupported managed-entry
expressions or unresolved custom bundles are rejected rather than evaluated or
guessed. Standard bundle compatibility is based on the verified 0.2.1-rc.2
managed rows. The shipped auto-review, agent-team-profile and inspector-profile
layers were also checked: they do not alter these three namespaces and can be
enabled alongside the base/web bundles. Locally readable bundle definitions
still take precedence over this bounded installation fallback. It does not introspect runtime plugin/schema defaults, environment
credentials, launch-time CLI overlays or an already-running session's model.

Both patch and credential documents are prevalidated. Related native mutations
use byte snapshots and compensating rollback on errors; this is not a distributed
transaction, crash journal, or coordination with another process writing the
same files. Avoid editing these settings simultaneously in DSH and CC Switch;
quit DSH before retrying a conflicting write. Use DSH's settings UI for configurations outside this adapter's
supported subset. A conflicting higher-precedence home override of any managed
namespace blocks writes conservatively, even when editing another managed
namespace, instead of reporting a successful switch that cannot take effect.

Before the first write, CC Switch migrates a pending legacy document using the
namespace mappings from `SettingsForms.importLegacyDocument`: map removed
sections (`ui-developer-tools` → `ui-settings`, `ui-onboarding` →
`ui-settings-general`, `shell` → the platform sandbox), drop the historical
invalid `profile` key, and merge sections into their patch entries. Unlike DSH's
rename-first path, CC Switch validates and persists the complete patch before
renaming the source to `settings.yaml.imported`; existing imported backups are
never overwritten. Errors preserve the original documents. Reads overlay a
still-unimported legacy document for the transition.

If the desktop profile does not exist yet, CC Switch initializes it exactly the
way DSH's `initProfile` does (manifest with the `dsh-base` + `dsh-web-app`
bundles the desktop app itself writes, empty patch template, pnpm workspace) —
existing files are never touched. `patchReload` is not emitted as a substitute
for the HMR plugin. File edits manage persisted Desktop settings; they do not
acknowledge a live runtime reload.

The DSH home directory resolves in this order: CC Switch directory override (`dshConfigDir` in Settings) > `$DSH_HOME` > `~/.dsh`. Under `cfg(test)` the settings override is skipped so `DSH_HOME`-based test isolation always wins.

## Commands

- `import_deepseek_harness_providers_from_live` — native-to-DB sync (also runs at startup).
- `get_dsh_current_state` — native provider ids, the current provider/model for card badges, and the credential references in use (new custom routes pick a reference that avoids them, so a second route can never silently overwrite the first route's key).
- `set_dsh_current_model` — writes `agent-default-model` after validating provider/model membership; because DSH couples the two, setting a model on a non-current provider also makes that provider current.

MCP and Skills entries are hidden for DSH because DSH exposes neither an MCP configuration namespace nor a skills directory.

## Ownership Boundaries

### Native adapter

`src-tauri/src/deepseek_harness_config.rs` parses and atomically updates DSH-owned documents. It resolves `$DSH_HOME`, supports official and pi-ai routes, reads the current model, and writes credentials under the version-1 `refs` map while preserving `records`.

### Provider service

`src-tauri/src/services/provider/deepseek_harness.rs` mirrors native routes into the existing provider database. Native files remain authoritative. The service records route ownership as `meta.providerType` (`dsh_deepseek` or `dsh_pi_ai`) so add, edit, delete, and switch operations write back to the correct patch entry. Custom routes persist their display name in the native entry, so renames survive the next native sync; the official route has no native display name, so an existing row keeps the name the user chose.

### Frontend

The shared provider page displays DSH cards and current state. The DSH add/edit experience uses the same shared provider form shell as OpenCode: `useDeepSeekHarnessFormState` keeps the DSH structured state in sync with the `settingsConfig` JSON, and `DeepSeekHarnessFormFields` renders the credential reference, base URL, API format, model catalog, and default model. Custom routes derive their credential reference from the provider key (reusing the preset catalog's `reserveApiKeyEnv` so references stay unique) until the user edits it by hand. The preset catalog is derived from the OpenCode preset list so both apps stay aligned.
