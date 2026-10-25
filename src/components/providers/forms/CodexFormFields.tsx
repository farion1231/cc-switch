import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { FormLabel } from "@/components/ui/form";
import { fieldClass, Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "@/lib/toast";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { CodexOAuthSection } from "./CodexOAuthSection";
import { CopilotAuthSection } from "./CopilotAuthSection";
import {
  ApiKeySection,
  DirectModeFormatWarning,
  EndpointField,
  FullUrlToggle,
  GptContextWindowHint,
  ModelDropdown,
  type DirectModeFormatNotice,
} from "./shared";
import { XaiOAuthSection } from "./XaiOAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
  type CopilotModel,
} from "@/lib/api/copilot";
import {
  fetchModelsForConfig,
  fetchXaiOauthModels,
  modelFetchRequestHeaders,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import { cn } from "@/lib/utils";
import { useCommittableRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import { codexPresetModelSources } from "@/config/presetModelMetadata";
import {
  fillCodexCatalogModel,
  metadataFilledAnything,
} from "./modelMetadataFill";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexCopilotApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderCategory,
} from "@/types";
import type { ManagedAuthProvider } from "@/lib/api";
import type { AppId } from "@/lib/api";

interface EndpointCandidate {
  url: string;
}

export function isCopilotModelSupportedByCodex(
  model: CopilotModel,
  format: CodexCopilotApiFormat = "auto",
): boolean {
  const endpoints =
    format === "openai_responses"
      ? ["/responses", "/v1/responses"]
      : format === "openai_chat"
        ? ["/chat/completions", "/v1/chat/completions"]
        : [
            "/responses",
            "/v1/responses",
            "/chat/completions",
            "/v1/chat/completions",
          ];
  return (model.supported_endpoints ?? []).some((endpoint) =>
    endpoints.includes(
      endpoint.split("?")[0].replace(/\/+$/, "").toLowerCase(),
    ),
  );
}

export function resolveCopilotReportedPromptLimit(
  current: CodexCatalogModel["contextWindow"],
  reported: number | undefined,
): CodexCatalogModel["contextWindow"] {
  return reported ?? current;
}

interface CodexFormFieldsProps {
  appId?: AppId;
  providerId?: string;
  isCopilotPreset?: boolean;
  isCopilotAuthenticated?: boolean;
  selectedGitHubAccountId?: string | null;
  onGitHubAccountSelect?: (accountId: string | null) => void;
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
  isCodexOauthPreset?: boolean;
  selectedCodexAccountId?: string | null;
  onCodexAccountSelect?: (accountId: string | null) => void;
  onCodexAuthSelectionConfirmed?: () => void;
  onCodexAuthSelectionInvalidated?: () => void;
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;
  codexOauthSelectionLabel?: string;
  codexOauthNoneOptionLabel?: string;
  codexOauthNoneOptionDescription?: string;
  codexOauthAllowUnboundSelection?: boolean;
  codexOauthAllowUnboundSelectionWithoutStatus?: boolean;
  codexOauthNativeLoginOnly?: boolean;
  codexOauthRequireExplicitSelection?: boolean;

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

  // Subagent defaults (config.toml `[agents]`). Codex only: Grok Build has no
  // such keys, and the Stack layout omits them (subagents follow the main model).
  subagentModel?: string;
  onSubagentModelChange?: (model: string) => void;
  subagentReasoningEffort?: string;
  onSubagentReasoningEffortChange?: (effort: string) => void;

  // API Format
  // Note: wire_api is always "responses" for Codex; apiFormat controls proxy-layer conversion
  apiFormat: CodexApiFormat;
  onApiFormatChange: (format: CodexApiFormat) => void;
  copilotApiFormat?: CodexCopilotApiFormat;
  onCopilotApiFormatChange?: (format: CodexCopilotApiFormat) => void;
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
  // Auto Review (approval reviewer) model: `modelCatalog.autoReviewModel`, one of
  // the catalog rows. Codex only; the Stack layout omits it (aggregation settings apply).
  autoReviewModel?: string;
  onAutoReviewModelChange?: (model: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // Local proxy User-Agent override
  customUserAgent: string;
  onCustomUserAgentChange: (value: string) => void;
  localProxyHeadersOverride: string;
  onLocalProxyHeadersOverrideChange: (value: string) => void;
  localProxyBodyOverride: string;
  onLocalProxyBodyOverrideChange: (value: string) => void;

  /**
   * 布局：`classic` 是直连 / 路由用的完整表单；`stack` 是 Stack 模式的简化面板（连接 +
   * 模型列表 + 高级），没有默认模型字段，列表第一行就是默认模型。
   */
  variant?: "classic" | "stack";
  /** 应用实际在直连时传：选了要靠路由转换的格式就在格式下面提示后果。 */
  directModeFormatNotice?: DirectModeFormatNotice;
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
    ...(seed?.reasoningLevels && seed.reasoningLevels.length > 0
      ? { reasoningLevels: seed.reasoningLevels }
      : {}),
    ...(seed?.defaultReasoningLevel
      ? { defaultReasoningLevel: seed.defaultReasoningLevel }
      : {}),
  };
}

// 有模型列表时第一行就是默认模型（`model`）。旧数据里默认模型可以是列表的任意一行，
// 这样的行挪到最前面显示；不在列表里时不动。
function withDefaultModelFirst<T extends CodexCatalogModel>(
  models: T[],
  defaultModel: string,
): T[] {
  const target = defaultModel.trim();
  const index = target
    ? models.findIndex((model) => model.model.trim() === target)
    : -1;
  if (index <= 0) return models;
  return [models[index], ...models.filter((_, i) => i !== index)];
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
        JSON.stringify(incoming.inputModalities ?? []) &&
      JSON.stringify(row.reasoningLevels ?? []) ===
        JSON.stringify(incoming.reasoningLevels ?? []) &&
      (row.defaultReasoningLevel ?? "") ===
        (incoming.defaultReasoningLevel ?? "")
    );
  });
}

// Reasoning effort levels Codex understands, in ascending depth order. The
// backend drops unknown values, so the UI only offers canonical ones.
const CODEX_REASONING_LEVELS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

// Select value standing for "don't write the effort key" (Radix Select items
// can't use an empty string). Shared by the main and subagent effort pickers.
const EFFORT_INHERIT = "__inherit__";
// 自动审批模型下拉的「不写覆盖、用 Codex 的默认」（Radix Select 不收空值）。
const AUTO_REVIEW_DEFAULT = "__codex_default__";
// Codex 不写覆盖时用的审批模型（codex-rs `DEFAULT_APPROVAL_REVIEW_PREFERRED_MODEL`）。
const CODEX_AUTO_REVIEW_MODEL = "codex-auto-review";

// Trigger label for the picked levels, kept narrow for the catalog cell. Each
// run of three or more adjacent canonical levels collapses to "first → last"
// ("none, low → max" when minimal is skipped); a range never spans a gap, so
// it can't imply an unpicked level. Non-canonical order stays a plain list.
export function formatReasoningLevelsLabel(levels: string[]): string {
  const canonical = CODEX_REASONING_LEVELS as readonly string[];
  const positions = levels.map((level) => canonical.indexOf(level));
  const ordered = positions.every(
    (position, index) =>
      position >= 0 && (index === 0 || position > positions[index - 1]),
  );
  if (!ordered) return levels.join(", ");

  const parts: string[] = [];
  let runStart = 0;
  for (let index = 1; index <= levels.length; index++) {
    if (
      index < levels.length &&
      positions[index] === positions[index - 1] + 1
    ) {
      continue;
    }
    const run = levels.slice(runStart, index);
    if (run.length >= 3) {
      parts.push(`${run[0]} → ${run[run.length - 1]}`);
    } else {
      parts.push(...run);
    }
    runStart = index;
  }
  return parts.join(", ");
}

// Sentinel for the default-level Select: Radix Select forbids empty item
// values, so "back to Auto" needs a non-empty value mapped to undefined.
const AUTO_DEFAULT_REASONING_LEVEL = "__auto__";

function ReasoningLevelsEditor({
  levels,
  defaultLevel,
  onLevelsChange,
  onDefaultLevelChange,
}: {
  levels?: string[];
  defaultLevel?: string;
  onLevelsChange: (levels: string[] | undefined) => void;
  onDefaultLevelChange: (level: string | undefined) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const selected = (levels ?? []).filter((level) =>
    (CODEX_REASONING_LEVELS as readonly string[]).includes(level),
  );

  const toggleLevel = (level: string) => {
    const picked = selected.includes(level)
      ? selected.filter((item) => item !== level)
      : [...selected, level];
    // Store in canonical ascending-depth order (not click order): the Codex
    // picker and the generated catalog both follow array order.
    const next = (CODEX_REASONING_LEVELS as readonly string[]).filter((item) =>
      picked.includes(item),
    );
    onLevelsChange(next.length > 0 ? next : undefined);
    if (defaultLevel && !next.includes(defaultLevel)) {
      onDefaultLevelChange(undefined);
    }
  };

  const triggerLabel =
    selected.length > 0
      ? formatReasoningLevelsLabel(selected)
      : t("codexConfig.reasoningLevelsNotSet", {
          defaultValue: "Not set",
        });

  // The label may be a range or truncated, so hovering spells out every
  // picked level plus the default one.
  const triggerTip =
    selected.length > 0 ? (
      <>
        <div>{selected.join(", ")}</div>
        {defaultLevel && (
          <div>
            {t("codexConfig.defaultReasoningLevelTip", {
              level: defaultLevel,
              defaultValue: "Default level: {{level}}",
            })}
          </div>
        )}
      </>
    ) : undefined;

  return (
    <Popover modal open={open} onOpenChange={setOpen}>
      <HoverTip content={triggerTip}>
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            aria-expanded={open}
            // 和同一行的 Input、Select 同一套外观，行高才对得齐。
            className={cn(
              fieldClass,
              "flex h-8 items-center justify-between gap-1 whitespace-nowrap text-start data-[state=open]:border-ring data-[state=open]:ring-[3px] data-[state=open]:ring-ring/20",
            )}
          >
            <span
              className={cn("truncate", selected.length === 0 && "text-fg-3")}
            >
              {triggerLabel}
            </span>
            <ChevronDown className="h-4 w-4 shrink-0 opacity-50" />
          </button>
        </PopoverTrigger>
      </HoverTip>
      <PopoverContent
        side="bottom"
        align="start"
        sideOffset={6}
        avoidCollisions
        collisionPadding={8}
        // Flipping alone can't help when neither side of the trigger has
        // room for the whole panel (short window, row mid-form): cap it to
        // the space Radix measured and let the level list scroll instead of
        // cutting off the bottom, including the default-level picker.
        className="z-[1000] flex max-h-[var(--radix-popover-content-available-height)] w-[var(--radix-popover-trigger-width)] flex-col p-0 border-border"
      >
        <Command className="min-h-0 flex-1">
          <CommandInput
            placeholder={t("codexConfig.reasoningLevelsSearch", {
              defaultValue: "Search reasoning levels...",
            })}
          />
          <CommandList>
            <CommandEmpty>
              {t("codexConfig.reasoningLevelsEmpty", {
                defaultValue: "No levels",
              })}
            </CommandEmpty>
            <CommandGroup>
              {CODEX_REASONING_LEVELS.map((level) => (
                <CommandItem
                  key={level}
                  value={level}
                  onSelect={() => toggleLevel(level)}
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4",
                      selected.includes(level) ? "opacity-100" : "opacity-0",
                    )}
                  />
                  <span className="flex-1">{level}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
        {selected.length > 0 && (
          <div className="shrink-0 border-t border-border p-2">
            <span className="text-xs text-fg-2">
              {t("codexConfig.defaultReasoningLevelLabel", {
                defaultValue: "Default level",
              })}
            </span>
            <Select
              value={defaultLevel ?? AUTO_DEFAULT_REASONING_LEVEL}
              onValueChange={(value) =>
                onDefaultLevelChange(
                  value === AUTO_DEFAULT_REASONING_LEVEL ? undefined : value,
                )
              }
            >
              <SelectTrigger className="mt-1 h-8 w-full">
                <SelectValue
                  placeholder={t(
                    "codexConfig.defaultReasoningLevelPlaceholder",
                    { defaultValue: "Auto" },
                  )}
                />
              </SelectTrigger>
              {/* Must render above the enclosing z-[1000] popover: the
                  default SelectContent z-[100] would hide the menu behind
                  the panel when it flips upward. */}
              <SelectContent className="z-[1100]">
                <SelectItem value={AUTO_DEFAULT_REASONING_LEVEL}>
                  {t("codexConfig.defaultReasoningLevelPlaceholder", {
                    defaultValue: "Auto",
                  })}
                </SelectItem>
                {selected.map((level) => (
                  <SelectItem key={level} value={level}>
                    {level}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

export function CodexFormFields({
  appId = "codex",
  providerId,
  isCopilotPreset,
  isCopilotAuthenticated,
  selectedGitHubAccountId,
  onGitHubAccountSelect,
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
  isCodexOauthPreset = false,
  selectedCodexAccountId,
  onCodexAccountSelect,
  onCodexAuthSelectionConfirmed,
  onCodexAuthSelectionInvalidated,
  onManageAuthAccounts,
  codexOauthSelectionLabel,
  codexOauthNoneOptionLabel,
  codexOauthNoneOptionDescription,
  codexOauthAllowUnboundSelection,
  codexOauthAllowUnboundSelectionWithoutStatus,
  codexOauthNativeLoginOnly,
  codexOauthRequireExplicitSelection,
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
  subagentModel = "",
  onSubagentModelChange,
  subagentReasoningEffort = "",
  onSubagentReasoningEffortChange,
  apiFormat,
  onApiFormatChange,
  copilotApiFormat = "auto",
  onCopilotApiFormatChange,
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
  autoReviewModel = "",
  onAutoReviewModelChange,
  onCatalogModelsChange,
  speedTestEndpoints,
  customUserAgent,
  onCustomUserAgentChange,
  localProxyHeadersOverride,
  onLocalProxyHeadersOverrideChange,
  localProxyBodyOverride,
  onLocalProxyBodyOverrideChange,
  variant = "classic",
  directModeFormatNotice,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  // 拉取请求序号：请求身份（Base URL / 完整地址开关 / API Key / 自定义 UA /
  // Header 覆盖）一变即自增，清空旧列表并作废在途响应——/models 结果可能按
  // Key 的模型授权或请求头身份返回，换号/换 Header 后残留旧列表会误导选择
  const fetchModelsSeqRef = useRef(0);

  useEffect(() => {
    fetchModelsSeqRef.current += 1;
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
  }, [
    codexBaseUrl,
    isFullUrl,
    codexApiKey,
    customUserAgent,
    isCopilotPreset,
    copilotApiFormat,
    isCopilotAuthenticated,
    selectedGitHubAccountId,
    localProxyHeadersOverride,
    isXaiOauthPreset,
    isXaiOauthAuthenticated,
    selectedXaiAccountId,
  ]);
  // 思考能力随 Chat 格式显示（仅 Chat Completions 转换路径用得上）；模型映射常驻
  //（填了才生成 catalog）。两者都已与「路由接管」概念解耦。
  const effectiveApiFormat = isCopilotPreset
    ? copilotApiFormat === "auto"
      ? "openai_chat"
      : copilotApiFormat
    : apiFormat;
  const isChatFormat = effectiveApiFormat === "openai_chat";
  const isAnthropicFormat = effectiveApiFormat === "anthropic";
  // Grok Build 复用本表单，但语义与 Codex 有差异（无模型映射、协议由 TOML 的
  // api_backend 声明、请求体也不是 Codex 发出的）——提示文案按 appId 分流，
  // 对应词条在 grokBuild.* 下。
  const isGrokBuild = appId === "grokbuild";
  const canEditCatalog = Boolean(onCatalogModelsChange);
  // 有模型列表的卡（两种布局）：列表第一行就是默认模型，没有单独的默认模型框；列表为空时
  // 直连布局才显示默认模型框。Grok Build 没有列表，官方卡不显示列表。
  const firstRowIsDefault = canEditCatalog && category !== "official";
  const canEditReasoning = Boolean(onCodexChatReasoningChange);
  const supportsThinking =
    codexChatReasoning.supportsThinking === true ||
    codexChatReasoning.supportsEffort === true;
  const supportsEffort = codexChatReasoning.supportsEffort === true;

  // 高级区在有任何可见配置时自动展开（仅折叠→展开，不会自动折叠）：自定义 UA /
  // 请求覆盖 / 非默认的上游格式 / 已配置思考能力。模型列表在高级区外面，不算。
  const hasRequestOverrides = Boolean(
    localProxyHeadersOverride.trim() || localProxyBodyOverride.trim(),
  );
  const hasAnyAdvancedValue =
    isCopilotPreset ||
    !!customUserAgent ||
    hasRequestOverrides ||
    isAnthropicFormat ||
    supportsThinking ||
    supportsEffort ||
    promptCacheRouting !== "auto" ||
    !!maxOutputTokens ||
    isFullUrl;
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

  // Stack 布局的高级区（思考能力、Anthropic 专有项、User-Agent、请求覆盖）：填了 UA 或请求
  // 覆盖才展开，其余多是预设自带的。
  const [stackAdvancedExpanded, setStackAdvancedExpanded] = useState(
    isFullUrl || !!customUserAgent || hasRequestOverrides,
  );

  // 显示用的顺序。挪动默认那一行不算修改：没动过列表时保存仍是原来的顺序。
  const orderCatalogModels = (models: CodexCatalogModel[]) =>
    firstRowIsDefault ? withDefaultModelFirst(models, codexModel) : models;

  const [catalogRows, setCatalogRows] = useState<CodexCatalogRow[]>(() =>
    orderCatalogModels(catalogModels).map((m) => createCatalogRow(m)),
  );

  // 记录上次发送给父组件的数据，避免重复触发
  const lastSentModelsRef = useRef<CodexCatalogModel[]>(
    orderCatalogModels(catalogModels),
  );

  // 父 → 子：仅当 prop 数据真的变化（预设切换 / 编辑加载）时才重建 rowId；
  // 同 shape 时保留现有 rowId，避免编辑过程中焦点丢失。
  useEffect(() => {
    const models = orderCatalogModels(catalogModels);
    setCatalogRows((current) => {
      if (catalogRowsMatchModels(current, models)) return current;
      return models.map((m) => createCatalogRow(m));
    });
    // 同步更新 ref，避免父组件传入新数据时子→父 effect 误判为本地修改
    lastSentModelsRef.current = models;
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  // `model` 比列表晚到（编辑加载时父组件先给列表、再从 TOML 读出 `model`）：在当前行上把它
  // 挪到第一行，行 id 不变，也不回传父组件。编辑中 `model` 只会跟着第一行变，这里不会动。
  // 放在子 → 父 effect 之后：同一次渲染里改了的行先回传，再看要不要挪。
  useEffect(() => {
    if (!firstRowIsDefault) return;
    const ordered = withDefaultModelFirst(catalogRows, codexModel);
    if (ordered === catalogRows) return;
    lastSentModelsRef.current = ordered.map(
      ({ rowId: _rowId, ...rest }) => rest,
    );
    setCatalogRows(ordered);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [codexModel, firstRowIsDefault]);

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

  const receiveFetchedModels = useCallback((models: FetchedModel[]) => {
    setFetchedModels(models);
    return models.length;
  }, []);

  const runModelFetch = useCallback(
    <T,>(
      fetchModels: () => Promise<T>,
      receiveModels: (models: T) => number,
      errorLogMessage: string,
    ) => {
      const seq = ++fetchModelsSeqRef.current;
      setIsFetchingModels(true);
      fetchModels()
        .then((models) => {
          if (seq !== fetchModelsSeqRef.current) return;
          const count = receiveModels(models);
          if (count === 0) {
            toast.info(t("providerForm.fetchModelsEmpty"));
          } else if (!canEditCatalog) {
            // 有模型列表时拉取成功会直接打开选择弹窗，不再提示成功。
            toast.success(t("providerForm.fetchModelsSuccess", { count }));
          }
        })
        .catch((err) => {
          if (seq !== fetchModelsSeqRef.current) return;
          console.warn(errorLogMessage, err);
          showFetchModelsError(err, t);
        })
        .finally(() => setIsFetchingModels(false));
    },
    [canEditCatalog, t],
  );

  const handleFetchModels = useCallback(() => {
    if (isCopilotPreset) {
      if (!isCopilotAuthenticated) {
        toast.error(
          t("copilot.loginRequired", {
            defaultValue: "请先登录 GitHub Copilot",
          }),
        );
        return;
      }
      runModelFetch(
        () =>
          selectedGitHubAccountId
            ? copilotGetModelsForAccount(selectedGitHubAccountId)
            : copilotGetModels(),
        (models) => {
          const usableModels = models.filter((model) =>
            isCopilotModelSupportedByCodex(model, copilotApiFormat),
          );
          const fetched = usableModels.map((model) => ({
            id: model.id,
            ownedBy: model.vendor || null,
          }));
          setFetchedModels(fetched);

          if (onCatalogModelsChange) {
            const existing = new Map(
              catalogModels.map((model) => [model.model, model]),
            );
            onCatalogModelsChange(
              usableModels.map((model) => ({
                ...(existing.get(model.id) ?? {}),
                model: model.id,
                displayName: model.name || model.id,
                contextWindow: resolveCopilotReportedPromptLimit(
                  existing.get(model.id)?.contextWindow,
                  model.context_window,
                ),
                supportsParallelToolCalls:
                  model.supports_parallel_tool_calls ??
                  existing.get(model.id)?.supportsParallelToolCalls ??
                  false,
                inputModalities: existing.get(model.id)?.inputModalities ?? [
                  "text",
                ],
                ...(model.reasoning_effort !== undefined
                  ? { reasoningLevels: model.reasoning_effort }
                  : {}),
              })),
            );
          }
          if (
            usableModels.length > 0 &&
            onModelChange &&
            !usableModels.some((model) => model.id === codexModel)
          ) {
            const defaultModel =
              usableModels.find((model) =>
                model.id.toLowerCase().startsWith("gpt-"),
              ) ?? usableModels[0];
            onModelChange(defaultModel.id);
          }
          return usableModels.length;
        },
        "[Copilot] Failed to fetch models:",
      );
      return;
    }

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
      runModelFetch(
        () => fetchXaiOauthModels(selectedXaiAccountId ?? null),
        receiveFetchedModels,
        "[XaiOAuth] Failed to fetch models:",
      );
      return;
    }

    if (!codexBaseUrl || !codexApiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!codexApiKey,
        hasBaseUrl: !!codexBaseUrl,
      });
      return;
    }
    runModelFetch(
      () =>
        fetchModelsForConfig(
          codexBaseUrl,
          codexApiKey,
          isFullUrl,
          undefined,
          customUserAgent,
          {
            requestHeaders: modelFetchRequestHeaders(localProxyHeadersOverride),
          },
        ),
      receiveFetchedModels,
      "[ModelFetch] Failed:",
    );
  }, [
    runModelFetch,
    receiveFetchedModels,
    codexBaseUrl,
    codexApiKey,
    codexModel,
    catalogModels,
    isFullUrl,
    customUserAgent,
    isCopilotPreset,
    copilotApiFormat,
    isCopilotAuthenticated,
    selectedGitHubAccountId,
    onCatalogModelsChange,
    onModelChange,
    localProxyHeadersOverride,
    isXaiOauthPreset,
    isXaiOauthAuthenticated,
    selectedXaiAccountId,
    t,
  ]);

  const fillModelMetadata = useModelMetadataFill({
    baseUrl: codexBaseUrl,
    presets: codexPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  // 补全要在「改模型名」提交后立刻读到那一行，所以这两处经 ref 同步提交。
  const [catalogRowsRef, commitCatalogRows] = useCommittableRef(
    catalogRows,
    setCatalogRows,
  );

  // 按模型名补上这一行已知的窗口、档位和模态（只补空着的）。
  const fillCatalogRowMetadata = useCallback(
    (rowId: string, modelId: string, silent?: boolean) =>
      fillModelMetadata(
        modelId,
        (metadata) => {
          const rows = catalogRowsRef.current;
          const current = rows.find((row) => row.rowId === rowId);
          if (current?.model.trim() !== modelId) return false;
          const filled = fillCodexCatalogModel(
            current,
            metadata,
            CODEX_REASONING_LEVELS,
          );
          if (!metadataFilledAnything(current, filled)) return false;
          commitCatalogRows(
            rows.map((row) => (row.rowId === rowId ? filled : row)),
          );
          return true;
        },
        { silent },
      ),
    [catalogRowsRef, commitCatalogRows, fillModelMetadata],
  );

  // 第一行就是 `model`（加载时已挪到最前面）：它不在列表里时没有这样的行；没填时保存会用
  // 第一行。
  const trimmedDefaultModel = codexModel.trim();
  const defaultRowIndex = trimmedDefaultModel
    ? catalogRows.findIndex((row) => row.model.trim() === trimmedDefaultModel)
    : 0;
  // 默认模型那一行就代表 `model`：改它的名字、删掉它，`model` 当场跟着变。没有这样的行时
  // 是 -1。
  const linkedDefaultIndex =
    firstRowIsDefault && trimmedDefaultModel ? defaultRowIndex : -1;

  // 列表从空变成有行时，当前默认模型排第一行（已在新增的行里就挪到最前，否则补一行），
  // 默认模型框随之隐藏，默认值不会悄悄换掉。
  const seedWithDefaultModel = useCallback(
    (additions: CodexCatalogRow[]): CodexCatalogRow[] => {
      if (!firstRowIsDefault || !trimmedDefaultModel) return additions;
      const ordered = withDefaultModelFirst(additions, trimmedDefaultModel);
      if (ordered[0]?.model.trim() === trimmedDefaultModel) return ordered;
      return [
        createCatalogRow({
          model: trimmedDefaultModel,
          displayName: trimmedDefaultModel,
        }),
        ...additions,
      ];
    },
    [firstRowIsDefault, trimmedDefaultModel],
  );

  const handleAddCatalogRow = useCallback(() => {
    if (!onCatalogModelsChange) return;
    const rows = catalogRowsRef.current;
    const added =
      rows.length === 0
        ? seedWithDefaultModel([createCatalogRow()])
        : [createCatalogRow()];
    commitCatalogRows([...rows, ...added]);
    for (const row of added) {
      if (row.model) fillCatalogRowMetadata(row.rowId, row.model);
    }
  }, [
    catalogRowsRef,
    commitCatalogRows,
    fillCatalogRowMetadata,
    onCatalogModelsChange,
    seedWithDefaultModel,
  ]);

  const handleUpdateCatalogRow = useCallback(
    (index: number, patch: Partial<CodexCatalogModel>) => {
      if (patch.model !== undefined && index === linkedDefaultIndex) {
        onModelChange?.(patch.model);
      }
      setCatalogRows((current) =>
        current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
      );
    },
    [linkedDefaultIndex, onModelChange],
  );

  const handleSelectFetchedCatalogModel = useCallback(
    (rowId: string, modelId: string) => {
      const rows = catalogRowsRef.current;
      if (rows.findIndex((row) => row.rowId === rowId) === linkedDefaultIndex) {
        onModelChange?.(modelId);
      }
      commitCatalogRows(
        rows.map((row) =>
          row.rowId === rowId
            ? {
                ...row,
                model: modelId,
                displayName: row.displayName?.trim()
                  ? row.displayName
                  : modelId,
              }
            : row,
        ),
      );
      fillCatalogRowMetadata(rowId, modelId);
    },
    [
      catalogRowsRef,
      commitCatalogRows,
      fillCatalogRowMetadata,
      linkedDefaultIndex,
      onModelChange,
    ],
  );

  const handleRemoveCatalogRow = useCallback(
    (index: number) => {
      if (index === linkedDefaultIndex) {
        const next = catalogRows
          .filter((_, i) => i !== index)
          .map((row) => row.model.trim())
          .find(Boolean);
        // 删光了就留着 `model`：列表为空时发布的正是它。
        if (next) onModelChange?.(next);
      }
      setCatalogRows((current) => current.filter((_, i) => i !== index));
    },
    [catalogRows, linkedDefaultIndex, onModelChange],
  );

  // 按弹窗里的勾选增删列表行：留下的行原样不动，新勾的追加到末尾并补参数。
  const handleApplyFetchedCatalogRows = useCallback(
    ({ add, remove }: FetchedModelSelection) => {
      const current = catalogRowsRef.current;
      const removed = new Set(remove);
      const kept = current.filter((row) => !removed.has(row.model.trim()));
      const configured = new Set(kept.map((row) => row.model.trim()));
      const picked = add
        .filter((id) => !configured.has(id))
        .map((id) => createCatalogRow({ model: id, displayName: id }));
      const additions =
        current.length === 0 ? seedWithDefaultModel(picked) : picked;
      const next = [...kept, ...additions];
      // 默认模型那一行被取消：和删行一样改用新的第一行；删光了就留着 `model`。
      if (linkedDefaultIndex >= 0 && removed.has(trimmedDefaultModel)) {
        const first = next.map((row) => row.model.trim()).find(Boolean);
        if (first) onModelChange?.(first);
      }
      commitCatalogRows(next);
      // 弹窗里已经显示过这些参数，补完不再提示。
      for (const row of additions) {
        fillCatalogRowMetadata(row.rowId, row.model, true);
      }
    },
    [
      catalogRowsRef,
      commitCatalogRows,
      fillCatalogRowMetadata,
      linkedDefaultIndex,
      onModelChange,
      seedWithDefaultModel,
      trimmedDefaultModel,
    ],
  );

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
        ownedBy: t("codexConfig.modelListTitle", {
          defaultValue: "模型列表",
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
  const isDefaultModelOutsideCatalog =
    catalogRows.length > 0 &&
    !!trimmedDefaultModel &&
    !catalogRows.some((row) => row.model.trim() === trimmedDefaultModel);

  const handleAddDefaultModelToCatalog = useCallback(() => {
    if (!onCatalogModelsChange || !trimmedDefaultModel) return;
    const row = createCatalogRow({
      model: trimmedDefaultModel,
      displayName: trimmedDefaultModel,
    });
    // 默认模型排第一位。
    const rows = catalogRowsRef.current;
    commitCatalogRows(firstRowIsDefault ? [row, ...rows] : [...rows, row]);
    fillCatalogRowMetadata(row.rowId, trimmedDefaultModel);
  }, [
    catalogRowsRef,
    commitCatalogRows,
    fillCatalogRowMetadata,
    firstRowIsDefault,
    onCatalogModelsChange,
    trimmedDefaultModel,
  ]);

  // 自动审批模型只能选列表里的：审批请求和主对话发给同一家，后端也只写目录里有的模型。
  const autoReviewChoices = useMemo(() => {
    const seen = new Set<string>();
    const choices: { model: string; label: string }[] = [];
    for (const row of catalogRows) {
      const model = row.model.trim();
      if (!model || seen.has(model)) continue;
      seen.add(model);
      choices.push({ model, label: row.displayName?.trim() || model });
    }
    return choices;
  }, [catalogRows]);
  const trimmedAutoReviewModel = autoReviewModel.trim();
  const autoReviewSelectValue = autoReviewChoices.some(
    (choice) => choice.model === trimmedAutoReviewModel,
  )
    ? trimmedAutoReviewModel
    : AUTO_REVIEW_DEFAULT;
  // 不写覆盖时 Codex 先找目录里的 codex-auto-review：没有列表（用 Codex 自带的列表）或列表里
  // 就有它时用它，否则由主模型自己审批。
  const autoReviewDefaultIsCodex =
    autoReviewChoices.length === 0 ||
    autoReviewChoices.some(
      (choice) => choice.model === CODEX_AUTO_REVIEW_MODEL,
    );

  // 子代理模型不在模型映射里：Codex 拿不到它的窗口和推理档位；经路由转成 Chat / Anthropic
  // 协议时还会被换成默认模型（转换路径只放行映射里的模型）。没有映射、走原生 Responses 的
  // 供应商直接请求任意模型名，不提示。
  const trimmedSubagentModel = subagentModel.trim();
  const isSubagentModelOutsideCatalog =
    !!trimmedSubagentModel &&
    (catalogRows.length > 0 || isChatFormat || isAnthropicFormat) &&
    !catalogRows.some((row) => row.model.trim() === trimmedSubagentModel);

  const handleAddSubagentModelToCatalog = useCallback(() => {
    if (!onCatalogModelsChange || !trimmedSubagentModel) return;
    const rows = catalogRowsRef.current;
    const added: CodexCatalogRow[] = [];
    // 映射原本为空时默认模型也一起加进去，不然加完它就成了映射外的模型。
    if (
      rows.length === 0 &&
      trimmedDefaultModel &&
      trimmedDefaultModel !== trimmedSubagentModel
    ) {
      added.push(
        createCatalogRow({
          model: trimmedDefaultModel,
          displayName: trimmedDefaultModel,
        }),
      );
    }
    added.push(
      createCatalogRow({
        model: trimmedSubagentModel,
        displayName: trimmedSubagentModel,
      }),
    );
    commitCatalogRows([...rows, ...added]);
    for (const row of added) fillCatalogRowMetadata(row.rowId, row.model);
  }, [
    catalogRowsRef,
    commitCatalogRows,
    fillCatalogRowMetadata,
    onCatalogModelsChange,
    trimmedDefaultModel,
    trimmedSubagentModel,
  ]);

  const renderCatalogActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      <FetchedModelPicker
        models={fetchedModels}
        configuredModelIds={catalogRows.map((row) => row.model)}
        onApply={handleApplyFetchedCatalogRows}
        onFetch={handleFetchModels}
        isFetching={isFetchingModels}
        metadataSource={{
          baseUrl: codexBaseUrl,
          presets: codexPresetModelSources,
        }}
      />
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

  // 上游格式及 Anthropic 格式专有的几项：经典布局都在高级选项里；Stack 布局把上游格式和
  // 认证字段放进连接区，其余留在高级选项里。
  // 字段名后面的「?」：规则和术语解释收进去；后果（要开路由、会报错）仍写在页面上。
  const labelWithHelp = (label: string, help: ReactNode, htmlFor?: string) => (
    <div className="flex items-center gap-0.5">
      <FormLabel htmlFor={htmlFor}>{label}</FormLabel>
      <HelpTip title={label}>{help}</HelpTip>
    </div>
  );

  const upstreamFormatSelect = (
    <div className="space-y-1.5">
      {labelWithHelp(
        t("codexConfig.upstreamFormatLabel", { defaultValue: "上游格式" }),
        isCopilotPreset
          ? t("codexConfig.upstreamFormatCopilotHint")
          : t("codexConfig.upstreamFormatHint", {
              defaultValue:
                "看供应商原生支持哪种协议：Responses 原样转发、不转换格式；Chat 或 Anthropic 由路由转成 Responses。",
            }),
        "codex-upstream-format",
      )}
      <Select
        value={isCopilotPreset ? copilotApiFormat : apiFormat}
        onValueChange={(value) => {
          if (isCopilotPreset) {
            if (
              value === "auto" ||
              value === "openai_chat" ||
              value === "openai_responses"
            ) {
              onCopilotApiFormatChange?.(value);
            }
          } else if (
            value === "openai_chat" ||
            value === "openai_responses" ||
            value === "anthropic"
          ) {
            onApiFormatChange(value);
          }
        }}
      >
        <SelectTrigger id="codex-upstream-format" className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {isCopilotPreset && (
            <SelectItem value="auto">
              {t("codexConfig.upstreamFormatAuto")}
            </SelectItem>
          )}
          <SelectItem value="openai_chat">
            {t("codexConfig.upstreamFormatChat", {
              defaultValue: "Chat Completions（需转换）",
            })}
          </SelectItem>
          <SelectItem value="openai_responses">
            {isCopilotPreset
              ? t("codexConfig.upstreamFormatCopilotResponses")
              : t("codexConfig.upstreamFormatResponses", {
                  defaultValue: "Responses（原生）",
                })}
          </SelectItem>
          {!isCopilotPreset && (
            <SelectItem value="anthropic">
              {t("codexConfig.upstreamFormatAnthropic", {
                defaultValue: "Anthropic Messages（需转换）",
              })}
            </SelectItem>
          )}
        </SelectContent>
      </Select>
      {/* 直连下 Chat / Anthropic 发不出请求，按实际模式提示后果；Copilot 恒需路由，单独提示。 */}
      {directModeFormatNotice &&
        !isCopilotPreset &&
        (isChatFormat || isAnthropicFormat) && (
          <DirectModeFormatWarning notice={directModeFormatNotice} />
        )}
      {isCopilotPreset && (
        <p className="text-xs leading-relaxed text-fg-2">
          {t("codexConfig.upstreamFormatCopilotRouteHint")}
        </p>
      )}
    </div>
  );

  const anthropicAuthFieldSelect = (
    <div className="space-y-1.5">
      {labelWithHelp(
        t("codexConfig.anthropicAuthFieldLabel", { defaultValue: "认证字段" }),
        t("codexConfig.anthropicAuthFieldHint", {
          defaultValue: "按网关读取 API Key 的请求头选择，两者只发其一。",
        }),
        "codex-anthropic-auth-field",
      )}
      <Select
        value={anthropicAuthField}
        onValueChange={(value) =>
          onAnthropicAuthFieldChange(value as ClaudeApiKeyField)
        }
      >
        <SelectTrigger id="codex-anthropic-auth-field" className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="ANTHROPIC_AUTH_TOKEN">
            {t("codexConfig.anthropicAuthFieldAuthToken", {
              defaultValue: "ANTHROPIC_AUTH_TOKEN（Authorization）",
            })}
          </SelectItem>
          <SelectItem value="ANTHROPIC_API_KEY">
            {t("codexConfig.anthropicAuthFieldApiKey", {
              defaultValue: "ANTHROPIC_API_KEY（x-api-key）",
            })}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>
  );

  const impersonateClaudeCodeToggle = (
    <div className="flex items-center justify-between gap-4 border-t border-border pt-3">
      {labelWithHelp(
        t("codexConfig.impersonateClaudeCodeLabel", {
          defaultValue: "模拟 Claude Code 客户端",
        }),
        t("codexConfig.impersonateClaudeCodeHint", {
          defaultValue:
            "网关只允许 Claude Code 使用时打开：伪装 Claude Code 的请求头，并在系统提示首行注入它的身份。",
        }),
      )}
      <Switch
        checked={impersonateClaudeCode}
        onCheckedChange={onImpersonateClaudeCodeChange}
        aria-label={t("codexConfig.impersonateClaudeCodeLabel", {
          defaultValue: "模拟 Claude Code 客户端",
        })}
      />
    </div>
  );

  const maxOutputTokensField = (
    <div className="space-y-1.5 border-t border-border pt-3">
      {labelWithHelp(
        t("codexConfig.maxOutputTokensLabel", {
          defaultValue: "最大输出 tokens",
        }),
        isGrokBuild
          ? t("grokBuild.maxOutputTokensHint", {
              defaultValue:
                "默认上限 8192，长回答或深度思考容易被截断。这里的值会作为 Anthropic 的 max_tokens 发出。",
            })
          : t("codexConfig.maxOutputTokensHint", {
              defaultValue:
                "Codex 不带输出上限，默认 8192，长回答或深度思考容易被截断。这里的值会作为 Anthropic 的 max_tokens 发出。",
            }),
        "codex-anthropic-max-output-tokens",
      )}
      <Input
        id="codex-anthropic-max-output-tokens"
        type="number"
        min={1}
        inputMode="numeric"
        value={maxOutputTokens}
        onChange={(event) =>
          onMaxOutputTokensChange(event.target.value.replace(/[^\d]/g, ""))
        }
        placeholder={t("codexConfig.maxOutputTokensPlaceholder", {
          defaultValue: "留空则使用默认 8192",
        })}
      />
      <p className="text-xs leading-relaxed text-fg-2">
        {t("codexConfig.maxOutputTokensWarning", {
          defaultValue: "不要超过模型或网关的真实输出上限，否则会返回 400。",
        })}
      </p>
    </div>
  );

  const reasoningFields = (
    <>
      <div className="space-y-2">
        {labelWithHelp(
          t("codexConfig.promptCacheRoutingLabel", {
            defaultValue: "提示词缓存路由",
          }),
          t("codexConfig.promptCacheRoutingHint", {
            defaultValue:
              "自动：只对确认兼容的上游发送 prompt_cache_key。开启：用于其他兼容网关。关闭：避免严格网关因未知字段返回 400。",
          }),
        )}
        <Select
          value={promptCacheRouting}
          onValueChange={(value) =>
            onPromptCacheRoutingChange(value as PromptCacheRoutingMode)
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
      </div>

      <FormLabel>
        {t("codexConfig.reasoningGroupTitle", {
          defaultValue: "思考能力",
        })}
      </FormLabel>

      <div className="flex items-center justify-between gap-4">
        {labelWithHelp(
          t("codexConfig.reasoningModeToggle", {
            defaultValue: "支持思考模式",
          }),
          t("codexConfig.reasoningModeHint", {
            defaultValue:
              "上游 Chat 接口能开关 thinking 时打开，Kimi、GLM、Qwen 等通常属于这一类。",
          }),
        )}
        <Switch
          checked={supportsThinking}
          onCheckedChange={handleReasoningThinkingChange}
          aria-label={t("codexConfig.reasoningModeToggle", {
            defaultValue: "支持思考模式",
          })}
        />
      </div>

      <div className="flex items-center justify-between gap-4 border-t border-border pt-3">
        {labelWithHelp(
          t("codexConfig.reasoningEffortToggle", {
            defaultValue: "支持思考等级",
          }),
          isGrokBuild
            ? t("grokBuild.reasoningEffortHint", {
                defaultValue:
                  "上游支持 low/high/max 等思考深度时打开。会同时打开思考模式，并把请求中的 reasoning effort 转成上游 Chat 参数。",
              })
            : t("codexConfig.reasoningEffortHint", {
                defaultValue:
                  "上游支持 low/high/max 等思考深度时打开。会同时打开思考模式，并把 Codex 的 reasoning.effort 转成上游 Chat 参数。",
              }),
        )}
        <Switch
          checked={supportsEffort}
          onCheckedChange={handleReasoningEffortChange}
          aria-label={t("codexConfig.reasoningEffortToggle", {
            defaultValue: "支持思考等级",
          })}
        />
      </div>
    </>
  );

  // 两项都只在路由模式下生效：在分组开头说一次，各自的作用收进字段名后的「?」。
  // Copilot 的请求由转发层按 Copilot 客户端的样子发，UA 和请求覆盖都会被跳过，不显示。
  const userAgentAndOverrides = isCopilotPreset ? null : (
    <>
      <p className="text-xs leading-relaxed text-fg-2">
        {isGrokBuild
          ? t("grokBuild.routeOnlySettingsHint", {
              defaultValue: "以下两项只在路由模式下生效。",
            })
          : t("codexConfig.routeOnlySettingsHint", {
              defaultValue: "以下两项只在路由、聚合模式下生效。",
            })}
      </p>
      <CustomUserAgentField
        id="codex-custom-user-agent"
        value={customUserAgent}
        onChange={onCustomUserAgentChange}
        help={t("providerForm.customUserAgentHelp", {
          defaultValue: "替换转发到供应商的请求里的 User-Agent。",
        })}
      />
      <div className="border-t border-border pt-3">
        <LocalProxyRequestOverridesField
          headersJson={localProxyHeadersOverride}
          bodyJson={localProxyBodyOverride}
          onHeadersJsonChange={onLocalProxyHeadersOverrideChange}
          onBodyJsonChange={onLocalProxyBodyOverrideChange}
          help={t("providerForm.localProxyRequestOverridesHelp", {
            defaultValue: "作用于格式转换后发给上游的请求。",
          })}
        />
      </div>
    </>
  );

  // 模型映射 / 模型列表的行。
  const renderCatalogRows = () => (
    <div className="space-y-2">
      {/* 列头：md+ 显示 */}
      <div className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_140px_minmax(0,1fr)_36px] gap-2 px-1 text-xs font-medium text-fg-2 md:grid">
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
        <span>
          {t("codexConfig.catalogColumnReasoning", {
            defaultValue: "思考等级",
          })}
        </span>
        <span />
      </div>

      {catalogRows.map((row, index) => (
        <div
          key={row.rowId}
          className="grid grid-cols-1 gap-2 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_140px_minmax(0,1fr)_36px]"
        >
          <Input
            value={row.displayName ?? ""}
            onChange={(event) =>
              handleUpdateCatalogRow(index, {
                displayName: event.target.value,
              })
            }
            placeholder={t("codexConfig.catalogDisplayNamePlaceholder", {
              defaultValue: "例如: DeepSeek V4 Flash",
            })}
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
              placeholder={t("codexConfig.catalogModelPlaceholder", {
                defaultValue: "例如: deepseek-v4-flash",
              })}
              aria-label={t("codexConfig.catalogColumnModel", {
                defaultValue: "实际请求模型",
              })}
              className="flex-1"
            />
            {fetchedModels.length > 0 && (
              <ModelDropdown
                models={fetchedModels}
                onSelect={(id) =>
                  handleSelectFetchedCatalogModel(row.rowId, id)
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
                contextWindow: event.target.value.replace(/[^\d]/g, ""),
              })
            }
            placeholder={t("codexConfig.contextWindowPlaceholder", {
              defaultValue: "例如: 128000",
            })}
            aria-label={t("codexConfig.catalogColumnContext", {
              defaultValue: "上下文窗口",
            })}
          />
          <ReasoningLevelsEditor
            levels={row.reasoningLevels}
            defaultLevel={row.defaultReasoningLevel}
            onLevelsChange={(levels) =>
              handleUpdateCatalogRow(index, {
                reasoningLevels: levels,
              })
            }
            onDefaultLevelChange={(level) =>
              handleUpdateCatalogRow(index, {
                defaultReasoningLevel: level,
              })
            }
          />
          <HoverTip content={t("common.delete", { defaultValue: "删除" })}>
            <Button
              aria-label={t("common.delete", { defaultValue: "删除" })}
              type="button"
              variant="ghost"
              size="icon"
              className="text-fg-2 hover:text-destructive"
              onClick={() => handleRemoveCatalogRow(index)}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </HoverTip>
          <GptContextWindowHint
            modelId={row.model}
            contextWindow={row.contextWindow}
            className="col-span-full px-1"
          />
        </div>
      ))}
    </div>
  );

  const oauthSections = (
    <>
      {isCopilotPreset && (
        <CopilotAuthSection
          mode="select"
          selectedAccountId={selectedGitHubAccountId}
          onAccountSelect={onGitHubAccountSelect}
          onManageAccounts={
            onManageAuthAccounts
              ? () => onManageAuthAccounts("github_copilot")
              : undefined
          }
        />
      )}

      {/* Codex OAuth 账号选择 */}
      {isCodexOauthPreset && (
        <CodexOAuthSection
          mode="select"
          selectedAccountId={selectedCodexAccountId}
          onAccountSelect={onCodexAccountSelect}
          onSelectionConfirmed={onCodexAuthSelectionConfirmed}
          onSelectionInvalidated={onCodexAuthSelectionInvalidated}
          onManageAccounts={
            onManageAuthAccounts
              ? () => onManageAuthAccounts("codex_oauth")
              : undefined
          }
          selectionLabel={codexOauthSelectionLabel}
          noneOptionLabel={codexOauthNoneOptionLabel}
          noneOptionDescription={codexOauthNoneOptionDescription}
          allowUnboundSelection={codexOauthAllowUnboundSelection}
          allowUnboundSelectionWithoutStatus={
            codexOauthAllowUnboundSelectionWithoutStatus
          }
          nativeLoginOnly={codexOauthNativeLoginOnly}
          requireExplicitSelection={codexOauthRequireExplicitSelection}
        />
      )}

      {/* xAI OAuth 认证（Grok 订阅托管账号） */}
      {isXaiOauthPreset && (
        <XaiOAuthSection
          selectedAccountId={selectedXaiAccountId}
          onAccountSelect={onXaiAccountSelect}
        />
      )}
    </>
  );

  const apiKeySection = (
    <>
      {/* Codex API Key 输入框（托管 OAuth 预设无需 Key） */}
      {!isCopilotPreset && !isCodexOauthPreset && !isXaiOauthPreset && (
        <ApiKeySection
          id="codexApiKey"
          label="API Key"
          value={codexApiKey}
          onChange={onApiKeyChange}
          category={category}
          required
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
    </>
  );

  const endpointSection = (
    <>
      {/* Codex Base URL 输入框（托管 OAuth 端点由 adapter 硬定向，不展示） */}
      {shouldShowSpeedTest && !isCopilotPreset && !isXaiOauthPreset && (
        <EndpointField
          id="codexBaseUrl"
          label={t("codexConfig.apiUrlLabel")}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.codexApiEndpointPlaceholder")}
          help={t("providerForm.codexApiHint")}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}
    </>
  );

  // 完整 URL 跟着地址框（托管 OAuth 端点由 adapter 硬定向，没有这个开关）。
  const showFullUrlToggle =
    shouldShowSpeedTest && !isCopilotPreset && !isXaiOauthPreset;
  const fullUrlToggle = (
    <FullUrlToggle
      checked={isFullUrl}
      onCheckedChange={onFullUrlChange}
      className="border-t border-border pt-3"
    />
  );

  const speedTestModal = (
    <>
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

  // 默认模型（`model`）不在列表里：没有哪一行是默认，提示并给出加入列表（排第一行）。
  const defaultModelNotInListNotice = isDefaultModelOutsideCatalog && (
    <p className="flex flex-wrap items-center gap-x-2 text-xs leading-relaxed text-fg-2">
      {t("codexConfig.defaultModelNotInList", {
        model: trimmedDefaultModel,
        defaultValue:
          "默认模型 {{model}} 不在列表里：Codex 默认仍请求它，但 /model 里没有这一项。",
      })}
      <Button
        type="button"
        variant="link"
        size="sm"
        className="h-auto p-0 text-xs"
        onClick={handleAddDefaultModelToCatalog}
      >
        {t("codexConfig.addToModelList", {
          defaultValue: "加入列表",
        })}
      </Button>
    </p>
  );

  // 子代理推理强度下拉：第一项是不写这个键；值不在已知档位里时原样列出，
  // 免得手写的值被下拉吞掉。
  const renderEffortSelect = (
    value: string,
    onChange: (effort: string) => void,
    ariaLabel: string,
    inheritLabel: string,
    className: string,
  ) => (
    <Select
      value={value || EFFORT_INHERIT}
      onValueChange={(next) => onChange(next === EFFORT_INHERIT ? "" : next)}
    >
      <SelectTrigger aria-label={ariaLabel} className={className}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={EFFORT_INHERIT}>{inheritLabel}</SelectItem>
        {value &&
          !(CODEX_REASONING_LEVELS as readonly string[]).includes(value) && (
            <SelectItem value={value}>{value}</SelectItem>
          )}
        {CODEX_REASONING_LEVELS.map((level) => (
          <SelectItem key={level} value={level}>
            {level}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );

  // 默认模型 —— config.toml 顶层 model，Codex 启动时默认请求的模型。实时写回 TOML；留空则删行。
  // 有模型列表时第一行就是它，不显示这个框（Grok Build 没有列表，一直显示）；列表为空时
  // 两种布局都显示，聚合模式下发布的就是它。
  const defaultModelField = category !== "official" &&
    onModelChange &&
    (!firstRowIsDefault || catalogRows.length === 0) && (
      <div className="space-y-1.5">
        {labelWithHelp(
          t("codexConfig.defaultModelLabel", { defaultValue: "默认模型" }),
          isGrokBuild
            ? t("grokBuild.defaultModelHint", {
                defaultValue: "Grok Build 默认请求的模型，随时可改。",
              })
            : t("codexConfig.defaultModelHint", {
                defaultValue:
                  "Codex 默认请求的模型。添加模型列表后，列表第一行就是默认模型。",
              }),
          "codexDefaultModel",
        )}
        <div className="flex gap-1">
          <Input
            id="codexDefaultModel"
            value={codexModel}
            onChange={(event) => onModelChange(event.target.value)}
            placeholder={
              isGrokBuild
                ? t("grokBuild.defaultModelPlaceholder", {
                    defaultValue: "例如: grok-4.5",
                  })
                : t("codexConfig.defaultModelPlaceholder", {
                    defaultValue: "例如: gpt-5.6",
                  })
            }
            className="flex-1"
          />
          {/* 有模型列表时用列表上的「获取模型列表」，这里不再放一个。 */}
          {!canEditCatalog && (
            <HoverTip content={t("providerForm.fetchModels")}>
              <Button
                aria-label={t("providerForm.fetchModels")}
                type="button"
                variant="outline"
                size="icon"
                onClick={handleFetchModels}
                disabled={isFetchingModels}
                className="shrink-0"
              >
                {isFetchingModels ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Download className="h-4 w-4" />
                )}
              </Button>
            </HoverTip>
          )}
          {defaultModelSuggestions.length > 0 && (
            <ModelDropdown
              models={defaultModelSuggestions}
              onSelect={(id) => onModelChange(id)}
            />
          )}
        </div>
      </div>
    );

  if (variant === "stack") {
    const showFormatFields = shouldShowSpeedTest && !isXaiOauthPreset;
    const showReasoning = isChatFormat && canEditReasoning;
    // Copilot 不显示 UA 和请求覆盖，Responses 格式下高级区就空了。
    const hasStackAdvancedContent =
      showFullUrlToggle ||
      (showFormatFields && isAnthropicFormat) ||
      showReasoning ||
      userAgentAndOverrides !== null;
    return (
      <>
        {oauthSections}
        {showFormatFields && upstreamFormatSelect}
        {apiKeySection}
        {showFormatFields && isAnthropicFormat && anthropicAuthFieldSelect}
        {endpointSection}

        {canEditCatalog && (
          <div className="space-y-3">
            <div className="space-y-1">
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-0.5">
                  <FormLabel>
                    {t("codexConfig.modelListTitle", {
                      defaultValue: "模型列表",
                    })}
                  </FormLabel>
                  <HelpTip
                    title={t("codexConfig.modelListTitle", {
                      defaultValue: "模型列表",
                    })}
                  >
                    {t("codexConfig.stackModelDefaultHelp", {
                      defaultValue:
                        "这些模型会出现在 Codex 的 /model 里，选中后请求直达这家。第一行是默认模型：这家被设为默认时，Codex 默认用它。",
                    })}
                  </HelpTip>
                </div>
                {renderCatalogActionButtons(
                  handleAddCatalogRow,
                  t("codexConfig.addCatalogModel", {
                    defaultValue: "手动添加",
                  }),
                )}
              </div>
              <p className="text-xs leading-relaxed text-fg-2">
                {t("codexConfig.modelListHint", {
                  defaultValue: "修改后需要重启 Codex。",
                })}
              </p>
            </div>
            {defaultModelNotInListNotice}
            {catalogRows.length > 0 ? (
              renderCatalogRows()
            ) : (
              <p className="text-xs leading-relaxed text-fg-2">
                {t("codexConfig.modelListEmpty", {
                  defaultValue: "未配置模型：聚合模式下只发布下面的默认模型。",
                })}
              </p>
            )}
          </div>
        )}
        {defaultModelField}

        {hasStackAdvancedContent && (
          <Collapsible
            open={stackAdvancedExpanded}
            onOpenChange={setStackAdvancedExpanded}
            className="rounded-lg border border-border p-4"
          >
            <CollapsibleTrigger asChild>
              <Button
                type="button"
                variant={null}
                size="sm"
                className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-fg-1 hover:opacity-70 active:scale-100"
              >
                {stackAdvancedExpanded ? (
                  <ChevronDown className="h-4 w-4" />
                ) : (
                  <ChevronRight className="h-4 w-4" />
                )}
                {t("providerForm.advancedOptionsToggle", {
                  defaultValue: "高级选项",
                })}
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="space-y-3 pt-3">
              {showFullUrlToggle && (
                <FullUrlToggle
                  checked={isFullUrl}
                  onCheckedChange={onFullUrlChange}
                />
              )}
              {showFormatFields &&
                isAnthropicFormat &&
                impersonateClaudeCodeToggle}
              {showFormatFields && isAnthropicFormat && maxOutputTokensField}
              {showReasoning && (
                <div
                  className={cn(
                    "space-y-3",
                    showFormatFields &&
                      isAnthropicFormat &&
                      "border-t border-border pt-3",
                  )}
                >
                  {reasoningFields}
                </div>
              )}
              {userAgentAndOverrides && (
                <div
                  className={cn(
                    "space-y-3",
                    (showFullUrlToggle ||
                      (showFormatFields && isAnthropicFormat) ||
                      showReasoning) &&
                      "border-t border-border pt-3",
                  )}
                >
                  {userAgentAndOverrides}
                </div>
              )}
            </CollapsibleContent>
          </Collapsible>
        )}

        {speedTestModal}
      </>
    );
  }

  return (
    <>
      {oauthSections}
      {apiKeySection}
      {endpointSection}

      {/* 模型列表 —— 生成 Codex 的模型目录（model_catalog_json）：/model 菜单列哪些模型，
          各自的窗口和思考等级。留空不生成。第一行是默认模型，子代理也从这里选。 */}
      {firstRowIsDefault && (
        <div className="space-y-3">
          <div className="space-y-1">
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-0.5">
                <FormLabel>
                  {t("codexConfig.modelListTitle", {
                    defaultValue: "模型列表",
                  })}
                </FormLabel>
                <HelpTip
                  title={t("codexConfig.modelListTitle", {
                    defaultValue: "模型列表",
                  })}
                >
                  {t("codexConfig.modelListDefaultHelp", {
                    defaultValue:
                      "这些模型会出现在 Codex 的 /model 里，第一行是默认模型。名字是官方 GPT 模型的行按官方条目生成，行里的显示名、窗口和思考等级不生效。",
                  })}
                </HelpTip>
              </div>
              {renderCatalogActionButtons(
                handleAddCatalogRow,
                t("codexConfig.addCatalogModel", {
                  defaultValue: "手动添加",
                }),
              )}
            </div>
            <p className="text-xs leading-relaxed text-fg-2">
              {t("codexConfig.modelListHint", {
                defaultValue: "修改后需要重启 Codex。",
              })}
            </p>
          </div>

          {defaultModelNotInListNotice}
          {catalogRows.length > 0 && renderCatalogRows()}
        </div>
      )}

      {/* 默认模型：列表为空（或没有列表）时才显示 */}
      {defaultModelField}

      {/* 子代理 —— config.toml 的 [agents] default_subagent_model / default_subagent_reasoning_effort。
          留空删键，子代理跟随主模型；聚合模式下不写进 live（改用聚合设置）。 */}
      {category !== "official" && onSubagentModelChange && (
        <div className="space-y-1.5">
          {labelWithHelp(
            t("codexConfig.subagentModelLabel", {
              defaultValue: "子代理模型",
            }),
            t("codexConfig.subagentModelHint", {
              defaultValue:
                "Codex 派出的子代理默认用的模型和推理强度。留空时沿用主代理的；只填模型时用该模型自己的默认强度。聚合模式下不生效，改用聚合页签「聚合设置」里的子代理模型。",
            }),
            "codexSubagentModel",
          )}
          <div className="flex gap-1">
            <Input
              id="codexSubagentModel"
              value={subagentModel}
              onChange={(event) => onSubagentModelChange(event.target.value)}
              placeholder={t("codexConfig.subagentModelPlaceholder", {
                defaultValue: "留空则跟随主模型",
              })}
              className="flex-1"
            />
            {defaultModelSuggestions.length > 0 && (
              <ModelDropdown
                models={defaultModelSuggestions}
                onSelect={(id) => onSubagentModelChange(id)}
              />
            )}
            {onSubagentReasoningEffortChange &&
              renderEffortSelect(
                subagentReasoningEffort,
                onSubagentReasoningEffortChange,
                t("codexConfig.subagentReasoningEffortLabel", {
                  defaultValue: "子代理推理强度",
                }),
                t("codexConfig.subagentReasoningEffortDefault", {
                  defaultValue: "默认强度",
                }),
                "w-36 shrink-0",
              )}
          </div>
          {isSubagentModelOutsideCatalog && (
            <p className="flex flex-wrap items-center gap-x-2 text-xs leading-relaxed text-fg-2">
              {t("codexConfig.subagentModelNotInCatalog", {
                defaultValue:
                  "该模型不在模型列表中：Codex 拿不到它的上下文窗口；经路由转成 Chat / Anthropic 协议时会被换成默认模型。",
              })}
              {onCatalogModelsChange && (
                <Button
                  type="button"
                  variant="link"
                  size="sm"
                  className="h-auto p-0 text-xs"
                  onClick={handleAddSubagentModelToCatalog}
                >
                  {t("codexConfig.addToModelList", {
                    defaultValue: "加入列表",
                  })}
                </Button>
              )}
            </p>
          )}
        </div>
      )}

      {/* 自动审批 —— 写进模型目录每一行的 auto_review_model_override，Codex 按主模型那一行
          选审批模型。只能选列表里的模型；聚合模式下不生效，改用聚合设置。 */}
      {category !== "official" && onAutoReviewModelChange && (
        <div className="space-y-1.5">
          {labelWithHelp(
            t("codexConfig.autoReviewModelLabel", {
              defaultValue: "自动审批模型",
            }),
            t("codexConfig.autoReviewModelHint", {
              defaultValue:
                "Codex 开了自动审批（Approve for me）时，判断操作能不能放行用的模型。审批请求和主对话发给同一家，所以只能选这张卡模型列表里的。聚合模式下不生效，改用聚合页签「聚合设置」里的自动审批模型。改完需要重启 Codex。",
            }),
            "codexAutoReviewModel",
          )}
          <Select
            value={autoReviewSelectValue}
            onValueChange={(next) => {
              // 表单里的 Radix Select 带一个隐藏的原生 select：编辑页加载时列表和已存的值同时
              // 到达，值先于选项注册，它会回报一次空串。选项都不是空值，空串只能是这个，忽略。
              if (!next) return;
              onAutoReviewModelChange(next === AUTO_REVIEW_DEFAULT ? "" : next);
            }}
            disabled={autoReviewChoices.length === 0}
          >
            <SelectTrigger
              id="codexAutoReviewModel"
              aria-label={t("codexConfig.autoReviewModelLabel", {
                defaultValue: "自动审批模型",
              })}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={AUTO_REVIEW_DEFAULT}>
                {autoReviewDefaultIsCodex
                  ? t("codexConfig.autoReviewModelCodexDefault", {
                      defaultValue: "Codex 默认（codex-auto-review）",
                    })
                  : t("codexConfig.autoReviewModelMainDefault", {
                      defaultValue: "主模型自己审批（默认）",
                    })}
              </SelectItem>
              {autoReviewChoices.map((choice) => (
                <SelectItem key={choice.model} value={choice.model}>
                  {choice.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {autoReviewChoices.length === 0 && (
            <p className="text-xs leading-relaxed text-fg-2">
              {t("codexConfig.autoReviewModelNoList", {
                defaultValue:
                  "这张卡没有模型列表：Codex 会用官方的 codex-auto-review 审批，只有部分转发 ChatGPT 账号的中转认它，不认就会报 404。在上面的模型列表里加上模型后就能选。",
              })}
            </p>
          )}
        </div>
      )}

      {/* 高级选项 —— 上游格式/思考能力/自定义 UA；预设供应商通常无需展开 */}
      {category !== "official" && (
        <Collapsible
          open={advancedExpanded}
          onOpenChange={setAdvancedExpanded}
          className="rounded-lg border border-border p-4"
        >
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-fg-1 hover:opacity-70 active:scale-100"
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
          <CollapsibleContent className="space-y-3 pt-3">
            {/* Copilot supports automatic capability routing or an explicit protocol;
                other providers retain their existing format controls. */}
            {(shouldShowSpeedTest || isCopilotPreset) && !isXaiOauthPreset && (
              <div className="space-y-3">
                {upstreamFormatSelect}
                {isAnthropicFormat && anthropicAuthFieldSelect}
                {isAnthropicFormat && impersonateClaudeCodeToggle}
                {isAnthropicFormat && maxOutputTokensField}
                {showFullUrlToggle && fullUrlToggle}
              </div>
            )}

            {isChatFormat && canEditReasoning && (
              <div
                className={cn(
                  "space-y-3",
                  shouldShowSpeedTest && "border-t border-border pt-3",
                )}
              >
                {reasoningFields}
              </div>
            )}

            {userAgentAndOverrides && (
              <div
                className={cn(
                  "space-y-3",
                  (shouldShowSpeedTest || (isChatFormat && canEditReasoning)) &&
                    "border-t border-border pt-3",
                )}
              >
                {userAgentAndOverrides}
              </div>
            )}
          </CollapsibleContent>
        </Collapsible>
      )}

      {speedTestModal}
    </>
  );
}
