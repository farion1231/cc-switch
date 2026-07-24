import type {
  CodexApiFormat,
  CodexCacheConfig,
  CodexCatalogModel,
  CodexModelCatalogConfig,
  CodexRoutingConfig,
  CodexRoutingRoute,
  Provider,
} from "@/types";
import type { FetchedModel } from "@/lib/api/model-fetch";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import {
  codexPlanModelListAction,
  isCodexCatalogOnlyPlanModelFetch,
  isCodexVolcengineAgentPlanModelFetch,
} from "@/utils/codexPlanModelFetch";

export const CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY =
  "ccswitchmulti.codexMultiRouterWizard.dismissed";
export const CODEX_MULTI_ROUTER_DEFAULT_ID = "codex-multirouter";
export const CODEX_MULTI_ROUTER_DEFAULT_NAME = "Codex MultiRouter";
export const CODEX_MULTI_ROUTER_PROXY_BASE_URL = "http://127.0.0.1:15721/v1";

export interface WizardModelFetchConfig {
  baseUrl: string;
  apiKey: string;
  isFullUrl?: boolean;
  modelsUrl?: string;
  customUserAgent?: string;
  volcengineModelListAction?: string;
  volcengineAccessKeyId?: string;
  volcengineSecretAccessKey?: string;
}

export interface WizardPlanBuildResult {
  plan: Provider;
  sourceProviders: Provider[];
}

export interface WizardPlanBuildOptions {
  planName?: string;
  catalogModelOrder?: string[];
  spawnAgentModels?: string[];
}

export interface WizardConfigIssue {
  providerId: string;
  providerName: string;
  reason: string;
}

export interface WizardModelNameCollision {
  upstreamModel: string;
  providerIds: string[];
  canonicalProviderIds: string[];
}

export type WizardConnectivityStatus = "pass" | "warn" | "fail" | "skipped";

export interface WizardConnectivityResult {
  providerId: string;
  providerName: string;
  model: string;
  status: WizardConnectivityStatus;
  canContinue: boolean;
  detail: string;
  recommendedApiFormat?: CodexApiFormat;
  url?: string;
  httpStatus?: number | null;
}

export interface WizardProtocolProbe {
  ok: boolean;
  detail: string;
  url?: string;
  httpStatus?: number | null;
}

// 读取 provider 上绑定的托管 Codex OAuth 账号；未绑定时交给后端使用默认账号。
export function readWizardCodexOAuthAccountId(
  provider: Provider,
): string | undefined {
  const meta = provider.meta as
    | (Provider["meta"] & {
        auth_binding?: { accountId?: string; account_id?: string };
      })
    | undefined;
  const authBinding = (meta?.authBinding ?? meta?.auth_binding) as
    | { accountId?: string; account_id?: string }
    | undefined;
  const accountId = authBinding?.accountId ?? authBinding?.account_id;
  return typeof accountId === "string" && accountId.trim()
    ? accountId.trim()
    : undefined;
}

// 判断模型源是否是官方 Codex OAuth 路径；这类 provider 使用 ChatGPT 登录，不走 API Key + /models。
export function isWizardCodexOAuthSource(provider: Provider): boolean {
  const config = provider.settingsConfig ?? {};
  const meta = provider.meta as
    | (Provider["meta"] & {
        auth_binding?: {
          source?: string;
          authProvider?: string;
          auth_provider?: string;
        };
      })
    | undefined;
  const providerType = String(
    meta?.providerType ?? config.providerType ?? "",
  ).toLowerCase();
  const authBinding = (meta?.authBinding ?? meta?.auth_binding) as
    | {
        source?: string;
        authProvider?: string;
        auth_provider?: string;
      }
    | undefined;
  const authSource = String(
    authBinding?.source ?? config.auth?.source ?? "",
  ).toLowerCase();
  const authProvider = String(
    authBinding?.authProvider ??
      authBinding?.auth_provider ??
      config.auth?.authProvider ??
      config.auth?.auth_provider ??
      "",
  ).toLowerCase();
  const authMode = String(config.auth?.auth_mode ?? "").toLowerCase();
  const baseUrl = readWizardProviderBaseUrl(provider).toLowerCase();
  const idOrName = `${provider.id} ${provider.name}`.toLowerCase();
  return (
    provider.category === "official" ||
    providerType.includes("codex_oauth") ||
    authSource === "managed_codex_oauth" ||
    authProvider === "codex_oauth" ||
    authMode === "chatgpt" ||
    baseUrl.includes("chatgpt.com/backend-api/codex") ||
    idOrName === "openai openai" ||
    (idOrName.includes("openai") && idOrName.includes("official"))
  );
}

// 判断模型源是否是官方/OAuth 路径；这些 provider 必须通过 ChatGPT 专用接口获取目录。
function isOfficialCodexSource(provider: Provider): boolean {
  return isWizardCodexOAuthSource(provider);
}

// 读取 Codex provider 的真实持久化模型目录；缺失或结构异常时返回空目录，不能伪造 OAuth 模型权限。
export function readWizardModelCatalog(
  provider: Provider,
): CodexCatalogModel[] {
  const models = provider.settingsConfig?.modelCatalog?.models;
  if (!Array.isArray(models)) {
    return [];
  }
  return models.filter(
    (model): model is CodexCatalogModel =>
      typeof model === "object" &&
      model !== null &&
      typeof (model as CodexCatalogModel).model === "string" &&
      Boolean((model as CodexCatalogModel).model.trim()),
  );
}

// 判断 provider 是否是 MultiRouter 方案；向导只把普通 provider 当作上游模型源。
export function isCodexMultiRouterPlan(provider: Provider): boolean {
  const routing = provider.settingsConfig?.codexRouting;
  return Boolean(
    routing &&
      typeof routing === "object" &&
      (routing.enabled !== false || Array.isArray(routing.routes)),
  );
}

// 从 Codex provider 配置里读取模型列表/连通性探测可用的推理 API Key。
function readWizardProviderApiKey(provider: Provider): string {
  const config = provider.settingsConfig ?? {};
  const auth = config.auth ?? {};
  return String(
    auth.OPENAI_API_KEY ??
      config.apiKey ??
      config.api_key ??
      config.experimental_bearer_token ??
      "",
  ).trim();
}

// 从 provider 配置里提取可调用 /models 的参数；官方 OAuth provider 没有普通 Base URL 时会被跳过。
export function getWizardModelFetchConfig(
  provider: Provider,
): WizardModelFetchConfig | null {
  if (isWizardCodexOAuthSource(provider)) return null;
  const config = provider.settingsConfig ?? {};
  const baseUrl = readWizardProviderBaseUrl(provider);
  const accessKeyId = provider.meta?.usage_script?.accessKeyId;
  const secretAccessKey = provider.meta?.usage_script?.secretAccessKey;
  const apiKey = readWizardProviderApiKey(provider);
  const volcengineModelListAction = codexPlanModelListAction({
    baseUrl,
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
    apiKey,
    accessKeyId,
    secretAccessKey,
  });
  if (!baseUrl || (!apiKey && !volcengineModelListAction)) return null;
  return {
    baseUrl,
    apiKey,
    isFullUrl: Boolean(provider.meta?.isFullUrl ?? config.isFullUrl),
    modelsUrl:
      typeof config.modelsUrl === "string" ? config.modelsUrl : undefined,
    customUserAgent: provider.meta?.customUserAgent,
    ...(volcengineModelListAction
      ? {
          volcengineModelListAction,
          volcengineAccessKeyId: accessKeyId,
          volcengineSecretAccessKey: secretAccessKey,
        }
      : {}),
  };
}

// 判断 provider 是否已有可用于路由的真实模型目录。
export function hasWizardModelCatalog(provider: Provider): boolean {
  return readWizardModelCatalog(provider).length > 0;
}

// 从 Codex provider 配置中读取 Base URL，兼容已扁平化字段和原始 config.toml 字符串。
export function readWizardProviderBaseUrl(provider: Provider): string {
  const config = provider.settingsConfig ?? {};
  const direct = String(
    config.base_url ?? config.baseURL ?? config.baseUrl ?? "",
  ).trim();
  if (direct) return direct;
  return typeof config.config === "string"
    ? (extractCodexBaseUrl(config.config) ?? "").trim()
    : "";
}

// 判断向导中的模型源是否属于只能使用内置目录的 Plan provider。
export function isWizardCatalogOnlyModelSource(provider: Provider): boolean {
  return isCodexCatalogOnlyPlanModelFetch({
    baseUrl: readWizardProviderBaseUrl(provider),
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
    apiKey: readWizardProviderApiKey(provider),
    accessKeyId: provider.meta?.usage_script?.accessKeyId,
    secretAccessKey: provider.meta?.usage_script?.secretAccessKey,
  });
}

// 判断向导中的模型源是否是火山 AgentPlan，供 UI 展示专用 OpenAPI 路径。
export function isWizardVolcengineAgentPlanModelSource(
  provider: Provider,
): boolean {
  return isCodexVolcengineAgentPlanModelFetch({
    baseUrl: readWizardProviderBaseUrl(provider),
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
  });
}

// 给状态机提供配置缺口列表；已有模型目录的 provider 可以继续进入路由预览，不强制要求 /models 可抓。
export function getWizardConfigIssues(
  providers: Provider[],
): WizardConfigIssue[] {
  return providers
    .filter((provider) => {
      if (isWizardCodexOAuthSource(provider)) return false;
      const hasCatalog = hasWizardModelCatalog(provider);
      if (isWizardCatalogOnlyModelSource(provider)) return !hasCatalog;
      return !getWizardModelFetchConfig(provider) && !hasCatalog;
    })
    .map((provider) => ({
      providerId: provider.id,
      providerName: provider.name,
      reason: isWizardCatalogOnlyModelSource(provider)
        ? "当前 Plan 缺少推理 API Key 或专用模型列表凭据，且没有可用 modelCatalog。"
        : "缺少 Base URL/API Key，且当前没有可用 modelCatalog。",
    }));
}

export interface MergeFetchedWizardModelsOptions {
  preserveExistingSelection?: boolean;
}

// 把 /models 返回值合并进 provider modelCatalog；可选择把已有目录当作用户保留列表，只刷新元数据不追加已删除模型。
export function mergeFetchedModelsIntoWizardProvider(
  provider: Provider,
  fetchedModels: FetchedModel[],
  options: MergeFetchedWizardModelsOptions = {},
): Provider {
  const existingModels = readWizardModelCatalog(provider);
  const byModel = new Map<string, CodexCatalogModel>();
  const byFetchedModel = new Map<string, string>();
  for (const model of existingModels) {
    byModel.set(model.model, model);
    const visibleModel = model.model?.trim();
    if (visibleModel) {
      byFetchedModel.set(visibleModel, model.model);
    }
    const upstreamModel = (
      model.upstreamModel ??
      model.upstream_model ??
      model.model
    )?.trim();
    if (upstreamModel) {
      byFetchedModel.set(upstreamModel, model.model);
    }
  }
  const shouldAppendFetchedModels =
    !options.preserveExistingSelection || existingModels.length === 0;
  for (const fetched of fetchedModels) {
    const modelId = fetched.id.trim();
    if (!modelId) continue;
    const visibleModelId = byFetchedModel.get(modelId) ?? modelId;
    const existing = byModel.get(visibleModelId);
    if (!existing && !shouldAppendFetchedModels) continue;
    byModel.set(visibleModelId, {
      ...(existing ?? {}),
      model: visibleModelId,
      upstreamModel:
        existing?.upstreamModel ?? existing?.upstream_model ?? modelId,
      displayName: existing?.displayName ?? visibleModelId,
      ...(fetched.contextWindow
        ? { contextWindow: fetched.contextWindow }
        : {}),
      ...(fetched.inputModalities && fetched.inputModalities.length > 0
        ? {
            inputModalities: fetched.inputModalities as Array<
              "text" | "image"
            >,
            input_modalities: fetched.inputModalities as Array<
              "text" | "image"
            >,
          }
        : {}),
      ...(fetched.supportsImage !== undefined && fetched.supportsImage !== null
        ? {
            supportsImage: fetched.supportsImage,
            supports_image: fetched.supportsImage,
          }
        : {}),
    });
  }
  const models = Array.from(byModel.values());
  const allowedModels = new Set(models.map((model) => model.model));
  const rawSpawnAgentModels =
    provider.settingsConfig?.modelCatalog?.spawnAgentModels;
  const spawnAgentModels = Array.isArray(rawSpawnAgentModels)
    ? rawSpawnAgentModels
        .filter(
          (model): model is string =>
            typeof model === "string" && allowedModels.has(model),
        )
        .slice(0, 5)
    : undefined;
  return {
    ...provider,
    settingsConfig: {
      ...provider.settingsConfig,
      modelCatalog: {
        ...(provider.settingsConfig?.modelCatalog ?? {}),
        models,
        ...(spawnAgentModels ? { spawnAgentModels } : {}),
      },
    },
  };
}

// 判断某个模型源是否应该优先保留原始可见模型名；官方/订阅源是重名冲突的 canonical 侧。
function isCanonicalModelSource(provider: Provider): boolean {
  return isOfficialCodexSource(provider);
}

// 收集重名模型冲突，供向导进入“重名确认”状态并展示需要用户理解的别名策略。
export function collectWizardModelNameCollisions(
  providers: Provider[],
): WizardModelNameCollision[] {
  const ownersByUpstream = new Map<string, Provider[]>();
  for (const provider of providers) {
    for (const model of readWizardModelCatalog(provider)) {
      const upstream =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      if (!upstream) continue;
      const owners = ownersByUpstream.get(upstream) ?? [];
      owners.push(provider);
      ownersByUpstream.set(upstream, owners);
    }
  }
  return Array.from(ownersByUpstream.entries())
    .filter(([, owners]) => owners.length > 1)
    .map(([upstreamModel, owners]) => ({
      upstreamModel,
      providerIds: owners.map((owner) => owner.id),
      canonicalProviderIds: owners
        .filter(isCanonicalModelSource)
        .map((owner) => owner.id),
    }));
}

// 把 provider 展示名清理成可放进模型 ID 的稳定后缀；优先使用用户能看懂的名称，避免泄露自动生成 ID。
function providerNameSuffix(provider: Provider): string {
  const cleanedName = provider.name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (cleanedName) return cleanedName;
  return (
    provider.id
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "provider"
  );
}

// 为非官方重名模型生成稳定别名，保留 upstreamModel 指向真实上游模型名。
function aliasModelName(provider: Provider, modelName: string): string {
  return `${modelName}-${providerNameSuffix(provider)}`;
}

// 从已处理别名的模型目录生成 route 级模型映射；后端物化 targetProviderId
// 时只读取原 provider 配置，必须靠这里把可见别名改回真实上游模型名。
function buildWizardRouteModelMap(
  provider: Provider,
): Record<string, string> | undefined {
  const entries = readWizardModelCatalog(provider)
    .map((model) => {
      const visibleModel = model.model?.trim();
      const upstreamModel = (
        model.upstreamModel ??
        model.upstream_model ??
        model.model
      )?.trim();
      return visibleModel && upstreamModel && visibleModel !== upstreamModel
        ? [visibleModel, upstreamModel]
        : null;
    })
    .filter((entry): entry is [string, string] => Boolean(entry));
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

// 判断模型目录是否主要是 OpenAI 官方 GPT/O 系列；这些模型应优先保持 Responses 原生链路。
function hasOpenAiResponsesNativeModels(provider: Provider): boolean {
  if (!isOfficialCodexSource(provider)) return false;
  return readWizardModelCatalog(provider).some((model) => {
    const upstream = (
      model.upstreamModel ??
      model.upstream_model ??
      model.model ??
      ""
    )
      .trim()
      .toLowerCase();
    return upstream.startsWith("gpt-") || /^o\d/.test(upstream);
  });
}

// 检测多个 provider 暴露的同名模型；官方保留原名，第三方/中转站自动生成可见别名。
export function resolveWizardModelNameCollisions(
  providers: Provider[],
): Provider[] {
  const ownersByUpstream = new Map<string, Provider[]>();
  for (const provider of providers) {
    for (const model of readWizardModelCatalog(provider)) {
      const upstream =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      if (!upstream) continue;
      const owners = ownersByUpstream.get(upstream) ?? [];
      owners.push(provider);
      ownersByUpstream.set(upstream, owners);
    }
  }

  return providers.map((provider) => {
    const nextModels = readWizardModelCatalog(provider).map((model) => {
      const upstream =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      const owners = ownersByUpstream.get(upstream) ?? [];
      if (owners.length <= 1 || isCanonicalModelSource(provider)) {
        return { ...model, upstreamModel: upstream };
      }
      return {
        ...model,
        model: aliasModelName(provider, upstream),
        displayName: model.displayName ?? aliasModelName(provider, upstream),
        upstreamModel: upstream,
      };
    });
    return {
      ...provider,
      settingsConfig: {
        ...provider.settingsConfig,
        modelCatalog: {
          ...(provider.settingsConfig?.modelCatalog ?? {}),
          models: nextModels,
        },
      },
    };
  });
}

// 按 provider 名称和模型名推断默认前缀；这些前缀只作为向导初始规则，后续可在工作台细调。
export function inferWizardRoutePrefixes(provider: Provider): string[] {
  const text = `${provider.id} ${provider.name} ${provider.category ?? ""} ${
    provider.meta?.providerType ?? ""
  }`.toLowerCase();
  const models = readWizardModelCatalog(provider).map((model) =>
    model.model.toLowerCase(),
  );
  const has = (value: string) =>
    text.includes(value) || models.some((model) => model.startsWith(value));
  const prefixes = new Set<string>();
  if (has("openai") || has("gpt")) prefixes.add("gpt");
  if (has("openai") || models.some((model) => /^o\d/.test(model))) {
    prefixes.add("o");
  }
  if (has("deepseek")) prefixes.add("deepseek");
  if (has("qwen")) prefixes.add("qwen");
  if (has("ollama") || has("vllm") || has("local")) prefixes.add("local");
  return Array.from(prefixes);
}

// 推断 route 上游协议；官方 GPT/O 模型优先走 Responses，未知第三方默认走 Chat Completions。
export function inferWizardApiFormat(provider: Provider): CodexApiFormat {
  const config = provider.settingsConfig ?? {};
  if (hasOpenAiResponsesNativeModels(provider)) {
    return "openai_responses";
  }
  return (
    provider.meta?.apiFormat ??
    config.apiFormat ??
    config.api_format ??
    "openai_chat"
  );
}

// 推断 provider 的缓存能力；这里只生成安全默认，不会让自动缓存平台收到 OpenAI 私有参数。
export function inferWizardCacheConfig(provider: Provider): CodexCacheConfig {
  const text = `${provider.id} ${provider.name} ${provider.category ?? ""} ${
    provider.meta?.providerType ?? ""
  }`.toLowerCase();
  const models = readWizardModelCatalog(provider).map((model) =>
    String(model.upstreamModel ?? model.upstream_model ?? model.model)
      .trim()
      .toLowerCase(),
  );
  const hasModel = (needle: string) =>
    models.some((model) => model.includes(needle));
  if (
    isOfficialCodexSource(provider) ||
    hasOpenAiResponsesNativeModels(provider)
  ) {
    return {
      cacheMode: "openai_prompt_cache",
      supportsPromptCacheKey: true,
      supportsPromptCacheRetention: true,
      promptCacheKey: provider.meta?.promptCacheKey,
      promptCacheRetention: provider.meta?.promptCacheRetention,
      usageFields: [
        "usage.input_tokens_details.cached_tokens",
        "usage.prompt_tokens_details.cached_tokens",
      ],
    };
  }
  if (text.includes("deepseek") || hasModel("deepseek")) {
    return {
      cacheMode: "deepseek_context_cache",
      usageFields: [
        "usage.prompt_cache_hit_tokens",
        "usage.prompt_cache_miss_tokens",
      ],
    };
  }
  if (
    text.includes("z.ai") ||
    text.includes("zai") ||
    text.includes("glm") ||
    hasModel("glm")
  ) {
    return {
      cacheMode: "glm_context_cache",
      usageFields: ["usage.prompt_tokens_details.cached_tokens"],
    };
  }
  if (text.includes("dashscope") || text.includes("qwen") || hasModel("qwen")) {
    return {
      cacheMode: "qwen_context_cache",
      usageFields: [
        "usage.input_tokens_details.cached_tokens",
        "usage.prompt_tokens_details.cached_tokens",
        "usage.prompt_tokens_details.cache_creation_input_tokens",
      ],
    };
  }
  return { cacheMode: "unknown" };
}

// 每个 provider 默认探测其 modelCatalog 暴露的全部可见模型；这是用户显式点击的真实请求，不在向导自动执行。
export function getWizardConnectivityProbeModels(provider: Provider): string[] {
  return Array.from(
    new Set(
      readWizardModelCatalog(provider)
        .map((model) => model.model?.trim())
        .filter((model): model is string => Boolean(model)),
    ),
  );
}

// 将真实 `/v1/responses` 探测结果归类为“可继续/阻塞”；Chat-only provider 的 Responses 失败不是阻塞。
export function classifyWizardConnectivityResult(args: {
  provider: Provider;
  model: string;
  ok: boolean;
  detail: string;
  url?: string;
  httpStatus?: number | null;
}): WizardConnectivityResult {
  const apiFormat = inferWizardApiFormat(args.provider);
  if (args.ok) {
    return {
      providerId: args.provider.id,
      providerName: args.provider.name,
      model: args.model,
      status: "pass",
      canContinue: true,
      detail: "直接 /v1/responses 探测通过。",
      url: args.url,
      httpStatus: args.httpStatus,
    };
  }

  const chatOnlyCanContinue = apiFormat === "openai_chat";
  return {
    providerId: args.provider.id,
    providerName: args.provider.name,
    model: args.model,
    status: chatOnlyCanContinue ? "warn" : "fail",
    canContinue: chatOnlyCanContinue,
    detail: chatOnlyCanContinue
      ? `直接 /v1/responses 失败，但该 provider 配置为 Chat Completions；运行时会由 MultiRouter 转换到 /chat/completions。上游返回：${args.detail}`
      : `该 provider 配置为 Responses 直连，/v1/responses 失败会阻塞真实 Codex 请求。上游返回：${args.detail}`,
    url: args.url,
    httpStatus: args.httpStatus,
  };
}

// 结合 Responses 与 Chat 两条真实探测结果，判断 provider 应该用哪个上游协议。
export function classifyWizardDualProtocolConnectivityResult(args: {
  provider: Provider;
  model: string;
  responses: WizardProtocolProbe;
  chat: WizardProtocolProbe;
}): WizardConnectivityResult {
  const base = {
    providerId: args.provider.id,
    providerName: args.provider.name,
    model: args.model,
    url: args.responses.url ?? args.chat.url,
    httpStatus: args.responses.httpStatus ?? args.chat.httpStatus,
  };
  if (args.responses.ok && args.chat.ok) {
    return {
      ...base,
      status: "pass",
      canContinue: true,
      recommendedApiFormat: "openai_responses",
      detail:
        "Responses 和 Chat Completions 的基础请求都可用；这只能证明协议入口、鉴权和模型名可用，Codex 原生工具链仍优先使用 Responses。",
    };
  }
  if (args.responses.ok) {
    return {
      ...base,
      status: "pass",
      canContinue: true,
      recommendedApiFormat: "openai_responses",
      detail: `Responses 基础请求可用，Chat Completions 不通；将使用 Responses。该结果不等于完整 Codex 功能验证，Chat 返回：${args.chat.detail}`,
    };
  }
  if (args.chat.ok) {
    return {
      ...base,
      status: "warn",
      canContinue: true,
      recommendedApiFormat: "openai_chat",
      detail: `Responses 不通但 Chat Completions 可用；将保留 Chat 转换路径。Responses 返回：${args.responses.detail}`,
    };
  }
  return {
    ...base,
    status: "fail",
    canContinue: false,
    detail: `Responses 和 Chat Completions 都不通，更可能是 API Key、Base URL、模型权限、额度、网络或上游可用性问题。Responses 返回：${args.responses.detail}；Chat 返回：${args.chat.detail}`,
  };
}

// 没有可探测配置时生成跳过结果；有模型目录则可继续但有风险，没有目录则阻塞。
export function skippedWizardConnectivityResult(
  provider: Provider,
  reason: string,
): WizardConnectivityResult {
  const hasCatalog = hasWizardModelCatalog(provider);
  return {
    providerId: provider.id,
    providerName: provider.name,
    model: "*",
    status: hasCatalog ? "skipped" : "fail",
    canContinue: hasCatalog,
    detail: hasCatalog
      ? `${reason}；已有 modelCatalog，允许继续但未验证真实响应。`
      : `${reason}；且没有 modelCatalog，不能确认路由可用。`,
  };
}

// 聚合连通性结果：只要存在阻塞项，状态机就不应自动进入保存发布。
export function canContinueAfterConnectivity(
  results: WizardConnectivityResult[],
): boolean {
  return results.length > 0 && results.every((result) => result.canContinue);
}

// 将 Chat / Responses 双协议探测结果反写到向导草稿；用户手动锁定的协议优先于探测推荐。
export function applyWizardConnectivityApiFormatOverrides(
  providers: Provider[],
  results: WizardConnectivityResult[],
): Provider[] {
  if (results.length === 0) return providers;
  const resultsByProvider = new Map<string, WizardConnectivityResult[]>();
  for (const result of results) {
    const bucket = resultsByProvider.get(result.providerId) ?? [];
    bucket.push(result);
    resultsByProvider.set(result.providerId, bucket);
  }

  return providers.map((provider) => {
    const providerResults = resultsByProvider.get(provider.id) ?? [];
    if (provider.meta?.apiFormatSource === "manual") {
      return provider;
    }
    const recommendedFormats = providerResults
      .map((result) => result.recommendedApiFormat)
      .filter((format): format is CodexApiFormat => Boolean(format));
    const hasResponsesRecommendation =
      recommendedFormats.includes("openai_responses");
    const hasChatRecommendation = recommendedFormats.includes("openai_chat");
    const hasResponsesPass = providerResults.some(
      (result) =>
        result.status === "pass" ||
        result.recommendedApiFormat === "openai_responses",
    );
    const hasBlockingFailure = providerResults.some(
      (result) => result.status === "fail",
    );
    if (hasBlockingFailure) return provider;
    if (hasChatRecommendation && !hasResponsesRecommendation) {
      return {
        ...provider,
        meta: {
          ...(provider.meta ?? {}),
          apiFormat: "openai_chat",
          apiFormatSource: "probe",
        },
        settingsConfig: {
          ...(provider.settingsConfig ?? {}),
          apiFormat: "openai_chat",
        },
      };
    }
    if (!hasResponsesPass) return provider;
    return {
      ...provider,
      meta: {
        ...(provider.meta ?? {}),
        apiFormat: "openai_responses",
        apiFormatSource: "probe",
      },
      settingsConfig: {
        ...(provider.settingsConfig ?? {}),
        apiFormat: "openai_responses",
      },
    };
  });
}

// 按用户最终保留的模型顺序过滤每个 provider 的目录；路由和最终 catalog 必须共享这份过滤结果。
export function filterWizardProvidersByModelOrder(
  providers: Provider[],
  modelOrder?: string[],
): Provider[] {
  if (!modelOrder) return providers;
  const enabledModels = new Set(
    modelOrder.map((model) => model.trim()).filter(Boolean),
  );
  const orderIndex = new Map(modelOrder.map((model, index) => [model, index]));
  return providers
    .map((provider) => {
      const models = readWizardModelCatalog(provider)
        .filter((model) => enabledModels.has(model.model))
        .sort(
          (left, right) =>
            (orderIndex.get(left.model) ?? Number.MAX_SAFE_INTEGER) -
            (orderIndex.get(right.model) ?? Number.MAX_SAFE_INTEGER),
        );
      return {
        ...provider,
        settingsConfig: {
          ...provider.settingsConfig,
          modelCatalog: {
            ...(provider.settingsConfig?.modelCatalog ?? {}),
            models,
          },
        },
      };
    })
    .filter((provider) => readWizardModelCatalog(provider).length > 0);
}

// 为模型源生成 provider 分组 route；只引用 targetProviderId，不复制第三方 bearer 密钥。
export function buildWizardRoutesFromSources(
  providers: Provider[],
): CodexRoutingRoute[] {
  return providers.map((provider) => {
    const models = readWizardModelCatalog(provider).map((model) => model.model);
    const modelMap = buildWizardRouteModelMap(provider);
    const oauthAccountId = isWizardCodexOAuthSource(provider)
      ? readWizardCodexOAuthAccountId(provider)
      : undefined;
    return {
      id: `router-${provider.id}`,
      label: provider.name,
      enabled: true,
      targetProviderId: provider.id,
      match: {
        models,
        prefixes: inferWizardRoutePrefixes(provider),
      },
      upstream: {
        apiFormat: inferWizardApiFormat(provider),
        auth: isWizardCodexOAuthSource(provider)
          ? {
              source: "managed_codex_oauth",
              authProvider: "codex_oauth",
              ...(oauthAccountId ? { accountId: oauthAccountId } : {}),
            }
          : { source: "provider_config" },
        ...(modelMap ? { modelMap } : {}),
      },
      capabilities: {
        codexCache: inferWizardCacheConfig(provider),
      },
    };
  });
}

// 从已处理重名的模型源生成 MultiRouter catalog；保留 upstreamModel 供运行时把别名映射回真实模型。
export function buildWizardModelCatalog(
  providers: Provider[],
  options: Pick<
    WizardPlanBuildOptions,
    "catalogModelOrder" | "spawnAgentModels"
  > = {},
): CodexModelCatalogConfig {
  const byModel = new Map<string, CodexCatalogModel>();
  for (const provider of providers) {
    for (const model of readWizardModelCatalog(provider)) {
      if (!byModel.has(model.model)) {
        byModel.set(model.model, model);
      }
    }
  }
  const baseModels = Array.from(byModel.values());
  const models = options.catalogModelOrder
    ? options.catalogModelOrder
        .map((model) => byModel.get(model))
        .filter((model): model is CodexCatalogModel => Boolean(model))
    : baseModels;
  const modelSet = new Set(models.map((model) => model.model));
  const spawnAgentModels = (options.spawnAgentModels ?? [])
    .map((model) => model.trim())
    .filter((model) => modelSet.has(model))
    .slice(0, 5);
  return {
    models,
    spawnAgentModels:
      spawnAgentModels.length > 0
        ? spawnAgentModels
        : models.map((model) => model.model).slice(0, 5),
  };
}

// 生成官方 OAuth 源的去重键；未绑定账号时都代表后端默认 ChatGPT 账号。
function wizardCodexOAuthDedupKey(provider: Provider): string {
  return `codex-oauth:${readWizardCodexOAuthAccountId(provider) ?? "default"}`;
}

// 为等价官方 OAuth 源打分；优先保留已有真实目录，其次保留稳定官方 seed。
function wizardCodexOAuthSourceScore(provider: Provider): number {
  let score = readWizardModelCatalog(provider).length * 1000;
  if (provider.id === "codex-official") score += 100;
  if (provider.category === "official") score += 50;
  if (readWizardCodexOAuthAccountId(provider)) score += 10;
  return score;
}

// 收敛等价官方 OAuth 源，避免 `default` 和 `codex-official` 对同一默认账号重复取模和重复报错。
function dedupeWizardCodexOAuthSources(providers: Provider[]): Provider[] {
  const result: Provider[] = [];
  const selectedByKey = new Map<string, Provider>();
  const indexByKey = new Map<string, number>();

  for (const provider of providers) {
    if (!isWizardCodexOAuthSource(provider)) {
      result.push(provider);
      continue;
    }

    const key = wizardCodexOAuthDedupKey(provider);
    const current = selectedByKey.get(key);
    if (!current) {
      selectedByKey.set(key, provider);
      indexByKey.set(key, result.length);
      result.push(provider);
      continue;
    }

    if (
      wizardCodexOAuthSourceScore(provider) <=
      wizardCodexOAuthSourceScore(current)
    ) {
      continue;
    }

    const index = indexByKey.get(key);
    if (index !== undefined) {
      result[index] = provider;
      selectedByKey.set(key, provider);
    }
  }

  return result;
}

// 过滤出向导默认可用的普通 Codex provider；空目录 provider 仍保留，便于引导用户先刷新模型。
export function defaultWizardModelSources(providers: Provider[]): Provider[] {
  return dedupeWizardCodexOAuthSources(
    providers.filter((provider) => !isCodexMultiRouterPlan(provider)),
  );
}

// 创建或更新 MultiRouter provider；草稿只在用户点击保存发布时写入数据库。
export function buildCodexMultiRouterWizardPlan(
  allProviders: Provider[],
  sourceProviders: Provider[],
  existingPlan?: Provider | null,
  options: WizardPlanBuildOptions = {},
): WizardPlanBuildResult {
  const resolvedSources = filterWizardProvidersByModelOrder(
    resolveWizardModelNameCollisions(sourceProviders),
    options.catalogModelOrder,
  );
  const routes = buildWizardRoutesFromSources(resolvedSources);
  const routing: CodexRoutingConfig = {
    enabled: true,
    defaultRouteId: routes[0]?.id,
    routes,
  };
  const existingIds = new Set(allProviders.map((provider) => provider.id));
  const planId =
    existingPlan?.id ??
    (existingIds.has(CODEX_MULTI_ROUTER_DEFAULT_ID)
      ? `${CODEX_MULTI_ROUTER_DEFAULT_ID}-${Date.now()}`
      : CODEX_MULTI_ROUTER_DEFAULT_ID);
  const plan: Provider = {
    ...(existingPlan ?? {
      id: planId,
      name: CODEX_MULTI_ROUTER_DEFAULT_NAME,
      category: "custom",
      createdAt: Date.now(),
    }),
    id: planId,
    name:
      options.planName?.trim() ||
      existingPlan?.name ||
      CODEX_MULTI_ROUTER_DEFAULT_NAME,
    category: existingPlan?.category ?? "custom",
    settingsConfig: {
      ...(existingPlan?.settingsConfig ?? {}),
      auth: existingPlan?.settingsConfig?.auth ?? {},
      base_url: CODEX_MULTI_ROUTER_PROXY_BASE_URL,
      baseUrl: CODEX_MULTI_ROUTER_PROXY_BASE_URL,
      config: existingPlan?.settingsConfig?.config ?? null,
      modelCatalog: buildWizardModelCatalog(resolvedSources, options),
      codexRouting: routing,
    },
  };
  return { plan, sourceProviders: resolvedSources };
}
