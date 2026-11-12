import {
  mcodeProviderPresets,
  type McodeProviderPreset,
} from "./mcodeProviderPresets";

/**
 * DevEco Code selects the AI SDK by `provider.npm`, not by an `api` enum —
 * `provider.api` is the endpoint URL. Values here are npm package names so the
 * protocol picker actually changes the SDK DevEco loads.
 */
export const DEVECO_NPM_PACKAGES = [
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/anthropic", label: "Anthropic Messages" },
  { value: "@ai-sdk/openai", label: "OpenAI Responses" },
] as const;

export type DevEcoNpmPackage = (typeof DEVECO_NPM_PACKAGES)[number]["value"];

const DEFAULT_NPM: DevEcoNpmPackage = "@ai-sdk/openai-compatible";

/** Mcode/Pi 的 `api` 枚举 → DevEco 的 SDK 包名。 */
const NPM_BY_API_FORMAT: Record<string, DevEcoNpmPackage> = {
  "anthropic-messages": "@ai-sdk/anthropic",
  "openai-completions": "@ai-sdk/openai-compatible",
  "openai-responses": "@ai-sdk/openai",
};

/**
 * 把 Mcode/Pi 风格的 `api` 枚举映射成 DevEco 的 SDK 包名。
 *
 * 旧配置与手改 JSON 里存的是 `api`（如 `anthropic-messages`）；用它选 npm 才能
 * 让协议切换真正生效，未识别的值退回默认的 openai-compatible。
 */
export function devecoNpmFromApiFormat(
  api: string | undefined,
): DevEcoNpmPackage {
  return NPM_BY_API_FORMAT[api ?? ""] ?? DEFAULT_NPM;
}

/** `api` 是 Mcode 的协议枚举（cc-switch 早期写入的形态），而不是 DevEco 的端点 URL。 */
export function isLegacyApiFormat(api: unknown): boolean {
  return typeof api === "string" && api in NPM_BY_API_FORMAT;
}

/**
 * 把存下来的 settings_config 归一化成 DevEco 形态。
 *
 * DevEco 是 OpenCode 分支，`npm` 决定 SDK；cc-switch 早期把协议写在 Mcode 的 `api`
 * 枚举里，那是错的（DevEco 的 `api` 是端点 URL）。因此只迁移「值为已知枚举」的
 * `api`——真正的 URL 原样保留，否则就会变成另一种配置丢失。
 */
export function normalizeDevecoConfig(
  stored: Record<string, unknown> | undefined,
): Record<string, unknown> | undefined {
  if (!stored) return undefined;
  const { api, ...rest } = stored;
  if (typeof stored.npm === "string") {
    // 已有 npm：只清理遗留的枚举值 api，真实 URL 保留。
    return isLegacyApiFormat(api) ? rest : stored;
  }
  if (isLegacyApiFormat(api)) {
    return { ...rest, npm: devecoNpmFromApiFormat(api as string) };
  }
  return stored;
}

export interface DevEcoProviderPreset
  extends Omit<McodeProviderPreset, "settingsConfig"> {
  settingsConfig: {
    name: string;
    npm: string;
    options: {
      baseURL: string;
      apiKey: string;
      headers?: Record<string, string>;
    };
    models: Record<string, import("@/types").OpenCodeModel>;
  };
}

// DevEco Code is an OpenCode fork: its provider schema is the OpenCode one, so
// presets must be translated into a real DevEco provider — `npm` carries the SDK
// choice and `api` (an URL in DevEco) is dropped rather than reused as an enum.
export const devecoProviderPresets: DevEcoProviderPreset[] =
  mcodeProviderPresets.map((preset) => ({
    ...preset,
    settingsConfig: {
      name: preset.settingsConfig.name,
      npm: devecoNpmFromApiFormat(preset.settingsConfig.api),
      options: {
        baseURL: preset.settingsConfig.options.baseURL,
        apiKey: preset.settingsConfig.options.apiKey,
        ...(preset.settingsConfig.options.headers
          ? { headers: preset.settingsConfig.options.headers }
          : {}),
      },
      models: preset.settingsConfig.models,
    },
  }));
