import { invoke } from "@tauri-apps/api/core";
import type {
  CodexWorkbenchSettings,
  CodexWorkbenchStatus,
  MarketIndex,
  ScriptInstallRequest,
  UserScriptInfo,
  MarketplaceResult,
  PluginCacheInfo,
  RadarResult,
} from "@/types/codexWorkbench";

export interface LaunchEnhancedCodexResult {
  state: string;
  pid: number | null;
  cdpPort: number | null;
  bridgePort: number | null;
  instanceId: string | null;
  message: string | null;
}

export const codexWorkbenchApi = {
  getStatus: () => invoke<CodexWorkbenchStatus>("get_codex_workbench_status"),

  getSettings: () =>
    invoke<CodexWorkbenchSettings>("get_codex_workbench_settings"),

  updateSettings: (settings: CodexWorkbenchSettings) =>
    invoke<void>("update_codex_workbench_settings", { settings }),

  launchEnhanced: () =>
    invoke<LaunchEnhancedCodexResult>("launch_enhanced_codex"),

  reinject: () =>
    invoke<CodexWorkbenchStatus>("reinject_codex_enhancements"),

  listScripts: () => invoke<UserScriptInfo[]>("list_codex_user_scripts"),

  setScriptEnabled: (key: string, enabled: boolean) =>
    invoke<void>("set_codex_user_script_enabled", { key, enabled }),

  deleteScript: (key: string) =>
    invoke<void>("delete_codex_user_script", { key }),

  importScript: (sourcePath: string, key?: string) =>
    invoke<UserScriptInfo>("import_codex_user_script", {
      sourcePath,
      key: key ?? null,
    }),

  openScriptsDir: () => invoke<void>("open_codex_scripts_dir"),

  refreshMarket: () =>
    invoke<MarketIndex>("refresh_codex_script_market"),

  getMarketCache: () =>
    invoke<MarketIndex | null>("get_codex_script_market_cache"),

  installMarketScript: (request: ScriptInstallRequest) =>
    invoke<UserScriptInfo>("install_codex_market_script", { request }),

  reinjectAfterScriptChange: () =>
    invoke<CodexWorkbenchStatus>("reinject_after_script_change"),

  getEffectiveHome: () => invoke<string>("get_codex_effective_home"),

  getPluginMarketplaceStatus: () =>
    invoke<MarketplaceResult>("get_codex_plugin_marketplace_status"),

  initializePluginMarketplace: () =>
    invoke<MarketplaceResult>("initialize_codex_plugin_marketplace"),

  listPluginCaches: () =>
    invoke<PluginCacheInfo[]>("list_codex_plugin_caches"),

  refreshPluginCache: (pluginId: string) =>
    invoke<PluginCacheInfo>("refresh_codex_plugin_cache", { pluginId }),

  getRadar: (refresh?: boolean) =>
    invoke<RadarResult>("get_codex_radar", { refresh: refresh ?? false }),
};
