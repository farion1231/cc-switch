import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Download, Info, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { toast } from "@/lib/toast";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import {
  fetchModelsForConfig,
  getAgyModels,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ProviderCategory } from "@/types";

interface EndpointCandidate {
  url: string;
}

interface GeminiFormFieldsProps {
  appId?: "gemini" | "agy";
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

  // Base URL
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Model
  shouldShowModelField: boolean;
  model: string;
  onModelChange: (value: string) => void;

  // 上游 API 格式（代理路由模式：gemini_native 透传 / openai_chat / anthropic）
  apiFormat?: string;
  onApiFormatChange?: (format: string) => void;
  // 协议转换时的出站模型名覆盖（发给上游的 model；不填用客户端请求的模型名）
  outboundModel?: string;
  onOutboundModelChange?: (model: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];
}

export function GeminiFormFields({
  appId = "gemini",
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelField,
  model,
  onModelChange,
  apiFormat = "gemini_native",
  onApiFormatChange,
  outboundModel = "",
  onOutboundModelChange,
  speedTestEndpoints,
}: GeminiFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const handleFetchModels = useCallback(() => {
    // agy 两种模式都只认它自己清单里的显示名，接口的模型 ID 对不上，所以一律问本机 agy。
    const agy = appId === "agy";
    const modelBaseUrl = baseUrl.trim();
    if (!agy && (!modelBaseUrl || !apiKey)) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!modelBaseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    (agy
      ? getAgyModels({ apiKey, baseUrl: modelBaseUrl })
      : fetchModelsForConfig(modelBaseUrl, apiKey, false)
    )
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
  }, [appId, baseUrl, apiKey, t]);

  // 检测是否为 Google 官方（使用 OAuth）
  const isGoogleOfficial =
    partnerPromotionKey?.toLowerCase() === "google-official";

  return (
    <>
      {/* Google OAuth 提示 */}
      {isGoogleOfficial && (
        <div className="rounded-lg border border-border-strong bg-subtle p-4">
          <div className="flex gap-3">
            <Info className="h-5 w-5 flex-shrink-0 text-fg-1" />
            <div className="space-y-1">
              <p className="text-sm font-medium text-fg-1">
                {t("provider.form.gemini.oauthTitle", {
                  defaultValue: "OAuth 认证模式",
                })}
              </p>
              <p className="text-sm text-fg-1">
                {t("provider.form.gemini.oauthHint", {
                  defaultValue:
                    "Google 官方使用 OAuth 个人认证，无需填写 API Key。首次使用时会自动打开浏览器进行登录。",
                })}
              </p>
            </div>
          </div>
        </div>
      )}

      {/* API Key 输入框 */}
      {shouldShowApiKey && !isGoogleOfficial && (
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

      {/* Base URL 输入框（统一使用与 Codex 相同的样式与交互） */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          required={appId !== "agy"}
          label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder", {
            defaultValue: "https://your-api-endpoint.com/",
          })}
          help={
            appId === "agy" ? t("provider.form.agy.endpointHelp") : undefined
          }
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* 上游 API 格式（代理路由模式：非 Gemini 协议上游需选择实际协议） */}
      {(appId === "gemini" || appId === "agy") &&
        onApiFormatChange &&
        category !== "official" && (
        <div className="space-y-2">
          <div className="flex items-center gap-0.5">
            <FormLabel htmlFor="gemini-api-format">
              {t("provider.form.gemini.apiFormat", {
                defaultValue: "上游 API 格式",
              })}
            </FormLabel>
            <HelpTip
              title={t("provider.form.gemini.apiFormat", {
                defaultValue: "上游 API 格式",
              })}
            >
              {t("provider.form.gemini.apiFormatHelp", {
                defaultValue:
                  "Gemini 协议（透传）：上游原生支持 Gemini API；OpenAI Chat：上游为 OpenAI 兼容接口（如 DeepSeek），代理自动做协议转换；Anthropic：上游为 Anthropic Messages 接口。仅代理路由模式生效。",
              })}
            </HelpTip>
          </div>
          <select
            id="gemini-api-format"
            className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            value={apiFormat}
            onChange={(event) => onApiFormatChange(event.target.value)}
          >
            <option value="gemini_native">
              {t("provider.form.gemini.apiFormatGeminiNative", {
                defaultValue: "Gemini 协议（透传）",
              })}
            </option>
            <option value="openai_chat">
              {t("provider.form.gemini.apiFormatOpenaiChat", {
                defaultValue: "OpenAI Chat（自动转换）",
              })}
            </option>
            <option value="anthropic">
              {t("provider.form.gemini.apiFormatAnthropic", {
                defaultValue: "Anthropic Messages（自动转换）",
              })}
            </option>
          </select>
        </div>
      )}

      {/* 出站模型（协议转换时发给上游的模型名；不填则发送客户端请求的模型名） */}
      {(appId === "gemini" || appId === "agy") &&
        onOutboundModelChange &&
        category !== "official" &&
        apiFormat !== "gemini_native" && (
          <div className="space-y-2">
            <div className="flex items-center gap-0.5">
              <FormLabel htmlFor="gemini-outbound-model">
                {t("provider.form.gemini.outboundModel", {
                  defaultValue: "出站模型",
                })}
              </FormLabel>
              <HelpTip
                title={t("provider.form.gemini.outboundModel", {
                  defaultValue: "出站模型",
                })}
              >
                {t("provider.form.gemini.outboundModelHelp", {
                  defaultValue:
                    "协议转换后发给上游的模型名。客户端请求的模型名（如 agy 的显示名）上游可能不认识，此处填上游实际可用的模型 ID；留空则按客户端请求的模型名发送。",
                })}
              </HelpTip>
            </div>
            <Input
              id="gemini-outbound-model"
              value={outboundModel}
              onChange={(event) =>
                onOutboundModelChange(event.target.value.trim())
              }
              placeholder={t(
                "provider.form.gemini.outboundModelPlaceholder",
                { defaultValue: "上游模型 ID，如 deepseek-v4-pro" },
              )}
            />
          </div>
        )}

      {/* Model 输入框 */}
      {shouldShowModelField && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-0.5">
              <FormLabel htmlFor="gemini-model">
                {t("provider.form.gemini.model", { defaultValue: "模型" })}
              </FormLabel>
              {appId === "agy" && (
                <HelpTip
                  title={t("provider.form.gemini.model", {
                    defaultValue: "模型",
                  })}
                >
                  {t("provider.form.agy.modelHelp")}
                </HelpTip>
              )}
            </div>
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
            id="gemini-model"
            value={model}
            onChange={onModelChange}
            placeholder={
              appId === "agy" && !apiKey
                ? "Gemini 3.8 Flash (High)"
                : "gemini-3.6-flash"
            }
            fetchedModels={fetchedModels}
            isLoading={isFetchingModels}
          />
        </div>
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId={appId}
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
}
