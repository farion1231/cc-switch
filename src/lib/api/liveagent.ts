import { invoke } from "@tauri-apps/api/core";

export interface LiveAgentSettings {
  configPath: string;
  defaultProvider: string;
  defaultModel: string;
}
export interface LiveAgentStatus {
  installed: boolean;
  configured: boolean;
  configPath: string;
}
export const liveagentApi = {
  getStatus: () => invoke<LiveAgentStatus>("get_liveagent_status"),
  getSettings: () => invoke<LiveAgentSettings>("get_liveagent_settings"),
  setDefaultModel: (provider: string, model: string) =>
    invoke<void>("set_liveagent_default_model", { provider, model }),
};
