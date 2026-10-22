import { piProviderPresets, type PiProviderPreset } from "./piProviderPresets";

/**
 * GitHub Copilot CLI 只从进程环境读自带模型（BYOK）：`COPILOT_PROVIDER_*` 和 `COPILOT_MODEL`。
 * 卡片里存的就是这些环境变量，「打开终端」时只交给那一个 copilot 进程。
 */
export type CopilotEnv = Record<string, string>;

export interface CopilotSettingsConfig {
  env: CopilotEnv;
}

export interface CopilotProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: CopilotSettingsConfig;
  /** 预设已知的模型 ID，供模型输入框下拉 */
  models?: string[];
}

export const COPILOT_ENV = {
  baseUrl: "COPILOT_PROVIDER_BASE_URL",
  type: "COPILOT_PROVIDER_TYPE",
  apiKey: "COPILOT_PROVIDER_API_KEY",
  bearerToken: "COPILOT_PROVIDER_BEARER_TOKEN",
  wireApi: "COPILOT_PROVIDER_WIRE_API",
  transport: "COPILOT_PROVIDER_TRANSPORT",
  azureApiVersion: "COPILOT_PROVIDER_AZURE_API_VERSION",
  modelId: "COPILOT_PROVIDER_MODEL_ID",
  wireModel: "COPILOT_PROVIDER_WIRE_MODEL",
  maxPromptTokens: "COPILOT_PROVIDER_MAX_PROMPT_TOKENS",
  maxOutputTokens: "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS",
  headers: "COPILOT_PROVIDER_HEADERS",
  model: "COPILOT_MODEL",
} as const;

export const COPILOT_PROVIDER_TYPES = ["openai", "azure", "anthropic"] as const;
export type CopilotProviderType = (typeof COPILOT_PROVIDER_TYPES)[number];
export const COPILOT_WIRE_APIS = ["completions", "responses"] as const;
export type CopilotWireApi = (typeof COPILOT_WIRE_APIS)[number];

export const COPILOT_OFFICIAL_PRESET_ID = "copilot-official";

/** 用 Copilot CLI 自己的 GitHub 登录；模型可留空（用 Copilot 的默认 / `/model` 选择）。 */
export const copilotOfficialPreset: CopilotProviderPreset = {
  name: "GitHub Copilot",
  providerKey: COPILOT_OFFICIAL_PRESET_ID,
  websiteUrl: "https://github.com/features/copilot/cli",
  icon: "githubcopilot",
  iconColor: "currentColor",
  category: "official",
  settingsConfig: { env: {} },
  models: [
    "auto",
    "claude-sonnet-4.6",
    "claude-opus-4.8",
    "claude-haiku-4.5",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5-mini",
  ],
};

/** Pi 的接口格式 → Copilot CLI 的协议与接口。Gemini、Bedrock 等没有对应协议，不提供预设。 */
function copilotProtocol(
  api: PiProviderPreset["settingsConfig"]["api"],
): CopilotEnv | null {
  switch (api) {
    case "openai-completions":
      return {
        [COPILOT_ENV.type]: "openai",
        [COPILOT_ENV.wireApi]: "completions",
      };
    case "openai-responses":
      return {
        [COPILOT_ENV.type]: "openai",
        [COPILOT_ENV.wireApi]: "responses",
      };
    case "anthropic-messages":
      return { [COPILOT_ENV.type]: "anthropic" };
    default:
      return null;
  }
}

export function formatCopilotHeaders(headers?: Record<string, string>) {
  return Object.entries(headers ?? {})
    .map(([name, value]) => `${name}: ${value}`)
    .join("\n");
}

// 复用 Pi 已维护的供应商目录（分组、合作信息、地址、模型），换算成 Copilot 的环境变量。
export const copilotProviderPresets: CopilotProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const { settingsConfig: source, ...identity } = preset;
    const protocol = copilotProtocol(source.api);
    if (!protocol || !source.models.length || !source.baseUrl) return [];
    const env: CopilotEnv = {
      [COPILOT_ENV.baseUrl]: source.baseUrl,
      ...protocol,
      [COPILOT_ENV.apiKey]:
        source.apiKey || (preset.category === "local" ? "local" : ""),
      [COPILOT_ENV.model]: source.models[0].id,
    };
    const headers = formatCopilotHeaders(source.headers);
    if (headers) env[COPILOT_ENV.headers] = headers;
    return [
      {
        ...identity,
        settingsConfig: { env },
        models: source.models.map((model) => model.id),
      },
    ];
  });

export function copilotEnvOf(settingsConfig: unknown): CopilotEnv {
  const env = (settingsConfig as { env?: unknown } | undefined)?.env;
  if (!env || typeof env !== "object" || Array.isArray(env)) return {};
  return Object.fromEntries(
    Object.entries(env as Record<string, unknown>).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  );
}

/** 设了地址才是自带模型（和 Copilot CLI 的判断一致）。 */
export function isCopilotByok(settingsConfig: unknown): boolean {
  return Boolean(copilotEnvOf(settingsConfig)[COPILOT_ENV.baseUrl]?.trim());
}

/** 只留非空值，保存时不写空的环境变量。 */
export function compactCopilotEnv(env: CopilotEnv): CopilotEnv {
  return Object.fromEntries(
    Object.entries(env)
      .map(([key, value]) => [key, value.trim()] as const)
      .filter(([, value]) => value.length > 0),
  );
}
