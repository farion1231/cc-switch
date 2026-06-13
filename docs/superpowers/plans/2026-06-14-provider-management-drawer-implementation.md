# Provider Management Drawer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the #4105 Provider management MVP: visible search, compact list, batch actions, grouped drawers, and group common config inheritance while keeping each sub-config as a normal Provider.

**Architecture:** Keep provider storage and switching APIs unchanged. Add focused frontend helper modules under `src/lib/provider-management`, then compose them in `ProviderList.tsx` with new presentational components. Optional grouping flags are stored in existing Provider `meta`, so no database schema migration is required.

**Tech Stack:** React 18, TypeScript, TanStack Query, Tauri provider API, Vitest/Testing Library where the local environment permits.

---

## File Structure

- Create `src/lib/provider-management/providerSummary.ts`: extract base URL, API key fingerprint, model summary, and searchable text.
- Create `src/lib/provider-management/providerGrouping.ts`: group Provider records into display groups.
- Create `src/lib/provider-management/providerGroupCommonConfig.ts`: extract and apply group common config fields to Provider config copies.
- Create `src/components/providers/ProviderManagementToolbar.tsx`: search input, result count, view toggle, selection summary, and batch buttons.
- Create `src/components/providers/ProviderCompactRow.tsx`: dense Provider row with checkbox, summary, actions, and drawer toggle.
- Create `src/components/providers/ProviderConfigDrawer.tsx`: sub-config summary rows and group common config checkboxes.
- Modify `src/components/providers/ProviderList.tsx`: state coordination for search, grouping, selection, view mode, drawer expansion, batch actions, and provider update mutation.
- Modify `src/components/providers/ProviderCard.tsx`: add optional checkbox, drawer toggle, and group summary props.
- Modify `src/types.ts` and `src-tauri/src/provider.rs`: add optional ProviderMeta grouping fields so updates preserve them.
- Add tests under `tests/lib/provider-management/*.test.ts` and extend `tests/components/ProviderList.test.tsx`.

---

## Task 1: Provider Summary Helpers

**Files:**
- Create: `src/lib/provider-management/providerSummary.ts`
- Test: `tests/lib/provider-management/providerSummary.test.ts`

- [ ] **Step 1: Write failing tests**

```ts
import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  extractProviderSummary,
  maskSecret,
} from "@/lib/provider-management/providerSummary";

const provider = (settingsConfig: Record<string, unknown>): Provider => ({
  id: "minimax-a",
  name: "Minimax API A",
  settingsConfig,
  meta: { providerType: "aggregator", apiFormat: "openai_chat" },
});

describe("providerSummary", () => {
  it("masks API keys without returning raw secrets", () => {
    expect(maskSecret("sk-1234567890abcdef")).toBe("sk-123...cdef");
  });

  it("extracts Claude base URL, key fingerprint, models, and search fields", () => {
    const summary = extractProviderSummary(
      provider({
        env: {
          ANTHROPIC_BASE_URL: "https://api.minimax.test/v1",
          ANTHROPIC_AUTH_TOKEN: "sk-1234567890abcdef",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "minimax-2.5",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "minimax-2.7",
        },
      }),
      "claude",
    );

    expect(summary.baseUrl).toBe("https://api.minimax.test/v1");
    expect(summary.apiKeyFingerprint).toBe("sk-123...cdef");
    expect(summary.modelSummary).toContain("Sonnet=minimax-2.5");
    expect(summary.searchText).toContain("minimax-2.7");
    expect(summary.searchText).not.toContain("sk-1234567890abcdef");
  });
});
```

- [ ] **Step 2: Verify tests fail**

Run: `npx vitest run tests/lib/provider-management/providerSummary.test.ts`

Expected in unrestricted environments: FAIL because the module does not exist. In the current Windows sandbox, Vitest may fail earlier with the known `Cannot read directory "../../../../.."` startup error; record that if it happens.

- [ ] **Step 3: Implement helper**

Implement:

- `maskSecret(secret: unknown): string | undefined`
- `extractProviderSummary(provider: Provider, appId: AppId): ProviderSummary`
- JSON path extraction for Claude/Gemini/OpenClaw/OpenCode/Hermes.
- Codex TOML extraction using existing `extractCodexBaseUrl` and `extractCodexModelName`.

- [ ] **Step 4: Verify**

Run:

- `npm run typecheck`
- `npx vitest run tests/lib/provider-management/providerSummary.test.ts` if the environment allows it.

- [ ] **Step 5: Commit**

```bash
git add src/lib/provider-management/providerSummary.ts tests/lib/provider-management/providerSummary.test.ts
git commit -m "feat: add provider management summaries"
```

---

## Task 2: Grouping And Group Common Config Helpers

**Files:**
- Create: `src/lib/provider-management/providerGrouping.ts`
- Create: `src/lib/provider-management/providerGroupCommonConfig.ts`
- Test: `tests/lib/provider-management/providerGrouping.test.ts`
- Test: `tests/lib/provider-management/providerGroupCommonConfig.test.ts`
- Modify: `src/types.ts`
- Modify: `src-tauri/src/provider.rs`

- [ ] **Step 1: Write failing tests**

```ts
import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import { buildProviderGroups } from "@/lib/provider-management/providerGrouping";

const makeProvider = (id: string, group?: string): Provider => ({
  id,
  name: id,
  settingsConfig: {},
  meta: group ? { providerGroup: group } : undefined,
});

describe("providerGrouping", () => {
  it("groups providers by explicit meta providerGroup", () => {
    const groups = buildProviderGroups(
      [makeProvider("a", "Minimax"), makeProvider("b", "Minimax")],
      "claude",
    );

    expect(groups).toHaveLength(1);
    expect(groups[0].label).toBe("Minimax");
    expect(groups[0].providers.map((provider) => provider.id)).toEqual([
      "a",
      "b",
    ]);
  });
});
```

```ts
import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  applyGroupCommonConfig,
  getGroupCommonConfigCandidates,
} from "@/lib/provider-management/providerGroupCommonConfig";

describe("providerGroupCommonConfig", () => {
  it("applies group base URL and API key to a Claude provider without storing the secret in meta", () => {
    const source: Provider = {
      id: "source",
      name: "Source",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-source-secret",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet-a",
        },
      },
    };
    const target: Provider = {
      id: "target",
      name: "Target",
      settingsConfig: { env: {} },
    };

    const candidates = getGroupCommonConfigCandidates(source, "claude");
    const updated = applyGroupCommonConfig(target, source, "claude", [
      "baseUrl",
      "apiKey",
    ]);

    expect(candidates.baseUrl?.value).toBe("https://api.example.com");
    expect((updated.settingsConfig.env as any).ANTHROPIC_BASE_URL).toBe(
      "https://api.example.com",
    );
    expect((updated.settingsConfig.env as any).ANTHROPIC_AUTH_TOKEN).toBe(
      "sk-source-secret",
    );
    expect(JSON.stringify(updated.meta)).not.toContain("sk-source-secret");
  });
});
```

- [ ] **Step 2: Verify tests fail**

Run:

- `npx vitest run tests/lib/provider-management/providerGrouping.test.ts`
- `npx vitest run tests/lib/provider-management/providerGroupCommonConfig.test.ts`

- [ ] **Step 3: Implement helpers and metadata types**

Add TypeScript fields:

```ts
providerGroup?: string;
providerVariantLabel?: string;
groupCommonConfigEnabled?: Record<string, boolean>;
```

Add Rust `ProviderMeta` fields with serde names:

```rust
#[serde(rename = "providerGroup", skip_serializing_if = "Option::is_none")]
pub provider_group: Option<String>,
#[serde(rename = "providerVariantLabel", skip_serializing_if = "Option::is_none")]
pub provider_variant_label: Option<String>,
#[serde(rename = "groupCommonConfigEnabled", skip_serializing_if = "HashMap::is_empty", default)]
pub group_common_config_enabled: HashMap<String, bool>,
```

Implement explicit `meta.providerGroup` grouping first, then conservative fallback grouping by base URL host.

- [ ] **Step 4: Verify**

Run:

- `npm run typecheck`
- `cargo test provider_meta --lib`

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src-tauri/src/provider.rs src/lib/provider-management/providerGrouping.ts src/lib/provider-management/providerGroupCommonConfig.ts tests/lib/provider-management
git commit -m "feat: add provider grouping helpers"
```

---

## Task 3: Toolbar, Search, Compact Mode, And Selection

**Files:**
- Create: `src/components/providers/ProviderManagementToolbar.tsx`
- Create: `src/components/providers/ProviderCompactRow.tsx`
- Modify: `src/components/providers/ProviderList.tsx`
- Modify: `tests/components/ProviderList.test.tsx`

- [ ] **Step 1: Write failing component tests**

Extend `ProviderList.test.tsx` with tests that:

- Render the visible search input without pressing `Ctrl+F`.
- Search by provider id/model/base URL.
- Switch to compact mode and render compact rows.
- Select visible providers and show selected count.

- [ ] **Step 2: Verify tests fail**

Run: `npx vitest run tests/components/ProviderList.test.tsx`

- [ ] **Step 3: Implement UI state and components**

Add state in `ProviderList.tsx`:

```ts
const [searchTerm, setSearchTerm] = useState("");
const [viewMode, setViewMode] = useState<"cards" | "compact">("cards");
const [selectedProviderIds, setSelectedProviderIds] = useState<Set<string>>(
  () => new Set(),
);
```

Filter providers using `extractProviderSummary(provider, appId).searchText`.

Render `ProviderManagementToolbar` above results and use `ProviderCompactRow` when `viewMode === "compact"`.

- [ ] **Step 4: Verify**

Run:

- `npm run typecheck`
- `npx vitest run tests/components/ProviderList.test.tsx` if available.

- [ ] **Step 5: Commit**

```bash
git add src/components/providers/ProviderManagementToolbar.tsx src/components/providers/ProviderCompactRow.tsx src/components/providers/ProviderList.tsx tests/components/ProviderList.test.tsx
git commit -m "feat: add provider management toolbar"
```

---

## Task 4: Drawer And Group Common Config UI

**Files:**
- Create: `src/components/providers/ProviderConfigDrawer.tsx`
- Modify: `src/components/providers/ProviderList.tsx`
- Modify: `src/components/providers/ProviderCard.tsx`
- Modify: `tests/components/ProviderList.test.tsx`

- [ ] **Step 1: Write failing tests**

Extend `ProviderList.test.tsx` with tests that:

- Open a group drawer and see two sub-config rows.
- Verify safe API key fingerprints are rendered and raw keys are not rendered.
- Toggle `Use group API key` and assert `providersApi.update` receives a provider with updated `settingsConfig` and metadata flags.

- [ ] **Step 2: Verify tests fail**

Run: `npx vitest run tests/components/ProviderList.test.tsx`

- [ ] **Step 3: Implement drawer**

`ProviderConfigDrawer` receives:

```ts
providers: Provider[];
appId: AppId;
primaryProvider: Provider;
onEdit(provider): void;
onDuplicate(provider): void;
onDelete(provider): void;
onTest?(provider): void;
onApplyGroupCommonConfig(provider, keys): void;
```

Use `getGroupCommonConfigCandidates` and `applyGroupCommonConfig` through the ProviderList update mutation.

- [ ] **Step 4: Verify**

Run:

- `npm run typecheck`
- `npx vitest run tests/components/ProviderList.test.tsx` if available.

- [ ] **Step 5: Commit**

```bash
git add src/components/providers/ProviderConfigDrawer.tsx src/components/providers/ProviderList.tsx src/components/providers/ProviderCard.tsx tests/components/ProviderList.test.tsx
git commit -m "feat: add provider config drawers"
```

---

## Task 5: Batch Actions And Final Verification

**Files:**
- Modify: `src/components/providers/ProviderManagementToolbar.tsx`
- Modify: `src/components/providers/ProviderList.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`
- Modify: `src/i18n/locales/ja.json`
- Test: `tests/components/ProviderList.test.tsx`

- [ ] **Step 1: Write failing tests**

Extend `ProviderList.test.tsx` with tests that:

- Select two providers.
- Click batch delete.
- Confirm the destructive dialog.
- Assert `onDelete` was called for both selected providers.

- [ ] **Step 2: Verify tests fail**

Run: `npx vitest run tests/components/ProviderList.test.tsx`

- [ ] **Step 3: Implement batch actions**

Implement supported operations:

- Test selected.
- Delete selected with confirmation.
- Add selected to live config for additive apps through existing `onSwitch`.
- Remove selected from live config for additive apps through existing `onRemoveFromConfig` or delete fallback.
- Failover queue add/remove when failover mode is active.

Keep batch enable disabled for single-current apps.

- [ ] **Step 4: Final verification**

Run:

- `npm run typecheck`
- `cargo test provider_meta --lib`
- `cargo test --lib`
- `npm run test:unit` and record the known Vitest startup result if the Windows sandbox still blocks config loading.

- [ ] **Step 5: Commit**

```bash
git add src/components/providers src/lib/provider-management src/types.ts src-tauri/src/provider.rs src/i18n/locales tests
git commit -m "feat: improve provider management"
```

