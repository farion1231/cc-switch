import { FormLabel } from "@/components/ui/form";
import { useTranslation } from "react-i18next";
import type { ProviderCategory } from "@/types";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";

interface QwenFormFieldsProps {
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  model: string;
  onModelChange: (value: string) => void;
}

export function QwenFormFields({
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  model,
  onModelChange,
}: QwenFormFieldsProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      {shouldShowApiKey && (
        <ApiKeySection
          id="qwen-api-key"
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}

      <EndpointField
        id="qwen-base-url"
        label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
        value={baseUrl}
        onChange={onBaseUrlChange}
        placeholder="https://dashscope.aliyuncs.com/compatible-mode/v1"
        showManageButton={false}
      />

      <div className="space-y-2">
        <FormLabel htmlFor="qwen-model">
          {t("provider.form.qwen.model", { defaultValue: "模型" })}
        </FormLabel>
        <ModelInputWithFetch
          id="qwen-model"
          value={model}
          onChange={onModelChange}
          placeholder="qwen3-coder-plus"
          fetchedModels={[]}
          isLoading={false}
        />
      </div>
    </div>
  );
}
