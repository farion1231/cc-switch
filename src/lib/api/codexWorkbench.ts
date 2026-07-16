import { invoke } from "@tauri-apps/api/core";
import type {
  CodexWorkbenchSettings,
  CodexWorkbenchStatus,
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
  getStatus: () =>
    invoke<CodexWorkbenchStatus>("get_codex_workbench_status"),

  getSettings: () =>
    invoke<CodexWorkbenchSettings>("get_codex_workbench_settings"),

  updateSettings: (settings: CodexWorkbenchSettings) =>
    invoke<void>("update_codex_workbench_settings", { settings }),

  launchEnhanced: () =>
    invoke<LaunchEnhancedCodexResult>("launch_enhanced_codex"),

  reinject: () =>
    invoke<CodexWorkbenchStatus>("reinject_codex_enhancements"),
};
