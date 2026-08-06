export interface CodexLiveEndpointConfig {
  createEndpoint: string;
  sidebandEndpoint: string;
}

function normalizeRelativeEndpoint(value: string): string {
  return value.trim().replace(/^\/+|\/+$/g, "");
}

export function isValidCodexLiveEndpoint(
  value: string,
  requiresCallId: boolean,
): boolean {
  const endpoint = normalizeRelativeEndpoint(value);
  if (
    !endpoint ||
    endpoint.includes("://") ||
    endpoint.includes("?") ||
    endpoint.includes("#") ||
    endpoint.includes("..") ||
    endpoint.includes("\\") ||
    /\s/.test(endpoint)
  ) {
    return false;
  }

  return !requiresCallId || endpoint.split("{call_id}").length - 1 === 1;
}

export function isValidCodexLiveConfig(
  config: CodexLiveEndpointConfig,
): boolean {
  return (
    isValidCodexLiveEndpoint(config.createEndpoint, false) &&
    isValidCodexLiveEndpoint(config.sidebandEndpoint, true)
  );
}
