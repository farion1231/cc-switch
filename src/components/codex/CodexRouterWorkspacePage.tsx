import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  AlertTriangle,
  ArrowRight,
  Bug,
  CheckCircle2,
  Clipboard,
  Database,
  FileClock,
  GitFork,
  GitBranch,
  GripVertical,
  Info,
  Layers3,
  Pencil,
  Play,
  Plus,
  RadioTower,
  RefreshCw,
  Route,
  Save,
  Server,
  Settings2,
  Trash2,
  Wand2,
  XCircle,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { providersApi } from "@/lib/api";
import {
  fetchCodexOauthCachedModels,
  fetchCodexOauthModels,
  fetchModelsForConfig,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { proxyApi } from "@/lib/api/proxy";
import {
  isWizardCodexOAuthSource,
  readWizardCodexOAuthAccountId,
  resolveWizardModelNameCollisions,
} from "@/lib/codexMultiRouterWizard";
import { syncCodexMultiRouterPlanWithProviders } from "@/lib/codexMultiRouterSync";
import { usageApi } from "@/lib/api/usage";
import {
  usageKeys,
  useCodexSubagentUsageStats,
  useRequestLogs,
} from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import { resolveFetchedCodexModelContextWindow } from "@/utils/codexModelContext";
import {
  catalogModelLabel,
  CODEX_SPAWN_AGENT_PRIORITY_MODELS,
  normalizeCodexSpawnAgentModels,
  normalizeSpawnAgentCandidateSelection,
  readCodexModelCatalog,
  reorderSpawnAgentCandidates,
  validateSpawnAgentCandidates,
  type CodexCatalogModel,
} from "@/utils/codexSpawnAgentCandidates";
import {
  extractCodexExperimentalBearerToken,
  getCodexBaseUrl,
} from "@/utils/providerConfigUtils";
import {
  codexCatalogOnlyPlanModelFetchMessage,
  codexPlanModelListAction,
  isCodexCatalogOnlyPlanModelFetch,
} from "@/utils/codexPlanModelFetch";
import type { Provider } from "@/types";
import type { RequestLog } from "@/types/usage";
import type {
  CodexDiagnosticCheck,
  CodexDiagnosticStatus,
  CodexModelPickerUnlockResult,
  CodexMultiRouterDiagnostics,
  CodexRouteSummary,
  CodexRouterLogEvent,
  GlobalProxyConfig,
  ProxyStatus,
} from "@/types/proxy";

export type WorkspaceTab =
  | "overview"
  | "sources"
  | "routes"
  | "status"
  | "test";

type StatusView = "link" | "protocol" | "debug" | "providers" | "traffic";

type SpawnAgentCandidateView = "selected" | "routed" | "priority" | "all";

// 模型菜单解锁只处理 Codex Desktop renderer 白名单，不改变 MultiRouter 路由或凭据。
const MODEL_PICKER_UNLOCK_TOOLTIP =
  "开启或确认 Codex 接管后，CCSwitchMulti 会自动尝试一次。只有当前 Codex Desktop 已普通启动且没有 remote debugging 时，才需要完全退出后点击这里；成功带 CDP 启动后，切换第三方 API Key 不需要重复解锁。它只注入 Desktop renderer 模型白名单补丁，不改变路由规则、API Key 或模型目录；CLI/app-server 仍由 config.toml、model_catalog_json、本地 /v1/models 和 MultiRouter 路由支持。";

// 链路页需要给出明确下一步，避免用户只看到诊断异常却不知道要触发解锁。
const MODEL_PICKER_UNLOCK_HINT =
  "开启或确认 Codex 接管后会自动尝试一次；若当前 Desktop 已普通启动且菜单仍只显示“自定义”，请完全退出 Codex Desktop 后点击“解锁模型菜单”。CLI/app-server 的模型目录修复走 live config、model_catalog_json 和本地 /v1/models，不需要把小写 codex.exe 当 Desktop 启动。";

type CodexRoute = {
  id?: string;
  label?: string;
  enabled?: boolean;
  targetProviderId?: string;
  target_provider_id?: string;
  providerId?: string;
  provider_id?: string;
  upstreamProviderId?: string;
  upstream_provider_id?: string;
  provider?: string;
  match?: {
    models?: string[];
    prefixes?: string[];
  };
  upstream?: {
    baseUrl?: string;
    base_url?: string;
    apiFormat?: string;
    wireApi?: string;
    wire_api?: string;
    targetProviderId?: string;
    target_provider_id?: string;
    providerId?: string;
    provider_id?: string;
    upstreamProviderId?: string;
    upstream_provider_id?: string;
    provider?: string;
    auth?: {
      source?: string;
    };
    modelMap?: Record<string, string>;
  };
  capabilities?: {
    textOnly?: boolean;
    supportsReasoning?: boolean;
    inputModalities?: string[];
  };
};

type CodexRouteCapabilities = NonNullable<CodexRoute["capabilities"]>;

type CodexRouting = {
  enabled?: boolean;
  defaultRouteId?: string;
  routes?: CodexRoute[];
};

type RouteEntry = {
  provider: Provider;
  route: CodexRoute;
  index: number;
};

type RouteTrafficRow = {
  providerId: string;
  providerName: string;
  model: string;
  routeId: string | null;
  routeLabel: string | null;
  configuredProtocol: string | null;
  configuredProtocolSource: string | null;
  configuredProtocolDetail: string | null;
  lastObservedProtocol: string | null;
  lastObservedAt: string | null;
  lastObservedUpstreamUrl: string | null;
  lastObservedEndpoint: string | null;
  requestCount: number;
  successCount: number;
  failedCount: number;
  totalTokens: number;
  avgLatencyMs: number;
};

type RouteTrafficTarget = {
  providerId: string;
  providerName: string;
};

type MultiRouterRuntimeStatus = {
  running: boolean;
  label: string;
  detail: string;
  tone: "ok" | "warn";
};

type RouteCandidate = {
  id: string;
  route: CodexRoute;
  provider?: Provider;
  isExisting: boolean;
  matchModels: string[];
  matchPrefixes: string[];
};

/// 将用于人眼识别的 route/provider 文本规整成稳定 key，供旧配置和新 provider 做语义匹配。
function normalizedRouteIdentityText(value?: string | null): string {
  return (value ?? "")
    .trim()
    .toLowerCase()
    .replace(/[\s_-]+/g, "");
}

/// 收集 provider 可暴露的模型集合；旧 inline route 没有 targetProviderId 时用它判断是否与新增 provider 等价。
function providerRouteModelIdentitySet(provider: Provider): Set<string> {
  return new Set(
    [
      ...readCodexModelCatalog(provider).models.map((model) => model.model),
      ...collectProviderModelIds(provider),
      ...inferProviderPrefixes(provider, collectProviderModelIds(provider)),
    ]
      .map(normalizedRouteIdentityText)
      .filter(Boolean),
  );
}

/// 旧版 route 可能只保存 label/match/upstream，没有 targetProviderId；这里用名称和模型交集找回对应模型源。
function findSemanticRouteProvider(
  route: CodexRoute,
  modelSources: Provider[],
): Provider | undefined {
  const targetProviderId = routeTargetProviderId(route);
  if (targetProviderId) {
    return modelSources.find((source) => source.id === targetProviderId);
  }

  const routeNames = [
    route.label,
    route.id,
    route.upstream?.provider,
    route.upstream?.providerId,
    route.upstream?.provider_id,
  ]
    .map(normalizedRouteIdentityText)
    .filter(Boolean);
  const routeModels = [
    ...(route.match?.models ?? []),
    ...(route.match?.prefixes ?? []),
  ]
    .map(normalizedRouteIdentityText)
    .filter(Boolean);

  return modelSources.find((source) => {
    const providerNames = [source.id, source.name, source.meta?.providerType]
      .map((value) =>
        typeof value === "string" ? normalizedRouteIdentityText(value) : "",
      )
      .filter(Boolean);
    if (
      routeNames.some((name) => providerNames.includes(name)) ||
      providerNames.some((name) => routeNames.includes(name))
    ) {
      return true;
    }

    const providerModels = providerRouteModelIdentitySet(source);
    return routeModels.some((model) => providerModels.has(model));
  });
}

/// route 去重以真实目标 provider 优先；没有目标 provider 的旧 route 则退回到语义匹配出的 provider。
function routeSemanticProviderId(
  route: CodexRoute,
  modelSources: Provider[],
): string | undefined {
  return (
    routeTargetProviderId(route) ??
    findSemanticRouteProvider(route, modelSources)?.id
  );
}

/// 已保存 route 中可能同时存在“旧 inline 规则”和“复用供应商配置规则”；展示和保存前都要归并。
export function dedupeCodexRoutesBySemanticProvider(
  routes: CodexRoute[],
  modelSources: Provider[],
): CodexRoute[] {
  const seenProviderIds = new Set<string>();
  const result: CodexRoute[] = [];
  for (const route of routes) {
    const semanticProviderId = routeSemanticProviderId(route, modelSources);
    if (semanticProviderId) {
      if (seenProviderIds.has(semanticProviderId)) continue;
      seenProviderIds.add(semanticProviderId);
    }
    result.push(route);
  }
  return result;
}

type MultiRouterSettingsDraft = {
  name: string;
  notes?: string;
  enabled: boolean;
  defaultRouteId?: string;
};

type ProviderModelRefreshState = {
  status: "loading" | "success" | "error" | "skipped";
  message: string;
  modelCount?: number;
};

// 自动刷新事务的内部结果；读取和写回分开返回，便于统一控制 loading 终态。
type ProviderModelRefreshResult =
  | { status: "stale" }
  | { status: "empty"; message?: string }
  | {
      status: "updated";
      models: FetchedModel[];
      nextProvider: Provider;
      affectedPlans: Provider[];
      usedCodexCache?: boolean;
      onlineErrorMessage?: string;
    };

type ProviderModelFetchConfig = {
  baseUrl: string;
  apiKey: string;
  isFullUrl: boolean;
  customUserAgent?: string;
  volcengineModelListAction?: string;
  volcengineAccessKeyId?: string;
  volcengineSecretAccessKey?: string;
  codexOAuthAccountId?: string;
  useCodexOAuth?: boolean;
  skipReason?: string;
};

type ProxyListenDraftValidation =
  | {
      ok: true;
      listenAddress: string;
      listenPort: number;
      baseUrl: string;
    }
  | {
      ok: false;
      error: string;
    };

type CodexCatalogModelDraft = {
  model: string;
  upstreamModel?: string;
  upstream_model?: string;
  displayName?: string;
  display_name?: string;
  contextWindow?: string | number;
  context_window?: string | number;
  inputModalities?: string[];
  input_modalities?: string[];
  textOnly?: boolean;
  text_only?: boolean;
  supportsImage?: boolean;
  supports_image?: boolean;
  vision?: boolean;
  capabilities?: CodexRouteCapabilities;
};

/// 读取 catalog 条目的真实上游模型名；未配置别名映射时，上游模型名就是可见模型名。
function catalogDraftUpstreamModel(model: {
  model?: string;
  upstreamModel?: string;
  upstream_model?: string;
}): string {
  return (
    model.upstreamModel ??
    model.upstream_model ??
    model.model ??
    ""
  ).trim();
}

type CodexModelCatalogDraft = {
  models: CodexCatalogModelDraft[];
  spawnAgentModels?: string[];
};

const DEFAULT_CODEX_PROXY_LISTEN_ADDRESS = "127.0.0.1";
const DEFAULT_CODEX_PROXY_LISTEN_PORT = 15721;
const MODEL_REFRESH_TIMEOUT_MS = 30_000;

/// 生成候选 provider /models 刷新的去重键；API Key 用短哈希参与比较，避免换 key 后仍复用旧请求。
function buildProviderModelRefreshAttemptKey(
  providerId: string,
  fetchConfig: ProviderModelFetchConfig,
): string {
  return [
    providerId,
    fetchConfig.baseUrl,
    hashSensitiveAttemptPart(fetchConfig.apiKey),
    fetchConfig.isFullUrl,
    fetchConfig.customUserAgent ?? "",
    fetchConfig.volcengineModelListAction ?? "",
    hashSensitiveAttemptPart(fetchConfig.volcengineAccessKeyId ?? ""),
    hashSensitiveAttemptPart(fetchConfig.volcengineSecretAccessKey ?? ""),
    fetchConfig.useCodexOAuth ?? false,
    hashSensitiveAttemptPart(fetchConfig.codexOAuthAccountId ?? ""),
  ].join("|");
}

/// 为敏感字段生成仅用于内存比较的稳定短哈希，避免把完整 API Key 塞进刷新状态键。
function hashSensitiveAttemptPart(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return `${value.length}:${(hash >>> 0).toString(16)}`;
}

/// 给前端自动刷新加一层兜底超时；后端正常有 timeout，但 IPC 异常挂起时也必须让 UI 退出 loading。
function withModelRefreshTimeout<T>(
  promise: Promise<T>,
  timeoutMs = MODEL_REFRESH_TIMEOUT_MS,
  onTimeout?: () => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timeoutId = window.setTimeout(() => {
      onTimeout?.();
      reject(
        new Error(
          `模型列表读取或写回超过 ${Math.round(timeoutMs / 1000)} 秒，请检查网络、provider /models 端点或本地配置写入状态。`,
        ),
      );
    }, timeoutMs);

    promise.then(resolve, reject).finally(() => window.clearTimeout(timeoutId));
  });
}

/// 将未知异常转成 UI 可展示文本，避免每个 catch 重复 `instanceof Error`。
function workspaceErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/// 读取 provider 模型列表；官方 OAuth 在线失败时回退到本地 Codex 模型缓存。
async function fetchProviderModelsWithFallback(
  fetchConfig: ProviderModelFetchConfig,
): Promise<{
  models: FetchedModel[];
  usedCodexCache: boolean;
  onlineErrorMessage?: string;
}> {
  if (!fetchConfig.useCodexOAuth) {
    return {
      models: await fetchModelsForConfig(
        fetchConfig.baseUrl,
        fetchConfig.apiKey,
        fetchConfig.isFullUrl,
        undefined,
        fetchConfig.customUserAgent,
        fetchConfig.volcengineModelListAction
          ? {
              action: fetchConfig.volcengineModelListAction,
              accessKeyId: fetchConfig.volcengineAccessKeyId ?? "",
              secretAccessKey: fetchConfig.volcengineSecretAccessKey ?? "",
            }
          : undefined,
      ),
      usedCodexCache: false,
    };
  }

  try {
    return {
      models: await fetchCodexOauthModels(fetchConfig.codexOAuthAccountId),
      usedCodexCache: false,
    };
  } catch (error) {
    const onlineErrorMessage = workspaceErrorMessage(error);
    const cachedModels = await fetchCodexOauthCachedModels();
    return {
      models: cachedModels,
      usedCodexCache: cachedModels.length > 0,
      onlineErrorMessage,
    };
  }
}

/// 提取普通 Codex provider 的 /models 读取配置；官方 OAuth/缺少普通端点的 provider 不走这里。
function getProviderModelFetchConfig(
  provider: Provider,
): ProviderModelFetchConfig {
  const settings = provider.settingsConfig ?? {};
  const configText = typeof settings.config === "string" ? settings.config : "";
  const auth =
    settings.auth &&
    typeof settings.auth === "object" &&
    !Array.isArray(settings.auth)
      ? (settings.auth as Record<string, unknown>)
      : {};
  const baseUrl = String(
    settings.base_url ??
      settings.baseURL ??
      settings.baseUrl ??
      getCodexBaseUrl(provider) ??
      "",
  ).trim();
  const apiKey = String(
    auth.OPENAI_API_KEY ??
      settings.apiKey ??
      settings.api_key ??
      extractCodexExperimentalBearerToken(configText) ??
      "",
  ).trim();
  const planFetchSource = {
    baseUrl,
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
    apiKey,
    accessKeyId: provider.meta?.usage_script?.accessKeyId,
    secretAccessKey: provider.meta?.usage_script?.secretAccessKey,
  };
  const planModelListAction = codexPlanModelListAction(planFetchSource);
  const isCatalogOnlyPlan = isCodexCatalogOnlyPlanModelFetch(planFetchSource);
  const isOfficialLike = isWizardCodexOAuthSource(provider);

  if (isOfficialLike) {
    return {
      baseUrl,
      apiKey,
      isFullUrl: false,
      useCodexOAuth: true,
      codexOAuthAccountId: readWizardCodexOAuthAccountId(provider),
    };
  }
  if (!baseUrl) {
    return {
      baseUrl,
      apiKey,
      isFullUrl: false,
      skipReason: "缺少 Base URL，无法读取模型列表。",
    };
  }
  if (isCatalogOnlyPlan) {
    const hasModelCatalog = readCodexModelCatalog(provider).models.some(
      (model) => (model.model ?? "").trim(),
    );
    return {
      baseUrl,
      apiKey,
      isFullUrl: false,
      skipReason: codexCatalogOnlyPlanModelFetchMessage(
        hasModelCatalog,
        planFetchSource,
      ),
    };
  }
  if (!apiKey && !planModelListAction) {
    return {
      baseUrl,
      apiKey,
      isFullUrl: false,
      skipReason: "缺少 API Key，无法读取模型列表。",
    };
  }

  return {
    baseUrl,
    apiKey,
    isFullUrl: Boolean(provider.meta?.isFullUrl ?? settings.isFullUrl),
    customUserAgent:
      typeof provider.meta?.customUserAgent === "string"
        ? provider.meta.customUserAgent
        : typeof settings.customUserAgent === "string"
          ? settings.customUserAgent
          : undefined,
    ...(planModelListAction
      ? {
          volcengineModelListAction: planModelListAction,
          volcengineAccessKeyId: planFetchSource.accessKeyId,
          volcengineSecretAccessKey: planFetchSource.secretAccessKey,
        }
      : {}),
  };
}

/// 将远端模型结果写回 provider 的 modelCatalog；普通源保留用户筛选，OAuth 源则追加官方新发布模型。
function providerWithFetchedModelCatalog(
  provider: Provider,
  fetchedModels: FetchedModel[],
): Provider {
  const currentCatalog = readCodexModelCatalog(provider);
  const fetchConfig = getProviderModelFetchConfig(provider);
  const models = currentCatalog.models.map((model) => {
    const id = model.model?.trim();
    return {
      model: id ?? "",
      ...(model.upstreamModel ? { upstreamModel: model.upstreamModel } : {}),
      ...(model.upstream_model ? { upstream_model: model.upstream_model } : {}),
      ...(model.displayName ? { displayName: model.displayName } : {}),
      ...(model.display_name ? { display_name: model.display_name } : {}),
      ...(model.contextWindow ? { contextWindow: model.contextWindow } : {}),
      ...(model.context_window ? { context_window: model.context_window } : {}),
      ...(model.inputModalities
        ? { inputModalities: model.inputModalities }
        : {}),
      ...(model.input_modalities
        ? { input_modalities: model.input_modalities }
        : {}),
      ...(model.textOnly !== undefined ? { textOnly: model.textOnly } : {}),
      ...(model.text_only !== undefined ? { text_only: model.text_only } : {}),
      ...(model.supportsImage !== undefined
        ? { supportsImage: model.supportsImage }
        : {}),
      ...(model.supports_image !== undefined
        ? { supports_image: model.supports_image }
        : {}),
      ...(model.vision !== undefined ? { vision: model.vision } : {}),
    } satisfies CodexCatalogModelDraft;
  });
  const byFetchedModel = new Map<string, number>();
  const byVisibleModel = new Map<string, number>();
  for (const model of currentCatalog.models) {
    const id = model.model?.trim();
    if (!id) continue;
    const index = models.findIndex((item) => item.model === id);
    if (index < 0) continue;
    byVisibleModel.set(id, index);
    const upstreamModel = catalogDraftUpstreamModel(models[index]);
    if (upstreamModel) {
      byFetchedModel.set(upstreamModel, index);
    }
  }
  const shouldAppendFetchedModels =
    currentCatalog.models.length === 0 || isWizardCodexOAuthSource(provider);

  for (const fetched of fetchedModels) {
    const id = fetched.id.trim();
    if (!id) continue;
    const existingIndex = byFetchedModel.get(id) ?? byVisibleModel.get(id);
    const contextWindow = resolveFetchedCodexModelContextWindow(fetched, {
      providerId: provider.id,
      providerName: provider.name,
      baseUrl: fetchConfig.baseUrl,
      existingModels: currentCatalog.models,
    });
    if (existingIndex !== undefined) {
      models[existingIndex] = {
        ...models[existingIndex],
        ...(contextWindow ? { contextWindow } : {}),
        ...(fetched.inputModalities && fetched.inputModalities.length > 0
          ? {
              inputModalities: fetched.inputModalities,
              input_modalities: fetched.inputModalities,
            }
          : {}),
        ...(fetched.supportsImage !== undefined &&
        fetched.supportsImage !== null
          ? {
              supportsImage: fetched.supportsImage,
              supports_image: fetched.supportsImage,
            }
          : {}),
      };
      continue;
    }
    if (!shouldAppendFetchedModels) continue;
    const nextModel: CodexCatalogModelDraft = {
      model: id,
      upstreamModel: id,
      displayName: id,
      ...(contextWindow ? { contextWindow } : {}),
      ...(fetched.inputModalities && fetched.inputModalities.length > 0
        ? {
            inputModalities: fetched.inputModalities,
            input_modalities: fetched.inputModalities,
          }
        : {}),
      ...(fetched.supportsImage !== undefined && fetched.supportsImage !== null
        ? {
            supportsImage: fetched.supportsImage,
            supports_image: fetched.supportsImage,
          }
        : {}),
    };
    byFetchedModel.set(id, models.length);
    byVisibleModel.set(id, models.length);
    models.push(nextModel);
  }

  return {
    ...provider,
    settingsConfig: {
      ...provider.settingsConfig,
      modelCatalog: {
        models,
        spawnAgentModels: normalizeCodexSpawnAgentModels(
          currentCatalog.spawnAgentModels,
          models,
        ),
      },
    },
  };
}

/// 把监听地址转换成客户端可连接的 host；0.0.0.0/:: 只能绑定，不能直接作为 Codex base_url。
export function codexProxyConnectHost(listenAddress: string): string {
  const trimmed = listenAddress.trim();
  if (trimmed === "0.0.0.0") return "127.0.0.1";
  if (trimmed === "::") return "::1";
  return trimmed || DEFAULT_CODEX_PROXY_LISTEN_ADDRESS;
}

/// 根据监听地址和端口生成 Codex Desktop 实际使用的 OpenAI Responses base_url。
export function buildCodexProxyBaseUrl(
  listenAddress: string,
  listenPort: number,
): string {
  const connectHost = codexProxyConnectHost(listenAddress);
  const hostForUrl =
    connectHost.includes(":") && !connectHost.startsWith("[")
      ? `[${connectHost}]`
      : connectHost;
  return `http://${hostForUrl}:${listenPort}/v1`;
}

/// 校验 MultiRouter 设置页里的本地代理监听草稿，避免保存空地址或非法端口导致接管配置不可用。
export function validateProxyListenDraft(
  listenAddress: string,
  listenPort: string,
): ProxyListenDraftValidation {
  const address = listenAddress.trim() || DEFAULT_CODEX_PROXY_LISTEN_ADDRESS;
  const portText = listenPort.trim();
  if (!/^\d+$/.test(portText)) {
    return { ok: false, error: "监听端口必须是 1024-65535 之间的数字。" };
  }
  const port = Number.parseInt(portText, 10);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    return { ok: false, error: "监听端口必须是 1024-65535 之间的数字。" };
  }
  return {
    ok: true,
    listenAddress: address,
    listenPort: port,
    baseUrl: buildCodexProxyBaseUrl(address, port),
  };
}

/// 汇总当前 MultiRouter 的运行态；只有当前方案已发布为 Codex provider 且代理/接管/入口/规则齐全才算运行中。
export function buildMultiRouterRuntimeStatus({
  selectedPlan,
  selectedRouting,
  enabledRouteCount,
  isProxyRunning,
  isCodexTakeoverActive,
  activeProviderId,
}: {
  selectedPlan: Provider | null;
  selectedRouting: CodexRouting | null;
  enabledRouteCount: number;
  isProxyRunning: boolean;
  isCodexTakeoverActive: boolean;
  activeProviderId?: string;
}): MultiRouterRuntimeStatus {
  if (!selectedPlan) {
    return {
      running: false,
      label: "未选择",
      detail: "当前没有选中的 MultiRouter。",
      tone: "warn",
    };
  }
  if (activeProviderId !== selectedPlan.id) {
    return {
      running: false,
      label: "未发布",
      detail: `当前 Codex provider 是 ${activeProviderId || "未设置"}，不是 ${selectedPlan.id}。`,
      tone: "warn",
    };
  }
  if (!isProxyRunning) {
    return {
      running: false,
      label: "代理未启动",
      detail: "本地 15721 接管代理未监听，Codex 请求不会进入 MultiRouter。",
      tone: "warn",
    };
  }
  if (!isCodexTakeoverActive) {
    return {
      running: false,
      label: "Codex 未接管",
      detail: "Codex live config 尚未指向本地代理。",
      tone: "warn",
    };
  }
  if (selectedRouting?.enabled === false) {
    return {
      running: false,
      label: "入口关闭",
      detail: "当前 MultiRouter 入口已关闭，规则会保留但不参与分流。",
      tone: "warn",
    };
  }
  if (enabledRouteCount === 0) {
    return {
      running: false,
      label: "无启用规则",
      detail:
        "当前 MultiRouter 没有启用中的路由规则，Codex 请求无法按 model 分流。",
      tone: "warn",
    };
  }
  return {
    running: true,
    label: "运行中",
    detail:
      "当前 MultiRouter 已作为 Codex provider 启动，Codex 请求会进入本地代理分流。",
    tone: "ok",
  };
}

/// 从 Provider 私有配置里读取 Codex 多模型路由配置；没有配置时返回 null，避免把普通模型源误判成路由方案。
export function readCodexRouting(
  provider: Provider | null | undefined,
): CodexRouting | null {
  if (!provider) return null;
  const routing = provider.settingsConfig?.codexRouting;
  if (!routing || typeof routing !== "object") return null;
  if (Array.isArray(routing)) {
    return {
      enabled: true,
      routes: routing.map(normalizeLegacyCodexRoutingRoute),
    };
  }
  return routing as CodexRouting;
}

/// 将旧版扁平 route 数组条目转换成新版 `codexRouting.routes[]` 条目，避免保存时清空历史路由。
function normalizeLegacyCodexRoutingRoute(
  route: any,
  index: number,
): CodexRoute {
  const models = Array.isArray(route?.models)
    ? route.models.filter(
        (item: unknown): item is string => typeof item === "string",
      )
    : Array.isArray(route?.match?.models)
      ? route.match.models.filter(
          (item: unknown): item is string => typeof item === "string",
        )
      : [];
  const prefixes = Array.isArray(route?.modelPrefixes)
    ? route.modelPrefixes
    : Array.isArray(route?.model_prefixes)
      ? route.model_prefixes
      : Array.isArray(route?.match?.prefixes)
        ? route.match.prefixes
        : [];
  const upstream = route?.upstream ?? {};
  return {
    id: String(route?.id || `route-${index + 1}`),
    label: typeof route?.label === "string" ? route.label : route?.name,
    enabled: route?.enabled !== false,
    targetProviderId:
      route?.targetProviderId ??
      route?.target_provider_id ??
      route?.providerId ??
      route?.provider_id ??
      upstream?.targetProviderId ??
      upstream?.target_provider_id ??
      upstream?.providerId ??
      upstream?.provider_id ??
      route?.provider ??
      upstream?.provider,
    match: {
      models,
      prefixes: prefixes.filter(
        (item: unknown): item is string => typeof item === "string",
      ),
    },
    upstream: {
      baseUrl:
        upstream?.baseUrl ??
        upstream?.baseURL ??
        upstream?.base_url ??
        route?.baseUrl ??
        route?.baseURL ??
        route?.base_url,
      apiFormat:
        upstream?.apiFormat ??
        upstream?.wireApi ??
        upstream?.wire_api ??
        route?.apiFormat ??
        route?.wireApi ??
        route?.wire_api,
      auth: upstream?.auth ?? route?.auth,
      modelMap: upstream?.modelMap ?? route?.modelMap,
    },
    capabilities: route?.capabilities,
  };
}

/// 判断一个 Provider 是否已经承载 Codex 多模型路由；即使暂时关闭，只要有规则也归为路由方案方便继续编辑。
export function isRoutingPlan(provider: Provider): boolean {
  const routing = readCodexRouting(provider);
  return Boolean(
    routing && (routing.enabled !== false || (routing.routes?.length ?? 0) > 0),
  );
}

/// 提取 route 的上游协议名，兼容历史字段和 UI 字段。
function routeApiFormat(route: CodexRoute): string {
  return (
    route.upstream?.apiFormat ??
    route.upstream?.wireApi ??
    route.upstream?.wire_api ??
    "openai_chat"
  );
}

/// 提取 route 引用的真实目标 Provider ID。
function routeTargetProviderId(route: CodexRoute): string | undefined {
  return [
    route.targetProviderId,
    route.target_provider_id,
    route.providerId,
    route.provider_id,
    route.upstreamProviderId,
    route.upstream_provider_id,
    route.provider,
    route.upstream?.targetProviderId,
    route.upstream?.target_provider_id,
    route.upstream?.providerId,
    route.upstream?.provider_id,
    route.upstream?.upstreamProviderId,
    route.upstream?.upstream_provider_id,
    route.upstream?.provider,
  ]
    .map((value) => value?.trim())
    .find(Boolean);
}

/// 查找 route 引用的真实目标 Provider。
function routeTargetProvider(
  route: CodexRoute,
  providersById: Map<string, Provider>,
): Provider | undefined {
  const targetProviderId = routeTargetProviderId(route);
  return targetProviderId ? providersById.get(targetProviderId) : undefined;
}

/// 把 provider 或 route 标识清理成稳定的路由 ID 片段；空值回退到 fallback，避免保存后出现不可选规则。
function safeRouteIdPart(value: string | undefined, fallback: string): string {
  const normalized = (value ?? "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || fallback;
}

/// 在候选路由集合内生成不冲突的 ID；已有配置优先保留，新增 provider 才追加序号。
function uniqueRouteId(preferredId: string, usedIds: Set<string>): string {
  const base = safeRouteIdPart(preferredId, "route");
  if (!usedIds.has(base)) {
    usedIds.add(base);
    return base;
  }

  let index = 2;
  while (usedIds.has(`${base}-${index}`)) index += 1;
  const nextId = `${base}-${index}`;
  usedIds.add(nextId);
  return nextId;
}

/// 从真实模型目录里推断精确模型名；没有目录时只读取显式单模型字段，不能伪造 OAuth 模型权限。
function collectProviderModelIds(provider: Provider): string[] {
  const catalogModels = readCodexModelCatalog(provider)
    .models.map((model) => model.model?.trim())
    .filter((model): model is string => Boolean(model));
  const singleModelFields = [
    provider.settingsConfig?.model,
    provider.settingsConfig?.defaultModel,
    provider.settingsConfig?.default_model,
  ].filter(
    (model): model is string => typeof model === "string" && !!model.trim(),
  );
  return Array.from(new Set([...catalogModels, ...singleModelFields]));
}

/// 从 provider 的 catalog 生成 route 级别模型映射；MultiRouter 后端只物化目标 provider，
/// 因此可见别名必须随 route 保存，不能依赖未改写的源 provider catalog。
function buildRouteModelMapFromProvider(
  provider: Provider,
): Record<string, string> | undefined {
  const entries = readCodexModelCatalog(provider)
    .models.map((model) => {
      const visibleModel = model.model?.trim();
      const upstreamModel = (
        model.upstreamModel ??
        model.upstream_model ??
        model.model ??
        ""
      ).trim();
      return visibleModel && upstreamModel && visibleModel !== upstreamModel
        ? [visibleModel, upstreamModel]
        : null;
    })
    .filter((entry): entry is [string, string] => Boolean(entry));
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

/// 归一化模型名尾段，用于保守识别已知纯文本模型，避免 provider 名称大小写或平台前缀影响判断。
function normalizeModelTailForCapability(model: string): string {
  const normalized = model.trim().toLowerCase();
  return normalized.split("/").pop() ?? normalized;
}

/// 少量内置纯文本模型兜底；只在 provider/catalog 没有显式能力声明时使用，避免把未知多模态模型误降级。
function modelNameLooksTextOnly(model: string): boolean {
  const tail = normalizeModelTailForCapability(model);
  const compactTail = tail.replace(/[^a-z0-9]/g, "");
  const exactTextOnlyModels = new Set([
    "ark-code-latest",
    "deepseek-chat",
    "deepseek-reasoner",
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "glm-5.1",
    "kat-coder",
    "kat-coder-pro",
    "kat-coder-pro-v1",
    "kat-coder-pro-v2",
    "ling-2.5-1t",
    "longcat-flash-chat",
    "mimo-v2.5-pro",
    "us.deepseek.r1-v1",
  ]);
  return (
    compactTail.startsWith("deepseekv4") ||
    exactTextOnlyModels.has(tail) ||
    tail.startsWith("minimax-m2.7") ||
    tail.startsWith("qwen3-coder") ||
    tail.startsWith("step-3.5-flash")
  );
}

/// 从单个 catalog 模型条目读取图片输入能力；显式字段优先于模型名兜底。
function imageSupportFromCatalogModel(
  model: CodexCatalogModel | CodexCatalogModelDraft,
): boolean | undefined {
  const supportsImage = model.supportsImage ?? model.supports_image;
  if (typeof supportsImage === "boolean") return supportsImage;
  if (typeof model.vision === "boolean") return model.vision;
  const textOnly = model.textOnly ?? model.text_only;
  if (typeof textOnly === "boolean") return !textOnly;
  const modalities = model.inputModalities ?? model.input_modalities;
  if (Array.isArray(modalities)) {
    return modalities.some((item) => item.toLowerCase() === "image");
  }
  return undefined;
}

/// 把图片能力结论转成 route/catalog 都能消费的统一能力对象。
function capabilitiesFromImageSupport(
  supportsImage: boolean | undefined,
): CodexRouteCapabilities | undefined {
  if (supportsImage === undefined) return undefined;
  return supportsImage
    ? { inputModalities: ["text", "image"], textOnly: false }
    : { inputModalities: ["text"], textOnly: true };
}

/// 基于目标 provider 的 catalog 和 route 匹配模型推断能力；不会把未知模型默认标成图文。
function inferRouteCapabilitiesFromProvider(
  provider: Provider,
  modelIds: string[],
): CodexRouteCapabilities | undefined {
  const catalogModels = readCodexModelCatalog(provider).models;
  const modelSet = new Set(
    modelIds.map((model) => model.trim()).filter(Boolean),
  );
  const relevantCatalogModels =
    modelSet.size > 0
      ? catalogModels.filter(
          (model) => model.model && modelSet.has(model.model),
        )
      : catalogModels;
  const imageSupport = relevantCatalogModels
    .map(imageSupportFromCatalogModel)
    .find((value): value is boolean => value !== undefined);
  if (imageSupport !== undefined) {
    return capabilitiesFromImageSupport(imageSupport);
  }

  const relevantModelIds =
    modelSet.size > 0
      ? Array.from(modelSet)
      : catalogModels
          .map((model) => model.model?.trim())
          .filter((model): model is string => Boolean(model));
  if (
    relevantModelIds.length > 0 &&
    relevantModelIds.every(modelNameLooksTextOnly)
  ) {
    return capabilitiesFromImageSupport(false);
  }
  return undefined;
}

/// 旧版 MultiRouter 候选曾经给所有 route 写入“图文 + 推理”的默认能力；这不是用户显式配置。
function isLegacyDefaultRouteCapabilities(
  capabilities?: CodexRouteCapabilities,
): boolean {
  if (!capabilities) return false;
  const modalities = capabilities.inputModalities ?? [];
  return (
    capabilities.textOnly === false &&
    capabilities.supportsReasoning === true &&
    modalities.length === 2 &&
    modalities.includes("text") &&
    modalities.includes("image")
  );
}

/// 用目标 provider 的真实能力修正 route；保留用户显式写入的非旧默认能力。
function normalizeRouteCapabilitiesFromProvider(
  route: CodexRoute,
  provider?: Provider,
): CodexRoute {
  if (!provider) return route;
  const inferred = inferRouteCapabilitiesFromProvider(
    provider,
    route.match?.models ?? collectProviderModelIds(provider),
  );
  if (!inferred) return route;
  if (
    route.capabilities &&
    !isLegacyDefaultRouteCapabilities(route.capabilities)
  ) {
    return route;
  }
  return { ...route, capabilities: inferred };
}

/// 从模型源 catalog 条目构造 MultiRouter 自己的 catalog 草稿；保留上下文窗口字段供 Codex 与第三方 API 继续透传。
function catalogDraftFromSourceModel(
  id: string,
  source?: CodexCatalogModelDraft | CodexCatalogModel,
): CodexCatalogModelDraft {
  const displayName = source?.displayName ?? source?.display_name;
  const upstreamModel = source?.upstreamModel ?? source?.upstream_model;
  const contextWindow = source?.contextWindow ?? source?.context_window;
  const capabilities = capabilitiesFromImageSupport(
    source ? imageSupportFromCatalogModel(source) : undefined,
  );
  return {
    model: id,
    ...(upstreamModel && upstreamModel !== id ? { upstreamModel } : {}),
    ...(displayName ? { displayName } : {}),
    ...(contextWindow ? { contextWindow } : {}),
    ...(source?.inputModalities
      ? { inputModalities: source.inputModalities }
      : {}),
    ...(source?.input_modalities
      ? { input_modalities: source.input_modalities }
      : {}),
    ...(source?.textOnly !== undefined ? { textOnly: source.textOnly } : {}),
    ...(source?.text_only !== undefined ? { text_only: source.text_only } : {}),
    ...(source?.supportsImage !== undefined
      ? { supportsImage: source.supportsImage }
      : {}),
    ...(source?.supports_image !== undefined
      ? { supports_image: source.supports_image }
      : {}),
    ...(source?.vision !== undefined ? { vision: source.vision } : {}),
    ...(capabilities ? { capabilities } : {}),
  };
}

/// 汇总所有模型源的模型目录；对象 catalog 优先，字符串模型名作为无元数据兜底。
function buildModelCatalogDraftFromSources(
  modelSources: Provider[],
): CodexCatalogModelDraft[] {
  const byModel = new Map<string, CodexCatalogModelDraft>();

  for (const provider of modelSources) {
    const sourceCatalogModels = readCodexModelCatalog(provider).models;
    for (const catalogModel of sourceCatalogModels) {
      const id = catalogModel.model?.trim();
      if (!id || byModel.has(id)) continue;
      byModel.set(id, catalogDraftFromSourceModel(id, catalogModel));
    }

    for (const model of collectProviderModelIds(provider)) {
      const id = model.trim();
      if (!id || byModel.has(id)) continue;
      byModel.set(id, { model: id });
    }
  }

  return Array.from(byModel.values());
}

/// 根据 provider 名称和模型名推断少量前缀；只作为无精确模型目录时的兜底，避免把路由规则做成空匹配。
function inferProviderPrefixes(
  provider: Provider,
  modelIds: string[],
): string[] {
  const text = `${provider.id} ${provider.name}`.toLowerCase();
  const prefixes = new Set<string>();
  const knownPrefixes = [
    "gpt",
    "o1",
    "o3",
    "o4",
    "qwen",
    "deepseek",
    "glm",
    "gemini",
    "claude",
  ];
  for (const prefix of knownPrefixes) {
    if (
      text.includes(prefix) ||
      modelIds.some((model) => model.toLowerCase().startsWith(prefix))
    ) {
      prefixes.add(prefix);
    }
  }
  if (text.includes("openai")) {
    ["gpt", "o1", "o3", "o4"].forEach((prefix) => prefixes.add(prefix));
  }
  return Array.from(prefixes);
}

/// 已保存的历史 route 可能没有 match 条件；编辑时用目标 Provider 的目录和名称推断一次，保存后写回稳定规则。
function enrichRouteMatchFromProvider(
  route: CodexRoute,
  provider?: Provider,
): CodexRoute {
  const existingModels = route.match?.models ?? [];
  const existingPrefixes = route.match?.prefixes ?? [];
  if (!provider || existingModels.length > 0 || existingPrefixes.length > 0) {
    return route;
  }
  const modelIds = collectProviderModelIds(provider);
  return {
    ...route,
    match: {
      models: modelIds,
      prefixes: inferProviderPrefixes(provider, modelIds),
    },
  };
}

/// 为普通模型源创建一条引用 provider 配置的路由；不复制 API Key/Base URL，避免工作台把来源配置写散。
function createRouteFromProvider(
  provider: Provider,
  usedIds: Set<string>,
): CodexRoute {
  const modelIds = collectProviderModelIds(provider);
  const prefixes = inferProviderPrefixes(provider, modelIds);
  const capabilities = inferRouteCapabilitiesFromProvider(provider, modelIds);
  const modelMap = buildRouteModelMapFromProvider(provider);
  return {
    id: uniqueRouteId(`router-${provider.id}`, usedIds),
    label: provider.name,
    enabled: true,
    targetProviderId: provider.id,
    match: {
      models: modelIds,
      prefixes,
    },
    upstream: {
      apiFormat: provider.meta?.apiFormat ?? "openai_chat",
      auth: { source: "provider_config" },
      ...(modelMap ? { modelMap } : {}),
    },
    ...(capabilities ? { capabilities } : {}),
  };
}

/// 合并现有 route 和所有普通模型源，给规则页提供“直接勾选候选 router”的完整候选列表。
function buildRouteCandidates(
  selectedPlan: Provider | null,
  modelSources: Provider[],
): RouteCandidate[] {
  const routableModelSources = resolveWizardModelNameCollisions(modelSources);
  const usedIds = new Set<string>();
  const candidates: RouteCandidate[] = [];
  const existingRoutes = selectedPlan
    ? dedupeCodexRoutesBySemanticProvider(
        readCodexRouting(selectedPlan)?.routes ?? [],
        routableModelSources,
      )
    : [];

  for (const route of existingRoutes) {
    const targetProviderId = routeTargetProviderId(route);
    const id = uniqueRouteId(
      route.id ?? targetProviderId ?? route.label ?? "route",
      usedIds,
    );
    const normalizedRoute: CodexRoute = { ...route, id };
    const provider =
      (targetProviderId
        ? routableModelSources.find((source) => source.id === targetProviderId)
        : undefined) ??
      findSemanticRouteProvider(normalizedRoute, routableModelSources);
    const routeWithInferredMatch = enrichRouteMatchFromProvider(
      normalizedRoute,
      provider,
    );
    const routeWithInferredCapabilities =
      normalizeRouteCapabilitiesFromProvider(routeWithInferredMatch, provider);
    candidates.push({
      id,
      route: routeWithInferredCapabilities,
      provider,
      isExisting: true,
      matchModels: routeWithInferredCapabilities.match?.models ?? [],
      matchPrefixes: routeWithInferredCapabilities.match?.prefixes ?? [],
    });
  }

  const existingProviderIds = new Set(
    candidates
      .map(
        (candidate) =>
          candidate.provider?.id ?? routeTargetProviderId(candidate.route),
      )
      .filter((id): id is string => Boolean(id)),
  );
  for (const provider of routableModelSources) {
    if (existingProviderIds.has(provider.id)) continue;
    const route = createRouteFromProvider(provider, usedIds);
    candidates.push({
      id: route.id!,
      route,
      provider,
      isExisting: false,
      matchModels: route.match?.models ?? [],
      matchPrefixes: route.match?.prefixes ?? [],
    });
  }

  return candidates;
}

/// 初次打开候选选择器时，根据已保存规则和入口意图生成“是否加入”的本地草稿。
function buildInitialRoutePickerSelectedIds(
  candidates: RouteCandidate[],
  selectAllByDefault?: boolean,
): Set<string> {
  return new Set(
    candidates
      .filter((candidate) => selectAllByDefault || candidate.isExisting)
      .map((candidate) => candidate.id),
  );
}

/// 初次打开候选选择器时，根据已保存规则和入口意图生成“是否启用”的本地草稿。
function buildInitialRoutePickerEnabledIds(
  candidates: RouteCandidate[],
  selectAllByDefault?: boolean,
): Set<string> {
  return new Set(
    candidates
      .filter(
        (candidate) => selectAllByDefault || candidate.route.enabled !== false,
      )
      .map((candidate) => candidate.id),
  );
}

/// 候选列表刷新时只为新出现的 router 应用默认值，已有候选保留用户尚未保存的勾选/启用草稿。
export function mergeRoutePickerDraftIds(
  currentIds: Set<string>,
  previousCandidateIds: string[],
  nextCandidateIds: string[],
  defaultIncludedIds: string[],
): Set<string> {
  const previousCandidateIdSet = new Set(previousCandidateIds);
  const nextCandidateIdSet = new Set(nextCandidateIds);
  const nextIds = new Set(
    Array.from(currentIds).filter((id) => nextCandidateIdSet.has(id)),
  );

  for (const id of defaultIncludedIds) {
    if (!previousCandidateIdSet.has(id) && nextCandidateIdSet.has(id)) {
      nextIds.add(id);
    }
  }

  return nextIds;
}

/// 把候选选择器里的宽松 route 规整成后端路由器可直接消费的稳定结构。
export function normalizeCodexRouteForSave(
  route: CodexRoute,
  index: number,
  usedIds: Set<string>,
): CodexRoute {
  const id = uniqueRouteId(
    route.id ??
      routeTargetProviderId(route) ??
      route.label ??
      `route-${index + 1}`,
    usedIds,
  );
  return {
    ...route,
    id,
    enabled: route.enabled !== false,
    targetProviderId: routeTargetProviderId(route),
    match: {
      models: route.match?.models ?? [],
      prefixes: route.match?.prefixes ?? [],
    },
    upstream: {
      ...route.upstream,
      apiFormat: routeApiFormat(route),
      auth: route.upstream?.auth ?? { source: "provider_config" },
    },
  };
}

/// 建立当前方案里已有 catalog 的索引；修复旧 route 时用它找回可见名背后的真实上游模型。
function buildExistingCatalogByModel(
  plan: Provider,
): Map<string, CodexCatalogModelDraft> {
  const existingCatalog = plan.settingsConfig?.modelCatalog;
  const existingModels = Array.isArray(existingCatalog?.models)
    ? (existingCatalog.models as CodexCatalogModelDraft[])
    : [];
  const existingModelById = new Map<string, CodexCatalogModelDraft>();
  for (const model of existingModels) {
    const id = model.model?.trim();
    if (id) existingModelById.set(id, model);
  }
  return existingModelById;
}

/// 解析 route 中某个可见模型真正要发给上游的模型名；route modelMap 优先于聚合 catalog。
function routeModelUpstreamForAliasRepair(
  route: CodexRoute,
  visibleModel: string,
  existingModelById: Map<string, CodexCatalogModelDraft>,
): string {
  const catalogModel = existingModelById.get(visibleModel);
  return (
    route.upstream?.modelMap?.[visibleModel] ??
    catalogModel?.upstreamModel ??
    catalogModel?.upstream_model ??
    visibleModel
  ).trim();
}

/// 手动保存 routes 前按 collision-resolved provider catalog 修复可见模型名。
///
/// 运行时只能根据 `request.model` 字符串匹配 route；如果官方和中转同时保存
/// `gpt-*` 这类同名 exact match，就会按 route 顺序抢路由。这里在保存前把非
/// canonical provider 的可见模型改成稳定别名，并同步写回 route 级 modelMap。
export function normalizeCodexRoutesForVisibleModelAliases(
  plan: Provider,
  routes: CodexRoute[],
  providersById: Map<string, Provider>,
): CodexRoute[] {
  const existingModelById = buildExistingCatalogByModel(plan);
  return routes.map((route) => {
    const targetProvider = routeTargetProvider(route, providersById);
    const targetCatalogModels = targetProvider
      ? readCodexModelCatalog(targetProvider).models
      : [];
    if (!targetProvider || targetCatalogModels.length === 0) return route;

    const targetModelByUpstream = new Map<
      string,
      CodexCatalogModel | CodexCatalogModelDraft
    >();
    const targetModelByVisible = new Map<
      string,
      CodexCatalogModel | CodexCatalogModelDraft
    >();
    for (const model of targetCatalogModels) {
      const visible = model.model?.trim();
      if (!visible) continue;
      targetModelByVisible.set(visible, model);
      const upstream = catalogDraftUpstreamModel(model);
      if (upstream && !targetModelByUpstream.has(upstream)) {
        targetModelByUpstream.set(upstream, model);
      }
    }

    const nextModels: string[] = [];
    for (const visibleModel of route.match?.models ?? []) {
      const trimmedVisible = visibleModel.trim();
      if (!trimmedVisible) continue;
      const upstream = routeModelUpstreamForAliasRepair(
        route,
        trimmedVisible,
        existingModelById,
      );
      const targetModel =
        targetModelByUpstream.get(upstream) ??
        targetModelByVisible.get(trimmedVisible);
      const nextVisible = targetModel?.model?.trim() ?? trimmedVisible;
      if (nextVisible && !nextModels.includes(nextVisible)) {
        nextModels.push(nextVisible);
      }
    }

    const nextModelMapEntries = nextModels
      .map((visibleModel) => {
        const targetModel = targetModelByVisible.get(visibleModel);
        const upstream = targetModel
          ? catalogDraftUpstreamModel(targetModel)
          : routeModelUpstreamForAliasRepair(
              route,
              visibleModel,
              existingModelById,
            );
        return upstream && upstream !== visibleModel
          ? [visibleModel, upstream]
          : null;
      })
      .filter((entry): entry is [string, string] => Boolean(entry));
    const nextModelMap =
      nextModelMapEntries.length > 0
        ? Object.fromEntries(nextModelMapEntries)
        : undefined;
    const { modelMap: _modelMap, ...upstreamWithoutModelMap } =
      route.upstream ?? {};

    return {
      ...route,
      match: {
        ...(route.match ?? {}),
        models: nextModels,
      },
      upstream: {
        ...upstreamWithoutModelMap,
        ...(nextModelMap ? { modelMap: nextModelMap } : {}),
      },
    };
  });
}

/// 判断某个 catalog 可见模型是否真的会被当前 route 接住。
///
/// MultiRouter runtime 只按请求里的可见模型字符串做 exact/prefix 匹配；如果 catalog
/// 暴露了 route 不匹配的模型，Codex 选择器会让用户选到一个随后落入其他 route 的模型。
function routeCanMatchVisibleCatalogModel(
  route: CodexRoute,
  visibleModel: string,
): boolean {
  const model = visibleModel.trim();
  if (!model) return false;
  const lowerModel = model.toLowerCase();
  if (
    (route.match?.models ?? []).some(
      (matchedModel) => matchedModel.trim().toLowerCase() === lowerModel,
    )
  ) {
    return true;
  }
  return (route.match?.prefixes ?? []).some((prefix) => {
    const normalizedPrefix = prefix.trim().toLowerCase();
    return normalizedPrefix && lowerModel.startsWith(normalizedPrefix);
  });
}

/// route 能力比 provider catalog 更接近最终路由结果；写入聚合 catalog，确保 Codex 看到的模型能力与规则一致。
function applyRouteCapabilitiesToCatalogModel(
  model: CodexCatalogModelDraft,
  route: CodexRoute,
): CodexCatalogModelDraft {
  if (!route.capabilities) return model;
  return {
    ...model,
    capabilities: route.capabilities,
    inputModalities:
      route.capabilities.inputModalities ?? model.inputModalities,
    textOnly: route.capabilities.textOnly ?? model.textOnly,
  };
}

/// 从已选 route 和目标模型源汇总 MultiRouter 的模型目录；Codex 选择器和 spawn_agent 都依赖这个目录。
export function buildModelCatalogForRoutes(
  plan: Provider,
  routes: CodexRoute[],
  providersById: Map<string, Provider>,
): CodexModelCatalogDraft {
  const existingCatalog = plan.settingsConfig?.modelCatalog;
  const existingModelById = buildExistingCatalogByModel(plan);

  const byModel = new Map<string, CodexCatalogModelDraft>();
  for (const route of routes) {
    if (route.enabled === false) continue;
    const targetProvider = routeTargetProviderId(route)
      ? providersById.get(routeTargetProviderId(route)!)
      : undefined;
    const targetCatalogModels = targetProvider
      ? readCodexModelCatalog(targetProvider).models
      : [];
    const routableCatalogModels = targetCatalogModels.filter((catalogModel) =>
      routeCanMatchVisibleCatalogModel(route, catalogModel.model ?? ""),
    );
    for (const catalogModel of routableCatalogModels) {
      const id = catalogModel.model?.trim();
      if (!id || byModel.has(id)) continue;
      byModel.set(
        id,
        applyRouteCapabilitiesToCatalogModel(
          catalogDraftFromSourceModel(id, catalogModel),
          route,
        ),
      );
    }
    for (const model of route.match?.models ?? []) {
      const id = model.trim();
      if (!id || byModel.has(id)) continue;
      byModel.set(
        id,
        applyRouteCapabilitiesToCatalogModel(
          existingModelById.get(id) ?? { model: id },
          route,
        ),
      );
    }
  }

  const existingSpawnAgentModels = Array.isArray(
    existingCatalog?.spawnAgentModels,
  )
    ? (existingCatalog.spawnAgentModels as string[])
    : [];
  const modelIds = Array.from(byModel.keys());
  const spawnAgentModels = existingSpawnAgentModels
    .filter((model) => byModel.has(model))
    .concat(
      modelIds.filter((model) => !existingSpawnAgentModels.includes(model)),
    )
    .slice(0, 5);
  return {
    models: Array.from(byModel.values()),
    spawnAgentModels,
  };
}

/// 生成工作台专用的新 MultiRouter provider；它只承载路由配置，不再让用户填写无关的上游密钥表单。
export function createDraftRoutingPlan(
  providers: Provider[],
  modelSources: Provider[],
): Provider {
  const routableModelSources = resolveWizardModelNameCollisions(modelSources);
  const existingIds = new Set(providers.map((provider) => provider.id));
  const id = uniqueRouteId("codex-multirouter", existingIds);
  const catalogModels = buildModelCatalogDraftFromSources(routableModelSources);
  const sourceModels = catalogModels.map((model) => model.model);
  const modelCatalog: CodexModelCatalogDraft = {
    models: catalogModels,
    spawnAgentModels: Array.from(new Set(sourceModels)).slice(0, 5),
  };
  return {
    id,
    name: "New Codex MultiRouter",
    category: "custom",
    settingsConfig: {
      auth: {},
      base_url: buildCodexProxyBaseUrl(
        DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
        DEFAULT_CODEX_PROXY_LISTEN_PORT,
      ),
      baseUrl: buildCodexProxyBaseUrl(
        DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
        DEFAULT_CODEX_PROXY_LISTEN_PORT,
      ),
      config: null,
      modelCatalog,
      codexRouting: {
        enabled: true,
        routes: [],
      },
    },
    createdAt: Date.now(),
  };
}

/// MultiRouter 设置页只允许修改方案元信息和入口开关；路由规则、模型目录和本地代理接管配置都继续由工作台自动维护。
export function applyMultiRouterSettingsDraft(
  plan: Provider,
  draft: MultiRouterSettingsDraft,
): Provider {
  const currentRouting = readCodexRouting(plan) ?? {};
  const nextRouting: CodexRouting = {
    ...currentRouting,
    enabled: draft.enabled,
    routes: currentRouting.routes ?? [],
  };
  const defaultRouteId = draft.defaultRouteId?.trim();
  if (
    defaultRouteId &&
    (nextRouting.routes ?? []).some((route) => route.id === defaultRouteId)
  ) {
    nextRouting.defaultRouteId = defaultRouteId;
  } else {
    delete nextRouting.defaultRouteId;
  }

  return {
    ...plan,
    name: draft.name.trim() || plan.name,
    notes: draft.notes?.trim() || undefined,
    settingsConfig: {
      ...plan.settingsConfig,
      auth: plan.settingsConfig?.auth ?? {},
      base_url:
        plan.settingsConfig?.base_url ??
        buildCodexProxyBaseUrl(
          DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
          DEFAULT_CODEX_PROXY_LISTEN_PORT,
        ),
      baseUrl:
        plan.settingsConfig?.baseUrl ??
        plan.settingsConfig?.base_url ??
        buildCodexProxyBaseUrl(
          DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
          DEFAULT_CODEX_PROXY_LISTEN_PORT,
        ),
      config: plan.settingsConfig?.config ?? null,
      codexRouting: nextRouting,
    },
  };
}

/// 提取 route 的上游地址；引用真实 Provider 时展示目标 Provider 的配置。
function routeBaseUrl(
  route: CodexRoute,
  providersById?: Map<string, Provider>,
): string {
  const target = providersById
    ? routeTargetProvider(route, providersById)
    : undefined;
  if (target) {
    const config = target.settingsConfig ?? {};
    return (
      config.base_url ??
      config.baseURL ??
      config.baseUrl ??
      `复用供应商配置：${target.name}`
    );
  }
  return (
    route.upstream?.baseUrl ?? route.upstream?.base_url ?? "继承模型源地址"
  );
}

/// 把内部认证枚举翻译成页面可理解的中文说明，避免把 provider_config 这类工程词直接丢给用户。
function authSourceLabel(source?: string): string {
  switch (source) {
    case "managed_codex_oauth":
      return "托管 Codex OAuth";
    case "managed_account":
      return "托管账号";
    case "provider_config":
      return "使用路由 API Key";
    default:
      return "继承模型源凭据";
  }
}

/// 把内部协议枚举翻译成用户能识别的接口类型。
function apiFormatLabel(format: string): string {
  switch (format) {
    case "openai_responses":
      return "OpenAI Responses";
    case "openai_messages":
      return "OpenAI Messages";
    case "openai_chat":
      return "OpenAI Chat";
    default:
      return format;
  }
}

/// 把后端协议判定来源翻译成用户能直接理解的说明。
function protocolDecisionSourceLabel(source?: string | null): string {
  switch (source) {
    case "managed_codex_oauth":
      return "官方 Codex OAuth";
    case "provider_meta_api_format":
      return "Provider meta.apiFormat";
    case "settings_api_format":
      return "Provider 配置 apiFormat";
    case "known_chat_completions_only_url":
      return "已知 Chat-only 地址";
    case "config_wire_api":
      return "config.toml wire_api";
    case "default_responses":
      return "默认 Responses";
    default:
      return source || "未探测";
  }
}

/// 汇总 route 可匹配的模型名和前缀，用于列表和测试页展示。
function routeMatchSummary(route: CodexRoute): string {
  const models = route.match?.models?.filter(Boolean) ?? [];
  const prefixes = route.match?.prefixes?.filter(Boolean) ?? [];
  const parts = [
    models.length > 0 ? `精确模型：${models.join(", ")}` : "",
    prefixes.length > 0 ? `模型前缀：${prefixes.join(", ")}` : "",
  ].filter(Boolean);
  return parts.join("；") || "尚未设置匹配条件";
}

/// 收集所有可被 Codex 请求命中的模型名，测试页会优先使用这些真实规则生成候选项。
function collectRouteModels(routes: RouteEntry[]): string[] {
  const modelNames = routes.flatMap(({ route }) => [
    ...(route.match?.models ?? []),
    ...(route.match?.prefixes ?? []).map((prefix) => `${prefix}*`),
  ]);
  return Array.from(new Set(modelNames.filter(Boolean)));
}

/// 根据当前 MultiRouter 规则反查 catalog 中真实存在的模型，用于子 Agent 候选页的“路由命中”选项卡。
function collectRoutedCatalogModels(
  routes: RouteEntry[],
  catalogModels: CodexCatalogModel[],
): string[] {
  const exactModels = new Set<string>();
  const prefixes: string[] = [];

  for (const { route } of routes) {
    for (const model of route.match?.models ?? []) {
      const normalized = model.trim();
      if (normalized) exactModels.add(normalized);
    }
    for (const prefix of route.match?.prefixes ?? []) {
      const normalized = prefix.trim();
      if (normalized) prefixes.push(normalized);
    }
  }

  const routed = catalogModels
    .map((model) => model.model?.trim())
    .filter((model): model is string => Boolean(model))
    .filter(
      (model) =>
        exactModels.has(model) ||
        prefixes.some((prefix) => model.startsWith(prefix)),
    );

  return Array.from(new Set(routed));
}

/// 判断请求模型是否命中某条 route；状态页用它把外层 router 日志重新归属到子 provider。
function routeMatchesModel(route: CodexRoute, model: string): boolean {
  const normalized = model.trim();
  if (!normalized) return false;
  const models = route.match?.models ?? [];
  const prefixes = route.match?.prefixes ?? [];
  return (
    models.includes(normalized) ||
    prefixes.some((prefix) => normalized.startsWith(prefix))
  );
}

/// 收集当前多路方案引用到的子 provider，避免状态页把普通 Codex provider 和 route target 混在一起。
function collectTargetProviderIds(
  routes: RouteEntry[],
  selectedPlan?: Provider | null,
): Set<string> {
  const ids = new Set<string>();
  for (const entry of routes) {
    if (selectedPlan && entry.provider.id !== selectedPlan.id) continue;
    const targetProviderId = routeTargetProviderId(entry.route);
    if (targetProviderId) ids.add(targetProviderId);
  }
  return ids;
}

/// 为内联 route 生成稳定统计 ID。内联 route 没有真实 providerId，但在状态页里
/// 仍然应该作为一个“子 Provider”展示，否则 Qwen/DeepSeek 会被统计成 0。
function routeTrafficId(entry: RouteEntry): string {
  const routeId =
    entry.route.id?.trim() ||
    entry.route.label?.trim() ||
    `route-${entry.index + 1}`;
  return `route:${entry.provider.id}:${routeId}`;
}

/// 把 route 映射成状态页可聚合的流量目标；优先使用真实 target provider，
/// 没有 targetProviderId 时退化为内联 route 自身。
function routeTrafficTarget(
  entry: RouteEntry,
  providersById: Map<string, Provider>,
): RouteTrafficTarget {
  const targetProviderId = routeTargetProviderId(entry.route);
  const targetProvider = targetProviderId
    ? providersById.get(targetProviderId)
    : undefined;
  if (targetProviderId) {
    return {
      providerId: targetProviderId,
      providerName: targetProvider?.name ?? targetProviderId,
    };
  }
  return {
    providerId: routeTrafficId(entry),
    providerName:
      entry.route.label?.trim() ||
      entry.route.id?.trim() ||
      `Route ${entry.index + 1}`,
  };
}

/// 从 router 日志反推 route id，兼容旧日志只写 effective_provider 的情况。
function routeIdFromRouterEvent(event: CodexRouterLogEvent): string | null {
  if (event.routeId?.trim()) return event.routeId.trim();
  const provider = event.effectiveProvider ?? event.provider ?? "";
  const marker = "::route::";
  const index = provider.indexOf(marker);
  return index >= 0
    ? provider.slice(index + marker.length).trim() || null
    : null;
}

/// 为 route 生成和后端诊断 route summary 对齐的稳定 key。
function routeEntryStatusKey(entry: RouteEntry): string {
  return (
    entry.route.id?.trim() ||
    entry.route.label?.trim() ||
    `route-${entry.index + 1}`
  );
}

/// 为后端返回的 route summary 生成稳定 key，便于前端合并协议探测结果。
function routeSummaryStatusKey(
  route: CodexRouteSummary,
  index: number,
): string {
  return route.id?.trim() || route.label?.trim() || `route-${index + 1}`;
}

/// 建立当前诊断 route summary 的查找表，避免状态页自己重复猜协议。
function buildRouteSummaryMap(
  diagnostics: CodexMultiRouterDiagnostics | null,
): Map<string, CodexRouteSummary> {
  return new Map(
    (diagnostics?.routePlan.routeSummaries ?? []).map((route, index) => [
      routeSummaryStatusKey(route, index),
      route,
    ]),
  );
}

/// 根据 route_id 或 model 匹配 router 日志对应的 route。
function routeEntryForRouterEvent(
  event: CodexRouterLogEvent,
  routes: RouteEntry[],
): RouteEntry | undefined {
  const routeId = routeIdFromRouterEvent(event);
  if (routeId) {
    const byId = routes.find(
      ({ route }) => route.id?.trim().toLowerCase() === routeId.toLowerCase(),
    );
    if (byId) return byId;
  }
  const model = event.model?.trim();
  return model
    ? routes.find(({ route }) => routeMatchesModel(route, model))
    : undefined;
}

/// router 诊断事件没有完整 token 和 latency 信息，只把可确认的 route/model
/// 请求计入请求数和失败数，避免把“没有外部 targetProviderId”误报为无流量。
function routerEventStatusCode(event: CodexRouterLogEvent): number {
  const parsed = Number.parseInt(event.status ?? "", 10);
  if (Number.isFinite(parsed)) return parsed;
  return event.event.includes("error") ? 500 : 0;
}

/// 把 route 的配置协议探测结果附着到统计行里。
function applyRouteProtocolMetadata(
  row: RouteTrafficRow,
  entry: RouteEntry | undefined,
  routeSummaries: Map<string, CodexRouteSummary>,
) {
  if (!entry) return;
  row.routeId = entry.route.id?.trim() || null;
  row.routeLabel = entry.route.label?.trim() || null;
  const summary = routeSummaries.get(routeEntryStatusKey(entry));
  if (!summary) return;
  row.configuredProtocol ??= summary.configuredProtocol;
  row.configuredProtocolSource ??= summary.configuredProtocolSource;
  row.configuredProtocolDetail ??= summary.configuredProtocolDetail;
}

/// 用最近的 request_prepared 事件覆盖统计行的真实出站协议。
function updateRouteObservedProtocol(
  row: RouteTrafficRow,
  event: CodexRouterLogEvent,
) {
  if (!event.actualProtocol) return;
  if (
    row.lastObservedAt &&
    event.timestamp.localeCompare(row.lastObservedAt) <= 0
  ) {
    return;
  }
  row.lastObservedProtocol = event.actualProtocol;
  row.lastObservedAt = event.timestamp;
  row.lastObservedUpstreamUrl = event.upstreamUrl;
  row.lastObservedEndpoint = event.effectiveEndpoint ?? event.endpoint;
}

/// 从请求日志聚合 MultiRouter 子 provider / model 流量；无法归属的日志留给状态页单独提示。
function buildRouteTrafficRows({
  logs,
  routerEvents = [],
  routes,
  selectedPlan,
  providersById,
  routeSummaries = new Map<string, CodexRouteSummary>(),
}: {
  logs: RequestLog[];
  routerEvents?: CodexRouterLogEvent[];
  routes: RouteEntry[];
  selectedPlan: Provider | null;
  providersById: Map<string, Provider>;
  routeSummaries?: Map<string, CodexRouteSummary>;
}): RouteTrafficRow[] {
  const selectedRoutes = selectedPlan
    ? routes.filter((entry) => entry.provider.id === selectedPlan.id)
    : routes;
  const targetProviderIds = collectTargetProviderIds(routes, selectedPlan);
  const buckets = new Map<
    string,
    RouteTrafficRow & { latencyTotalMs: number }
  >();

  function addTrafficSample(
    target: RouteTrafficTarget,
    model: string,
    statusCode: number,
    tokens: number,
    latencyMs: number,
    matchedRoute?: RouteEntry,
  ) {
    const key = `${target.providerId}::${model}`;
    const current =
      buckets.get(key) ??
      ({
        providerId: target.providerId,
        providerName: target.providerName,
        model,
        routeId: null,
        routeLabel: null,
        configuredProtocol: null,
        configuredProtocolSource: null,
        configuredProtocolDetail: null,
        lastObservedProtocol: null,
        lastObservedAt: null,
        lastObservedUpstreamUrl: null,
        lastObservedEndpoint: null,
        requestCount: 0,
        successCount: 0,
        failedCount: 0,
        totalTokens: 0,
        avgLatencyMs: 0,
        latencyTotalMs: 0,
      } satisfies RouteTrafficRow & { latencyTotalMs: number });

    applyRouteProtocolMetadata(current, matchedRoute, routeSummaries);
    current.requestCount += 1;
    if (statusCode >= 200 && statusCode < 400) {
      current.successCount += 1;
    } else if (statusCode >= 400) {
      current.failedCount += 1;
    }
    current.totalTokens += tokens;
    current.latencyTotalMs += latencyMs;
    current.avgLatencyMs = Math.round(
      current.latencyTotalMs / current.requestCount,
    );
    buckets.set(key, current);
  }

  for (const log of logs) {
    if (log.appType !== "codex") continue;
    if ((log.dataSource ?? "proxy") !== "proxy") continue;
    const requestedModel = log.requestModel || log.model;
    const matchedRoute = selectedRoutes.find(({ route }) =>
      routeMatchesModel(route, requestedModel),
    );
    const model = requestedModel || log.model || "unknown";
    const target = matchedRoute
      ? routeTrafficTarget(matchedRoute, providersById)
      : targetProviderIds.has(log.providerId)
        ? {
            providerId: log.providerId,
            providerName:
              providersById.get(log.providerId)?.name ??
              log.providerName ??
              log.providerId,
          }
        : undefined;
    if (!target) continue;

    addTrafficSample(
      target,
      model,
      log.statusCode,
      log.inputTokens +
        log.outputTokens +
        log.cacheReadTokens +
        log.cacheCreationTokens,
      log.latencyMs,
      matchedRoute,
    );
  }

  const terminalRouterEvents = routerEvents.filter((event) =>
    ["upstream_status", "upstream_error", "upstream_send_error"].includes(
      event.event,
    ),
  );
  const countableRouterEvents =
    terminalRouterEvents.length > 0
      ? terminalRouterEvents
      : routerEvents.filter((event) =>
          ["route_resolved", "request_prepared"].includes(event.event),
        );

  for (const event of countableRouterEvents) {
    const matchedRoute = routeEntryForRouterEvent(event, selectedRoutes);
    if (!matchedRoute) continue;
    addTrafficSample(
      routeTrafficTarget(matchedRoute, providersById),
      event.model || matchedRoute.route.match?.models?.[0] || "unknown",
      routerEventStatusCode(event),
      0,
      0,
      matchedRoute,
    );
  }

  for (const event of routerEvents.filter((entry) =>
    Boolean(entry.actualProtocol),
  )) {
    const matchedRoute = routeEntryForRouterEvent(event, selectedRoutes);
    if (!matchedRoute) continue;
    const target = routeTrafficTarget(matchedRoute, providersById);
    const model =
      event.model || matchedRoute.route.match?.models?.[0] || "unknown";
    const key = `${target.providerId}::${model}`;
    const current =
      buckets.get(key) ??
      ({
        providerId: target.providerId,
        providerName: target.providerName,
        model,
        routeId: null,
        routeLabel: null,
        configuredProtocol: null,
        configuredProtocolSource: null,
        configuredProtocolDetail: null,
        lastObservedProtocol: null,
        lastObservedAt: null,
        lastObservedUpstreamUrl: null,
        lastObservedEndpoint: null,
        requestCount: 0,
        successCount: 0,
        failedCount: 0,
        totalTokens: 0,
        avgLatencyMs: 0,
        latencyTotalMs: 0,
      } satisfies RouteTrafficRow & { latencyTotalMs: number });
    applyRouteProtocolMetadata(current, matchedRoute, routeSummaries);
    updateRouteObservedProtocol(current, event);
    buckets.set(key, current);
  }

  return Array.from(buckets.values())
    .map(({ latencyTotalMs: _latencyTotalMs, ...row }) => row)
    .sort((a, b) => b.requestCount - a.requestCount);
}

/// 显示 Codex 多模型路由工作台；它只复用 Provider 配置，不创建第二套数据库。
/// 注意：要让 Codex 请求真正进入路由，仍然必须开启 Codex app takeover，把 Codex live 配置指向本地代理。
export function CodexRouterWorkspacePage({
  providers,
  proxyStatus,
  isProxyRunning,
  isCodexTakeoverActive,
  activeProviderId,
  initialProviderId,
  initialTab = "status",
  onEditProvider,
  onDeletePlan,
  onCreateProvider,
  onRuntimeReady,
}: {
  providers: Provider[];
  proxyStatus?: ProxyStatus;
  isProxyRunning: boolean;
  isCodexTakeoverActive: boolean;
  activeProviderId?: string;
  initialProviderId?: string | null;
  initialTab?: WorkspaceTab;
  onEditProvider: (provider: Provider) => void;
  onDeletePlan: (provider: Provider) => void;
  onCreateProvider: () => void;
  onRuntimeReady?: (provider: Provider) => void;
}) {
  const [activeTab, setActiveTab] = useState<WorkspaceTab>(initialTab);
  const [selectedPlanId, setSelectedPlanId] = useState<string | null>(null);
  const [selectedRouteKey, setSelectedRouteKey] = useState<string | null>(null);
  const [testModel, setTestModel] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);
  const [isRoutePickerOpen, setIsRoutePickerOpen] = useState(false);
  const [isPlanSettingsOpen, setIsPlanSettingsOpen] = useState(false);
  const [routePickerMessage, setRoutePickerMessage] = useState<string | null>(
    null,
  );
  const [routePickerError, setRoutePickerError] = useState<string | null>(null);
  const [isSavingRoutes, setIsSavingRoutes] = useState(false);
  const [isSavingPlanSettings, setIsSavingPlanSettings] = useState(false);
  const [routePickerSelectAll, setRoutePickerSelectAll] = useState(false);
  const [optimisticRoutingPlan, setOptimisticRoutingPlan] =
    useState<Provider | null>(null);
  const [optimisticModelSourcesById, setOptimisticModelSourcesById] = useState<
    Record<string, Provider>
  >({});
  const [providerModelRefreshStates, setProviderModelRefreshStates] = useState<
    Record<string, ProviderModelRefreshState>
  >({});
  const appliedInitialNavigationRef = useRef<string | null>(null);
  const workspaceScrollRef = useRef<HTMLDivElement | null>(null);
  const modelRefreshAttemptedKeysRef = useRef<Set<string>>(new Set());
  // 记录每个 provider 当前最新的 /models 刷新 attempt；普通 rerender 会触发 effect cleanup，不能因此吞掉同批并发请求的终态。
  const modelRefreshActiveAttemptKeysRef = useRef<Record<string, string>>({});
  // 记录已超时的 attempt，避免后台迟到的 IPC 继续把 loading/error 覆盖成 success。
  const modelRefreshTimedOutAttemptKeysRef = useRef<Set<string>>(new Set());
  const queryClient = useQueryClient();

  /// 按下一版启用 Provider 集合同步失效旧刷新，避免路由保存窗口内迟到结果写回旧 catalog。
  function invalidateModelRefreshesOutside(
    enabledProviderIds: ReadonlySet<string>,
  ) {
    setProviderModelRefreshStates((current) => {
      const next = Object.fromEntries(
        Object.entries(current).filter(([providerId]) =>
          enabledProviderIds.has(providerId),
        ),
      );
      return Object.keys(next).length === Object.keys(current).length
        ? current
        : next;
    });
    for (const [providerId, attemptKey] of Object.entries(
      modelRefreshActiveAttemptKeysRef.current,
    )) {
      if (enabledProviderIds.has(providerId)) continue;
      delete modelRefreshActiveAttemptKeysRef.current[providerId];
      modelRefreshAttemptedKeysRef.current.delete(attemptKey);
      modelRefreshTimedOutAttemptKeysRef.current.delete(attemptKey);
    }
  }

  const effectiveProviders = useMemo(() => {
    const optimisticSourceEntries = Object.entries(optimisticModelSourcesById);
    const hasOptimisticSources = optimisticSourceEntries.length > 0;
    if (!optimisticRoutingPlan && !hasOptimisticSources) return providers;

    const replaced = providers.map((provider) =>
      provider.id === optimisticRoutingPlan?.id
        ? optimisticRoutingPlan
        : (optimisticModelSourcesById[provider.id] ?? provider),
    );
    if (!optimisticRoutingPlan) return replaced;
    const withRoutingPlan = providers.some(
      (provider) => provider.id === optimisticRoutingPlan.id,
    )
      ? replaced
      : [...providers, optimisticRoutingPlan];
    return withRoutingPlan;
  }, [optimisticModelSourcesById, optimisticRoutingPlan, providers]);
  const routingPlans = useMemo(
    () => effectiveProviders.filter(isRoutingPlan),
    [effectiveProviders],
  );
  const routingPlanIdSet = useMemo(
    () => new Set(routingPlans.map((provider) => provider.id)),
    [routingPlans],
  );
  const modelSources = useMemo(
    () => effectiveProviders.filter((provider) => !isRoutingPlan(provider)),
    [effectiveProviders],
  );
  const routableModelSources = useMemo(
    () => resolveWizardModelNameCollisions(modelSources),
    [modelSources],
  );
  const providersById = useMemo(
    () =>
      new Map(effectiveProviders.map((provider) => [provider.id, provider])),
    [effectiveProviders],
  );
  const routableProvidersById = useMemo(() => {
    const byId = new Map(
      effectiveProviders.map((provider) => [provider.id, provider]),
    );
    for (const source of routableModelSources) {
      byId.set(source.id, source);
    }
    return byId;
  }, [effectiveProviders, routableModelSources]);
  // 模型刷新目标必须来自当前方案的启用 route；旧 inline route 仅做语义定位，
  // 不能因缺少 targetProviderId 退回到刷新全部候选 Provider。
  const selectedPlanForModelRefresh =
    routingPlans.find((provider) => provider.id === selectedPlanId) ??
    routingPlans[0] ??
    null;
  const enabledModelSourceIdsForRefresh = useMemo(() => {
    const ids = new Set<string>();
    const routes = dedupeCodexRoutesBySemanticProvider(
      readCodexRouting(selectedPlanForModelRefresh)?.routes ?? [],
      modelSources,
    );
    for (const route of routes) {
      if (route.enabled === false) continue;
      const providerId = routeSemanticProviderId(route, modelSources);
      if (providerId) ids.add(providerId);
    }
    return ids;
  }, [modelSources, selectedPlanForModelRefresh]);

  // route 停用后立即移除旧刷新卡片，并让进行中的结果失效；同时释放最新去重键，
  // 保证用户重新启用该 route 时可以重新读取模型，而不是被历史 attempt 永久拦截。
  useEffect(() => {
    invalidateModelRefreshesOutside(enabledModelSourceIdsForRefresh);
  }, [enabledModelSourceIdsForRefresh]);

  // 进入路由页时只刷新当前方案已启用的模型源；停用候选不能借刷新副作用
  // 重新进入聚合 catalog，OAuth 与普通 /models 仍共用后续写回事务。
  useEffect(() => {
    if (activeTab !== "routes") return;
    if (enabledModelSourceIdsForRefresh.size === 0) return;

    for (const provider of modelSources.filter((source) =>
      enabledModelSourceIdsForRefresh.has(source.id),
    )) {
      const fetchConfig = getProviderModelFetchConfig(provider);
      const attemptKey = buildProviderModelRefreshAttemptKey(
        provider.id,
        fetchConfig,
      );
      if (modelRefreshAttemptedKeysRef.current.has(attemptKey)) continue;
      modelRefreshAttemptedKeysRef.current.add(attemptKey);
      modelRefreshActiveAttemptKeysRef.current[provider.id] = attemptKey;
      modelRefreshTimedOutAttemptKeysRef.current.delete(attemptKey);

      if (fetchConfig.skipReason) {
        setProviderModelRefreshStates((current) => ({
          ...current,
          [provider.id]: {
            status: "skipped",
            message: fetchConfig.skipReason!,
          },
        }));
        continue;
      }

      setProviderModelRefreshStates((current) => ({
        ...current,
        [provider.id]: {
          status: "loading",
          message: "正在读取模型列表...",
        },
      }));

      const isCurrentAttempt = () =>
        modelRefreshActiveAttemptKeysRef.current[provider.id] === attemptKey &&
        !modelRefreshTimedOutAttemptKeysRef.current.has(attemptKey);

      // 将模型目录读取、provider catalog 写回、受影响路由方案重建视为一个事务；
      // 任何阶段卡住都必须让刷新卡片落到终态，不能只保护最前面的网络请求。
      const refreshTask = (async (): Promise<ProviderModelRefreshResult> => {
        const { models, usedCodexCache, onlineErrorMessage } =
          await fetchProviderModelsWithFallback(fetchConfig);
        if (!isCurrentAttempt()) {
          return { status: "stale" };
        }
        if (models.length === 0) {
          return {
            status: "empty",
            message: onlineErrorMessage
              ? `OAuth 在线模型列表获取失败：${onlineErrorMessage}；本地缓存没有可恢复的官方模型目录。`
              : "获取模型列表失败：远端返回空列表，请检查当前 provider 配置。",
          };
        }

        const nextProvider = providerWithFetchedModelCatalog(provider, models);
        setProviderModelRefreshStates((current) => ({
          ...current,
          [provider.id]: {
            status: "loading",
            message: usedCodexCache
              ? `OAuth 在线读取失败，已从本地 Codex 模型缓存读取 ${models.length} 个模型，正在写回本地配置...`
              : `已读取 ${models.length} 个模型，正在写回本地配置...`,
            modelCount: models.length,
          },
        }));

        await providersApi.update(nextProvider, "codex");
        if (!isCurrentAttempt()) {
          return { status: "stale" };
        }

        const updatedProvidersById = new Map(providersById);
        updatedProvidersById.set(nextProvider.id, nextProvider);
        const updatedRoutableSources = resolveWizardModelNameCollisions(
          modelSources.map((source) =>
            source.id === nextProvider.id ? nextProvider : source,
          ),
        );
        const updatedRoutableProvidersById = new Map(updatedProvidersById);
        for (const source of updatedRoutableSources) {
          updatedRoutableProvidersById.set(source.id, source);
        }
        const affectedPlans: Provider[] = [];
        for (const plan of routingPlans) {
          // 每个 plan 都交给同步层判定是否变化：旧版内联 OAuth route 没有
          // targetProviderId，若在页面层提前过滤，就会出现 provider 已刷新而旧
          // MultiRouter 永远停在旧模型目录的断层。同步层返回 null 时不会写库。
          const syncResult = syncCodexMultiRouterPlanWithProviders(
            plan,
            updatedRoutableProvidersById,
          );
          if (!syncResult) continue;
          const nextPlan = syncResult.plan;
          await providersApi.update(nextPlan, "codex");
          if (!isCurrentAttempt()) {
            return { status: "stale" };
          }
          affectedPlans.push(nextPlan);
        }

        return {
          status: "updated",
          models,
          nextProvider,
          affectedPlans,
          usedCodexCache,
          onlineErrorMessage,
        };
      })();

      withModelRefreshTimeout(refreshTask, MODEL_REFRESH_TIMEOUT_MS, () =>
        modelRefreshTimedOutAttemptKeysRef.current.add(attemptKey),
      )
        .then(async (result) => {
          if (result.status === "stale") return;
          if (result.status === "empty") {
            setProviderModelRefreshStates((current) => ({
              ...current,
              [provider.id]: {
                status: "error",
                message:
                  result.message ??
                  "获取模型列表失败：远端返回空列表，请检查当前 provider 配置。",
                modelCount: 0,
              },
            }));
            return;
          }
          if (!isCurrentAttempt()) return;
          setOptimisticModelSourcesById((current) => ({
            ...current,
            [result.nextProvider.id]: result.nextProvider,
          }));
          queryClient.setQueryData(["providers", "codex"], (current: any) => ({
            ...(current ?? { currentProviderId: "" }),
            providers: {
              ...Object.fromEntries(providersById),
              ...(current?.providers ?? {}),
              [result.nextProvider.id]: result.nextProvider,
              ...Object.fromEntries(
                result.affectedPlans.map((plan) => [plan.id, plan]),
              ),
            },
          }));
          const selectedAffectedPlan = result.affectedPlans.find(
            (plan) => plan.id === selectedPlanId,
          );
          if (selectedAffectedPlan) {
            setOptimisticRoutingPlan(selectedAffectedPlan);
          }
          setProviderModelRefreshStates((current) => ({
            ...current,
            [provider.id]: {
              status: "success",
              message: result.usedCodexCache
                ? `OAuth 在线读取失败，已使用本地 Codex 模型缓存更新 ${
                    readCodexModelCatalog(result.nextProvider).models.length
                  } 个模型。在线错误：${result.onlineErrorMessage}`
                : `已读取并更新 ${
                    readCodexModelCatalog(result.nextProvider).models.length
                  } 个模型。`,
              modelCount: readCodexModelCatalog(result.nextProvider).models
                .length,
            },
          }));
          await queryClient.invalidateQueries({
            queryKey: ["providers", "codex"],
          });
        })
        .catch((error) => {
          if (
            modelRefreshActiveAttemptKeysRef.current[provider.id] !== attemptKey
          ) {
            return;
          }
          setProviderModelRefreshStates((current) => ({
            ...current,
            [provider.id]: {
              status: "error",
              message: `获取模型列表失败，请检查当前 provider 配置：${workspaceErrorMessage(error)}`,
            },
          }));
        });
    }
  }, [
    activeTab,
    enabledModelSourceIdsForRefresh,
    modelSources,
    providersById,
    queryClient,
    routingPlans,
    selectedPlanId,
  ]);

  const routeEntries = routingPlans.flatMap((provider) =>
    dedupeCodexRoutesBySemanticProvider(
      readCodexRouting(provider)?.routes ?? [],
      modelSources,
    ).map((route, index) => ({
      provider,
      route,
      index,
    })),
  );
  const enabledRoutes = routeEntries.filter(
    ({ route }) => route.enabled !== false,
  );
  const routeModels = collectRouteModels(routeEntries);
  const selectedPlan =
    routingPlans.find((provider) => provider.id === selectedPlanId) ??
    routingPlans[0] ??
    null;
  const selectedRouting = selectedPlan ? readCodexRouting(selectedPlan) : null;
  const selectedPlanRouteEntries = selectedPlan
    ? routeEntries.filter(({ provider }) => provider.id === selectedPlan.id)
    : routeEntries;
  const selectedRoute =
    selectedPlanRouteEntries.find(
      ({ provider, route, index }) =>
        `${provider.id}:${route.id ?? index}` === selectedRouteKey,
    ) ?? selectedPlanRouteEntries[0];

  // 从主页或 Provider 列表跳转进来时，直接定位到指定 MultiRouter 和目标功能页。
  useEffect(() => {
    if (!initialProviderId) return;
    const navigationKey = `${initialProviderId}:${initialTab}`;
    if (appliedInitialNavigationRef.current === navigationKey) return;
    if (!routingPlanIdSet.has(initialProviderId)) return;
    setSelectedPlanId(initialProviderId);
    setSelectedRouteKey(null);
    setActiveTab(initialTab);
    appliedInitialNavigationRef.current = navigationKey;
  }, [initialProviderId, initialTab, routingPlanIdSet]);

  // 工作台不同页签内容高度差异很大；切换页签时回到顶部，避免沿用上一页滚动位置导致目标页像没有跳转。
  useEffect(() => {
    const scrollTo = workspaceScrollRef.current?.scrollTo;
    if (typeof scrollTo !== "function") return;
    scrollTo.call(workspaceScrollRef.current, 0, 0);
  }, [activeTab]);

  useEffect(() => {
    const persistedPlan = optimisticRoutingPlan
      ? providers.find((provider) => provider.id === optimisticRoutingPlan.id)
      : null;
    if (
      persistedPlan &&
      JSON.stringify(persistedPlan.settingsConfig?.codexRouting) ===
        JSON.stringify(optimisticRoutingPlan?.settingsConfig?.codexRouting) &&
      JSON.stringify(persistedPlan.settingsConfig?.modelCatalog) ===
        JSON.stringify(optimisticRoutingPlan?.settingsConfig?.modelCatalog)
    ) {
      setOptimisticRoutingPlan(null);
    }
  }, [optimisticRoutingPlan, providers]);

  // 普通 provider 的 /models 刷新会先写 DB，再等待父级 query refetch；这里保留短期覆盖层，
  // 确保候选 router 和空 match route 立即消费新 catalog，同时在配置变化或父级追上后自动释放。
  useEffect(() => {
    setOptimisticModelSourcesById((current) => {
      let changed = false;
      const next = { ...current };
      for (const [providerId, optimisticProvider] of Object.entries(current)) {
        const persistedProvider = providers.find(
          (provider) => provider.id === providerId,
        );
        if (!persistedProvider) {
          delete next[providerId];
          changed = true;
          continue;
        }

        const persistedAttemptKey = buildProviderModelRefreshAttemptKey(
          providerId,
          getProviderModelFetchConfig(persistedProvider),
        );
        const optimisticAttemptKey = buildProviderModelRefreshAttemptKey(
          providerId,
          getProviderModelFetchConfig(optimisticProvider),
        );
        const catalogPersisted =
          JSON.stringify(persistedProvider.settingsConfig?.modelCatalog) ===
          JSON.stringify(optimisticProvider.settingsConfig?.modelCatalog);
        if (persistedAttemptKey !== optimisticAttemptKey || catalogPersisted) {
          delete next[providerId];
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [providers]);

  /// 新建 MultiRouter 直接创建带 codexRouting 的工作台 provider，不再打开普通供应商表单。
  async function handleCreatePlan() {
    const nextPlan = createDraftRoutingPlan(providers, modelSources);
    setIsSavingRoutes(true);
    setRoutePickerError(null);
    setRoutePickerMessage(null);
    try {
      await providersApi.add(nextPlan, "codex", false);
      queryClient.setQueryData(["providers", "codex"], (current: any) =>
        current?.providers
          ? {
              ...current,
              providers: { ...current.providers, [nextPlan.id]: nextPlan },
            }
          : current,
      );
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      await queryClient.refetchQueries({
        queryKey: ["providers", "codex"],
        type: "active",
      });
      setOptimisticRoutingPlan(nextPlan);
      setSelectedPlanId(nextPlan.id);
      setSelectedRouteKey(null);
      setActiveTab("routes");
      setRoutePickerSelectAll(true);
      setIsRoutePickerOpen(true);
      setRoutePickerMessage("已创建新的多路路由，请选择要接入的候选 router。");
    } catch (error) {
      setRoutePickerError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setIsSavingRoutes(false);
    }
  }

  /// MultiRouter 方案只打开工作台专用设置；普通模型源仍进入通用 Provider 表单。
  function handleEditPlan(provider: Provider) {
    if (isRoutingPlan(provider)) {
      setSelectedPlanId(provider.id);
      setActiveTab("routes");
      setRoutePickerError(null);
      setRoutePickerMessage(null);
      setIsPlanSettingsOpen(true);
      return;
    }
    onEditProvider(provider);
  }

  /// 保存 MultiRouter 方案元信息时不触碰 routes/modelCatalog，避免普通 Provider 表单误清空路由私有字段。
  async function handleSavePlanSettings(
    plan: Provider,
    draft: MultiRouterSettingsDraft,
  ) {
    const nextProvider = applyMultiRouterSettingsDraft(plan, draft);
    setIsSavingPlanSettings(true);
    setRoutePickerError(null);
    setRoutePickerMessage(null);
    try {
      await providersApi.update(nextProvider, "codex");
      queryClient.setQueryData(["providers", "codex"], (current: any) =>
        current?.providers
          ? {
              ...current,
              providers: {
                ...current.providers,
                [nextProvider.id]: nextProvider,
              },
            }
          : current,
      );
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      await queryClient.refetchQueries({
        queryKey: ["providers", "codex"],
        type: "active",
      });
      setOptimisticRoutingPlan(nextProvider);
      setSelectedPlanId(nextProvider.id);
      setIsPlanSettingsOpen(false);
      setRoutePickerMessage("多路路由设置已保存，接管配置由系统继续自动维护。");
    } catch (error) {
      setRoutePickerError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setIsSavingPlanSettings(false);
    }
  }

  /// 路由规则编辑只更新 codexRouting.routes，不再进入通用 Provider 表单，避免“添加 router”卡死路径。
  async function handleSaveRoutingRoutes(plan: Provider, routes: CodexRoute[]) {
    const currentRouting = readCodexRouting(plan) ?? {};
    const usedRouteIds = new Set<string>();
    const normalizedRouteDrafts = dedupeCodexRoutesBySemanticProvider(
      routes.map((route, index) =>
        normalizeCodexRouteForSave(route, index, usedRouteIds),
      ),
      routableModelSources,
    );
    const normalizedRoutes = normalizeCodexRoutesForVisibleModelAliases(
      plan,
      normalizedRouteDrafts,
      routableProvidersById,
    );
    const enabledRouteIds = normalizedRoutes
      .filter((route) => route.enabled !== false)
      .map((route) => route.id)
      .filter((id): id is string => Boolean(id));
    const defaultRouteId = normalizedRoutes.some(
      (route) => route.id && route.id === currentRouting.defaultRouteId,
    )
      ? currentRouting.defaultRouteId
      : (enabledRouteIds[0] ?? normalizedRoutes[0]?.id);
    const nextRouting: CodexRouting = {
      ...currentRouting,
      enabled: currentRouting.enabled ?? true,
      routes: normalizedRoutes,
    };
    if (defaultRouteId) {
      nextRouting.defaultRouteId = defaultRouteId;
    } else {
      delete nextRouting.defaultRouteId;
    }
    const nextProvider: Provider = {
      ...plan,
      settingsConfig: {
        ...plan.settingsConfig,
        modelCatalog: buildModelCatalogForRoutes(
          plan,
          normalizedRoutes,
          routableProvidersById,
        ),
        codexRouting: nextRouting,
      },
    };
    const nextEnabledProviderIds = new Set<string>();
    for (const route of normalizedRoutes) {
      if (route.enabled === false) continue;
      const providerId = routeSemanticProviderId(route, modelSources);
      if (providerId) nextEnabledProviderIds.add(providerId);
    }

    // 必须在持久化前同步失效旧 attempt；否则保存等待期间迟到的 /models
    // 结果仍会按旧 routingPlans 写回 provider 和 plan catalog。
    invalidateModelRefreshesOutside(nextEnabledProviderIds);
    setOptimisticRoutingPlan(nextProvider);
    setSelectedPlanId(plan.id);
    setIsSavingRoutes(true);
    setRoutePickerError(null);
    setRoutePickerMessage(null);
    try {
      await providersApi.update(nextProvider, "codex");
      queryClient.setQueryData(["providers", "codex"], (current: any) =>
        current?.providers
          ? {
              ...current,
              providers: {
                ...current.providers,
                [nextProvider.id]: nextProvider,
              },
            }
          : current,
      );
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      await queryClient.refetchQueries({
        queryKey: ["providers", "codex"],
        type: "active",
      });
      setSelectedRouteKey(
        normalizedRoutes[0]?.id ? `${plan.id}:${normalizedRoutes[0].id}` : null,
      );
      setRoutePickerMessage(
        "路由规则已保存，候选 router 选择已写入当前多路路由方案。",
      );
      setRoutePickerSelectAll(false);
      setIsRoutePickerOpen(false);
    } catch (error) {
      // 保存失败时回滚原方案；启用集合恢复后刷新 effect 会重新读取被失效的 Provider。
      setOptimisticRoutingPlan(plan);
      setRoutePickerError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setIsSavingRoutes(false);
    }
  }

  /// 选择方案只改变工作台焦点，不修改数据库。
  function handleSelectPlan(provider: Provider) {
    setSelectedPlanId(provider.id);
    setActiveTab("routes");
  }

  /// 选择规则后跳转到规则页，让卡片产生明确的可操作反馈。
  function handleSelectRoute(entry: RouteEntry) {
    setSelectedPlanId(entry.provider.id);
    setSelectedRouteKey(
      `${entry.provider.id}:${entry.route.id ?? entry.index}`,
    );
    setActiveTab("routes");
  }

  /// 从任何规则入口打开候选选择器时，先切到规则页并清理上一次保存提示。
  function handleOpenRoutePicker(provider?: Provider | null) {
    if (provider) setSelectedPlanId(provider.id);
    setActiveTab("routes");
    setRoutePickerError(null);
    setRoutePickerMessage(null);
    setRoutePickerSelectAll(false);
    setIsRoutePickerOpen(true);
  }

  /// 页面内测试只做规则匹配预览，不发真实上游请求，避免误触发计费或账号请求。
  function handlePreviewRoute() {
    const model = testModel.trim();
    if (!model) {
      setTestResult(
        "请输入一个 Codex 请求里的 model，例如 gpt-5.4-mini 或 qwen3.6。",
      );
      return;
    }

    const matched = enabledRoutes.find(({ route }) => {
      const models = route.match?.models ?? [];
      const prefixes = route.match?.prefixes ?? [];
      return (
        models.includes(model) ||
        prefixes.some((prefix) => model.startsWith(prefix))
      );
    });

    if (matched) {
      const result = `${model} 会命中「${matched.route.label || matched.route.id || "未命名规则"}」，上游为 ${routeBaseUrl(matched.route, providersById)}。`;
      setTestResult(result);
      return;
    }

    const fallback = selectedRouting?.defaultRouteId
      ? `没有精确命中，会走默认路由 ${selectedRouting.defaultRouteId}。`
      : "没有命中任何启用规则，且当前方案没有默认路由。";
    setTestResult(fallback);
  }

  return (
    <div className="flex h-full flex-col overflow-hidden px-6 py-4">
      <div
        ref={workspaceScrollRef}
        className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-2"
      >
        <HeaderPanel
          onCreatePlan={handleCreatePlan}
          onJump={(tab) => setActiveTab(tab)}
        />

        <Tabs
          value={activeTab}
          onValueChange={(value) => setActiveTab(value as WorkspaceTab)}
        >
          <div className="sticky top-0 z-10 -mx-1 bg-background/95 px-1 py-2 backdrop-blur">
            <TabsList className="grid w-full grid-cols-5 bg-muted p-1 dark:bg-slate-950/40">
              <WorkspaceTabTrigger
                value="overview"
                icon={Layers3}
                label="总览"
              />
              <WorkspaceTabTrigger
                value="sources"
                icon={Server}
                label="模型源"
              />
              <WorkspaceTabTrigger
                value="routes"
                icon={Route}
                label="路由规则"
              />
              <WorkspaceTabTrigger
                value="status"
                icon={Activity}
                label="状态"
              />
              <WorkspaceTabTrigger value="test" icon={Play} label="测试发布" />
            </TabsList>
          </div>

          <TabsContent value="overview" className="mt-3">
            <OverviewTab
              routingPlans={routingPlans}
              routeEntries={routeEntries}
              modelSources={modelSources}
              onCreatePlan={handleCreatePlan}
              onSelectPlan={handleSelectPlan}
              onEditPlan={handleEditPlan}
              onDeletePlan={onDeletePlan}
              onSelectRoute={handleSelectRoute}
              providersById={providersById}
              onJump={setActiveTab}
            />
          </TabsContent>

          <TabsContent value="sources" className="mt-3">
            <SourcesTab
              modelSources={modelSources}
              routingPlans={routingPlans}
              onCreatePlan={handleCreatePlan}
              onCreateProvider={onCreateProvider}
              onEditPlan={handleEditPlan}
              onSelectPlan={handleSelectPlan}
            />
          </TabsContent>

          <TabsContent value="routes" className="mt-3">
            <RoutesTab
              routingPlans={routingPlans}
              routeEntries={routeEntries}
              selectedPlan={selectedPlan}
              selectedRoute={selectedRoute}
              modelSources={modelSources}
              onCreatePlan={handleCreatePlan}
              onCreateProvider={onCreateProvider}
              onOpenRoutePicker={handleOpenRoutePicker}
              onSaveRoutes={handleSaveRoutingRoutes}
              onSelectPlan={handleSelectPlan}
              onSelectRoute={handleSelectRoute}
              onEditPlan={handleEditPlan}
              onDeletePlan={onDeletePlan}
              providersById={providersById}
              proxyStatus={proxyStatus}
              isProxyRunning={isProxyRunning}
              isCodexTakeoverActive={isCodexTakeoverActive}
              activeProviderId={activeProviderId}
              providerModelRefreshStates={providerModelRefreshStates}
              isRoutePickerOpen={isRoutePickerOpen}
              isSavingRoutes={isSavingRoutes}
              isPlanSettingsOpen={isPlanSettingsOpen}
              isSavingPlanSettings={isSavingPlanSettings}
              onPlanSettingsOpenChange={setIsPlanSettingsOpen}
              onSavePlanSettings={handleSavePlanSettings}
              routePickerSelectAll={routePickerSelectAll}
              routePickerMessage={routePickerMessage}
              routePickerError={routePickerError}
              onRoutePickerOpenChange={setIsRoutePickerOpen}
            />
          </TabsContent>

          <TabsContent value="status" className="mt-3">
            <StatusTab
              selectedPlan={selectedPlan}
              selectedRouting={selectedRouting}
              routeEntries={routeEntries}
              providersById={providersById}
              proxyStatus={proxyStatus}
              isProxyRunning={isProxyRunning}
              isCodexTakeoverActive={isCodexTakeoverActive}
              activeProviderId={activeProviderId}
              onEditPlan={handleEditPlan}
              onDeletePlan={onDeletePlan}
              onRuntimeReady={onRuntimeReady}
            />
          </TabsContent>

          <TabsContent value="test" className="mt-3">
            <TestTab
              selectedPlan={selectedPlan}
              selectedRouting={selectedRouting}
              routeModels={routeModels}
              testModel={testModel}
              testResult={testResult}
              onModelChange={setTestModel}
              onPreviewRoute={handlePreviewRoute}
              onEditPlan={handleEditPlan}
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}

/// 顶部工作台只保留定位说明和主要动作；运行态证据统一放到状态页，避免两处重复。
function HeaderPanel({
  onCreatePlan,
  onJump,
}: {
  onCreatePlan: () => void;
  onJump: (tab: WorkspaceTab) => void;
}) {
  return (
    <div className="overflow-hidden rounded-lg border border-border bg-card dark:border-slate-700/80 dark:bg-slate-950/30">
      <div className="flex flex-wrap items-center justify-between gap-3 bg-gradient-to-r from-blue-50 via-background to-emerald-50 px-4 py-3 dark:from-blue-950/45 dark:via-slate-900 dark:to-emerald-950/30">
        <div className="min-w-0 space-y-2">
          <div className="flex items-center gap-2 text-base font-semibold">
            <GitBranch className="h-4 w-4 text-blue-600 dark:text-blue-300" />
            Codex 多模型路由工作台
          </div>
          <p className="max-w-4xl text-xs leading-5 text-muted-foreground dark:text-slate-400">
            这里配置的是“Codex 自己怎么按 model 选择多个上游模型”。Codex
            仍然只连接一个 CC Switch 本地代理；路由规则负责把
            gpt、qwen、deepseek 等模型名分流到不同上游。
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              onClick={onCreatePlan}
              size="sm"
              className="gap-2 bg-blue-600 hover:bg-blue-500"
            >
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => onJump("routes")}
              className="gap-2"
            >
              <Settings2 className="h-4 w-4" />
              管理路由规则
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => onJump("status")}
              className="gap-2"
            >
              <Activity className="h-4 w-4" />
              查看链路状态
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

/// 选项卡触发器封装，统一图标和可点击态。
function WorkspaceTabTrigger({
  value,
  icon: Icon,
  label,
}: {
  value: WorkspaceTab;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
}) {
  return (
    <TabsTrigger value={value} className="min-w-0 gap-2">
      <Icon className="h-4 w-4 flex-shrink-0" />
      <span className="hidden truncate sm:inline">{label}</span>
    </TabsTrigger>
  );
}

/// 总览页展示当前方案、关键规则和下一步动作，避免用户只看到一堆不可操作卡片。
function OverviewTab({
  routingPlans,
  routeEntries,
  modelSources,
  providersById,
  onCreatePlan,
  onSelectPlan,
  onEditPlan,
  onDeletePlan,
  onSelectRoute,
  onJump,
}: {
  routingPlans: Provider[];
  routeEntries: RouteEntry[];
  modelSources: Provider[];
  providersById: Map<string, Provider>;
  onCreatePlan: () => void;
  onSelectPlan: (provider: Provider) => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
  onDeletePlan: (provider: Provider) => void;
  onSelectRoute: (entry: RouteEntry) => void;
  onJump: (tab: WorkspaceTab) => void;
}) {
  return (
    <div className="grid gap-4 xl:grid-cols-[1.05fr_0.95fr]">
      <section className="rounded-lg border border-blue-200 bg-blue-50/70 p-4 dark:border-blue-700/40 dark:bg-blue-950/15">
        <SectionHeader
          icon={Layers3}
          title="多路路由"
          detail="每个多路路由都是一个 Codex 可连接的本地代理入口。"
          action={
            <Button
              size="sm"
              onClick={onCreatePlan}
              className="gap-2 bg-blue-600 hover:bg-blue-500"
            >
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
          }
        />
        <div className="mt-3 grid gap-3">
          {routingPlans.length === 0 ? (
            <EmptyState
              icon={Wand2}
              title="还没有多路路由"
              detail="先创建一个多路路由，再把多个模型源挂到它下面。"
              actionLabel="创建多路路由"
              onAction={onCreatePlan}
            />
          ) : (
            routingPlans.map((provider) => (
              <div
                key={provider.id}
                className="group rounded-lg border border-blue-200 bg-card p-4 text-left transition hover:border-blue-400 hover:bg-blue-50 hover:shadow-[0_0_0_1px_rgba(96,165,250,0.25)] dark:border-blue-600/40 dark:bg-slate-950/40 dark:hover:bg-blue-950/30 dark:hover:shadow-[0_0_0_1px_rgba(96,165,250,0.35)]"
              >
                <PlanCardContent provider={provider} />
                <div className="mt-3 flex flex-wrap gap-2">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => onSelectPlan(provider)}
                    className="gap-2"
                  >
                    <Route className="h-4 w-4" />
                    路由规则
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => onEditPlan(provider, "重命名多路路由")}
                    className="gap-2"
                  >
                    <Pencil className="h-4 w-4" />
                    重命名/设置
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => onDeletePlan(provider)}
                    className="gap-2 border-rose-300 bg-background/70 text-rose-700 hover:bg-rose-50 dark:border-rose-500/50 dark:bg-rose-500/10 dark:text-rose-100 dark:hover:bg-rose-500/20"
                  >
                    <Trash2 className="h-4 w-4" />
                    删除
                  </Button>
                </div>
              </div>
            ))
          )}
        </div>
      </section>

      <section className="rounded-lg border border-emerald-200 bg-emerald-50/70 p-4 dark:border-emerald-700/40 dark:bg-emerald-950/10">
        <SectionHeader
          icon={Route}
          title="最近路由规则"
          detail="点击规则可以进入详情和测试。"
          action={
            <Button
              size="sm"
              variant="outline"
              onClick={() => onJump("routes")}
              className="gap-2"
            >
              查看全部
              <ArrowRight className="h-4 w-4" />
            </Button>
          }
        />
        <div className="mt-3 grid gap-2">
          {routeEntries.slice(0, 4).map((entry) => (
            <RouteListButton
              key={`${entry.provider.id}-${entry.route.id ?? entry.index}`}
              entry={entry}
              providersById={providersById}
              onClick={() => onSelectRoute(entry)}
            />
          ))}
          {routeEntries.length === 0 && (
            <EmptyState
              icon={Route}
              title="还没有规则"
              detail="创建多路路由后，在编辑表单里添加模型匹配规则。"
              actionLabel="创建多路路由"
              onAction={onCreatePlan}
            />
          )}
        </div>
      </section>

      <section className="rounded-lg border border-amber-200 bg-amber-50/70 p-4 dark:border-amber-700/40 dark:bg-amber-950/10 xl:col-span-2">
        <SectionHeader
          icon={Server}
          title="可接入模型源"
          detail="这些不是单独一类难懂的 Provider，而是可以被路由方案接入的上游模型源。"
          action={
            <Button
              size="sm"
              variant="outline"
              onClick={() => onJump("sources")}
            >
              选择模型源
            </Button>
          }
        />
        <div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          {modelSources.slice(0, 8).map((provider) => (
            <SourceMiniCard key={provider.id} provider={provider} />
          ))}
        </div>
      </section>
    </div>
  );
}

/// 模型源页展示可被纳入路由的上游，并把“编辑后接入”作为明确操作。
function SourcesTab({
  modelSources,
  routingPlans,
  onCreatePlan,
  onCreateProvider,
  onEditPlan,
  onSelectPlan,
}: {
  modelSources: Provider[];
  routingPlans: Provider[];
  onCreatePlan: () => void;
  onCreateProvider: () => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
  onSelectPlan: (provider: Provider) => void;
}) {
  return (
    <div className="grid gap-4 xl:grid-cols-[0.8fr_1.2fr]">
      <section className="rounded-lg border border-blue-200 bg-blue-50/70 p-4 dark:border-blue-700/40 dark:bg-blue-950/15">
        <SectionHeader
          icon={Layers3}
          title="多路路由方案"
          detail="这是 Codex 最终连接的路由入口；选择后到“路由规则”里挂接模型源。"
          action={
            <Button
              size="sm"
              onClick={onCreatePlan}
              className="gap-2 bg-blue-600 hover:bg-blue-500"
            >
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
          }
        />
        <div className="mt-3 grid gap-2">
          {routingPlans.map((provider) => (
            <div
              key={provider.id}
              className="rounded-lg border border-blue-200 bg-card p-3 text-left transition hover:border-blue-400 hover:bg-blue-50 dark:border-blue-700/40 dark:bg-slate-950/40 dark:hover:bg-blue-950/30"
            >
              <PlanCardContent provider={provider} compact />
              <div className="mt-3 flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => onSelectPlan(provider)}
                  className="gap-2"
                >
                  <Route className="h-4 w-4" />
                  路由规则
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => onEditPlan(provider, "重命名多路路由")}
                  className="gap-2"
                >
                  <Pencil className="h-4 w-4" />
                  重命名/设置
                </Button>
              </div>
            </div>
          ))}
          {routingPlans.length === 0 && (
            <EmptyState
              icon={Layers3}
              title="还没有多路路由"
              detail="先创建一个 Codex 多模型路由入口，再选择模型源接入。"
              actionLabel="创建多路路由"
              onAction={onCreatePlan}
            />
          )}
        </div>
      </section>

      <section className="rounded-lg border border-amber-200 bg-amber-50/70 p-4 dark:border-amber-700/40 dark:bg-amber-950/10">
        <SectionHeader
          icon={Server}
          title="选择模型源"
          detail="这里选择要接入多路路由的上游模型源；点卡片进入模型源配置。"
          action={
            <Button
              size="sm"
              variant="outline"
              onClick={onCreateProvider}
              className="gap-2"
            >
              <Plus className="h-4 w-4" />
              添加模型源
            </Button>
          }
        />
        <div className="mt-3 grid gap-3 md:grid-cols-2">
          {modelSources.map((provider) => (
            <button
              key={provider.id}
              type="button"
              onClick={() =>
                onEditPlan(provider, "选择并编辑模型源，准备接入多路路由")
              }
              className="group rounded-lg border border-amber-200 bg-card p-4 text-left transition hover:border-amber-400 hover:bg-amber-50 hover:shadow-[0_0_0_1px_rgba(251,191,36,0.18)] dark:border-amber-700/40 dark:bg-slate-950/40 dark:hover:bg-amber-950/20 dark:hover:shadow-[0_0_0_1px_rgba(251,191,36,0.25)]"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold text-foreground dark:text-slate-100">
                    {provider.name}
                  </div>
                  <div className="mt-1 truncate text-xs text-muted-foreground dark:text-slate-400">
                    ID：{provider.id}
                  </div>
                </div>
                <Badge className="border-amber-200 bg-amber-100 text-amber-800 dark:border-amber-500/50 dark:bg-amber-500/15 dark:text-amber-100">
                  可选
                </Badge>
              </div>
              <div className="mt-4 flex items-center justify-between text-xs">
                <span className="text-muted-foreground dark:text-slate-400">
                  选择这个模型源
                </span>
                <span className="inline-flex items-center gap-1 text-amber-700 opacity-80 group-hover:opacity-100 dark:text-amber-200">
                  选择
                  <Pencil className="h-3.5 w-3.5" />
                </span>
              </div>
            </button>
          ))}
          {modelSources.length === 0 && (
            <EmptyState
              icon={Server}
              title="还没有模型源"
              detail="先添加一个普通 Codex 模型源，再把它接入多路路由。"
              actionLabel="添加模型源"
              onAction={onCreateProvider}
            />
          )}
        </div>
      </section>
    </div>
  );
}

/// 路由规则页提供方案选择、规则列表和右侧详情，形成真实的“查/改/删入口”工作流。
function RoutesTab({
  routingPlans,
  routeEntries,
  selectedPlan,
  selectedRoute,
  modelSources,
  providersById,
  onCreatePlan,
  onCreateProvider,
  onOpenRoutePicker,
  onSaveRoutes,
  onSelectPlan,
  onSelectRoute,
  onEditPlan,
  onDeletePlan,
  proxyStatus,
  isProxyRunning,
  isCodexTakeoverActive,
  activeProviderId,
  providerModelRefreshStates,
  isRoutePickerOpen,
  isSavingRoutes,
  isPlanSettingsOpen,
  isSavingPlanSettings,
  onPlanSettingsOpenChange,
  onSavePlanSettings,
  routePickerSelectAll,
  routePickerMessage,
  routePickerError,
  onRoutePickerOpenChange,
}: {
  routingPlans: Provider[];
  routeEntries: RouteEntry[];
  selectedPlan: Provider | null;
  selectedRoute?: RouteEntry;
  modelSources: Provider[];
  providersById: Map<string, Provider>;
  onCreatePlan: () => void;
  onCreateProvider: () => void;
  onOpenRoutePicker: (provider?: Provider | null) => void;
  onSaveRoutes: (plan: Provider, routes: CodexRoute[]) => Promise<void>;
  onSelectPlan: (provider: Provider) => void;
  onSelectRoute: (entry: RouteEntry) => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
  onDeletePlan: (provider: Provider) => void;
  proxyStatus?: ProxyStatus;
  isProxyRunning: boolean;
  isCodexTakeoverActive: boolean;
  activeProviderId?: string;
  providerModelRefreshStates: Record<string, ProviderModelRefreshState>;
  isRoutePickerOpen: boolean;
  isSavingRoutes: boolean;
  isPlanSettingsOpen: boolean;
  isSavingPlanSettings: boolean;
  onPlanSettingsOpenChange: (open: boolean) => void;
  onSavePlanSettings: (
    plan: Provider,
    draft: MultiRouterSettingsDraft,
  ) => Promise<void>;
  routePickerSelectAll: boolean;
  routePickerMessage: string | null;
  routePickerError: string | null;
  onRoutePickerOpenChange: (open: boolean) => void;
}) {
  const actionPanelRef = useRef<HTMLDivElement | null>(null);
  const selectedPlanRoutes = selectedPlan
    ? routeEntries.filter(({ provider }) => provider.id === selectedPlan.id)
    : routeEntries;
  const enabledPlanRouteCount = selectedPlanRoutes.filter(
    ({ route }) => route.enabled !== false,
  ).length;
  const selectedRouting = selectedPlan ? readCodexRouting(selectedPlan) : null;
  const effectiveActiveProviderId =
    activeProviderId ?? proxyStatus?.current_provider_id ?? undefined;
  const { data: globalProxyConfig } = useQuery({
    queryKey: ["globalProxyConfig"],
    queryFn: () => proxyApi.getGlobalProxyConfig(),
  });
  const configuredListenAddress = globalProxyConfig
    ? `${globalProxyConfig.listenAddress}:${globalProxyConfig.listenPort}`
    : `${DEFAULT_CODEX_PROXY_LISTEN_ADDRESS}:${DEFAULT_CODEX_PROXY_LISTEN_PORT}`;
  const runtimeStatus = buildMultiRouterRuntimeStatus({
    selectedPlan,
    selectedRouting,
    enabledRouteCount: enabledPlanRouteCount,
    isProxyRunning,
    isCodexTakeoverActive,
    activeProviderId: effectiveActiveProviderId,
  });

  // 候选 router 选择器和设置面板都是行内展开；打开后主动定位到面板，避免用户停在上方规则区误以为没有响应。
  useEffect(() => {
    if (!isRoutePickerOpen && !isPlanSettingsOpen) return;
    window.setTimeout(() => {
      const scrollIntoView = actionPanelRef.current?.scrollIntoView;
      if (typeof scrollIntoView !== "function") return;
      scrollIntoView.call(actionPanelRef.current, {
        behavior: "smooth",
        block: "nearest",
      });
    }, 0);
  }, [isPlanSettingsOpen, isRoutePickerOpen, selectedPlan?.id]);

  return (
    <div className="space-y-3">
      <MultiRouterCurrentStatus
        selectedPlan={selectedPlan}
        totalRouteCount={selectedPlanRoutes.length}
        enabledRouteCount={enabledPlanRouteCount}
        runtimeStatus={runtimeStatus}
        proxyStatus={proxyStatus}
        configuredListenAddress={configuredListenAddress}
        isProxyRunning={isProxyRunning}
        isCodexTakeoverActive={isCodexTakeoverActive}
        activeProviderId={effectiveActiveProviderId}
      />
      <div className="grid gap-3 xl:grid-cols-[300px_minmax(0,1fr)]">
        <section className="rounded-lg border border-blue-200 bg-blue-50/70 p-3 dark:border-blue-700/40 dark:bg-blue-950/15">
          <SectionHeader
            icon={Layers3}
            title="选择多路路由"
            detail="每个多路路由可包含多条分流规则。"
            action={
              <Button
                size="sm"
                onClick={onCreatePlan}
                className="gap-2 bg-blue-600 hover:bg-blue-500"
              >
                <Plus className="h-4 w-4" />
                创建多路路由
              </Button>
            }
          />
          <div className="mt-2 grid gap-2">
            {routingPlans.map((provider) => {
              const active = selectedPlan?.id === provider.id;
              return (
                <div
                  key={provider.id}
                  className={cn(
                    "rounded-lg border p-2.5 text-left transition",
                    active
                      ? "border-blue-400 bg-blue-50 text-blue-900 shadow-[0_0_0_1px_rgba(96,165,250,0.25)] dark:bg-blue-600/20 dark:text-blue-100 dark:shadow-[0_0_0_1px_rgba(96,165,250,0.35)]"
                      : "border-border bg-card text-foreground hover:border-blue-400 hover:bg-blue-50 dark:border-slate-700 dark:bg-slate-950/40 dark:hover:border-blue-500 dark:hover:bg-blue-950/20",
                  )}
                >
                  <PlanCardContent provider={provider} compact />
                  <div className="mt-2 flex flex-wrap gap-2">
                    <Button
                      type="button"
                      size="sm"
                      variant={active ? "default" : "outline"}
                      onClick={() => onSelectPlan(provider)}
                      className={cn(
                        "h-8 gap-1.5 px-2.5",
                        active ? "bg-blue-600 hover:bg-blue-500" : "",
                      )}
                    >
                      <CheckCircle2 className="h-4 w-4" />
                      {active ? "当前选中" : "选择"}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => onEditPlan(provider, "重命名多路路由")}
                      className="h-8 gap-1.5 px-2.5"
                    >
                      <Pencil className="h-4 w-4" />
                      改名
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => onDeletePlan(provider)}
                      className="h-8 gap-1.5 border-rose-300 bg-background/70 px-2.5 text-rose-700 hover:bg-rose-50 dark:border-rose-500/50 dark:bg-rose-500/10 dark:text-rose-100 dark:hover:bg-rose-500/20"
                    >
                      <Trash2 className="h-4 w-4" />
                      删除
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        </section>

        <section className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_300px]">
          <div className="rounded-lg border border-emerald-200 bg-emerald-50/70 p-3 dark:border-emerald-700/40 dark:bg-emerald-950/10">
            <SectionHeader
              icon={Route}
              title="规则列表"
              detail="点击规则查看详情；每条规则的“启用”只表示参与匹配，不是启动服务。"
              action={
                selectedPlan ? (
                  <Button
                    size="sm"
                    onClick={() => onOpenRoutePicker(selectedPlan)}
                    className="gap-2 bg-emerald-600 hover:bg-emerald-500"
                  >
                    <Pencil className="h-4 w-4" />
                    编辑匹配规则
                  </Button>
                ) : null
              }
            />
            <div className="mt-2 grid gap-2">
              {selectedPlanRoutes.map((entry) => (
                <RouteListButton
                  key={`${entry.provider.id}-${entry.route.id ?? entry.index}`}
                  entry={entry}
                  providersById={providersById}
                  active={
                    selectedRoute?.provider.id === entry.provider.id &&
                    selectedRoute.index === entry.index
                  }
                  onClick={() => onSelectRoute(entry)}
                />
              ))}
              {selectedPlanRoutes.length === 0 && (
                <EmptyState
                  icon={Route}
                  title="这个方案还没有规则"
                  detail="点击编辑规则，直接勾选要接入的候选 router。"
                  actionLabel="编辑多路路由"
                  onAction={() =>
                    selectedPlan
                      ? onOpenRoutePicker(selectedPlan)
                      : onCreatePlan()
                  }
                />
              )}
            </div>
          </div>

          <RouteDetailPanel
            selectedRoute={selectedRoute}
            selectedPlan={selectedPlan}
            providersById={providersById}
            onOpenRoutePicker={onOpenRoutePicker}
          />
        </section>
      </div>

      <div ref={actionPanelRef}>
        {selectedPlan && isRoutePickerOpen ? (
          <RouteCandidatePicker
            selectedPlan={selectedPlan}
            modelSources={modelSources}
            providerModelRefreshStates={providerModelRefreshStates}
            onSaveRoutes={onSaveRoutes}
            onCreateProvider={onCreateProvider}
            onClose={() => onRoutePickerOpenChange(false)}
            isSaving={isSavingRoutes}
            selectAllByDefault={routePickerSelectAll}
          />
        ) : null}

        {selectedPlan && isPlanSettingsOpen ? (
          <MultiRouterSettingsPanel
            selectedPlan={selectedPlan}
            selectedRoutes={selectedPlanRoutes}
            onSave={onSavePlanSettings}
            onClose={() => onPlanSettingsOpenChange(false)}
            isSaving={isSavingPlanSettings}
          />
        ) : null}
      </div>

      {(routePickerMessage || routePickerError) && (
        <div
          className={cn(
            "rounded-lg border p-3 text-sm",
            routePickerError
              ? "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-100"
              : "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-100",
          )}
        >
          {routePickerError ?? routePickerMessage}
        </div>
      )}

      <ProviderModelRefreshPanel
        modelSources={modelSources}
        states={providerModelRefreshStates}
      />

      <SpawnAgentCandidatesPanel
        selectedPlan={selectedPlan}
        selectedRoutes={selectedPlanRoutes}
      />
    </div>
  );
}

/// 路由页顶部的当前 MultiRouter 状态带，明确区分“页面选中”和“已经作为 Codex provider 运行”。
function MultiRouterCurrentStatus({
  selectedPlan,
  totalRouteCount,
  enabledRouteCount,
  runtimeStatus,
  proxyStatus,
  configuredListenAddress,
  isProxyRunning,
  isCodexTakeoverActive,
  activeProviderId,
}: {
  selectedPlan: Provider | null;
  totalRouteCount: number;
  enabledRouteCount: number;
  runtimeStatus: MultiRouterRuntimeStatus;
  proxyStatus?: ProxyStatus;
  configuredListenAddress: string;
  isProxyRunning: boolean;
  isCodexTakeoverActive: boolean;
  activeProviderId?: string;
}) {
  const listenAddress = proxyStatus
    ? `${proxyStatus.address}:${proxyStatus.port}`
    : "未监听";
  const runtimeClass =
    runtimeStatus.tone === "ok"
      ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/50 dark:bg-emerald-500/15 dark:text-emerald-100"
      : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/50 dark:bg-amber-500/15 dark:text-amber-100";
  return (
    <section className="rounded-lg border border-blue-200 bg-blue-50/70 p-3 dark:border-blue-700/40 dark:bg-slate-950/55">
      <div className="grid gap-2 xl:grid-cols-[minmax(220px,0.75fr)_minmax(0,1.6fr)]">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <RadioTower className="h-4 w-4 text-blue-600 dark:text-blue-300" />
            <span className="text-sm font-semibold text-foreground dark:text-slate-100">
              当前 MultiRouter
            </span>
            <Badge className={cn("border", runtimeClass)}>
              {runtimeStatus.label}
            </Badge>
          </div>
          <div className="mt-1 truncate text-base font-semibold text-foreground dark:text-slate-50">
            {selectedPlan?.name ?? "未选择多路路由"}
          </div>
          <div
            className="mt-0.5 truncate text-xs text-muted-foreground dark:text-slate-400"
            title={runtimeStatus.detail}
          >
            {runtimeStatus.detail}
          </div>
        </div>
        <div className="grid min-w-0 gap-1.5 text-xs sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-7">
          <StatusInlineItem
            label="选中方案"
            value={selectedPlan?.id ?? "无"}
            ok={Boolean(selectedPlan)}
          />
          <StatusInlineItem
            label="当前 Provider"
            value={activeProviderId ?? "未设置"}
            ok={Boolean(selectedPlan && activeProviderId === selectedPlan.id)}
          />
          <StatusInlineItem
            label="监听配置"
            value={configuredListenAddress}
            ok={Boolean(configuredListenAddress)}
          />
          <StatusInlineItem
            label="运行监听"
            value={isProxyRunning ? listenAddress : "未运行"}
            ok={isProxyRunning}
          />
          <StatusInlineItem
            label="Codex 接管"
            value={isCodexTakeoverActive ? "已接管" : "未接管"}
            ok={isCodexTakeoverActive}
          />
          <StatusInlineItem
            label="启用规则"
            value={`${enabledRouteCount} / ${totalRouteCount} 条`}
            ok={enabledRouteCount > 0}
          />
          <StatusInlineItem
            label="入口状态"
            value={selectedPlan ? runtimeStatus.label : "未选择"}
            ok={runtimeStatus.running}
          />
        </div>
      </div>
    </section>
  );
}

/// 状态带内的短字段，避免把关键运行信号藏进长说明文本。
function StatusInlineItem({
  label,
  value,
  ok,
}: {
  label: string;
  value: string;
  ok: boolean;
}) {
  return (
    <div className="min-w-0 rounded-md border border-border bg-background/80 px-2 py-1.5 dark:border-slate-800 dark:bg-slate-950/60">
      <div className="text-[11px] text-muted-foreground dark:text-slate-500">
        {label}
      </div>
      <div
        className={cn(
          "mt-0.5 truncate font-mono text-[11px]",
          ok
            ? "text-emerald-700 dark:text-emerald-200"
            : "text-amber-700 dark:text-amber-200",
        )}
        title={value}
      >
        {value}
      </div>
    </div>
  );
}

/// MultiRouter 专用设置面板：只暴露方案级元信息和入口状态，避免用户误填普通供应商 API 字段。
function MultiRouterSettingsPanel({
  selectedPlan,
  selectedRoutes,
  onSave,
  onClose,
  isSaving,
}: {
  selectedPlan: Provider;
  selectedRoutes: RouteEntry[];
  onSave: (plan: Provider, draft: MultiRouterSettingsDraft) => Promise<void>;
  onClose: () => void;
  isSaving: boolean;
}) {
  const queryClient = useQueryClient();
  const selectedRouting = readCodexRouting(selectedPlan) ?? {};
  const [name, setName] = useState(selectedPlan.name);
  const [notes, setNotes] = useState(selectedPlan.notes ?? "");
  const [enabled, setEnabled] = useState(selectedRouting.enabled !== false);
  const [defaultRouteId, setDefaultRouteId] = useState(
    selectedRouting.defaultRouteId ?? "",
  );
  const { data: globalProxyConfig, error: globalProxyConfigError } = useQuery<
    GlobalProxyConfig,
    Error
  >({
    queryKey: ["globalProxyConfig"],
    queryFn: () => proxyApi.getGlobalProxyConfig(),
  });
  const [listenAddress, setListenAddress] = useState(
    DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
  );
  const [listenPort, setListenPort] = useState(
    String(DEFAULT_CODEX_PROXY_LISTEN_PORT),
  );
  const [listenerError, setListenerError] = useState<string | null>(null);
  const [isSavingListener, setIsSavingListener] = useState(false);

  useEffect(() => {
    const routing = readCodexRouting(selectedPlan) ?? {};
    setName(selectedPlan.name);
    setNotes(selectedPlan.notes ?? "");
    setEnabled(routing.enabled !== false);
    setDefaultRouteId(routing.defaultRouteId ?? "");
  }, [selectedPlan]);

  useEffect(() => {
    if (!globalProxyConfig) return;
    setListenAddress(
      globalProxyConfig.listenAddress || DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
    );
    setListenPort(
      String(globalProxyConfig.listenPort || DEFAULT_CODEX_PROXY_LISTEN_PORT),
    );
    setListenerError(null);
  }, [globalProxyConfig]);

  useEffect(() => {
    if (!globalProxyConfigError) return;
    setListenerError(globalProxyConfigError.message);
  }, [globalProxyConfigError]);

  /// 保存前同时写回方案草稿和全局监听配置；API Key 仍不在 MultiRouter 页面直接编辑。
  async function handleSave() {
    const listener = validateProxyListenDraft(listenAddress, listenPort);
    if (!listener.ok) {
      setListenerError(listener.error);
      return;
    }

    setIsSavingListener(true);
    setListenerError(null);
    try {
      const currentConfig =
        globalProxyConfig ?? (await proxyApi.getGlobalProxyConfig());
      if (
        currentConfig.listenAddress !== listener.listenAddress ||
        currentConfig.listenPort !== listener.listenPort
      ) {
        const nextConfig = {
          ...currentConfig,
          listenAddress: listener.listenAddress,
          listenPort: listener.listenPort,
        };
        await proxyApi.updateGlobalProxyConfig(nextConfig);
        queryClient.setQueryData(["globalProxyConfig"], nextConfig);
        queryClient.invalidateQueries({ queryKey: ["globalProxyConfig"] });
        queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
        queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      }
    } catch (error) {
      setListenerError(error instanceof Error ? error.message : String(error));
      setIsSavingListener(false);
      return;
    }

    await onSave(selectedPlan, {
      name,
      notes,
      enabled,
      defaultRouteId,
    });
    setIsSavingListener(false);
  }

  const routeOptions = selectedRoutes
    .map(({ route }) => ({
      id: route.id,
      label: route.label || route.id || "未命名规则",
      enabled: route.enabled !== false,
    }))
    .filter((route): route is { id: string; label: string; enabled: boolean } =>
      Boolean(route.id),
    );
  const listenerPreview = validateProxyListenDraft(listenAddress, listenPort);
  const previewBaseUrl = listenerPreview.ok
    ? listenerPreview.baseUrl
    : buildCodexProxyBaseUrl(
        DEFAULT_CODEX_PROXY_LISTEN_ADDRESS,
        DEFAULT_CODEX_PROXY_LISTEN_PORT,
      );
  const autoManagedRows = [
    {
      label: "Codex provider id",
      value: "codex_model_router_v2",
      detail: "统一稳定桶，多个 MultiRouter 不需要分别填写",
    },
    {
      label: "base_url",
      value: previewBaseUrl,
      detail: "切换或接管时由 CC Switch 投影到 Codex live config",
    },
    {
      label: "wire_api",
      value: "responses",
      detail: "Codex 只连接本地代理，真实上游协议由 route 决定",
    },
    {
      label: "model_catalog_json",
      value: "cc-switch-model-catalog.json",
      detail: "根据当前方案的 routes/modelCatalog 自动生成",
    },
  ];

  return (
    <section className="rounded-lg border border-blue-200 bg-card p-4 shadow-[0_0_0_1px_rgba(59,130,246,0.10)] dark:border-blue-700/50 dark:bg-slate-950/70 dark:shadow-[0_0_0_1px_rgba(59,130,246,0.15)]">
      <SectionHeader
        icon={Settings2}
        title="多路路由设置"
        detail="这里配置 MultiRouter 方案名称、默认路由和本地代理监听入口；上游 API Key 仍由各 route 目标模型源维护。"
        action={
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={onClose}
              disabled={isSaving || isSavingListener}
            >
              关闭
            </Button>
            <Button
              size="sm"
              onClick={handleSave}
              disabled={isSaving || isSavingListener}
              className="gap-2 bg-blue-600 hover:bg-blue-500"
            >
              <Save className="h-4 w-4" />
              {isSaving || isSavingListener ? "保存中" : "保存设置"}
            </Button>
          </div>
        }
      />

      <div className="mt-4 grid gap-4 lg:grid-cols-[1fr_1fr]">
        <div className="space-y-3">
          <div className="grid gap-2">
            <label className="text-xs font-semibold text-muted-foreground dark:text-slate-300">
              方案名称
            </label>
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="h-10 rounded-md border border-blue-200 bg-background px-3 text-sm outline-none transition placeholder:text-muted-foreground focus:border-blue-400 focus:ring-2 focus:ring-blue-500/20 dark:border-blue-700/50 dark:bg-slate-950/80 dark:placeholder:text-slate-500 dark:focus:ring-blue-500/30"
              placeholder="例如：Codex MultiRouter"
              disabled={isSaving || isSavingListener}
            />
          </div>
          <div className="grid gap-2">
            <label className="text-xs font-semibold text-muted-foreground dark:text-slate-300">
              备注
            </label>
            <textarea
              value={notes}
              onChange={(event) => setNotes(event.target.value)}
              rows={3}
              className="min-h-[84px] resize-y rounded-md border border-blue-200 bg-background px-3 py-2 text-sm outline-none transition placeholder:text-muted-foreground focus:border-blue-400 focus:ring-2 focus:ring-blue-500/20 dark:border-blue-700/50 dark:bg-slate-950/80 dark:placeholder:text-slate-500 dark:focus:ring-blue-500/30"
              placeholder="例如：默认 Codex 多模型路由"
              disabled={isSaving || isSavingListener}
            />
          </div>
          <label className="flex items-start justify-between gap-3 rounded-lg border border-border bg-muted/40 p-3 dark:border-slate-700 dark:bg-slate-950/50">
            <span>
              <span className="block text-sm font-semibold text-foreground dark:text-slate-100">
                MultiRouter 入口
              </span>
              <span className="mt-1 block text-xs leading-5 text-muted-foreground dark:text-slate-400">
                关闭后该方案不会参与 Codex model 分流，但 routes 会保留。
              </span>
            </span>
            <input
              type="checkbox"
              checked={enabled}
              onChange={(event) => setEnabled(event.target.checked)}
              className="mt-1 h-5 w-5 accent-blue-500"
              disabled={isSaving || isSavingListener}
            />
          </label>
          <div className="grid gap-2">
            <label className="text-xs font-semibold text-muted-foreground dark:text-slate-300">
              默认路由
            </label>
            <select
              value={defaultRouteId}
              onChange={(event) => setDefaultRouteId(event.target.value)}
              className="h-10 rounded-md border border-blue-200 bg-background px-3 text-sm outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-500/20 dark:border-blue-700/50 dark:bg-slate-950/80 dark:focus:ring-blue-500/30"
              disabled={
                isSaving || isSavingListener || routeOptions.length === 0
              }
            >
              <option value="">不设置默认路由</option>
              {routeOptions.map((route) => (
                <option key={route.id} value={route.id}>
                  {route.label}
                  {route.enabled ? "" : "（已停用）"}
                </option>
              ))}
            </select>
            <p className="text-xs leading-5 text-muted-foreground dark:text-slate-500">
              没有精确命中 model
              时才会使用默认路由；匹配规则仍在“编辑匹配规则”里选择。
            </p>
          </div>
          <div className="grid gap-3 rounded-lg border border-blue-200 bg-blue-50/70 p-3 dark:border-blue-700/40 dark:bg-blue-950/10 sm:grid-cols-[1fr_120px]">
            <div className="grid gap-2">
              <label className="text-xs font-semibold text-muted-foreground dark:text-slate-300">
                监听接口
              </label>
              <input
                value={listenAddress}
                onChange={(event) => setListenAddress(event.target.value)}
                className="h-10 rounded-md border border-blue-200 bg-background px-3 font-mono text-sm outline-none transition placeholder:text-muted-foreground focus:border-blue-400 focus:ring-2 focus:ring-blue-500/20 dark:border-blue-700/50 dark:bg-slate-950/80 dark:placeholder:text-slate-500 dark:focus:ring-blue-500/30"
                placeholder="127.0.0.1"
                disabled={isSaving || isSavingListener}
              />
            </div>
            <div className="grid gap-2">
              <label className="text-xs font-semibold text-muted-foreground dark:text-slate-300">
                监听端口
              </label>
              <input
                value={listenPort}
                onChange={(event) => setListenPort(event.target.value)}
                className="h-10 rounded-md border border-blue-200 bg-background px-3 font-mono text-sm outline-none transition placeholder:text-muted-foreground focus:border-blue-400 focus:ring-2 focus:ring-blue-500/20 dark:border-blue-700/50 dark:bg-slate-950/80 dark:placeholder:text-slate-500 dark:focus:ring-blue-500/30"
                placeholder="15721"
                inputMode="numeric"
                disabled={isSaving || isSavingListener}
              />
            </div>
            <div className="sm:col-span-2">
              <p className="break-all text-xs leading-5 text-muted-foreground dark:text-slate-500">
                Codex Desktop 使用：{previewBaseUrl}
              </p>
              {listenerError ? (
                <p className="mt-1 text-xs leading-5 text-rose-700 dark:text-rose-300">
                  {listenerError}
                </p>
              ) : null}
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-border bg-muted/40 p-3 dark:border-slate-700 dark:bg-slate-950/45">
          <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground dark:text-slate-100">
            <Info className="h-4 w-4 text-blue-600 dark:text-blue-300" />
            自动维护的接管配置
          </div>
          <div className="grid gap-2">
            {autoManagedRows.map((row) => (
              <div
                key={row.label}
                className="rounded-md border border-border bg-background/80 p-3 dark:border-slate-800 dark:bg-slate-950/70"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-xs font-semibold text-muted-foreground dark:text-slate-400">
                    {row.label}
                  </span>
                  <Badge className="border-blue-200 bg-blue-50 text-blue-800 dark:border-blue-500/50 dark:bg-blue-500/15 dark:text-blue-100">
                    自动
                  </Badge>
                </div>
                <div className="mt-1 break-all font-mono text-xs text-foreground dark:text-slate-100">
                  {row.value}
                </div>
                <div className="mt-1 text-xs leading-5 text-muted-foreground dark:text-slate-500">
                  {row.detail}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/// 进入路由页后自动读取候选 provider 的模型列表；这里集中展示成功/失败，避免候选缺失时无从判断。
function ProviderModelRefreshPanel({
  modelSources,
  states,
}: {
  modelSources: Provider[];
  states: Record<string, ProviderModelRefreshState>;
}) {
  const visibleRows = modelSources
    .map((provider) => ({ provider, state: states[provider.id] }))
    .filter(({ state }) => state && state.status !== "skipped");

  if (visibleRows.length === 0) return null;

  return (
    <section className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/45">
      <div className="mb-2 flex items-center gap-2 text-sm font-semibold text-foreground dark:text-slate-100">
        <RefreshCw className="h-4 w-4 text-sky-600 dark:text-sky-300" />
        候选 provider 模型列表刷新
      </div>
      <div className="grid gap-1.5 md:grid-cols-2 xl:grid-cols-4">
        {visibleRows.map(({ provider, state }) => {
          const tone =
            state.status === "success"
              ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-700/50 dark:bg-emerald-950/30 dark:text-emerald-100"
              : state.status === "loading"
                ? "border-sky-200 bg-sky-50 text-sky-800 dark:border-sky-700/50 dark:bg-sky-950/30 dark:text-sky-100"
                : "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-700/50 dark:bg-rose-950/30 dark:text-rose-100";
          return (
            <div
              key={provider.id}
              className={cn("rounded-md border px-2 py-1.5", tone)}
            >
              <div className="flex min-w-0 items-center justify-between gap-2">
                <span className="truncate text-xs font-semibold">
                  {provider.name}
                </span>
                <Badge className="shrink-0 border border-current bg-transparent text-[10px]">
                  {state.status === "success"
                    ? `${state.modelCount ?? 0} 个模型`
                    : state.status === "loading"
                      ? "读取中"
                      : "失败"}
                </Badge>
              </div>
              <div className="mt-1 truncate text-xs" title={state.message}>
                {state.message}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/// 子 Agent 候选模型属于路由规则配置：前五个会进入 Codex spawn_agent 的可用模型窗口。
/// 规则选择器是工作台专用编辑界面：用户只勾选候选 router，保存时统一写回 codexRouting.routes。
function RouteCandidatePicker({
  selectedPlan,
  modelSources,
  providerModelRefreshStates,
  onSaveRoutes,
  onCreateProvider,
  onClose,
  isSaving,
  selectAllByDefault,
}: {
  selectedPlan: Provider;
  modelSources: Provider[];
  providerModelRefreshStates: Record<string, ProviderModelRefreshState>;
  onSaveRoutes: (plan: Provider, routes: CodexRoute[]) => Promise<void>;
  onCreateProvider: () => void;
  onClose: () => void;
  isSaving: boolean;
  selectAllByDefault?: boolean;
}) {
  const candidates = useMemo(
    () => buildRouteCandidates(selectedPlan, modelSources),
    [selectedPlan, modelSources],
  );
  const candidateIds = useMemo(
    () => candidates.map((candidate) => candidate.id),
    [candidates],
  );
  const candidateIdsKey = candidateIds.join("\n");
  const draftPlanIdRef = useRef<string | null>(null);
  const draftCandidateIdsRef = useRef<string[]>([]);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() =>
    buildInitialRoutePickerSelectedIds(candidates, selectAllByDefault),
  );
  const [enabledIds, setEnabledIds] = useState<Set<string>>(() =>
    buildInitialRoutePickerEnabledIds(candidates, selectAllByDefault),
  );

  useEffect(() => {
    const currentPlanId = selectedPlan?.id ?? null;
    const previousPlanId = draftPlanIdRef.current;
    const previousCandidateIds = draftCandidateIdsRef.current;
    const selectedDefaults = Array.from(
      buildInitialRoutePickerSelectedIds(candidates, selectAllByDefault),
    );
    const enabledDefaults = Array.from(
      buildInitialRoutePickerEnabledIds(candidates, selectAllByDefault),
    );

    if (previousPlanId !== currentPlanId) {
      setSelectedIds(new Set(selectedDefaults));
      setEnabledIds(new Set(enabledDefaults));
    } else {
      setSelectedIds((current) =>
        mergeRoutePickerDraftIds(
          current,
          previousCandidateIds,
          candidateIds,
          selectedDefaults,
        ),
      );
      setEnabledIds((current) =>
        mergeRoutePickerDraftIds(
          current,
          previousCandidateIds,
          candidateIds,
          enabledDefaults,
        ),
      );
    }

    draftPlanIdRef.current = currentPlanId;
    draftCandidateIdsRef.current = candidateIds;
  }, [candidateIdsKey, candidates, selectedPlan?.id, selectAllByDefault]);

  /// 切换 Set 状态时始终返回新实例，避免 React 因引用未变而跳过刷新。
  function toggleSetValue(
    setter: Dispatch<SetStateAction<Set<string>>>,
    id: string,
  ) {
    setter((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  /// 保存前只保留勾选项，并把启用状态同步到 route.enabled；取消勾选即删除该 route。
  async function handleSave() {
    const routes = candidates
      .filter((candidate) => selectedIds.has(candidate.id))
      .map((candidate) => ({
        ...candidate.route,
        enabled: enabledIds.has(candidate.id),
      }));
    await onSaveRoutes(selectedPlan, routes);
  }

  return (
    <section className="rounded-lg border border-emerald-200 bg-card p-3 shadow-[0_0_0_1px_rgba(16,185,129,0.10)] dark:border-emerald-700/50 dark:bg-slate-950/70 dark:shadow-[0_0_0_1px_rgba(16,185,129,0.15)]">
      <SectionHeader
        icon={Route}
        title="选择候选 router"
        detail="这里直接选择哪些模型源进入当前多路路由；取消勾选会从规则中移除，不再打开普通供应商编辑表单。"
        action={
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setSelectedIds(
                  new Set(candidates.map((candidate) => candidate.id)),
                );
                setEnabledIds(
                  new Set(candidates.map((candidate) => candidate.id)),
                );
              }}
              disabled={candidates.length === 0 || isSaving}
            >
              全选并启用
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                setSelectedIds(
                  new Set(
                    candidates
                      .filter((candidate) => candidate.isExisting)
                      .map((candidate) => candidate.id),
                  ),
                );
                setEnabledIds(
                  new Set(
                    candidates
                      .filter(
                        (candidate) =>
                          candidate.isExisting &&
                          candidate.route.enabled !== false,
                      )
                      .map((candidate) => candidate.id),
                  ),
                );
              }}
              disabled={isSaving}
            >
              只保留当前状态
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={onClose}
              disabled={isSaving}
            >
              关闭
            </Button>
            <Button
              size="sm"
              onClick={handleSave}
              disabled={isSaving}
              className="gap-2 bg-emerald-600 hover:bg-emerald-500"
            >
              <Save className="h-4 w-4" />
              {isSaving ? "保存中" : "保存规则"}
            </Button>
          </div>
        }
      />

      <div className="mt-2 grid gap-2 md:grid-cols-2">
        {candidates.map((candidate) => {
          const checked = selectedIds.has(candidate.id);
          const enabled = enabledIds.has(candidate.id);
          const targetLabel =
            candidate.provider?.name ??
            routeTargetProviderId(candidate.route) ??
            "自定义 route";
          const refreshState = candidate.provider
            ? providerModelRefreshStates[candidate.provider.id]
            : undefined;
          return (
            <div
              key={candidate.id}
              className={cn(
                "rounded-lg border p-2.5 transition",
                checked
                  ? "border-emerald-300 bg-emerald-50 dark:border-emerald-500/60 dark:bg-emerald-500/10"
                  : "border-border bg-background dark:border-slate-700 dark:bg-slate-950/40",
              )}
            >
              <div className="flex flex-wrap items-start justify-between gap-2">
                <button
                  type="button"
                  onClick={() => toggleSetValue(setSelectedIds, candidate.id)}
                  className="flex min-w-0 flex-1 items-start gap-2 text-left"
                >
                  <span
                    className={cn(
                      "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded border",
                      checked
                        ? "border-emerald-300 bg-emerald-500 text-slate-950"
                        : "border-border bg-muted dark:border-slate-600 dark:bg-slate-900",
                    )}
                  >
                    {checked ? <CheckCircle2 className="h-3.5 w-3.5" /> : null}
                  </span>
                  <span className="min-w-0">
                    <span className="flex min-w-0 flex-wrap items-center gap-2">
                      <span className="truncate text-sm font-semibold text-foreground dark:text-slate-100">
                        {candidate.route.label || targetLabel}
                      </span>
                      <Badge
                        className={cn(
                          "border text-[11px]",
                          !checked
                            ? "border-border bg-muted text-muted-foreground dark:border-slate-600 dark:bg-slate-900 dark:text-slate-300"
                            : enabled
                              ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/60 dark:bg-emerald-500/15 dark:text-emerald-100"
                              : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/60 dark:bg-amber-500/15 dark:text-amber-100",
                        )}
                      >
                        {!checked
                          ? "未加入"
                          : enabled
                            ? "已加入并启用"
                            : "已加入但停用"}
                      </Badge>
                    </span>
                    <span className="mt-0.5 block truncate text-xs text-muted-foreground dark:text-slate-400">
                      {targetLabel} ·{" "}
                      {candidate.isExisting ? "已在规则中" : "候选模型源"}
                    </span>
                  </span>
                </button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    if (!checked) {
                      setSelectedIds(
                        (current) => new Set([...current, candidate.id]),
                      );
                      setEnabledIds(
                        (current) => new Set([...current, candidate.id]),
                      );
                      return;
                    }
                    toggleSetValue(setEnabledIds, candidate.id);
                  }}
                  disabled={isSaving}
                  className={cn(
                    "h-8 min-w-[78px] px-2",
                    enabled
                      ? "border-emerald-300 text-emerald-700 dark:border-emerald-500/50 dark:text-emerald-100"
                      : "border-amber-300 text-amber-700 dark:border-amber-500/50 dark:text-amber-100",
                  )}
                >
                  {!checked ? "启用" : enabled ? "已启用" : "已停用"}
                </Button>
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5 text-xs">
                {candidate.matchModels.slice(0, 6).map((model) => (
                  <span
                    key={model}
                    className="rounded-full border border-border bg-muted px-2 py-0.5 text-muted-foreground dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300"
                  >
                    {model}
                  </span>
                ))}
                {candidate.matchModels.length > 6 ? (
                  <span className="rounded-full border border-border bg-muted px-2 py-0.5 text-muted-foreground dark:border-slate-700 dark:bg-slate-900 dark:text-slate-400">
                    +{candidate.matchModels.length - 6}
                  </span>
                ) : null}
                {candidate.matchPrefixes.map((prefix) => (
                  <span
                    key={prefix}
                    className="rounded-full border border-blue-200 bg-blue-50 px-2 py-0.5 text-blue-800 dark:border-blue-700/60 dark:bg-blue-950/40 dark:text-blue-100"
                  >
                    {prefix}*
                  </span>
                ))}
                {candidate.matchModels.length === 0 &&
                candidate.matchPrefixes.length === 0 ? (
                  <span className="rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 text-amber-800 dark:border-amber-600/60 dark:bg-amber-950/30 dark:text-amber-100">
                    未发现模型目录，保存后可在模型源补充目录
                  </span>
                ) : null}
              </div>
              {refreshState && refreshState.status !== "skipped" ? (
                <div
                  className={cn(
                    "mt-2 rounded-md border px-2 py-1.5 text-xs leading-5",
                    refreshState.status === "success"
                      ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-700/50 dark:bg-emerald-950/30 dark:text-emerald-100"
                      : refreshState.status === "loading"
                        ? "border-sky-200 bg-sky-50 text-sky-800 dark:border-sky-700/50 dark:bg-sky-950/30 dark:text-sky-100"
                        : "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-700/50 dark:bg-rose-950/30 dark:text-rose-100",
                  )}
                >
                  {refreshState.message}
                </div>
              ) : null}
            </div>
          );
        })}
        {candidates.length === 0 ? (
          <EmptyState
            icon={Server}
            title="没有可选 router"
            detail="先添加至少一个 Codex 模型源，添加完成后会回到这里继续选择候选 router。"
            actionLabel="添加模型源"
            onAction={onCreateProvider}
          />
        ) : null}
      </div>
    </section>
  );
}

function SpawnAgentCandidatesPanel({
  selectedPlan,
  selectedRoutes,
}: {
  selectedPlan: Provider | null;
  selectedRoutes: RouteEntry[];
}) {
  const [diagnostics, setDiagnostics] =
    useState<CodexMultiRouterDiagnostics | null>(null);
  const [candidateView, setCandidateView] =
    useState<SpawnAgentCandidateView>("selected");
  const [draftSpawnAgentModels, setDraftSpawnAgentModels] = useState<string[]>(
    [],
  );
  const [candidateSaveError, setCandidateSaveError] = useState<string | null>(
    null,
  );
  const [candidateSaveMessage, setCandidateSaveMessage] = useState<
    string | null
  >(null);
  const [candidateValidationMessage, setCandidateValidationMessage] = useState<
    string | null
  >(null);
  const [isSavingCandidates, setIsSavingCandidates] = useState(false);
  const [isValidatingCandidates, setIsValidatingCandidates] = useState(false);
  const queryClient = useQueryClient();
  const candidateSensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  const selectedCatalog = readCodexModelCatalog(selectedPlan);
  const selectedCatalogModelKey = selectedCatalog.models
    .map((model) => model.model?.trim() ?? "")
    .join("\n");
  const selectedCatalogSpawnAgentKey =
    selectedCatalog.spawnAgentModels.join("\n");
  const selectedCatalogByModel = new Map(
    selectedCatalog.models
      .filter((model) => model.model?.trim())
      .map((model) => [model.model!.trim(), model]),
  );
  const spawnAgentVisibleLimit =
    diagnostics?.liveConfig.spawnAgentVisibleModelLimit ?? 5;
  const configuredSpawnAgentModels = selectedCatalog.spawnAgentModels
    .map((model) => selectedCatalogByModel.get(model) ?? { model })
    .slice(0, spawnAgentVisibleLimit);
  const generatedVisibleModels =
    diagnostics?.liveConfig.modelCatalogFirstModels
      ?.slice(0, spawnAgentVisibleLimit)
      .map((model) => selectedCatalogByModel.get(model) ?? { model }) ?? [];
  const previewVisibleModels =
    generatedVisibleModels.length > 0
      ? generatedVisibleModels
      : configuredSpawnAgentModels.length > 0
        ? configuredSpawnAgentModels
        : selectedCatalog.models.slice(0, spawnAgentVisibleLimit);
  const routedCatalogModelIds = useMemo(
    () => collectRoutedCatalogModels(selectedRoutes, selectedCatalog.models),
    [selectedRoutes, selectedCatalog.models],
  );
  const draftVisibleModels = draftSpawnAgentModels.map(
    (model) => selectedCatalogByModel.get(model) ?? { model },
  );
  const candidateCatalog = {
    ...selectedCatalog,
    spawnAgentModels: draftSpawnAgentModels,
  };
  const localCandidateValidation = validateSpawnAgentCandidates(
    candidateCatalog,
    draftSpawnAgentModels.length > 0
      ? draftSpawnAgentModels
      : selectedCatalog.models
          .map((model) => model.model?.trim())
          .filter((model): model is string => Boolean(model))
          .slice(0, spawnAgentVisibleLimit),
    [],
    spawnAgentVisibleLimit,
  );
  const actualCandidateValidation = validateSpawnAgentCandidates(
    candidateCatalog,
    diagnostics?.liveConfig.modelCatalogFirstModels ?? [],
    [],
    spawnAgentVisibleLimit,
  );
  const candidateSourceModels = {
    selected: draftSpawnAgentModels,
    routed: routedCatalogModelIds,
    priority: CODEX_SPAWN_AGENT_PRIORITY_MODELS.filter((model) =>
      selectedCatalogByModel.has(model),
    ),
    all: selectedCatalog.models
      .map((model) => model.model?.trim())
      .filter((model): model is string => Boolean(model)),
  } satisfies Record<SpawnAgentCandidateView, string[]>;
  const selectedCandidateSet = new Set(draftSpawnAgentModels);
  const hasCandidateChanges =
    draftSpawnAgentModels.join("\n") !==
    selectedCatalog.spawnAgentModels.join("\n");
  const spawnAgentMissingPriorityModels =
    diagnostics?.liveConfig.spawnAgentMissingPriorityModels ?? [];

  useEffect(() => {
    setDraftSpawnAgentModels(
      normalizeSpawnAgentCandidateSelection(
        selectedCatalog.spawnAgentModels,
        selectedCatalog.models,
        spawnAgentVisibleLimit,
      ),
    );
    setCandidateSaveError(null);
    setCandidateSaveMessage(null);
    setCandidateValidationMessage(null);
  }, [
    selectedPlan?.id,
    selectedCatalogSpawnAgentKey,
    selectedCatalogModelKey,
    spawnAgentVisibleLimit,
  ]);

  /// 点击候选模型时只改变草稿；保存前不会写数据库，便于用户先检查和拖动排序。
  function toggleSpawnAgentCandidate(model: string) {
    setCandidateSaveError(null);
    setCandidateSaveMessage(null);
    setCandidateValidationMessage(null);
    setDraftSpawnAgentModels((current) => {
      if (current.includes(model)) {
        return current.filter((item) => item !== model);
      }
      return normalizeSpawnAgentCandidateSelection(
        [...current, model],
        selectedCatalog.models,
        spawnAgentVisibleLimit,
      );
    });
  }

  /// 拖拽结束后只重排当前草稿，并继续受 Codex spawn_agent 前五个可见模型限制保护。
  function handleSpawnAgentDragEnd(event: DragEndEvent) {
    const activeModel = String(event.active.id);
    const overModel = event.over ? String(event.over.id) : "";
    if (!overModel) return;
    setDraftSpawnAgentModels((current) =>
      reorderSpawnAgentCandidates(
        current,
        activeModel,
        overModel,
        spawnAgentVisibleLimit,
      ),
    );
  }

  /// 写回 provider 时只更新 cc-switch 私有的 modelCatalog.spawnAgentModels，避免破坏 auth、routing 和统计归属。
  async function saveSpawnAgentCandidates() {
    if (!selectedPlan) return;
    setIsSavingCandidates(true);
    setCandidateSaveError(null);
    setCandidateSaveMessage(null);
    try {
      const normalized = normalizeSpawnAgentCandidateSelection(
        draftSpawnAgentModels,
        selectedCatalog.models,
        spawnAgentVisibleLimit,
      );
      const currentModelCatalog =
        selectedPlan.settingsConfig?.modelCatalog &&
        typeof selectedPlan.settingsConfig.modelCatalog === "object"
          ? selectedPlan.settingsConfig.modelCatalog
          : {};
      const nextProvider: Provider = {
        ...selectedPlan,
        settingsConfig: {
          ...selectedPlan.settingsConfig,
          modelCatalog: {
            ...currentModelCatalog,
            spawnAgentModels: normalized,
          },
        },
      };
      await providersApi.update(nextProvider, "codex");
      setDraftSpawnAgentModels(normalized);
      setCandidateSaveMessage(
        `已保存 ${normalized.length} 个子 Agent 可见候选；重启 Codex 后生效。`,
      );
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    } catch (error) {
      setCandidateSaveError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setIsSavingCandidates(false);
    }
  }

  /// 校验分两步：先检查本地草稿窗口，再读取 live 诊断，确认 Codex 实际生成的前五个模型。
  async function validateSpawnAgentCandidateWindow() {
    setIsValidatingCandidates(true);
    setCandidateValidationMessage(null);
    try {
      const result = await proxyApi.diagnoseCodexMultiRouter(
        selectedPlan?.id ?? null,
      );
      setDiagnostics(result);
      const actual = validateSpawnAgentCandidates(
        candidateCatalog,
        result.liveConfig.modelCatalogFirstModels ?? [],
        [],
        result.liveConfig.spawnAgentVisibleModelLimit ?? spawnAgentVisibleLimit,
      );
      const missing = [
        ...new Set([
          ...actual.missingSelectedModels,
          ...actual.missingPriorityModels,
        ]),
      ];
      setCandidateValidationMessage(
        missing.length > 0
          ? `live 前 ${actual.visibleModels.length} 个候选仍缺少：${missing.join(", ")}`
          : `校验通过：live 可见窗口已覆盖当前选择，实际窗口为 ${actual.visibleModels.join(", ") || "空"}`,
      );
    } catch (error) {
      setCandidateValidationMessage(
        `校验失败：${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      setIsValidatingCandidates(false);
    }
  }

  return (
    <section className="rounded-lg border border-violet-200 bg-violet-50/70 p-3 dark:border-violet-700/40 dark:bg-violet-950/15">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="flex items-center gap-2 text-sm font-semibold text-violet-800 dark:text-violet-100">
            <GitBranch className="h-4 w-4" />子 Agent 候选模型
          </div>
          <div className="mt-0.5 text-xs text-violet-700/80 dark:text-violet-200/80">
            前 {spawnAgentVisibleLimit} 个进入
            spawn_agent；拖拽只改候选顺序，不改实际路由。
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={validateSpawnAgentCandidateWindow}
            disabled={isValidatingCandidates || !selectedPlan}
            className="gap-2 border-emerald-300 bg-background/70 text-emerald-700 hover:bg-emerald-50 dark:border-emerald-500/50 dark:bg-emerald-500/10 dark:text-emerald-100 dark:hover:bg-emerald-500/20"
          >
            {isValidatingCandidates ? (
              <RefreshCw className="h-4 w-4 animate-spin" />
            ) : (
              <CheckCircle2 className="h-4 w-4" />
            )}
            校验候选
          </Button>
          <Button
            size="sm"
            onClick={saveSpawnAgentCandidates}
            disabled={
              isSavingCandidates || !selectedPlan || !hasCandidateChanges
            }
            className="gap-2 bg-violet-600 hover:bg-violet-500"
          >
            {isSavingCandidates ? (
              <RefreshCw className="h-4 w-4 animate-spin" />
            ) : (
              <Save className="h-4 w-4" />
            )}
            保存排序
          </Button>
        </div>
      </div>

      <div className="mt-2 grid items-stretch gap-2 xl:grid-cols-[minmax(0,1fr)_minmax(260px,0.65fr)]">
        <div className="space-y-2">
          <div>
            <div className="mb-1.5 text-xs font-semibold text-violet-800 dark:text-violet-100">
              Codex spawn_agent 前五可用模型
            </div>
            <div className="grid gap-1.5 md:grid-cols-5">
              {previewVisibleModels.length > 0 ? (
                previewVisibleModels.map((model, index) => (
                  <div
                    key={`${model.model ?? index}-${index}`}
                    className="min-w-0 rounded-md border border-amber-300 bg-amber-50 px-2 py-1.5 shadow-[0_0_0_1px_rgba(251,191,36,0.12)] dark:border-amber-400/70 dark:bg-amber-500/15 dark:shadow-[0_0_0_1px_rgba(251,191,36,0.18)]"
                  >
                    <div className="flex items-center justify-between gap-2 text-[10px] text-amber-700 dark:text-amber-200">
                      <span>#{index + 1}</span>
                      <span>spawn</span>
                    </div>
                    <div
                      className="mt-0.5 truncate font-mono text-[11px] text-foreground dark:text-slate-50"
                      title={catalogModelLabel(model)}
                    >
                      {catalogModelLabel(model)}
                    </div>
                  </div>
                ))
              ) : (
                <div className="rounded-md border border-violet-200 bg-background/80 px-3 py-2 text-xs text-violet-800 dark:border-violet-800/60 dark:bg-slate-950/45 dark:text-violet-100 md:col-span-5">
                  当前 MultiRouter provider 还没有
                  modelCatalog；请先在模型映射里添加 OpenAI / Qwen / DeepSeek
                  等候选模型。
                </div>
              )}
            </div>
          </div>

          <div>
            <div className="mb-1.5 flex items-center justify-between gap-2">
              <div className="text-xs font-semibold text-violet-800 dark:text-violet-100">
                可拖拽排序的前五候选
              </div>
              <Badge className="border border-violet-200 bg-violet-50 text-violet-800 dark:border-violet-500/40 dark:bg-violet-500/10 dark:text-violet-100">
                {draftSpawnAgentModels.length} / {spawnAgentVisibleLimit}
              </Badge>
            </div>
            <DndContext
              sensors={candidateSensors}
              collisionDetection={closestCenter}
              onDragEnd={handleSpawnAgentDragEnd}
            >
              <SortableContext
                items={draftSpawnAgentModels}
                strategy={verticalListSortingStrategy}
              >
                <div className="grid gap-1.5">
                  {draftVisibleModels.length > 0 ? (
                    draftVisibleModels.map((model, index) => (
                      <SortableSpawnAgentCandidate
                        key={model.model}
                        model={model}
                        index={index}
                        onRemove={toggleSpawnAgentCandidate}
                      />
                    ))
                  ) : (
                    <div className="rounded-md border border-dashed border-violet-200 bg-background/70 px-3 py-2 text-xs text-violet-800 dark:border-violet-700/60 dark:bg-slate-950/30 dark:text-violet-100">
                      还没有选择子 Agent 候选；从右侧候选池添加，最多{" "}
                      {spawnAgentVisibleLimit} 个。
                    </div>
                  )}
                </div>
              </SortableContext>
            </DndContext>
          </div>
        </div>

        <div className="flex h-full min-h-0 flex-col rounded-md border border-violet-200 bg-background/70 p-2 dark:border-violet-800/50 dark:bg-slate-950/35">
          <Tabs
            value={candidateView}
            onValueChange={(value) =>
              setCandidateView(value as SpawnAgentCandidateView)
            }
            className="flex h-full min-h-0 flex-col"
          >
            <TabsList className="grid w-full grid-cols-4 bg-muted p-1 dark:bg-slate-950/60">
              <TabsTrigger value="selected">已选</TabsTrigger>
              <TabsTrigger value="routed">路由</TabsTrigger>
              <TabsTrigger value="priority">重点</TabsTrigger>
              <TabsTrigger value="all">全部</TabsTrigger>
            </TabsList>
            {(["selected", "routed", "priority", "all"] as const).map(
              (view) => (
                <TabsContent
                  key={view}
                  value={view}
                  className="mt-2 min-h-0 flex-1"
                >
                  <div className="max-h-[220px] min-h-[132px] space-y-1.5 overflow-y-auto pr-1 xl:max-h-[260px]">
                    {candidateSourceModels[view].length > 0 ? (
                      candidateSourceModels[view].map((model) => {
                        const catalogModel = selectedCatalogByModel.get(
                          model,
                        ) ?? { model };
                        const isSelected = selectedCandidateSet.has(model);
                        const selectedIndex =
                          draftSpawnAgentModels.indexOf(model);
                        return (
                          <button
                            key={`${view}-${model}`}
                            type="button"
                            onClick={() => toggleSpawnAgentCandidate(model)}
                            disabled={
                              !isSelected &&
                              draftSpawnAgentModels.length >=
                                spawnAgentVisibleLimit
                            }
                            className={cn(
                              "flex w-full items-center justify-between gap-2 rounded-md border px-2 py-1.5 text-left text-xs transition",
                              isSelected
                                ? "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-400/70 dark:bg-amber-500/15 dark:text-amber-50"
                                : "border-border bg-card text-foreground hover:border-violet-300 hover:bg-violet-50 dark:border-slate-700 dark:bg-slate-950/45 dark:text-slate-200 dark:hover:border-violet-500/60 dark:hover:bg-violet-500/10",
                              !isSelected &&
                                draftSpawnAgentModels.length >=
                                  spawnAgentVisibleLimit
                                ? "cursor-not-allowed opacity-45"
                                : "",
                            )}
                          >
                            <span className="min-w-0 truncate font-mono">
                              {catalogModelLabel(catalogModel)}
                            </span>
                            <Badge
                              className={cn(
                                "shrink-0 border text-[10px]",
                                isSelected
                                  ? "border-amber-300 bg-amber-100 text-amber-800 dark:border-amber-300/70 dark:bg-amber-200/10 dark:text-amber-50"
                                  : "border-border bg-muted text-muted-foreground dark:border-slate-600 dark:bg-slate-800 dark:text-slate-300",
                              )}
                            >
                              {isSelected
                                ? `前五 #${selectedIndex + 1}`
                                : "添加"}
                            </Badge>
                          </button>
                        );
                      })
                    ) : (
                      <div className="rounded-md border border-dashed border-border px-3 py-2 text-xs text-muted-foreground dark:border-slate-700 dark:text-slate-400">
                        这个来源暂时没有可用模型。
                      </div>
                    )}
                  </div>
                </TabsContent>
              ),
            )}
          </Tabs>
        </div>
      </div>

      <div className="mt-2 flex flex-wrap gap-1.5 text-[11px] text-violet-700/80 dark:text-violet-200/80">
        <Badge className="border border-violet-200 bg-violet-50 text-violet-800 dark:border-violet-500/40 dark:bg-violet-500/10 dark:text-violet-100">
          catalog: {selectedCatalog.models.length}
        </Badge>
        <Badge className="border border-violet-200 bg-violet-50 text-violet-800 dark:border-violet-500/40 dark:bg-violet-500/10 dark:text-violet-100">
          路由命中: {routedCatalogModelIds.length}
        </Badge>
        <Badge className="border border-violet-200 bg-violet-50 text-violet-800 dark:border-violet-500/40 dark:bg-violet-500/10 dark:text-violet-100">
          来源: {generatedVisibleModels.length > 0 ? "诊断实测" : "配置预览"}
        </Badge>
        <Badge
          className={cn(
            "border",
            localCandidateValidation.missingSelectedModels.length === 0
              ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-100"
              : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100",
          )}
        >
          本地检查:{" "}
          {localCandidateValidation.missingSelectedModels.length === 0
            ? "已选已覆盖"
            : `缺 ${localCandidateValidation.missingSelectedModels.length} 个已选`}
        </Badge>
      </div>

      {candidateSaveError ? (
        <div className="mt-3 rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-xs leading-5 text-rose-700 dark:border-rose-700/50 dark:bg-rose-950/30 dark:text-rose-100">
          保存失败：{candidateSaveError}
        </div>
      ) : null}
      {candidateSaveMessage ? (
        <div className="mt-3 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs leading-5 text-emerald-800 dark:border-emerald-700/50 dark:bg-emerald-950/30 dark:text-emerald-100">
          {candidateSaveMessage}
        </div>
      ) : null}
      {candidateValidationMessage ? (
        <div className="mt-3 rounded-md border border-sky-200 bg-sky-50 px-3 py-2 text-xs leading-5 text-sky-800 dark:border-sky-700/50 dark:bg-sky-950/30 dark:text-sky-100">
          {candidateValidationMessage}
        </div>
      ) : null}
      {actualCandidateValidation.missingSelectedModels.length > 0 ? (
        <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-800 dark:border-amber-700/50 dark:bg-amber-950/30 dark:text-amber-100">
          live 可见窗口还没覆盖已选模型：
          {actualCandidateValidation.missingSelectedModels.join(", ")}
          。保存后请重启 Codex Desktop/app-server 再校验。
        </div>
      ) : null}
      {spawnAgentMissingPriorityModels.length > 0 ? (
        <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-800 dark:border-amber-700/50 dark:bg-amber-950/30 dark:text-amber-100">
          仍有重点模型不在前 {spawnAgentVisibleLimit} 个可见候选中：
          {spawnAgentMissingPriorityModels.join(", ")}
          。请把它们加入子 Agent 候选列表并重启 Codex Desktop/app-server。
        </div>
      ) : null}
    </section>
  );
}

/// 状态页把代理运行态、Codex 接管态、路由配置态和最近流量放在同一视图里。
function StatusTab({
  selectedPlan,
  selectedRouting,
  routeEntries,
  providersById,
  proxyStatus,
  isProxyRunning,
  isCodexTakeoverActive,
  activeProviderId,
  onEditPlan,
  onDeletePlan,
  onRuntimeReady,
}: {
  selectedPlan: Provider | null;
  selectedRouting: CodexRouting | null;
  routeEntries: RouteEntry[];
  providersById: Map<string, Provider>;
  proxyStatus?: ProxyStatus;
  isProxyRunning: boolean;
  isCodexTakeoverActive: boolean;
  activeProviderId?: string;
  onEditPlan: (provider: Provider, detail?: string) => void;
  onDeletePlan: (provider: Provider) => void;
  onRuntimeReady?: (provider: Provider) => void;
}) {
  const queryClient = useQueryClient();
  const range = useMemo(() => ({ preset: "today" as const }), []);
  const { data: requestLogs, isLoading } = useRequestLogs({
    filters: { appType: "codex" },
    range,
    page: 0,
    pageSize: 500,
    options: { refetchInterval: 5000 },
  });
  const {
    data: subagentUsage,
    isLoading: isLoadingSubagentUsage,
    error: subagentUsageError,
  } = useCodexSubagentUsageStats(range, 80, {
    refetchInterval: 10000,
  });
  const [diagnostics, setDiagnostics] =
    useState<CodexMultiRouterDiagnostics | null>(null);
  const [diagnoseError, setDiagnoseError] = useState<string | null>(null);
  const [isDiagnosing, setIsDiagnosing] = useState(false);
  const [modelPickerUnlockResult, setModelPickerUnlockResult] =
    useState<CodexModelPickerUnlockResult | null>(null);
  const [modelPickerUnlockError, setModelPickerUnlockError] = useState<
    string | null
  >(null);
  const [isUnlockingModelPicker, setIsUnlockingModelPicker] = useState(false);
  const [statusView, setStatusView] = useState<StatusView>("link");
  const [isRefreshingValidation, setIsRefreshingValidation] = useState(false);
  const [validationRefreshMessage, setValidationRefreshMessage] = useState<
    string | null
  >(null);
  const [isSyncingSessionUsage, setIsSyncingSessionUsage] = useState(false);
  const [sessionSyncMessage, setSessionSyncMessage] = useState<string | null>(
    null,
  );
  const logs = requestLogs?.data ?? [];
  const proxyLogs = logs.filter(
    (log) => (log.dataSource ?? "proxy") === "proxy",
  );
  const sessionLogs = logs.filter(
    (log) => (log.dataSource ?? "proxy") !== "proxy",
  );
  const selectedRoutes = selectedPlan
    ? routeEntries.filter(({ provider }) => provider.id === selectedPlan.id)
    : routeEntries;
  const routerEvents = diagnostics?.routerLog.recentEvents ?? [];
  const routerRequestEvents = routerEvents.filter((event) =>
    [
      "route_resolved",
      "request_prepared",
      "upstream_send",
      "upstream_status",
      "upstream_error",
      "upstream_send_error",
    ].includes(event.event),
  );
  const routeTargetCount = new Set(
    selectedRoutes.map(
      (entry) => routeTrafficTarget(entry, providersById).providerId,
    ),
  ).size;
  const routeSummaryMap = useMemo(
    () => buildRouteSummaryMap(diagnostics),
    [diagnostics],
  );
  const trafficRows = buildRouteTrafficRows({
    logs: proxyLogs,
    routerEvents,
    routes: routeEntries,
    selectedPlan,
    providersById,
    routeSummaries: routeSummaryMap,
  });
  const protocolRows = trafficRows.filter(
    (row) =>
      row.configuredProtocol ||
      row.lastObservedProtocol ||
      row.requestCount > 0,
  );
  const routerLogs = routerEvents;
  const routedLogs = proxyLogs.filter((log) =>
    trafficRows.some(
      (row) =>
        row.providerId === log.providerId ||
        row.model === (log.requestModel || log.model),
    ),
  );
  const latestLog = proxyLogs[0];
  const latestForwardOk = latestLog
    ? latestLog.statusCode >= 200 && latestLog.statusCode < 400
    : false;
  // 配置成功不能只看“任意 Codex 请求成功”，必须看到当前 MultiRouter 方案的 route 有成功转发证据。
  const currentRouteForwardOk = trafficRows.some((row) => row.successCount > 0);
  const listenAddress = proxyStatus
    ? `${proxyStatus.address}:${proxyStatus.port}`
    : "未启动";
  const activeTargetLabel =
    activeProviderId && providersById.get(activeProviderId)
      ? `${providersById.get(activeProviderId)?.name} (${activeProviderId})`
      : activeProviderId || "未命中";
  const routeEnabled = selectedRouting?.enabled !== false;
  const hasEnabledRoutes = selectedRoutes.some(
    ({ route }) => route.enabled !== false,
  );
  const runtimeStatus = buildMultiRouterRuntimeStatus({
    selectedPlan,
    selectedRouting,
    enabledRouteCount: selectedRoutes.filter(
      ({ route }) => route.enabled !== false,
    ).length,
    isProxyRunning,
    isCodexTakeoverActive,
    activeProviderId,
  });
  const configReady = Boolean(
    isProxyRunning &&
      isCodexTakeoverActive &&
      selectedPlan &&
      activeProviderId === selectedPlan.id &&
      routeEnabled &&
      hasEnabledRoutes,
  );
  const trafficVerified = currentRouteForwardOk;
  const linkOnline = Boolean(runtimeStatus.running && trafficVerified);
  const readinessIssues = [
    !isProxyRunning ? "本地代理未监听" : "",
    !isCodexTakeoverActive ? "Codex live 配置未接管" : "",
    !selectedPlan ? "未选择 MultiRouter provider" : "",
    selectedPlan && activeProviderId !== selectedPlan.id
      ? "当前 Codex provider 不是选中的 MultiRouter"
      : "",
    selectedPlan && !routeEnabled ? "MultiRouter 入口已关闭" : "",
    selectedPlan && routeEnabled && !hasEnabledRoutes
      ? "没有启用的匹配规则"
      : "",
  ].filter(Boolean);

  // 当状态页确认 MultiRouter 配置与真实转发都成功后，通知 App 进入“配置成功 -> 历史修复”收尾流程。
  useEffect(() => {
    if (!selectedPlan || !linkOnline || !currentRouteForwardOk) return;
    onRuntimeReady?.(selectedPlan);
  }, [currentRouteForwardOk, linkOnline, onRuntimeReady, selectedPlan]);

  /// 配置完成返回状态页后，手动刷新所有校验数据，避免用户等待轮询才看到最新监听、接管和转发日志。
  async function refreshValidationState() {
    setIsRefreshingValidation(true);
    setValidationRefreshMessage(null);
    try {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["proxyStatus"] }),
        queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] }),
        queryClient.invalidateQueries({ queryKey: ["providers", "codex"] }),
        queryClient.invalidateQueries({ queryKey: usageKeys.all }),
      ]);
      await Promise.all([
        queryClient.refetchQueries({
          queryKey: ["proxyStatus"],
          type: "active",
        }),
        queryClient.refetchQueries({
          queryKey: ["proxyTakeoverStatus"],
          type: "active",
        }),
        queryClient.refetchQueries({
          queryKey: ["providers", "codex"],
          type: "active",
        }),
        queryClient.refetchQueries({ queryKey: usageKeys.all, type: "active" }),
      ]);
      setValidationRefreshMessage(
        "已刷新校验状态，请查看链路卡片和最近转发表。",
      );
    } catch (error) {
      setValidationRefreshMessage(
        `刷新校验失败：${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      setIsRefreshingValidation(false);
    }
  }

  /// 手动同步 Codex JSONL 会话用量，让子 Agent 统计立即看到最新 token_count。
  async function syncCodexSessionUsage() {
    setIsSyncingSessionUsage(true);
    setSessionSyncMessage(null);
    try {
      const result = await usageApi.syncSessionUsage();
      setSessionSyncMessage(
        result.imported > 0
          ? `已同步 ${result.imported} 条会话用量记录`
          : "会话用量已是最新",
      );
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
    } catch (error) {
      setSessionSyncMessage(
        `同步失败：${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      setIsSyncingSessionUsage(false);
    }
  }

  /// 一键诊断只读取本地现场和 router 日志，不向真实上游发起模型请求。
  async function runDiagnostics(nextView: StatusView = "debug") {
    setStatusView(nextView);
    setIsDiagnosing(true);
    setDiagnoseError(null);
    try {
      const result = await proxyApi.diagnoseCodexMultiRouter(
        selectedPlan?.id ?? null,
      );
      setDiagnostics(result);
    } catch (error) {
      setDiagnoseError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsDiagnosing(false);
    }
  }

  /// Codex Desktop 模型菜单还会被 renderer 白名单二次过滤；这里显式触发 CDP 注入/启动修复。
  async function unlockModelPicker() {
    setIsUnlockingModelPicker(true);
    setModelPickerUnlockError(null);
    try {
      const result = await proxyApi.unlockCodexModelPicker();
      setModelPickerUnlockResult(result);
    } catch (error) {
      setModelPickerUnlockError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setIsUnlockingModelPicker(false);
    }
  }

  return (
    <div className="space-y-4">
      <StatusViewSwitcher
        value={statusView}
        diagnostics={diagnostics}
        protocolCount={protocolRows.length}
        trafficCount={
          trafficRows.length + (subagentUsage?.modelStats.length ?? 0)
        }
        providerCount={selectedRoutes.length}
        onChange={setStatusView}
      />

      {statusView === "link" && (
        <section className="rounded-lg border border-border bg-card p-4 dark:border-slate-700 dark:bg-slate-950/40">
          <SectionHeader
            icon={Activity}
            title="链路状态"
            detail="默认先看这里：只有监听、Codex 接管、路由入口和至少一条匹配规则都通过，Codex 请求才会进入 MultiRouter。"
            action={
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={refreshValidationState}
                  disabled={isRefreshingValidation}
                  className="gap-2 border-slate-300 bg-background/70 text-slate-700 hover:bg-slate-50 dark:border-slate-500/50 dark:bg-slate-500/10 dark:text-slate-100 dark:hover:bg-slate-500/20"
                >
                  <RefreshCw
                    className={cn(
                      "h-4 w-4",
                      isRefreshingValidation ? "animate-spin" : "",
                    )}
                  />
                  {isRefreshingValidation ? "刷新中" : "刷新校验"}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => runDiagnostics("debug")}
                  disabled={isDiagnosing}
                  className="gap-2 border-amber-300 bg-background/70 text-amber-700 hover:bg-amber-50 dark:border-amber-500/50 dark:bg-amber-500/10 dark:text-amber-100 dark:hover:bg-amber-500/20"
                >
                  {isDiagnosing ? (
                    <RefreshCw className="h-4 w-4 animate-spin" />
                  ) : (
                    <Bug className="h-4 w-4" />
                  )}
                  Debug 检查
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => runDiagnostics("protocol")}
                  disabled={isDiagnosing}
                  className="gap-2 border-cyan-300 bg-background/70 text-cyan-700 hover:bg-cyan-50 dark:border-cyan-500/50 dark:bg-cyan-500/10 dark:text-cyan-100 dark:hover:bg-cyan-500/20"
                >
                  {isDiagnosing ? (
                    <RefreshCw className="h-4 w-4 animate-spin" />
                  ) : (
                    <GitBranch className="h-4 w-4" />
                  )}
                  协议探测
                </Button>
                <TooltipProvider delayDuration={200}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        className="inline-flex"
                        title={MODEL_PICKER_UNLOCK_TOOLTIP}
                      >
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={unlockModelPicker}
                          disabled={isUnlockingModelPicker}
                          className="gap-2 border-indigo-300 bg-background/70 text-indigo-700 hover:bg-indigo-50 dark:border-indigo-500/50 dark:bg-indigo-500/10 dark:text-indigo-100 dark:hover:bg-indigo-500/20"
                        >
                          {isUnlockingModelPicker ? (
                            <RefreshCw className="h-4 w-4 animate-spin" />
                          ) : (
                            <Wand2 className="h-4 w-4" />
                          )}
                          解锁模型菜单
                        </Button>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent
                      side="bottom"
                      align="end"
                      className="max-w-80 whitespace-normal text-left leading-5"
                    >
                      {MODEL_PICKER_UNLOCK_TOOLTIP}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
                {selectedPlan ? (
                  <>
                    <Button
                      size="sm"
                      onClick={() =>
                        onEditPlan(selectedPlan, "打开多路路由配置")
                      }
                      className="gap-2 bg-blue-600 hover:bg-blue-500"
                    >
                      <Pencil className="h-4 w-4" />
                      编辑配置
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => onDeletePlan(selectedPlan)}
                      className="gap-2 border-rose-300 bg-background/70 text-rose-700 hover:bg-rose-50 dark:border-rose-500/50 dark:bg-rose-500/10 dark:text-rose-100 dark:hover:bg-rose-500/20"
                    >
                      <Trash2 className="h-4 w-4" />
                      删除
                    </Button>
                  </>
                ) : null}
              </div>
            }
          />
          <div className="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-5">
            <StatusCard
              ok={linkOnline}
              label="当前链路"
              value={
                linkOnline ? "在线" : configReady ? "待请求验证" : "未就绪"
              }
              detail={
                linkOnline
                  ? "Codex 请求会进入本地代理并按 model 分流"
                  : configReady
                    ? "配置和监听已就绪，等待当前方案的路由转发成功"
                    : readinessIssues.join("；") || "等待状态刷新"
              }
            />
            <StatusCard
              ok={isProxyRunning}
              label="监听"
              value={isProxyRunning ? "成功" : "未启动"}
              detail={listenAddress}
            />
            <StatusCard
              ok={isCodexTakeoverActive}
              label="Codex 接管"
              value={isCodexTakeoverActive ? "已接管" : "未接管"}
              detail="Codex 请求需要指向本地代理才会进入路由"
            />
            <StatusCard
              ok={Boolean(selectedPlan && routeEnabled)}
              label="路由入口"
              value={
                selectedPlan ? (routeEnabled ? "已启用" : "已关闭") : "未选择"
              }
              detail={selectedPlan?.name ?? "暂无 MultiRouter provider"}
            />
            <StatusCard
              ok={currentRouteForwardOk}
              label="最近转发"
              value={
                currentRouteForwardOk
                  ? "当前方案成功"
                  : latestLog
                    ? latestForwardOk
                      ? `成功 ${latestLog.statusCode}`
                      : `失败 ${latestLog.statusCode}`
                    : "暂无请求"
              }
              detail={
                currentRouteForwardOk
                  ? "已确认当前 MultiRouter route 命中并完成上游转发"
                  : latestLog?.errorMessage ||
                    latestLog?.requestModel ||
                    latestLog?.model ||
                    "等待 Codex 请求"
              }
            />
          </div>
          {validationRefreshMessage ? (
            <div className="mt-3 rounded-lg border border-slate-200 bg-slate-50 p-3 text-xs leading-5 text-slate-700 dark:border-slate-600/50 dark:bg-slate-900/60 dark:text-slate-200">
              {validationRefreshMessage}
            </div>
          ) : null}
          {!modelPickerUnlockResult ? (
            <div className="mt-3 rounded-lg border border-indigo-200 bg-indigo-50 p-3 text-xs leading-5 text-indigo-800 dark:border-indigo-700/50 dark:bg-indigo-950/25 dark:text-indigo-100">
              {MODEL_PICKER_UNLOCK_HINT}
            </div>
          ) : null}
          {modelPickerUnlockError ? (
            <div className="mt-3 rounded-lg border border-rose-200 bg-rose-50 p-3 text-xs leading-5 text-rose-700 dark:border-rose-700/50 dark:bg-rose-950/30 dark:text-rose-100">
              模型菜单解锁失败：{modelPickerUnlockError}
            </div>
          ) : null}
          {modelPickerUnlockResult ? (
            <div
              className={cn(
                "mt-3 rounded-lg border p-3 text-xs leading-5",
                modelPickerUnlockResult.injected
                  ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-700/50 dark:bg-emerald-950/25 dark:text-emerald-100"
                  : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-700/50 dark:bg-amber-950/25 dark:text-amber-100",
              )}
            >
              <div className="font-semibold">
                {modelPickerUnlockResult.injected
                  ? "模型菜单白名单已注入"
                  : "模型菜单白名单尚未注入"}
              </div>
              <div className="mt-1">{modelPickerUnlockResult.message}</div>
              <div className="mt-1 font-mono text-[11px] opacity-80">
                models={modelPickerUnlockResult.modelCount} port=
                {modelPickerUnlockResult.debugPort ?? "-"} launched=
                {String(modelPickerUnlockResult.launched)}
              </div>
              {modelPickerUnlockResult.codexExecutable ? (
                <div className="mt-2 rounded-md border border-current/20 bg-white/40 p-2 text-[11px] leading-5 dark:bg-black/10">
                  Codex Desktop 主程序：
                  <span className="font-mono break-all">
                    {modelPickerUnlockResult.codexExecutable}
                  </span>
                  {!modelPickerUnlockResult.injected
                    ? "。已捕获该 Desktop 路径；请完全退出 Codex Desktop 后再次点击“解锁模型菜单”，让 CCSwitchMulti 用 remote debugging 启动同一个 Desktop。成功注入后，切换第三方 API Key 不需要重复解锁；CLI/app-server 继续使用 live config、model_catalog_json 和本地 /v1/models 路径。"
                    : ""}
                </div>
              ) : null}
            </div>
          ) : null}
          <div className="mt-4 grid gap-3 text-sm md:grid-cols-3">
            <DetailRow label="当前代理目标" value={activeTargetLabel} />
            <DetailRow
              label="启用匹配规则"
              value={`${selectedRoutes.filter(({ route }) => route.enabled !== false).length} / ${selectedRoutes.length}`}
            />
            <DetailRow
              label="代理累计请求"
              value={`${proxyStatus?.total_requests ?? 0} 次，成功率 ${proxyStatus?.success_rate ?? 0}%`}
            />
          </div>
          <div className="mt-3">
            <DetailRow
              label="最近错误"
              value={proxyStatus?.last_error || latestLog?.errorMessage || "无"}
            />
          </div>
        </section>
      )}

      {statusView === "debug" && (
        <DiagnosticsPanel
          diagnostics={diagnostics}
          isLoading={isDiagnosing}
          error={diagnoseError}
          onRun={() => runDiagnostics("debug")}
        />
      )}

      {statusView === "protocol" && (
        <section className="rounded-lg border border-cyan-200 bg-cyan-50/70 p-4 dark:border-cyan-700/40 dark:bg-cyan-950/10">
          <SectionHeader
            icon={GitBranch}
            title="协议探测"
            detail="配置判定来自后端共享协议决策；最近实测来自 request_prepared 日志，能直接看到某个模型最后真实出站走的是 Responses、Chat 还是 Messages。"
            action={
              <Button
                size="sm"
                variant="outline"
                onClick={() => runDiagnostics("protocol")}
                disabled={isDiagnosing}
                className="gap-2 border-cyan-300 bg-background/70 text-cyan-700 hover:bg-cyan-50 dark:border-cyan-500/50 dark:bg-cyan-500/10 dark:text-cyan-100 dark:hover:bg-cyan-500/20"
              >
                {isDiagnosing ? (
                  <RefreshCw className="h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="h-4 w-4" />
                )}
                重新探测
              </Button>
            }
          />
          {!diagnostics && !isDiagnosing ? (
            <div className="mt-3 rounded-lg border border-border bg-muted/40 p-4 text-sm leading-6 text-muted-foreground dark:border-slate-700 dark:bg-slate-950/50 dark:text-slate-300">
              尚未执行协议探测。点击右上角按钮后，会读取当前 MultiRouter route
              规则并结合最近 router
              日志，展示每个模型的配置协议和最近一次真实出站协议。
            </div>
          ) : null}
          {protocolRows.length > 0 ? (
            <div className="mt-3 overflow-hidden rounded-lg border border-border dark:border-slate-700">
              <div className="grid grid-cols-[1.1fr_1fr_1fr_1fr_1.4fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground dark:bg-slate-900/80 dark:text-slate-300">
                <span>Provider / Route</span>
                <span>Model</span>
                <span>配置判定</span>
                <span>最近实测</span>
                <span>来源</span>
              </div>
              {protocolRows.map((row) => (
                <div
                  key={`protocol-${row.providerId}-${row.model}`}
                  className="grid grid-cols-[1.1fr_1fr_1fr_1fr_1.4fr] gap-2 border-t border-border px-3 py-2 text-xs text-foreground dark:border-slate-800 dark:text-slate-300"
                >
                  <div className="min-w-0">
                    <div className="truncate">{row.providerName}</div>
                    <div className="truncate text-[11px] text-muted-foreground dark:text-slate-500">
                      {row.routeLabel || row.routeId || "未命名规则"}
                    </div>
                  </div>
                  <span className="truncate font-mono">{row.model}</span>
                  <div className="min-w-0">
                    <div className="truncate">
                      {row.configuredProtocol
                        ? apiFormatLabel(row.configuredProtocol)
                        : "待探测"}
                    </div>
                    <div
                      className="truncate text-[11px] text-muted-foreground dark:text-slate-500"
                      title={row.configuredProtocolDetail ?? undefined}
                    >
                      {protocolDecisionSourceLabel(
                        row.configuredProtocolSource,
                      )}
                    </div>
                  </div>
                  <div className="min-w-0">
                    <div className="truncate">
                      {row.lastObservedProtocol
                        ? apiFormatLabel(row.lastObservedProtocol)
                        : "暂无实测"}
                    </div>
                    <div className="truncate text-[11px] text-muted-foreground dark:text-slate-500">
                      {row.lastObservedAt ?? "等待新请求"}
                    </div>
                  </div>
                  <div className="min-w-0">
                    <div
                      className="truncate"
                      title={
                        row.lastObservedUpstreamUrl ??
                        row.lastObservedEndpoint ??
                        row.configuredProtocolDetail ??
                        undefined
                      }
                    >
                      {row.lastObservedUpstreamUrl ??
                        row.lastObservedEndpoint ??
                        row.configuredProtocolDetail ??
                        "无"}
                    </div>
                    <div className="truncate text-[11px] text-muted-foreground dark:text-slate-500">
                      最近请求 {row.requestCount} 次，失败 {row.failedCount} 次
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ) : diagnostics ? (
            <div className="mt-3 rounded-lg border border-border bg-muted/40 p-4 text-sm leading-6 text-muted-foreground dark:border-slate-700 dark:bg-slate-950/50 dark:text-slate-300">
              当前没有可归属的协议探测结果。已加载 route {selectedRoutes.length}{" "}
              条， router 事件 {routerRequestEvents.length} 条；请先让 Codex
              发起一次真实请求，再重新探测。
            </div>
          ) : null}
        </section>
      )}

      {statusView === "providers" && (
        <section className="rounded-lg border border-blue-200 bg-blue-50/70 p-4 dark:border-blue-700/40 dark:bg-blue-950/15">
          <SectionHeader
            icon={GitFork}
            title="分流子 Provider"
            detail="这些子 Provider 来自当前 MultiRouter 的 route target，转换层跟随各自供应商配置。"
          />
          <div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {selectedRoutes.map((entry) => {
              const targetProviderId = routeTargetProviderId(entry.route);
              const targetProvider = routeTargetProvider(
                entry.route,
                providersById,
              );
              return (
                <div
                  key={`${entry.provider.id}-${entry.route.id ?? entry.index}`}
                  className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/50"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-foreground dark:text-slate-100">
                        {targetProvider?.name ?? targetProviderId ?? "内联上游"}
                      </div>
                      <div className="mt-1 truncate text-xs text-muted-foreground dark:text-slate-400">
                        {entry.route.label || entry.route.id || "未命名规则"}
                      </div>
                    </div>
                    <Badge
                      className={cn(
                        "border",
                        entry.route.enabled === false
                          ? "border-slate-500/50 bg-slate-500/10 text-slate-200"
                          : "border-emerald-500/50 bg-emerald-500/15 text-emerald-100",
                      )}
                    >
                      {entry.route.enabled === false ? "已停用" : "已启用"}
                    </Badge>
                  </div>
                  <div className="mt-3 text-xs leading-5 text-muted-foreground dark:text-slate-400">
                    {routeMatchSummary(entry.route)}
                  </div>
                </div>
              );
            })}
            {selectedRoutes.length === 0 && (
              <EmptyState
                icon={Route}
                title="还没有分流规则"
                detail="添加 route 后，这里会列出每个子 Provider 和它负责的模型。"
                actionLabel="编辑多路路由"
                onAction={() => selectedPlan && onEditPlan(selectedPlan)}
              />
            )}
          </div>
        </section>
      )}

      {statusView === "traffic" && (
        <div className="space-y-4">
          <section className="rounded-lg border border-emerald-200 bg-emerald-50/70 p-4 dark:border-emerald-700/40 dark:bg-emerald-950/10">
            <SectionHeader
              icon={Database}
              title="今日子 Provider / Model 流量"
              detail="基于真实 Codex 代理请求日志聚合；若后端只记录外层 MultiRouter，页面会按 requestModel 尝试回归属到 route target。"
            />
            <div className="mt-3 overflow-hidden rounded-lg border border-border dark:border-slate-700">
              <div className="grid grid-cols-[1.2fr_1.2fr_0.7fr_0.7fr_0.8fr_0.8fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground dark:bg-slate-900/80 dark:text-slate-300">
                <span>Provider</span>
                <span>Model</span>
                <span className="text-right">请求</span>
                <span className="text-right">失败</span>
                <span className="text-right">Tokens</span>
                <span className="text-right">延迟</span>
              </div>
              {isLoading ? (
                <div className="p-4 text-sm text-muted-foreground">
                  正在读取统计...
                </div>
              ) : trafficRows.length > 0 ? (
                trafficRows.map((row) => (
                  <div
                    key={`${row.providerId}-${row.model}`}
                    className="grid grid-cols-[1.2fr_1.2fr_0.7fr_0.7fr_0.8fr_0.8fr] gap-2 border-t border-border px-3 py-2 text-xs text-foreground dark:border-slate-800 dark:text-slate-300"
                  >
                    <span className="truncate">{row.providerName}</span>
                    <span className="truncate font-mono">{row.model}</span>
                    <span className="text-right">{row.requestCount}</span>
                    <span className="text-right">{row.failedCount}</span>
                    <span className="text-right">
                      {row.totalTokens.toLocaleString()}
                    </span>
                    <span className="text-right">{row.avgLatencyMs}ms</span>
                  </div>
                ))
              ) : (
                <div className="p-4 text-sm leading-6 text-muted-foreground">
                  暂无可归属到子 Provider 的请求日志。今日 Codex 日志{" "}
                  {logs.length} 条，其中真实代理转发 {proxyLogs.length}{" "}
                  条，Codex 会话同步 {sessionLogs.length} 条，外层 MultiRouter
                  日志 {routerLogs.length} 条，目标 Provider 数{" "}
                  {routeTargetCount} 个。
                </div>
              )}
            </div>
            <div className="mt-3 text-xs text-muted-foreground">
              已尝试归属真实代理日志 {routedLogs.length} 条、router 诊断事件{" "}
              {routerRequestEvents.length} 条；这里不把 codex_session
              历史同步当作转发。
            </div>
          </section>

          <section className="rounded-lg border border-violet-200 bg-violet-50/70 p-4 dark:border-violet-700/40 dark:bg-violet-950/10">
            <SectionHeader
              icon={GitFork}
              title="今日子 Agent 会话流量"
              detail="基于 Codex 本地 JSONL/SQLite 的 subagent 会话列表和 token_count 用量；按模型汇总子 Agent 数、请求和 token。"
              action={
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => void syncCodexSessionUsage()}
                  disabled={isSyncingSessionUsage}
                  className="gap-2 border-violet-300 bg-background/70 text-violet-700 hover:bg-violet-100 dark:border-violet-500/50 dark:bg-violet-500/10 dark:text-violet-100 dark:hover:bg-violet-500/20"
                >
                  {isSyncingSessionUsage ? (
                    <RefreshCw className="h-4 w-4 animate-spin" />
                  ) : (
                    <RefreshCw className="h-4 w-4" />
                  )}
                  同步会话用量
                </Button>
              }
            />
            {sessionSyncMessage && (
              <div className="mt-3 rounded-md border border-violet-200 bg-background/70 px-3 py-2 text-xs text-muted-foreground dark:border-violet-700/50 dark:bg-violet-950/30 dark:text-violet-100">
                {sessionSyncMessage}
              </div>
            )}
            {subagentUsageError && (
              <div className="mt-3 rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700 dark:border-rose-700/50 dark:bg-rose-950/30 dark:text-rose-100">
                子 Agent 用量读取失败：
                {subagentUsageError instanceof Error
                  ? subagentUsageError.message
                  : String(subagentUsageError)}
              </div>
            )}
            {subagentUsage?.skippedReason && (
              <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-700/50 dark:bg-amber-950/30 dark:text-amber-100">
                Codex 历史读取跳过：{subagentUsage.skippedReason}
              </div>
            )}

            <div className="mt-3 overflow-hidden rounded-lg border border-border dark:border-slate-700">
              <div className="grid grid-cols-[1.4fr_0.7fr_0.7fr_0.9fr_0.7fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground dark:bg-slate-900/80 dark:text-slate-300">
                <span>模型</span>
                <span className="text-right">子 Agent</span>
                <span className="text-right">请求</span>
                <span className="text-right">Tokens</span>
                <span className="text-right">费用</span>
              </div>
              {isLoadingSubagentUsage ? (
                <div className="p-4 text-sm text-muted-foreground">
                  正在读取子 Agent 统计...
                </div>
              ) : subagentUsage?.modelStats.length ? (
                subagentUsage.modelStats.map((row) => (
                  <div
                    key={row.model}
                    className="grid grid-cols-[1.4fr_0.7fr_0.7fr_0.9fr_0.7fr] gap-2 border-t border-border px-3 py-2 text-xs text-foreground dark:border-slate-800 dark:text-slate-300"
                  >
                    <span className="truncate font-mono">{row.model}</span>
                    <span className="text-right">{row.agentCount}</span>
                    <span className="text-right">{row.requestCount}</span>
                    <span className="text-right">
                      {row.totalTokens.toLocaleString()}
                    </span>
                    <span className="text-right">
                      {formatUsageCost(row.totalCost)}
                    </span>
                  </div>
                ))
              ) : (
                <div className="p-4 text-sm leading-6 text-muted-foreground">
                  暂无子 Agent 会话用量。已读取{" "}
                  {subagentUsage?.totalAgents ?? 0} 个本地子 Agent
                  会话；如果刚刚运行过子 Agent，请先点击“同步会话用量”。
                </div>
              )}
            </div>

            <div className="mt-3 rounded-lg border border-border bg-background/70 px-3 py-2 text-xs leading-6 text-muted-foreground dark:border-slate-700 dark:bg-slate-950/20 dark:text-slate-300">
              已读取 {subagentUsage?.totalAgents ?? 0} 个本地子 Agent 会话，
              归并为 {subagentUsage?.modelStats.length ?? 0}{" "}
              个模型分组。状态库：
              {subagentUsage?.stateDbPath ?? "未定位"}。
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

/// 格式化美元成本，保留小额用量的可见精度。
function formatUsageCost(value?: string): string {
  const parsed = Number.parseFloat(value ?? "");
  if (!Number.isFinite(parsed)) return "$0.000000";
  return `$${parsed.toFixed(parsed > 0 && parsed < 0.01 ? 6 : 4)}`;
}

/// 测试发布页只做本地匹配预览，并展示下一步如何发布到 Codex。
function TestTab({
  selectedPlan,
  selectedRouting,
  routeModels,
  testModel,
  testResult,
  onModelChange,
  onPreviewRoute,
  onEditPlan,
}: {
  selectedPlan: Provider | null;
  selectedRouting: CodexRouting | null;
  routeModels: string[];
  testModel: string;
  testResult: string | null;
  onModelChange: (value: string) => void;
  onPreviewRoute: () => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
}) {
  return (
    <div className="grid gap-4 xl:grid-cols-[1fr_420px]">
      <section className="rounded-lg border border-purple-200 bg-purple-50/70 p-4 dark:border-purple-700/40 dark:bg-purple-950/10">
        <SectionHeader
          icon={Play}
          title="匹配预览"
          detail="输入 Codex 请求中的 model，先在本地预览会命中哪条规则。"
        />
        <div className="mt-4 grid gap-3 md:grid-cols-[1fr_auto]">
          <input
            value={testModel}
            onChange={(event) => onModelChange(event.target.value)}
            placeholder="例如：gpt-5.4-mini、qwen3.6、deepseek-v4-flash"
            className="h-10 rounded-md border border-purple-200 bg-background px-3 text-sm outline-none transition placeholder:text-muted-foreground focus:border-purple-400 focus:ring-2 focus:ring-purple-500/20 dark:border-purple-700/50 dark:bg-slate-950/70 dark:placeholder:text-slate-500 dark:focus:ring-purple-500/30"
          />
          <Button
            onClick={onPreviewRoute}
            className="gap-2 bg-purple-600 hover:bg-purple-500"
          >
            <Play className="h-4 w-4" />
            预览命中
          </Button>
        </div>
        {routeModels.length > 0 && (
          <div className="mt-3 flex flex-wrap gap-2">
            {routeModels.slice(0, 10).map((model) => (
              <button
                key={model}
                type="button"
                onClick={() => onModelChange(model.replace(/\*$/, ""))}
                className="rounded-full border border-purple-200 bg-purple-50 px-3 py-1 text-xs text-purple-800 transition hover:border-purple-300 hover:bg-purple-100 dark:border-purple-500/40 dark:bg-purple-500/10 dark:text-purple-100 dark:hover:bg-purple-500/20"
              >
                {model}
              </button>
            ))}
          </div>
        )}
        <div className="mt-4 rounded-lg border border-purple-200 bg-background/80 p-4 dark:border-purple-700/40 dark:bg-slate-950/50">
          <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
            <Activity className="h-4 w-4 text-purple-600 dark:text-purple-300" />
            预览结果
          </div>
          <p className="text-sm leading-6 text-muted-foreground dark:text-slate-300">
            {testResult ??
              "还没有执行预览。这里不会请求真实上游，也不会消耗额度。"}
          </p>
        </div>
      </section>

      <section className="rounded-lg border border-emerald-200 bg-emerald-50/70 p-4 dark:border-emerald-700/40 dark:bg-emerald-950/10">
        <SectionHeader
          icon={RadioTower}
          title="发布检查"
          detail="确认后再到配置表单保存。"
          action={
            selectedPlan ? (
              <Button
                size="sm"
                onClick={() => onEditPlan(selectedPlan, "打开发布前配置检查")}
                className="gap-2 bg-emerald-600 hover:bg-emerald-500"
              >
                <Pencil className="h-4 w-4" />
                编辑多路路由
              </Button>
            ) : null
          }
        />
        <div className="mt-4 space-y-3">
          <ChecklistItem ok={Boolean(selectedPlan)} label="已选择多路路由" />
          <ChecklistItem
            ok={selectedRouting?.enabled !== false}
            label="多路路由处于启用状态"
          />
          <ChecklistItem
            ok={Boolean(selectedRouting?.defaultRouteId)}
            label="已设置默认路由"
          />
          <ChecklistItem
            ok={(selectedRouting?.routes?.length ?? 0) > 0}
            label="至少有一条路由规则"
          />
          <ChecklistItem ok label="不会切换 Codex 当前 Provider" />
        </div>
      </section>
    </div>
  );
}

/// 状态页内部的分段切换；一次只展开一个信息域，避免 Debug、Provider 和流量表挤在同一屏。
function StatusViewSwitcher({
  value,
  diagnostics,
  protocolCount,
  trafficCount,
  providerCount,
  onChange,
}: {
  value: StatusView;
  diagnostics: CodexMultiRouterDiagnostics | null;
  protocolCount: number;
  trafficCount: number;
  providerCount: number;
  onChange: (value: StatusView) => void;
}) {
  const failedCount =
    diagnostics?.checks.filter((check) => check.status === "fail").length ?? 0;
  const warnCount =
    diagnostics?.checks.filter((check) => check.status === "warn").length ?? 0;
  const debugBadge = diagnostics
    ? failedCount > 0
      ? `${failedCount} 阻塞`
      : warnCount > 0
        ? `${warnCount} 警告`
        : "已检查"
    : "未检查";

  const items: Array<{
    value: StatusView;
    icon: React.ComponentType<{ className?: string }>;
    label: string;
    detail: string;
  }> = [
    {
      value: "link",
      icon: Activity,
      label: "链路",
      detail: "监听 / 接管 / 入口",
    },
    {
      value: "protocol",
      icon: GitBranch,
      label: "协议",
      detail: `${protocolCount} 个模型`,
    },
    {
      value: "debug",
      icon: Bug,
      label: "Debug",
      detail: debugBadge,
    },
    {
      value: "providers",
      icon: GitFork,
      label: "分流",
      detail: `${providerCount} 个目标`,
    },
    {
      value: "traffic",
      icon: Database,
      label: "流量",
      detail: `${trafficCount} 组统计`,
    },
  ];

  return (
    <div className="rounded-lg border border-border bg-card p-2 dark:border-slate-700 dark:bg-slate-950/40">
      <div className="grid gap-2 md:grid-cols-5">
        {items.map((item) => {
          const Icon = item.icon;
          const active = value === item.value;
          return (
            <button
              key={item.value}
              type="button"
              onClick={() => onChange(item.value)}
              className={cn(
                "flex min-w-0 items-center gap-3 rounded-md border px-3 py-2 text-left transition",
                active
                  ? "border-blue-400 bg-blue-50 text-blue-800 dark:border-blue-500/60 dark:bg-blue-600/20 dark:text-blue-100"
                  : "border-border bg-background text-muted-foreground hover:border-blue-300 hover:bg-blue-50 dark:border-slate-700 dark:bg-slate-950/40 dark:text-slate-300 dark:hover:border-blue-500/50 dark:hover:bg-blue-950/20",
              )}
            >
              <Icon className="h-4 w-4 shrink-0" />
              <span className="min-w-0">
                <span className="block truncate text-sm font-semibold">
                  {item.label}
                </span>
                <span className="block truncate text-xs opacity-70">
                  {item.detail}
                </span>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/// MultiRouter Debug 面板展示后端真实检查结果，重点区分“没进入本地路由”和“进入后上游失败”。
function DiagnosticsPanel({
  diagnostics,
  isLoading,
  error,
  onRun,
}: {
  diagnostics: CodexMultiRouterDiagnostics | null;
  isLoading: boolean;
  error: string | null;
  onRun: () => void;
}) {
  const failedChecks =
    diagnostics?.checks.filter((check) => check.status === "fail") ?? [];
  const warningChecks =
    diagnostics?.checks.filter((check) => check.status === "warn") ?? [];
  const visibleCheckCards =
    diagnostics?.checks.filter(
      (check) => check.status !== "fail" && check.status !== "warn",
    ) ?? [];

  return (
    <div className="rounded-lg border border-amber-200 bg-amber-50/70 p-4 dark:border-amber-600/40 dark:bg-amber-950/10">
      <SectionHeader
        icon={Bug}
        title="Debug 检查"
        detail="只检查本机监听、Codex live config、WebSocket 回退、路由规则和 router 日志，不会向真实上游发送模型请求。"
        action={
          <Button
            size="sm"
            variant="outline"
            onClick={onRun}
            disabled={isLoading}
            className="gap-2 border-amber-300 bg-background/70 text-amber-700 hover:bg-amber-50 dark:border-amber-500/50 dark:bg-amber-500/10 dark:text-amber-100 dark:hover:bg-amber-500/20"
          >
            {isLoading ? (
              <RefreshCw className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="h-4 w-4" />
            )}
            重新检查
          </Button>
        }
      />

      {error && (
        <div className="mt-3 rounded-lg border border-rose-200 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-100">
          {error}
        </div>
      )}

      {!diagnostics && !error && (
        <div className="mt-3 rounded-lg border border-border bg-muted/40 p-3 text-sm leading-6 text-muted-foreground dark:border-slate-700 dark:bg-slate-950/50 dark:text-slate-300">
          尚未运行 Debug 检查。点击按钮后会读取真实 Codex live
          配置和本地路由日志，用来确认请求是否进入 MultiRouter。
        </div>
      )}

      {diagnostics && (
        <div className="mt-4 space-y-4">
          <div
            className={cn(
              "rounded-lg border p-3",
              diagnostics.ready
                ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-100"
                : "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-100",
            )}
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <div className="text-sm font-semibold">
                  {diagnostics.ready ? "关键链路通过" : "发现阻塞项"}
                </div>
                <div className="mt-1 text-xs leading-5 opacity-80">
                  {diagnostics.nextAction}
                </div>
              </div>
              <Badge
                className={cn(
                  "border",
                  diagnostics.ready
                    ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/50 dark:bg-emerald-500/15 dark:text-emerald-100"
                    : "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-500/50 dark:bg-rose-500/15 dark:text-rose-100",
                )}
              >
                {diagnostics.generatedAt}
              </Badge>
            </div>
          </div>

          {(failedChecks.length > 0 || warningChecks.length > 0) && (
            <div className="grid gap-3 md:grid-cols-2">
              {failedChecks.length > 0 && (
                <DebugIssueList
                  title="阻塞项"
                  tone="fail"
                  items={diagnostics.blockingIssues}
                />
              )}
              {warningChecks.length > 0 && (
                <DebugIssueList
                  title="警告"
                  tone="warn"
                  items={diagnostics.warnings}
                />
              )}
            </div>
          )}

          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {visibleCheckCards.map((check) => (
              <DiagnosticCheckCard key={check.id} check={check} />
            ))}
          </div>

          <div className="grid gap-3 text-sm xl:grid-cols-4">
            <div className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/50">
              <div className="mb-3 flex items-center gap-2 font-semibold text-foreground dark:text-slate-100">
                <Settings2 className="h-4 w-4 text-blue-600 dark:text-blue-300" />
                Codex Live Config
              </div>
              <div className="space-y-2">
                <DetailRow
                  label="配置文件"
                  value={diagnostics.liveConfig.path}
                />
                <DetailRow
                  label="model_provider"
                  value={diagnostics.liveConfig.modelProvider ?? "未设置"}
                />
                <DetailRow
                  label="active base_url"
                  value={diagnostics.liveConfig.activeBaseUrl ?? "未设置"}
                />
                <DetailRow
                  label="supports_websockets"
                  value={String(diagnostics.liveConfig.supportsWebsockets)}
                />
                <DetailRow
                  label="wire_api"
                  value={diagnostics.liveConfig.wireApi ?? "未设置"}
                />
                <DetailRow
                  label="model_catalog_json"
                  value={diagnostics.liveConfig.modelCatalogJson ?? "未设置"}
                />
                <DetailRow
                  label="catalog 模型数"
                  value={
                    diagnostics.liveConfig.modelCatalogModelCount == null
                      ? "未知"
                      : `${diagnostics.liveConfig.modelCatalogModelCount}`
                  }
                />
                <DetailRow
                  label="config 修改时间"
                  value={diagnostics.liveConfig.configModifiedAt ?? "未知"}
                />
                <DetailRow
                  label="catalog 修改时间"
                  value={
                    diagnostics.liveConfig.modelCatalogModifiedAt ?? "未知"
                  }
                />
              </div>
            </div>

            <div className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/50">
              <div className="mb-3 flex items-center gap-2 font-semibold text-foreground dark:text-slate-100">
                <Server className="h-4 w-4 text-violet-600 dark:text-violet-300" />
                Codex Desktop
              </div>
              <div className="space-y-2">
                <DetailRow
                  label="进程"
                  value={
                    diagnostics.desktopRuntime?.running
                      ? `${diagnostics.desktopRuntime.processCount} 个`
                      : "未检测到"
                  }
                />
                <DetailRow
                  label="app-server"
                  value={
                    diagnostics.desktopRuntime?.appServerRunning
                      ? `${diagnostics.desktopRuntime.appServerCount} 个`
                      : "未检测到"
                  }
                />
                <DetailRow
                  label="最新 app-server 启动"
                  value={
                    diagnostics.desktopRuntime?.newestAppServerStartedAt ??
                    "未知"
                  }
                />
                <DetailRow
                  label="stale catalog"
                  value={
                    diagnostics.desktopRuntime?.mayHaveStaleModelCatalog
                      ? "可能"
                      : "未发现"
                  }
                />
                <DetailRow
                  label="检测错误"
                  value={diagnostics.desktopRuntime?.detectionError ?? "无"}
                />
              </div>
            </div>

            <div className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/50">
              <div className="mb-3 flex items-center gap-2 font-semibold text-foreground dark:text-slate-100">
                <Route className="h-4 w-4 text-emerald-600 dark:text-emerald-300" />
                Route Plan
              </div>
              <div className="space-y-2">
                <DetailRow
                  label="Provider"
                  value={
                    diagnostics.routePlan.providerName ??
                    diagnostics.routePlan.providerId ??
                    "未找到"
                  }
                />
                <DetailRow
                  label="入口状态"
                  value={diagnostics.routePlan.routingEnabled ? "启用" : "关闭"}
                />
                <DetailRow
                  label="启用规则"
                  value={`${diagnostics.routePlan.enabledRouteCount} / ${diagnostics.routePlan.routeCount}`}
                />
                <DetailRow
                  label="默认路由"
                  value={diagnostics.routePlan.defaultRouteId ?? "未设置"}
                />
              </div>
            </div>

            <div className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/50">
              <div className="mb-3 flex items-center gap-2 font-semibold text-foreground dark:text-slate-100">
                <FileClock className="h-4 w-4 text-amber-600 dark:text-amber-300" />
                Router Log
              </div>
              <div className="space-y-2">
                <DetailRow
                  label="日志文件"
                  value={diagnostics.routerLog.exists ? "存在" : "不存在"}
                />
                <DetailRow
                  label="已扫描事件"
                  value={`${diagnostics.routerLog.totalScanned}`}
                />
                <DetailRow
                  label="匹配当前 Router"
                  value={`${diagnostics.routerLog.matchedScanned}`}
                />
                <DetailRow
                  label="最近请求"
                  value={diagnostics.routerLog.latestRequestAt ?? "无"}
                />
                <DetailRow
                  label="最近错误"
                  value={diagnostics.routerLog.latestError ?? "无"}
                />
              </div>
            </div>
          </div>

          {diagnostics.routePlan.routeSummaries.length > 0 && (
            <div className="overflow-hidden rounded-lg border border-border dark:border-slate-700">
              <div className="grid grid-cols-[1fr_1fr_1fr_1fr_0.8fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground dark:bg-slate-900/80 dark:text-slate-300">
                <span>规则</span>
                <span>目标 Provider</span>
                <span>配置协议</span>
                <span>判定来源</span>
                <span>模型</span>
              </div>
              {diagnostics.routePlan.routeSummaries.map((route, index) => (
                <div
                  key={`${route.id ?? index}-${route.targetProviderId ?? "inline"}`}
                  className="grid grid-cols-[1fr_1fr_1fr_1fr_0.8fr] gap-2 border-t border-border px-3 py-2 text-xs text-foreground dark:border-slate-800 dark:text-slate-300"
                >
                  <span className="truncate">
                    {route.label ?? route.id ?? `规则 ${index + 1}`}
                    {route.enabled ? "" : "（停用）"}
                  </span>
                  <span className="truncate">
                    {route.targetProviderName ??
                      route.targetProviderId ??
                      "内联配置"}
                    {route.targetProviderId && !route.targetExists
                      ? "（不存在）"
                      : ""}
                  </span>
                  <span
                    className="truncate"
                    title={route.configuredProtocolDetail ?? undefined}
                  >
                    {route.configuredProtocol
                      ? apiFormatLabel(route.configuredProtocol)
                      : (route.apiFormat ?? "跟随")}
                  </span>
                  <span className="truncate">
                    {protocolDecisionSourceLabel(
                      route.configuredProtocolSource,
                    )}
                  </span>
                  <span className="truncate">
                    {[
                      ...route.models,
                      ...route.prefixes.map((prefix) => `${prefix}*`),
                    ]
                      .slice(0, 3)
                      .join(", ") || "默认"}
                  </span>
                </div>
              ))}
            </div>
          )}

          {diagnostics.routerLog.recentEvents.length > 0 && (
            <div className="overflow-hidden rounded-lg border border-border dark:border-slate-700">
              <div className="grid grid-cols-[1fr_0.9fr_0.8fr_0.9fr_0.6fr_1.6fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground dark:bg-slate-900/80 dark:text-slate-300">
                <span>时间</span>
                <span>事件</span>
                <span>协议</span>
                <span>Provider</span>
                <span>状态</span>
                <span>摘要</span>
              </div>
              {diagnostics.routerLog.recentEvents.slice(0, 12).map((event) => (
                <div
                  key={`${event.timestamp}-${event.event}-${event.line}`}
                  className="grid grid-cols-[1fr_0.9fr_0.8fr_0.9fr_0.6fr_1.6fr] gap-2 border-t border-border px-3 py-2 text-xs text-foreground dark:border-slate-800 dark:text-slate-300"
                >
                  <span className="truncate">{event.timestamp}</span>
                  <span className="truncate font-mono">{event.event}</span>
                  <span className="truncate">
                    {event.actualProtocol
                      ? apiFormatLabel(event.actualProtocol)
                      : "-"}
                  </span>
                  <span className="truncate">
                    {event.outerProvider && event.effectiveProvider
                      ? `${event.outerProvider} -> ${event.effectiveProvider}`
                      : (event.provider ?? "-")}
                  </span>
                  <span className="truncate">{event.status ?? "-"}</span>
                  <span
                    className="truncate"
                    title={event.upstreamUrl ?? event.line}
                  >
                    {event.error ??
                      event.upstreamUrl ??
                      event.model ??
                      event.line}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/// Debug 阻塞项/警告列表，避免用户在检查卡片里逐项翻找最关键结论。
function DebugIssueList({
  title,
  tone,
  items,
}: {
  title: string;
  tone: "fail" | "warn";
  items: string[];
}) {
  return (
    <div
      className={cn(
        "rounded-lg border p-3 text-sm",
        tone === "fail"
          ? "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-100"
          : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100",
      )}
    >
      <div className="mb-2 font-semibold">{title}</div>
      <div className="space-y-1 text-xs leading-5 opacity-85">
        {items.map((item) => (
          <div key={item}>{item}</div>
        ))}
      </div>
    </div>
  );
}

/// 单个 Debug 检查项卡片，展示状态、说明和后端返回的关键证据。
function DiagnosticCheckCard({ check }: { check: CodexDiagnosticCheck }) {
  const meta = diagnosticStatusMeta(check.status);
  const Icon = meta.icon;

  return (
    <div className={cn("rounded-lg border p-3", meta.className)}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">{check.label}</div>
          <div className="mt-1 text-xs leading-5 opacity-80">
            {check.detail}
          </div>
        </div>
        <Icon className="h-4 w-4 shrink-0 opacity-85" />
      </div>
      {check.evidence.length > 0 && (
        <div className="mt-2 space-y-1 font-mono text-[11px] opacity-70">
          {check.evidence.slice(0, 3).map((item) => (
            <div key={item} className="truncate" title={item}>
              {item}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/// 将后端诊断状态映射成 UI 颜色和图标。
function diagnosticStatusMeta(status: CodexDiagnosticStatus): {
  icon: React.ComponentType<{ className?: string }>;
  className: string;
} {
  switch (status) {
    case "pass":
      return {
        icon: CheckCircle2,
        className:
          "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-100",
      };
    case "warn":
      return {
        icon: AlertTriangle,
        className:
          "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100",
      };
    case "fail":
      return {
        icon: XCircle,
        className:
          "border-rose-200 bg-rose-50 text-rose-700 dark:border-rose-500/40 dark:bg-rose-500/10 dark:text-rose-100",
      };
    case "info":
    default:
      return {
        icon: Info,
        className:
          "border-blue-200 bg-blue-50 text-blue-800 dark:border-blue-500/40 dark:bg-blue-500/10 dark:text-blue-100",
      };
  }
}

/// 状态卡用于表达在线/离线这类二值信号，避免用户在长文本里找关键状态。
function StatusCard({
  ok,
  label,
  value,
  detail,
}: {
  ok: boolean;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div
      className={cn(
        "rounded-lg border p-3",
        ok
          ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-100"
          : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs opacity-80">{label}</span>
        <span className="h-2.5 w-2.5 rounded-full bg-current" />
      </div>
      <div className="mt-2 text-lg font-semibold text-foreground dark:text-white">
        {value}
      </div>
      <div className="mt-1 truncate text-xs opacity-75" title={detail}>
        {detail}
      </div>
    </div>
  );
}

/// 通用标题行，统一不同页面区块的操作按钮位置。
function SectionHeader({
  icon: Icon,
  title,
  detail,
  action,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  detail: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2 text-base font-semibold text-foreground dark:text-slate-100">
          <Icon className="h-4 w-4 text-blue-600 dark:text-blue-300" />
          {title}
        </div>
        <p className="mt-1 text-xs leading-5 text-muted-foreground dark:text-slate-400">
          {detail}
        </p>
      </div>
      {action}
    </div>
  );
}

/// 子 Agent 候选排序项封装 dnd-kit 绑定，保持拖拽句柄、删除按钮和模型标签的行为一致。
function SortableSpawnAgentCandidate({
  model,
  index,
  onRemove,
}: {
  model: CodexCatalogModel;
  index: number;
  onRemove: (model: string) => void;
}) {
  const modelId = model.model?.trim() ?? "";
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: modelId });

  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
      }}
      className={cn(
        "flex items-center gap-2 rounded-md border border-violet-200 bg-background/80 px-2 py-2 text-xs dark:border-violet-800/60 dark:bg-slate-950/50",
        isDragging ? "opacity-60 shadow-lg shadow-violet-950/40" : "",
      )}
    >
      <button
        type="button"
        className="grid h-7 w-7 shrink-0 place-items-center rounded border border-violet-200 bg-violet-50 text-violet-700 hover:bg-violet-100 dark:border-violet-700/60 dark:bg-violet-500/10 dark:text-violet-200 dark:hover:bg-violet-500/20"
        {...attributes}
        {...listeners}
        aria-label={`拖动 ${modelId}`}
      >
        <GripVertical className="h-4 w-4" />
      </button>
      <div className="w-8 shrink-0 text-[11px] text-violet-700 dark:text-violet-300">
        #{index + 1}
      </div>
      <div
        className="min-w-0 flex-1 truncate font-mono text-foreground dark:text-slate-100"
        title={catalogModelLabel(model)}
      >
        {catalogModelLabel(model)}
      </div>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        onClick={() => onRemove(modelId)}
        className="h-7 w-7 shrink-0 p-0 text-muted-foreground hover:bg-rose-50 hover:text-rose-700 dark:text-slate-300 dark:hover:bg-rose-500/15 dark:hover:text-rose-100"
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  );
}

/// 路由方案卡片内容；外层决定是按钮还是静态容器。
function PlanCardContent({
  provider,
  compact = false,
}: {
  provider: Provider;
  compact?: boolean;
}) {
  const routing = readCodexRouting(provider);
  const routes = routing?.routes ?? [];

  return (
    <div className="min-w-0">
      <div className="flex flex-wrap items-center gap-2">
        <span className="truncate font-semibold text-foreground dark:text-slate-100">
          {provider.name}
        </span>
        <Badge
          className={cn(
            "border",
            routing?.enabled === false
              ? "border-border bg-muted text-muted-foreground dark:border-slate-500/50 dark:bg-slate-500/10 dark:text-slate-200"
              : "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/50 dark:bg-emerald-500/15 dark:text-emerald-100",
          )}
        >
          {routing?.enabled === false ? "入口已停用" : "入口已启用"}
        </Badge>
      </div>
      <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground dark:text-slate-400">
        <span>规则 {routes.length} 条</span>
        {routing?.defaultRouteId && <span>默认 {routing.defaultRouteId}</span>}
        {!compact && <span>ID {provider.id}</span>}
      </div>
    </div>
  );
}

/// 路由规则按钮，比普通卡片有更明显的 hover 和 active 态。
function RouteListButton({
  entry,
  providersById,
  active = false,
  onClick,
}: {
  entry: RouteEntry;
  providersById: Map<string, Provider>;
  active?: boolean;
  onClick: () => void;
}) {
  const format = routeApiFormat(entry.route);
  const targetProvider = routeTargetProvider(entry.route, providersById);

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        // 路由卡片必须允许收缩到父级网格宽度，否则长模型名会把右侧详情栏挤出并遮挡。
        "group min-w-0 w-full rounded-lg border px-3 py-2 text-left transition",
        active
          ? "border-emerald-400 bg-emerald-50 shadow-[0_0_0_1px_rgba(52,211,153,0.20)] dark:bg-emerald-600/20 dark:shadow-[0_0_0_1px_rgba(52,211,153,0.3)]"
          : "border-border bg-card hover:border-emerald-400 hover:bg-emerald-50 dark:border-slate-700 dark:bg-slate-950/40 dark:hover:bg-emerald-950/20",
      )}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-foreground dark:text-slate-100">
            {entry.route.label || entry.route.id || "未命名规则"}
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground dark:text-slate-400">
            所属多路路由：{entry.provider.name}
          </div>
        </div>
        <Badge
          className={cn(
            "border",
            entry.route.enabled === false
              ? "border-border bg-muted text-muted-foreground dark:border-slate-500/50 dark:bg-slate-500/10 dark:text-slate-200"
              : "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/50 dark:bg-emerald-500/15 dark:text-emerald-100",
          )}
        >
          {entry.route.enabled === false ? "已停用" : "已启用"}
        </Badge>
      </div>
      <div className="mt-2 flex flex-wrap gap-1.5 text-xs">
        <span className="rounded-full border border-blue-200 bg-blue-50 px-2 py-0.5 text-blue-800 dark:border-blue-500/40 dark:bg-blue-500/10 dark:text-blue-100">
          {targetProvider ? "复用供应商配置" : apiFormatLabel(format)}
        </span>
        <span className="rounded-full border border-border bg-muted px-2 py-0.5 text-muted-foreground dark:border-slate-600 dark:bg-slate-900 dark:text-slate-300">
          {targetProvider
            ? `目标：${targetProvider.name}`
            : authSourceLabel(entry.route.upstream?.auth?.source)}
        </span>
      </div>
      <div className="mt-1.5 min-w-0 break-words whitespace-normal text-xs leading-5 text-muted-foreground dark:text-slate-400">
        {routeMatchSummary(entry.route)}
      </div>
    </button>
  );
}

/// 右侧规则详情，把“查看、编辑、删除入口、复制模型名”分开展示，减少不可操作感。
function RouteDetailPanel({
  selectedRoute,
  selectedPlan,
  providersById,
  onOpenRoutePicker,
}: {
  selectedRoute?: RouteEntry;
  selectedPlan: Provider | null;
  providersById: Map<string, Provider>;
  onOpenRoutePicker: (provider?: Provider | null) => void;
}) {
  if (!selectedRoute) {
    return (
      <section className="rounded-lg border border-border bg-card p-3 dark:border-slate-700 dark:bg-slate-950/40">
        <EmptyState
          icon={Route}
          title="请选择一条规则"
          detail="左侧点击规则后，这里会展示上游、匹配条件和操作入口。"
          actionLabel={selectedPlan ? "编辑多路路由" : "创建多路路由"}
          onAction={() => selectedPlan && onOpenRoutePicker(selectedPlan)}
        />
      </section>
    );
  }

  const route = selectedRoute.route;
  const matchedModels = route.match?.models ?? [];
  const targetProviderId = routeTargetProviderId(route);
  const targetProvider = routeTargetProvider(route, providersById);

  return (
    <section className="rounded-lg border border-emerald-200 bg-emerald-50/70 p-3 dark:border-emerald-700/40 dark:bg-slate-950/50">
      <SectionHeader
        icon={Database}
        title={route.label || route.id || "规则详情"}
        detail="这里是当前规则的只读摘要；修改接入范围请打开候选 router 选择器。"
        action={
          <Button
            size="sm"
            onClick={() => onOpenRoutePicker(selectedRoute.provider)}
            className="gap-2 bg-emerald-600 hover:bg-emerald-500"
          >
            <Pencil className="h-4 w-4" />
            编辑
          </Button>
        }
      />
      <div className="mt-3 space-y-2 text-sm">
        <DetailRow label="匹配条件" value={routeMatchSummary(route)} />
        {targetProviderId ? (
          <DetailRow
            label="目标供应商"
            value={
              targetProvider
                ? `${targetProvider.name} (${targetProvider.id})`
                : `未找到目标供应商：${targetProviderId}`
            }
          />
        ) : null}
        <DetailRow
          label="上游地址"
          value={routeBaseUrl(route, providersById)}
        />
        <DetailRow
          label="接口类型"
          value={
            targetProvider
              ? "跟随目标供应商"
              : apiFormatLabel(routeApiFormat(route))
          }
        />
        <DetailRow
          label="认证方式"
          value={
            targetProvider
              ? "跟随目标供应商"
              : authSourceLabel(route.upstream?.auth?.source)
          }
        />
        <DetailRow
          label="能力"
          value={[
            route.capabilities?.textOnly ? "仅文本" : "图文",
            route.capabilities?.supportsReasoning ? "推理" : null,
          ]
            .filter(Boolean)
            .join(" / ")}
        />
      </div>
      <div className="mt-3 grid gap-2">
        <Button
          variant="outline"
          className="justify-start gap-2"
          onClick={() =>
            navigator.clipboard?.writeText(matchedModels.join(", "))
          }
          disabled={matchedModels.length === 0}
        >
          <Clipboard className="h-4 w-4" />
          复制精确模型名
        </Button>
        <Button
          variant="outline"
          className="justify-start gap-2 text-rose-700 hover:bg-rose-50 hover:text-rose-800 dark:text-rose-200 dark:hover:text-rose-100"
          onClick={() => onOpenRoutePicker(selectedRoute.provider)}
        >
          <Trash2 className="h-4 w-4" />
          到候选列表取消勾选
        </Button>
      </div>
    </section>
  );
}

/// 只读详情行，避免信息散落成难扫描的长段落。
function DetailRow({ label, value }: { label: string; value?: string }) {
  return (
    <div className="rounded-md border border-border bg-muted/40 p-3 dark:border-slate-800 dark:bg-slate-950/50">
      <div className="text-xs text-muted-foreground dark:text-slate-500">
        {label}
      </div>
      <div className="mt-1 break-words text-foreground dark:text-slate-200">
        {value || "未配置"}
      </div>
    </div>
  );
}

/// 模型源迷你卡，仅用于总览页快速提示。
function SourceMiniCard({ provider }: { provider: Provider }) {
  return (
    <div className="rounded-lg border border-amber-200 bg-card p-3 dark:border-amber-700/30 dark:bg-slate-950/40">
      <div className="truncate text-sm font-semibold text-foreground dark:text-slate-100">
        {provider.name}
      </div>
      <div className="mt-1 truncate text-xs text-muted-foreground dark:text-slate-400">
        {provider.id}
      </div>
    </div>
  );
}

/// 发布检查项用色彩表达状态，避免所有信息都像普通文字。
function ChecklistItem({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-md border p-3 text-sm",
        ok
          ? "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-500/40 dark:bg-emerald-500/10 dark:text-emerald-100"
          : "border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100",
      )}
    >
      <CheckCircle2 className="h-4 w-4" />
      {label}
    </div>
  );
}

/// 空状态组件带明确动作按钮，让无数据场景仍可继续操作。
function EmptyState({
  icon: Icon,
  title,
  detail,
  actionLabel,
  onAction,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  detail: string;
  actionLabel: string;
  onAction?: () => void;
}) {
  return (
    <div className="rounded-lg border border-dashed border-border bg-muted/40 p-5 dark:border-slate-700 dark:bg-slate-950/40">
      <div className="flex items-start gap-3">
        <Icon className="mt-0.5 h-5 w-5 text-muted-foreground dark:text-slate-400" />
        <div className="min-w-0 flex-1">
          <div className="font-semibold text-foreground dark:text-slate-100">
            {title}
          </div>
          <p className="mt-1 text-sm leading-6 text-muted-foreground dark:text-slate-400">
            {detail}
          </p>
          {onAction && (
            <Button
              size="sm"
              variant="outline"
              onClick={onAction}
              className="mt-3"
            >
              {actionLabel}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
