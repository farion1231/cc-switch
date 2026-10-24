import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";

export interface OmpCurrentState {
  enabledProviderIds: string[];
  defaultProviderId: string | null;
}

export const ompApi = {
  async getCurrentState(): Promise<OmpCurrentState> {
    return await invoke("get_omp_current_state");
  },

  async updateProviderUsageScript(
    id: string,
    usageScript: UsageScript,
  ): Promise<boolean> {
    return await invoke("update_omp_provider_usage_script", {
      id,
      usageScript,
    });
  },
};
