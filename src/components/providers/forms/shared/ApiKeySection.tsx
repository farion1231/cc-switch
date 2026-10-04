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
  /** 保存时会校验 Key 非空的表单才传；官方 / 云厂商 / 禁用时不标星 */
  required?: boolean;
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
  required = false,
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
  const isDisabled = disabled ?? category === "official";
  const isRequired =
    required &&
    !isDisabled &&
    category !== "official" &&
    category !== "cloud_provider";

  const showLink = shouldShowLink && Boolean(websiteUrl);
  // 推广语跟着「获取 API Key」走，按输入框说明文字的样式写在框下面（v7 不画推广框）
  const promotion =
    showLink && partnerPromotionKey
      ? t(`providerForm.partnerPromotion.${partnerPromotionKey}`, {
          defaultValue: "",
        })
      : "";

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
      disabled={isDisabled}
      required={isRequired}
      labelAside={
        showLink ? (
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
      hint={promotion || undefined}
    />
  );
}
