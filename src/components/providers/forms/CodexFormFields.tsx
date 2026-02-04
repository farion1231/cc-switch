import { useTranslation } from "react-i18next";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderCategory } from "@/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FormLabel } from "@/components/ui/form";

interface EndpointCandidate {
  url: string;
}

// Codex API 格式类型
export type CodexApiFormat = "responses" | "chat";

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

  // API Format (for Codex providers)
  apiFormat?: CodexApiFormat;
  onApiFormatChange?: (format: CodexApiFormat) => void;
  shouldShowApiFormatSelector?: boolean;
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
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelField = true,
  modelName = "",
  onModelNameChange,
  speedTestEndpoints,
  apiFormat = "responses",
  onApiFormatChange,
  shouldShowApiFormatSelector = false,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  // 根据 API 格式选择提示文本
  const apiHint =
    apiFormat === "chat"
      ? t("providerForm.codexApiHintChat", {
          defaultValue:
            "💡 填写兼容 OpenAI Chat Completions 格式的服务端点地址",
        })
      : t("providerForm.codexApiHint");

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

      {/* API 格式选择器 */}
      {shouldShowApiFormatSelector && onApiFormatChange && (
        <div className="space-y-2">
          <FormLabel htmlFor="codexApiFormat">
            {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
          </FormLabel>
          <Select value={apiFormat} onValueChange={onApiFormatChange}>
            <SelectTrigger id="codexApiFormat" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="responses">
                {t("providerForm.codexApiFormatResponses", {
                  defaultValue: "OpenAI Responses (默认)",
                })}
              </SelectItem>
              <SelectItem value="chat">
                {t("providerForm.codexApiFormatChat", {
                  defaultValue: "OpenAI Chat Completions",
                })}
              </SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.codexApiFormatHint", {
              defaultValue: "选择供应商支持的 API 格式",
            })}
          </p>
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
          hint={apiHint}
          onManageClick={() => onEndpointModalToggle(true)}
          appType="codex"
          apiFormat={apiFormat}
        />
      )}

      {/* Codex Model Name 输入框 */}
      {shouldShowModelField && onModelNameChange && (
        <div className="space-y-2">
          <label
            htmlFor="codexModelName"
            className="block text-sm font-medium text-foreground"
          >
            {t("codexConfig.modelName", { defaultValue: "模型名称" })}
          </label>
          <input
            id="codexModelName"
            type="text"
            value={modelName}
            onChange={(e) => onModelNameChange(e.target.value)}
            placeholder={t("codexConfig.modelNamePlaceholder", {
              defaultValue: "例如: gpt-5-codex",
            })}
            className="w-full px-3 py-2 border border-border-default bg-background text-foreground rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 transition-colors"
          />
          <p className="text-xs text-muted-foreground">
            {t("codexConfig.modelNameHint", {
              defaultValue: "指定使用的模型，将自动更新到 config.toml 中",
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
