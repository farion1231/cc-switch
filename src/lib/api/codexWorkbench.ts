import { invoke } from "@tauri-apps/api/core";
import type {
  CodexWorkbenchSettings,
  CodexWorkbenchStatus,
  MarketIndex,
  ScriptInstallRequest,
  UserScriptInfo,
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

  getScriptsDir: () => invoke<string>("get_codex_scripts_dir"),

  refreshMarket: () =>
    invoke<MarketIndex>("refresh_codex_script_market"),

  getMarketCache: () =>
    invoke<MarketIndex | null>("get_codex_script_market_cache"),

  installMarketScript: (request: ScriptInstallRequest) =>
    invoke<UserScriptInfo>("install_codex_market_script", { request }),

  reinjectAfterScriptChange: () =>
    invoke<CodexWorkbenchStatus>("reinject_after_script_change"),
};
