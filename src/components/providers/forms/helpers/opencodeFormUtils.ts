import type { OpenCodeModel, OpenCodeProviderConfig } from "@/types";
import type { PricingModelSourceOption } from "../ProviderAdvancedConfig";

// ── Default configs ──────────────────────────────────────────────────

export const CLAUDE_DEFAULT_CONFIG = JSON.stringify({ env: {} }, null, 2);
export const CLAUDE_DESKTOP_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      ANTHROPIC_BASE_URL: "",
      ANTHROPIC_AUTH_TOKEN: "",
    },
  },
  null,
  2,
);
export const CODEX_DEFAULT_CONFIG = JSON.stringify(
  { auth: {}, config: "" },
  null,
  2,
);
export const GEMINI_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      GOOGLE_GEMINI_BASE_URL: "",
      GEMINI_API_KEY: "",
      GEMINI_MODEL: "gemini-3-pro-preview",
    },
  },
  null,
  2,
);

export const OPENCODE_DEFAULT_NPM = "@ai-sdk/openai-compatible";
export const OPENCODE_DEFAULT_CONFIG = JSON.stringify(
  {
    npm: OPENCODE_DEFAULT_NPM,
    options: {
      baseURL: "",
      apiKey: "",
      setCacheKey: true,
    },
    models: {},
  },
  null,
  2,
);
export const OPENCODE_KNOWN_OPTION_KEYS = [
  "baseURL",
  "apiKey",
  "headers",
] as const;

export const OPENCLAW_DEFAULT_CONFIG = JSON.stringify(
  {
    baseUrl: "",
    apiKey: "",
    api: "openai-completions",
    models: [],
  },
  null,
  2,
);

// ── Pure functions ───────────────────────────────────────────────────

export function isKnownOpencodeOptionKey(key: string): boolean {
  return OPENCODE_KNOWN_OPTION_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_OPTION_KEYS)[number],
  );
}

export function parseOpencodeConfig(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const normalize = (
    parsed: Partial<OpenCodeProviderConfig>,
  ): OpenCodeProviderConfig => ({
    npm: parsed.npm || OPENCODE_DEFAULT_NPM,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  });

  try {
    const parsed = JSON.parse(
      settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
    ) as Partial<OpenCodeProviderConfig>;
    return normalize(parsed);
  } catch {
    return {
      npm: OPENCODE_DEFAULT_NPM,
      options: {},
      models: {},
    };
  }
}

export function parseOpencodeConfigStrict(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const parsed = JSON.parse(
    settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
  ) as Partial<OpenCodeProviderConfig>;
  return {
    npm: parsed.npm || OPENCODE_DEFAULT_NPM,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  };
}

export const OPENCODE_KNOWN_MODEL_KEYS = ["name", "limit", "options"] as const;

export function isKnownModelKey(key: string): boolean {
  return OPENCODE_KNOWN_MODEL_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_MODEL_KEYS)[number],
  );
}

/**
 * 获取模型的额外字段（用户自定义字段）
 * 排除保留字段（name, limit, options）和高级字段
 * 这些字段不会在 UI 中显示，仅用于内部处理
 */
export function getModelExtraFields(
  model: OpenCodeModel,
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(model)) {
    if (!isKnownModelKey(k) && !isAdvancedModelKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

// 高级属性：limit, modalities, variants 等顶层字段（排除 name 和 options）
export const OPENCODE_ADVANCED_MODEL_KEYS = ["limit", "modalities", "variants", "cost", "contextLimit", "outputLimit"] as const;

/**
 * 检查字段是否为高级模型属性
 * 高级属性包括：limit, modalities, variants, cost, contextLimit, outputLimit
 * 这些字段在 UI 的"高级属性"区域显示，与 options 同级（顶层字段）
 */
export function isAdvancedModelKey(key: string): boolean {
  return OPENCODE_ADVANCED_MODEL_KEYS.includes(
    key as (typeof OPENCODE_ADVANCED_MODEL_KEYS)[number],
  );
}

/**
 * 获取模型的高级属性字段（顶层字段，与 options 同级）
 * 返回所有非保留字段（排除 name, limit, options），包括高级字段和用户自定义字段
 */
export function getModelAdvancedFields(
  model: OpenCodeModel,
): Record<string, string> {
  const advanced: Record<string, string> = {};
  for (const [k, v] of Object.entries(model)) {
    // 排除保留字段 name, limit, options，返回所有其他顶层字段
    if (k !== "name" && k !== "limit" && k !== "options") {
      advanced[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return advanced;
}

export function toOpencodeExtraOptions(
  options: OpenCodeProviderConfig["options"],
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(options || {})) {
    if (!isKnownOpencodeOptionKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

export { buildOmoProfilePreview } from "@/types/omo";

export const normalizePricingSource = (
  value?: string,
): PricingModelSourceOption =>
  value === "request" || value === "response" ? value : "inherit";
