import type { FetchedModel } from "@/lib/api/model-fetch";
import type { CursorProviderType } from "@/lib/api/cursor";

export type ContextWindowSource = "provider" | "inferred" | "unknown";

export interface CursorModelMetadata {
  providerGroup: string;
  contextWindowTokens: number;
  contextWindowSource: ContextWindowSource;
}

const MODEL_CONTEXT_RULES: ReadonlyArray<readonly [RegExp, number]> = [
  [/\b(?:claude|sonnet|opus|haiku)[-_ ]?4(?:[.-]\d+)?\b/i, 200_000],
  [/\bclaude[-_ ]?3(?:[.-]5|[.-]7)?\b/i, 200_000],
  [/\bgpt[-_ ]?5(?:[.-]\d+)?\b/i, 400_000],
  [/\bgpt[-_ ]?4[.-]1\b/i, 1_000_000],
  [/\bgpt[-_ ]?4o\b/i, 128_000],
  [/\bo[134](?:[-_ ]|\b)/i, 200_000],
  [/\bgemini[-_ ]?(?:2[.-]5|3)(?:[-_ ]|\b)/i, 1_000_000],
  [/\bdeepseek[-_ ]?(?:v3|r1)\b/i, 128_000],
  [/\bglm[-_ ]?(?:4[.-]5|4[.-]6|5)(?:[-_ ]|\b)/i, 200_000],
  [/\bqwen3(?:[-_ ]|\b)/i, 262_144],
  [/\bkimi[-_ ]?k2(?:[-_ ]|\b)/i, 262_144],
  [/\bgrok[-_ ]?4(?:[.-]\d+)?\b/i, 256_000],
];

const HOST_GROUP_RULES: ReadonlyArray<readonly [RegExp, string]> = [
  [/anthropic\.com$/i, "Anthropic"],
  [/openai\.com$/i, "OpenAI"],
  [/openrouter\.ai$/i, "OpenRouter"],
  [/deepseek\.com$/i, "DeepSeek"],
  [/(?:bigmodel\.cn|z\.ai)$/i, "智谱 AI"],
  [/moonshot\.cn$/i, "Moonshot AI"],
  [/volces\.com$/i, "火山引擎"],
  [/aliyuncs\.com$/i, "阿里云百炼"],
  [/siliconflow\.cn$/i, "硅基流动"],
  [/x\.ai$/i, "xAI"],
  [/googleapis\.com$/i, "Google"],
];

export interface CursorEndpointGroup {
  key: string;
  label: string;
  baseUrl: string;
}

export const normalizeCursorEndpoint = (baseUrl: string): string => {
  const raw = baseUrl.trim();
  if (!raw) return "";
  try {
    const url = new URL(raw);
    url.hash = "";
    url.pathname = url.pathname.replace(/\/+$/, "") || "/";
    return url.toString().replace(/\/$/, "");
  } catch {
    return raw.replace(/\/+$/, "");
  }
};

export const inferEndpointLabel = (
  baseUrl: string,
  type: CursorProviderType,
): string => {
  try {
    const hostname = new URL(baseUrl).hostname;
    const group = HOST_GROUP_RULES.find(([pattern]) =>
      pattern.test(hostname),
    )?.[1];
    return group || hostname;
  } catch {
    return type === "anthropic" ? "Anthropic Compatible" : "OpenAI Compatible";
  }
};

export const resolveCursorEndpointGroup = (
  baseUrl: string,
  providerGroup: string | undefined,
  type: CursorProviderType,
): CursorEndpointGroup => ({
  key: normalizeCursorEndpoint(baseUrl),
  label: providerGroup?.trim() || inferEndpointLabel(baseUrl, type),
  baseUrl: baseUrl.trim(),
});

export const inferContextWindowTokens = (modelId: string): number =>
  MODEL_CONTEXT_RULES.find(([pattern]) => pattern.test(modelId))?.[1] ?? 0;

export const inferProviderGroup = (
  _model: Pick<FetchedModel, "ownedBy">,
  baseUrl: string,
  type: CursorProviderType,
): string => inferEndpointLabel(baseUrl, type);

export const resolveCursorModelMetadata = (
  model: FetchedModel,
  baseUrl: string,
  type: CursorProviderType,
): CursorModelMetadata => {
  const providerValue = model.contextWindowTokens ?? 0;
  const inferredValue = inferContextWindowTokens(model.id);
  return {
    providerGroup: inferProviderGroup(model, baseUrl, type),
    contextWindowTokens: providerValue || inferredValue,
    contextWindowSource: providerValue
      ? "provider"
      : inferredValue
        ? "inferred"
        : "unknown",
  };
};

export const formatTokenCount = (tokens: number): string => {
  if (tokens >= 1_000_000) {
    return `${Number((tokens / 1_000_000).toFixed(2))}M`;
  }
  if (tokens >= 1_000) {
    return `${Number((tokens / 1_000).toFixed(1))}K`;
  }
  return String(tokens);
};
