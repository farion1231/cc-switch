import { invoke } from "@tauri-apps/api/core";
import type {
  CodexWorkbenchSettings,
  CodexWorkbenchStatus,
} from "@/types/codexWorkbench";

export const codexWorkbenchApi = {
  getStatus: () =>
    invoke<CodexWorkbenchStatus>("get_codex_workbench_status"),

  getSettings: () =>
    invoke<CodexWorkbenchSettings>("get_codex_workbench_settings"),

  updateSettings: (settings: CodexWorkbenchSettings) =>
    invoke<void>("update_codex_workbench_settings", { settings }),
};
