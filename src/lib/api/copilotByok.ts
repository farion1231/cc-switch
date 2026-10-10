import { invoke } from "@tauri-apps/api/core";

export type CopilotByokApiType = "chat-completions" | "responses" | "messages";

export interface CopilotByokModel {
  id: string;
  modelId: string;
  name: string;
  enabled: boolean;
  toolCalling?: boolean | null;
  vision?: boolean | null;
  contextWindow?: number | null;
  maxOutputTokens?: number | null;
  extra: Record<string, unknown>;
}

export interface CopilotByokGroup {
  id: string;
  name: string;
  url: string;
  apiKey: string;
  apiType: CopilotByokApiType;
  websiteUrl?: string | null;
  notes?: string | null;
  icon?: string | null;
  iconColor?: string | null;
  enabled: boolean;
  requestHeaders: Record<string, string>;
  models: CopilotByokModel[];
  extra: Record<string, unknown>;
}

export interface CopilotByokTargetState {
  id: string;
  source: "detected" | "custom";
  edition: "stable" | "insiders" | null;
  editionName: string | null;
  profileId: string | null;
  profileName: string;
  isDefault: boolean;
  languageModelsPath: string;
  configExists: boolean;
  backupExists: boolean;
  selected: boolean;
  managedGroupCount: number;
  readError: string | null;
}

export interface CopilotByokState {
  groups: CopilotByokGroup[];
  targets: CopilotByokTargetState[];
  selectedTargetIds: string[];
  managedModelCount: number;
}

export interface CopilotByokSyncResult {
  targetIds: string[];
  managedModelCount: number;
  changedTargetCount: number;
}

export const copilotByokApi = {
  getState: () => invoke<CopilotByokState>("copilot_byok_get_state"),
  setTargets: (targetIds: string[]) =>
    invoke<CopilotByokState>("copilot_byok_set_targets", { targetIds }),
  addCustomTarget: (path: string, name?: string) =>
    invoke<CopilotByokState>("copilot_byok_add_custom_target", { path, name }),
  removeCustomTarget: (targetId: string) =>
    invoke<CopilotByokState>("copilot_byok_remove_custom_target", { targetId }),
  upsertGroup: (group: CopilotByokGroup) =>
    invoke<CopilotByokState>("copilot_byok_upsert_group", { group }),
  deleteGroup: (groupId: string) =>
    invoke<CopilotByokState>("copilot_byok_delete_group", { groupId }),
  sync: () => invoke<CopilotByokSyncResult>("copilot_byok_sync"),
  restoreBackup: (targetId: string) =>
    invoke<boolean>("copilot_byok_restore_backup", { targetId }),
};
