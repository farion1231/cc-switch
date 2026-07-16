import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import { CopilotAuthSection } from "./CopilotAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
  type CopilotModel,
} from "@/lib/api/copilot";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import {
  hasClaudeOneMMarker,
  stripClaudeOneMMarker,
} from "./hooks/useModelState";
import { cn } from "@/lib/utils";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderCategory,
} from "@/types";

interface EndpointCandidate {
  url: string;
}

/**
 * Codex + GitHub Copilot 模型下拉列表数据源。
 *
 * forwarder.rs 现在会 live 解析请求模型的真实 Copilot vendor：确认为非
 * OpenAI（如 Anthropic Claude，含 `-1m` 扩展上下文变体）时自动把请求转成
 * Chat Completions 发到 Copilot 的 `/chat/completions`；OpenAI vendor（gpt
 * 系列）保持原有的 Responses 透传。因此这里不再按 vendor 过滤——Copilot
 * 账号能选到的模型（含 1M 变体）都展示出来，交给用户在"实际请求模型"里选择，
 * 后端会按 live vendor 自动路由到正确的上游格式。
 *
 * 导出用于单元测试。
 */
export function mapCopilotModelsForCodex(
  models: CopilotModel[],
): FetchedModel[] {
  return models.map((m) => ({
    id: m.id,
    ownedBy: m.vendor ?? null,
  }));
}

/**
 * 计算在 GitHub Copilot 模型下拉里选中某个 model id 时，目录行的"上下文窗口"
 * 是否应该自动回填成该 id 在 live `/models` 里声明的真实值。
 *
 * 只在这一行原本为空时才回填，绝不覆盖用户已手动填写的值；该 id 没有 live
 * contextWindow（未拉取过、或字段缺失）时也不回填，让用户自己判断/手填，
 * 而不是显示一个猜测出来的数字。返回 `undefined` 表示不回填。
 *
 * 导出用于单元测试。
 */
export function resolveCopilotContextWindowAutofill(
  currentContextWindow: string | number | undefined,
  liveContextWindow: number | undefined,
): string | undefined {
  if (currentContextWindow) return undefined;
  if (!liveContextWindow || liveContextWindow <= 0) return undefined;
  return String(liveContextWindow);
}

/**
 * 目录行当前是否已经声明 1M。`[1M]` 仅用于识别并迁移旧配置；新配置统一通过
 * contextWindow 声明，不修改实际请求模型。
 *
 * 导出用于单元测试。
 */
export function isCopilotOneMEnabled(
  model: string,
  contextWindow: string | number | undefined,
): boolean {
  if (hasClaudeOneMMarker(model)) return true;
  const ctx =
    typeof contextWindow === "string"
      ? Number.parseInt(contextWindow, 10)
      : contextWindow;
  return typeof ctx === "number" && Number.isFinite(ctx) && ctx >= 1_000_000;
}

export interface CopilotOneMToggleResult {
  model: string;
  /** `undefined` 表示清空该字段（交给后端默认值），而非置为空字符串。 */
  contextWindow: string | undefined;
}

/**
 * 计算勾选/取消勾选 1M 时目录行应更新成的 model + contextWindow。
 *
 * 对所有 Copilot vendor 使用同一规则：勾选只声明 contextWindow=1M，取消时
 * 尽可能恢复 live contextWindow；实际请求模型 ID 始终保持不变。
 *
 * 导出用于单元测试。
 */
export function resolveCopilotOneMToggle(
  model: string,
  enabled: boolean,
  liveModels: CopilotModel[],
): CopilotOneMToggleResult {
  const baseModel = stripClaudeOneMMarker(model);
  const exact = liveModels.find(
    (m) => m.id.toLowerCase() === baseModel.toLowerCase(),
  );

  if (enabled) {
    return {
      model: baseModel,
      contextWindow: "1000000",
    };
  }

  return {
    model: baseModel,
    contextWindow: exact?.context_window
      ? String(exact.context_window)
      : undefined,
  };
}

interface CodexFormFieldsProps {
  providerId?: string;
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

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // Local proxy User-Agent override
  customUserAgent: string;
  onCustomUserAgentChange: (value: string) => void;
  localProxyHeadersOverride: string;
  onLocalProxyHeadersOverrideChange: (value: string) => void;
  localProxyBodyOverride: string;
  onLocalProxyBodyOverrideChange: (value: string) => void;

  // GitHub Copilot OAuth（托管账号）
  isCopilotPreset?: boolean;
  usesOAuth?: boolean;
  selectedGitHubAccountId?: string | null;
  onGitHubAccountSelect?: (accountId: string | null) => void;
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

export function CodexFormFields({
  providerId,
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
  speedTestEndpoints,
  customUserAgent,
  onCustomUserAgentChange,
  localProxyHeadersOverride,
  onLocalProxyHeadersOverrideChange,
  localProxyBodyOverride,
  onLocalProxyBodyOverrideChange,
  isCopilotPreset,
  usesOAuth,
  selectedGitHubAccountId,
  onGitHubAccountSelect,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  // GitHub Copilot 专用：最近一次 live /models 拉取的原始结果（含每个 id 的
  // 真实 vendor/context_window）。用于选中模型时自动回填目录行的上下文窗口，
  // 以及取消 1M 声明时恢复该 model id 的 live contextWindow。
  const [copilotLiveModels, setCopilotLiveModels] = useState<CopilotModel[]>(
    [],
  );
  const copilotContextWindowById = useMemo(() => {
    const map: Record<string, number> = {};
    for (const m of copilotLiveModels) {
      if (typeof m.context_window === "number" && m.context_window > 0) {
        map[m.id] = m.context_window;
      }
    }
    return map;
  }, [copilotLiveModels]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  // 拉取请求序号：请求身份（Base URL / 完整地址开关 / API Key / 自定义 UA）
  // 一变即自增，清空旧列表并作废在途响应——/models 结果可能按 Key 的模型
  // 授权返回，换号后残留旧列表会误导选择
  const fetchModelsSeqRef = useRef(0);

  useEffect(() => {
    fetchModelsSeqRef.current += 1;
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setCopilotLiveModels((prev) => (prev.length === 0 ? prev : []));
  }, [codexBaseUrl, isFullUrl, codexApiKey, customUserAgent]);
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
  const [advancedExpanded, setAdvancedExpanded] = useState(hasAnyAdvancedValue);

  // 预设/编辑加载填充高级值后自动展开（仅从折叠→展开，不会自动折叠）
  useEffect(() => {
    if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue]);

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
    // GitHub Copilot（托管账号）：用登录账号取模型，无需 API Key。
    // 见文件顶部 mapCopilotModelsForCodex 的说明：展示账号可见的全部模型
    // （含 Claude/-1m 变体），由后端按 live vendor 自动路由。
    if (isCopilotPreset) {
      const seq = ++fetchModelsSeqRef.current;
      setIsFetchingModels(true);
      const fetchModels = selectedGitHubAccountId
        ? copilotGetModelsForAccount(selectedGitHubAccountId)
        : copilotGetModels();
      fetchModels
        .then((models) => {
          if (seq !== fetchModelsSeqRef.current) return;
          const mapped = mapCopilotModelsForCodex(models);
          setFetchedModels(mapped);
          setCopilotLiveModels(models);
          if (mapped.length === 0) {
            toast.info(t("providerForm.fetchModelsEmpty"));
          } else {
            toast.success(
              t("providerForm.fetchModelsSuccess", { count: mapped.length }),
            );
          }
        })
        .catch((err) => {
          if (seq !== fetchModelsSeqRef.current) return;
          console.warn("[Copilot] Failed to fetch models:", err);
          showFetchModelsError(err, t);
        })
        .finally(() => {
          if (seq === fetchModelsSeqRef.current) setIsFetchingModels(false);
        });
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
    t,
    isCopilotPreset,
    selectedGitHubAccountId,
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
      {/* GitHub Copilot OAuth 认证（托管账号，token 由后端动态注入） */}
      {isCopilotPreset && (
        <CopilotAuthSection
          selectedAccountId={selectedGitHubAccountId}
          onAccountSelect={onGitHubAccountSelect}
        />
      )}

      {/* Codex API Key 输入框（非 OAuth 预设时显示） */}
      {!usesOAuth && (
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
      {/* Codex Base URL 输入框 */}
      {shouldShowSpeedTest && (
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
              {isCopilotPreset
                ? t("codexConfig.advancedSectionHintCopilot", {
                    defaultValue:
                      "包含上游格式、模型映射、思考能力与自定义 User-Agent。GitHub Copilot 始终需要本地路由（token 动态注入），任何上游格式都需开启路由接管。",
                  })
                : t("codexConfig.advancedSectionHint", {
                    defaultValue:
                      "包含上游格式、模型映射、思考能力与自定义 User-Agent。使用 Chat Completions 协议的供应商需开启路由接管才能使用。",
                  })}
            </p>
          )}
          <CollapsibleContent className="space-y-3 pt-3">
            {/* 上游格式 —— Chat/Anthropic 需开启路由接管（走代理转换），Responses 原生直连。
                但 Copilot 即使走 Responses 也强制路由（token 动态注入 + 指纹头），
                故 Copilot 下 Responses 标签与说明也标注「需开启路由」。
                沿用 shouldShowSpeedTest 门控，cloud_provider 保持不可切换。 */}
            {shouldShowSpeedTest && (
              <div className="space-y-3">
                {isCopilotPreset ? (
                  <div className="space-y-1.5 rounded-md border border-border/70 bg-muted/20 p-3">
                    <FormLabel>
                      {t("codexConfig.upstreamFormatAutoCopilot", {
                        defaultValue: "按模型自动选择",
                      })}
                    </FormLabel>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.upstreamFormatAutoCopilotHint", {
                        defaultValue:
                          "CC Switch 会根据 GitHub Copilot 返回的模型能力自动选择 Messages、Responses 或 Chat 协议，无需手动配置。",
                      })}
                    </p>
                  </div>
                ) : (
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
                          {isCopilotPreset
                            ? t("codexConfig.upstreamFormatResponsesCopilot", {
                                defaultValue: "Responses（原生，需开启路由）",
                              })
                            : t("codexConfig.upstreamFormatResponses", {
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
                )}

                {!isCopilotPreset && isAnthropicFormat && (
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
                        {isCopilotPreset
                          ? t("codexConfig.catalogColumnContextOrOneM", {
                              defaultValue: "上下文窗口 / 1M",
                            })
                          : t("codexConfig.catalogColumnContext", {
                              defaultValue: "上下文窗口",
                            })}
                      </span>
                      <span />
                    </div>

                    {catalogRows.map((row, index) => {
                      // GitHub Copilot 供应商：这一列始终是 1M 勾选框（与
                      // Claude Code 风格一致），不随行内当前的模型文本切换
                      // 控件类型——即便这一行还没填模型、或填的是 GPT 系列，
                      // 也显示勾选框；勾选只修改 contextWindow，不修改实际
                      // 请求模型。
                      const isCopilotOneMRow = Boolean(isCopilotPreset);
                      const oneMEnabled = isCopilotOneMRow
                        ? isCopilotOneMEnabled(row.model, row.contextWindow)
                        : false;
                      return (
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
                                onSelect={(id) => {
                                  // GitHub Copilot：若该 model id 的真实
                                  // contextWindow 已从 live /models 拉到，且
                                  // 用户还没手填过这一行，自动回填，省得对
                                  // -1m 等扩展上下文变体的真实值靠猜测手填。
                                  const autofillContextWindow =
                                    resolveCopilotContextWindowAutofill(
                                      row.contextWindow,
                                      copilotContextWindowById[id],
                                    );
                                  handleUpdateCatalogRow(index, {
                                    model: id,
                                    displayName: row.displayName?.trim()
                                      ? row.displayName
                                      : id,
                                    ...(autofillContextWindow !== undefined
                                      ? { contextWindow: autofillContextWindow }
                                      : null),
                                  });
                                }}
                              />
                            )}
                          </div>
                          {isCopilotOneMRow ? (
                            <div className="flex h-9 items-center gap-2">
                              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                                <Checkbox
                                  checked={oneMEnabled}
                                  onCheckedChange={(checked) => {
                                    const result = resolveCopilotOneMToggle(
                                      row.model,
                                      checked === true,
                                      copilotLiveModels,
                                    );
                                    handleUpdateCatalogRow(index, {
                                      model: result.model,
                                      contextWindow: result.contextWindow,
                                    });
                                  }}
                                  aria-label={t("codexConfig.modelOneMLabel", {
                                    defaultValue: "1M",
                                  })}
                                />
                                {t("codexConfig.modelOneMLabel", {
                                  defaultValue: "1M",
                                })}
                              </label>
                            </div>
                          ) : (
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
                              aria-label={t(
                                "codexConfig.catalogColumnContext",
                                {
                                  defaultValue: "上下文窗口",
                                },
                              )}
                            />
                          )}
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
                      );
                    })}
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
          appId="codex"
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
