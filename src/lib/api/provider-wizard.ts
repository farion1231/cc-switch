import { invoke } from "@tauri-apps/api/core";

export type UpstreamProtocol =
  | "anthropic_messages"
  | "open_ai_chat"
  | "open_ai_responses";

export type AuthMode = "bearer" | "x_api_key";
export type UrlMode = "base" | "full_endpoint";
export type ProbeConfidence = "high" | "medium" | "low";

export interface ProviderProbeInput {
  baseUrl: string;
  apiKey: string;
  modelsUrl?: string;
  model?: string;
  allowInferenceProbe: boolean;
}

export interface DetectedModel {
  id: string;
  ownedBy?: string | null;
}

export interface ProtocolCapability {
  protocol: UpstreamProtocol;
  endpoint: string;
  authMode: AuthMode;
  supported: boolean;
  confidence: ProbeConfidence;
  evidence: string[];
}

export interface ProviderProbeResult {
  normalizedBaseUrl: string;
  urlMode: UrlMode;
  models: DetectedModel[];
  capabilities: ProtocolCapability[];
  recommendedModel?: string | null;
  warnings: string[];
}

export interface ProviderInstallSelection {
  name: string;
  baseUrl: string;
  apiKey: string;
  model: string;
  claudeProtocol?: UpstreamProtocol;
  codexProtocol?: UpstreamProtocol;
}

export interface AppInstallPreview {
  app: "claude" | "codex";
  providerId: string;
  protocol: UpstreamProtocol;
  mode: "direct" | "proxy";
  model: string;
  filesToChange: string[];
  restartRequired: boolean;
  redactedConfig: Record<string, unknown>;
  warnings: string[];
}

export interface ProviderInstallPreview {
  providerId: string;
  normalizedBaseUrl: string;
  urlMode: UrlMode;
  claude?: AppInstallPreview | null;
  codex?: AppInstallPreview | null;
  proxyWillStart: boolean;
  warnings: string[];
}

export interface ApplyProviderInstallResult {
  appliedApps: Array<"claude" | "codex">;
  rolledBack: boolean;
  rollbackErrors: string[];
  restartRequiredApps: Array<"claude" | "codex">;
}

export const providerWizardApi = {
  async probe(input: ProviderProbeInput): Promise<ProviderProbeResult> {
    return invoke("probe_provider_capabilities_command", { input });
  },

  async preview(
    selection: ProviderInstallSelection,
  ): Promise<ProviderInstallPreview> {
    return invoke("preview_provider_install_command", { selection });
  },

  async apply(
    selection: ProviderInstallSelection,
  ): Promise<ApplyProviderInstallResult> {
    return invoke("apply_provider_install_command", { selection });
  },
};
