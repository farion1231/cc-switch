import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Loader2 } from "lucide-react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
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
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { copilotGetModels } from "@/lib/api/copilot";
import type { CopilotModel } from "@/lib/api/copilot";
import type { ProviderCategory, ClaudeApiFormat } from "@/types";
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

  // GitHub Copilot OAuth
  isCopilotPreset?: boolean;

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
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();

  // Copilot 可用模型列表
  const [copilotModels, setCopilotModels] = useState<CopilotModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);

  useEffect(() => {
    if (!isCopilotPreset) return;

    let cancelled = false;
    setModelsLoading(true);
    console.log("[Copilot] Fetching models, isCopilotPreset:", isCopilotPreset);
    copilotGetModels()
      .then((models) => {
        console.log("[Copilot] Fetched models:", models.length, models);
        if (!cancelled) setCopilotModels(models);
      })
      .catch((err) => {
        console.warn("[Copilot] Failed to fetch models:", err);
      })
      .finally(() => {
        if (!cancelled) setModelsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isCopilotPreset]);

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

    return (
      <Input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onModelChange(field, e.target.value)}
        placeholder={placeholder}
        autoComplete="off"
      />
    );
  };

  return (
    <>
      {/* GitHub Copilot OAuth 认证 */}
      {isCopilotPreset && <CopilotAuthSection />}

      {/* API Key 输入框（非 Copilot 预设时显示） */}
      {shouldShowApiKey && !isCopilotPreset && (
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

      {/* API 格式选择（仅非官方供应商显示） */}
      {shouldShowModelSelector && (
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

      {/* 模型选择器 */}
      {shouldShowModelSelector && (
        <div className="space-y-3">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* 主模型 */}
            <div className="space-y-2">
              <FormLabel htmlFor="claudeModel">
                {t("providerForm.anthropicModel", { defaultValue: "主模型" })}
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
