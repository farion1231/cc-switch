import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "@/lib/toast";
import { Checkbox } from "@/components/ui/checkbox";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
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
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import {
  ApiKeySection,
  DirectModeFormatWarning,
  EndpointField,
  FullUrlToggle,
  ModelDropdown,
  ModelInputWithFetch,
  type DirectModeFormatNotice,
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
  modelFetchRequestHeaders,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import {
  ClaudeStackModelsField,
  type ClaudeStackModelRow,
} from "./ClaudeStackModelsField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";
import type { ManagedAuthProvider } from "@/lib/api";
import {
  hasClaudeOneMMarker,
  setClaudeOneMMarker,
  stripClaudeOneMMarker,
  type ClaudeModelEnvField,
} from "./hooks/useModelState";
import {
  providerPresets,
  type TemplateValueConfig,
} from "@/config/claudeProviderPresets";

interface EndpointCandidate {
  url: string;
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

  /**
   * 布局：`classic` 是直连 / 路由用的完整表单；`stack` 是 Stack 模式的简化面板（连接 +
   * 模型列表 + 高级），不显示模型映射。
   */
  variant?: "classic" | "stack";
  /** 应用实际在直连时传：选了要靠路由转换的格式就在格式下面提示后果。 */
  directModeFormatNotice?: DirectModeFormatNotice;
  /** Stack 布局的模型列表（第一行是这家的默认模型）。 */
  stackModelRows?: ClaudeStackModelRow[];
  onStackModelRowsChange?: (rows: ClaudeStackModelRow[]) => void;
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
  variant = "classic",
  directModeFormatNotice,
  stackModelRows = [],
  onStackModelRowsChange,
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
    (!isXaiOauthPreset && !isCodexOauthPreset && apiFormat !== "anthropic") ||
    apiKeyField !== "ANTHROPIC_AUTH_TOKEN" ||
    isFullUrl ||
    customUserAgent ||
    hasRequestOverrides
  );
  const [advancedExpanded, setAdvancedExpanded] = useState(
    isXaiOauthPreset ? false : hasAnyAdvancedValue,
  );

  // 预设填充高级值后自动展开（仅从折叠→展开，不会自动折叠）
  useEffect(() => {
    if (isXaiOauthPreset) {
      setAdvancedExpanded(false);
    } else if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue, isXaiOauthPreset]);

  // Stack 布局的高级区只有完整 URL、User-Agent 和请求覆盖，开了或填了才展开。
  const [stackAdvancedExpanded, setStackAdvancedExpanded] = useState(
    isFullUrl || !!customUserAgent || hasRequestOverrides,
  );

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
  const fallbackUsesOneM = hasClaudeOneMMarker(claudeModel);

  // 通用模型获取（非 Copilot 供应商）
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  // 聚合布局的模型列表拉取成功后直接打开选择弹窗，不再提示成功。
  const opensModelPicker = variant === "stack" && !!onStackModelRowsChange;
  const showModelFetchResult = useCallback(
    (count: number) => {
      if (count === 0) {
        toast.info(t("providerForm.fetchModelsEmpty"));
      } else if (!opensModelPicker) {
        toast.success(t("providerForm.fetchModelsSuccess", { count }));
      }
    },
    [opensModelPicker, t],
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
    fetchModelsForConfig(
      baseUrl,
      apiKey,
      isFullUrl,
      modelsUrl,
      customUserAgent,
      {
        requestHeaders: modelFetchRequestHeaders(localProxyHeadersOverride),
      },
    )
      .then((models) => {
        setFetchedModels(models);
        showModelFetchResult(models.length);
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [
    baseUrl,
    apiKey,
    isFullUrl,
    customUserAgent,
    localProxyHeadersOverride,
    showModelFetchResult,
    t,
  ]);

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
    },
    {
      role: "opus",
      label: t("providerForm.modelRoleOpus", { defaultValue: "Opus" }),
      model: defaultOpusModel,
      displayName: defaultOpusModelName,
      modelField: "ANTHROPIC_DEFAULT_OPUS_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
      inputId: "claudeDefaultOpusModel",
    },
    {
      role: "fable",
      label: t("providerForm.modelRoleFable", { defaultValue: "Fable" }),
      model: defaultFableModel,
      displayName: defaultFableModelName,
      modelField: "ANTHROPIC_DEFAULT_FABLE_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
      inputId: "claudeDefaultFableModel",
    },
    {
      role: "haiku",
      label: t("providerForm.modelRoleHaiku", { defaultValue: "Haiku" }),
      model: defaultHaikuModel,
      displayName: defaultHaikuModelName,
      modelField: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
      inputId: "claudeDefaultHaikuModel",
    },
    {
      role: "subagent",
      label: t("providerForm.modelRoleSubagent", {
        defaultValue: "Subagent",
      }),
      model: subagentModel,
      modelField: "CLAUDE_CODE_SUBAGENT_MODEL",
      inputId: "claudeCodeSubagentModel",
    },
  ];

  const handleRoleModelChange = (row: ModelRoleRow, value: string) => {
    const oldModelBase = stripClaudeOneMMarker(row.model).trim();
    const nextModelBase = stripClaudeOneMMarker(value).trim();
    const displayName = row.displayName?.trim() ?? "";
    const shouldSyncDisplayName = !displayName || displayName === oldModelBase;
    onModelChange(row.modelField, value);
    if (row.displayNameField && shouldSyncDisplayName) {
      onModelChange(row.displayNameField, nextModelBase);
    }
  };

  const handleRoleOneMChange = (row: ModelRoleRow, enabled: boolean) => {
    handleRoleModelChange(row, setClaudeOneMMarker(row.model, enabled));
  };

  const oauthSections = (
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
    </>
  );

  const apiKeySection = (
    <>
      {/* API Key 输入框（非 OAuth 预设时显示） */}
      {shouldShowApiKey && !usesOAuth && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          required
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}
    </>
  );

  const templateSection = (
    <>
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
    </>
  );

  // 字段说明一律收进字段名后的「?」（和 Codex 表单一致），页面上只留后果提示。
  const endpointHint =
    apiFormat === "openai_responses"
      ? t("providerForm.apiHintResponses")
      : apiFormat === "openai_chat"
        ? t("providerForm.apiHintOAI")
        : apiFormat === "gemini_native"
          ? t("providerForm.apiHintGeminiNative")
          : t("providerForm.apiHint");

  const endpointSection = (
    <>
      {/* Base URL 输入框；Codex / xAI 托管账号卡的上游地址由后端固定，不显示 */}
      {shouldShowSpeedTest && !isCodexOauthPreset && !isXaiOauthPreset && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder")}
          help={endpointHint}
          showManageButton={showEndpointTools}
          onManageClick={
            showEndpointTools ? () => onEndpointModalToggle(true) : undefined
          }
        />
      )}
    </>
  );

  const speedTestModal = (
    <>
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
    </>
  );

  const copilotRoutingNotice = isCopilotPreset && (
    <p className="text-xs leading-relaxed text-fg-2">
      {t("copilot.automaticFormatHint")}
    </p>
  );

  // 上游格式、认证字段：经典布局放在高级选项里，Stack 布局放在连接区。
  const apiFormatLabel = t("providerForm.apiFormat", {
    defaultValue: "上游格式",
  });

  const apiFormatField = (
    <>
      {/* 上游格式选择（仅非云服务商显示；xAI、Codex 托管账号卡的格式由后端固定） */}
      {category !== "cloud_provider" &&
        !isXaiOauthPreset &&
        !isCodexOauthPreset && (
          <div className="space-y-2">
            <div className="flex items-center gap-0.5">
              <FormLabel htmlFor="apiFormat">{apiFormatLabel}</FormLabel>
              <HelpTip title={apiFormatLabel}>
                {t("providerForm.apiFormatHelp", {
                  defaultValue:
                    "看供应商原生支持哪种协议：Anthropic Messages 原样转发、不转换格式；其余三种由路由转成 Anthropic Messages。",
                })}
              </HelpTip>
            </div>
            <Select
              value={apiFormat}
              onValueChange={onApiFormatChange}
              disabled={isCopilotPreset}
            >
              <SelectTrigger id="apiFormat" className="w-full">
                <SelectValue>
                  {isCopilotPreset ? t("copilot.automaticFormat") : undefined}
                </SelectValue>
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
            {directModeFormatNotice && apiFormat !== "anthropic" && (
              <DirectModeFormatWarning notice={directModeFormatNotice} />
            )}
          </div>
        )}
    </>
  );

  // 认证字段选择器
  const authFieldLabel = t("providerForm.authField", {
    defaultValue: "认证字段",
  });
  const authFieldSelect = (
    <div className="space-y-2">
      <div className="flex items-center gap-0.5">
        <FormLabel>{authFieldLabel}</FormLabel>
        <HelpTip title={authFieldLabel}>
          {t("providerForm.authFieldHint", {
            defaultValue:
              "直连时是写进 settings.json 的变量名；经过路由时，AUTH_TOKEN 以 Bearer、API_KEY 以 x-api-key 发给上游。",
          })}
        </HelpTip>
      </div>
      <Select
        value={apiKeyField}
        onValueChange={(v) => onApiKeyFieldChange(v as ClaudeApiKeyField)}
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
    </div>
  );

  const fallbackModelLabel = t("providerForm.fallbackModelLabel", {
    defaultValue: "默认兜底模型",
  });
  // 只有第三方端点才认不出原始 Claude 模型名：直连看后台任务用的 Haiku 档，路由看兜底模型本身。
  const usesThirdPartyEndpoint =
    category !== "official" && category !== "cloud_provider";
  const fallbackModelMissing =
    usesThirdPartyEndpoint &&
    !(directModeFormatNotice ? defaultHaikuModel : claudeModel).trim();

  // 完整 URL 跟着地址框：托管账号卡的地址由后端固定，没有这个开关。
  const fullUrlToggle = shouldShowSpeedTest &&
    showEndpointTools &&
    !isCodexOauthPreset &&
    !isXaiOauthPreset && (
      <FullUrlToggle
        checked={isFullUrl}
        onCheckedChange={onFullUrlChange}
        geminiNative={apiFormat === "gemini_native"}
      />
    );

  // 两项都只在路由、聚合模式下生效：在分组开头说一次，各自的作用收进字段名后的「?」。
  const userAgentAndOverrides = (
    <>
      <p className="text-xs leading-relaxed text-fg-2">
        {t("codexConfig.routeOnlySettingsHint", {
          defaultValue: "以下两项只在路由、聚合模式下生效。",
        })}
      </p>
      <CustomUserAgentField
        id="claude-custom-user-agent"
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

  if (variant === "stack") {
    // 批量勾选模型用当前预设对应的那份上游列表。
    const stackFetchedModels: FetchedModel[] = isCopilotPreset
      ? copilotModels.map((m) => ({ id: m.id, ownedBy: m.vendor || null }))
      : isCodexOauthPreset
        ? codexOauthModels
        : isXaiOauthPreset
          ? xaiOauthModels
          : fetchedModels;
    // 认证字段只对 Anthropic 格式、自己填 Key 的供应商有意义，放在 Key 旁边。
    const showAuthField =
      shouldShowApiKey &&
      !usesOAuth &&
      !isCodexOauthPreset &&
      !isXaiOauthPreset &&
      apiFormat === "anthropic";

    return (
      <>
        {oauthSections}
        {copilotRoutingNotice}
        {apiFormatField}
        {apiKeySection}
        {showAuthField && authFieldSelect}
        {templateSection}
        {endpointSection}
        {speedTestModal}

        {onStackModelRowsChange && (
          <ClaudeStackModelsField
            rows={stackModelRows}
            onRowsChange={onStackModelRowsChange}
            fetchedModels={stackFetchedModels}
            onFetchModels={handleModelFetchClick}
            isFetchingModels={modelFetchLoading}
            baseUrl={baseUrl}
          />
        )}

        {/* 高级区只有 UA 和请求覆盖；Copilot 的转发会跳过这两项，整块不显示 */}
        {!isCopilotPreset && (
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
                {t("providerForm.advancedOptionsToggle")}
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="space-y-4 pt-2">
              {fullUrlToggle && (
                <div className="border-b border-border pb-3">
                  {fullUrlToggle}
                </div>
              )}
              {userAgentAndOverrides}
            </CollapsibleContent>
          </Collapsible>
        )}
      </>
    );
  }

  return (
    <>
      {oauthSections}
      {copilotRoutingNotice}
      {apiKeySection}
      {templateSection}
      {endpointSection}
      {speedTestModal}

      {shouldShowModelSelector && (
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
              {t("providerForm.advancedOptionsToggle")}
            </Button>
          </CollapsibleTrigger>
          <CollapsibleContent className="space-y-4 pt-2">
            {apiFormatField}

            {fullUrlToggle}

            {/* 认证字段只决定 Anthropic 格式上游的认证头（OpenAI 格式一律 Bearer）；Copilot
                靠它决定占位符写在哪个变量；Codex / xAI 托管账号卡的认证由后端注入。 */}
            {!isCodexOauthPreset &&
              !isXaiOauthPreset &&
              (apiFormat === "anthropic" || isCopilotPreset) &&
              authFieldSelect}

            {/* 模型映射 */}
            <div className="space-y-1 border-t border-border pt-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-0.5">
                  <FormLabel>{t("providerForm.modelMappingLabel")}</FormLabel>
                  <HelpTip title={t("providerForm.modelMappingLabel")}>
                    {t("providerForm.modelMappingHint")}
                  </HelpTip>
                </div>
                <div className="flex gap-2">
                  {/* 一键设置按钮 */}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      // 按面板从上到下取值，默认兜底模型最后使用。
                      const value =
                        defaultSonnetModel ||
                        defaultOpusModel ||
                        defaultFableModel ||
                        defaultHaikuModel ||
                        subagentModel ||
                        claudeModel;
                      if (value) {
                        for (const row of modelRoleRows) {
                          onModelChange(row.modelField, value);
                          if (row.displayNameField) {
                            onModelChange(
                              row.displayNameField,
                              stripClaudeOneMMarker(value),
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
            </div>

            <div className="space-y-3">
              <div className="hidden grid-cols-[120px_1fr_minmax(0,1fr)_104px] gap-2 px-1 text-xs font-medium text-fg-2 md:grid">
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
                const modelBase = stripClaudeOneMMarker(row.model);
                const usesOneM = hasClaudeOneMMarker(row.model);

                return (
                  <div
                    key={row.role}
                    className="grid grid-cols-1 gap-2 md:grid-cols-[120px_1fr_minmax(0,1fr)_104px]"
                  >
                    <div className="flex h-9 items-center rounded-md border border-input bg-subtle px-3 text-sm font-medium text-fg-2">
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
                      <div className="flex h-9 items-center rounded-md border border-input bg-subtle px-3 text-sm text-fg-2">
                        {t("providerForm.modelNoDisplayName", {
                          defaultValue: "不显示在 /model 菜单",
                        })}
                      </div>
                    )}
                    {renderModelInput(
                      row.inputId,
                      modelBase,
                      row.modelField,
                      t("providerForm.modelPlaceholder", { defaultValue: "" }),
                      (value) =>
                        handleRoleModelChange(
                          row,
                          setClaudeOneMMarker(value, usesOneM),
                        ),
                    )}
                    <label className="flex h-9 items-center gap-2 text-sm text-fg-2">
                      <Checkbox
                        checked={usesOneM}
                        onCheckedChange={(checked) =>
                          handleRoleOneMChange(row, checked === true)
                        }
                      />
                      {t("providerForm.modelOneMLabel", {
                        defaultValue: "1M",
                      })}
                    </label>
                  </div>
                );
              })}
            </div>

            <div className="space-y-2 border-t border-border pt-4">
              <div className="flex items-center gap-0.5">
                <FormLabel htmlFor="claudeModel">
                  {fallbackModelLabel}
                </FormLabel>
                {/* 直连时它是 Claude Code 的主模型；经过路由时才是各档留空、没匹配到档位的回落。 */}
                <HelpTip title={fallbackModelLabel}>
                  {directModeFormatNotice
                    ? t("providerForm.fallbackModelHelpDirect", {
                        defaultValue:
                          "写进 ANTHROPIC_MODEL，是 Claude Code 启动时用的主模型。后台任务另用 Haiku 那一档。",
                      })
                    : t("providerForm.fallbackModelHelp", {
                        defaultValue:
                          "没落到 Sonnet、Opus、Fable、Haiku 任一档的请求，改用这个模型发给上游。",
                      })}
                </HelpTip>
              </div>
              <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_minmax(0,104px)]">
                {renderModelInput(
                  "claudeModel",
                  stripClaudeOneMMarker(claudeModel),
                  "ANTHROPIC_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                  (value) =>
                    onModelChange(
                      "ANTHROPIC_MODEL",
                      setClaudeOneMMarker(value, fallbackUsesOneM),
                    ),
                )}
                <label className="flex h-9 items-center gap-2 text-sm text-fg-2">
                  <Checkbox
                    checked={fallbackUsesOneM}
                    onCheckedChange={(checked) => {
                      const base = stripClaudeOneMMarker(claudeModel).trim();
                      if (!base) return;
                      onModelChange(
                        "ANTHROPIC_MODEL",
                        setClaudeOneMMarker(base, checked === true),
                      );
                    }}
                  />
                  {t("providerForm.modelOneMLabel", {
                    defaultValue: "1M",
                  })}
                </label>
              </div>
              {fallbackModelMissing && (
                <p className="text-xs leading-relaxed text-warning-text">
                  {directModeFormatNotice
                    ? t("providerForm.fallbackModelHaikuMissing", {
                        defaultValue:
                          "Haiku 一档没填：后台任务会用原始 Claude 模型名请求上游，第三方端点可能报错。",
                      })
                    : t("providerForm.fallbackModelMissing", {
                        defaultValue:
                          "没填：没落到任何一档的请求会用原始 Claude 模型名发给上游，可能报错。",
                      })}
                </p>
              )}
            </div>

            {/* Copilot 的请求由转发层按 Copilot 客户端的样子发，UA 和请求覆盖都会被跳过 */}
            {!isCopilotPreset && (
              <div className="space-y-4 border-t border-border pt-4">
                {userAgentAndOverrides}
              </div>
            )}
          </CollapsibleContent>
        </Collapsible>
      )}
    </>
  );
}
