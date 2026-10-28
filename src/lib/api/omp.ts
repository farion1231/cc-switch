import { invoke } from "@tauri-apps/api/core";

export type OmpThinkingLevel =
  | "off"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "auto";
export const OMP_THINKING_LEVELS: OmpThinkingLevel[] = [
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "auto",
];
export type OmpModelParameters = Partial<{
  name: string;
  contextWindow: number;
  maxContextWindow: number;
  maxTokens: number;
  input: ("text" | "image")[];
  reasoning: boolean;
}>;
export interface OmpCatalogModel {
  provider: string;
  id: string;
  kind: string | null;
  name: string;
  contextWindow: number | null;
  maxContextWindow: number | null;
  maxTokens: number | null;
  input: ("text" | "image")[];
  reasoning: boolean;
  thinking: {
    mode:
      | "effort"
      | "budget"
      | "google-level"
      | "anthropic-adaptive"
      | "anthropic-budget-effort";
    efforts: Exclude<OmpThinkingLevel, "off" | "auto">[];
    defaultLevel?: Exclude<OmpThinkingLevel, "off" | "auto">;
    requiresEffort?: boolean;
  } | null;
  source: "nativeCache" | "configured";
  cacheUpdatedAt: number | null;
}
export interface OmpModelManagementState {
  scope: "global";
  settingsPath: string;
  modelsPath: string;
  settingsRevision: string;
  modelsRevision: string;
  defaultModel: string | null;
  defaultThinkingLevel: Exclude<OmpThinkingLevel, "off"> | null;
  modelRoles: Record<string, string | string[]>;
  roleNames: string[];
  agentModelOverrides: Record<string, string | string[]>;
  modelOverrides: {
    provider: string;
    modelId: string;
    parameters: OmpModelParameters;
  }[];
  catalogStatus: "ready" | "unavailable" | "unsupported" | "error";
  catalogMessage: string | null;
  models: OmpCatalogModel[];
}
export interface OmpSettingsEntryPatch {
  section: "modelRoles" | "agentModelOverrides" | "defaultThinkingLevel";
  key: string;
  expected: string | string[] | null;
  value: string | string[] | null;
}
export interface OmpModelParameterPatch {
  field: keyof OmpModelParameters;
  expected: unknown | null;
  value: unknown | null;
}
export const ompApi = {
  getState: () =>
    invoke<OmpModelManagementState>("get_omp_model_management_state"),
  patchSettings: (patch: {
    expectedRevision: string;
    expectedPath: string;
    changes: OmpSettingsEntryPatch[];
  }) => invoke<OmpModelManagementState>("patch_omp_model_settings", { patch }),
  patchOverride: (patch: {
    expectedRevision: string;
    expectedPath: string;
    provider: string;
    modelId: string;
    changes: OmpModelParameterPatch[];
  }) => invoke<OmpModelManagementState>("patch_omp_model_override", { patch }),
};
