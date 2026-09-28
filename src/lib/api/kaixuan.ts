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
  /**
   * The local port actually being probed (`KAIXUAN_LOCAL_GATEWAY_PORT`, default
   * 8782); `null` for the public endpoint.
   *
   * Surfaced so the UI can show the real port instead of a hardcoded 8782 —
   * the port is resolved at runtime from an env var, so any literal baked into
   * the UI goes stale the moment the user overrides it.
   */
  localPort: number | null;
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
