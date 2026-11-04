import type { AppId } from "@/lib/api";
import type { DimModel } from "@/lib/api/dim-oauth";
import type {
  ProviderMeta,
  OpenCodeModel,
  OpenClawProviderConfig,
} from "@/types";
import type { KimiProviderConfig } from "@/config/kimiProviderPresets";

export type DimApiFormat = NonNullable<ProviderMeta["apiFormat"]>;
export const DIM_PLACEHOLDER = "PROXY_MANAGED";
export const dimOAuthPreset = {
  name: "DimAgent",
  websiteUrl: "https://dimagent.com",
  providerKey: "dimagent",
  category: "third_party" as const,
  providerType: "dim_oauth" as const,
  requiresOAuth: true,
  icon: "dimagent",
  iconColor: "#64C878",
};
export function dimFormatsForApp(appId: AppId): readonly DimApiFormat[] {
  if (appId === "gemini" || appId === "agy") return ["gemini_native"];
  if (["codex", "grokbuild", "mcode", "dsh", "hermes"].includes(appId))
    return ["openai_chat", "openai_responses", "anthropic"];
  return ["openai_chat", "openai_responses", "anthropic", "gemini_native"];
}
export const dimNativeRouting = (appId: AppId) =>
  ["claude", "claude-desktop", "codex", "gemini", "grokbuild"].includes(appId);
export const dimBaseUrl = (issuer: string, format: DimApiFormat) =>
  issuer +
  (format === "gemini_native"
    ? "/v1beta"
    : format === "anthropic"
      ? ""
      : "/v1");
export const dimPiApi = {
  openai_chat: "openai-completions",
  openai_responses: "openai-responses",
  anthropic: "anthropic-messages",
  gemini_native: "google-generative-ai",
} as const;
export const dimSdk = {
  openai_chat: "@ai-sdk/openai-compatible",
  openai_responses: "@ai-sdk/openai",
  anthropic: "@ai-sdk/anthropic",
  gemini_native: "@ai-sdk/google",
} as const;
export function dimOpenCodeModels(
  model: DimModel,
): Record<string, OpenCodeModel> {
  return {
    [model.id]: {
      name: model.name,
      ...(model.context_window != null || model.max_output_tokens != null
        ? {
            limit: {
              ...(model.context_window != null
                ? { context: model.context_window }
                : {}),
              ...(model.max_output_tokens != null
                ? { output: model.max_output_tokens }
                : {}),
            },
          }
        : {}),
    },
  };
}
export function dimMcodeConfig(
  model: DimModel,
  format: DimApiFormat,
  issuer: string,
) {
  return {
    name: "DimAgent",
    kind: "custom",
    enabled: true,
    api: dimPiApi[format],
    options: { baseURL: dimBaseUrl(issuer, format), apiKey: DIM_PLACEHOLDER },
    models: dimOpenCodeModels(model),
  };
}
export function dimPiConfig(
  model: DimModel,
  format: DimApiFormat,
  issuer: string,
  omp = false,
) {
  return {
    ...(!omp ? { name: "DimAgent" } : {}),
    baseUrl: dimBaseUrl(issuer, format),
    api: dimPiApi[format],
    apiKey: DIM_PLACEHOLDER,
    models: [
      {
        id: model.id,
        name: model.name,
        ...(model.context_window != null
          ? { contextWindow: model.context_window }
          : {}),
        ...(model.max_output_tokens != null
          ? { maxTokens: model.max_output_tokens }
          : {}),
      },
    ],
  };
}
export function dimKimiConfig(
  model: DimModel,
  format: DimApiFormat,
  issuer: string,
  providerKey: string,
): KimiProviderConfig {
  if (model.context_window == null || model.context_window <= 0)
    throw new Error(
      "DimAgent model requires a declared context window for Kimi Code",
    );
  const types = {
    openai_chat: "openai",
    openai_responses: "openai_responses",
    anthropic: "anthropic",
    gemini_native: "google-genai",
  } as const;
  return {
    provider: {
      type: types[format],
      base_url:
        format === "gemini_native" ? issuer : dimBaseUrl(issuer, format),
      api_key: DIM_PLACEHOLDER,
    },
    models: {
      [`${providerKey || "custom"}/${model.id}`]: {
        model: model.id,
        display_name: model.name,
        max_context_size: model.context_window,
      },
    },
  };
}
export function dimOpenClawConfig(
  model: DimModel,
  format: DimApiFormat,
  issuer: string,
): OpenClawProviderConfig {
  return {
    baseUrl: dimBaseUrl(issuer, format),
    apiKey: DIM_PLACEHOLDER,
    api: dimPiApi[format],
    models: [
      {
        id: model.id,
        name: model.name,
        ...(model.context_window != null
          ? { contextWindow: model.context_window }
          : {}),
        ...(model.max_output_tokens != null
          ? { maxTokens: model.max_output_tokens }
          : {}),
      },
    ],
  };
}
