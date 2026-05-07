import { invoke } from "@tauri-apps/api/core";

// ─── Types ────────────────────────────────────────────────────

export type HealthStatus = "Healthy" | "NeedsInstall" | "NeedsRepair" | "PartiallyHealthy";

export type IssueSeverity = "Critical" | "High" | "Medium" | "Low";

export type IssueCategory =
  | "NotInstalled"
  | "EnvConflict"
  | "ConfigCorrupted"
  | "PermissionDenied"
  | "VersionOutdated"
  | "NodeJsMissing";

export interface FixAction {
  type: "InstallTool" | "InstallNodeJs" | "RemoveEnvVar" | "RepairConfig" | "FixPermission" | "UpdateTool";
  tool?: string;
  var_name?: string;
  source?: string;
  path?: string;
  current?: string;
  latest?: string;
}

export interface DiagnosisIssue {
  id: string;
  severity: IssueSeverity;
  category: IssueCategory;
  title: string;
  description: string;
  auto_fixable: boolean;
  fix_action?: FixAction;
}

export interface ToolStatus {
  installed: boolean;
  version?: string;
  latest_version?: string;
  issues: string[];
}

export interface DiagnosisResult {
  overall_status: HealthStatus;
  issues: DiagnosisIssue[];
  tools_status: Record<string, ToolStatus>;
}

export interface InstallResult {
  success: boolean;
  message: string;
  installed_version?: string;
  action?: "install" | "upgrade" | "none";
  already_installed?: boolean;
  verified?: boolean;
  error_code?: string;
}

export interface FixResult {
  fixed: string[];
  failed: Array<[string, string]>;
}

// ─── API ──────────────────────────────────────────────────────

export const doctorApi = {
  async diagnoseEnvironment(): Promise<DiagnosisResult> {
    return await invoke("diagnose_environment");
  },

  async installTool(tool: string): Promise<InstallResult> {
    return await invoke("install_tool", { tool });
  },

  async fixEnvironment(issues: DiagnosisIssue[]): Promise<FixResult> {
    return await invoke("fix_environment", { issues });
  },
};
