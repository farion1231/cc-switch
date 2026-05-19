import { useTranslation } from "react-i18next";
import { ApiKeySection, EndpointField } from "./shared";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { ProviderCategory } from "@/types";

interface KimiFormFieldsProps {
  providerId?: string;
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  providerName: string;
  onProviderNameChange: (name: string) => void;
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
  providerName,
  onProviderNameChange,
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
    <div className="space-y-4">
      {/* Provider 名称输入框 */}
      <div className="space-y-2">
        <Label htmlFor="kimiProviderName">
          {t("providerForm.providerName", { defaultValue: "Provider 名称" })}
        </Label>
        <Input
          id="kimiProviderName"
          value={providerName}
          onChange={(e) => onProviderNameChange(e.target.value)}
          placeholder={t("providerForm.providerNamePlaceholder", {
            defaultValue: "如 ccswitch、office、personal",
          })}
        />
        <p className="text-xs text-muted-foreground">
          {t("providerForm.providerNameHint", {
            defaultValue: "对应 ~/.kimi/config.toml 中的 [providers.{名字}]",
          })}
        </p>
      </div>

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
    </div>
  );
}
