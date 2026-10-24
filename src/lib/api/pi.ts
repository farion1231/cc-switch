import { invoke } from "@tauri-apps/api/core";

export interface PiCurrentState {
  enabledProviderIds: string[];
  defaultProviderId: string | null;
}

export type PiSessionDiscovery =
  | {
      status: "available";
    }
  | {
      status: "requires_project_context";
      configuredPath: string;
    }
  | {
      status: "unavailable";
      reason: string;
    };

export const piApi = {
  async getCurrentState(): Promise<PiCurrentState> {
    return await invoke("get_pi_current_state");
  },

  async getSessionDiscovery(): Promise<PiSessionDiscovery> {
    return await invoke("get_pi_session_discovery");
  },
};
