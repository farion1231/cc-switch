/**
 * Pi Agent provider presets — based on official Pi documentation
 * https://pi.dev/docs/latest/models
 * https://pi.dev/docs/latest/providers
 */
import type { PiProviderTemplate, PiApiType, PiModelDraft } from "@/types/pi";

export interface PiProviderPreset {
  id: PiProviderTemplate;
  label: string;
  description: string;
  defaultApi: PiApiType;
  defaultBaseUrl?: string;
}

/**
 * Template presets — used to select the API type and provide sensible defaults
 */
export const piProviderPresets: PiProviderPreset[] = [
  {
    id: "openAiCompatible",
    label: "OpenAI-compatible",
    description: "OpenAI Chat Completions (most compatible)",
    defaultApi: "openai-completions",
  },
  {
    id: "openAiResponses",
    label: "OpenAI Responses",
    description: "OpenAI Responses API (o-series reasoning models)",
    defaultApi: "openai-responses",
  },
  {
    id: "anthropicCompatible",
    label: "Anthropic-compatible",
    description: "Anthropic Messages API (Claude models)",
    defaultApi: "anthropic-messages",
  },
  {
    id: "googleGenerativeAi",
    label: "Google Generative AI",
    description: "Google AI Studio / Gemini API",
    defaultApi: "google-generative-ai",
    defaultBaseUrl: "https://generativelanguage.googleapis.com/v1beta",
  },
  {
    id: "localOpenAiCompatible",
    label: "Local (Ollama/vLLM/LM Studio)",
    description: "Local inference servers at localhost",
    defaultApi: "openai-completions",
    defaultBaseUrl: "http://localhost:11434/v1",
  },
  {
    id: "custom",
    label: "Custom",
    description: "Manually configure all fields",
    defaultApi: "openai-completions",
  },
];

// ============================================================================
// Built-in provider vendor presets — real configurations from Pi official docs
// ============================================================================

export interface PiVendorPreset {
  /** Provider ID to write into models.json (key under "providers") */
  providerId: string;
  /** Display name */
  name: string;
  /** Provider website */
  websiteUrl?: string;
  /** API type */
  api: PiApiType;
  /** Base URL (required for custom, optional for built-in overrides) */
  baseUrl?: string;
  /** Default env var name for the API key */
  apiKeyEnvVar: string;
  /** Whether this is a built-in Pi provider (override mode) */
  isBuiltin: boolean;
  /** Category for display grouping */
  category: "official" | "cloud" | "cn_provider" | "aggregator" | "local";
  /** Default models to pre-populate */
  defaultModels: PiModelDraft[];
  /** Description */
  description: string;
  /** Icon name (from cc-switch icons) */
  icon?: string;
}

export const piVendorPresets: PiVendorPreset[] = [
  // ─── Official / Built-in Providers ───────────────────────────────────────────
  {
    providerId: "openai",
    name: "OpenAI",
    websiteUrl: "https://platform.openai.com",
    api: "openai-responses",
    apiKeyEnvVar: "OPENAI_API_KEY",
    isBuiltin: true,
    category: "official",
    defaultModels: [
      {
        id: "o3",
        name: "o3",
        nameTouched: true,
        reasoning: true,
        contextWindow: 200000,
        maxTokens: 100000,
      },
      {
        id: "o4-mini",
        name: "o4-mini",
        nameTouched: true,
        reasoning: true,
        contextWindow: 200000,
        maxTokens: 100000,
      },
      {
        id: "gpt-5.1",
        name: "GPT-5.1",
        nameTouched: true,
        contextWindow: 1047576,
        maxTokens: 65536,
      },
      {
        id: "codex-mini-latest",
        name: "Codex Mini",
        nameTouched: true,
        reasoning: true,
        contextWindow: 192000,
        maxTokens: 65536,
      },
    ],
    description: "OpenAI official API — requires OPENAI_API_KEY",
    icon: "openai",
  },
  {
    providerId: "anthropic",
    name: "Anthropic",
    websiteUrl: "https://console.anthropic.com",
    api: "anthropic-messages",
    apiKeyEnvVar: "ANTHROPIC_API_KEY",
    isBuiltin: true,
    category: "official",
    defaultModels: [
      {
        id: "claude-sonnet-4-5-20250514",
        name: "Claude Sonnet 4.5",
        nameTouched: true,
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 200000,
        maxTokens: 16384,
      },
      {
        id: "claude-opus-4-8-20250619",
        name: "Claude Opus 4.8",
        nameTouched: true,
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 200000,
        maxTokens: 32000,
      },
      {
        id: "claude-haiku-4-5-20251001",
        name: "Claude Haiku 4.5",
        nameTouched: true,
        input: ["text", "image"],
        contextWindow: 200000,
        maxTokens: 8192,
      },
    ],
    description: "Anthropic official API — requires ANTHROPIC_API_KEY",
    icon: "anthropic",
  },
  {
    providerId: "google",
    name: "Google Gemini",
    websiteUrl: "https://aistudio.google.com",
    api: "google-generative-ai",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    apiKeyEnvVar: "GEMINI_API_KEY",
    isBuiltin: true,
    category: "official",
    defaultModels: [
      {
        id: "gemini-2.5-pro",
        name: "Gemini 2.5 Pro",
        nameTouched: true,
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 1048576,
        maxTokens: 65536,
      },
      {
        id: "gemini-2.5-flash",
        name: "Gemini 2.5 Flash",
        nameTouched: true,
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 1048576,
        maxTokens: 65536,
      },
    ],
    description: "Google AI Studio — requires GEMINI_API_KEY",
    icon: "gemini",
  },
  {
    providerId: "deepseek",
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    api: "openai-completions",
    baseUrl: "https://api.deepseek.com/v1",
    apiKeyEnvVar: "DEEPSEEK_API_KEY",
    isBuiltin: true,
    category: "official",
    defaultModels: [
      {
        id: "deepseek-r1",
        name: "DeepSeek R1",
        nameTouched: true,
        reasoning: true,
        contextWindow: 128000,
        maxTokens: 65536,
      },
      {
        id: "deepseek-chat",
        name: "DeepSeek V3",
        nameTouched: true,
        contextWindow: 128000,
        maxTokens: 8192,
      },
    ],
    description: "DeepSeek official API — requires DEEPSEEK_API_KEY",
    icon: "deepseek",
  },
  {
    providerId: "xai",
    name: "xAI (Grok)",
    websiteUrl: "https://console.x.ai",
    api: "openai-completions",
    baseUrl: "https://api.x.ai/v1",
    apiKeyEnvVar: "XAI_API_KEY",
    isBuiltin: true,
    category: "official",
    defaultModels: [
      {
        id: "grok-3",
        name: "Grok 3",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
        maxTokens: 32768,
      },
      {
        id: "grok-3-mini",
        name: "Grok 3 Mini",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
        maxTokens: 32768,
      },
    ],
    description: "xAI Grok API — requires XAI_API_KEY",
  },
  {
    providerId: "mistral",
    name: "Mistral AI",
    websiteUrl: "https://console.mistral.ai",
    api: "openai-completions",
    baseUrl: "https://api.mistral.ai/v1",
    apiKeyEnvVar: "MISTRAL_API_KEY",
    isBuiltin: true,
    category: "official",
    defaultModels: [
      {
        id: "codestral-latest",
        name: "Codestral",
        nameTouched: true,
        contextWindow: 256000,
        maxTokens: 32768,
      },
      {
        id: "mistral-large-latest",
        name: "Mistral Large",
        nameTouched: true,
        contextWindow: 128000,
        maxTokens: 32768,
      },
    ],
    description: "Mistral AI API — requires MISTRAL_API_KEY",
  },

  // ─── Aggregators ─────────────────────────────────────────────────────────────
  {
    providerId: "openrouter",
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    api: "openai-completions",
    baseUrl: "https://openrouter.ai/api/v1",
    apiKeyEnvVar: "OPENROUTER_API_KEY",
    isBuiltin: true,
    category: "aggregator",
    defaultModels: [
      {
        id: "anthropic/claude-sonnet-4",
        name: "Claude Sonnet 4",
        nameTouched: true,
        reasoning: true,
        contextWindow: 200000,
      },
      {
        id: "openai/o3",
        name: "o3",
        nameTouched: true,
        reasoning: true,
        contextWindow: 200000,
      },
      {
        id: "google/gemini-2.5-pro",
        name: "Gemini 2.5 Pro",
        nameTouched: true,
        reasoning: true,
        contextWindow: 1048576,
      },
    ],
    description:
      "OpenRouter — multi-provider aggregator, requires OPENROUTER_API_KEY",
  },
  {
    providerId: "together",
    name: "Together AI",
    websiteUrl: "https://www.together.ai",
    api: "openai-completions",
    baseUrl: "https://api.together.xyz/v1",
    apiKeyEnvVar: "TOGETHER_API_KEY",
    isBuiltin: true,
    category: "aggregator",
    defaultModels: [
      {
        id: "deepseek-ai/DeepSeek-R1",
        name: "DeepSeek R1",
        nameTouched: true,
        reasoning: true,
        contextWindow: 128000,
      },
      {
        id: "Qwen/Qwen3-Coder",
        name: "Qwen3 Coder",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
      },
    ],
    description:
      "Together AI — open model inference, requires TOGETHER_API_KEY",
  },
  {
    providerId: "fireworks",
    name: "Fireworks AI",
    websiteUrl: "https://fireworks.ai",
    api: "openai-completions",
    baseUrl: "https://api.fireworks.ai/inference/v1",
    apiKeyEnvVar: "FIREWORKS_API_KEY",
    isBuiltin: true,
    category: "aggregator",
    defaultModels: [
      {
        id: "accounts/fireworks/models/deepseek-r1",
        name: "DeepSeek R1",
        nameTouched: true,
        reasoning: true,
        contextWindow: 128000,
      },
    ],
    description: "Fireworks AI — fast inference, requires FIREWORKS_API_KEY",
  },
  {
    providerId: "groq",
    name: "Groq",
    websiteUrl: "https://console.groq.com",
    api: "openai-completions",
    baseUrl: "https://api.groq.com/openai/v1",
    apiKeyEnvVar: "GROQ_API_KEY",
    isBuiltin: true,
    category: "aggregator",
    defaultModels: [
      {
        id: "llama-4-maverick-17b-128e-instruct",
        name: "Llama 4 Maverick",
        nameTouched: true,
        contextWindow: 131072,
      },
      {
        id: "qwen-qwq-32b",
        name: "QwQ 32B",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
      },
    ],
    description: "Groq — ultra-fast LPU inference, requires GROQ_API_KEY",
  },

  // ─── China Providers ─────────────────────────────────────────────────────────
  {
    providerId: "kimi-coding",
    name: "Kimi (Moonshot)",
    websiteUrl: "https://platform.moonshot.cn",
    api: "openai-completions",
    baseUrl: "https://api.moonshot.cn/v1",
    apiKeyEnvVar: "KIMI_API_KEY",
    isBuiltin: true,
    category: "cn_provider",
    defaultModels: [
      {
        id: "kimi-k2.5",
        name: "Kimi K2.5",
        nameTouched: true,
        reasoning: true,
        contextWindow: 262144,
        maxTokens: 262144,
      },
    ],
    description: "Kimi For Coding — requires KIMI_API_KEY",
  },
  {
    providerId: "minimax",
    name: "MiniMax",
    websiteUrl: "https://www.minimaxi.com",
    api: "openai-completions",
    baseUrl: "https://api.minimax.chat/v1",
    apiKeyEnvVar: "MINIMAX_API_KEY",
    isBuiltin: true,
    category: "cn_provider",
    defaultModels: [
      {
        id: "MiniMax-M2.7",
        name: "MiniMax M2.7",
        nameTouched: true,
        contextWindow: 204800,
        maxTokens: 131072,
      },
    ],
    description: "MiniMax API — requires MINIMAX_API_KEY",
  },
  {
    providerId: "xiaomi",
    name: "Xiaomi MiMo",
    websiteUrl: "https://dev.mi.com",
    api: "openai-completions",
    baseUrl: "https://api.xiaomi.com/v1",
    apiKeyEnvVar: "XIAOMI_API_KEY",
    isBuiltin: true,
    category: "cn_provider",
    defaultModels: [
      {
        id: "MiMo-7B-RL",
        name: "MiMo 7B RL",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
      },
    ],
    description: "Xiaomi MiMo — requires XIAOMI_API_KEY",
  },

  // ─── Cloud Providers ─────────────────────────────────────────────────────────
  {
    providerId: "nvidia",
    name: "NVIDIA NIM",
    websiteUrl: "https://build.nvidia.com",
    api: "openai-completions",
    baseUrl: "https://integrate.api.nvidia.com/v1",
    apiKeyEnvVar: "NVIDIA_API_KEY",
    isBuiltin: true,
    category: "cloud",
    defaultModels: [
      {
        id: "nvidia/llama-3.3-nemotron-super-49b-v1",
        name: "Nemotron Super 49B",
        nameTouched: true,
        contextWindow: 131072,
      },
    ],
    description: "NVIDIA NIM — requires NVIDIA_API_KEY",
  },
  {
    providerId: "cerebras",
    name: "Cerebras",
    websiteUrl: "https://cloud.cerebras.ai",
    api: "openai-completions",
    baseUrl: "https://api.cerebras.ai/v1",
    apiKeyEnvVar: "CEREBRAS_API_KEY",
    isBuiltin: true,
    category: "cloud",
    defaultModels: [
      {
        id: "llama-4-scout-17b-16e-instruct",
        name: "Llama 4 Scout 17B",
        nameTouched: true,
        contextWindow: 131072,
      },
    ],
    description: "Cerebras — fast inference, requires CEREBRAS_API_KEY",
  },
  {
    providerId: "huggingface",
    name: "Hugging Face",
    websiteUrl: "https://huggingface.co/inference-api",
    api: "openai-completions",
    baseUrl: "https://router.huggingface.co/v1",
    apiKeyEnvVar: "HF_TOKEN",
    isBuiltin: true,
    category: "cloud",
    defaultModels: [
      {
        id: "Qwen/Qwen3-235B-A22B",
        name: "Qwen3 235B",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
      },
    ],
    description: "Hugging Face Inference — requires HF_TOKEN",
  },

  // ─── Local ──────────────────────────────────────────────────────────────────
  {
    providerId: "ollama",
    name: "Ollama",
    websiteUrl: "https://ollama.com",
    api: "openai-completions",
    baseUrl: "http://localhost:11434/v1",
    apiKeyEnvVar: "",
    isBuiltin: false,
    category: "local",
    defaultModels: [
      {
        id: "qwen3:32b",
        name: "Qwen3 32B",
        nameTouched: true,
        reasoning: true,
        contextWindow: 131072,
      },
      {
        id: "llama3.1:8b",
        name: "Llama 3.1 8B",
        nameTouched: true,
        contextWindow: 128000,
      },
    ],
    description: "Ollama — local inference (no API key needed)",
  },
  {
    providerId: "lmstudio",
    name: "LM Studio",
    websiteUrl: "https://lmstudio.ai",
    api: "openai-completions",
    baseUrl: "http://localhost:1234/v1",
    apiKeyEnvVar: "",
    isBuiltin: false,
    category: "local",
    defaultModels: [
      {
        id: "loaded-model",
        name: "Current Loaded Model",
        nameTouched: true,
        contextWindow: 128000,
      },
    ],
    description: "LM Studio — local inference (no API key needed)",
  },
  {
    providerId: "vllm",
    name: "vLLM",
    websiteUrl: "https://docs.vllm.ai",
    api: "openai-completions",
    baseUrl: "http://localhost:8000/v1",
    apiKeyEnvVar: "",
    isBuiltin: false,
    category: "local",
    defaultModels: [
      {
        id: "served-model",
        name: "Served Model",
        nameTouched: true,
        contextWindow: 128000,
      },
    ],
    description: "vLLM — high-throughput local serving (no API key needed)",
  },
];

/** Look up a vendor preset by provider ID */
export function findVendorPreset(
  providerId: string,
): PiVendorPreset | undefined {
  return piVendorPresets.find((p) => p.providerId === providerId);
}
