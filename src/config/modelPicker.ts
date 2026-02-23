/**
 * Model Picker Configuration for CC Switch
 * 
 * Predefined model lists for popular providers.
 * Each provider has exactly 5 recommended models for different use cases.
 * Compatible with Tauri backend integration.
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
  models: ModelInfo[];  // Exactly 5 models
  defaultModel: string;
}

// ============================================================================
// OpenRouter - 5 Models
// ============================================================================
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
      description: "Largest context window with strong reasoning",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.0000035,
      outputCostPerToken: 0.0000105,
      capabilities: ["vision", "long_context"],
      recommendedFor: ["analysis", "summarization", "research"],
    },
    {
      id: "meta-llama/llama-3.1-405b-instruct",
      name: "Llama 3.1 405B",
      provider: "Meta",
      description: "Most powerful open source model",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000005,
      outputCostPerToken: 0.000005,
      capabilities: ["function_calling", "reasoning"],
      recommendedFor: ["coding", "analysis", "open_source"],
    },
    {
      id: "mistralai/mistral-large-2411",
      name: "Mistral Large",
      provider: "Mistral",
      description: "European model with multilingual excellence",
      contextWindow: 128000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000004,
      outputCostPerToken: 0.000012,
      capabilities: ["function_calling", "multilingual"],
      recommendedFor: ["multilingual", "coding", "european"],
    },
  ],
};

// ============================================================================
// Anthropic - 5 Models
// ============================================================================
export const ANTHROPIC_MODELS: ProviderModelPicker = {
  providerId: "anthropic",
  providerName: "Anthropic",
  defaultModel: "claude-sonnet-4-20250514",
  models: [
    {
      id: "claude-sonnet-4-20250514",
      name: "Claude Sonnet 4",
      provider: "Anthropic",
      description: "Latest Sonnet with balanced performance",
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
      description: "Most powerful for complex tasks",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000015,
      outputCostPerToken: 0.000075,
      capabilities: ["vision", "function_calling", "reasoning"],
      recommendedFor: ["analysis", "research", "complex_tasks"],
    },
    {
      id: "claude-3-5-haiku-20241022",
      name: "Claude 3.5 Haiku",
      provider: "Anthropic",
      description: "Fast and cost-effective",
      contextWindow: 200000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000001,
      outputCostPerToken: 0.000005,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["chat", "cost_effective", "fast_response"],
    },
    {
      id: "claude-3-haiku-20240307",
      name: "Claude Haiku 3",
      provider: "Anthropic",
      description: "Ultra-fast for simple tasks",
      contextWindow: 200000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.00000025,
      outputCostPerToken: 0.00000125,
      capabilities: ["vision", "function_calling"],
      recommendedFor: ["chat", "simple_tasks", "batch_processing"],
    },
    {
      id: "claude-3-opus-20240229",
      name: "Claude Opus 3",
      provider: "Anthropic",
      description: "Previous generation flagship",
      contextWindow: 200000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000015,
      outputCostPerToken: 0.000075,
      capabilities: ["vision", "function_calling", "reasoning"],
      recommendedFor: ["analysis", "legacy_support"],
    },
  ],
};

// ============================================================================
// OpenAI - 5 Models
// ============================================================================
export const OPENAI_MODELS: ProviderModelPicker = {
  providerId: "openai",
  providerName: "OpenAI",
  defaultModel: "gpt-4.1",
  models: [
    {
      id: "gpt-4.1",
      name: "GPT-4.1",
      provider: "OpenAI",
      description: "Latest GPT-4 with improved capabilities",
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
      description: "Fast multimodal flagship",
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
      recommendedFor: ["chat", "cost_effective", "high_volume"],
    },
    {
      id: "o1",
      name: "o1",
      provider: "OpenAI",
      description: "Advanced reasoning model",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.000015,
      outputCostPerToken: 0.00006,
      capabilities: ["reasoning", "math", "science"],
      recommendedFor: ["reasoning", "math", "science", "complex_tasks"],
    },
    {
      id: "o3-mini",
      name: "o3 Mini",
      provider: "OpenAI",
      description: "Fast reasoning model",
      contextWindow: 128000,
      maxOutputTokens: 4096,
      inputCostPerToken: 0.0000011,
      outputCostPerToken: 0.0000044,
      capabilities: ["reasoning", "coding"],
      recommendedFor: ["coding", "reasoning", "cost_effective"],
    },
  ],
};

// ============================================================================
// Google - 5 Models
// ============================================================================
export const GOOGLE_MODELS: ProviderModelPicker = {
  providerId: "google",
  providerName: "Google",
  defaultModel: "gemini-2.5-pro",
  models: [
    {
      id: "gemini-2.5-pro",
      name: "Gemini 2.5 Pro",
      provider: "Google",
      description: "Latest Pro model with advanced reasoning",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.0000035,
      outputCostPerToken: 0.0000105,
      capabilities: ["vision", "long_context", "reasoning"],
      recommendedFor: ["analysis", "research", "multimodal"],
    },
    {
      id: "gemini-2.5-flash",
      name: "Gemini 2.5 Flash",
      provider: "Google",
      description: "Fast and efficient model",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.0000003,
      outputCostPerToken: 0.0000012,
      capabilities: ["vision", "long_context"],
      recommendedFor: ["chat", "cost_effective", "high_volume"],
    },
    {
      id: "gemini-2.0-flash",
      name: "Gemini 2.0 Flash",
      provider: "Google",
      description: "Previous generation fast model",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.00000015,
      outputCostPerToken: 0.0000006,
      capabilities: ["vision"],
      recommendedFor: ["chat", "legacy_support"],
    },
    {
      id: "gemini-2.0-flash-lite",
      name: "Gemini 2.0 Flash Lite",
      provider: "Google",
      description: "Most cost-effective Google model",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.000000075,
      outputCostPerToken: 0.0000003,
      capabilities: ["vision"],
      recommendedFor: ["cost_effective", "batch_processing"],
    },
    {
      id: "gemini-1.5-pro",
      name: "Gemini 1.5 Pro",
      provider: "Google",
      description: "Previous Pro with large context",
      contextWindow: 1048576,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.0000035,
      outputCostPerToken: 0.0000105,
      capabilities: ["vision", "long_context"],
      recommendedFor: ["analysis", "legacy_support"],
    },
  ],
};

// ============================================================================
// DeepSeek - 5 Models
// ============================================================================
export const DEEPSEEK_MODELS: ProviderModelPicker = {
  providerId: "deepseek",
  providerName: "DeepSeek",
  defaultModel: "deepseek-chat",
  models: [
    {
      id: "deepseek-chat",
      name: "DeepSeek Chat",
      provider: "DeepSeek",
      description: "Best for conversational tasks",
      contextWindow: 128000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.00000027,
      outputCostPerToken: 0.0000011,
      capabilities: ["function_calling", "coding"],
      recommendedFor: ["chat", "coding", "cost_effective"],
    },
    {
      id: "deepseek-coder",
      name: "DeepSeek Coder",
      provider: "DeepSeek",
      description: "Specialized for code generation",
      contextWindow: 128000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.00000027,
      outputCostPerToken: 0.0000011,
      capabilities: ["coding", "function_calling"],
      recommendedFor: ["coding", "code_review", "debugging"],
    },
    {
      id: "deepseek-reasoner",
      name: "DeepSeek Reasoner",
      provider: "DeepSeek",
      description: "Enhanced reasoning capabilities",
      contextWindow: 64000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.00000055,
      outputCostPerToken: 0.0000022,
      capabilities: ["reasoning", "math"],
      recommendedFor: ["reasoning", "math", "analysis"],
    },
    {
      id: "deepseek-v3",
      name: "DeepSeek V3",
      provider: "DeepSeek",
      description: "Latest general purpose model",
      contextWindow: 128000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.00000027,
      outputCostPerToken: 0.0000011,
      capabilities: ["function_calling", "vision"],
      recommendedFor: ["chat", "analysis", "multimodal"],
    },
    {
      id: "deepseek-v2.5",
      name: "DeepSeek V2.5",
      provider: "DeepSeek",
      description: "Previous generation balanced model",
      contextWindow: 128000,
      maxOutputTokens: 8192,
      inputCostPerToken: 0.00000014,
      outputCostPerToken: 0.00000056,
      capabilities: ["function_calling"],
      recommendedFor: ["cost_effective", "legacy_support"],
    },
  ],
};

// ============================================================================
// All Model Pickers
// ============================================================================
export const MODEL_PICKERS: Record<string, ProviderModelPicker> = {
  openrouter: OPENROUTER_MODELS,
  anthropic: ANTHROPIC_MODELS,
  openai: OPENAI_MODELS,
  google: GOOGLE_MODELS,
  deepseek: DEEPSEEK_MODELS,
};

// ============================================================================
// Helper Functions
// ============================================================================

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

/**
 * Get models as array for Tauri/JSON serialization.
 * Returns exactly 5 models.
 */
export function getModelsArray(providerId: string): ModelInfo[] {
  const picker = getModelPicker(providerId);
  if (!picker) return [];
  return picker.models;
}

/**
 * Get all providers info for Tauri.
 */
export function getAllProvidersInfo(): Array<{
  providerId: string;
  providerName: string;
  modelCount: number;
  defaultModel: string;
}> {
  return Object.values(MODEL_PICKERS).map(picker => ({
    providerId: picker.providerId,
    providerName: picker.providerName,
    modelCount: picker.models.length,
    defaultModel: picker.defaultModel,
  }));
}
