import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import { extractErrorMessage } from "@/utils/errorUtils";
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
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Download,
  ExternalLink,
  Loader2,
  RefreshCw,
  TriangleAlert,
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
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { useCredentialScanStatus } from "@/lib/query/subscription";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";
import type { CredentialStatus } from "@/types/subscription";
import type { TemplateValueConfig } from "@/config/claudeProviderPresets";
import { cn } from "@/lib/utils";
import type { LucideIcon } from "lucide-react";
import type { TFunction } from "i18next";

function getGeminiStatusCopy(
  status: CredentialStatus | undefined,
  t: TFunction,
): {
  icon: LucideIcon;
  containerClassName: string;
  iconClassName: string;
  title: string;
  description: string;
} {
  switch (status) {
    case "valid":
      return {
        icon: CheckCircle2,
        containerClassName:
          "border-green-200 bg-green-50 dark:border-green-800 dark:bg-green-950/40",
        iconClassName: "text-green-600 dark:text-green-400",
        title: t("provider.form.claude.geminiOAuth.status.validTitle", {
          defaultValue: "Detected local Gemini OAuth",
        }),
        description: t(
          "provider.form.claude.geminiOAuth.status.validDescription",
          {
            defaultValue:
              "Claude will use the Gemini OAuth credentials already stored on this device.",
          },
        ),
      };
    case "expired":
      return {
        icon: TriangleAlert,
        containerClassName:
          "border-amber-200 bg-amber-50 dark:border-amber-800 dark:bg-amber-950/30",
        iconClassName: "text-amber-600 dark:text-amber-400",
        title: t("provider.form.claude.geminiOAuth.status.expiredTitle", {
          defaultValue: "Local Gemini OAuth expired",
        }),
        description: t(
          "provider.form.claude.geminiOAuth.status.expiredDescription",
          {
            defaultValue:
              "Please sign in to Gemini again, then refresh the local credential scan.",
          },
        ),
      };
    case "parse_error":
      return {
        icon: AlertCircle,
        containerClassName:
          "border-red-200 bg-red-50 dark:border-red-800 dark:bg-red-950/30",
        iconClassName: "text-red-600 dark:text-red-400",
        title: t("provider.form.claude.geminiOAuth.status.parseErrorTitle", {
          defaultValue: "Failed to parse local Gemini OAuth",
        }),
        description: t(
          "provider.form.claude.geminiOAuth.status.parseErrorDescription",
          {
            defaultValue:
              "cc-switch found Gemini OAuth data on this device, but could not parse it.",
          },
        ),
      };
    case "not_found":
    default:
      return {
        icon: AlertCircle,
        containerClassName:
          "border-slate-200 bg-slate-50 dark:border-slate-800 dark:bg-slate-900/40",
        iconClassName: "text-slate-600 dark:text-slate-400",
        title: t("provider.form.claude.geminiOAuth.status.notFoundTitle", {
          defaultValue: "No local Gemini OAuth found",
        }),
        description: t(
          "provider.form.claude.geminiOAuth.status.notFoundDescription",
          {
            defaultValue:
              "Sign in to Gemini first, then cc-switch can reuse the local OAuth credentials.",
          },
        ),
      };
  }
}

function GeminiOauthStatusCard({ websiteUrl }: { websiteUrl: string }) {
  const { t } = useTranslation();
  const isTauriRuntimeAvailable =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const { data, isFetching, isLoading, refetch } = useCredentialScanStatus(
    "gemini",
    isTauriRuntimeAvailable,
  );

  const statusCopy = useMemo(
    () =>
      isTauriRuntimeAvailable
        ? getGeminiStatusCopy(data?.credentialStatus, t)
        : getGeminiStatusCopy(undefined, t),
    [data?.credentialStatus, isTauriRuntimeAvailable, t],
  );
  const StatusIcon = statusCopy.icon;
  const detail = isTauriRuntimeAvailable
    ? data?.credentialMessage?.trim()
    : t("provider.form.claude.geminiOAuth.browserPreviewHint", {
        defaultValue:
          "Browser preview mode cannot access the local Tauri runtime. Open the desktop app to scan local Gemini OAuth credentials.",
      });

  const [isInstalling, setIsInstalling] = useState(false);

  const handleLaunchLogin = async () => {
    if (!isTauriRuntimeAvailable) {
      toast.error(
        t("provider.form.claude.geminiOAuth.tauriRequired", {
          defaultValue:
            "This action is only available in the desktop app. Please open cc-switch in Tauri and try again.",
        }),
      );
      return;
    }

    try {
      const { subscriptionApi } = await import("@/lib/api/subscription");
      await subscriptionApi.launchGeminiOauthLogin();
      toast.success(
        t("provider.form.claude.geminiOAuth.loginLaunched", {
          defaultValue: "已启动 Gemini CLI，请在终端中完成登录",
        }),
      );
      // 延迟刷新状态，给用户时间完成登录
      setTimeout(() => refetch(), 3000);
    } catch (error) {
      const errorMsg = extractErrorMessage(error);
      // 如果是未安装错误，显示安装提示
      if (errorMsg.includes("未安装") || errorMsg.includes("未检测到")) {
        toast.info(
          t("provider.form.claude.geminiOAuth.notInstalled", {
            defaultValue: "Gemini CLI 未安装，请点击下方安装按钮",
          }),
        );
      } else {
        toast.error(
          errorMsg ||
            t("provider.form.claude.geminiOAuth.launchFailed", {
              defaultValue: "启动 Gemini CLI 失败",
            }),
        );
      }
    }
  };

  const handleInstall = async (useBun: boolean) => {
    if (!isTauriRuntimeAvailable) {
      toast.error(
        t("provider.form.claude.geminiOAuth.tauriRequired", {
          defaultValue:
            "This action is only available in the desktop app. Please open cc-switch in Tauri and try again.",
        }),
      );
      return;
    }

    setIsInstalling(true);
    try {
      const { subscriptionApi } = await import("@/lib/api/subscription");
      await subscriptionApi.installGeminiCli(useBun);
      toast.success(
        t("provider.form.claude.geminiOAuth.installSuccess", {
          defaultValue: "Gemini CLI 安装成功！",
        }),
      );
      // 安装成功后刷新状态
      setTimeout(() => refetch(), 1000);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("provider.form.claude.geminiOAuth.installFailed", {
            defaultValue: "安装失败",
          }),
      );
    } finally {
      setIsInstalling(false);
    }
  };

  return (
    <div className="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950/40">
      <div className="space-y-4">
        <div className="space-y-1">
          <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
            {t("provider.form.claude.geminiOAuth.title", {
              defaultValue: "Gemini OAuth authentication",
            })}
          </p>
          <p className="text-sm text-blue-700 dark:text-blue-300">
            {t("provider.form.claude.geminiOAuth.description", {
              defaultValue:
                "Claude will use Gemini Official through the Gemini OAuth credentials already stored on this device. No API key is required.",
            })}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-300/80">
            {t("provider.form.claude.geminiOAuth.scanHint", {
              defaultValue:
                "cc-switch scans local Gemini OAuth credentials on this device and does not ask you to paste tokens manually.",
            })}
          </p>
        </div>

        <div
          className={cn(
            "rounded-md border px-3 py-3",
            statusCopy.containerClassName,
          )}
        >
          <div className="flex items-start gap-3">
            <StatusIcon className={cn("mt-0.5 h-4 w-4 shrink-0", statusCopy.iconClassName)} />
            <div className="min-w-0 space-y-1">
              <p className="text-sm font-medium">{statusCopy.title}</p>
              <p className="text-xs text-muted-foreground">
                {detail || statusCopy.description}
              </p>
            </div>
          </div>
        </div>

        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              size="sm"
              onClick={handleLaunchLogin}
              disabled={
                !isTauriRuntimeAvailable || isFetching || isLoading || isInstalling
              }
            >
              <Download className="h-3.5 w-3.5" />
              {t("provider.form.claude.geminiOAuth.launchCli", {
                defaultValue: "Open terminal to sign in to Gemini",
              })}
            </Button>
            {websiteUrl && (
              <Button asChild type="button" size="sm" variant="outline">
                <a href={websiteUrl} target="_blank" rel="noreferrer">
                  <ExternalLink className="h-3.5 w-3.5" />
                  {t("provider.form.claude.geminiOAuth.openWebsite", {
                    defaultValue: "Open Gemini website",
                  })}
                </a>
              </Button>
            )}
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => refetch()}
              disabled={
                !isTauriRuntimeAvailable || isFetching || isLoading || isInstalling
              }
            >
              <RefreshCw
                className={cn(
                  "h-3.5 w-3.5",
                  (isFetching || isLoading) && "animate-spin",
                )}
              />
              {t("provider.form.claude.geminiOAuth.refresh", {
                defaultValue: "Refresh status",
              })}
            </Button>
          </div>

          <div className="flex flex-wrap gap-2 pt-2 border-t border-blue-200 dark:border-blue-800">
            <span className="text-xs text-blue-700 dark:text-blue-300 self-center">
              {t("provider.form.claude.geminiOAuth.installHint", {
                defaultValue: "未安装？",
              })}
            </span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => handleInstall(false)}
              disabled={!isTauriRuntimeAvailable || isInstalling}
            >
              {isInstalling ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5" />
              )}
              npm
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => handleInstall(true)}
              disabled={!isTauriRuntimeAvailable || isInstalling}
            >
              {isInstalling ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : null}
              bun
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

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

  // Gemini OAuth (Claude -> Gemini Official)
  isGeminiOauthPreset?: boolean;

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

  // Model Selector
  shouldShowModelSelector: boolean;
  claudeModel: string;
  reasoningModel: string;
  defaultHaikuModel: string;
  defaultSonnetModel: string;
  defaultOpusModel: string;
  onModelChange: (
    field:
      | "ANTHROPIC_MODEL"
      | "ANTHROPIC_REASONING_MODEL"
      | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
      | "ANTHROPIC_DEFAULT_SONNET_MODEL"
      | "ANTHROPIC_DEFAULT_OPUS_MODEL",
    value: string,
  ) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // API Format (for third-party providers that use OpenAI Chat Completions format)
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
  selectedCodexAccountId,
  onCodexAccountSelect,
  isGeminiOauthPreset,
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
  shouldShowModelSelector,
  claudeModel,
  reasoningModel,
  defaultHaikuModel,
  defaultSonnetModel,
  defaultOpusModel,
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
    reasoningModel ||
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

  // 通用模型获取（非 Copilot 供应商）
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey, isFullUrl)
      .then((models) => {
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
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, isFullUrl, t]);

  // 当 Copilot 预设且已认证时，加载可用模型
  useEffect(() => {
    // 如果不是 Copilot 预设或未认证，清空模型列表
    if (!isCopilotPreset || !isCopilotAuthenticated) {
      setCopilotModels([]);
      setModelsLoading(false);
      return;
    }

    let cancelled = false;
    setModelsLoading(true);
    const fetchModels = selectedGitHubAccountId
      ? copilotGetModelsForAccount(selectedGitHubAccountId)
      : copilotGetModels();

    fetchModels
      .then((models) => {
        if (!cancelled) setCopilotModels(models);
      })
      .catch((err) => {
        console.warn("[Copilot] Failed to fetch models:", err);
        if (!cancelled) {
          toast.error(
            t("copilot.loadModelsFailed", {
              defaultValue: "加载 Copilot 模型列表失败",
            }),
          );
        }
      })
      .finally(() => {
        if (!cancelled) setModelsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isCopilotPreset, isCopilotAuthenticated, selectedGitHubAccountId]);

  // 模型输入框：支持手动输入 + 下拉选择
  const renderModelInput = (
    id: string,
    value: string,
    field: ClaudeFormFieldsProps["onModelChange"] extends (
      f: infer F,
      v: string,
    ) => void
      ? F
      : never,
    placeholder?: string,
  ) => {
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
            onChange={(e) => onModelChange(field, e.target.value)}
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
                      onSelect={() => onModelChange(field, model.id)}
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
            onChange={(e) => onModelChange(field, e.target.value)}
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

    // 非 Copilot 供应商: 使用 ModelInputWithFetch（获取按钮在 section 标题旁）
    return (
      <ModelInputWithFetch
        id={id}
        value={value}
        onChange={(v) => onModelChange(field, v)}
        placeholder={placeholder}
        fetchedModels={fetchedModels}
        isLoading={isFetchingModels}
      />
    );
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
        />
      )}

      {/* Gemini OAuth 认证（Claude -> Gemini Official） */}
      {isGeminiOauthPreset && <GeminiOauthStatusCard websiteUrl={websiteUrl} />}

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
                : apiFormat === "gemini"
                  ? t("providerForm.apiHintGemini", {
                      defaultValue:
                        "Gemini API 端点（如 https://generativelanguage.googleapis.com/v1beta）",
                    })
                  : t("providerForm.apiHint")
          }
          onManageClick={() => onEndpointModalToggle(true)}
          showFullUrlToggle={true}
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
        />
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
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
                    <SelectItem value="gemini">
                      {t("providerForm.apiFormatGemini", {
                        defaultValue: "Gemini generateContent (需转换)",
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
                {!isCopilotPreset && (
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
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.modelMappingHint")}
              </p>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {/* 主模型 */}
              <div className="space-y-2">
                <FormLabel htmlFor="claudeModel">
                  {t("providerForm.anthropicModel", {
                    defaultValue: "主模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeModel",
                  claudeModel,
                  "ANTHROPIC_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                )}
              </div>

              {/* 推理模型 */}
              <div className="space-y-2">
                <FormLabel htmlFor="reasoningModel">
                  {t("providerForm.anthropicReasoningModel")}
                </FormLabel>
                {renderModelInput(
                  "reasoningModel",
                  reasoningModel,
                  "ANTHROPIC_REASONING_MODEL",
                )}
              </div>

              {/* 默认 Haiku */}
              <div className="space-y-2">
                <FormLabel htmlFor="claudeDefaultHaikuModel">
                  {t("providerForm.anthropicDefaultHaikuModel", {
                    defaultValue: "Haiku 默认模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeDefaultHaikuModel",
                  defaultHaikuModel,
                  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                  t("providerForm.haikuModelPlaceholder", { defaultValue: "" }),
                )}
              </div>

              {/* 默认 Sonnet */}
              <div className="space-y-2">
                <FormLabel htmlFor="claudeDefaultSonnetModel">
                  {t("providerForm.anthropicDefaultSonnetModel", {
                    defaultValue: "Sonnet 默认模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeDefaultSonnetModel",
                  defaultSonnetModel,
                  "ANTHROPIC_DEFAULT_SONNET_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                )}
              </div>

              {/* 默认 Opus */}
              <div className="space-y-2">
                <FormLabel htmlFor="claudeDefaultOpusModel">
                  {t("providerForm.anthropicDefaultOpusModel", {
                    defaultValue: "Opus 默认模型",
                  })}
                </FormLabel>
                {renderModelInput(
                  "claudeDefaultOpusModel",
                  defaultOpusModel,
                  "ANTHROPIC_DEFAULT_OPUS_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                )}
              </div>
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}
    </>
  );
}
