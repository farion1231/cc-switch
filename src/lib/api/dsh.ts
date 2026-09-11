import { invoke } from "@tauri-apps/api/core";

export interface DshCurrentState {
  providerIds: string[];
  currentProviderId: string | null;
  currentModel: string | null;
}

export const dshApi = {
  async getCurrentState(): Promise<DshCurrentState> {
    return await invoke("get_dsh_current_state");
  },

  async setCurrentModel(
    providerId: string,
    modelId: string,
  ): Promise<boolean> {
    return await invoke("set_dsh_current_model", { providerId, modelId });
  },
};
