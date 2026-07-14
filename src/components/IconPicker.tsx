import React, { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProviderIcon } from "./ProviderIcon";
import { iconList } from "@/icons/extracted";
import { searchIcons, getIconMetadata } from "@/icons/extracted/metadata";
import { cn } from "@/lib/utils";

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
  const [searchQuery, setSearchQuery] = useState("");

  // 过滤图标列表
  const filteredIcons = useMemo(() => {
    if (!searchQuery) return iconList;
    return searchIcons(searchQuery);
  }, [searchQuery]);

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
          onChange={(e) => setSearchQuery(e.target.value)}
          className="mt-2"
        />
      </div>

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

      {filteredIcons.length === 0 && (
        <div className="text-center py-8 text-muted-foreground">
          {t("iconPicker.noResults", { defaultValue: "未找到匹配的图标" })}
        </div>
      )}
    </div>
  );
};
