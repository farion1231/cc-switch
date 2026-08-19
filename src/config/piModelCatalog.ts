import type { PiThinkingProfileId } from "./piThinkingProfiles";

export type PiModelInput = "text" | "image";

export interface PiModelCapabilities {
  name: string;
  reasoning: boolean;
  input: PiModelInput[];
  contextWindow: number;
  maxTokens: number;
}

export interface PiModelCatalogEntry {
  capabilities: PiModelCapabilities;
}

/**
 * Reviewed model values used only to build Pi provider presets.
 */
export const piModelCatalog = {
  "amazon/nova-pro": {
    capabilities: {
      name: "Amazon Nova Pro",
      reasoning: false,
      input: ["text", "image"],
      contextWindow: 300_000,
      maxTokens: 8_192,
    },
  },
  "anthropic/claude-fable-5": {
    capabilities: {
      name: "Claude Fable 5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "anthropic/claude-haiku-4.5": {
    capabilities: {
      name: "Claude Haiku 4.5 (latest)",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 200_000,
      maxTokens: 64_000,
    },
  },
  "anthropic/claude-haiku-4.5-20251001": {
    capabilities: {
      name: "Claude Haiku 4.5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 200_000,
      maxTokens: 64_000,
    },
  },
  "anthropic/claude-opus-4.6": {
    capabilities: {
      name: "Claude Opus 4.6",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "anthropic/claude-opus-4.7": {
    capabilities: {
      name: "Claude Opus 4.7",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "anthropic/claude-opus-4.8": {
    capabilities: {
      name: "Claude Opus 4.8",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "anthropic/claude-opus-5": {
    capabilities: {
      name: "Claude Opus 5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "anthropic/claude-sonnet-4.6": {
    capabilities: {
      name: "Claude Sonnet 4.6",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "anthropic/claude-sonnet-5": {
    capabilities: {
      name: "Claude Sonnet 5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "deepseek/deepseek-r1": {
    capabilities: {
      name: "DeepSeek R1",
      reasoning: true,
      input: ["text"],
      contextWindow: 128_000,
      maxTokens: 32_768,
    },
  },
  "deepseek/deepseek-v4-flash": {
    capabilities: {
      name: "DeepSeek V4 Flash",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 384_000,
    },
  },
  "deepseek/deepseek-v4-pro": {
    capabilities: {
      name: "DeepSeek V4 Pro",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 384_000,
    },
  },
  "google/gemini-2.5-flash": {
    capabilities: {
      name: "Gemini 2.5 Flash",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 65_536,
    },
  },
  "google/gemini-2.5-pro": {
    capabilities: {
      name: "Gemini 2.5 Pro",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 65_536,
    },
  },
  "google/gemini-3.1-pro-preview": {
    capabilities: {
      name: "Gemini 3.1 Pro Preview",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 65_536,
    },
  },
  "google/gemini-3.5-flash": {
    capabilities: {
      name: "Gemini 3.5 Flash",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 65_536,
    },
  },
  "google/gemini-3.6-flash": {
    capabilities: {
      name: "Gemini 3.6 Flash",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 65_536,
    },
  },
  "inclusionai/ling-2.5-1t": {
    capabilities: {
      name: "Ling 2.5-1T",
      reasoning: false,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 16_384,
    },
  },
  "longcat/longcat-2.0": {
    capabilities: {
      name: "LongCat 2.0",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 131_072,
    },
  },
  "meta/llama-4-maverick": {
    capabilities: {
      name: "Meta Llama 4 Maverick",
      reasoning: false,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 16_384,
    },
  },
  "minimax/minimax-m2.7": {
    capabilities: {
      name: "MiniMax-M2.7",
      reasoning: true,
      input: ["text"],
      contextWindow: 204_800,
      maxTokens: 131_072,
    },
  },
  "minimax/minimax-m3": {
    capabilities: {
      name: "MiniMax-M3",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 128_000,
    },
  },
  "moonshotai/kimi-k2.5": {
    capabilities: {
      name: "Kimi K2.5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 262_144,
      maxTokens: 262_144,
    },
  },
  "moonshotai/kimi-k2.6": {
    capabilities: {
      name: "Kimi K2.6",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 262_144,
      maxTokens: 262_144,
    },
  },
  "moonshotai/kimi-k2.7-code": {
    capabilities: {
      name: "Kimi K2.7 Code",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 262_144,
      maxTokens: 262_144,
    },
  },
  "moonshotai/kimi-k3": {
    capabilities: {
      name: "Kimi K3",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 131_072,
    },
  },
  "openai/gpt-5": {
    capabilities: {
      name: "GPT-5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5-mini": {
    capabilities: {
      name: "GPT-5 Mini",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.1": {
    capabilities: {
      name: "GPT-5.1",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.2": {
    capabilities: {
      name: "GPT-5.2",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.2-codex": {
    capabilities: {
      name: "GPT-5.2 Codex",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.3-codex": {
    capabilities: {
      name: "GPT-5.3 Codex",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.3-codex-spark": {
    capabilities: {
      name: "GPT-5.3 Codex Spark",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 128_000,
      maxTokens: 32_000,
    },
  },
  "openai/gpt-5.4": {
    capabilities: {
      name: "GPT-5.4",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 272_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.4-mini": {
    capabilities: {
      name: "GPT-5.4 mini",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 400_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.5": {
    capabilities: {
      name: "GPT-5.5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 272_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.6-luna": {
    capabilities: {
      name: "GPT-5.6 Luna",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 272_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.6-sol": {
    capabilities: {
      name: "GPT-5.6 Sol",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 272_000,
      maxTokens: 128_000,
    },
  },
  "openai/gpt-5.6-terra": {
    capabilities: {
      name: "GPT-5.6 Terra",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 272_000,
      maxTokens: 128_000,
    },
  },
  "openai/o3": {
    capabilities: {
      name: "o3",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 200_000,
      maxTokens: 100_000,
    },
  },
  "openai/o4-mini": {
    capabilities: {
      name: "o4-mini",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 200_000,
      maxTokens: 100_000,
    },
  },
  "qwen/qwen3-coder-plus": {
    capabilities: {
      name: "Qwen3 Coder Plus",
      reasoning: false,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 65_536,
    },
  },
  "qwen/qwen3-coder-480b": {
    capabilities: {
      name: "Qwen3 Coder 480B",
      reasoning: false,
      input: ["text"],
      contextWindow: 262_144,
      maxTokens: 65_536,
    },
  },
  "qwen/qwen3.6-plus": {
    capabilities: {
      name: "Qwen3.6 Plus",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 65_536,
    },
  },
  "qwen/qwen3.7-max": {
    capabilities: {
      name: "Qwen3.7 Max",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 131_072,
    },
  },
  "qwen/qwen3.8-max-preview": {
    capabilities: {
      name: "Qwen3.8 Max Preview",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 131_072,
    },
  },
  "stepfun/step-3.5-flash": {
    capabilities: {
      name: "Step 3.5 Flash",
      reasoning: true,
      input: ["text"],
      contextWindow: 256_000,
      maxTokens: 256_000,
    },
  },
  "streamlake/kat-coder-pro": {
    capabilities: {
      name: "KAT-Coder Pro",
      reasoning: true,
      input: ["text"],
      contextWindow: 256_000,
      maxTokens: 32_000,
    },
  },
  "volcengine/ark-code-latest": {
    capabilities: {
      name: "Ark Code Latest",
      reasoning: false,
      input: ["text"],
      contextWindow: 128_000,
      maxTokens: 16_384,
    },
  },
  "volcengine/doubao-seed-2.1-pro": {
    capabilities: {
      name: "Doubao Seed 2.1 Pro",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 128_000,
      maxTokens: 16_384,
    },
  },
  "xai/grok-4.3": {
    capabilities: {
      name: "Grok 4.3",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_000_000,
      maxTokens: 30_000,
    },
  },
  "xai/grok-4.5": {
    capabilities: {
      name: "Grok 4.5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 500_000,
      maxTokens: 500_000,
    },
  },
  "xiaomi/mimo-v2.5": {
    capabilities: {
      name: "MiMo-V2.5",
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 1_048_576,
      maxTokens: 131_072,
    },
  },
  "xiaomi/mimo-v2.5-pro": {
    capabilities: {
      name: "MiMo-V2.5-Pro",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_048_576,
      maxTokens: 131_072,
    },
  },
  "zai/glm-5.1": {
    capabilities: {
      name: "GLM-5.1",
      reasoning: true,
      input: ["text"],
      contextWindow: 200_000,
      maxTokens: 131_072,
    },
  },
  "zai/glm-5.2": {
    capabilities: {
      name: "GLM-5.2",
      reasoning: true,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 131_072,
    },
  },
} as const satisfies Record<string, PiModelCatalogEntry>;

export type PiModelCatalogKey = keyof typeof piModelCatalog;

export const PI_MODEL_CATALOG_REFERENCE: unique symbol = Symbol(
  "piModelCatalogReference",
);

export interface PiModelCatalogReference {
  catalogKey: PiModelCatalogKey;
  presetThinkingProfileId?: PiThinkingProfileId;
}

export interface PiCatalogModel extends PiModelCapabilities {
  id: string;
  [PI_MODEL_CATALOG_REFERENCE]: PiModelCatalogReference;
}

type PiModelOptions = Partial<PiModelCapabilities> & {
  id: string;
  thinkingProfile?: PiThinkingProfileId;
};

export function piModel(
  catalogKey: PiModelCatalogKey,
  options: PiModelOptions,
): PiCatalogModel {
  const profile = piModelCatalog[catalogKey].capabilities;
  const { id, thinkingProfile, ...overrides } = options;
  const input = overrides.input ?? profile.input;
  return {
    ...profile,
    ...overrides,
    id,
    input: [...input],
    [PI_MODEL_CATALOG_REFERENCE]: {
      catalogKey,
      ...(thinkingProfile ? { presetThinkingProfileId: thinkingProfile } : {}),
    },
  };
}

export function getPiModelCatalogReference(
  model: unknown,
): PiModelCatalogReference | undefined {
  if (!model || typeof model !== "object") return undefined;
  return (model as Partial<PiCatalogModel>)[PI_MODEL_CATALOG_REFERENCE];
}
