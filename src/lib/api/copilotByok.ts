import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";
import type { StreamCheckResult } from "./connectivity-check";

export type CopilotByokApiType = "chat-completions" | "responses" | "messages";

export type CopilotByokReasoningEffortFormat = CopilotByokApiType;

export type CopilotByokEditTool =
  | "find-replace"
  | "multi-find-replace"
  | "apply-patch"
  | "code-rewrite";

export interface CopilotByokModel {
  id: string;
  modelId: string;
  name: string;
  enabled: boolean;
  toolCalling?: boolean | null;
  vision?: boolean | null;
  thinking?: boolean | null;
  streaming?: boolean | null;
  contextWindow?: number | null;
  maxInputTokens?: number | null;
  maxOutputTokens?: number | null;
  editTools: string[];
  zeroDataRetentionEnabled: boolean;
  supportsReasoningEffort: string[];
  reasoningEffortFormat: CopilotByokReasoningEffortFormat | null;
  modelOptions: unknown;
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
  category?: "official" | "custom" | null;
  usageScript?: UsageScript | null;
  enabled: boolean;
  requestHeaders: Record<string, string>;
  models: CopilotByokModel[];
  extra: Record<string, unknown>;
}

export type VsCodeEdition = "stable" | "insiders";

export interface CopilotByokTargetState {
  id: string;
  source: "detected" | "custom";
  edition: VsCodeEdition | null;
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

export interface CopilotCliState {
  supported: boolean;
  enabled: boolean;
  selectedGroupId: string | null;
  selectedModelId: string | null;
  selectedProviderName: string | null;
  selectedModelName: string | null;
  environmentMatches: boolean;
  environmentConflicts: string[];
  officialActivationRequiresConfirmation: boolean;
}

export interface CopilotByokState {
  groups: CopilotByokGroup[];
  targets: CopilotByokTargetState[];
  selectedTargetIds: string[];
  managedModelCount: number;
  cli: CopilotCliState;
}

export interface CopilotByokSyncResult {
  targetIds: string[];
  managedModelCount: number;
  changedTargetCount: number;
}

export interface CopilotByokImportResult {
  targetId: string;
  importedGroupCount: number;
  importedModelCount: number;
  reusedModelCount: number;
  skippedGroupCount: number;
  changedTargetCount: number;
  warnings: string[];
}

export const copilotByokApi = {
  getState(): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_get_state");
  },

  setCliSelection(groupId: string, modelId: string): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_set_cli_selection", {
      groupId,
      modelId,
    });
  },

  disableCli(): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_disable_cli");
  },

  setTargets(targetIds: string[]): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_set_targets", {
      targetIds,
    });
  },

  addCustomTarget(
    path: string,
    name?: string | null,
  ): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_add_custom_target", {
      path,
      name: name || null,
    });
  },

  removeCustomTarget(targetId: string): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_remove_custom_target", {
      targetId,
    });
  },

  upsertGroup(group: CopilotByokGroup): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_upsert_group", {
      group,
    });
  },

  deleteGroup(groupId: string): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_delete_group", {
      groupId,
    });
  },

  reorderGroups(groupIds: string[]): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_reorder_groups", {
      groupIds,
    });
  },

  importModels(targetId: string): Promise<CopilotByokImportResult> {
    return invoke<CopilotByokImportResult>("copilot_byok_import_models", {
      targetId,
    });
  },

  sync(): Promise<CopilotByokSyncResult> {
    return invoke<CopilotByokSyncResult>("copilot_byok_sync");
  },

  restoreBackup(targetId: string): Promise<boolean> {
    return invoke<boolean>("copilot_byok_restore_backup", { targetId });
  },

  checkConnection(groupId: string): Promise<StreamCheckResult> {
    return invoke<StreamCheckResult>("copilot_byok_check_connection", {
      groupId,
    });
  },

  updateUsageScript(
    groupId: string,
    usageScript: UsageScript,
  ): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_byok_update_usage_script", {
      groupId,
      usageScript,
    });
  },
};

/** Independent first-class GitHub Copilot CLI provider catalog and switcher. */
export const copilotCliApi = {
  getState(): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_get_state");
  },

  setSelection(
    groupId: string,
    groupName: string,
    confirmUnmanagedClear = false,
  ): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_set_selection", {
      groupId,
      groupName,
      confirmUnmanagedClear,
    });
  },

  disable(): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_disable");
  },

  upsertGroup(group: CopilotByokGroup): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_upsert_group", { group });
  },

  deleteGroup(groupId: string): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_delete_group", { groupId });
  },

  reorderGroups(groupIds: string[]): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_reorder_groups", { groupIds });
  },

  checkConnection(groupId: string): Promise<StreamCheckResult> {
    return invoke<StreamCheckResult>("copilot_cli_check_connection", {
      groupId,
    });
  },

  updateUsageScript(
    groupId: string,
    usageScript: UsageScript,
  ): Promise<CopilotByokState> {
    return invoke<CopilotByokState>("copilot_cli_update_usage_script", {
      groupId,
      usageScript,
    });
  },

  openTerminal(groupId: string, cwd: string): Promise<boolean> {
    return invoke<boolean>("copilot_cli_open_terminal", { groupId, cwd });
  },
};
