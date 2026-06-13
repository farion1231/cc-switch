# Provider Management Drawer Design

## Context

Issue #4105 asks for better management when users have many aggregator API providers. The current Provider list is mostly card based. It has a hidden `Ctrl+F` search overlay, but search only matches name, notes, and URL. There is no batch selection, no dense list mode, and no grouped view for multiple related API configurations.

Users also want a lightweight "multi API / multi config" view. The goal is not to introduce a new backend routing model. It is to make several existing Provider records feel like variants of one logical provider in the UI, while preserving the current Provider storage, switching, editing, testing, and live config behavior.

## Goals

1. Add a visible Provider management toolbar with search, view mode, and batch actions.
2. Expand fuzzy search to include provider id, name, notes, website URL, API/base URL, provider type, model names, and safe API key fingerprints.
3. Add a compact list mode so users can scan many aggregator providers without large card scrolling.
4. Add batch operations for selected providers where semantics are clear.
5. Add a Provider drawer that shows related sub-configurations under a provider or group.
6. Keep sub-configurations as existing Provider records so the backend switching model does not change.
7. Add a common-config helper that can extract shared settings from the current provider while excluding API and model-specific fields.

## Non-Goals

- Do not add backend load balancing across multiple API keys.
- Do not add random/round-robin selection inside one Provider.
- Do not store multiple active endpoint configs inside one Provider record.
- Do not change the provider database schema.
- Do not duplicate provider records automatically without an explicit user action.
- Do not expose full API keys in list, search result, drawer, or compact row UI.

## Data Model

### Existing Provider Records Stay Canonical

Each sub-configuration remains a normal Provider:

- It has its own `id`.
- It has its own `settingsConfig`.
- It can be edited, tested, deleted, duplicated, switched, or added to live config with existing logic.

The drawer is a display and organization layer.

### Group Common Configuration

Provider groups may expose "group common config" candidates. This is different from the existing app-level common config snippet used by the provider forms.

Group common config means:

- A logical provider group can have shared values such as base URL, API key, model mapping, API format, or other app-specific fields.
- Each sub-config can opt into shared fields with checkboxes.
- Opting in writes the selected shared values back into that sub-config's own Provider record.
- The backend still only receives normal Provider records.

This keeps the mental model simple:

- The group is for organizing and reusing values.
- The Provider remains the source of truth for what gets written into Claude Code, Codex, Gemini, OpenClaw, Hermes, or other target clients.

The UI should present group common config as a convenience for editing related Provider records together, not as a new runtime layer.

### Optional Group Metadata

Add optional fields to `ProviderMeta`:

```ts
providerGroup?: string;
providerVariantLabel?: string;
groupCommonConfigEnabled?: Record<string, boolean>;
```

Rust `ProviderMeta` should mirror these optional fields so metadata is not dropped during update flows.

`groupCommonConfigEnabled` records which group-shared fields this Provider inherits. Initial field keys should be app-neutral where possible:

- `apiKey`
- `baseUrl`
- `modelMapping`
- `apiFormat`
- `customUserAgent`

Apps can ignore unsupported field keys.

The first implementation should support both:

- Explicit grouping through `meta.providerGroup`.
- Automatic grouping fallback when no explicit group is set.

Automatic grouping should be conservative:

- Prefer exact `meta.providerGroup`.
- Else group by normalized provider name prefix only when names clearly share a base pattern.
- Else group by normalized base URL host for aggregator-like third party providers.
- Else leave the provider ungrouped.

## User Experience

### Toolbar

Add a toolbar above the provider list:

- Search input, always visible.
- Result count: `Showing X of Y`.
- View toggle: `Cards` / `Compact`.
- Selection summary when items are selected.
- Batch action menu.

The existing `Ctrl+F` shortcut should focus the visible search input instead of opening a separate floating search panel.

### Search

Search should match:

- Provider name.
- Provider id.
- Notes.
- Website URL.
- Extracted base URL.
- Provider category.
- `meta.providerType`.
- API format.
- Model ids and model aliases.
- Safe API key fingerprint, for example `sk-1234...abcd`.

Search must not render or return full API keys.

### Card Mode

Card mode keeps the current ProviderCard layout and adds:

- A selection checkbox.
- A drawer toggle button.
- Optional group summary label when the provider has grouped variants.

### Compact Mode

Compact mode shows a dense row per provider or group:

- Select checkbox.
- Status/current marker.
- Provider icon and name.
- API/base URL summary.
- Model summary.
- Usage or quota summary when available.
- Primary action.
- Inline action icons.
- Drawer toggle.

Compact rows should be stable height and should not expand except through the drawer.

### Drawer

The drawer opens below a provider/group row. It lists related Provider records as sub-config rows and, when a group has more than one Provider, shows a small group common config section.

The group common config section shows shared values detected from the group, for example:

- Shared API key fingerprint.
- Shared base URL or host.
- Shared model mapping.
- Shared API format.
- Shared local proxy User-Agent.

Each sub-config row can show checkboxes for supported shared fields:

- `Use group API key`
- `Use group base URL`
- `Use group model mapping`
- `Use group API format`
- `Use group User-Agent`

When checked, the shared field value is written into that Provider's own config. When unchecked, the Provider keeps or edits its own value independently.

Each sub-config row shows:

- Variant label or provider name.
- Safe API key fingerprint.
- Base URL host or endpoint.
- App-specific model summary.
- API format or routing mode.
- Current/live status.
- Edit, test, duplicate, delete, and configure usage actions.
- Group common config inheritance checkboxes when supported.

Example for Claude Code:

```text
API sk-2311...asda
Sonnet = minimax-2.5
Opus = minimax-2.7
Haiku = minimax-2.5-lite
Format = OpenAI Chat
```

Example for Codex:

```text
API sk-prod...8f2a
Model = gpt-5.4
Wire API = Responses
Base URL = api.example.com
```

Example for Gemini:

```text
API AIza...x9Q
Model = gemini-2.5-pro
Base URL = generativelanguage.googleapis.com
```

The drawer summary itself should stay lightweight. It may provide inheritance checkboxes for group common config, but full arbitrary edits continue through the existing Edit Provider dialog.

## Batch Operations

Batch operations should operate on selected visible Provider records.

Supported operations:

- Test selected providers.
- Delete selected providers, with confirmation.
- Add selected providers to live config for additive-mode apps.
- Remove selected providers from live config for additive-mode apps.
- Add selected providers to failover queue when failover mode is active.
- Remove selected providers from failover queue when failover mode is active.

Unsupported operations:

- Batch enable/switch for single-current apps, because only one provider can be current.
- Batch edit, because each provider has different detailed fields.

Delete confirmation must list the count and a small sample of provider names.

## Common Config Helper

There are two distinct common-config concepts:

1. App-level common config snippet, which already exists in provider forms.
2. Group common config, which is scoped to one provider group drawer and is used to keep related Provider records aligned.

### App-Level Common Config

Existing forms already support writing common config snippets. Add an explicit helper:

```text
Update common config from current provider, excluding API and model fields
```

This helper extracts provider-neutral settings from the current editor state and saves them as the app's common config snippet.

Fields to exclude:

- API keys and tokens.
- Base URLs.
- Model names.
- Claude model mappings and display names.
- Codex model and model catalog entries.
- Gemini model field.
- Provider-specific auth bindings.

Fields that may be included:

- Attribution options.
- Tool/team feature flags.
- Auto-upgrade flags.
- Non-secret request behavior flags.
- Other shared settings already accepted by existing common-config extraction.

This feature should reuse existing common-config APIs and hooks where possible. If the backend extraction already excludes the relevant fields for an app, the UI can call the existing extraction path and only add clearer labeling.

### Group Common Config

Group common config can include API and model fields because it is not global across the whole app. It only applies to selected Provider records inside one drawer group.

For the first implementation:

- Derive group common config candidates from the first selected/primary Provider in the group.
- Let users apply individual fields to sibling sub-configs with checkboxes.
- Persist the result by updating each affected Provider's normal `settingsConfig` and `meta.groupCommonConfigEnabled`.
- Never store full API keys in group metadata; only store inheritance flags. The actual API key remains in each Provider's normal config after apply.

This means a Minimax group can have one shared base URL and API key, while each sub-config keeps a different Claude model mapping, or vice versa.

## Component Boundaries

Create focused frontend helpers instead of expanding `ProviderList.tsx` indefinitely:

- `providerSearch.ts`: extracts searchable text and safe fingerprints.
- `providerSummary.ts`: extracts base URL, API key fingerprint, and model summaries.
- `providerGrouping.ts`: creates grouped display rows from provider records.
- `providerGroupCommonConfig.ts`: extracts supported group-shared fields and applies them to Provider config copies.
- `ProviderManagementToolbar.tsx`: search, view mode, selection, and batch actions.
- `ProviderCompactRow.tsx`: dense row UI.
- `ProviderConfigDrawer.tsx`: expanded sub-config summary.

`ProviderList.tsx` should own state coordination:

- Search text.
- View mode.
- Selected provider ids.
- Expanded group/provider ids.
- Invoking existing callbacks for actions.

## Accessibility And Safety

- All icon-only buttons need titles or accessible labels.
- Selection checkbox labels must include provider names.
- Drawer toggles must expose expanded/collapsed state.
- API keys must be masked before rendering.
- Group common config must store inheritance flags only, not a second copy of secrets.
- Batch delete must require confirmation.
- Search must never include full raw keys in visible output.

## Testing

Add or update tests for:

- Search matches provider id, base URL, model, provider type, and safe API key fingerprint.
- Full API key is not rendered in compact row or drawer.
- Compact mode renders more concise rows and still wires actions.
- Drawer groups related providers and shows model/API summaries.
- Group common config checkboxes apply shared API/base/model fields by updating normal Provider records.
- Group common config does not store raw API keys in metadata.
- Selection supports visible results and clears invalid selections when filtering.
- Batch delete calls the existing delete callback once per selected provider after confirmation.
- Common-config helper label/action is present where supported.

Use existing frontend unit tests where possible. The local environment may block Vitest startup on Windows due an access-denied config resolution issue; `npm run typecheck` remains required verification.

## Rollout

Phase 1:

- Visible toolbar.
- Expanded search.
- Compact mode.
- Selection and batch actions.
- Drawer summary with automatic grouping.
- Group common config extraction and per-sub-config inheritance checkboxes.

Phase 2:

- Edit UI for explicit `providerGroup` and `providerVariantLabel`.
- Persist optional group metadata through TypeScript and Rust ProviderMeta.
- Refine per-app model summaries.

Phase 3:

- Optional advanced grouping controls if users need manual regrouping at scale.
