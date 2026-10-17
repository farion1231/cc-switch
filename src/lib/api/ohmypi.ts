import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";

export interface OhMyPiCurrentState {
  enabledProviderIds: string[];
  defaultProviderId: string | null;
}

export const ohmypiApi = {
  async getCurrentState(): Promise<OhMyPiCurrentState> {
    return await invoke("get_ohmypi_current_state");
  },

  async updateProviderUsageScript(
    id: string,
    usageScript: UsageScript,
  ): Promise<boolean> {
    return await invoke("update_ohmypi_provider_usage_script", {
      id,
      usageScript,
    });
  },
};
