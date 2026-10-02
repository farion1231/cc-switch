import { useTranslation } from "react-i18next";
import ApiKeyInput from "../ApiKeyInput";
import type { ProviderCategory } from "@/types";

interface ApiKeySectionProps {
  id?: string;
  label?: string;
  value: string;
  onChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowLink: boolean;
  websiteUrl: string;
  placeholder?: {
    official: string;
    thirdParty: string;
  };
  disabled?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
}

export function ApiKeySection({
  id,
  label,
  value,
  onChange,
  category,
  shouldShowLink,
  websiteUrl,
  placeholder,
  disabled,
  partnerPromotionKey,
}: ApiKeySectionProps) {
  const { t } = useTranslation();

  const defaultPlaceholder = {
    official: t("providerForm.officialNoApiKey", {
      defaultValue: "官方供应商无需 API Key",
    }),
    thirdParty: t("providerForm.apiKeyAutoFill", {
      defaultValue: "输入 API Key，将自动填充到配置",
    }),
  };

  const finalPlaceholder = placeholder || defaultPlaceholder;

  // 促销框已取消（v7 不画任何推广样式）；partnerPromotionKey 仍有别的判断在用，只是不再渲染。
  void partnerPromotionKey;

  return (
    <ApiKeyInput
      id={id}
      label={label}
      value={value}
      onChange={onChange}
      placeholder={
        category === "official"
          ? finalPlaceholder.official
          : finalPlaceholder.thirdParty
      }
      disabled={disabled ?? category === "official"}
      labelAside={
        shouldShowLink && websiteUrl ? (
          <a
            href={websiteUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="text-caption text-fg-1 underline underline-offset-2 hover:text-fg-2"
          >
            {t("providerForm.getApiKey", { defaultValue: "获取 API Key" })} ↗
          </a>
        ) : null
      }
    />
  );
}
