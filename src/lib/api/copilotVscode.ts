import { invoke } from "@tauri-apps/api/core";

export interface CopilotVscodeSyncStatus {
  pending: boolean;
  error: string | null;
}

export const copilotVscodeStatusKey = ["copilot-vscode-sync-status"] as const;

export const copilotVscodeApi = {
  targets: () => invoke<string[]>("get_copilot_vscode_targets"),
  sync: () => invoke<void>("sync_copilot_vscode"),
  status: () =>
    invoke<CopilotVscodeSyncStatus>("get_copilot_vscode_sync_status"),
};
