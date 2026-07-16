/** Codex App 增强开关矩阵 */
export interface CodexEnhancementSettings {
  pluginUnlock: boolean;
  autoExpand: boolean;
  sessionDelete: boolean;
  wideConversation: boolean;
  nativeMenu: boolean;
  userScriptRuntime: boolean;
  markdownExport: boolean;
  modelSwitcher: boolean;
  systemPrompt: boolean;
  reasoningResume: boolean;
  reasoningToken: boolean;
}

/** Codex 工作台设置 */
export interface CodexWorkbenchSettings {
  enhancements: CodexEnhancementSettings;
  autoLaunch: boolean;
  autoStartProxy: boolean;
  scriptMarketUrl: string;
  radarTtlMinutes: number;
}

/** 工作台运行时状态 */
export interface CodexWorkbenchStatus {
  platformSupported: boolean;
  installState: string;
  runtimeState: string;
  cdpPort: number | null;
  bridgeState: string;
  currentProviderId: string | null;
  proxyRunning: boolean;
  lastError: string | null;
}

export const DEFAULT_CODEX_ENHANCEMENTS: CodexEnhancementSettings = {
  pluginUnlock: true,
  autoExpand: true,
  sessionDelete: true,
  wideConversation: true,
  nativeMenu: true,
  userScriptRuntime: true,
  markdownExport: false,
  modelSwitcher: false,
  systemPrompt: false,
  reasoningResume: false,
  reasoningToken: false,
};

export const DEFAULT_CODEX_WORKBENCH_SETTINGS: CodexWorkbenchSettings = {
  enhancements: DEFAULT_CODEX_ENHANCEMENTS,
  autoLaunch: true,
  autoStartProxy: true,
  scriptMarketUrl:
    "https://raw.githubusercontent.com/BigPizzaV3/CodexPlusPlusScriptMarket/main/index.json",
  radarTtlMinutes: 30,
};
