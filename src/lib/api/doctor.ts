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
  /**
   * RemoveEnvVar 专用：诊断阶段从 EnvConflict 透传过来的真实值。
   * Windows 上注册表与进程环境可能不同步，这里保留真实值用于备份回滚。
   */
  var_value?: string;
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
