import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";
import type { AppId } from "./types";

/** Pi and Oh My Pi share the provider form, membership state and usage-script command. */
export type PiFamilyAppId = Extract<AppId, "pi" | "omp">;

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
  async getCurrentState(app: PiFamilyAppId = "pi"): Promise<PiCurrentState> {
    return await invoke(`get_${app}_current_state`);
  },

  async updateProviderUsageScript(
    id: string,
    usageScript: UsageScript,
    app: PiFamilyAppId = "pi",
  ): Promise<boolean> {
    return await invoke("update_pi_provider_usage_script", {
      app,
      id,
      usageScript,
    });
  },

  async getSessionDiscovery(): Promise<PiSessionDiscovery> {
    return await invoke("get_pi_session_discovery");
  },
};
