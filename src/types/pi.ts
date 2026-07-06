export type PiProviderMode = "custom" | "builtinOverride";

export type PiProviderTemplate =
  | "openAiCompatible"
  | "openAiResponses"
  | "anthropicCompatible"
  | "googleGenerativeAi"
  | "localOpenAiCompatible"
  | "custom";

export type PiApiType =
  | "openai-completions"
  | "openai-responses"
  | "anthropic-messages"
  | "google-generative-ai";

export type PiApiKeyMode = "env" | "literal" | "command" | "none";

export interface PiApiKeyDraft {
  mode: PiApiKeyMode;
  value: string;
}

export interface PiHeaderDraft {
  key: string;
  value: string;
}

export interface PiModelCost {
  input: number;
  output: number;
  cacheRead?: number;
  cacheWrite?: number;
}

export interface PiModelDraft {
  id: string;
  name?: string | null;
  nameTouched: boolean;
  reasoning?: boolean;
  input?: string[];
  contextWindow?: number;
  maxTokens?: number;
  cost?: PiModelCost;
}

/** OpenAI-compatible compat flags */
export interface PiOpenAiCompat {
  supportsDeveloperRole?: boolean;
  supportsReasoningEffort?: boolean;
  supportsUsageInStreaming?: boolean;
  maxTokensField?: "max_completion_tokens" | "max_tokens";
  thinkingFormat?: string;
}

/** Anthropic-compatible compat flags */
export interface PiAnthropicCompat {
  supportsEagerToolInputStreaming?: boolean;
  supportsLongCacheRetention?: boolean;
  forceAdaptiveThinking?: boolean;
  allowEmptySignature?: boolean;
}

export type PiProviderCompat = PiOpenAiCompat & PiAnthropicCompat;

export interface PiProviderDraft {
  mode: PiProviderMode;
  providerId: string;
  template: PiProviderTemplate;
  baseUrl?: string | null;
  api: string;
  apiKey: PiApiKeyDraft;
  headers: PiHeaderDraft[];
  models: PiModelDraft[];
  compat?: PiProviderCompat | null;
  advancedJson?: Record<string, unknown> | null;
}

export interface PiProviderPatchPreview {
  nextModelsJson: Record<string, unknown>;
  currentFileHash: string;
  summary: string[];
}

export interface PiProviderApplyResult {
  fileHash: string;
  modelsJson: Record<string, unknown>;
  backupPath: string;
}

export interface PiConnectivityResult {
  reachable: boolean;
  statusCode?: number;
  errorKind?: string;
  detail?: string;
}

export type PiProvidersMap = Record<string, Record<string, unknown>>;
