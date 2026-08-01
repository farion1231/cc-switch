import { invoke } from "@tauri-apps/api/core";
import type { CodexSubagentSettingsView } from "@/types";

export const codexSubagentsApi = {
  getSettings(): Promise<CodexSubagentSettingsView> {
    return invoke("get_codex_subagent_settings");
  },

  saveSettings(
    model: string | null,
    reasoningEffort: string | null,
  ): Promise<CodexSubagentSettingsView> {
    return invoke("save_codex_subagent_settings", {
      model,
      reasoningEffort,
    });
  },
};
