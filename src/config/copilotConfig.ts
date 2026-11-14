/**
 * GitHub Copilot CLI 卡片的配置形状（与后端 `copilot_config.rs` 严格对应）。
 *
 * - 官方卡：`{ env: { COPILOT_MODEL? }, reasoningEffort? }`，只用于「打开终端」，不写任何文件。
 * - 自带模型（BYOK）卡：`{ provider, models, reasoningEffort? }`。「添加」把它写进 Copilot 原生的
 *   `providers.json`：`provider` 补上 `name = 卡片 id` 成为一个供应商条目，`models` 每项补上
 *   `provider = 卡片 id`；Copilot 的 `/model` 里显示为 `<卡片 id>/<模型 id>`。`reasoningEffort`
 *   是卡片自己的字段，只给「打开终端」的 `--reasoning-effort` 用，不进 providers.json。
 *
 * 这个文件不引用预设目录，供卡片、额度、终端等轻量地判断卡片类型。
 */

export const COPILOT_PROVIDER_TYPES = ["openai", "azure", "anthropic"] as const;
export type CopilotProviderType = (typeof COPILOT_PROVIDER_TYPES)[number];
export const COPILOT_WIRE_APIS = ["completions", "responses"] as const;
export type CopilotWireApi = (typeof COPILOT_WIRE_APIS)[number];

/** providers.json 的供应商条目去掉 `name`（name 永远是卡片 id）。未列出的键原样保留。 */
export interface CopilotProviderNode {
  type?: CopilotProviderType;
  wireApi?: CopilotWireApi;
  transport?: "http" | "websockets";
  baseUrl: string;
  apiKey?: string;
  bearerToken?: string;
  azure?: { apiVersion?: string } & Record<string, unknown>;
  headers?: Record<string, string>;
  [key: string]: unknown;
}

/** providers.json 的模型条目去掉 `provider`（写文件时补上卡片 id）。未列出的键原样保留。 */
export interface CopilotModel {
  id: string;
  name?: string;
  wireModel?: string;
  maxPromptTokens?: number;
  maxContextWindowTokens?: number;
  maxOutputTokens?: number;
  [key: string]: unknown;
}

/** Copilot reserves output tokens before building the prompt. Let it choose its
 * default when a catalog advertises the whole context window as output. */
export function copilotCatalogOutputLimit(
  output: number | undefined,
  context: number | undefined,
): number | undefined {
  return output && output > 0 && (!context || output < context)
    ? output
    : undefined;
}

export interface CopilotByokConfig {
  provider: CopilotProviderNode;
  models: CopilotModel[];
  reasoningEffort?: string;
}

/** Copilot CLI `--reasoning-effort` 的取值（与后端 `REASONING_EFFORTS` 一致）。 */
export const COPILOT_REASONING_EFFORTS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

/**
 * 卡片 id 就是 providers.json 里的供应商名：非空、不含 `/`、空白和控制字符，不超过 64 字符。
 * 表单只让输入 `[A-Za-z0-9._-]`，比后端的规则更严。
 */
export const COPILOT_PROVIDER_ID_MAX = 64;
export function isValidCopilotProviderId(id: string): boolean {
  return (
    id.length > 0 &&
    id.length <= COPILOT_PROVIDER_ID_MAX &&
    !/[/\s\p{Cc}]/u.test(id)
  );
}
export const normalizeCopilotProviderId = (value: string) =>
  value.replace(/[^A-Za-z0-9._-]/g, "").slice(0, COPILOT_PROVIDER_ID_MAX);

const isPlainObject = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === "object" && !Array.isArray(value);

/** PR #8753 期间的旧形状：自带模型全在 `env` 的 `COPILOT_PROVIDER_*` 里。 */
export const COPILOT_LEGACY_ENV = {
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

/** 卡片 `env` 里的字符串值（官方卡的 `COPILOT_MODEL`、旧形状的 BYOK 变量）。 */
export function copilotEnvOf(settingsConfig: unknown): Record<string, string> {
  const env = isPlainObject(settingsConfig) ? settingsConfig.env : undefined;
  if (!isPlainObject(env)) return {};
  return Object.fromEntries(
    Object.entries(env).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  );
}

function copilotProviderNodeOf(
  settingsConfig: unknown,
): CopilotProviderNode | null {
  const provider = isPlainObject(settingsConfig)
    ? settingsConfig.provider
    : undefined;
  return isPlainObject(provider) ? (provider as CopilotProviderNode) : null;
}

/** 有 `provider` 节点就是自带模型卡；旧形状按 `COPILOT_PROVIDER_BASE_URL` 认（只为显示）。 */
export function isCopilotByok(settingsConfig: unknown): boolean {
  if (copilotProviderNodeOf(settingsConfig)) return true;
  return Boolean(
    copilotEnvOf(settingsConfig)[COPILOT_LEGACY_ENV.baseUrl]?.trim(),
  );
}

/** 官方卡：category 是 official，或者配置里没有自带模型。 */
export function isCopilotOfficialCard(provider: {
  category?: string;
  settingsConfig: unknown;
}): boolean {
  return (
    provider.category === "official" || !isCopilotByok(provider.settingsConfig)
  );
}

export function reasoningEffortOf(settingsConfig: unknown): string {
  const value = isPlainObject(settingsConfig)
    ? settingsConfig.reasoningEffort
    : undefined;
  return typeof value === "string" &&
    (COPILOT_REASONING_EFFORTS as readonly string[]).includes(value.trim())
    ? value.trim()
    : "";
}

/** 旧形状 `COPILOT_PROVIDER_HEADERS` 的「名称: 值」行文本。 */
function parseLegacyHeaders(text: string | undefined): Record<string, string> {
  const headers: Record<string, string> = {};
  for (const line of (text ?? "").split(/\r?\n|\\n/)) {
    const index = line.indexOf(":");
    if (index <= 0) continue;
    const name = line.slice(0, index).trim();
    if (name) headers[name] = line.slice(index + 1).trim();
  }
  return headers;
}

const positiveNumber = (value: string | undefined) => {
  const number = Number(value?.trim());
  return Number.isFinite(number) && number > 0 ? number : undefined;
};

/** 旧 env 形状换成新形状（与后端 list 时的就地转换一致），表单打开旧行时用。 */
function legacyByokConfig(env: Record<string, string>): CopilotByokConfig {
  const value = (key: keyof typeof COPILOT_LEGACY_ENV) =>
    env[COPILOT_LEGACY_ENV[key]]?.trim() || undefined;
  const provider: CopilotProviderNode = { baseUrl: value("baseUrl") ?? "" };
  const type = value("type");
  if (type) provider.type = type as CopilotProviderType;
  const wireApi = value("wireApi");
  if (wireApi) provider.wireApi = wireApi as CopilotWireApi;
  const transport = value("transport");
  if (transport) provider.transport = transport as "http" | "websockets";
  const apiKey = value("apiKey");
  if (apiKey) provider.apiKey = apiKey;
  const bearerToken = value("bearerToken");
  if (bearerToken) provider.bearerToken = bearerToken;
  const azureApiVersion = value("azureApiVersion");
  if (azureApiVersion) provider.azure = { apiVersion: azureApiVersion };
  const headers = parseLegacyHeaders(env[COPILOT_LEGACY_ENV.headers]);
  if (Object.keys(headers).length) provider.headers = headers;

  const wireModel = value("wireModel");
  const modelId = value("modelId");
  const id = value("model") ?? modelId ?? wireModel ?? "";
  const model: CopilotModel = { id };
  if (wireModel && wireModel !== id) model.wireModel = wireModel;
  if (modelId && modelId !== id) model.modelId = modelId;
  const maxPromptTokens = positiveNumber(value("maxPromptTokens"));
  if (maxPromptTokens) model.maxPromptTokens = maxPromptTokens;
  const maxOutputTokens = positiveNumber(value("maxOutputTokens"));
  if (maxOutputTokens) model.maxOutputTokens = maxOutputTokens;
  return { provider, models: id ? [model] : [] };
}

/** 读出自带模型卡的配置；不是自带模型卡时给一份空白配置。 */
export function copilotByokConfigOf(
  settingsConfig: unknown,
): CopilotByokConfig {
  const reasoningEffort = reasoningEffortOf(settingsConfig);
  const node = copilotProviderNodeOf(settingsConfig);
  let config: CopilotByokConfig;
  if (node) {
    const models = isPlainObject(settingsConfig)
      ? settingsConfig.models
      : undefined;
    config = {
      provider: {
        ...node,
        baseUrl: typeof node.baseUrl === "string" ? node.baseUrl : "",
      },
      models: Array.isArray(models)
        ? models.filter(isPlainObject).map((model) => ({
            ...model,
            id: typeof model.id === "string" ? model.id : "",
          }))
        : [],
    };
  } else if (isCopilotByok(settingsConfig)) {
    config = legacyByokConfig(copilotEnvOf(settingsConfig));
  } else {
    config = emptyCopilotByokConfig();
  }
  return reasoningEffort ? { ...config, reasoningEffort } : config;
}

export function emptyCopilotByokConfig(): CopilotByokConfig {
  return {
    provider: { type: "openai", wireApi: "completions", baseUrl: "" },
    models: [],
  };
}

/**
 * 表单里的「接口格式」：一项同时决定 `type` 和 `wireApi`。Anthropic 只有 Messages 接口，
 * 不写 `wireApi`。
 */
export const COPILOT_API_FORMATS = [
  {
    id: "openai-completions",
    label: "OpenAI Chat Completions",
    type: "openai",
    wireApi: "completions",
  },
  {
    id: "openai-responses",
    label: "OpenAI Responses",
    type: "openai",
    wireApi: "responses",
  },
  { id: "anthropic", label: "Anthropic Messages", type: "anthropic" },
  {
    id: "azure-completions",
    label: "Azure OpenAI Chat Completions",
    type: "azure",
    wireApi: "completions",
  },
  {
    id: "azure-responses",
    label: "Azure OpenAI Responses",
    type: "azure",
    wireApi: "responses",
  },
] as const satisfies ReadonlyArray<{
  id: string;
  label: string;
  type: CopilotProviderType;
  wireApi?: CopilotWireApi;
}>;
export type CopilotApiFormatId = (typeof COPILOT_API_FORMATS)[number]["id"];

/** 按 Copilot CLI 的默认值（openai / completions）读出当前格式。 */
export function copilotApiFormatOf(
  provider: Pick<CopilotProviderNode, "type" | "wireApi">,
): CopilotApiFormatId {
  const type = provider.type || "openai";
  if (type === "anthropic") return "anthropic";
  const responses = provider.wireApi === "responses";
  if (type === "azure")
    return responses ? "azure-responses" : "azure-completions";
  return responses ? "openai-responses" : "openai-completions";
}

/**
 * 换接口格式：同时改 `type` / `wireApi`；离开 Azure 时去掉 `azure`，离开 Responses 时去掉
 * websockets 传输（Copilot 只允许 websockets 配 responses，否则整个文件被拒）。
 */
export function applyCopilotApiFormat(
  provider: CopilotProviderNode,
  id: CopilotApiFormatId,
): CopilotProviderNode {
  const format = COPILOT_API_FORMATS.find((item) => item.id === id);
  if (!format) return provider;
  const next: CopilotProviderNode = { ...provider, type: format.type };
  if ("wireApi" in format) next.wireApi = format.wireApi;
  else delete next.wireApi;
  if (format.type !== "azure") delete next.azure;
  if (next.wireApi !== "responses" && next.transport === "websockets") {
    delete next.transport;
  }
  return next;
}
