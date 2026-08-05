import { invoke } from "@tauri-apps/api/core";

export type PiCurrentOwnership =
  | "managed"
  | "pi_native"
  | "external"
  | "unconfigured";
export interface PiCurrentState {
  providerKey?: string;
  modelId?: string;
  managedProviderId?: string;
  ownership: PiCurrentOwnership;
  enabledProviderIds: string[];
  driftedProviderIds: string[];
}

export type PiSessionDiscovery =
  | {
      status: "available";
      root: string;
      source: "environment" | "settings" | "default";
    }
  | {
      status: "requires_project_context";
      configuredPath: string;
      source: "environment" | "settings";
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
