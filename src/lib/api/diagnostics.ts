import { invoke } from "@tauri-apps/api/core";

export type DiagnosticStatus = "ok" | "warning" | "error";

export interface DiagnosticCheck {
  id: string;
  label: string;
  status: DiagnosticStatus;
  message: string;
  suggestion?: string | null;
  details?: unknown;
}

export interface DiagnosticReport {
  generatedAt: string;
  checks: DiagnosticCheck[];
  summary: {
    ok: number;
    warnings: number;
    errors: number;
    degradedAgentGateway: boolean;
  };
}

export interface DiagnosticError {
  code: string;
  message: string;
  suggestion: string;
  details?: string | null;
}

export const diagnosticsApi = {
  async runAll(): Promise<DiagnosticReport> {
    return await invoke("diagnostics_run_all");
  },

  async checkDependencies(): Promise<DiagnosticCheck[]> {
    return await invoke("diagnostics_check_dependencies");
  },

  async checkPorts(): Promise<DiagnosticCheck[]> {
    return await invoke("diagnostics_check_ports");
  },

  async checkPermissions(): Promise<DiagnosticCheck[]> {
    return await invoke("diagnostics_check_permissions");
  },

  async exportReport(): Promise<string> {
    return await invoke("diagnostics_export_report");
  },
};
