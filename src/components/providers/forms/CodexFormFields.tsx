import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { Download, Loader2, Minus, Plus } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FormLabel } from "@/components/ui/form";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
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

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // API Format
  apiFormat: string;
  onApiFormatChange: (format: string) => void;

  // Model Mapping
  codexModels: string[];
  onCodexModelsChange: (models: string[]) => void;
  codexDefaultModel: string;
  onCodexDefaultModelChange: (model: string) => void;
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
  speedTestEndpoints,
  apiFormat,
  onApiFormatChange,
  codexModels,
  onCodexModelsChange,
  onCodexDefaultModelChange,
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

      {/* Codex API 格式选择 */}
      {shouldShowSpeedTest && (
        <div className="space-y-2">
          <FormLabel htmlFor="codexApiFormat">
            {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
          </FormLabel>
          <Select value={apiFormat} onValueChange={onApiFormatChange}>
            <SelectTrigger id="codexApiFormat" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="openai_responses">
                {t("providerForm.apiFormatOpenAIResponses", {
                  defaultValue: "OpenAI Responses API (原生)",
                })}
              </SelectItem>
              <SelectItem value="openai_chat">
                {t("providerForm.apiFormatOpenAIChat", {
                  defaultValue: "OpenAI Chat Completions (需开启路由)",
                })}
              </SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.codexApiFormatHint", {
              defaultValue:
                "OpenAI Responses API 是 Codex CLI 原生格式。选择 Chat Completions 需要 cc-switch 路由进行格式转换。",
            })}
          </p>
        </div>
      )}

      {/* 获取模型 + 模型映射（动态行，第一行为默认模型） */}
      {shouldShowSpeedTest && (
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <label className="block text-sm font-medium text-foreground">
              {t("providerForm.modelMapping", { defaultValue: "模型映射" })}
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

          {(fetchedModels.length > 0 || codexModels.length > 0) && (
            <div className="space-y-1.5">
              {codexModels.length === 0 && (
                <p className="text-xs text-muted-foreground">
                  {t("providerForm.addModelRow", {
                    defaultValue: "点击 + 添加模型，第一个模型作为默认模型",
                  })}
                </p>
              )}
              {codexModels.map((modelId, index) => {
                // 合并已保存但不在 fetchedModels 中的模型名，确保显示
                const allOptions = [
                  ...fetchedModels,
                  ...(!fetchedModels.some((m) => m.id === modelId)
                    ? [{ id: modelId, ownedBy: null as string | null }]
                    : []),
                ];
                const availableModels = allOptions.filter(
                  (m) => !codexModels.includes(m.id) || m.id === modelId,
                );
                const hasFetched = fetchedModels.length > 0;
                return (
                  <div key={index} className="flex items-center gap-2">
                    <div className="flex-1">
                      {hasFetched ? (
                        <select
                          value={modelId}
                          onChange={(e) => {
                            const newId = e.target.value;
                            const updated = [...codexModels];
                            updated[index] = newId;
                            onCodexModelsChange(updated);
                            if (index === 0) {
                              onCodexDefaultModelChange(newId);
                            }
                          }}
                          className="flex h-9 w-full rounded-md border border-border-default bg-background px-3 py-2 text-sm focus:outline-none focus:border-border-active"
                        >
                          {availableModels.length === 0 ? (
                            <option value={modelId}>{modelId}</option>
                          ) : (
                            availableModels.map((m) => (
                              <option key={m.id} value={m.id}>
                                {m.id}{index === 0 ? " (默认)" : ""}
                              </option>
                            ))
                          )}
                        </select>
                      ) : (
                        <div className="flex h-9 items-center rounded-md border px-3 text-sm">
                          {modelId}
                          {index === 0 && (
                            <span className="ml-1 text-muted-foreground">
                              {t("providerForm.defaultModelTag", {
                                defaultValue: "(默认)",
                              })}
                            </span>
                          )}
                        </div>
                      )}
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                      onClick={() => {
                        const updated = codexModels.filter(
                          (_, i) => i !== index,
                        );
                        onCodexModelsChange(updated);
                        if (index === 0 && updated.length > 0) {
                          onCodexDefaultModelChange(updated[0]);
                        }
                      }}
                    >
                      <Minus className="h-4 w-4" />
                    </Button>
                  </div>
                );
              })}
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="gap-1"
                onClick={() => {
                  const available = fetchedModels.find(
                    (m) => !codexModels.includes(m.id),
                  );
                  if (available) {
                    const updated = [...codexModels, available.id];
                    onCodexModelsChange(updated);
                    if (updated.length === 1) {
                      onCodexDefaultModelChange(available.id);
                    }
                  }
                }}
                disabled={codexModels.length >= fetchedModels.length}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("providerForm.addModel", { defaultValue: "添加模型" })}
              </Button>
            </div>
          )}
          {fetchedModels.length === 0 && codexModels.length === 0 && !isFetchingModels && (
            <p className="text-xs text-muted-foreground">
              {t("providerForm.fetchModelsToConfigure", {
                defaultValue: "输入 API Key 和 Base URL 后点击'获取模型'来配置模型映射",
              })}
            </p>
          )}

        </div>
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
