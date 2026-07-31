import { invoke } from "@tauri-apps/api/core";

export interface ClaudeScienceStatus {
  installed: boolean;
  running: boolean;
  pid?: number | null;
  port?: number | null;
  binaryPath?: string | null;
  proxyBaseUrl?: string | null;
  error?: string | null;
}

export interface ClaudeScienceLaunchResult {
  proxyBaseUrl: string;
  pid?: number | null;
  port?: number | null;
  binaryPath: string;
}

export const claudeScienceApi = {
  async getStatus(): Promise<ClaudeScienceStatus> {
    return invoke("get_claude_science_status");
  },

  async launchWithProxy(): Promise<ClaudeScienceLaunchResult> {
    return invoke("launch_claude_science_with_proxy");
  },

  async stop(): Promise<void> {
    return invoke("stop_claude_science");
  },
};
