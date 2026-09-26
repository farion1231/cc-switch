/**
 * StepCode provider presets configuration
 *
 * StepCode（StepFun "StepCode" CLI）uses an additive providers structure in
 * `~/.stepcode/models.json` that is IDENTICAL to OpenClaw's
 * (`{ baseUrl, apiKey, api, models: [...] }`, camelCase). The preset/type
 * surface here mirrors the OpenClaw implementation so the add-provider form,
 * preset selector, and live-config data flow behave the same way.
 */
import type { ProviderCategory, StepCodeProviderConfig } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface StepcodeProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  /** StepCode settings_config structure */
  settingsConfig: StepCodeProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  /** Template variable definitions */
  templateValues?: Record<string, TemplateValueConfig>;
  /** Visual theme config */
  theme?: PresetTheme;
  /** Icon name */
  icon?: string;
  /** Icon color */
  iconColor?: string;
  /** Mark as custom template (for UI distinction) */
  isCustomTemplate?: boolean;
}

/**
 * StepCode API protocol options
 * StepCode reuses the OpenAI-compatible protocol family. Kept minimal — the
 * three protocols below cover the endpoints StepCode documents.
 */
export const stepcodeApiProtocols = [
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "openai-completions", label: "OpenAI Completions" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
] as const;

/**
 * StepCode provider presets list.
 *
 * StepCode ships without a curated sponsor catalog, so this starts empty and
 * users configure providers via the "custom" entry. The array is exported (and
 * typed) so the preset selector / ProviderForm wiring mirrors OpenClaw exactly
 * and can grow presets later without touching the form.
 */
export const stepcodeProviderPresets: StepcodeProviderPreset[] = [];
