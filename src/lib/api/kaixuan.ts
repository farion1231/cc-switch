import { invoke } from "@tauri-apps/api/core";

// ============================================================================
// kaixuan bundle API（Tauri bindings）
// ============================================================================

export interface BundleSpecView {
  id: string;
  displayName: string;
  appType: string;
  endpointCount: number;
}

export interface InstallBundleRequest {
  bundleId: string;
  appType: string;
  /** key = endpoint role (e.g. "primary"), value = API key string. */
  apiKeys?: Record<string, string>;
}

export interface InstallBundleResult {
  bundleId: string;
  appType: string;
  installedProviderIds: string[];
  primaryProviderId: string;
  autoFailoverEnabled: boolean;
  missingEnvVars: string[];
}

// ============================================================================
// gateway health API
// ============================================================================

export interface GatewayEndpointMeta {
  id: string;
  label: string;
  url: string;
  role: "primary" | "secondary" | string;
}

export interface GatewayHealth {
  endpointId: string;
  url: string;
  reachable: boolean;
  httpStatus: number | null;
  latencyMs: number | null;
  error: string | null;
  probedAtMs: number;
}

export interface StartLocalGatewayResult {
  success: boolean;
  command: string;
  stdout: string;
  stderr: string;
  exitCode: number | null;
  durationMs: number;
}

export const kaixuanApi = {
  // ---- bundle ----
  listBundles: () => invoke<BundleSpecView[]>("list_provider_bundles"),
  installBundle: (request: InstallBundleRequest) =>
    invoke<InstallBundleResult>("install_provider_bundle", { request }),

  // ---- gateway health ----
  listGatewayEndpoints: () =>
    invoke<GatewayEndpointMeta[]>("list_gateway_endpoints"),
  probeGateway: (endpointId: string, url?: string) =>
    invoke<GatewayHealth>("probe_gateway", { endpointId, url }),
  probeAllGateways: () => invoke<GatewayHealth[]>("probe_all_gateways"),
  startLocalGateway: () =>
    invoke<StartLocalGatewayResult>("start_local_gateway"),
};