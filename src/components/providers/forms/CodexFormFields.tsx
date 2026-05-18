import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { Download, Loader2 } from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ProviderCategory } from "@/types";

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

  // Model Name
  shouldShowModelField?: boolean;
  modelName?: string;
  onModelNameChange?: (model: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // Chat Compat mode
  chatCompat: boolean;
  onChatCompatChange: (enabled: boolean) => void;

  // Model mapping (e.g. {"gpt-5.4": "deepseek-v4-flash"})
  modelMapJson: string;
  onModelMapJsonChange: (json: string) => void;
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
  shouldShowModelField = true,
  modelName = "",
  onModelNameChange,
  speedTestEndpoints,
  chatCompat,
  onChatCompatChange,
  modelMapJson,
  onModelMapJsonChange,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

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
          placeholder={t("providerForm.codexApiEndpointPlaceholder")}
          hint={t("providerForm.codexApiHint")}
          showFullUrlToggle
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* Chat Compat 格式转换开关 */}
      <div className="flex items-center justify-between p-3 bg-blue-50 dark:bg-blue-950/30 border border-blue-200 dark:border-blue-800 rounded-md">
        <div className="flex flex-col gap-1">
          <label
            htmlFor="codexChatCompat"
            className="text-sm font-medium text-foreground cursor-pointer"
          >
            {t("codexConfig.chatCompatLabel", {
              defaultValue: "Chat Compat 模式",
            })}
          </label>
          <p className="text-xs text-muted-foreground max-w-md">
            {t("codexConfig.chatCompatHint", {
              defaultValue:
                "启用后将 Codex 的 Responses API 请求转换为 Chat Completions 格式发给上游（如 DeepSeek），并将响应转换回 Responses API 格式",
            })}
          </p>
        </div>
        <label className="relative inline-flex items-center cursor-pointer shrink-0">
          <input
            id="codexChatCompat"
            type="checkbox"
            checked={chatCompat}
            onChange={(e) => onChatCompatChange(e.target.checked)}
            className="sr-only peer"
          />
          <div className="w-9 h-5 bg-gray-200 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-gray-600 peer-checked:bg-blue-600" />
        </label>
      </div>

      {/* 模型名称映射 */}
      {chatCompat && (
        <div className="space-y-2">
          <label
            htmlFor="codexModelMap"
            className="block text-sm font-medium text-foreground"
          >
            {t("codexConfig.modelMapLabel", {
              defaultValue: "模型名称映射",
            })}
          </label>
          <textarea
            id="codexModelMap"
            value={modelMapJson}
            onChange={(e) => onModelMapJsonChange(e.target.value)}
            placeholder='{"gpt-5.4": "deepseek-v4-flash"}'
            rows={3}
            className="w-full px-3 py-2 text-sm font-mono border border-border rounded-md bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-blue-500 dark:focus:ring-blue-400"
          />
          <p className="text-xs text-muted-foreground">
            {t("codexConfig.modelMapHint", {
              defaultValue:
                "JSON 对象，Codex 发送的模型名 → 上游真实模型名。例如将 gpt-5.4 映射为 deepseek-v4-flash",
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
