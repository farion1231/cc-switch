import path from "node:path";
import { configDefaults, defineWorkspace } from "vitest/config";

const SRC_ALIAS = {
  "@": path.resolve(__dirname, "./src"),
};

/**
 * Pure-Node tests with no DOM/RTL dependency and no module/global/env
 * mutations. Safe to run in `node` without `setupFiles` and without per-file
 * worker isolation (`isolate: false`).
 *
 * Explicit paths ensure any new test file defaults safely into `jsdom` until
 * verified pure-Node, and `tests/config/tokenplusProviderPresets.test.ts`
 * (which uses `renderHook`) remains in `jsdom`.
 */
const NODE_UNISOLATED_INCLUDE = [
  // src/config
  "src/config/codexProviderPresets.test.ts",
  "src/config/codexProviderPresets.tokenPlanTextOnly.test.ts",
  "src/config/codexProviderPresets.zhipuFlashVision.test.ts",
  "src/config/codingPlanProviders.test.ts",
  "src/config/copilotProviderPresets.test.ts",
  "src/config/dshProviderPresets.test.ts",
  "src/config/grokBuildProviderPresets.test.ts",
  "src/config/jiekouProviderPresets.test.ts",
  "src/config/kimiProviderPresets.test.ts",
  "src/config/mcodeProviderPresets.test.ts",
  "src/config/ppioProviderPresets.test.ts",
  "src/config/workbuddyProviderPresets.test.ts",
  // src/lib & src/utils
  "src/lib/utils/base64.test.ts",
  "src/lib/version.test.ts",
  "src/utils/codexConfigUtils.test.ts",
  "src/utils/deeplinkRisk.test.ts",
  "src/utils/grokBuildConfig.test.ts",
  "src/utils/providerCapabilities.test.ts",
  "src/utils/providerIcon.test.ts",
  // tests/components (pure helper/logic tests)
  "tests/components/CodexFormFields.copilotCapabilities.test.ts",
  "tests/components/ModelsDevPickerDialog.test.ts",
  "tests/components/OmoFormFields.mergeCustomModelsIntoStore.test.ts",
  "tests/components/ProviderCardLayout.test.ts",
  "tests/components/ProviderForm.codexCatalog.test.ts",
  "tests/components/SelectContentScroll.test.ts",
  "tests/components/SelectItemIndicator.test.ts",
  "tests/components/UsageTrendChart.test.ts",
  "tests/components/codexReasoningLevelsLabel.test.ts",
  "tests/components/mcpDraft.test.ts",
  "tests/components/modelMetadataFill.test.ts",
  "tests/components/npmBinCollision.test.ts",
  "tests/components/openclaw.utils.test.ts",
  "tests/components/providerPresentation.test.ts",
  "tests/components/quotaRules.test.ts",
  "tests/components/requestLogColumns.test.ts",
  "tests/components/usageFormat.test.ts",
  "tests/components/usageProviderLabel.test.ts",
  "tests/components/workbuddyQuota.test.ts",
  // tests/components/sessions/reader
  "tests/components/sessions/reader/agentStyles.test.ts",
  "tests/components/sessions/reader/describeEvent.test.ts",
  "tests/components/sessions/reader/toolSummary.test.ts",
  "tests/components/sessions/reader/turns.test.ts",
  // tests/config (excluding tokenplusProviderPresets.test.ts which uses renderHook)
  "tests/config/appConfig.test.tsx",
  "tests/config/appSpecs.test.ts",
  "tests/config/backendConstants.test.ts",
  "tests/config/claudeDesktopProviderPresets.test.ts",
  "tests/config/claudePresetKeyFields.test.ts",
  "tests/config/claudeProviderPresets.test.ts",
  "tests/config/codexChatProviderPresets.test.ts",
  "tests/config/codexReasoningLevelPresets.test.ts",
  "tests/config/codexTemplates.test.ts",
  "tests/config/commandCodeProviderPresets.test.ts",
  "tests/config/commandCodeUsage.test.ts",
  "tests/config/doubaoSeedPresets.test.ts",
  "tests/config/genesisProviderPresets.test.ts",
  "tests/config/hermesBuiltinProviderIds.test.ts",
  "tests/config/hermesMinimaxProviderKey.test.ts",
  "tests/config/languages.test.ts",
  "tests/config/localeCoverage.test.ts",
  "tests/config/localeParity.test.ts",
  "tests/config/longcatProviderPresets.test.ts",
  "tests/config/mimoTokenPlanPresets.test.ts",
  "tests/config/mistralPiProviderPresets.test.ts",
  "tests/config/moarkProviderPresets.test.ts",
  "tests/config/ompProviderPresets.test.ts",
  "tests/config/openclawProviderPresets.test.ts",
  "tests/config/opencodeProviderPresets.test.ts",
  "tests/config/piIcon.test.ts",
  "tests/config/piProviderPresets.test.ts",
  "tests/config/piThinkingProfiles.test.ts",
  "tests/config/presetModelLists.test.ts",
  "tests/config/presetModelMetadata.test.ts",
  "tests/config/presetPreferenceKeys.test.ts",
  "tests/config/qianfanTokenPlanPresets.test.ts",
  "tests/config/readmeParity.test.ts",
  "tests/config/subrouterProviderPresets.test.ts",
  "tests/config/teamorouterClaudePresets.test.ts",
  "tests/config/therouterOpenCodeOpenClawPresets.test.ts",
  "tests/config/therouterProviderPresets.test.ts",
  "tests/config/tokenPlanProviderPresets.test.ts",
  "tests/config/tokenplusAdditionalApps.test.ts",
  "tests/config/tokenplusMinimalModels.test.ts",
  "tests/config/tools.test.ts",
  "tests/config/whatsNewEntries.test.ts",
  "tests/config/xaiOauthProviderPresets.test.ts",
  // tests/icons
  "tests/icons/themeAdapt.test.ts",
  // tests/lib
  "tests/lib/claudePlan.test.ts",
  "tests/lib/cn.test.ts",
  "tests/lib/keepLastGoodUsage.test.ts",
  "tests/lib/managedAccountUsage.test.ts",
  "tests/lib/modelMetadata.test.ts",
  "tests/lib/piPromptSlug.test.ts",
  "tests/lib/piPromptTemplate.test.ts",
  "tests/lib/registeredCommands.test.ts",
  "tests/lib/requestOverrides.test.ts",
  "tests/lib/sequentialBulkAction.test.ts",
  "tests/lib/skillErrorReason.test.ts",
  "tests/lib/userAgent.test.ts",
  "tests/lib/whatsNew.test.ts",
  // tests/tray-panel & tests/types
  "tests/tray-panel/menuAim.test.ts",
  "tests/types/sessionFixtures.test.ts",
  "tests/types/usage.test.ts",
  // tests/utils
  "tests/utils/claudeEditorOverlay.test.ts",
  "tests/utils/codexConfigUtils.baseUrl.test.ts",
  "tests/utils/codexConfigUtils.codex.test.ts",
  "tests/utils/customIcon.test.ts",
  "tests/utils/deepLinkConfigPreview.test.ts",
  "tests/utils/errorUtils.test.ts",
  "tests/utils/omoConfig.test.ts",
  "tests/utils/providerConfigUtils.bedrock.test.ts",
  "tests/utils/providerMetaUtils.test.ts",
  "tests/utils/routeSwitchNotification.test.ts",
  "tests/utils/textNormalization.test.ts",
  "tests/utils/usageDisplay.test.ts",
];

/**
 * Non-DOM tests that use `vi.mock`, `vi.stubGlobal`, fake timers, or
 * `process.env` mutations (e.g. `TZ`). Run in `node` without jsdom/MSW setup,
 * but keep per-file isolation (`isolate: true`).
 */
const NODE_ISOLATED_INCLUDE = [
  "src/lib/api/model-fetch.test.ts",
  "tests/components/sessionUtils.test.ts",
  "tests/lib/modelsDevAutoSync.test.ts",
  "tests/lib/sessionsApi.test.ts",
  "tests/lib/toast.test.ts",
  "tests/lib/usageRange.test.ts",
  "tests/utils/codexHistorySplitHint.test.ts",
  "tests/utils/deepClone.test.ts",
];

export default defineWorkspace([
  {
    resolve: {
      alias: SRC_ALIAS,
    },
    test: {
      name: "node-unisolated",
      include: NODE_UNISOLATED_INCLUDE,
      environment: "node",
      setupFiles: [],
      globals: true,
      isolate: false,
    },
  },
  {
    resolve: {
      alias: SRC_ALIAS,
    },
    test: {
      name: "node-isolated",
      include: NODE_ISOLATED_INCLUDE,
      environment: "node",
      setupFiles: [],
      globals: true,
      isolate: true,
    },
  },
  {
    extends: "./vitest.config.ts",
    test: {
      name: "jsdom",
      exclude: [
        ...configDefaults.exclude,
        ...NODE_UNISOLATED_INCLUDE,
        ...NODE_ISOLATED_INCLUDE,
      ],
      pool: "forks",
      isolate: true,
    },
  },
]);
