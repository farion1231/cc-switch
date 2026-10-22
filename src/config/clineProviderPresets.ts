import { z } from "zod";
import {
  piProviderPresets,
  type PiApiFormat,
  type PiPresetModel,
  type PiProviderPreset,
} from "./piProviderPresets";

/** Protocols accepted for custom providers in Cline's `models.json`. */
export const CLINE_PROTOCOLS = [
  "openai-chat",
  "openai-responses",
  "anthropic",
  "gemini",
  "openai-r1",
  "ai-sdk",
] as const;
export type ClineProtocol = (typeof CLINE_PROTOCOLS)[number];

/** Numeric model fields; Cline resets its files when any of them is not a number. */
export const CLINE_MODEL_NUMBER_FIELDS = [
  "contextWindow",
  "maxTokens",
  "inputPrice",
  "outputPrice",
  "cacheReadsPrice",
  "cacheWritesPrice",
] as const;

/** Prefix of custom provider ids created by CC Switch (never a built-in id). */
export const CLINE_CUSTOM_ID_PREFIX = "ccs-";

// Strings are kept loose here so native entries with values this editor does not
// manage still load; `validateClineConfig` enforces what the backend rejects.
const clineModelSchema = z
  .object({
    id: z.string().optional(),
    name: z.string().optional(),
    contextWindow: z.number().optional(),
    maxTokens: z.number().optional(),
    inputPrice: z.number().optional(),
    outputPrice: z.number().optional(),
    cacheReadsPrice: z.number().optional(),
    cacheWritesPrice: z.number().optional(),
    capabilities: z.array(z.string()).optional(),
  })
  .passthrough();

export const clineConfigSchema = z
  .object({
    settings: z
      .object({
        apiKey: z.string().optional(),
        model: z.string().optional(),
        baseUrl: z.string().optional(),
        headers: z.record(z.string(), z.string()).optional(),
      })
      .passthrough(),
    tokenSource: z.enum(["manual", "oauth", "migration"]).optional(),
    provider: z
      .object({
        name: z.string(),
        baseUrl: z.string(),
        defaultModelId: z.string().optional(),
        protocol: z.string().optional(),
        client: z.string().optional(),
      })
      .passthrough()
      .optional(),
    models: z.record(z.string(), clineModelSchema).optional(),
  })
  .passthrough();

export type ClineProviderConfig = z.infer<typeof clineConfigSchema>;
export type ClineModel = z.infer<typeof clineModelSchema>;

/** Custom providers are declared in `models.json`; built-in ones are not. */
export function isClineCustomConfig(config: unknown): boolean {
  if (!config || typeof config !== "object") return false;
  const provider = (config as Record<string, unknown>).provider;
  return Boolean(provider) && typeof provider === "object";
}

/** OAuth entries (e.g. the Cline account) keep their tokens in Cline. */
export function isClineOAuthConfig(config: unknown): boolean {
  return (
    Boolean(config) &&
    typeof config === "object" &&
    (config as Record<string, unknown>).tokenSource === "oauth"
  );
}

export function emptyClineCustomConfig(
  protocol: ClineProtocol = "openai-chat",
): ClineProviderConfig {
  return {
    settings: { apiKey: "" },
    provider: {
      name: "",
      baseUrl: "",
      protocol,
      ...(protocol === "anthropic" ? { client: "anthropic" } : {}),
    },
    models: {},
  };
}

/**
 * Cline sends Anthropic-protocol providers to /chat/completions unless the
 * client is also "anthropic"; any other protocol must not keep that client.
 */
export function withClineProtocol(
  provider: NonNullable<ClineProviderConfig["provider"]>,
  protocol: string,
): NonNullable<ClineProviderConfig["provider"]> {
  const { client: _client, ...rest } = provider;
  return protocol === "anthropic"
    ? { ...rest, protocol, client: "anthropic" }
    : { ...rest, protocol };
}

/** `ccs-<ascii slug>`, unique among `existingIds` (`-2`, `-3`… on collision). */
export function generateClineProviderId(
  name: string,
  existingIds: Iterable<string>,
): string {
  const slug = name
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const base = `${CLINE_CUSTOM_ID_PREFIX}${slug || "custom"}`;
  const taken = new Set(existingIds);
  if (!taken.has(base)) return base;
  let index = 2;
  while (taken.has(`${base}-${index}`)) index += 1;
  return `${base}-${index}`;
}

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      (url.protocol === "http:" || url.protocol === "https:") && !!url.host
    );
  } catch {
    return false;
  }
}

/**
 * Mirrors the backend's `cline_config::validate_provider`. Returns i18n keys
 * (under `cline.validation.*`) for every rule the config breaks.
 */
export function validateClineConfig(config: ClineProviderConfig): string[] {
  const issues: string[] = [];
  const { settings } = config;
  if (settings.baseUrl?.trim() && !isHttpUrl(settings.baseUrl.trim())) {
    issues.push("cline.validation.baseUrl");
  }
  if (
    Object.values(settings.headers ?? {}).some(
      (value) => typeof value !== "string",
    )
  ) {
    issues.push("cline.validation.headers");
  }
  if (!config.provider) return issues;

  const { provider } = config;
  if (!provider.name.trim()) issues.push("cline.validation.name");
  if (!isHttpUrl(provider.baseUrl.trim())) {
    issues.push("cline.validation.baseUrl");
  }
  if (
    provider.protocol !== undefined &&
    !(CLINE_PROTOCOLS as readonly string[]).includes(provider.protocol)
  ) {
    issues.push("cline.validation.protocol");
  }
  if (provider.protocol === "anthropic" && provider.client !== "anthropic") {
    issues.push("cline.validation.anthropicClient");
  }
  const entries = Object.entries(config.models ?? {});
  if (!entries.length) {
    issues.push("cline.validation.models");
    return [...new Set(issues)];
  }
  if (entries.some(([key]) => !key.trim())) {
    issues.push("cline.validation.modelId");
  }
  if (
    entries.some(([, model]) =>
      CLINE_MODEL_NUMBER_FIELDS.some(
        (field) =>
          model[field] !== undefined &&
          (typeof model[field] !== "number" ||
            !Number.isFinite(model[field] as number)),
      ),
    )
  ) {
    issues.push("cline.validation.modelNumber");
  }
  if (!settings.model || !config.models?.[settings.model]) {
    issues.push("cline.validation.selectedModel");
  }
  return [...new Set(issues)];
}

/**
 * Cline's default endpoints for built-in providers, used to fetch their model
 * lists when no base URL override is set.
 */
export const CLINE_BUILTIN_MODEL_BASE_URLS: Record<string, string> = {
  ollama: "http://localhost:11434/v1",
  lmstudio: "http://localhost:1234/v1",
  litellm: "http://localhost:4000/v1",
  deepseek: "https://api.deepseek.com/v1",
  groq: "https://api.groq.com/openai/v1",
  openrouter: "https://openrouter.ai/api/v1",
  "openai-native": "https://api.openai.com/v1",
  xai: "https://api.x.ai/v1",
};

export interface ClineProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: ClineProviderConfig;
  /** Model ids offered for built-in providers, whose model list Cline owns. */
  suggestedModels?: string[];
}

/** A built-in Cline provider slot a catalog vendor maps onto. */
interface BuiltinTarget {
  id: string;
  /** Keep the catalog endpoint (regional / plan endpoints); otherwise Cline's default. */
  keepBaseUrl?: boolean;
}

// Only vendors whose catalog endpoint is the same service as Cline's built-in
// provider; everything else becomes a custom provider in models.json.
const BUILTIN_TARGETS: Record<string, BuiltinTarget> = {
  "cc-switch-kimi": { id: "moonshot", keepBaseUrl: true },
  "cc-switch-kimi-global": { id: "moonshot", keepBaseUrl: true },
  "cc-switch-deep-seek": { id: "deepseek" },
  "cc-switch-zhipu-glm-en": { id: "zai-coding-plan" },
  "cc-switch-qianwenai": { id: "qwen" },
  "cc-switch-dou-bao-seed": { id: "doubao" },
  "cc-switch-groq": { id: "groq" },
  "cc-switch-sambanova": { id: "sambanova" },
  "cc-switch-open-router": { id: "openrouter" },
  "cc-switch-ollama": { id: "ollama" },
  "cc-switch-lmstudio": { id: "lmstudio" },
};

// Bedrock's signed Converse API and Gemini's native API have no catalog
// equivalent here, so only OpenAI- and Anthropic-style endpoints are admitted.
const CLINE_API_FORMATS: Partial<Record<PiApiFormat, ClineProtocol>> = {
  "openai-completions": "openai-chat",
  "openai-responses": "openai-responses",
  "anthropic-messages": "anthropic",
};

function toClineModel(model: PiPresetModel): ClineModel {
  const capabilities = ["tools"];
  if (model.input.includes("image")) capabilities.push("images");
  if (model.reasoning) capabilities.push("reasoning");
  return {
    id: model.id,
    name: model.name,
    contextWindow: model.contextWindow,
    maxTokens: model.maxTokens,
    capabilities,
  };
}

// Cline appends only /messages or /chat/completions; Anthropic-style catalog
// roots omit the /v1 that Claude clients add themselves.
function clineBaseUrl(baseUrl: string, protocol: ClineProtocol): string {
  const trimmed = baseUrl.replace(/\/+$/, "");
  return protocol === "anthropic" && !/\/v1$/.test(trimmed)
    ? `${trimmed}/v1`
    : trimmed;
}

const catalogPresets: ClineProviderPreset[] = piProviderPresets.flatMap(
  (preset): ClineProviderPreset[] => {
    const { settingsConfig: source, ...identity } = preset;
    const protocol = CLINE_API_FORMATS[source.api];
    if (!protocol || source.compat || !source.models.length) return [];
    const local = preset.category === "local";
    const headers = source.headers ? { ...source.headers } : undefined;
    const target = BUILTIN_TARGETS[preset.providerKey];
    if (target) {
      return [
        {
          ...identity,
          providerKey: target.id,
          suggestedModels: source.models.map((model) => model.id),
          settingsConfig: {
            settings: {
              ...(local ? {} : { apiKey: source.apiKey }),
              model: source.models[0].id,
              ...(target.keepBaseUrl ? { baseUrl: source.baseUrl } : {}),
              ...(headers ? { headers } : {}),
            },
          },
        },
      ];
    }
    const models = Object.fromEntries(
      source.models.map((model) => [model.id, toClineModel(model)]),
    );
    return [
      {
        ...identity,
        settingsConfig: {
          settings: {
            apiKey: source.apiKey || (local ? "local" : ""),
            model: source.models[0].id,
            ...(headers ? { headers } : {}),
          },
          provider: withClineProtocol(
            {
              name: preset.name,
              baseUrl: clineBaseUrl(source.baseUrl, protocol),
              defaultModelId: source.models[0].id,
            },
            protocol,
          ),
          models,
        },
      },
    ];
  },
);

function builtinPreset(
  preset: Omit<ClineProviderPreset, "settingsConfig"> & {
    /** Omitted when Cline picks its own default model. */
    model?: string;
  },
): ClineProviderPreset {
  const { model, ...rest } = preset;
  return {
    ...rest,
    suggestedModels: rest.suggestedModels ?? (model ? [model] : []),
    settingsConfig: { settings: { apiKey: "", ...(model ? { model } : {}) } },
  };
}

// Cline's own provider slots that have no counterpart in the shared catalog.
const nativePresets: ClineProviderPreset[] = [
  builtinPreset({
    name: "Anthropic",
    providerKey: "anthropic",
    websiteUrl: "https://www.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    category: "cn_official",
    icon: "anthropic",
    model: "claude-sonnet-5",
  }),
  builtinPreset({
    name: "OpenAI",
    providerKey: "openai-native",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    category: "cn_official",
    icon: "openai",
    model: "gpt-5.4",
  }),
  builtinPreset({
    name: "Google Gemini",
    providerKey: "gemini",
    websiteUrl: "https://ai.google.dev",
    apiKeyUrl: "https://aistudio.google.com/apikey",
    category: "cn_official",
    icon: "gemini",
  }),
  builtinPreset({
    name: "xAI",
    providerKey: "xai",
    websiteUrl: "https://x.ai",
    apiKeyUrl: "https://console.x.ai",
    category: "cn_official",
    icon: "xai",
    model: "grok-4.20-0309-non-reasoning",
  }),
  builtinPreset({
    name: "LiteLLM",
    providerKey: "litellm",
    websiteUrl: "https://www.litellm.ai",
    category: "local",
  }),
];

export const CLINE_CUSTOM_OPENAI_PRESET_KEY = "custom-openai";
export const CLINE_CUSTOM_ANTHROPIC_PRESET_KEY = "custom-anthropic";

// Blank custom providers for any OpenAI- or Anthropic-compatible endpoint.
const customPresets: ClineProviderPreset[] = [
  {
    name: "Custom (OpenAI-compatible)",
    nameKey: "cline.preset.customOpenAI",
    providerKey: CLINE_CUSTOM_OPENAI_PRESET_KEY,
    websiteUrl: "",
    category: "custom",
    settingsConfig: emptyClineCustomConfig("openai-chat"),
  },
  {
    name: "Custom (Anthropic-compatible)",
    nameKey: "cline.preset.customAnthropic",
    providerKey: CLINE_CUSTOM_ANTHROPIC_PRESET_KEY,
    websiteUrl: "",
    category: "custom",
    settingsConfig: emptyClineCustomConfig("anthropic"),
  },
];

export const clineProviderPresets: ClineProviderPreset[] = [
  ...customPresets,
  ...nativePresets,
  ...catalogPresets,
];
