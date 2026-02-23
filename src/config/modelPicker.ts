/**
 * Model Picker Configuration for CC Switch
 * 
 * Predefined model lists for popular providers like OpenRouter.
 * Each provider has 4-5 recommended models for different use cases.
 */

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  description: string;
  contextWindow: number;
  maxOutputTokens: number;
  inputCostPerToken?: number;
  outputCostPerToken?: number;
  capabilities?: string[];
  recommendedFor?: string[];
}

export interface ProviderModelPicker {
  providerId: string;
  providerName: string;
  models: ModelInfo[];
  defaultModel: string;
}

// OpenRouter Model Picker
export const OPENROUTER_MODELS: ProviderModelPicker = {
  providerId: "openrouter",
  providerName: "OpenRouter",
  defaultModel: "anthropic/claude-3.5-sonnet",
  models: [
    {
      id: "anthropic/claude-3.5-sonnet",
      name: "Claude 3.5 Sonnet",
      provider: "Anthropic",
      description: "Best overall performance for most tasks",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000003,
      outputCostPerToken: 0.000015,
      capabilities: ["vision", "function_calling", "reasoning"],
      recommendedFor: ["coding", "chat", "analysis", "writing"],
    },
    {
      id: "openai/gpt-4o",
      name: "GPT-4o",
      provider: "OpenAI",
      description: "Fast and capable multimodal model",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000005,
      outputCostPerToken: 0.000015,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["chat", "vision", "multilingual"],
    },
    {
      id: "google/gemini-pro-1.5",
      name: "Gemini 1.5 Pro",
      provider: "Google",
      description: "Large context window with strong reasoning",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.0000035,
      outputCostPerToken: 0.0000105,
      capabilities: ["vision", "long_context"],
      recommendedFor: ["analysis", "summarization", "research"],
    },
    {
      id: "meta-llama/llama-3-70b-instruct",
      name: "Llama 3 70B",
      provider: "Meta",
      description: "Open source model with excellent performance",
      contextWindow: 8192,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.0000008,
      outputCostPerToken: 0.0000008,
      capabilities: ["function_calling"],
      recommendedFor: ["coding", "chat", "cost_effective"],
    },
    {
      id: "mistralai/mistral-large",
      name: "Mistral Large",
      provider: "Mistral",
      description: "European model with strong multilingual support",
      contextWindow: 32000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000004,
      outputCostPerToken: 0.000012,
      capabilities: ["function_calling", "multilingual"],
      recommendedFor: ["multilingual", "coding", "analysis"],
    },
  ],
};

// Anthropic Model Picker
export const ANTHROPIC_MODELS: ProviderModelPicker = {
  providerId: "anthropic",
  providerName: "Anthropic",
  defaultModel: "claude-sonnet-4-20250514",
  models: [
    {
      id: "claude-sonnet-4-20250514",
      name: "Claude Sonnet 4",
      provider: "Anthropic",
      description: "Latest Sonnet model with balanced performance",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000003,
      outputCostPerToken: 0.000015,
      capabilities: ["vision", "function_calling", "reasoning"],
      recommendedFor: ["coding", "chat", "analysis"],
    },
    {
      id: "claude-opus-4-20250514",
      name: "Claude Opus 4",
      provider: "Anthropic",
      description: "Most powerful model for complex tasks",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000015,
      outputCostPerToken: 0.000075,
      capabilities: ["vision", "function_calling", "reasoning"],
      recommendedFor: ["analysis", "research", "complex_tasks"],
    },
    {
      id: "claude-haiku-3-20240307",
      name: "Claude Haiku 3",
      provider: "Anthropic",
      description: "Fast and cost-effective for simple tasks",
      contextWindow: 200000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.00000025,
      outputCostPerToken: 0.00000125,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["chat", "cost_effective", "simple_tasks"],
    },
    {
      id: "claude-3-5-haiku-20241022",
      name: "Claude 3.5 Haiku",
      provider: "Anthropic",
      description: "Latest Haiku with improved capabilities",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000001,
      outputCostPerToken: 0.000005,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["chat", "cost_effective", "fast_response"],
    },
  ],
};

// OpenAI Model Picker
export const OPENAI_MODELS: ProviderModelPicker = {
  providerId: "openai",
  providerName: "OpenAI",
  defaultModel: "gpt-4.1",
  models: [
    {
      id: "gpt-4.1",
      name: "GPT-4.1",
      provider: "OpenAI",
      description: "Latest GPT-4 model with improved capabilities",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000005,
      outputCostPerToken: 0.000015,
      capabilities: ["vision", "function_calling", "reasoning"],
      recommendedFor: ["coding", "chat", "analysis"],
    },
    {
      id: "gpt-4o",
      name: "GPT-4o",
      provider: "OpenAI",
      description: "Fast multimodal model",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000005,
      outputCostPerToken: 0.000015,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["vision", "chat", "multilingual"],
    },
    {
      id: "gpt-4o-mini",
      name: "GPT-4o Mini",
      provider: "OpenAI",
      description: "Cost-effective with strong performance",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.00000015,
      outputCostPerToken: 0.0000006,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["chat", "cost_effective", "simple_tasks"],
    },
    {
      id: "o1-preview",
      name: "o1 Preview",
      provider: "OpenAI",
      description: "Reasoning model for complex problems",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000015,
      outputCostPerToken: 0.00006,
      capabilities: ["reasoning", "math", "science"],
      recommendedFor: ["reasoning", "math", "science", "complex_tasks"],
    },
  ],
};

// All model pickers
export const MODEL_PICKERS: Record<string, ProviderModelPicker> = {
  openrouter: OPENROUTER_MODELS,
  anthropic: ANTHROPIC_MODELS,
  openai: OPENAI_MODELS,
};

// Helper functions
export function getModelPicker(providerId: string): ProviderModelPicker | undefined {
  return MODEL_PICKERS[providerId];
}

export function getModelIds(providerId: string): string[] {
  const picker = getModelPicker(providerId);
  if (!picker) return [];
  return picker.models.map(m => m.id);
}

export function getDefaultModel(providerId: string): string | undefined {
  const picker = getModelPicker(providerId);
  return picker?.defaultModel;
}
