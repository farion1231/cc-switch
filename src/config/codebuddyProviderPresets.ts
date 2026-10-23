import type { ProviderCategory } from "@/types";
import type { PresetTheme } from "./claudeProviderPresets";
import type { PresetFamilyFields } from "./presetFamilies";

/**
 * One entry of CodeBuddy Code's `~/.codebuddy/models.json` `models` array.
 * See https://www.codebuddy.ai/docs/cli/models — the model `id` is the CC Switch
 * provider key, so it is not stored here.
 */
export interface CodebuddyModelConfig {
  name?: string;
  vendor?: string;
  url?: string;
  apiKey?: string;
  /**
   * Not edited by the form: CC Switch only targets OpenAI Chat Completions (full
   * `/chat/completions` URL). Kept as-is when an existing entry carries it.
   */
  api?: string;
  maxInputTokens?: number;
  maxOutputTokens?: number;
  supportsToolCall?: boolean;
  supportsImages?: boolean;
  supportsReasoning?: boolean;
  [key: string]: unknown;
}

export interface CodebuddyProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string;
  /** Model id written to models.json. */
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: CodebuddyModelConfig;
  category?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
}

/**
 * "CodeBuddy official sign-in": not a models.json entry. The card binds one CodeBuddy
 * account from the Auth Center (`meta.authBinding`, authProvider `codebuddy_oauth`);
 * Switch on the card replaces CodeBuddy's global sign-in with that account.
 */
export const CODEBUDDY_OFFICIAL_AUTH_PROVIDER = "codebuddy_oauth";
export const CODEBUDDY_OFFICIAL_ID_PREFIX = "codebuddy-official-";

// Values follow each vendor's own CodeBuddy integration guide.
export const codebuddyProviderPresets: CodebuddyProviderPreset[] = [
  {
    name: "CodeBuddy Official",
    nameKey: "codebuddy.official.presetName",
    providerKey: "",
    websiteUrl: "https://www.codebuddy.ai",
    category: "official",
    icon: "codebuddy",
    settingsConfig: {},
  },
  {
    // https://api-docs.deepseek.com/quick_start/agent_integrations/workbuddy/
    name: "DeepSeek",
    providerKey: "deepseek-flash",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    category: "cn_official",
    icon: "deepseek",
    settingsConfig: {
      name: "DeepSeek Flash",
      vendor: "DeepSeek",
      url: "https://api.deepseek.com/v1/chat/completions",
      apiKey: "",
      maxInputTokens: 128000,
      maxOutputTokens: 8192,
      supportsToolCall: true,
      supportsImages: true,
    },
  },
  {
    // https://mimo.mi.com/docs/en-US/tokenplan/integration/codebuddy
    name: "Xiaomi MiMo",
    providerKey: "mimo-v2.6-pro",
    websiteUrl: "https://platform.xiaomimimo.com",
    category: "cn_official",
    icon: "xiaomimimo",
    settingsConfig: {
      name: "mimo-v2.6-pro",
      vendor: "MiMo",
      url: "https://api.xiaomimimo.com/v1/chat/completions",
      apiKey: "",
      supportsToolCall: true,
      supportsImages: true,
    },
  },
  {
    // https://longcat.chat/platform/docs/code-buddy
    name: "LongCat",
    providerKey: "LongCat-2.5-Preview",
    websiteUrl: "https://longcat.chat/platform",
    category: "cn_official",
    icon: "longcat",
    settingsConfig: {
      name: "LongCat-2.5-Preview",
      vendor: "LongCat",
      url: "https://api.longcat.chat/openai/v1/chat/completions",
      apiKey: "",
      supportsToolCall: true,
      supportsReasoning: true,
    },
  },
];
