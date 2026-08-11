/**
 * Kimi Code CLI provider presets configuration
 *
 * Kimi uses a TOML config at `~/.kimi-code/config.toml` with additive
 * providers: every provider is written to `[providers.<name>]` plus its
 * `[models."<alias>"]` entries, and switching updates the top-level
 * `default_model`.
 *
 * settings_config shape (snake_case, mirroring the TOML fields):
 * ```json
 * {
 *   "name": "kimi",
 *   "type": "openai",
 *   "base_url": "https://api.moonshot.cn/v1",
 *   "api_key": "",
 *   "models": [{ "id": "kimi-k2.7-code", "name": "Kimi K2.7 Code", "max_context_size": 262144 }],
 *   "default_model": "kimi-k2.7-code"
 * }
 * ```
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/** Kimi provider protocol type (written to `[providers.<name>].type`). */
export type KimiApiType =
  | "kimi"
  | "anthropic"
  | "openai"
  | "openai_responses"
  | "google-genai"
  | "vertexai";

/** Default protocol used when a provider has no stored value yet. */
export const KIMI_DEFAULT_API_TYPE: KimiApiType = "openai";

/** Dropdown options for the protocol type selector. `labelKey` is looked up in i18n. */
export const kimiApiTypes: Array<{
  value: KimiApiType;
  labelKey: string;
}> = [
  { value: "kimi", labelKey: "kimi.form.typeKimi" },
  { value: "openai", labelKey: "kimi.form.typeOpenai" },
  { value: "openai_responses", labelKey: "kimi.form.typeOpenaiResponses" },
  { value: "anthropic", labelKey: "kimi.form.typeAnthropic" },
  { value: "google-genai", labelKey: "kimi.form.typeGoogleGenai" },
  { value: "vertexai", labelKey: "kimi.form.typeVertexai" },
];

/** A model entry under a Kimi provider. Serialized to `[models."<id>"]`. */
export interface KimiModel {
  /** Model alias — becomes the TOML key and the value written to top-level default_model. */
  id: string;
  /** Optional display label (written to `display_name`). */
  name?: string;
  /** Context window in tokens (written to `max_context_size`). */
  max_context_size?: number;
  /** Capability tags (thinking / tool_use / ...). */
  capabilities?: string[];
}

export interface KimiProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: KimiProviderSettingsConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
}

export interface KimiProviderSettingsConfig {
  name: string;
  type?: KimiApiType;
  base_url?: string;
  api_key?: string;
  /** UI-side ordered list; serialized to TOML as `[models."<id>"]` entries. */
  models?: KimiModel[];
  /** Alias written to the top-level `default_model` on switch. */
  default_model?: string;
  [key: string]: unknown;
}

export const kimiProviderPresets: KimiProviderPreset[] = [
  // ===== 官方预设 =====
  {
    name: "Kimi For Coding",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    settingsConfig: {
      name: "kimi_coding",
      type: "kimi",
      base_url: "https://api.kimi.com/coding/v1",
      api_key: "",
      models: [
        {
          id: "kimi-code/k3",
          name: "Kimi K3",
          max_context_size: 1048576,
          capabilities: [
            "thinking",
            "always_thinking",
            "image_in",
            "video_in",
            "tool_use",
          ],
        },
        {
          id: "kimi-code/kimi-for-coding",
          name: "Kimi For Coding",
          max_context_size: 262144,
          capabilities: [
            "thinking",
            "always_thinking",
            "image_in",
            "video_in",
            "tool_use",
          ],
        },
        {
          id: "kimi-code/kimi-for-coding-highspeed",
          name: "Kimi For Coding (High-Speed)",
          max_context_size: 262144,
          capabilities: [
            "thinking",
            "always_thinking",
            "image_in",
            "video_in",
            "tool_use",
          ],
        },
      ],
      default_model: "kimi-code/k3",
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi 开放平台",
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    settingsConfig: {
      name: "kimi_platform",
      type: "openai",
      base_url: "https://api.moonshot.cn/v1",
      api_key: "",
      models: [
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          max_context_size: 262144,
          capabilities: ["thinking", "tool_use"],
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          max_context_size: 1048576,
          capabilities: ["thinking", "always_thinking", "tool_use"],
        },
      ],
      default_model: "kimi-k2.7-code",
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#1783FF",
  },
  {
    name: "Kimi Open Platform (Global)",
    websiteUrl: "https://platform.kimi.ai",
    settingsConfig: {
      name: "kimi_platform_global",
      type: "openai",
      base_url: "https://api.moonshot.ai/v1",
      api_key: "",
      models: [
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          max_context_size: 262144,
          capabilities: ["thinking", "tool_use"],
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          max_context_size: 1048576,
          capabilities: ["thinking", "always_thinking", "tool_use"],
        },
      ],
      default_model: "kimi-k2.7-code",
    },
    category: "official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#1783FF",
  },
  // ===== 常见聚合商预设（Kimi 兼容协议）=====
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      name: "packycode_kimi",
      type: "anthropic",
      base_url: "https://www.packyapi.ai",
      api_key: "",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
      ],
      default_model: "claude-opus-5",
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      name: "aicodemirror_kimi",
      type: "anthropic",
      base_url: "https://api.aicodemirror.ai/api/claudecode",
      api_key: "",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
      ],
      default_model: "claude-opus-5",
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      name: "siliconflow_kimi",
      type: "openai",
      base_url: "https://api.siliconflow.cn/v1",
      api_key: "",
      models: [
        { id: "Pro/MiniMaxAI/MiniMax-M2.7", name: "Pro / MiniMax M2.7" },
      ],
      default_model: "Pro/MiniMaxAI/MiniMax-M2.7",
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    settingsConfig: {
      name: "zetaapi_kimi",
      type: "openai",
      base_url: "https://api.zetaapi.ai/v1",
      api_key: "",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
      default_model: "gpt-5.6-sol",
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    settingsConfig: {
      name: "claudecn_kimi",
      type: "anthropic",
      base_url: "https://claudecn.top",
      api_key: "",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
      ],
      default_model: "claude-opus-5",
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
  },
];
