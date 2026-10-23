import { invoke } from "@tauri-apps/api/core";

/** State of the `cline-hub` shared by the Cline CLI and desktop app. */
export interface ClineHubStatus {
  running: boolean;
  /** Running, but started before custom providers (`models.json`) changed. */
  stale: boolean;
  pid?: number | null;
  startedAt?: string | null;
}

export const clineApi = {
  async getHubStatus(): Promise<ClineHubStatus> {
    return await invoke("get_cline_hub_status");
  },
};
