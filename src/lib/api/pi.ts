import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";
import type { AppId } from "./types";

/** Pi, RPI and Oh My Pi share provider-state and usage-script behavior. */
export type PiFamilyAppId = Extract<AppId, "pi" | "rpi" | "omp">;

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
    return await invoke(
      app === "rpi"
        ? "get_rpi_current_state"
        : app === "omp"
          ? "get_omp_current_state"
          : "get_pi_current_state",
    );
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

  async updateRpiProviderUsageScript(
    id: string,
    usageScript: UsageScript,
  ): Promise<boolean> {
    return await invoke("update_rpi_provider_usage_script", {
      id,
      usageScript,
    });
  },

  async getSessionDiscovery(): Promise<PiSessionDiscovery> {
    return await invoke("get_pi_session_discovery");
  },
};

export type { AppId };
