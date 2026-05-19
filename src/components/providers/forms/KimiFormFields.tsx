import { useTranslation } from "react-i18next";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderCategory } from "@/types";

interface KimiFormFieldsProps {
  providerId?: string;
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
}

export function KimiFormFields({
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
}: KimiFormFieldsProps) {
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

      {/* Base URL 输入框 */}
      <EndpointField
        id="baseUrl"
        label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
        value={baseUrl}
        onChange={onBaseUrlChange}
        placeholder={t("providerForm.apiEndpointPlaceholder", {
          defaultValue: "https://your-api-endpoint.com/",
        })}
      />
    </>
  );
}
