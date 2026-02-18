export * from "./slots";
export * from "./endpoints";

import { ENDPOINTS } from "./endpoints";
import {
  APP_SLOTS,
  fitsSlot,
  deriveApps,
  isUniversal,
  ProviderEndpoint,
} from "./slots";

export const FLOW: Record<string, ProviderEndpoint[]> = {
  claude: ENDPOINTS.filter((e) => fitsSlot(e, APP_SLOTS.claude)),
  codex: ENDPOINTS.filter((e) => fitsSlot(e, APP_SLOTS.codex)),
  gemini: ENDPOINTS.filter((e) => fitsSlot(e, APP_SLOTS.gemini)),
  opencode: ENDPOINTS.filter((e) => fitsSlot(e, APP_SLOTS.opencode)),
  openclaw: ENDPOINTS.filter((e) => fitsSlot(e, APP_SLOTS.openclaw)),
};

export const UNIVERSAL = ENDPOINTS.filter(isUniversal);

export function flowTo(appId: string): ProviderEndpoint[] {
  return FLOW[appId] || [];
}

export function getDefaultApps(endpointId: string): Record<string, boolean> {
  const ep = ENDPOINTS.find((e) => e.id === endpointId);
  return ep ? deriveApps(ep) : { claude: false, codex: false, gemini: false };
}
