/**
 * Code-based capability queries avoid DDL changes: source is reversible, schemas are not.
 */

export type ApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "google";

export interface Transport {
  formats: Set<ApiFormat>;
  baseUrl: string;
  supportsModelsEndpoint: boolean;
}

export interface ProviderEndpoint {
  id: string;
  name: string;
  category: "official" | "cn_official" | "aggregator" | "partner" | "custom";
  transport: Transport;
  icon?: string;
  iconColor?: string;
  websiteUrl?: string;
  apiKeyUrl?: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
}

export interface Slot {
  acceptsFormats: Set<ApiFormat>;
}

export const APP_SLOTS: Record<string, Slot> = {
  claude: { acceptsFormats: new Set(["anthropic"]) },
  codex: { acceptsFormats: new Set(["openai_responses"]) },
  gemini: { acceptsFormats: new Set(["google"]) },
  opencode: { acceptsFormats: new Set(["anthropic", "openai_chat", "google"]) },
  openclaw: { acceptsFormats: new Set(["anthropic", "openai_chat"]) },
};

export function fitsSlot(endpoint: ProviderEndpoint, slot: Slot): boolean {
  for (const fmt of endpoint.transport.formats) {
    if (slot.acceptsFormats.has(fmt)) return true;
  }
  return false;
}

export function deriveApps(
  endpoint: ProviderEndpoint,
): Record<string, boolean> {
  return {
    claude: fitsSlot(endpoint, APP_SLOTS.claude),
    codex: fitsSlot(endpoint, APP_SLOTS.codex),
    gemini: fitsSlot(endpoint, APP_SLOTS.gemini),
  };
}

export function isUniversal(endpoint: ProviderEndpoint): boolean {
  const apps = deriveApps(endpoint);
  return apps.claude && apps.codex && apps.gemini;
}

export function flowToSlot(
  endpoints: ProviderEndpoint[],
  slot: Slot,
): ProviderEndpoint[] {
  return endpoints.filter((ep) => fitsSlot(ep, slot));
}
