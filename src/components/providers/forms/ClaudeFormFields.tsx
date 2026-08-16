import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
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
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Wand2,
  RefreshCw,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import {
  ApiKeySection,
  EndpointField,
  ModelDropdown,
  ModelInputWithFetch,
} from "./shared";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { CodexOAuthSection } from "./CodexOAuthSection";
import { XaiOAuthSection } from "./XaiOAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
} from "@/lib/api/copilot";
import type { CopilotModel } from "@/lib/api/copilot";
import {
  fetchCodexOauthModels,
  fetchXaiOauthModels,
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";
import type { ManagedAuthProvider } from "@/lib/api";
import {
  parseModelSuffix,
  readContextWindows,
  readContextWindowTokens,
  writeContextWindow,
  stripModelSuffix,
  parseWindowToken,
  type ClaudeModelEnvField,
} from "./hooks/useModelState";
import {
  providerPresets,
  type TemplateValueConfig,
} from "@/config/claudeProviderPresets";
import {
  applyAutoSyncCompactRatioSetting,
  applyAutoSyncContextWindowSetting,
  resolveAutoSyncContextWindow,
  AUTO_SYNC_COMPACT_RATIO_MIN,
  AUTO_SYNC_COMPACT_RATIO_MAX,
} from "@/utils/settingsConfigMerge";

interface EndpointCandidate {
  url: string;
}

function parseAutoSyncCompactRatio(config?: string): number | null {
  try {
    const parsed = JSON.parse(config ?? "{}") as Record<string, unknown>;
    const ratio = parsed.autoSyncCompactRatio;
    if (
      typeof ratio === "number" &&
      Number.isFinite(ratio) &&
      ratio >= AUTO_SYNC_COMPACT_RATIO_MIN &&
      ratio <= AUTO_SYNC_COMPACT_RATIO_MAX
    ) {
      return ratio;
    }
  } catch {
    // 忽略无效 JSON，按空值处理
  }
  return null;
}

function formatAutoSyncCompactRatio(ratio: number | null): string {
  return ratio === null ? "" : String(ratio);
}

function parseContextWindowInput(input: string): number | null {
  // 与 setModelSuffix + parseModelSuffix 等价：trim 后按纯数字/Kk/Mm 解析
  return parseWindowToken(input) ?? null;
}

function contextWindowInputValue(
  contextWindows: Record<string, number>,
  contextWindowTokens: Record<string, string>,
  roleEnvKey: string,
  model: string,
): string {
  const token = contextWindowTokens[roleEnvKey];
  if (token) return token;
  const window = contextWindows[roleEnvKey] ?? parseModelSuffix(model).window;
  return window ? String(window) : "";
}

interface ClaudeFormFieldsProps {
  providerId?: string;
  // API Key
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // GitHub Copilot OAuth
  isCopilotPreset?: boolean;
  usesOAuth?: boolean;
  isCopilotAuthenticated?: boolean;
  /** 当前选中的 GitHub 账号 ID（多账号支持） */
  selectedGitHubAccountId?: string | null;
  /** GitHub 账号选择回调（多账号支持） */
  onGitHubAccountSelect?: (accountId: string | null) => void;
  /** 打开托管账号管理入口 */
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;

  // Codex OAuth (ChatGPT Plus/Pro)
  isCodexOauthPreset?: boolean;
  isCodexOauthAuthenticated?: boolean;
  selectedCodexAccountId?: string | null;
  onCodexAccountSelect?: (accountId: string | null) => void;
  codexFastMode?: boolean;
  onCodexFastModeChange?: (enabled: boolean) => void;

  // xAI OAuth
  isXaiOauthPreset?: boolean;
  isXaiOauthAuthenticated?: boolean;
  selectedXaiAccountId?: string | null;
  onXaiAccountSelect?: (accountId: string | null) => void;

  // Template Values
  templateValueEntries: Array<[string, TemplateValueConfig]>;
  templateValues: Record<string, TemplateValueConfig>;
  templatePresetName: string;
  onTemplateValueChange: (key: string, value: string) => void;

  // Base URL
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;
  showEndpointTools?: boolean;

  // Model Selector
  shouldShowModelSelector: boolean;
  claudeModel: string;
  defaultHaikuModel: string;
  defaultHaikuModelName: string;
  defaultSonnetModel: string;
  defaultSonnetModelName: string;
  defaultOpusModel: string;
  defaultOpusModelName: string;
  defaultFableModel: string;
  defaultFableModelName: string;
  subagentModel: string;
  onModelChange: (field: ClaudeModelEnvField, value: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // API Format (for Claude-compatible providers that need request/response conversion)
  apiFormat: ClaudeApiFormat;
  onApiFormatChange: (format: ClaudeApiFormat) => void;

  // Auth Field (ANTHROPIC_AUTH_TOKEN or ANTHROPIC_API_KEY)
  apiKeyField: ClaudeApiKeyField;
  onApiKeyFieldChange: (field: ClaudeApiKeyField) => void;

  // Full URL mode
  isFullUrl: boolean;
  onFullUrlChange: (value: boolean) => void;

  // Local proxy User-Agent override
  customUserAgent: string;
  onCustomUserAgentChange: (value: string) => void;
  localProxyHeadersOverride: string;
  onLocalProxyHeadersOverrideChange: (value: string) => void;
  localProxyBodyOverride: string;
  onLocalProxyBodyOverrideChange: (value: string) => void;
  /** 路由（代理接管）模式：直连解冻后自动同步开关不再按模式门控，该 prop 仅保留 API 兼容 */
  isProxyTakeover?: boolean;
  settingsConfig?: string;
  onSettingsConfigChange?: (config: string) => void;
  onAutoSyncContextWindowChange?: (checked: boolean) => void;
  onAutoSyncCompactRatioChange?: (ratio: number | null) => void;
}

export function ClaudeFormFields({
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  isCopilotPreset,
  usesOAuth,
  isCopilotAuthenticated,
  selectedGitHubAccountId,
  onGitHubAccountSelect,
  onManageAuthAccounts,
  isCodexOauthPreset,
  isCodexOauthAuthenticated,
  selectedCodexAccountId,
  onCodexAccountSelect,
  codexFastMode,
  onCodexFastModeChange,
  isXaiOauthPreset,
  isXaiOauthAuthenticated,
  selectedXaiAccountId,
  onXaiAccountSelect,
  templateValueEntries,
  templateValues,
  templatePresetName,
  onTemplateValueChange,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  showEndpointTools = true,
  shouldShowModelSelector,
  claudeModel,
  defaultHaikuModel,
  defaultHaikuModelName,
  defaultSonnetModel,
  defaultSonnetModelName,
  defaultOpusModel,
  defaultOpusModelName,
  defaultFableModel,
  defaultFableModelName,
  subagentModel,
  onModelChange,
  speedTestEndpoints,
  apiFormat,
  onApiFormatChange,
  apiKeyField,
  onApiKeyFieldChange,
  isFullUrl,
  onFullUrlChange,
  customUserAgent,
  onCustomUserAgentChange,
  localProxyHeadersOverride,
  onLocalProxyHeadersOverrideChange,
  localProxyBodyOverride,
  onLocalProxyBodyOverrideChange,
  // 自动同步已直连解冻，不再按接管状态门控；保留 prop 以兼容 ProviderForm/测试传参。
  isProxyTakeover: _isProxyTakeover = false,
  settingsConfig,
  onSettingsConfigChange,
  onAutoSyncContextWindowChange,
  onAutoSyncCompactRatioChange,
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();
  const hasRequestOverrides = Boolean(
    localProxyHeadersOverride.trim() || localProxyBodyOverride.trim(),
  );
  const hasAnyAdvancedValue = !!(
    claudeModel ||
    defaultHaikuModel ||
    defaultSonnetModel ||
    defaultOpusModel ||
    defaultFableModel ||
    subagentModel ||
    (!isXaiOauthPreset && apiFormat !== "anthropic") ||
    apiKeyField !== "ANTHROPIC_AUTH_TOKEN" ||
    customUserAgent ||
    hasRequestOverrides
  );
  const [advancedExpanded, setAdvancedExpanded] = useState(
    isXaiOauthPreset ? false : hasAnyAdvancedValue,
  );
  // 自动同步上下文窗口开关
  // 用 local state 而非直接从 settingsConfig 派生：父组件用 form.setValue
  // 更新 settingsConfig，但 react-hook-form 的 setValue 默认不触发 re-render，
  // 直接派生会导致 Switch 点击后 checked 不更新（看起来"不能切换"）。
  // local state 立即响应点击，useEffect 在 settingsConfig 外部变化时同步。
  const [autoSyncContextWindow, setAutoSyncContextWindow] = useState(() =>
    resolveAutoSyncContextWindow(settingsConfig ?? "{}"),
  );

  useEffect(() => {
    setAutoSyncContextWindow(
      resolveAutoSyncContextWindow(settingsConfig ?? "{}"),
    );
  }, [settingsConfig]);

  // 每行窗口输入框共用一次解析，避免每次渲染整份 settingsConfig JSON.parse
  const contextWindows = useMemo(
    () => readContextWindows(settingsConfig ?? "{}"),
    [settingsConfig],
  );
  // display-only 原样串（200k / 2m）：重开编辑页时优先回显，数字解析值在 contextWindows。
  const contextWindowTokens = useMemo(
    () => readContextWindowTokens(settingsConfig ?? "{}"),
    [settingsConfig],
  );

  // 上下文长度输入框：输入过程中保留用户原样字符串（支持 200k / 2m），
  // 失焦时才解析写入 contextWindows；未输入时回退到已存值显示。
  const [rawWindowInput, setRawWindowInput] = useState<Record<string, string>>(
    {},
  );

  const [autoSyncCompactRatioInput, setAutoSyncCompactRatioInput] = useState(
    () => formatAutoSyncCompactRatio(parseAutoSyncCompactRatio(settingsConfig)),
  );
  const [autoSyncCompactRatioError, setAutoSyncCompactRatioError] =
    useState("");

  useEffect(() => {
    setAutoSyncCompactRatioInput(
      formatAutoSyncCompactRatio(parseAutoSyncCompactRatio(settingsConfig)),
    );
    setAutoSyncCompactRatioError("");
  }, [settingsConfig]);

  const handleAutoSyncCompactRatioChange = useCallback(
    (value: string) => {
      setAutoSyncCompactRatioInput(value);
      if (value.trim() === "") {
        setAutoSyncCompactRatioError("");
        if (onAutoSyncCompactRatioChange) {
          onAutoSyncCompactRatioChange(null);
        } else if (onSettingsConfigChange) {
          onSettingsConfigChange(
            applyAutoSyncCompactRatioSetting(settingsConfig ?? "{}", null),
          );
        }
        return;
      }
      const parsed = Number(value);
      if (
        Number.isFinite(parsed) &&
        parsed >= AUTO_SYNC_COMPACT_RATIO_MIN &&
        parsed <= AUTO_SYNC_COMPACT_RATIO_MAX
      ) {
        setAutoSyncCompactRatioError("");
        if (onAutoSyncCompactRatioChange) {
          onAutoSyncCompactRatioChange(parsed);
        } else if (onSettingsConfigChange) {
          onSettingsConfigChange(
            applyAutoSyncCompactRatioSetting(settingsConfig ?? "{}", parsed),
          );
        }
      } else {
        setAutoSyncCompactRatioError(
          t("providerForm.autoSyncCompactRatioInvalid", {
            defaultValue: "压缩比例必须是 0.2~0.95 之间的数字",
          }),
        );
      }
    },
    [onAutoSyncCompactRatioChange, onSettingsConfigChange, settingsConfig, t],
  );

  const handleAutoSyncCompactRatioBlur = useCallback(() => {
    if (!autoSyncCompactRatioError) return;
    setAutoSyncCompactRatioInput(
      formatAutoSyncCompactRatio(parseAutoSyncCompactRatio(settingsConfig)),
    );
    setAutoSyncCompactRatioError("");
  }, [autoSyncCompactRatioError, settingsConfig]);

  const handleAutoSyncChange = useCallback(
    (checked: boolean) => {
      // 立即更新 local state，让 Switch 视觉响应
      setAutoSyncContextWindow(checked);
      if (onAutoSyncContextWindowChange) {
        onAutoSyncContextWindowChange(checked);
        return;
      }
      if (!onSettingsConfigChange) return;
      onSettingsConfigChange(
        applyAutoSyncContextWindowSetting(settingsConfig ?? "{}", checked),
      );
    },
    [settingsConfig, onSettingsConfigChange, onAutoSyncContextWindowChange],
  );

  const handleContextWindowChange = useCallback(
    (roleEnvKey: string, input: string, token: string | null) => {
      if (!onSettingsConfigChange) return;
      const trimmed = input.trim();
      if (!trimmed) {
        onSettingsConfigChange(
          writeContextWindow(settingsConfig ?? "{}", roleEnvKey, null, null),
        );
        return;
      }
      const window = parseContextWindowInput(input);
      if (window === null) return;
      onSettingsConfigChange(
        writeContextWindow(settingsConfig ?? "{}", roleEnvKey, window, token),
      );
    },
    [settingsConfig, onSettingsConfigChange],
  );

  const commitWindow = useCallback(
    (field: ClaudeModelEnvField, raw: string) => {
      const trimmed = raw.trim();
      const window = trimmed ? parseContextWindowInput(trimmed) : null;
      const accepted = !trimmed || window !== null;
      if (accepted) {
        // 合法输入：原样串随解析值一起持久化（display-only token），
        // 保存重开后仍原样回显，不再被数字替换。
        handleContextWindowChange(
          field,
          trimmed,
          trimmed && window !== null ? trimmed : null,
        );
      }
      setRawWindowInput((prev) => {
        // 提交后统一删除会话内 raw：显示回退到刚写入的 contextWindowTokens
        // 原样串（值一致）；空或非法输入则回退到已存值显示。
        const next = { ...prev };
        delete next[field];
        return next;
      });
    },
    [handleContextWindowChange],
  );

  // 回车只结束该输入框的编辑状态（等价于失焦提交解析），不触发表单整体保存。
  const commitContextWindowFromKeyDown = useCallback(
    (
      event: ReactKeyboardEvent<HTMLInputElement>,
      field: ClaudeModelEnvField,
    ) => {
      if (event.key !== "Enter") return;
      event.preventDefault();
      event.stopPropagation();
      const raw = rawWindowInput[field];
      if (raw !== undefined) {
        commitWindow(field, raw);
      }
      event.currentTarget.blur();
    },
    [commitWindow, rawWindowInput],
  );

  const clearRawWindowInput = useCallback((field: ClaudeModelEnvField) => {
    setRawWindowInput((prev) => {
      if (!(field in prev)) return prev;
      const next = { ...prev };
      delete next[field];
      return next;
    });
  }, []);

  // 预设填充高级值后自动展开（仅从折叠→展开，不会自动折叠）
  useEffect(() => {
    if (isXaiOauthPreset) {
      setAdvancedExpanded(false);
    } else if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue, isXaiOauthPreset]);

  // Copilot 可用模型列表
  const [copilotModels, setCopilotModels] = useState<CopilotModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const copilotModelsRequestRef = useRef(0);

  // Codex OAuth 可用模型列表
  const [codexOauthModels, setCodexOauthModels] = useState<FetchedModel[]>([]);
  const [codexOauthModelsLoading, setCodexOauthModelsLoading] = useState(false);
  const codexOauthModelsRequestRef = useRef(0);

  const [xaiOauthModels, setXaiOauthModels] = useState<FetchedModel[]>([]);
  const [xaiOauthModelsLoading, setXaiOauthModelsLoading] = useState(false);
  const xaiOauthModelsRequestRef = useRef(0);

  // 通用模型获取（非 Copilot 供应商）
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const showModelFetchResult = useCallback(
    (count: number) => {
      if (count === 0) {
        toast.info(t("providerForm.fetchModelsEmpty"));
      } else {
        toast.success(t("providerForm.fetchModelsSuccess", { count }));
      }
    },
    [t],
  );

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    // 当 baseURL 仍是某预设的默认值时，优先使用预设上的 modelsUrl 覆写
    // 避免多走一次失败的候选请求（如 DeepSeek 把 /models 挂在根，而不是 /anthropic 子路径下）
    const matchedPreset = providerPresets.find((p) => {
      const env = (p.settingsConfig as { env?: Record<string, string> })?.env;
      return env?.ANTHROPIC_BASE_URL === baseUrl;
    });
    const modelsUrl = matchedPreset?.modelsUrl;

    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey, isFullUrl, modelsUrl, customUserAgent)
      .then((models) => {
        setFetchedModels(models);
        showModelFetchResult(models.length);
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, isFullUrl, customUserAgent, showModelFetchResult, t]);

  const handleFetchCopilotModels = useCallback(() => {
    if (!isCopilotAuthenticated) {
      toast.error(
        t("copilot.loginRequired", {
          defaultValue: "请先登录 GitHub Copilot",
        }),
      );
      return;
    }

    const requestId = copilotModelsRequestRef.current + 1;
    copilotModelsRequestRef.current = requestId;
    setModelsLoading(true);
    const fetchModels = selectedGitHubAccountId
      ? copilotGetModelsForAccount(selectedGitHubAccountId)
      : copilotGetModels();

    fetchModels
      .then((models) => {
        if (copilotModelsRequestRef.current !== requestId) return;
        setCopilotModels(models);
        showModelFetchResult(models.length);
      })
      .catch((err) => {
        if (copilotModelsRequestRef.current !== requestId) return;
        console.warn("[Copilot] Failed to fetch models:", err);
        toast.error(
          t("copilot.loadModelsFailed", {
            defaultValue: "加载 Copilot 模型列表失败",
          }),
        );
      })
      .finally(() => {
        if (copilotModelsRequestRef.current === requestId) {
          setModelsLoading(false);
        }
      });
  }, [
    isCopilotAuthenticated,
    selectedGitHubAccountId,
    showModelFetchResult,
    t,
  ]);

  const handleFetchCodexOauthModels = useCallback(() => {
    if (!isCodexOauthAuthenticated) {
      toast.error(
        t("codexOauth.loginRequired", {
          defaultValue: "请先登录 ChatGPT 账号",
        }),
      );
      return;
    }

    const requestId = codexOauthModelsRequestRef.current + 1;
    codexOauthModelsRequestRef.current = requestId;
    setCodexOauthModelsLoading(true);
    fetchCodexOauthModels(selectedCodexAccountId)
      .then((models) => {
        if (codexOauthModelsRequestRef.current !== requestId) return;
        setCodexOauthModels(models);
        showModelFetchResult(models.length);
      })
      .catch((err) => {
        if (codexOauthModelsRequestRef.current !== requestId) return;
        console.warn("[CodexOAuth] Failed to fetch models:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (codexOauthModelsRequestRef.current === requestId) {
          setCodexOauthModelsLoading(false);
        }
      });
  }, [
    isCodexOauthAuthenticated,
    selectedCodexAccountId,
    showModelFetchResult,
    t,
  ]);

  const handleFetchXaiOauthModels = useCallback(() => {
    if (!isXaiOauthAuthenticated) {
      toast.error(
        t("xaiOauth.loginRequired", {
          defaultValue: "请先登录 xAI 账号",
        }),
      );
      return;
    }

    const requestId = xaiOauthModelsRequestRef.current + 1;
    xaiOauthModelsRequestRef.current = requestId;
    setXaiOauthModelsLoading(true);
    fetchXaiOauthModels(selectedXaiAccountId)
      .then((models) => {
        if (xaiOauthModelsRequestRef.current !== requestId) return;
        setXaiOauthModels(models);
        showModelFetchResult(models.length);
      })
      .catch((err) => {
        if (xaiOauthModelsRequestRef.current !== requestId) return;
        console.warn("[XaiOAuth] Failed to fetch models:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (xaiOauthModelsRequestRef.current === requestId) {
          setXaiOauthModelsLoading(false);
        }
      });
  }, [isXaiOauthAuthenticated, selectedXaiAccountId, showModelFetchResult, t]);

  useEffect(() => {
    copilotModelsRequestRef.current += 1;
    setCopilotModels([]);
    setModelsLoading(false);
  }, [isCopilotPreset, isCopilotAuthenticated, selectedGitHubAccountId]);

  useEffect(() => {
    codexOauthModelsRequestRef.current += 1;
    setCodexOauthModels([]);
    setCodexOauthModelsLoading(false);
  }, [isCodexOauthPreset, isCodexOauthAuthenticated, selectedCodexAccountId]);

  useEffect(() => {
    xaiOauthModelsRequestRef.current += 1;
    setXaiOauthModels([]);
    setXaiOauthModelsLoading(false);
  }, [isXaiOauthPreset, isXaiOauthAuthenticated, selectedXaiAccountId]);

  const modelFetchLoading = isCopilotPreset
    ? modelsLoading
    : isCodexOauthPreset
      ? codexOauthModelsLoading
      : isXaiOauthPreset
        ? xaiOauthModelsLoading
        : isFetchingModels;
  const handleModelFetchClick = isCopilotPreset
    ? handleFetchCopilotModels
    : isCodexOauthPreset
      ? handleFetchCodexOauthModels
      : isXaiOauthPreset
        ? handleFetchXaiOauthModels
        : handleFetchModels;

  // 模型输入框：支持手动输入 + 下拉选择
  const renderModelInput = (
    id: string,
    value: string,
    field: ClaudeModelEnvField,
    placeholder?: string,
    onValueChange?: (value: string) => void,
  ) => {
    const updateValue =
      onValueChange ?? ((next: string) => onModelChange(field, next));

    if (isCodexOauthPreset) {
      return (
        <ModelInputWithFetch
          id={id}
          value={value}
          onChange={updateValue}
          placeholder={placeholder}
          fetchedModels={codexOauthModels}
          isLoading={codexOauthModelsLoading}
        />
      );
    }

    if (isXaiOauthPreset) {
      return (
        <ModelInputWithFetch
          id={id}
          value={value}
          onChange={updateValue}
          placeholder={placeholder}
          fetchedModels={xaiOauthModels}
          isLoading={xaiOauthModelsLoading}
        />
      );
    }

    if (isCopilotPreset && copilotModels.length > 0) {
      // Reuse the searchable dropdown by mapping Copilot models to FetchedModel.
      const copilotFetchedModels: FetchedModel[] = copilotModels.map((m) => ({
        id: m.id,
        ownedBy: m.vendor || null,
      }));

      return (
        <div className="flex gap-1">
          <Input
            id={id}
            type="text"
            value={value}
            onChange={(e) => updateValue(e.target.value)}
            placeholder={placeholder}
            autoComplete="off"
            className="flex-1"
          />
          <ModelDropdown models={copilotFetchedModels} onSelect={updateValue} />
        </div>
      );
    }

    if (isCopilotPreset && modelsLoading) {
      return (
        <div className="flex gap-1">
          <Input
            id={id}
            type="text"
            value={value}
            onChange={(e) => updateValue(e.target.value)}
            placeholder={placeholder}
            autoComplete="off"
            className="flex-1"
          />
          <Button variant="outline" size="icon" className="shrink-0" disabled>
            <Loader2 className="h-4 w-4 animate-spin" />
          </Button>
        </div>
      );
    }

    if (isCopilotPreset) {
      return (
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => updateValue(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
        />
      );
    }

    // 普通供应商: 使用 ModelInputWithFetch（获取按钮在 section 标题旁）
    return (
      <ModelInputWithFetch
        id={id}
        value={value}
        onChange={updateValue}
        placeholder={placeholder}
        fetchedModels={fetchedModels}
        isLoading={isFetchingModels}
      />
    );
  };

  type ModelRoleRow = {
    role: "sonnet" | "opus" | "fable" | "haiku" | "subagent";
    label: string;
    model: string;
    displayName?: string;
    modelField: ClaudeModelEnvField;
    displayNameField?: ClaudeModelEnvField;
    inputId: string;
    supportsContextWindow: boolean;
  };

  const modelRoleRows: ModelRoleRow[] = [
    {
      role: "sonnet",
      label: t("providerForm.modelRoleSonnet", { defaultValue: "Sonnet" }),
      model: defaultSonnetModel,
      displayName: defaultSonnetModelName,
      modelField: "ANTHROPIC_DEFAULT_SONNET_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
      inputId: "claudeDefaultSonnetModel",
      supportsContextWindow: true,
    },
    {
      role: "opus",
      label: t("providerForm.modelRoleOpus", { defaultValue: "Opus" }),
      model: defaultOpusModel,
      displayName: defaultOpusModelName,
      modelField: "ANTHROPIC_DEFAULT_OPUS_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
      inputId: "claudeDefaultOpusModel",
      supportsContextWindow: true,
    },
    {
      role: "fable",
      label: t("providerForm.modelRoleFable", { defaultValue: "Fable" }),
      model: defaultFableModel,
      displayName: defaultFableModelName,
      modelField: "ANTHROPIC_DEFAULT_FABLE_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
      inputId: "claudeDefaultFableModel",
      supportsContextWindow: true,
    },
    {
      role: "haiku",
      label: t("providerForm.modelRoleHaiku", { defaultValue: "Haiku" }),
      model: defaultHaikuModel,
      displayName: defaultHaikuModelName,
      modelField: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
      inputId: "claudeDefaultHaikuModel",
      supportsContextWindow: true,
    },
    {
      role: "subagent",
      label: t("providerForm.modelRoleSubagent", {
        defaultValue: "Subagent",
      }),
      model: subagentModel,
      modelField: "CLAUDE_CODE_SUBAGENT_MODEL",
      inputId: "claudeCodeSubagentModel",
      supportsContextWindow: true,
    },
  ];

  const handleRoleModelChange = (row: ModelRoleRow, value: string) => {
    const oldModelBase = stripModelSuffix(row.model).trim();
    // 模型字段保留用户原样输入（可含 [200k] 后缀，保存路径负责剥离迁移）；
    // display-name 始终同步干净模型名（不带窗口后缀）。
    const nextModel = value.trim();
    const displayName = row.displayName?.trim() ?? "";
    const shouldSyncDisplayName = !displayName || displayName === oldModelBase;
    onModelChange(row.modelField, nextModel);
    // 模型名变化后窗口值可能随之变化（后缀迁移），清掉该角色的原样输入缓存。
    if (nextModel !== row.model) {
      clearRawWindowInput(row.modelField);
    }
    if (row.displayNameField && shouldSyncDisplayName) {
      onModelChange(row.displayNameField, stripModelSuffix(nextModel).trim());
    }
  };

  return (
    <>
      {/* GitHub Copilot OAuth 认证 */}
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

      {/* Codex OAuth 认证 (ChatGPT Plus/Pro) */}
      {isCodexOauthPreset && (
        <CodexOAuthSection
          mode="select"
          selectedAccountId={selectedCodexAccountId}
          onAccountSelect={onCodexAccountSelect}
          onManageAccounts={
            onManageAuthAccounts
              ? () => onManageAuthAccounts("codex_oauth")
              : undefined
          }
          fastModeEnabled={codexFastMode}
          onFastModeChange={onCodexFastModeChange}
        />
      )}

      {isXaiOauthPreset && (
        <XaiOAuthSection
          selectedAccountId={selectedXaiAccountId}
          onAccountSelect={onXaiAccountSelect}
        />
      )}

      {/* API Key 输入框（非 OAuth 预设时显示） */}
      {shouldShowApiKey && !usesOAuth && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}

      {/* 模板变量输入 */}
      {templateValueEntries.length > 0 && (
        <div className="space-y-3">
          <FormLabel>
            {t("providerForm.parameterConfig", {
              name: templatePresetName,
              defaultValue: `${templatePresetName} 参数配置`,
            })}
          </FormLabel>
          <div className="space-y-4">
            {templateValueEntries.map(([key, config]) => (
              <div key={key} className="space-y-2">
                <FormLabel htmlFor={`template-${key}`}>
                  {config.label}
                </FormLabel>
                <Input
                  id={`template-${key}`}
                  type="text"
                  required
                  value={
                    templateValues[key]?.editorValue ??
                    config.editorValue ??
                    config.defaultValue ??
                    ""
                  }
                  onChange={(e) => onTemplateValueChange(key, e.target.value)}
                  placeholder={config.placeholder || config.label}
                  autoComplete="off"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Base URL 输入框 */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder")}
          hint={
            apiFormat === "openai_responses"
              ? t("providerForm.apiHintResponses")
              : apiFormat === "openai_chat"
                ? t("providerForm.apiHintOAI")
                : apiFormat === "gemini_native"
                  ? t("providerForm.apiHintGeminiNative")
                  : t("providerForm.apiHint")
          }
          fullUrlHint={
            apiFormat === "gemini_native"
              ? t("providerForm.fullUrlHintGeminiNative")
              : undefined
          }
          showManageButton={showEndpointTools}
          onManageClick={
            showEndpointTools ? () => onEndpointModalToggle(true) : undefined
          }
          showFullUrlToggle={showEndpointTools && !isXaiOauthPreset}
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
        />
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && showEndpointTools && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="claude"
          providerId={providerId}
          value={baseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}

      {shouldShowModelSelector && (
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
              {t("providerForm.advancedOptionsToggle")}
            </Button>
          </CollapsibleTrigger>
          {!advancedExpanded && (
            <p className="text-xs text-muted-foreground mt-1 ml-1">
              {t("providerForm.advancedOptionsHint")}
            </p>
          )}
          <CollapsibleContent className="space-y-4 pt-2">
            {/* 上游格式选择（仅非云服务商显示） */}
            {category !== "cloud_provider" && !isXaiOauthPreset && (
              <div className="space-y-2">
                <FormLabel htmlFor="apiFormat">
                  {t("providerForm.apiFormat", { defaultValue: "上游格式" })}
                </FormLabel>
                <Select value={apiFormat} onValueChange={onApiFormatChange}>
                  <SelectTrigger id="apiFormat" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="anthropic">
                      {t("providerForm.apiFormatAnthropic", {
                        defaultValue: "Anthropic Messages (原生)",
                      })}
                    </SelectItem>
                    <SelectItem value="openai_chat">
                      {t("providerForm.apiFormatOpenAIChat", {
                        defaultValue: "OpenAI Chat Completions (需转换)",
                      })}
                    </SelectItem>
                    <SelectItem value="openai_responses">
                      {t("providerForm.apiFormatOpenAIResponses", {
                        defaultValue: "OpenAI Responses API (需转换)",
                      })}
                    </SelectItem>
                    <SelectItem value="gemini_native">
                      {t("providerForm.apiFormatGeminiNative", {
                        defaultValue: "Gemini Native generateContent (需转换)",
                      })}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {t("providerForm.apiFormatHint", {
                    defaultValue:
                      "供应商原生为 Anthropic Messages API 就选 Anthropic Messages（直连，不转换格式）；使用 Chat Completions 协议就选 Chat；使用 Responses API 就选 Responses；使用 Gemini generateContent 协议就选 Gemini Native。Chat、Responses 与 Gemini Native 均需开启路由接管才能转换为 Anthropic Messages。",
                  })}
                </p>
              </div>
            )}

            {/* 认证字段选择器 */}
            <div className="space-y-2">
              <FormLabel>
                {t("providerForm.authField", { defaultValue: "认证字段" })}
              </FormLabel>
              <Select
                value={apiKeyField}
                onValueChange={(v) =>
                  onApiKeyFieldChange(v as ClaudeApiKeyField)
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                    {t("providerForm.authFieldAuthToken", {
                      defaultValue: "ANTHROPIC_AUTH_TOKEN（默认）",
                    })}
                  </SelectItem>
                  <SelectItem value="ANTHROPIC_API_KEY">
                    {t("providerForm.authFieldApiKey", {
                      defaultValue: "ANTHROPIC_API_KEY",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.authFieldHint", {
                  defaultValue: "选择写入配置的认证环境变量名",
                })}
              </p>
            </div>

            {/* 模型映射 */}
            <div className="space-y-1 border-t border-border-default pt-2">
              <div className="flex items-center justify-between">
                <FormLabel>{t("providerForm.modelMappingLabel")}</FormLabel>
                <div className="flex gap-2">
                  {/* 一键设置按钮 */}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      const value =
                        claudeModel ||
                        defaultSonnetModel ||
                        defaultOpusModel ||
                        defaultFableModel ||
                        defaultHaikuModel ||
                        subagentModel;
                      if (value) {
                        for (const row of modelRoleRows) {
                          const roleValue = stripModelSuffix(value);
                          onModelChange(row.modelField, roleValue);
                          if (row.displayNameField) {
                            onModelChange(
                              row.displayNameField,
                              stripModelSuffix(roleValue),
                            );
                          }
                        }
                        toast.success(
                          t("providerForm.quickSetSuccess", {
                            defaultValue: "已将模型名称应用到所有角色",
                          }),
                        );
                      }
                    }}
                    disabled={
                      !claudeModel &&
                      !defaultHaikuModel &&
                      !defaultSonnetModel &&
                      !defaultOpusModel &&
                      !defaultFableModel &&
                      !subagentModel
                    }
                    className="h-7 gap-1"
                  >
                    <Wand2 className="h-3.5 w-3.5" />
                    {t("providerForm.quickSetModels", {
                      defaultValue: "一键设置",
                    })}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleModelFetchClick}
                    disabled={modelFetchLoading}
                    className="h-7 gap-1"
                  >
                    {modelFetchLoading ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Download className="h-3.5 w-3.5" />
                    )}
                    {t("providerForm.fetchModels")}
                  </Button>
                </div>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.modelMappingHint")}
              </p>
            </div>

            <div className="space-y-3">
              <div className="hidden grid-cols-[120px_1fr_minmax(0,1fr)_104px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                <span>
                  {t("providerForm.modelRoleLabel", {
                    defaultValue: "模型角色",
                  })}
                </span>
                <span>
                  {t("providerForm.modelDisplayNameLabel", {
                    defaultValue: "显示名称",
                  })}
                </span>
                <span>
                  {t("providerForm.requestModelLabel", {
                    defaultValue: "实际请求模型",
                  })}
                </span>
                <span>
                  {t("providerForm.modelOneMHeader", {
                    defaultValue: "声明支持 1M",
                  })}
                </span>
              </div>

              {modelRoleRows.map((row) => {
                const modelBase = stripModelSuffix(row.model);

                return (
                  <div
                    key={row.role}
                    className="grid grid-cols-1 gap-2 md:grid-cols-[120px_1fr_minmax(0,1fr)_104px]"
                  >
                    <div className="flex h-9 items-center rounded-md border border-input bg-muted px-3 text-sm font-medium text-muted-foreground">
                      {row.label}
                    </div>
                    {row.displayNameField ? (
                      <Input
                        value={row.displayName ?? ""}
                        onChange={(event) =>
                          onModelChange(
                            row.displayNameField!,
                            event.target.value,
                          )
                        }
                        placeholder={
                          modelBase ||
                          t("providerForm.modelDisplayNamePlaceholder", {
                            defaultValue: "例如 DeepSeek V4 Pro",
                          })
                        }
                        autoComplete="off"
                      />
                    ) : (
                      <div className="flex h-9 items-center rounded-md border border-input bg-muted px-3 text-sm text-muted-foreground">
                        {t("providerForm.modelNoDisplayName", {
                          defaultValue: "不显示在 /model 菜单",
                        })}
                      </div>
                    )}
                    {renderModelInput(
                      row.inputId,
                      row.model,
                      row.modelField,
                      t("providerForm.modelPlaceholder", { defaultValue: "" }),
                      (value) => handleRoleModelChange(row, value),
                    )}
                    {row.supportsContextWindow && (
                      <Input
                        inputMode="text"
                        className="w-[90px] text-center font-mono text-sm"
                        value={
                          rawWindowInput[row.modelField] ??
                          contextWindowInputValue(
                            contextWindows,
                            contextWindowTokens,
                            row.modelField,
                            row.model,
                          )
                        }
                        onChange={(event) => {
                          // 输入中原样显示（支持 K/M），失焦时再解析写入。
                          setRawWindowInput((prev) => ({
                            ...prev,
                            [row.modelField]: event.target.value,
                          }));
                        }}
                        onBlur={() => {
                          const raw = rawWindowInput[row.modelField];
                          if (raw === undefined) return;
                          commitWindow(row.modelField, raw);
                        }}
                        onKeyDown={(event) =>
                          commitContextWindowFromKeyDown(event, row.modelField)
                        }
                        placeholder={t(
                          "providerForm.modelContextWindowPlaceholder",
                          {
                            defaultValue: "1M / 200K",
                          },
                        )}
                        aria-label={t("providerForm.modelContextWindowLabel", {
                          defaultValue: "Context Window",
                        })}
                      />
                    )}
                  </div>
                );
              })}
            </div>

            <div className="mt-3 flex items-center gap-2 rounded-full border border-border/70 bg-muted/30 px-2.5 py-1 w-fit">
              <RefreshCw className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-xs font-medium text-foreground">
                {t("providerForm.autoSyncContextWindow", {
                  defaultValue: "自动同步模型上下文长度",
                })}
              </span>
              <Switch
                checked={autoSyncContextWindow}
                onCheckedChange={handleAutoSyncChange}
                aria-label={t("providerForm.autoSyncContextWindow", {
                  defaultValue: "自动同步模型上下文长度",
                })}
                className="h-5 w-9"
              />
              <label
                htmlFor="auto-sync-compact-ratio"
                className="text-xs text-muted-foreground"
              >
                {t("providerForm.autoSyncCompactRatio", {
                  defaultValue: "压缩比例",
                })}
              </label>
              <Input
                id="auto-sync-compact-ratio"
                type="number"
                min={AUTO_SYNC_COMPACT_RATIO_MIN}
                max={AUTO_SYNC_COMPACT_RATIO_MAX}
                step={0.05}
                className="h-7 w-20 text-center"
                value={autoSyncCompactRatioInput}
                onChange={(event) =>
                  handleAutoSyncCompactRatioChange(event.target.value)
                }
                onBlur={handleAutoSyncCompactRatioBlur}
                disabled={!autoSyncContextWindow}
                aria-label={t("providerForm.autoSyncCompactRatio", {
                  defaultValue: "压缩比例",
                })}
              />
            </div>
            {autoSyncCompactRatioError && (
              <p className="mt-1.5 ml-1 text-xs text-red-500">
                {autoSyncCompactRatioError}
              </p>
            )}
            <p className="mt-1.5 ml-1 text-xs leading-relaxed text-muted-foreground">
              {t(
                autoSyncContextWindow
                  ? "providerForm.autoSyncContextWindowTooltipOn"
                  : "providerForm.autoSyncContextWindowTooltipOff",
                {
                  defaultValue: autoSyncContextWindow
                    ? "上下文长度和压缩阈值按切换的模型更新配置 json。切换后需重启 Claude Code（退出后用 claude --resume 恢复会话）才生效。多CC终端使用不同模型，以最后切换模型时的上下文长度作为全局变量。"
                    : "关闭后，切换模型不再自动更新上下文长度与压缩阈值。",
                },
              )}
            </p>
            <p className="mt-1.5 ml-1 text-xs leading-relaxed text-muted-foreground">
              {t("providerForm.autoSyncCompactRatioHint", {
                defaultValue:
                  "该参数是模型自动压缩上下文窗口的比例，范围 0.2~0.95，留空按 0.95 处理。",
              })}
            </p>

            <div className="space-y-2 border-t border-border-default pt-4">
              <FormLabel htmlFor="claudeModel">
                {t("providerForm.fallbackModelLabel", {
                  defaultValue: "默认兜底模型",
                })}
              </FormLabel>
              <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_minmax(0,104px)]">
                {renderModelInput(
                  "claudeModel",
                  claudeModel,
                  "ANTHROPIC_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                  (value) => {
                    clearRawWindowInput("ANTHROPIC_MODEL");
                    onModelChange("ANTHROPIC_MODEL", value.trim());
                  },
                )}
                <Input
                  inputMode="text"
                  className="w-[90px] text-center font-mono text-sm"
                  value={
                    rawWindowInput.ANTHROPIC_MODEL ??
                    contextWindowInputValue(
                      contextWindows,
                      contextWindowTokens,
                      "ANTHROPIC_MODEL",
                      claudeModel,
                    )
                  }
                  onChange={(event) => {
                    setRawWindowInput((prev) => ({
                      ...prev,
                      ANTHROPIC_MODEL: event.target.value,
                    }));
                  }}
                  onBlur={() => {
                    const raw = rawWindowInput.ANTHROPIC_MODEL;
                    if (raw === undefined) return;
                    commitWindow("ANTHROPIC_MODEL", raw);
                  }}
                  onKeyDown={(event) =>
                    commitContextWindowFromKeyDown(event, "ANTHROPIC_MODEL")
                  }
                  placeholder={t("providerForm.modelContextWindowPlaceholder", {
                    defaultValue: "1M / 200K",
                  })}
                  aria-label={t("providerForm.modelContextWindowLabel", {
                    defaultValue: "Context Window",
                  })}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.fallbackModelHint", {
                  defaultValue:
                    "用于未明确落到 Sonnet、Opus、Fable、Haiku 角色的请求。使用第三方/中转端点时建议填写：否则这些请求（含 Haiku 后台子任务）会以原始 Claude 模型名透传给上游，可能因上游无此模型而报错。官方端点可留空。",
                })}
              </p>
            </div>

            <CustomUserAgentField
              id="claude-custom-user-agent"
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
          </CollapsibleContent>
        </Collapsible>
      )}
    </>
  );
}
