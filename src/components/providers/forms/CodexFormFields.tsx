import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import {
  ApiKeySection,
  EndpointField,
  ModelDropdown,
  ModelInputWithFetch,
} from "./shared";
import { XaiOAuthSection } from "./XaiOAuthSection";
import {
  fetchCodexOfficialModels,
  fetchModelsForConfig,
  fetchXaiOauthModels,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import { cn } from "@/lib/utils";
import { extractCodexModelName } from "@/utils/providerConfigUtils";
import { CODEX_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  CodexCustomModel,
  PromptCacheRoutingMode,
  ProviderCategory,
  Provider,
} from "@/types";
import type { AppId } from "@/lib/api";

interface EndpointCandidate {
  url: string;
}

interface CodexFormFieldsProps {
  appId?: AppId;
  providerId?: string;
  // xAI OAuth 托管预设（Grok 订阅）：隐藏 API Key / 端点输入，挂账号选择区块
  isXaiOauthPreset?: boolean;
  isXaiOauthAuthenticated?: boolean;
  selectedXaiAccountId?: string | null;
  onXaiAccountSelect?: (accountId: string | null) => void;
  // API Key
  codexApiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  shouldShowSpeedTest: boolean;
  codexBaseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isFullUrl: boolean;
  onFullUrlChange: (value: boolean) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Default model (config.toml top-level `model`)
  codexModel?: string;
  onModelChange?: (model: string) => void;

  // API Format
  // Note: wire_api is always "responses" for Codex; apiFormat controls proxy-layer conversion
  apiFormat: CodexApiFormat;
  onApiFormatChange: (format: CodexApiFormat) => void;
  // Auth field for the Anthropic Messages upstream (only used when apiFormat === "anthropic")
  anthropicAuthField: ClaudeApiKeyField;
  onAnthropicAuthFieldChange: (value: ClaudeApiKeyField) => void;
  // Anthropic path: whether to emulate the Claude Code client
  impersonateClaudeCode: boolean;
  onImpersonateClaudeCodeChange: (value: boolean) => void;
  // Anthropic path: output ceiling override (empty string = use default). Digits only.
  maxOutputTokens: string;
  onMaxOutputTokensChange: (value: string) => void;
  codexChatReasoning?: CodexChatReasoning;
  onCodexChatReasoningChange?: (value: CodexChatReasoning) => void;
  promptCacheRouting: PromptCacheRoutingMode;
  onPromptCacheRoutingChange: (value: PromptCacheRoutingMode) => void;

  // Model Catalog
  catalogModels?: CodexCatalogModel[];
  onCatalogModelsChange?: (models: CodexCatalogModel[]) => void;

  // 官方 Codex 供应商的自定义模型（对外 ID -> 绑定供应商）
  codexCustomModels?: CodexCustomModel[];
  onCodexCustomModelsChange?: (models: CodexCustomModel[]) => void;
  // 可绑定的 Codex 供应商列表（下拉选择目标供应商）
  codexProviders?: Provider[];
  // 官方 Codex：是否启用官方登录（关闭 = 聚合模式，多供应商模型切换）
  enableOfficialLogin?: boolean;
  onEnableOfficialLoginChange?: (value: boolean) => void;
  // 官方 Codex：模型聚合总开关。关闭时隐藏登录选择与自定义模型配置，
  // 官方供应商按普通官方登录处理。
  codexAggregationEnabled?: boolean;
  onCodexAggregationEnabledChange?: (value: boolean) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // Local proxy User-Agent override
  customUserAgent: string;
  onCustomUserAgentChange: (value: string) => void;
  localProxyHeadersOverride: string;
  onLocalProxyHeadersOverrideChange: (value: string) => void;
  localProxyBodyOverride: string;
  onLocalProxyBodyOverrideChange: (value: string) => void;
}

type CodexCatalogRow = CodexCatalogModel & { rowId: string };

function createCatalogRow(seed?: Partial<CodexCatalogModel>): CodexCatalogRow {
  return {
    rowId: crypto.randomUUID(),
    model: seed?.model ?? "",
    displayName: seed?.displayName ?? "",
    contextWindow: seed?.contextWindow ?? "",
    // Carry native-profile overrides verbatim (not user-editable in the row UI,
    // but must survive load->save so the official catalog fidelity is kept).
    ...(seed?.supportsParallelToolCalls !== undefined
      ? { supportsParallelToolCalls: seed.supportsParallelToolCalls }
      : {}),
    ...(seed?.inputModalities ? { inputModalities: seed.inputModalities } : {}),
    ...(seed?.baseInstructions
      ? { baseInstructions: seed.baseInstructions }
      : {}),
  };
}

// Compares rows (with rowId) to incoming models (without) by data fields only,
// so both sync effects can use the same equality definition. Hidden native-profile
// fields are included so switching between providers with identical visible fields
// but different base_instructions / tools / modalities still rebuilds the rows.
function catalogRowsMatchModels(
  rows: CodexCatalogModel[],
  models: CodexCatalogModel[],
): boolean {
  if (rows.length !== models.length) return false;
  return rows.every((row, i) => {
    const incoming = models[i];
    return (
      row.model === (incoming.model ?? "") &&
      (row.displayName ?? "") === (incoming.displayName ?? "") &&
      String(row.contextWindow ?? "") ===
        String(incoming.contextWindow ?? "") &&
      (row.supportsParallelToolCalls ?? null) ===
        (incoming.supportsParallelToolCalls ?? null) &&
      (row.baseInstructions ?? "") === (incoming.baseInstructions ?? "") &&
      JSON.stringify(row.inputModalities ?? []) ===
        JSON.stringify(incoming.inputModalities ?? [])
    );
  });
}

type CodexCustomModelRow = CodexCustomModel & { rowId: string };

// Codex 桌面端只认官方模型名（gpt-5.x 等）：自定义模型必须用这些名字
// 作为「模型名称」展示，实际请求再映射回目标供应商的上游模型。
// 这是动态获取官方模型失败时的回退快照，需随官方模型上下线手动维护。
const CODEX_OFFICIAL_MODEL_NAMES = [
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "gpt-5.5",
  "gpt-5.4-mini",
  "gpt-5.2",
  "codex-auto-review",
] as const;

/// “清除插槽”选项的哨兵值：选中后把该行 model 清空。
const CODEX_SLOT_CLEAR_VALUE = "__codex_slot_clear__";

function createCustomModelRow(
  seed?: Partial<CodexCustomModel>,
): CodexCustomModelRow {
  return {
    rowId: crypto.randomUUID(),
    model: seed?.model ?? "",
    providerId: seed?.providerId ?? "",
    upstreamModel: seed?.upstreamModel ?? "",
    displayName: seed?.displayName ?? "",
    contextWindow: seed?.contextWindow ?? "",
    ...(seed?.supportsParallelToolCalls !== undefined
      ? { supportsParallelToolCalls: seed.supportsParallelToolCalls }
      : {}),
    ...(seed?.inputModalities ? { inputModalities: seed.inputModalities } : {}),
    ...(seed?.baseInstructions
      ? { baseInstructions: seed.baseInstructions }
      : {}),
  };
}

export function customRowsMatchModels(
  rows: CodexCustomModel[],
  models: CodexCustomModel[],
): boolean {
  if (rows.length !== models.length) return false;
  return rows.every((row, i) => {
    const incoming = models[i];
    return (
      row.model === (incoming.model ?? "") &&
      row.providerId === (incoming.providerId ?? "") &&
      (row.upstreamModel ?? "") === (incoming.upstreamModel ?? "") &&
      (row.displayName ?? "") === (incoming.displayName ?? "") &&
      String(row.contextWindow ?? "") ===
        String(incoming.contextWindow ?? "") &&
      (row.supportsParallelToolCalls ?? null) ===
        (incoming.supportsParallelToolCalls ?? null) &&
      (row.baseInstructions ?? "") === (incoming.baseInstructions ?? "") &&
      JSON.stringify(row.inputModalities ?? []) ===
        JSON.stringify(incoming.inputModalities ?? [])
    );
  });
}

export function CodexFormFields({
  appId = "codex",
  providerId,
  isXaiOauthPreset,
  isXaiOauthAuthenticated,
  selectedXaiAccountId,
  onXaiAccountSelect,
  codexApiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  codexBaseUrl,
  onBaseUrlChange,
  isFullUrl,
  onFullUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  codexModel = "",
  onModelChange,
  apiFormat,
  onApiFormatChange,
  anthropicAuthField,
  onAnthropicAuthFieldChange,
  impersonateClaudeCode,
  onImpersonateClaudeCodeChange,
  maxOutputTokens,
  onMaxOutputTokensChange,
  codexChatReasoning = {},
  onCodexChatReasoningChange,
  promptCacheRouting,
  onPromptCacheRoutingChange,
  catalogModels = [],
  onCatalogModelsChange,
  codexCustomModels = [],
  onCodexCustomModelsChange,
  codexProviders = [],
  enableOfficialLogin = true,
  onEnableOfficialLoginChange,
  codexAggregationEnabled = true,
  onCodexAggregationEnabledChange,
  speedTestEndpoints,
  customUserAgent,
  onCustomUserAgentChange,
  localProxyHeadersOverride,
  onLocalProxyHeadersOverrideChange,
  localProxyBodyOverride,
  onLocalProxyBodyOverrideChange,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  // 拉取请求序号：请求身份（Base URL / 完整地址开关 / API Key / 自定义 UA）
  // 一变即自增，清空旧列表并作废在途响应——/models 结果可能按 Key 的模型
  // 授权返回，换号后残留旧列表会误导选择
  const fetchModelsSeqRef = useRef(0);

  useEffect(() => {
    fetchModelsSeqRef.current += 1;
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
  }, [
    codexBaseUrl,
    isFullUrl,
    codexApiKey,
    customUserAgent,
    isXaiOauthPreset,
    isXaiOauthAuthenticated,
    selectedXaiAccountId,
  ]);
  // 思考能力随 Chat 格式显示（仅 Chat Completions 转换路径用得上）；模型映射常驻
  //（填了才生成 catalog）。两者都已与「路由接管」概念解耦。
  const isChatFormat = apiFormat === "openai_chat";
  const isAnthropicFormat = apiFormat === "anthropic";
  const canEditCatalog = Boolean(onCatalogModelsChange);
  const canEditReasoning = Boolean(onCodexChatReasoningChange);
  const supportsThinking =
    codexChatReasoning.supportsThinking === true ||
    codexChatReasoning.supportsEffort === true;
  const supportsEffort = codexChatReasoning.supportsEffort === true;

  // 高级区在有任何可见配置时自动展开（仅折叠→展开，不会自动折叠）：自定义 UA /
  // 请求覆盖 / 已填模型映射 / 原生 Responses（需维护 catalog）/ 已配置思考能力。
  const hasRequestOverrides = Boolean(
    localProxyHeadersOverride.trim() || localProxyBodyOverride.trim(),
  );
  const hasAnyAdvancedValue =
    !!customUserAgent ||
    hasRequestOverrides ||
    catalogModels.length > 0 ||
    apiFormat === "openai_responses" ||
    isAnthropicFormat ||
    supportsThinking ||
    supportsEffort ||
    promptCacheRouting !== "auto" ||
    !!maxOutputTokens;
  const [advancedExpanded, setAdvancedExpanded] = useState(
    isXaiOauthPreset ? false : hasAnyAdvancedValue,
  );

  // 预设/编辑加载填充高级值后自动展开（仅从折叠→展开，不会自动折叠）；
  // xAI OAuth 托管预设的高级值都是预设自带的，无需展示，保持折叠
  useEffect(() => {
    if (isXaiOauthPreset) {
      return;
    }
    if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue, isXaiOauthPreset]);

  // 官方 Codex 自定义模型的插槽下拉：优先从官方动态获取当前模型，失败（未登录/
  // 请求异常）时回退到内置快照列表，避免绑定已下线的插槽导致模型在 Codex 里不显示。
  const [officialModelSlugs, setOfficialModelSlugs] = useState<string[]>([]);
  useEffect(() => {
    if (category !== "official") {
      return;
    }
    let cancelled = false;
    fetchCodexOfficialModels()
      .then((slugs) => {
        if (!cancelled && slugs.length > 0) {
          setOfficialModelSlugs(slugs);
        }
      })
      .catch(() => {
        // 保持空数组，回退到 CODEX_OFFICIAL_MODEL_NAMES
      });
    return () => {
      cancelled = true;
    };
  }, [category]);

  const [catalogRows, setCatalogRows] = useState<CodexCatalogRow[]>(() =>
    catalogModels.map((m) => createCatalogRow(m)),
  );

  // 记录上次发送给父组件的数据，避免重复触发
  const lastSentModelsRef = useRef<CodexCatalogModel[]>(catalogModels);

  // 父 → 子：仅当 prop 数据真的变化（预设切换 / 编辑加载）时才重建 rowId；
  // 同 shape 时保留现有 rowId，避免编辑过程中焦点丢失。
  useEffect(() => {
    setCatalogRows((current) => {
      if (catalogRowsMatchModels(current, catalogModels)) return current;
      return catalogModels.map((m) => createCatalogRow(m));
    });
    // 同步更新 ref，避免父组件传入新数据时子→父 effect 误判为本地修改
    lastSentModelsRef.current = catalogModels;
  }, [catalogModels]);

  // 子 → 父：rowId 是视图层概念，不应进入持久化数据；剥离后再回传。
  // 注意：依赖数组不包含 catalogModels，避免父→子更新触发子→父回调形成循环。
  useEffect(() => {
    if (!onCatalogModelsChange) return;
    const next: CodexCatalogModel[] = catalogRows.map(
      ({ rowId: _rowId, ...rest }) => rest,
    );
    // 只有当数据真的变化时才通知父组件
    if (catalogRowsMatchModels(catalogRows, lastSentModelsRef.current)) return;
    lastSentModelsRef.current = next;
    onCatalogModelsChange(next);
  }, [catalogRows, onCatalogModelsChange]);

  // ---- 官方 Codex 自定义模型行（对外 ID -> 绑定供应商） ----
  const [customRows, setCustomRows] = useState<CodexCustomModelRow[]>(() =>
    codexCustomModels.map((m) => createCustomModelRow(m)),
  );

  const lastSentCustomModelsRef = useRef<CodexCustomModel[]>(codexCustomModels);

  useEffect(() => {
    setCustomRows((current) => {
      if (customRowsMatchModels(current, codexCustomModels)) return current;
      return codexCustomModels.map((m) => createCustomModelRow(m));
    });
    lastSentCustomModelsRef.current = codexCustomModels;
  }, [codexCustomModels]);

  useEffect(() => {
    if (!onCodexCustomModelsChange) return;
    const next: CodexCustomModel[] = customRows.map(
      ({ rowId: _rowId, ...rest }) => rest,
    );
    if (customRowsMatchModels(customRows, lastSentCustomModelsRef.current))
      return;
    lastSentCustomModelsRef.current = next;
    onCodexCustomModelsChange(next);
  }, [customRows, onCodexCustomModelsChange]);

  const handleAddCustomModelRow = useCallback(() => {
    if (!onCodexCustomModelsChange) return;
    setCustomRows((current) => [...current, createCustomModelRow()]);
  }, [onCodexCustomModelsChange]);

  const handleRemoveCustomModelRow = useCallback((index: number) => {
    setCustomRows((current) => current.filter((_, i) => i !== index));
  }, []);

  // ---- 自定义模型：从目标供应商的模型目录自动带出模型/上下文 ----
  const providerCatalogModels = useCallback(
    (providerId: string): CodexCatalogModel[] => {
      const provider = codexProviders.find((p) => p.id === providerId);
      const catalog = (provider?.settingsConfig as any)?.modelCatalog?.models;
      return Array.isArray(catalog)
        ? catalog.filter(
            (m: CodexCatalogModel) =>
              typeof m.model === "string" && m.model.trim(),
          )
        : [];
    },
    [codexProviders],
  );

  // 实际请求模型下拉选项：供应商已配置的模型目录（与目标供应商列同款 Select）
  const customCatalogByProvider = useMemo(() => {
    const map: Record<string, CodexCatalogModel[]> = {};
    for (const provider of codexProviders) {
      const models = providerCatalogModels(provider.id);
      if (models.length === 0) continue;
      map[provider.id] = models;
    }
    return map;
  }, [codexProviders, providerCatalogModels]);

  // 选定供应商后：带出默认上游模型，并尽量自动填显示名/上下文窗口。
  // 模型名称（Codex 展示用）保持用户选择不变。
  const handleCustomProviderChange = useCallback(
    (index: number, providerId: string) => {
      const provider = codexProviders.find((p) => p.id === providerId);
      const models = providerCatalogModels(providerId);
      const defaultUpstreamModel =
        extractCodexModelName(
          typeof provider?.settingsConfig?.config === "string"
            ? provider.settingsConfig.config
            : "",
        ) ||
        models[0]?.model ||
        "";
      const match = models.find((m) => m.model === defaultUpstreamModel);
      setCustomRows((current) =>
        current.map((row, i) =>
          i === index
            ? {
                ...row,
                providerId,
                upstreamModel: defaultUpstreamModel,
                // 菜单显示名始终读目标供应商目录中该模型的显示名
                displayName: match?.displayName?.trim() || "",
                // 上下文窗口只读继承自供应商模型目录，不由用户手填
                contextWindow: match?.contextWindow ?? "",
              }
            : row,
        ),
      );
    },
    [codexProviders, providerCatalogModels],
  );

  // 实际请求模型（上游）变化：命中目录时自动补显示名/上下文（仅当用户没手填）
  const handleCustomModelChange = useCallback(
    (index: number, model: string) => {
      setCustomRows((current) => {
        const row = current[index];
        const match = providerCatalogModels(row.providerId).find(
          (m) => m.model === model,
        );
        return current.map((r, i) =>
          i === index
            ? {
                ...r,
                upstreamModel: model,
                // 命中目录时自动补显示名/上下文；目录外（自由输入）保留旧值，
                // 避免把已派生的显示名/上下文清空。
                displayName: match
                  ? match.displayName?.trim() || r.displayName || ""
                  : r.displayName,
                contextWindow: match
                  ? match.contextWindow ?? r.contextWindow ?? ""
                  : r.contextWindow,
              }
            : r,
        );
      });
    },
    [providerCatalogModels],
  );

  // 模型名称（Codex 展示用）变化：只改对外 ID，不影响上游模型
  const handleCustomDisplayModelChange = useCallback(
    (index: number, model: string) => {
      setCustomRows((current) =>
        current.map((r, i) => (i === index ? { ...r, model } : r)),
      );
    },
    [],
  );

  const handleReasoningThinkingChange = useCallback(
    (checked: boolean) => {
      if (!onCodexChatReasoningChange) return;
      onCodexChatReasoningChange({
        ...codexChatReasoning,
        supportsThinking: checked,
        supportsEffort: checked ? codexChatReasoning.supportsEffort : false,
      });
    },
    [codexChatReasoning, onCodexChatReasoningChange],
  );

  const handleReasoningEffortChange = useCallback(
    (checked: boolean) => {
      if (!onCodexChatReasoningChange) return;
      onCodexChatReasoningChange({
        ...codexChatReasoning,
        supportsThinking: checked ? true : codexChatReasoning.supportsThinking,
        supportsEffort: checked,
        effortParam: checked
          ? (codexChatReasoning.effortParam ?? "reasoning_effort")
          : "none",
      });
    },
    [codexChatReasoning, onCodexChatReasoningChange],
  );

  const handleFetchModels = useCallback(() => {
    // xAI OAuth 托管预设：不走 base_url + key 的 /models 探测，
    // 直接用托管账号 token 拉取（与 Claude 表单同一后端命令）
    if (isXaiOauthPreset) {
      if (!isXaiOauthAuthenticated) {
        toast.error(
          t("xaiOauth.loginRequired", {
            defaultValue: "请先登录 xAI 账号",
          }),
        );
        return;
      }
      const seq = ++fetchModelsSeqRef.current;
      setIsFetchingModels(true);
      fetchXaiOauthModels(selectedXaiAccountId ?? null)
        .then((models) => {
          if (seq !== fetchModelsSeqRef.current) return;
          setFetchedModels(models);
          if (models.length === 0) {
            toast.info(t("providerForm.fetchModelsEmpty"));
          } else {
            toast.success(
              t("providerForm.fetchModelsSuccess", { count: models.length }),
            );
          }
        })
        .catch((err) => {
          if (seq !== fetchModelsSeqRef.current) return;
          console.warn("[XaiOAuth] Failed to fetch models:", err);
          showFetchModelsError(err, t);
        })
        .finally(() => setIsFetchingModels(false));
      return;
    }

    if (!codexBaseUrl || !codexApiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!codexApiKey,
        hasBaseUrl: !!codexBaseUrl,
      });
      return;
    }
    const seq = ++fetchModelsSeqRef.current;
    setIsFetchingModels(true);
    fetchModelsForConfig(
      codexBaseUrl,
      codexApiKey,
      isFullUrl,
      undefined,
      customUserAgent,
    )
      .then((models) => {
        if (seq !== fetchModelsSeqRef.current) return;
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        if (seq !== fetchModelsSeqRef.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [
    codexBaseUrl,
    codexApiKey,
    isFullUrl,
    customUserAgent,
    isXaiOauthPreset,
    isXaiOauthAuthenticated,
    selectedXaiAccountId,
    t,
  ]);

  const handleAddCatalogRow = useCallback(() => {
    if (!onCatalogModelsChange) return;
    setCatalogRows((current) => [...current, createCatalogRow()]);
  }, [onCatalogModelsChange]);

  const handleUpdateCatalogRow = useCallback(
    (index: number, patch: Partial<CodexCatalogModel>) => {
      setCatalogRows((current) =>
        current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
      );
    },
    [],
  );

  const handleRemoveCatalogRow = useCallback((index: number) => {
    setCatalogRows((current) => current.filter((_, i) => i !== index));
  }, []);

  // 默认模型下拉建议 = 模型映射的"实际请求模型"列 ∪ 拉取到的 /models 列表
  const defaultModelSuggestions = useMemo<FetchedModel[]>(() => {
    const seen = new Set<string>();
    const suggestions: FetchedModel[] = [];
    for (const row of catalogRows) {
      const id = row.model.trim();
      if (!id || seen.has(id)) continue;
      seen.add(id);
      suggestions.push({
        id,
        ownedBy: t("codexConfig.modelMappingTitle", {
          defaultValue: "模型映射",
        }),
      });
    }
    for (const model of fetchedModels) {
      if (seen.has(model.id)) continue;
      seen.add(model.id);
      suggestions.push(model);
    }
    return suggestions;
  }, [catalogRows, fetchedModels, t]);

  // 填了映射时才提示"默认模型不在映射中"（无映射的供应商本来就直接请求任意模型名）
  const trimmedDefaultModel = codexModel.trim();
  const isDefaultModelOutsideCatalog =
    catalogRows.length > 0 &&
    !!trimmedDefaultModel &&
    !catalogRows.some((row) => row.model.trim() === trimmedDefaultModel);

  const handleAddDefaultModelToCatalog = useCallback(() => {
    if (!onCatalogModelsChange || !trimmedDefaultModel) return;
    setCatalogRows((current) => [
      ...current,
      createCatalogRow({
        model: trimmedDefaultModel,
        displayName: trimmedDefaultModel,
      }),
    ]);
  }, [onCatalogModelsChange, trimmedDefaultModel]);

  const renderCatalogActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={handleFetchModels}
        disabled={isFetchingModels}
        className="h-7 gap-1"
      >
        {isFetchingModels ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Download className="h-3.5 w-3.5" />
        )}
        {t("providerForm.fetchModels")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onAdd}
        className="h-7 gap-1"
      >
        <Plus className="h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <>
      {/* xAI OAuth 认证（Grok 订阅托管账号） */}
      {isXaiOauthPreset && (
        <XaiOAuthSection
          selectedAccountId={selectedXaiAccountId}
          onAccountSelect={onXaiAccountSelect}
        />
      )}

      {/* Codex API Key 输入框（托管 OAuth 预设无需 Key） */}
      {!isXaiOauthPreset && (
        <ApiKeySection
          id="codexApiKey"
          label="API Key"
          value={codexApiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
          placeholder={{
            official: t("providerForm.codexOfficialNoApiKey", {
              defaultValue: "官方供应商无需 API Key",
            }),
            thirdParty: t("providerForm.codexApiKeyAutoFill", {
              defaultValue: "输入 API Key，将自动填充到配置",
            }),
          }}
        />
      )}

      {/* Codex Base URL 输入框（托管 OAuth 端点由 adapter 硬定向，不展示） */}
      {shouldShowSpeedTest && !isXaiOauthPreset && (
        <EndpointField
          id="codexBaseUrl"
          label={t("codexConfig.apiUrlLabel")}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.codexApiEndpointPlaceholder")}
          hint={t("providerForm.codexApiHint")}
          showFullUrlToggle
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* 默认模型 —— config.toml 顶层 model，Codex 启动时默认请求的模型。
          实时写回 TOML；留空则删行（有映射时保存回退为映射第一行）。 */}
      {category !== "official" && onModelChange && (
        <div className="space-y-1.5">
          <FormLabel htmlFor="codexDefaultModel">
            {t("codexConfig.defaultModelLabel", { defaultValue: "默认模型" })}
          </FormLabel>
          <div className="flex gap-1">
            <Input
              id="codexDefaultModel"
              value={codexModel}
              onChange={(event) => onModelChange(event.target.value)}
              placeholder={t("codexConfig.defaultModelPlaceholder", {
                defaultValue: "例如: gpt-5.6",
              })}
              className="flex-1"
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={handleFetchModels}
              disabled={isFetchingModels}
              className="shrink-0"
              title={t("providerForm.fetchModels")}
            >
              {isFetchingModels ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Download className="h-4 w-4" />
              )}
            </Button>
            {defaultModelSuggestions.length > 0 && (
              <ModelDropdown
                models={defaultModelSuggestions}
                onSelect={(id) => onModelChange(id)}
              />
            )}
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t("codexConfig.defaultModelHint", {
              defaultValue:
                "Codex 默认请求的模型，随时可改，无需等待预设更新。留空且配置了模型映射时，默认使用映射第一行。",
            })}
          </p>
          {isDefaultModelOutsideCatalog && (
            <p className="flex flex-wrap items-center gap-x-2 text-xs leading-relaxed text-muted-foreground">
              {t("codexConfig.defaultModelNotInCatalog", {
                defaultValue:
                  "该模型不在模型映射中，Codex 的 /model 菜单不会列出它（直接请求仍然有效）。",
              })}
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0 text-xs"
                onClick={handleAddDefaultModelToCatalog}
              >
                {t("codexConfig.addToModelMapping", {
                  defaultValue: "加入映射",
                })}
              </Button>
            </p>
          )}
        </div>
      )}

      {/* 官方 Codex 自定义模型：把额外模型路由到 cc-switch 绑定的供应商 */}
      {category === "official" && onCodexCustomModelsChange && (
        <div className="space-y-4">
          {/* 模型聚合总开关：关闭时隐藏登录选择与自定义模型配置 */}
          <div className="flex items-center justify-between gap-4">
            <div className="space-y-1">
              <FormLabel>
                {t("codexConfig.aggregationLabel", {
                  defaultValue: "模型聚合",
                })}
              </FormLabel>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {t("codexConfig.aggregationHint", {
                  defaultValue:
                    "开启后可将其他供应商的模型聚合进 Codex（需先开启本地路由/代理才生效）。关闭时按普通官方模型登录处理。",
                })}
              </p>
            </div>
            <Switch
              checked={codexAggregationEnabled}
              onCheckedChange={(value) =>
                onCodexAggregationEnabledChange?.(value)
              }
              aria-label={t("codexConfig.aggregationLabel", {
                defaultValue: "模型聚合",
              })}
            />
          </div>

          {codexAggregationEnabled && (
            <>
              <div className="flex items-center justify-between gap-4">
                <div className="space-y-1">
                  <FormLabel>
                    {t("codexConfig.enableOfficialLoginLabel", {
                      defaultValue: "启用官方登录",
                    })}
                  </FormLabel>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {enableOfficialLogin
                      ? t("codexConfig.enableOfficialLoginHintOn", {
                          defaultValue:
                            "使用 ChatGPT 账号登录官方模型；下方自定义模型仍走各自供应商的凭据。",
                        })
                      : t("codexConfig.enableOfficialLoginHintOff", {
                          defaultValue:
                            "无需登录：Codex 的模型全部来自下方配置的多个供应商，请求按模型路由到对应供应商。",
                        })}
                  </p>
                </div>
                <Switch
                  checked={enableOfficialLogin}
                  onCheckedChange={(value) =>
                    onEnableOfficialLoginChange?.(value)
                  }
                  aria-label={t("codexConfig.enableOfficialLoginLabel", {
                    defaultValue: "启用官方登录",
                  })}
                />
              </div>

              <div className="space-y-1">
                <div className="flex items-center justify-between gap-3">
                  <FormLabel>
                    {t("codexConfig.customModelsTitle", {
                      defaultValue: "自定义模型（官方登录）",
                    })}
                  </FormLabel>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleAddCustomModelRow}
                    className="h-7 gap-1"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t("codexConfig.addCustomModel", {
                      defaultValue: "添加自定义模型",
                    })}
                  </Button>
                </div>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {enableOfficialLogin
                    ? t("codexConfig.customModelsHint", {
                        defaultValue:
                          "配置后 Codex 会在模型列表里看到这些模型；选择它们时，本地代理会把请求转发到你绑定的供应商（保留官方登录，仅支持 OpenAI Responses 协议）。",
                      })
                    : t("codexConfig.customModelsHintAggregate", {
                        defaultValue:
                          "Codex 桌面端只认官方模型名：为每个模型选一个 Codex 插槽名（如 gpt-5.2），实际请求会映射到你绑定的供应商的上游模型。",
                      })}
                </p>
              </div>

              {customRows.length > 0 && (
                <div className="space-y-2">
                  <div className="hidden grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_110px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                    <span>
                      {t("codexConfig.customColumnProvider", {
                        defaultValue: "目标供应商",
                      })}
                    </span>
                    <span>
                      {t("codexConfig.customColumnModelName", {
                        defaultValue: "Codex 插槽",
                      })}
                    </span>
                    <span>
                      {t("codexConfig.catalogColumnDisplay", {
                        defaultValue: "菜单显示名",
                      })}
                    </span>
                    <span>
                      {t("codexConfig.catalogColumnModel", {
                        defaultValue: "实际请求模型",
                      })}
                    </span>
                    <span>
                      {t("codexConfig.catalogColumnContext", {
                        defaultValue: "上下文窗口",
                      })}
                    </span>
                    <span />
                  </div>

                  {customRows.map((row, index) => (
                    <div
                      key={row.rowId}
                      className="grid grid-cols-1 gap-2 md:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_110px_36px]"
                    >
                      <Select
                        value={row.providerId}
                        onValueChange={(value) =>
                          handleCustomProviderChange(index, value)
                        }
                      >
                        <SelectTrigger
                          className="min-w-0 select-fade-value"
                          aria-label={t("codexConfig.customColumnProvider", {
                            defaultValue: "目标供应商",
                          })}
                        >
                          <SelectValue
                            placeholder={t(
                              "codexConfig.customProviderPlaceholder",
                              {
                                defaultValue: "选择供应商",
                              },
                            )}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          {codexProviders
                            // 聚合模式（官方登录关闭）下官方供应商无自有凭据，
                            // 路由到它会拿 Bearer PROXY_MANAGED 被官方校验拒绝，
                            // 从目标选择器排除，避免保存出必坏的映射。
                            .filter(
                              (provider) =>
                                !(
                                  !enableOfficialLogin &&
                                  provider.id === CODEX_OFFICIAL_PROVIDER_ID
                                ),
                            )
                            .map((provider) => (
                              <SelectItem key={provider.id} value={provider.id}>
                                {provider.name}
                              </SelectItem>
                            ))}
                        </SelectContent>
                      </Select>
                      <Select
                        value={row.model}
                        onValueChange={(value) =>
                          handleCustomDisplayModelChange(
                            index,
                            value === CODEX_SLOT_CLEAR_VALUE ? "" : value,
                          )
                        }
                      >
                        <SelectTrigger
                          className="min-w-0 select-fade-value"
                          aria-label={t("codexConfig.customColumnModelName", {
                            defaultValue: "Codex 插槽",
                          })}
                          title={t("codexConfig.customModelNameHint", {
                            defaultValue:
                              "Codex 只显示官方模型名。这里选一个 Codex 插槽名（如 gpt-5.2）让该模型出现在列表里；实际请求走目标供应商的上游模型。",
                          })}
                        >
                          <SelectValue
                            placeholder={t(
                              "codexConfig.customModelNamePlaceholder",
                              {
                                defaultValue: "选一个 Codex 插槽（如 gpt-5.2）",
                              },
                            )}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={CODEX_SLOT_CLEAR_VALUE}>
                            {t("codexConfig.customSlotClear", {
                              defaultValue: "清除插槽",
                            })}
                          </SelectItem>
                          {(() => {
                            // 插槽基础列表：优先官方动态获取，失败回退内置快照。
                            const slotBase =
                              officialModelSlugs.length > 0
                                ? officialModelSlugs
                                : CODEX_OFFICIAL_MODEL_NAMES;
                            // 已被其他行选中的插槽从本行下拉中过滤掉，避免两个模型
                            // 占用同一插槽（models_cache 按 slug 去重，只会显示一个）。
                            // 当前行已选的值始终保留，避免值不在选项里。
                            const usedElsewhere = new Set(
                              customRows
                                .filter((_r, i2) => i2 !== index)
                                .map((r) => r.model)
                                .filter(Boolean),
                            );
                            const slotOptions: string[] = slotBase.filter(
                              (m) => !usedElsewhere.has(m),
                            );
                            if (row.model && !slotOptions.includes(row.model)) {
                              slotOptions.push(row.model);
                            }
                            return slotOptions;
                          })().map((model) => (
                            <SelectItem key={model} value={model}>
                              {model}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <div
                        className="flex h-9 min-w-0 items-center rounded-md border border-transparent px-3 text-sm text-muted-foreground"
                        aria-label={t("codexConfig.catalogColumnDisplay", {
                          defaultValue: "菜单显示名",
                        })}
                        title={t("codexConfig.customDisplayNameReadonlyHint", {
                          defaultValue: "显示名自动取自目标供应商的模型目录",
                        })}
                      >
                        <span className="flex-1 text-fade-out">
                          {row.displayName?.trim() || row.model || "—"}
                        </span>
                      </div>
                      <ModelInputWithFetch
                        id={`custom-upstream-${index}`}
                        value={row.upstreamModel ?? row.model}
                        onChange={(value) => handleCustomModelChange(index, value)}
                        placeholder={t("codexConfig.catalogColumnModel", {
                          defaultValue: "实际请求模型",
                        })}
                        ariaLabel={t("codexConfig.catalogColumnModel", {
                          defaultValue: "实际请求模型",
                        })}
                        fetchedModels={(customCatalogByProvider[row.providerId] ?? []).map(
                          (model) => ({
                            id: model.model,
                            ownedBy:
                              codexProviders.find(
                                (p) => p.id === row.providerId,
                              )?.name ?? "Catalog",
                          }),
                        )}
                        isLoading={false}
                        disabled={!row.providerId}
                      />
                      <div
                        className="flex h-9 min-w-0 items-center rounded-md border border-transparent px-3 text-sm text-muted-foreground"
                        aria-label={t("codexConfig.catalogColumnContext", {
                          defaultValue: "上下文窗口",
                        })}
                        title={t("codexConfig.customContextReadonlyHint", {
                          defaultValue:
                            "上下文窗口继承自目标供应商的模型目录，不可编辑",
                        })}
                      >
                        <span className="flex-1 text-fade-out">
                          {row.contextWindow
                            ? `${row.contextWindow}`
                            : t("codexConfig.catalogContextPlaceholder", {
                                defaultValue: "默认 128k",
                              })}
                        </span>
                      </div>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        onClick={() => handleRemoveCustomModelRow(index)}
                        className="h-8 w-8 text-muted-foreground hover:text-destructive"
                        aria-label={t("codexConfig.removeCustomModel", {
                          defaultValue: "删除",
                        })}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      )}

      {/* 高级选项 —— 上游格式/模型映射/思考能力/自定义 UA；预设供应商通常无需展开 */}
      {category !== "official" && (
        <Collapsible
          open={advancedExpanded}
          onOpenChange={setAdvancedExpanded}
          className="rounded-lg border border-border-default p-4"
        >
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
            >
              {advancedExpanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("providerForm.advancedOptionsToggle", {
                defaultValue: "高级选项",
              })}
            </Button>
          </CollapsibleTrigger>
          {!advancedExpanded && (
            <p className="mt-1 ml-1 text-xs text-muted-foreground">
              {t("codexConfig.advancedSectionHint", {
                defaultValue:
                  "包含上游格式、模型映射、思考能力与自定义 User-Agent。使用 Chat Completions 协议的供应商需开启路由接管才能使用。",
              })}
            </p>
          )}
          <CollapsibleContent className="space-y-3 pt-3">
            {/* 上游格式 —— Chat 需开启路由接管（走代理转换），Responses 原生直连。
                沿用 shouldShowSpeedTest 门控，cloud_provider 保持不可切换；
                xAI OAuth 托管预设格式钉死 Responses，不可切换。 */}
            {shouldShowSpeedTest && !isXaiOauthPreset && (
              <div className="space-y-3">
                <div className="space-y-1.5">
                  <FormLabel htmlFor="codex-upstream-format">
                    {t("codexConfig.upstreamFormatLabel", {
                      defaultValue: "上游格式",
                    })}
                  </FormLabel>
                  <Select
                    value={apiFormat}
                    onValueChange={(value) =>
                      onApiFormatChange(value as CodexApiFormat)
                    }
                  >
                    <SelectTrigger
                      id="codex-upstream-format"
                      className="w-full"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="openai_chat">
                        {t("codexConfig.upstreamFormatChat", {
                          defaultValue: "Chat Completions（需开启路由）",
                        })}
                      </SelectItem>
                      <SelectItem value="openai_responses">
                        {t("codexConfig.upstreamFormatResponses", {
                          defaultValue: "Responses（原生）",
                        })}
                      </SelectItem>
                      <SelectItem value="anthropic">
                        {t("codexConfig.upstreamFormatAnthropic", {
                          defaultValue: "Anthropic Messages（需开启路由）",
                        })}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.upstreamFormatHint", {
                      defaultValue:
                        "供应商原生是 Responses API 就选 Responses（直连，不转换格式）；使用 Chat Completions 协议就选 Chat；供应商只提供原生 Anthropic Messages 协议就选 Anthropic Messages。Chat 与 Anthropic Messages 均需开启路由接管才能转换为 Responses。",
                    })}
                  </p>
                </div>

                {isAnthropicFormat && (
                  <div className="space-y-1.5">
                    <FormLabel htmlFor="codex-anthropic-auth-field">
                      {t("codexConfig.anthropicAuthFieldLabel", {
                        defaultValue: "认证字段",
                      })}
                    </FormLabel>
                    <Select
                      value={anthropicAuthField}
                      onValueChange={(value) =>
                        onAnthropicAuthFieldChange(value as ClaudeApiKeyField)
                      }
                    >
                      <SelectTrigger
                        id="codex-anthropic-auth-field"
                        className="w-full"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                          {t("codexConfig.anthropicAuthFieldAuthToken", {
                            defaultValue:
                              "ANTHROPIC_AUTH_TOKEN（Authorization）",
                          })}
                        </SelectItem>
                        <SelectItem value="ANTHROPIC_API_KEY">
                          {t("codexConfig.anthropicAuthFieldApiKey", {
                            defaultValue: "ANTHROPIC_API_KEY（x-api-key）",
                          })}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.anthropicAuthFieldHint", {
                        defaultValue:
                          "选择网关接收 API Key 的请求头：ANTHROPIC_AUTH_TOKEN 发送 Authorization: Bearer；ANTHROPIC_API_KEY 发送 x-api-key。两者只发其一。",
                      })}
                    </p>
                  </div>
                )}

                {isAnthropicFormat && (
                  <div className="flex items-center justify-between gap-4 border-t border-border-default pt-3">
                    <div className="space-y-1">
                      <FormLabel>
                        {t("codexConfig.impersonateClaudeCodeLabel", {
                          defaultValue: "模拟 Claude Code 客户端",
                        })}
                      </FormLabel>
                      <p className="text-xs leading-relaxed text-muted-foreground">
                        {t("codexConfig.impersonateClaudeCodeHint", {
                          defaultValue:
                            "网关或其上游限制只能通过 Claude Code 使用时开启：伪装 User-Agent、anthropic-beta、x-app 请求头，并在系统提示首行注入 Claude Code 身份。",
                        })}
                      </p>
                    </div>
                    <Switch
                      checked={impersonateClaudeCode}
                      onCheckedChange={onImpersonateClaudeCodeChange}
                      aria-label={t("codexConfig.impersonateClaudeCodeLabel", {
                        defaultValue: "模拟 Claude Code 客户端",
                      })}
                    />
                  </div>
                )}

                {isAnthropicFormat && (
                  <div className="space-y-1.5 border-t border-border-default pt-3">
                    <FormLabel htmlFor="codex-anthropic-max-output-tokens">
                      {t("codexConfig.maxOutputTokensLabel", {
                        defaultValue: "最大输出 tokens",
                      })}
                    </FormLabel>
                    <Input
                      id="codex-anthropic-max-output-tokens"
                      type="number"
                      min={1}
                      inputMode="numeric"
                      value={maxOutputTokens}
                      onChange={(event) =>
                        onMaxOutputTokensChange(
                          event.target.value.replace(/[^\d]/g, ""),
                        )
                      }
                      placeholder={t("codexConfig.maxOutputTokensPlaceholder", {
                        defaultValue: "留空则使用默认 8192",
                      })}
                    />
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.maxOutputTokensHint", {
                        defaultValue:
                          "Codex 不会把 model_max_output_tokens 写进请求体，默认上限 8192 容易在长回答或深度思考时被截断（stop_reason=max_tokens）。此处设置会作为 Anthropic 的 max_tokens 覆盖请求值。请勿超过该模型/网关的真实输出上限，否则可能 400。留空使用默认 8192。",
                      })}
                    </p>
                  </div>
                )}
              </div>
            )}

            {isChatFormat && canEditReasoning && (
              <div
                className={cn(
                  "space-y-3",
                  shouldShowSpeedTest && "border-t border-border-default pt-3",
                )}
              >
                <div className="space-y-2">
                  <FormLabel>
                    {t("codexConfig.promptCacheRoutingLabel", {
                      defaultValue: "提示词缓存路由",
                    })}
                  </FormLabel>
                  <Select
                    value={promptCacheRouting}
                    onValueChange={(value) =>
                      onPromptCacheRoutingChange(
                        value as PromptCacheRoutingMode,
                      )
                    }
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">
                        {t("codexConfig.promptCacheRoutingAuto", {
                          defaultValue: "自动（推荐）",
                        })}
                      </SelectItem>
                      <SelectItem value="enabled">
                        {t("codexConfig.promptCacheRoutingEnabled", {
                          defaultValue: "开启",
                        })}
                      </SelectItem>
                      <SelectItem value="disabled">
                        {t("codexConfig.promptCacheRoutingDisabled", {
                          defaultValue: "关闭",
                        })}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.promptCacheRoutingHint", {
                      defaultValue:
                        "自动模式仅对已确认兼容的上游发送 prompt_cache_key；开启可用于其他兼容网关，关闭可避免严格网关因未知字段返回 400。只使用客户端提供的稳定会话 ID。",
                    })}
                  </p>
                </div>

                <div className="space-y-1">
                  <FormLabel>
                    {t("codexConfig.reasoningGroupTitle", {
                      defaultValue: "思考能力",
                    })}
                  </FormLabel>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.reasoningSectionHint", {
                      defaultValue:
                        "预设供应商已自动配置；自定义供应商会按名称/地址自动推断。仅当自动识别不准时才需手动覆盖。",
                    })}
                  </p>
                </div>

                <div className="flex items-center justify-between gap-4">
                  <div className="space-y-1">
                    <FormLabel>
                      {t("codexConfig.reasoningModeToggle", {
                        defaultValue: "支持思考模式",
                      })}
                    </FormLabel>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.reasoningModeHint", {
                        defaultValue:
                          "上游 Chat Completions 接口支持开启或关闭 thinking 时启用。Kimi、GLM、Qwen 等通常属于这一类。",
                      })}
                    </p>
                  </div>
                  <Switch
                    checked={supportsThinking}
                    onCheckedChange={handleReasoningThinkingChange}
                    aria-label={t("codexConfig.reasoningModeToggle", {
                      defaultValue: "支持思考模式",
                    })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 border-t border-border-default pt-3">
                  <div className="space-y-1">
                    <FormLabel>
                      {t("codexConfig.reasoningEffortToggle", {
                        defaultValue: "支持思考等级",
                      })}
                    </FormLabel>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.reasoningEffortHint", {
                        defaultValue:
                          "上游支持 low/high/max 等思考深度控制时启用。启用后会自动启用思考模式，并把 Codex 的 reasoning.effort 转成上游 Chat 参数。",
                      })}
                    </p>
                  </div>
                  <Switch
                    checked={supportsEffort}
                    onCheckedChange={handleReasoningEffortChange}
                    aria-label={t("codexConfig.reasoningEffortToggle", {
                      defaultValue: "支持思考等级",
                    })}
                  />
                </div>
              </div>
            )}

            {/* 模型映射 / 模型目录 —— 与「路由接管」解耦，常驻显示（可编辑即渲染）。
                填了才生成 catalog：Chat 模式生成兼容路由、原生 Responses 生成
                model-catalogs.json；留空则不生成。排在自定义 UA 之前。 */}
            {canEditCatalog && (
              <div
                className={cn(
                  "space-y-4",
                  (shouldShowSpeedTest || (isChatFormat && canEditReasoning)) &&
                    "border-t border-border-default pt-3",
                )}
              >
                <div className="space-y-1">
                  <div className="flex items-center justify-between gap-3">
                    <FormLabel>
                      {t("codexConfig.modelMappingTitle", {
                        defaultValue: "模型映射",
                      })}
                    </FormLabel>
                    {renderCatalogActionButtons(
                      handleAddCatalogRow,
                      t("codexConfig.addCatalogModel", {
                        defaultValue: "添加模型",
                      }),
                    )}
                  </div>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.modelMappingHint", {
                      defaultValue:
                        "选择模型角色后，CC Switch 会自动生成 Codex 兼容路由；菜单显示名可以填 DeepSeek、Kimi 等品牌模型，实际请求模型按右侧填写内容发送。",
                    })}
                  </p>
                </div>

                {catalogRows.length > 0 && (
                  <div className="space-y-2">
                    {/* 列头：md+ 显示 */}
                    <div className="hidden grid-cols-[1fr_1fr_140px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                      <span>
                        {t("codexConfig.catalogColumnDisplay", {
                          defaultValue: "菜单显示名",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogColumnModel", {
                          defaultValue: "实际请求模型",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogColumnContext", {
                          defaultValue: "上下文窗口",
                        })}
                      </span>
                      <span />
                    </div>

                    {catalogRows.map((row, index) => (
                      <div
                        key={row.rowId}
                        className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_1fr_140px_36px]"
                      >
                        <Input
                          value={row.displayName ?? ""}
                          onChange={(event) =>
                            handleUpdateCatalogRow(index, {
                              displayName: event.target.value,
                            })
                          }
                          placeholder={t(
                            "codexConfig.catalogDisplayNamePlaceholder",
                            {
                              defaultValue: "例如: DeepSeek V4 Flash",
                            },
                          )}
                          aria-label={t("codexConfig.catalogColumnDisplay", {
                            defaultValue: "菜单显示名",
                          })}
                        />
                        <div className="flex gap-1">
                          <Input
                            value={row.model}
                            onChange={(event) =>
                              handleUpdateCatalogRow(index, {
                                model: event.target.value,
                              })
                            }
                            placeholder={t(
                              "codexConfig.catalogModelPlaceholder",
                              {
                                defaultValue: "例如: deepseek-v4-flash",
                              },
                            )}
                            aria-label={t("codexConfig.catalogColumnModel", {
                              defaultValue: "实际请求模型",
                            })}
                            className="flex-1"
                          />
                          {fetchedModels.length > 0 && (
                            <ModelDropdown
                              models={fetchedModels}
                              onSelect={(id) =>
                                handleUpdateCatalogRow(index, {
                                  model: id,
                                  displayName: row.displayName?.trim()
                                    ? row.displayName
                                    : id,
                                })
                              }
                            />
                          )}
                        </div>
                        <Input
                          type="number"
                          min={1}
                          inputMode="numeric"
                          value={row.contextWindow ?? ""}
                          onChange={(event) =>
                            handleUpdateCatalogRow(index, {
                              contextWindow: event.target.value.replace(
                                /[^\d]/g,
                                "",
                              ),
                            })
                          }
                          placeholder={t(
                            "codexConfig.contextWindowPlaceholder",
                            {
                              defaultValue: "例如: 128000",
                            },
                          )}
                          aria-label={t("codexConfig.catalogColumnContext", {
                            defaultValue: "上下文窗口",
                          })}
                        />
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-9 w-9 text-muted-foreground hover:text-destructive"
                          onClick={() => handleRemoveCatalogRow(index)}
                          title={t("common.delete", { defaultValue: "删除" })}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            <div
              className={cn(
                "space-y-3",
                (shouldShowSpeedTest ||
                  (isChatFormat && canEditReasoning) ||
                  canEditCatalog) &&
                  "border-t border-border-default pt-3",
              )}
            >
              <CustomUserAgentField
                id="codex-custom-user-agent"
                value={customUserAgent}
                onChange={onCustomUserAgentChange}
              />
              <div className="border-t border-border-default pt-3">
                <LocalProxyRequestOverridesField
                  headersJson={localProxyHeadersOverride}
                  bodyJson={localProxyBodyOverride}
                  onHeadersJsonChange={onLocalProxyHeadersOverrideChange}
                  onBodyJsonChange={onLocalProxyBodyOverrideChange}
                />
              </div>
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}

      {/* 端点测速弹窗 - Codex */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId={appId}
          providerId={providerId}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}
    </>
  );
}
