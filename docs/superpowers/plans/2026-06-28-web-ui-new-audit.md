# web-ui to web-ui-new Port Audit

Date: 2026-06-28
Branch: web-ui-new (based on origin/main @ 61d7ac01fb9d0a3541f426c41dde7331049230a5)
Source: web-ui (tip tracked on origin)

## Branch State

- `web-ui-new`: clean, no local changes, tip at `61d7ac01` (origin/main)
- `web-ui`: 288 files changed relative to origin/main (103 A, 20 D, 165 M)
- **Critical finding:** 165 files are **Modified** on `web-ui` relative to `origin/main`. These are the real reconciliation work — they exist on both branches and have divergent content that must be manually reviewed and merged.

## Change Summary by Category

| Category | Count |
|----------|-------|
| Added (A) | 103 |
| Deleted (D) | 20 |
| Modified (M) | 165 |
| **Total** | **288** |

## Modified Files (M) — Risk Classification

> **Note:** File counts in risk tiers are approximate. Some files (e.g., `Cargo.lock`, `tauri.conf.json`, `.gitignore`) could reasonably be classified in multiple tiers. The totals are directional guides for prioritization, not exact accounting.

### High Risk — Core Rust (47 files)

These files contain backend business logic, proxy handlers, database schema, and Tauri commands. Any merge error here causes runtime failures or security issues.

- `src-tauri/Cargo.lock`
- `src-tauri/Cargo.toml`
- `src-tauri/src/app_config.rs`
- `src-tauri/src/claude_desktop_config.rs`
- `src-tauri/src/claude_mcp.rs`
- `src-tauri/src/codex_history_migration.rs`
- `src-tauri/src/commands/coding_plan.rs`
- `src-tauri/src/commands/provider.rs`
- `src-tauri/src/commands/settings.rs`
- `src-tauri/src/commands/workspace.rs`
- `src-tauri/src/config.rs`
- `src-tauri/src/database/dao/prompts.rs`
- `src-tauri/src/database/dao/providers.rs`
- `src-tauri/src/database/dao/usage_rollup.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/database/schema.rs`
- `src-tauri/src/database/tests.rs`
- `src-tauri/src/deeplink/prompt.rs`
- `src-tauri/src/deeplink/provider.rs`
- `src-tauri/src/deeplink/tests.rs`
- `src-tauri/src/hermes_config.rs`
- `src-tauri/src/init_status.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/openclaw_config.rs`
- `src-tauri/src/prompt.rs`
- `src-tauri/src/provider.rs`
- `src-tauri/src/proxy/forwarder.rs`
- `src-tauri/src/proxy/handlers.rs`
- `src-tauri/src/proxy/http_client.rs`
- `src-tauri/src/proxy/mod.rs`
- `src-tauri/src/proxy/providers/codex_oauth_auth.rs`
- `src-tauri/src/proxy/providers/copilot_auth.rs`
- `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- `src-tauri/src/proxy/response_processor.rs`
- `src-tauri/src/services/coding_plan.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/services/prompt.rs`
- `src-tauri/src/services/provider/live.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/provider/usage.rs`
- `src-tauri/src/services/proxy.rs`
- `src-tauri/src/services/skill.rs`
- `src-tauri/src/services/subscription.rs`
- `src-tauri/src/session_manager/providers/claude.rs`
- `src-tauri/src/tray.rs`

### High Risk — Frontend Core (82 files)

React components, hooks, API layer, type definitions, and new component additions. Merge errors cause UI bugs or type mismatches.

> **Note:** This tier lists 82 files total (75 Modified + 7 Added). The Modified subtotal is 75.

**Modified (M):**
- `src/App.tsx`
- `src/components/DeepLinkImportDialog.tsx`
- `src/components/SubscriptionQuotaFooter.tsx`
- `src/components/UsageScriptModal.tsx`
- `src/components/common/FullScreenPanel.tsx`
- `src/components/openclaw/AgentsDefaultsPanel.tsx`
- `src/components/openclaw/EnvPanel.tsx`
- `src/components/openclaw/ToolsPanel.tsx`
- `src/components/providers/AddProviderDialog.tsx`
- `src/components/providers/ProviderCard.tsx`
- `src/components/providers/ProviderList.tsx`
- `src/components/providers/forms/ClaudeDesktopProviderForm.tsx`
- `src/components/providers/forms/ClaudeFormFields.tsx`
- `src/components/providers/forms/CodexFormFields.tsx`
- `src/components/providers/forms/HermesFormFields.tsx`
- `src/components/providers/forms/OmoFormFields.tsx`
- `src/components/providers/forms/OpenClawFormFields.tsx`
- `src/components/providers/forms/ProviderForm.tsx`
- `src/components/providers/forms/ProviderPresetSelector.tsx`
- `src/components/providers/forms/shared/ApiKeySection.tsx`
- `src/components/settings/AboutSection.tsx`
- `src/components/settings/LogConfigPanel.tsx`
- `src/components/settings/RectifierConfigPanel.tsx`
- `src/components/settings/SettingsPage.tsx`
- `src/components/settings/SkillStorageLocationSettings.tsx`
- `src/components/skills/SkillsPage.tsx`
- `src/components/skills/UnifiedSkillsPanel.tsx`
- `src/components/theme-provider.tsx`
- `src/components/ui/select.tsx`
- `src/components/universal/UniversalProviderFormModal.tsx`
- `src/components/universal/UniversalProviderPanel.tsx`
- `src/components/usage/PricingConfigPanel.tsx`
- `src/components/usage/UsageDashboard.tsx`
- `src/components/usage/UsageDateRangePicker.tsx`
- `src/components/workspace/DailyMemoryPanel.tsx`
- `src/components/workspace/WorkspaceFileEditor.tsx`
- `src/components/workspace/WorkspaceFilesPanel.tsx`
- `src/config/claudeDesktopProviderPresets.ts`
- `src/config/claudeProviderPresets.ts`
- `src/config/codexProviderPresets.ts`
- `src/config/codingPlanProviders.ts`
- `src/config/geminiProviderPresets.ts`
- `src/config/hermesProviderPresets.ts`
- `src/config/openclawProviderPresets.ts`
- `src/config/opencodeProviderPresets.ts`
- `src/hooks/useDirectorySettings.ts`
- `src/hooks/useHermes.ts`
- `src/hooks/useMcp.ts`
- `src/hooks/useOpenClaw.ts`
- `src/hooks/useProviderActions.ts`
- `src/hooks/useProxyConfig.ts`
- `src/hooks/useProxyStatus.ts`
- `src/hooks/useSettingsForm.ts`
- `src/hooks/useSkills.ts`
- `src/hooks/useTauriEvent.ts`
- `src/index.html`
- `src/lib/api/auth.ts`
- `src/lib/api/copilot.ts`
- `src/lib/api/env.ts`
- `src/lib/api/index.ts`
- `src/lib/api/model-fetch.ts`
- `src/lib/api/omo.ts`
- `src/lib/api/proxy.ts`
- `src/lib/api/subscription.ts`
- `src/lib/api/usage.ts`
- `src/lib/api/workspace.ts`
- `src/lib/clipboard.ts`
- `src/lib/query/mutations.ts`
- `src/lib/query/proxy.ts`
- `src/lib/query/queries.ts`
- `src/lib/query/usage.ts`
- `src/lib/updater.ts`
- `src/lib/usageRange.ts`
- `src/main.tsx`
- `src/types.ts`
- `src/types/usage.ts`

**Added (A):**
- `src/components/auth/LoginPage.tsx`
- `src/components/common/ErrorBoundary.tsx`
- `src/components/settings/WebServerSettings.tsx`
- `src/components/terminal/Terminal.tsx`
- `src/components/terminal/TerminalModal.tsx`
- `src/components/terminal/index.ts`
- `src/hooks/useWebAuthSync.ts`

### Medium Risk — Build Config (8 files)

Package and build configuration changes. Must be carefully merged to preserve dependency resolution.

- `package.json`
- `pnpm-lock.yaml`
- `pnpm-workspace.yaml`
- `src-tauri/tauri.conf.json`
- `tsconfig.json`
- `vite.config.ts`
- `vitest.config.ts`

### Low Risk — Tests (13 files)

Test files. Can be reviewed after core code is merged.

- `src-tauri/tests/mcp_commands.rs`
- `src-tauri/tests/provider_service.rs`
- `src-tauri/tests/skill_sync.rs`
- `src-tauri/tests/support.rs`
- `tests/components/ClaudeDesktopProviderForm.test.tsx`
- `tests/components/ClaudeFormFields.test.tsx`
- `tests/components/ProviderPresetSelector.test.tsx`
- `tests/components/SkillsPageInstall.test.tsx`
- `tests/config/claudeProviderPresets.test.ts`
- `tests/config/codexChatProviderPresets.test.ts`
- `tests/hooks/useSettingsForm.test.tsx`
- `tests/integration/App.test.tsx`
- `tests/setupGlobals.ts`

### Low Risk — Docs/Assets/Metadata (22 files)

Documentation, READMEs, locale files, icon metadata, and GitHub metadata. Low risk of merge conflicts.

- `.github/workflows/release.yml`
- `.gitignore`
- `CHANGELOG.md`
- `README.md`
- `README_DE.md`
- `README_JA.md`
- `README_ZH.md`
- `docs/guides/codex-official-auth-preservation-guide-en.md`
- `docs/guides/codex-official-auth-preservation-guide-ja.md`
- `docs/guides/codex-official-auth-preservation-guide-zh.md`
- `docs/user-manual/en/1-getting-started/1.2-installation.md`
- `docs/user-manual/en/2-providers/2.5-usage-query.md`
- `docs/user-manual/ja/1-getting-started/1.2-installation.md`
- `docs/user-manual/ja/2-providers/2.5-usage-query.md`
- `docs/user-manual/zh/1-getting-started/1.2-installation.md`
- `docs/user-manual/zh/2-providers/2.5-usage-query.md`
- `src/i18n/locales/en.json`
- `src/i18n/locales/ja.json`
- `src/i18n/locales/zh-TW.json`
- `src/i18n/locales/zh.json`
- `src/icons/extracted/index.ts`
- `src/icons/extracted/metadata.ts`

---

### Recommended Merge Strategy for Modified Files

For each of the 165 Modified files, compare the `origin/main` and `web-ui` versions side-by-side:
- **Prefer `web-ui`** for web-UI-specific changes (runtime switch, web API layer, new components, web server routes).
- **Prefer `origin/main`** for docs and partner assets unless they conflict with web UI needs (e.g., removed banners that reference deprecated features).
- **Manually reconcile** all core Rust and TypeScript files — these contain the bulk of the business logic changes and must be reviewed line-by-line to avoid regressions.

---

## Deleted Files (D) — Must be Removed

These 20 files were deleted on `web-ui` and must also be deleted on `web-ui-new`:

- `assets/partners/banners/kimi-banner-en.png`
- `assets/partners/banners/kimi-banner-zh.png`
- `assets/partners/logos/etok.png`
- `docs/guides/codex-desktop-custom-model-visibility-en.md`
- `docs/guides/codex-desktop-custom-model-visibility-ja.md`
- `docs/guides/codex-desktop-custom-model-visibility-zh.md`
- `docs/release-notes/v3.16.4-en.md`
- `docs/release-notes/v3.16.4-ja.md`
- `docs/release-notes/v3.16.4-zh.md`
- `src-tauri/src/proxy/content_encoding.rs`
- `src/components/DatabaseUpgrade.tsx`
- `src/components/providers/forms/LocalProxyRequestOverridesField.tsx`
- `src/icons/extracted/etok.png`
- `src/icons/extracted/subrouter.svg`
- `src/lib/requestOverrides.ts`
- `tests/components/ProviderCardLayout.test.ts`
- `tests/components/SelectItemIndicator.test.ts`
- `tests/config/subrouterProviderPresets.test.ts`
- `tests/hooks/useUpdateProviderMutation.test.tsx`
- `tests/lib/requestOverrides.test.ts`

---

## Recent Fix Commits (7b465868..web-ui)

These 7 commits landed on `web-ui` after the divergence point and must be cherry-picked or re-applied during the port:

| Commit | Message | Files Touched |
|--------|---------|---------------|
| `b24a8038` | fix(web/skills): sync DB skillStorageLocation on early migration return | `src-tauri/src/services/skill.rs` |
| `4472d213` | fix(web/settings): sync skill storage location to DB settings after migration in Docker/web mode | `src-tauri/src/services/skill.rs`, `src-tauri/src/web/models/mod.rs`, `src-tauri/src/web/routes/settings.rs`, `src/components/settings/SettingsPage.tsx` |
| `0b344dd4` | fix(settings): invalidate settings query cache after config import | `src/App.tsx` |
| `aa78bf22` | fix(tests): repair Rust and frontend test suites | `src-tauri/src/database/dao/providers.rs`, `src-tauri/src/database/mod.rs`, `src-tauri/src/database/schema.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/proxy/mod.rs`, `src-tauri/src/services/skill.rs`, `src-tauri/tests/dao_mcp.rs`, `src-tauri/tests/dao_proxy.rs`, `src-tauri/tests/dao_settings.rs`, `src-tauri/tests/mcp_modules.rs`, `src-tauri/tests/proxy_modules.rs`, `src-tauri/tests/services_config.rs`, `src-tauri/tests/services_env_manager.rs`, `src-tauri/tests/services_provider_endpoints.rs`, `src-tauri/tests/skill_sync.rs`, `src/components/settings/SettingsPage.tsx`, `src/components/settings/SkillStorageLocationSettings.tsx`, `src/hooks/useSettingsForm.ts`, `tests/api/helpers.ts`, `tests/hooks/useSettingsForm.test.tsx`, `tests/integration/App.test.tsx`, `vitest.config.ts` |
| `abc0ec29` | fix(docker): replace corepack enable with npm install -g pnpm | `Dockerfile` |
| `efaf3dd7` | fix(settings): invalidate React Query cache after skill storage location migration | `src/components/settings/SettingsPage.tsx` |
| `69d9e300` | fix(workspace): use web-aware workspaceApi in openclaw files panel | `src/components/workspace/DailyMemoryPanel.tsx`, `src/components/workspace/WorkspaceFileEditor.tsx`, `src/components/workspace/WorkspaceFilesPanel.tsx` |

**Note:** `aa78bf22` is the largest fix commit (22 files). It repairs test suites and should be applied after the core backend is ported but before running tests.

---

## Subsystem Inventories (Two-Dot Diff)

### 00-build-packaging (20 files)

Build tooling, packaging, and configuration files.

**Added (A):**
- `.dockerignore`
- `.env.example`
- `Dockerfile`
- `dev.ubuntu.sh`
- `docker-compose.yml`
- `package-lock.json`
- `playwright.config.ts`
- `scripts/build-web.sh`
- `vitest.api.config.ts`

**Modified (M):**
- `.github/workflows/release.yml`
- `.gitignore`
- `package.json`
- `pnpm-lock.yaml`
- `pnpm-workspace.yaml`
- `src-tauri/Cargo.lock`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `tsconfig.json`
- `vite.config.ts`
- `vitest.config.ts`

---

### 01-backend-web-server (22 files)

Web server routes, handlers, middleware, and models for the web UI mode. All new files.

**Added (A):**
- `src-tauri/src/web/mod.rs`
- `src-tauri/src/web/handlers/mod.rs`
- `src-tauri/src/web/handlers/terminal.rs`
- `src-tauri/src/web/handlers/ws.rs`
- `src-tauri/src/web/middleware/auth.rs`
- `src-tauri/src/web/middleware/mod.rs`
- `src-tauri/src/web/models/app_state.rs`
- `src-tauri/src/web/models/mod.rs`
- `src-tauri/src/web/routes/auth.rs`
- `src-tauri/src/web/routes/hermes.rs`
- `src-tauri/src/web/routes/logs.rs`
- `src-tauri/src/web/routes/mcp.rs`
- `src-tauri/src/web/routes/mod.rs`
- `src-tauri/src/web/routes/openclaw.rs`
- `src-tauri/src/web/routes/prompts.rs`
- `src-tauri/src/web/routes/providers.rs`
- `src-tauri/src/web/routes/proxy.rs`
- `src-tauri/src/web/routes/sessions.rs`
- `src-tauri/src/web/routes/settings.rs`
- `src-tauri/src/web/routes/skills.rs`
- `src-tauri/src/web/routes/workspace.rs`
- `src-tauri/src/web_server.rs`

---

### 02-backend-shared-services (9 files)

Core business logic services. Mix of modified and added files.

**Added (A):**
- `src-tauri/src/services/workspace.rs`
- `src-tauri/src/testing.rs`
- `src-tauri/src/api_only.rs`

**Modified (M):**
- `src-tauri/src/services/coding_plan.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/services/prompt.rs`
- `src-tauri/src/services/proxy.rs`
- `src-tauri/src/services/skill.rs`
- `src-tauri/src/services/subscription.rs`

---

### 03-backend-commands (4 files)

Refactored Tauri commands.

**Modified (M):**
- `src-tauri/src/commands/coding_plan.rs`
- `src-tauri/src/commands/provider.rs`
- `src-tauri/src/commands/settings.rs`
- `src-tauri/src/commands/workspace.rs`

---

### 04-frontend-web-api (15 files)

Web-specific API client layer. All new files.

**Added (A):**
- `src/lib/api/web/auth.ts`
- `src/lib/api/web/config.ts`
- `src/lib/api/web/hermes.ts`
- `src/lib/api/web/index.ts`
- `src/lib/api/web/mcp.ts`
- `src/lib/api/web/openclaw.ts`
- `src/lib/api/web/prompts.ts`
- `src/lib/api/web/providers.ts`
- `src/lib/api/web/proxy.ts`
- `src/lib/api/web/sessions.ts`
- `src/lib/api/web/settings.ts`
- `src/lib/api/web/skills.ts`
- `src/lib/api/web/types.ts`
- `src/lib/api/web/workspace.test.ts`
- `src/lib/api/web/workspace.ts`

---

### 05-frontend-runtime-switch (24 files)

Runtime detection and unified API layer. Mostly modified existing files plus a few additions.

**Added (A):**
- `src/lib/api/web-client.ts`
- `src/lib/environment.ts`
- `src/lib/webLogger.ts`
- `src/hooks/useWebAuthSync.ts`

**Modified (M):**
- `src/lib/api/auth.ts`
- `src/lib/api/copilot.ts`
- `src/lib/api/env.ts`
- `src/lib/api/index.ts`
- `src/lib/api/model-fetch.ts`
- `src/lib/api/omo.ts`
- `src/lib/api/proxy.ts`
- `src/lib/api/subscription.ts`
- `src/lib/api/usage.ts`
- `src/lib/api/workspace.ts`
- `src/lib/clipboard.ts`
- `src/lib/query/mutations.ts`
- `src/lib/query/proxy.ts`
- `src/lib/query/queries.ts`
- `src/lib/query/usage.ts`
- `src/lib/updater.ts`
- `src/lib/usageRange.ts`
- `src/main.tsx`
- `src/types.ts`
- `src/types/usage.ts`

---

### 06-frontend-components (66 files)

React components, hooks, contexts, config, and types.

**Added (A):**
- `src/assets/fable5-verified.png`
- `src/components/auth/LoginPage.tsx`
- `src/components/common/ErrorBoundary.tsx`
- `src/components/settings/WebServerSettings.tsx`
- `src/components/terminal/Terminal.tsx`
- `src/components/terminal/TerminalModal.tsx`
- `src/components/terminal/index.ts`

**Modified (M):**
- `src/App.tsx`
- `src/components/DeepLinkImportDialog.tsx`
- `src/components/SubscriptionQuotaFooter.tsx`
- `src/components/UsageScriptModal.tsx`
- `src/components/common/FullScreenPanel.tsx`
- `src/components/openclaw/AgentsDefaultsPanel.tsx`
- `src/components/openclaw/EnvPanel.tsx`
- `src/components/openclaw/ToolsPanel.tsx`
- `src/components/providers/AddProviderDialog.tsx`
- `src/components/providers/ProviderCard.tsx`
- `src/components/providers/ProviderList.tsx`
- `src/components/providers/forms/ClaudeDesktopProviderForm.tsx`
- `src/components/providers/forms/ClaudeFormFields.tsx`
- `src/components/providers/forms/CodexFormFields.tsx`
- `src/components/providers/forms/HermesFormFields.tsx`
- `src/components/providers/forms/OmoFormFields.tsx`
- `src/components/providers/forms/OpenClawFormFields.tsx`
- `src/components/providers/forms/ProviderForm.tsx`
- `src/components/providers/forms/ProviderPresetSelector.tsx`
- `src/components/providers/forms/shared/ApiKeySection.tsx`
- `src/components/settings/AboutSection.tsx`
- `src/components/settings/LogConfigPanel.tsx`
- `src/components/settings/RectifierConfigPanel.tsx`
- `src/components/settings/SettingsPage.tsx`
- `src/components/settings/SkillStorageLocationSettings.tsx`
- `src/components/skills/SkillsPage.tsx`
- `src/components/skills/UnifiedSkillsPanel.tsx`
- `src/components/theme-provider.tsx`
- `src/components/ui/select.tsx`
- `src/components/universal/UniversalProviderFormModal.tsx`
- `src/components/universal/UniversalProviderPanel.tsx`
- `src/components/usage/PricingConfigPanel.tsx`
- `src/components/usage/UsageDashboard.tsx`
- `src/components/usage/UsageDateRangePicker.tsx`
- `src/components/workspace/DailyMemoryPanel.tsx`
- `src/components/workspace/WorkspaceFileEditor.tsx`
- `src/components/workspace/WorkspaceFilesPanel.tsx`
- `src/config/claudeDesktopProviderPresets.ts`
- `src/config/claudeProviderPresets.ts`
- `src/config/codexProviderPresets.ts`
- `src/config/codingPlanProviders.ts`
- `src/config/geminiProviderPresets.ts`
- `src/config/hermesProviderPresets.ts`
- `src/config/openclawProviderPresets.ts`
- `src/config/opencodeProviderPresets.ts`
- `src/hooks/useDirectorySettings.ts`
- `src/hooks/useHermes.ts`
- `src/hooks/useMcp.ts`
- `src/hooks/useOpenClaw.ts`
- `src/hooks/useProviderActions.ts`
- `src/hooks/useProxyConfig.ts`
- `src/hooks/useProxyStatus.ts`
- `src/hooks/useSettingsForm.ts`
- `src/hooks/useSkills.ts`
- `src/hooks/useTauriEvent.ts`
- `src/index.html`

**Deleted (D):**
- `src/components/DatabaseUpgrade.tsx`
- `src/components/providers/forms/LocalProxyRequestOverridesField.tsx`
- `src/lib/requestOverrides.ts`

---

### 07-tests (49 files)

Rust and TypeScript test additions and modifications.

**Added (A):**
- `src-tauri/tests/dao_mcp.rs`
- `src-tauri/tests/dao_providers.rs`
- `src-tauri/tests/dao_proxy.rs`
- `src-tauri/tests/dao_settings.rs`
- `src-tauri/tests/mcp_modules.rs`
- `src-tauri/tests/proxy_modules.rs`
- `src-tauri/tests/services_config.rs`
- `src-tauri/tests/services_env_manager.rs`
- `src-tauri/tests/services_provider_endpoints.rs`
- `src-tauri/tests/services_speedtest.rs`
- `tests/api/api.test.ts`
- `tests/api/helpers.ts`
- `tests/components/ErrorBoundary.test.tsx`
- `tests/components/WebServerSettings.test.tsx`
- `tests/components/auth/LoginPage.test.tsx`
- `tests/components/terminal/Terminal.test.tsx`
- `tests/components/terminal/TerminalModal.test.tsx`
- `tests/e2e/sessions.spec.ts`
- `tests/e2e/web-smoke.spec.ts`
- `tests/hooks/useWebAuthSync.test.tsx`
- `tests/integration/AppLogout.test.tsx`
- `tests/lib/apiRuntimeSelection.test.ts`
- `tests/lib/desktopSessionsApi.test.ts`
- `tests/lib/schemas.test.ts`
- `tests/lib/updater.test.ts`
- `tests/lib/utils.test.ts`
- `tests/lib/web-client.test.ts`
- `tests/lib/webAuthApi.test.ts`
- `tests/lib/webProxyApi.test.ts`
- `tests/lib/webSessionsApi.test.ts`
- `tests/utils/uuid.test.ts`

**Modified (M):**
- `src-tauri/tests/mcp_commands.rs`
- `src-tauri/tests/provider_service.rs`
- `src-tauri/tests/skill_sync.rs`
- `src-tauri/tests/support.rs`
- `tests/components/ClaudeDesktopProviderForm.test.tsx`
- `tests/components/ClaudeFormFields.test.tsx`
- `tests/components/ProviderPresetSelector.test.tsx`
- `tests/components/SkillsPageInstall.test.tsx`
- `tests/config/claudeProviderPresets.test.ts`
- `tests/config/codexChatProviderPresets.test.ts`
- `tests/hooks/useSettingsForm.test.tsx`
- `tests/integration/App.test.tsx`
- `tests/setupGlobals.ts`

**Deleted (D):**
- `tests/components/ProviderCardLayout.test.ts`
- `tests/components/SelectItemIndicator.test.ts`
- `tests/config/subrouterProviderPresets.test.ts`
- `tests/hooks/useUpdateProviderMutation.test.tsx`
- `tests/lib/requestOverrides.test.ts`

---

### 08-i18n-assets-docs (43 files)

Internationalization, partner assets, documentation, and GitHub metadata.

**Added (A):**
- `assets/partners/logos/ctok.png`
- `docs/superpowers/plans/2026-06-08-web-ui-testing.md`
- `docs/superpowers/plans/2026-06-12-fix-web-prompt-management.md`
- `docs/superpowers/plans/2026-06-13-desktop-logo-login-expiry.md`
- `docs/superpowers/plans/2026-06-16-auth-token-redesign.md`
- `docs/superpowers/plans/2026-06-16-logout-button-token-reveal.md`
- `docs/superpowers/plans/2026-06-17-fix-web-openclaw-workspace.md`
- `docs/superpowers/specs/2026-06-08-web-ui-testing-design.md`
- `docs/superpowers/specs/2026-06-13-desktop-logo-login-expiry-design.md`
- `docs/superpowers/specs/2026-06-16-auth-token-redesign-design.md`
- `docs/superpowers/specs/2026-06-16-logout-button-token-reveal-design.md`
- `src/icons/extracted/ctok.svg`

**Modified (M):**
- `CHANGELOG.md`
- `README.md`
- `README_DE.md`
- `README_JA.md`
- `README_ZH.md`
- `docs/guides/codex-official-auth-preservation-guide-en.md`
- `docs/guides/codex-official-auth-preservation-guide-ja.md`
- `docs/guides/codex-official-auth-preservation-guide-zh.md`
- `docs/user-manual/en/1-getting-started/1.2-installation.md`
- `docs/user-manual/en/2-providers/2.5-usage-query.md`
- `docs/user-manual/ja/1-getting-started/1.2-installation.md`
- `docs/user-manual/ja/2-providers/2.5-usage-query.md`
- `docs/user-manual/zh/1-getting-started/1.2-installation.md`
- `docs/user-manual/zh/2-providers/2.5-usage-query.md`
- `src/i18n/locales/en.json`
- `src/i18n/locales/ja.json`
- `src/i18n/locales/zh-TW.json`
- `src/i18n/locales/zh.json`
- `src/icons/extracted/index.ts`
- `src/icons/extracted/metadata.ts`

**Deleted (D):**
- `assets/partners/banners/kimi-banner-en.png`
- `assets/partners/banners/kimi-banner-zh.png`
- `assets/partners/logos/etok.png`
- `docs/guides/codex-desktop-custom-model-visibility-en.md`
- `docs/guides/codex-desktop-custom-model-visibility-ja.md`
- `docs/guides/codex-desktop-custom-model-visibility-zh.md`
- `docs/release-notes/v3.16.4-en.md`
- `docs/release-notes/v3.16.4-ja.md`
- `docs/release-notes/v3.16.4-zh.md`
- `src/icons/extracted/etok.png`
- `src/icons/extracted/subrouter.svg`

---

### 09-backend-core (36 files)

Core Rust backend — proxy, database, deeplink, session manager, MCP, config, and misc modules. Excludes web server (01) and shared services (02) which are catalogued separately.

**Modified (M):**
- `src-tauri/src/app_config.rs`
- `src-tauri/src/claude_desktop_config.rs`
- `src-tauri/src/claude_mcp.rs`
- `src-tauri/src/codex_history_migration.rs`
- `src-tauri/src/config.rs`
- `src-tauri/src/database/dao/prompts.rs`
- `src-tauri/src/database/dao/providers.rs`
- `src-tauri/src/database/dao/usage_rollup.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/database/schema.rs`
- `src-tauri/src/database/tests.rs`
- `src-tauri/src/deeplink/prompt.rs`
- `src-tauri/src/deeplink/provider.rs`
- `src-tauri/src/deeplink/tests.rs`
- `src-tauri/src/hermes_config.rs`
- `src-tauri/src/init_status.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/openclaw_config.rs`
- `src-tauri/src/prompt.rs`
- `src-tauri/src/provider.rs`
- `src-tauri/src/proxy/forwarder.rs`
- `src-tauri/src/proxy/handlers.rs`
- `src-tauri/src/proxy/http_client.rs`
- `src-tauri/src/proxy/mod.rs`
- `src-tauri/src/proxy/providers/codex_oauth_auth.rs`
- `src-tauri/src/proxy/providers/copilot_auth.rs`
- `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- `src-tauri/src/proxy/response_processor.rs`
- `src-tauri/src/services/provider/live.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/provider/usage.rs`
- `src-tauri/src/session_manager/providers/claude.rs`
- `src-tauri/src/tray.rs`

**Deleted (D):**
- `src-tauri/src/proxy/content_encoding.rs`

---

## Dependency Map (Based on Two-Dot Diff)

```
00-build-packaging
  ├── 01-backend-web-server (needs Cargo.toml, tauri.conf.json)
  ├── 02-backend-shared-services (needs Cargo.toml)
  ├── 03-backend-commands (needs 02, 01)
  └── 06-frontend-components (needs package.json, vite.config.ts, tsconfig.json)

01-backend-web-server
  └── 03-backend-commands (commands call web routes)

02-backend-shared-services
  ├── 01-backend-web-server (web routes use services)
  └── 03-backend-commands (commands use services)

03-backend-commands
  └── 07-tests (Rust tests exercise commands)

04-frontend-web-api
  └── 05-frontend-runtime-switch (runtime switch calls web API)

05-frontend-runtime-switch
  ├── 04-frontend-web-api (web mode)
  └── 06-frontend-components (components use unified API)

06-frontend-components
  └── 07-tests (TS tests exercise components)

> **Note:** The runtime switch layer (05) is logically below components (06). Components consume the runtime switch; there is no reverse dependency.

07-tests
  └── (depends on all above, but can be ported after)

08-i18n-assets-docs
  └── 06-frontend-components (i18n strings used by components)

09-backend-core
  └── 01-backend-web-server (web server is part of core backend)
```

**Port order recommendation:**
1. `00-build-packaging` — establish build infrastructure (highest risk: `package.json`, `Cargo.toml`)
2. `09-backend-core` + `02-backend-shared-services` — core backend (165 M files need manual merge)
3. `01-backend-web-server` — web server routes (all new, low risk)
4. `03-backend-commands` — command layer (4 M files)
5. `05-frontend-runtime-switch` + `04-frontend-web-api` — API abstraction
6. `06-frontend-components` — UI layer (82 M files + 7 A files)
7. `08-i18n-assets-docs` — assets and docs (low risk)
8. `07-tests` — test suite (apply `aa78bf22` fixes here)

---

## High-Risk Overlap List

**Result: 165 MODIFIED FILES**

Unlike the three-dot diff which showed zero overlap, the two-dot diff reveals that **165 files exist on both `origin/main` and `web-ui` with divergent content**. These are the files that require manual review and merge decisions during the port.

### Critical Merge Areas (by file count)

| Area | Modified Count | Risk |
|------|-------------|------|
| Frontend components/hooks/config | 73 | HIGH — UI behavior changes |
| Core Rust backend | 48 | HIGH — runtime logic changes |
| Build config | 8 | MEDIUM — dependency resolution |
| Tests | 13 | LOW — can be fixed after core merge |
| Docs/Assets | 17 | LOW — mostly additive changes |

### Highest Individual Risk Files

These files have the most complex merge history and are touched by multiple recent fix commits:

| File | Risk | Reason |
|------|------|--------|
| `src-tauri/src/services/skill.rs` | CRITICAL | Touched by 3 fix commits (`b24a8038`, `4472d213`, `aa78bf22`) |
| `src/components/settings/SettingsPage.tsx` | CRITICAL | Touched by 3 fix commits (`4472d213`, `aa78bf22`, `efaf3dd7`) |
| `src-tauri/src/lib.rs` | HIGH | Core module registration, touched by `aa78bf22` |
| `src-tauri/src/database/mod.rs` | HIGH | Database init, touched by `aa78bf22` |
| `src-tauri/src/proxy/mod.rs` | HIGH | Proxy module, touched by `aa78bf22` |
| `src/App.tsx` | HIGH | Root component, touched by `0b344dd4` |
| `src-tauri/src/database/schema.rs` | HIGH | Schema changes, touched by `aa78bf22` |
| `src-tauri/src/database/dao/providers.rs` | HIGH | DAO changes, touched by `aa78bf22` |
| `src/hooks/useSettingsForm.ts` | MEDIUM | Touched by `aa78bf22` |
| `tests/integration/App.test.tsx` | MEDIUM | Touched by `aa78bf22` |

---

## Trivial Bulk-Port List

These files can be copied almost verbatim with minimal or no review:

1. **New web server files** (`src-tauri/src/web/` — 22 files) — all Added, no merge needed
2. **New web API files** (`src/lib/api/web/` — 17 files) — all Added, no merge needed
3. **New test files** (`tests/` Added — 20 files) — all Added, no merge needed
4. **New Rust test files** (`src-tauri/tests/` Added — 10 files) — all Added, no merge needed
5. **New docs/plans/specs** (`docs/superpowers/` — 11 files) — all Added, no merge needed
6. **New terminal components** (`src/components/terminal/` — 3 files) — all Added, no merge needed
7. **New assets** (`src/assets/fable5-verified.png`, `src/icons/extracted/ctok.svg`) — static assets
8. **New build scripts** (`scripts/build-web.sh`, `dev.ubuntu.sh`) — utility scripts

**Total trivial files: ~90+ (about 31% of the port)**

---

## Risk Assessment Summary

| Risk Level | Count | Description |
|------------|-------|-------------|
| **Low** | ~110 | New files (A), trivial assets, docs — can be bulk-copied |
| **Medium** | ~21 | Build config, tests, docs modifications |
| **High** | ~121 | Core Rust (48) + Frontend (75 Modified) modifications |
| **Critical** | 2 | Files touched by 3+ fix commits (`skill.rs`, `SettingsPage.tsx`) |

**Overall assessment:** This is a **medium-to-high risk port** because of 165 Modified files requiring manual reconciliation. The main risks are:
1. **165 modified files** need careful three-way merge review against `origin/main`
2. **7 recent fix commits** (`b24a8038` through `69d9e300`) must be re-applied or cherry-picked
3. `src-tauri/src/services/skill.rs` and `src/components/settings/SettingsPage.tsx` are the highest-risk files (3 fix commits each)
4. Build tooling changes (`package.json`, `Cargo.toml`, `vite.config.ts`) may require iterative fixes
5. 20 deleted files must be properly removed to avoid stale code

---

## Verification Checklist

After each subsystem is ported, run the following before proceeding to the next:

- [ ] **Rust backend**: `cargo check` and `cargo test` pass in `src-tauri/`
- [ ] **Frontend types**: `pnpm install` and `pnpm tsc --noEmit` pass
- [ ] **Unit tests**: `pnpm test:unit` passes
- [ ] **Web server smoke test**: headless check that the web server starts and serves the UI
