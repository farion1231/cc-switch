import { useCallback, useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";
import { Download, Loader2 } from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { CodexApiFormat, CodexChatCompatibilityMode, ProviderCategory } from "@/types";

interface EndpointCandidate {
  url: string;
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

  // API Format
  apiFormat: CodexApiFormat;
  onApiFormatChange: (format: CodexApiFormat) => void;

  // Chat 兼容模式
  chatCompatibilityMode: CodexChatCompatibilityMode;
  onChatCompatibilityModeChange: (mode: CodexChatCompatibilityMode) => void;

  // Model Name
  shouldShowModelField?: boolean;
  modelName?: string;
  onModelNameChange?: (model: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];
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
  apiFormat,
  onApiFormatChange,
  chatCompatibilityMode,
  onChatCompatibilityModeChange,
  shouldShowModelField = true,
  modelName = "",
  onModelNameChange,
  speedTestEndpoints,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  // DeepSeek 模型检测（Hook 必须在顶层调用）
  const isDeepSeekModel = useMemo(() => {
    const model = (modelName || "").toLowerCase();
    return model.includes("deepseek") || model.includes("deep-seek");
  }, [modelName]);

  const handleFetchModels = useCallback(() => {
    if (!codexBaseUrl || !codexApiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!codexApiKey,
        hasBaseUrl: !!codexBaseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(codexBaseUrl, codexApiKey, isFullUrl)
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
  }, [codexBaseUrl, codexApiKey, isFullUrl, t]);

  return (
    <>
      {/* Codex API Key 输入框 */}
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

      {/* Codex Base URL 输入框 */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="codexBaseUrl"
          label={t("codexConfig.apiUrlLabel")}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          placeholder={
            apiFormat === "openai_chat"
              ? t("providerForm.codexApiChatEndpointPlaceholder")
              : t("providerForm.codexApiEndpointPlaceholder")
          }
          hint={
            apiFormat === "openai_chat"
              ? t("providerForm.codexApiHintChat")
              : t("providerForm.codexApiHint")
          }
          fullUrlHint={
            apiFormat === "openai_chat"
              ? t("providerForm.codexApiHintChat")
              : undefined
          }
          showFullUrlToggle
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* Codex API 格式选择 */}
      {shouldShowSpeedTest && (
        <div className="space-y-2">
          <FormLabel htmlFor="codexApiFormat">
            {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
          </FormLabel>
          <Select
            value={apiFormat}
            onValueChange={(value) =>
              onApiFormatChange(value as CodexApiFormat)
            }
          >
            <SelectTrigger id="codexApiFormat" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="openai_responses">
                {t("providerForm.codexApiFormatResponses", {
                  defaultValue: "OpenAI Responses API (原生)",
                })}
              </SelectItem>
              <SelectItem value="openai_chat">
                {t("providerForm.codexApiFormatOpenAIChat", {
                  defaultValue: "OpenAI Chat Completions (需开启路由)",
                })}
              </SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.codexApiFormatHint", {
              defaultValue:
                "选择供应商真实支持的 Codex API 格式；Chat Completions 会通过本地路由自动转换为 Responses。",
            })}
          </p>
        </div>
      )}

      {/* Codex Chat 兼容模式 — 仅在 openai_chat 且模型名包含 DeepSeek 时显示 */}
      {shouldShowSpeedTest && apiFormat === "openai_chat" && isDeepSeekModel && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <FormLabel htmlFor="codexChatCompatSwitch">
              {t("providerForm.codexChatCompatibilityMode", {
                defaultValue: "Chat 兼容模式",
              })}
            </FormLabel>
            <Switch
              id="codexChatCompatSwitch"
              checked={chatCompatibilityMode === "deepseek_thinking"}
              onCheckedChange={(checked) =>
                onChatCompatibilityModeChange(
                  checked ? "deepseek_thinking" : "standard",
                )
              }
            />
          </div>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.codexChatCompatibilityModeHint", {
              defaultValue:
                "DeepSeek 模型必须开启此选项，否则可能无法正常对话。开启后会适配 reasoning_content 与 tool_calls 的合并。",
            })}
          </p>
        </div>
      )}

      {/* Codex Model Name 输入框 */}
      {shouldShowModelField && onModelNameChange && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <label
              htmlFor="codexModelName"
              className="block text-sm font-medium text-foreground"
            >
              {t("codexConfig.modelName", { defaultValue: "模型名称" })}
            </label>
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
          </div>
          <ModelInputWithFetch
            id="codexModelName"
            value={modelName}
            onChange={(v) => onModelNameChange!(v)}
            placeholder={t("codexConfig.modelNamePlaceholder", {
              defaultValue: "例如: gpt-5.4",
            })}
            fetchedModels={fetchedModels}
            isLoading={isFetchingModels}
          />
          <p className="text-xs text-muted-foreground">
            {modelName.trim()
              ? t("codexConfig.modelNameHint", {
                  defaultValue: "指定使用的模型，将自动更新到 config.toml 中",
                })
              : t("providerForm.modelHint", {
                  defaultValue: "💡 留空将使用供应商的默认模型",
                })}
          </p>
        </div>
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
