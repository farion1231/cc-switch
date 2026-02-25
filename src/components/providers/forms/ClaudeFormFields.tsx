import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";
import type { TemplateValueConfig } from "@/config/claudeProviderPresets";

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

  // Auth Key Field (ANTHROPIC_AUTH_TOKEN vs ANTHROPIC_API_KEY)
  apiKeyField: ClaudeApiKeyField;
  onApiKeyFieldChange: (field: ClaudeApiKeyField) => void;
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
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();

  return (
    <>
      {/* API Key 输入框 */}
      {shouldShowApiKey && (
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
            apiFormat === "openai_chat"
              ? t("providerForm.apiHintOAI")
              : t("providerForm.apiHint")
          }
          onManageClick={() => onEndpointModalToggle(true)}
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

      {/* API 格式选择（仅非官方、非云服务商显示） */}
      {shouldShowModelSelector && category !== "cloud_provider" && (
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
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.apiFormatHint", {
              defaultValue: "选择供应商 API 的输入格式",
            })}
          </p>
        </div>
      )}

      {/* 认证字段选择（仅非官方供应商显示） */}
      {shouldShowModelSelector && (
        <div className="space-y-2">
          <FormLabel htmlFor="apiKeyField">
            {t("providerForm.authField", { defaultValue: "认证字段" })}
          </FormLabel>
          <Select
            value={apiKeyField}
            onValueChange={(v) => onApiKeyFieldChange(v as ClaudeApiKeyField)}
          >
            <SelectTrigger id="apiKeyField" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                {t("providerForm.authFieldAuthToken", {
                  defaultValue: "Auth Token (默认)",
                })}
              </SelectItem>
              <SelectItem value="ANTHROPIC_API_KEY">
                {t("providerForm.authFieldApiKey", {
                  defaultValue: "API Key",
                })}
              </SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.authFieldHint", {
              defaultValue:
                "大多数第三方供应商使用 Auth Token；少数供应商需要 API Key",
            })}
          </p>
        </div>
      )}

      {/* 模型选择器 */}
      {shouldShowModelSelector && (
        <div className="space-y-3">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* 主模型 */}
            <div className="space-y-2">
              <FormLabel htmlFor="claudeModel">
                {t("providerForm.anthropicModel", { defaultValue: "主模型" })}
              </FormLabel>
              <Input
                id="claudeModel"
                type="text"
                value={claudeModel}
                onChange={(e) =>
                  onModelChange("ANTHROPIC_MODEL", e.target.value)
                }
                placeholder={t("providerForm.modelPlaceholder", {
                  defaultValue: "",
                })}
                autoComplete="off"
              />
            </div>

            {/* 推理模型 */}
            <div className="space-y-2">
              <FormLabel htmlFor="reasoningModel">
                {t("providerForm.anthropicReasoningModel")}
              </FormLabel>
              <Input
                id="reasoningModel"
                type="text"
                value={reasoningModel}
                onChange={(e) =>
                  onModelChange("ANTHROPIC_REASONING_MODEL", e.target.value)
                }
                autoComplete="off"
              />
            </div>

            {/* 默认 Haiku */}
            <div className="space-y-2">
              <FormLabel htmlFor="claudeDefaultHaikuModel">
                {t("providerForm.anthropicDefaultHaikuModel", {
                  defaultValue: "Haiku 默认模型",
                })}
              </FormLabel>
              <Input
                id="claudeDefaultHaikuModel"
                type="text"
                value={defaultHaikuModel}
                onChange={(e) =>
                  onModelChange("ANTHROPIC_DEFAULT_HAIKU_MODEL", e.target.value)
                }
                placeholder={t("providerForm.haikuModelPlaceholder", {
                  defaultValue: "",
                })}
                autoComplete="off"
              />
            </div>

            {/* 默认 Sonnet */}
            <div className="space-y-2">
              <FormLabel htmlFor="claudeDefaultSonnetModel">
                {t("providerForm.anthropicDefaultSonnetModel", {
                  defaultValue: "Sonnet 默认模型",
                })}
              </FormLabel>
              <Input
                id="claudeDefaultSonnetModel"
                type="text"
                value={defaultSonnetModel}
                onChange={(e) =>
                  onModelChange(
                    "ANTHROPIC_DEFAULT_SONNET_MODEL",
                    e.target.value,
                  )
                }
                placeholder={t("providerForm.modelPlaceholder", {
                  defaultValue: "",
                })}
                autoComplete="off"
              />
            </div>

            {/* 默认 Opus */}
            <div className="space-y-2">
              <FormLabel htmlFor="claudeDefaultOpusModel">
                {t("providerForm.anthropicDefaultOpusModel", {
                  defaultValue: "Opus 默认模型",
                })}
              </FormLabel>
              <Input
                id="claudeDefaultOpusModel"
                type="text"
                value={defaultOpusModel}
                onChange={(e) =>
                  onModelChange("ANTHROPIC_DEFAULT_OPUS_MODEL", e.target.value)
                }
                placeholder={t("providerForm.modelPlaceholder", {
                  defaultValue: "",
                })}
                autoComplete="off"
              />
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.modelHelper", {
              defaultValue:
                "可选：指定默认使用的 Claude 模型，留空则使用系统默认。",
            })}
          </p>
        </div>
      )}
    </>
  );
}
