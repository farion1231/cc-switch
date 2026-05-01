import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProviderIcon } from "./ProviderIcon";
import { iconList } from "@/icons/extracted";
import { searchIcons, getIconMetadata } from "@/icons/extracted/metadata";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { cn } from "@/lib/utils";
import { isHttpsIconUrl } from "@/utils/iconUtils";

interface IconPickerProps {
  value?: string; // 当前选中的图标
  onValueChange: (icon: string) => void; // 选择回调
  color?: string; // 预览颜色
}

export const IconPicker: React.FC<IconPickerProps> = ({
  value,
  onValueChange,
}) => {
  const { t } = useTranslation();
  const [searchQuery, setSearchQuery] = useState(() =>
    isHttpsIconUrl(value) ? (value ?? "") : "",
  );
  const debouncedSearchQuery = useDebouncedValue(searchQuery, 250);
  const debouncedTrimmedQuery = debouncedSearchQuery.trim();
  const isImageUrl = isHttpsIconUrl(debouncedTrimmedQuery);

  // 过滤图标列表
  const filteredIcons = useMemo(() => {
    if (isImageUrl) return [];
    if (!searchQuery) return iconList;
    return searchIcons(searchQuery);
  }, [isImageUrl, searchQuery]);

  return (
    <div className="space-y-4">
      <div>
        <Label htmlFor="icon-search">
          {t("iconPicker.search", { defaultValue: "搜索图标" })}
        </Label>
        <Input
          id="icon-search"
          type="text"
          placeholder={t("iconPicker.searchPlaceholder", {
            defaultValue: "输入图标名称...",
          })}
          value={searchQuery}
          onChange={(e) => {
            const nextValue = e.target.value;
            const trimmedValue = nextValue.trim();
            setSearchQuery(nextValue);

            if (isHttpsIconUrl(trimmedValue)) {
              if (value !== trimmedValue) {
                onValueChange(trimmedValue);
              }
              return;
            }

            if (!trimmedValue && isHttpsIconUrl(value)) {
              onValueChange("");
            }
          }}
          className="mt-2"
        />
      </div>

      {isImageUrl ? (
        <button
          type="button"
          onClick={() => onValueChange(debouncedTrimmedQuery)}
          className={cn(
            "flex w-full items-center gap-3 rounded-lg border-2 p-3 text-left",
            value === debouncedTrimmedQuery
              ? "border-primary bg-primary/10"
              : "border-transparent hover:bg-accent hover:border-primary/50",
          )}
          title={debouncedTrimmedQuery}
        >
          <ProviderIcon
            icon={debouncedTrimmedQuery}
            name={t("providerIcon.imageUrl", { defaultValue: "Image URL" })}
            size={32}
          />
          <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground">
            {debouncedTrimmedQuery}
          </span>
        </button>
      ) : (
        <div className="max-h-[65vh] overflow-y-auto pr-1">
          <div className="grid grid-cols-6 sm:grid-cols-8 lg:grid-cols-10 gap-2">
            {filteredIcons.map((iconName) => {
              const meta = getIconMetadata(iconName);
              const isSelected = value === iconName;

              return (
                <button
                  key={iconName}
                  type="button"
                  onClick={() => onValueChange(iconName)}
                  className={cn(
                    "flex flex-col items-center gap-1 p-3 rounded-lg",
                    "border-2 transition-all duration-200",
                    "hover:bg-accent hover:border-primary/50",
                    isSelected
                      ? "border-primary bg-primary/10"
                      : "border-transparent",
                  )}
                  title={meta?.displayName || iconName}
                >
                  <ProviderIcon icon={iconName} name={iconName} size={32} />
                  <span className="text-xs text-muted-foreground truncate w-full text-center">
                    {meta?.displayName || iconName}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {!isImageUrl && filteredIcons.length === 0 && (
        <div className="text-center py-8 text-muted-foreground">
          {t("iconPicker.noResults", { defaultValue: "未找到匹配的图标" })}
        </div>
      )}
    </div>
  );
};
