import { invoke } from "@tauri-apps/api/core";

export interface ClaudeNotifyStatus {
  port?: number | null;
  listening: boolean;
  hooksApplied: boolean;
}

export const claudeNotifyApi = {
  async applyHooks(): Promise<boolean> {
    return await invoke("apply_claude_notify_hook_config");
  },

  async clearHooks(): Promise<boolean> {
    return await invoke("clear_claude_notify_hook_config");
  },

  async getStatus(): Promise<ClaudeNotifyStatus> {
    return await invoke("get_claude_notify_status");
  },
};
