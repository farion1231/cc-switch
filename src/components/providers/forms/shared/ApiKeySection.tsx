import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import ApiKeyInput from "../ApiKeyInput";
import { Input } from "@/components/ui/input";
import type { ProviderApiKey, ProviderCategory } from "@/types";

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
  apiKeys?: ProviderApiKey[];
  onApiKeysChange?: (keys: ProviderApiKey[]) => void;
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
  apiKeys,
  onApiKeysChange,
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

  const keys = apiKeys ?? [{ key: value }];
  const supportsMultiple = onApiKeysChange !== undefined;
  const [selectedIndex, setSelectedIndex] = useState(() =>
    Math.max(
      keys.findIndex(({ key }) => key === value),
      0,
    ),
  );
  const changeKey = (index: number, key: string) => {
    const next = [...keys];
    next[index] = { ...next[index], key };
    onApiKeysChange?.(next);
    if (index === selectedIndex) onChange(key);
  };
  const changeNote = (index: number, note: string) => {
    const next = [...keys];
    next[index] = { ...next[index], note };
    onApiKeysChange?.(next);
  };
  const selectKey = (index: number) => {
    setSelectedIndex(index);
    onChange(keys[index].key);
  };
  const addKey = () => {
    const next = [...keys, { key: "" }];
    onApiKeysChange?.(next);
    setSelectedIndex(next.length - 1);
    onChange("");
  };
  const removeKey = (index: number) => {
    if (keys.length === 1) return;
    const next = keys.filter((_, i) => i !== index);
    onApiKeysChange?.(next);
    if (index === selectedIndex) {
      const nextIndex = Math.min(index, next.length - 1);
      setSelectedIndex(nextIndex);
      onChange(next[nextIndex].key);
    } else if (index < selectedIndex) {
      setSelectedIndex(selectedIndex - 1);
    }
  };

  return (
    <div className="space-y-2">
      <label
        htmlFor={id ?? "apiKey"}
        className="block text-sm font-medium text-foreground"
      >
        {label ?? "API Key"}
      </label>
      {keys.map((entry, index) => (
        <div key={index} className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <ApiKeyInput
              id={index === 0 ? id : `${id}-${index}`}
              label=""
              value={entry.key}
              onChange={(next) => changeKey(index, next)}
              placeholder={
                category === "official"
                  ? finalPlaceholder.official
                  : finalPlaceholder.thirdParty
              }
              disabled={disabled ?? category === "official"}
            />
          </div>
          <Input
            value={entry.note ?? ""}
            onChange={(event) => changeNote(index, event.target.value)}
            placeholder={t("providerForm.apiKeyNote", { defaultValue: "备注" })}
            disabled={disabled ?? category === "official"}
            className="w-40"
          />
          <input
            type="radio"
            checked={index === selectedIndex}
            onChange={() => selectKey(index)}
            aria-label={t("providerForm.enableApiKey", {
              defaultValue: "启用此 API Key",
            })}
            disabled={disabled ?? category === "official"}
            className="size-4 shrink-0 accent-primary"
          />
          {supportsMultiple && keys.length > 1 && (
            <button
              type="button"
              onClick={() => removeKey(index)}
              className="mb-1 p-2 text-muted-foreground hover:text-destructive"
              aria-label={t("providerForm.removeApiKey", {
                defaultValue: "删除 API Key",
              })}
            >
              <Trash2 size={16} />
            </button>
          )}
        </div>
      ))}
      {supportsMultiple && (
        <button
          type="button"
          onClick={addKey}
          disabled={disabled ?? category === "official"}
          className="flex items-center gap-1 text-sm text-primary hover:underline disabled:cursor-not-allowed disabled:text-muted-foreground"
        >
          <Plus size={16} />
          {t("providerForm.addApiKey", { defaultValue: "新增 API Key" })}
        </button>
      )}
      {/* API Key 获取链接 */}
      {shouldShowLink && websiteUrl && (
        <div className="space-y-2 -mt-1 pl-1">
          <a
            href={websiteUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
          >
            {t("providerForm.getApiKey", {
              defaultValue: "获取 API Key",
            })}
          </a>

          {/* 促销信息（与 isPartner 解耦：仅凭 partnerPromotionKey 即可展示，星标仍由 isPartner 控制） */}
          {partnerPromotionKey && (
            <div className="rounded-md bg-blue-50 dark:bg-blue-950/30 p-2.5 border border-blue-200 dark:border-blue-800">
              <p className="text-xs leading-relaxed text-blue-700 dark:text-blue-300">
                💡{" "}
                {t(`providerForm.partnerPromotion.${partnerPromotionKey}`, {
                  defaultValue: "",
                })}
              </p>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
