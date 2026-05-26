import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Wand2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { CodexOAuthSection } from "./CodexOAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
} from "@/lib/api/copilot";
import type { CopilotModel } from "@/lib/api/copilot";
import {
  fetchCodexOauthModels,
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";
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

  // Codex OAuth (ChatGPT Plus/Pro)
  isCodexOauthPreset?: boolean;
  isCodexOauthAuthenticated?: boolean;
  selectedCodexAccountId?: string | null;
  onCodexAccountSelect?: (accountId: string | null) => void;
  codexFastMode?: boolean;
  onCodexFastModeChange?: (enabled: boolean) => void;

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
  isCodexOauthPreset,
  isCodexOauthAuthenticated,
  selectedCodexAccountId,
  onCodexAccountSelect,
  codexFastMode,
  onCodexFastModeChange,
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
  onModelChange,
  speedTestEndpoints,
  apiFormat,
  onApiFormatChange,
  apiKeyField,
  onApiKeyFieldChange,
  isFullUrl,
  onFullUrlChange,
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();
  const hasAnyAdvancedValue = !!(
    claudeModel ||
    defaultHaikuModel ||
    defaultSonnetModel ||
    defaultOpusModel ||
    apiFormat !== "anthropic" ||
    apiKeyField !== "ANTHROPIC_AUTH_TOKEN"
  );
  const [advancedExpanded, setAdvancedExpanded] = useState(hasAnyAdvancedValue);

  // 预设填充高级值后自动展开（仅从折叠→展开，不会自动折叠）
  useEffect(() => {
    if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue]);

  // Copilot 可用模型列表
  const [copilotModels, setCopilotModels] = useState<CopilotModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const copilotModelsRequestRef = useRef(0);

  // Codex OAuth 可用模型列表
  const [codexOauthModels, setCodexOauthModels] = useState<FetchedModel[]>([]);
  const [codexOauthModelsLoading, setCodexOauthModelsLoading] = useState(false);
  const codexOauthModelsRequestRef = useRef(0);

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
    fetchModelsForConfig(baseUrl, apiKey, isFullUrl, modelsUrl)
      .then((models) => {
        setFetchedModels(models);
        showModelFetchResult(models.length);
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, isFullUrl, showModelFetchResult, t]);

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

  const modelFetchLoading = isCopilotPreset
    ? modelsLoading
    : isCodexOauthPreset
      ? codexOauthModelsLoading
      : isFetchingModels;
  const handleModelFetchClick = isCopilotPreset
    ? handleFetchCopilotModels
    : isCodexOauthPreset
      ? handleFetchCodexOauthModels
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

    if (isCopilotPreset && copilotModels.length > 0) {
      // 按 vendor 分组
      const grouped: Record<string, CopilotModel[]> = {};
      for (const model of copilotModels) {
        const vendor = model.vendor || "Other";
        if (!grouped[vendor]) grouped[vendor] = [];
        grouped[vendor].push(model);
      }
      const vendors = Object.keys(grouped).sort();

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
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon" className="shrink-0">
                <ChevronDown className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="end"
              className="max-h-64 overflow-y-auto z-[200]"
            >
              {vendors.map((vendor, vi) => (
                <div key={vendor}>
                  {vi > 0 && <DropdownMenuSeparator />}
                  <DropdownMenuLabel>{vendor}</DropdownMenuLabel>
                  {grouped[vendor].map((model) => (
                    <DropdownMenuItem
                      key={model.id}
                      onSelect={() => updateValue(model.id)}
                    >
                      {model.id}
                    </DropdownMenuItem>
                  ))}
                </div>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
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
    role: "sonnet" | "opus" | "haiku";
    label: string;
    model: string;
    displayName: string;
    modelField: ClaudeModelEnvField;
    displayNameField: ClaudeModelEnvField;
    inputId: string;
    supportsOneM: boolean;
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
      supportsOneM: true,
    },
    {
      role: "opus",
      label: t("providerForm.modelRoleOpus", { defaultValue: "Opus" }),
      model: defaultOpusModel,
      displayName: defaultOpusModelName,
      modelField: "ANTHROPIC_DEFAULT_OPUS_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
      inputId: "claudeDefaultOpusModel",
      supportsOneM: true,
    },
    {
      role: "haiku",
      label: t("providerForm.modelRoleHaiku", { defaultValue: "Haiku" }),
      model: defaultHaikuModel,
      displayName: defaultHaikuModelName,
      modelField: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
      inputId: "claudeDefaultHaikuModel",
      supportsOneM: false,
    },
  ];

  const handleRoleModelChange = (row: ModelRoleRow, value: string) => {
    const oldModelBase = stripClaudeOneMMarker(row.model).trim();
    const normalizedValue = row.supportsOneM
      ? value
      : stripClaudeOneMMarker(value);
    const nextModelBase = stripClaudeOneMMarker(normalizedValue).trim();
    const displayName = row.displayName.trim();
    const shouldSyncDisplayName = !displayName || displayName === oldModelBase;
    onModelChange(row.modelField, normalizedValue);
    if (shouldSyncDisplayName) {
      onModelChange(row.displayNameField, nextModelBase);
    }
  };

  const handleRoleOneMChange = (row: ModelRoleRow, enabled: boolean) => {
    if (!row.supportsOneM) return;
    handleRoleModelChange(row, setClaudeOneMMarker(row.model, enabled));
  };

  return (
    <>
      {/* GitHub Copilot OAuth 认证 */}
      {isCopilotPreset && (
        <CopilotAuthSection
          selectedAccountId={selectedGitHubAccountId}
          onAccountSelect={onGitHubAccountSelect}
        />
      )}

      {/* Codex OAuth 认证 (ChatGPT Plus/Pro) */}
      {isCodexOauthPreset && (
        <CodexOAuthSection
          selectedAccountId={selectedCodexAccountId}
          onAccountSelect={onCodexAccountSelect}
          fastModeEnabled={codexFastMode}
          onFastModeChange={onCodexFastModeChange}
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
          showFullUrlToggle={showEndpointTools}
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

      {/* 高级选项（API 格式 + 认证字段 + 模型映射） */}
      {shouldShowModelSelector && (
        <Collapsible open={advancedExpanded} onOpenChange={setAdvancedExpanded}>
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
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
            {/* API 格式选择（仅非云服务商显示） */}
            {category !== "cloud_provider" && (
              <div className="space-y-2">
                <FormLabel htmlFor="apiFormat">
                  {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
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
                <p className="text-xs text-muted-foreground">
                  {t("providerForm.apiFormatHint", {
                    defaultValue: "选择供应商 API 的输入格式",
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
            <div className="space-y-1 pt-2 border-t">
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
                        defaultHaikuModel;
                      if (value) {
                        for (const row of modelRoleRows) {
                          const roleValue = row.supportsOneM
                            ? value
                            : stripClaudeOneMMarker(value);
                          onModelChange(row.modelField, roleValue);
                          onModelChange(
                            row.displayNameField,
                            stripClaudeOneMMarker(roleValue),
                          );
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
                      !defaultOpusModel
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
                const modelBase = stripClaudeOneMMarker(row.model);
                const usesOneM =
                  row.supportsOneM && hasClaudeOneMMarker(row.model);

                return (
                  <div
                    key={row.role}
                    className="grid grid-cols-1 gap-2 md:grid-cols-[120px_1fr_minmax(0,1fr)_104px]"
                  >
                    <div className="flex h-9 items-center rounded-md border border-input bg-muted px-3 text-sm font-medium text-muted-foreground">
                      {row.label}
                    </div>
                    <Input
                      value={row.displayName}
                      onChange={(event) =>
                        onModelChange(row.displayNameField, event.target.value)
                      }
                      placeholder={
                        modelBase ||
                        t("providerForm.modelDisplayNamePlaceholder", {
                          defaultValue: "例如 DeepSeek V4 Pro",
                        })
                      }
                      autoComplete="off"
                    />
                    {renderModelInput(
                      row.inputId,
                      modelBase,
                      row.modelField,
                      t("providerForm.modelPlaceholder", { defaultValue: "" }),
                      (value) =>
                        handleRoleModelChange(
                          row,
                          row.supportsOneM
                            ? setClaudeOneMMarker(value, usesOneM)
                            : stripClaudeOneMMarker(value),
                        ),
                    )}
                    {row.supportsOneM && (
                      <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
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
                    )}
                  </div>
                );
              })}
            </div>

            <div className="space-y-2 border-t pt-4">
              <FormLabel htmlFor="claudeModel">
                {t("providerForm.fallbackModelLabel", {
                  defaultValue: "默认兜底模型",
                })}
              </FormLabel>
              {renderModelInput(
                "claudeModel",
                claudeModel,
                "ANTHROPIC_MODEL",
                t("providerForm.modelPlaceholder", { defaultValue: "" }),
              )}
              <p className="text-xs text-muted-foreground">
                {t("providerForm.fallbackModelHint", {
                  defaultValue:
                    "仅在 Claude Code 请求没有明确落到 Sonnet、Opus 或 Haiku 角色时使用；通常可以留空。",
                })}
              </p>
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}
    </>
  );
}
