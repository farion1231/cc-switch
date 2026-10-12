import React, { useEffect, useState, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Upload, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { ProviderIcon } from "./ProviderIcon";
import { ProviderIconBox } from "./ProviderIconBox";
import { iconList } from "@/icons/extracted";
import { searchIcons, getIconMetadata } from "@/icons/extracted/metadata";
import { cn } from "@/lib/utils";
import { toast } from "@/lib/toast";
import {
  CUSTOM_ICON_ACCEPT,
  CustomIconError,
  isCustomIcon,
  normalizeIconFile,
} from "@/utils/customIcon";

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
  const [uploading, setUploading] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  // 每次选择（上传、移除、点内置图标）都加一：处理中的上传完成时序号已经变了，
  // 说明用户后来又做了别的选择，旧结果作废，不能把它写回去
  const selectionSeqRef = useRef(0);
  const customIcon = isCustomIcon(value) ? value : undefined;

  // 关掉选择器也让处理中的上传作废：重新打开是一个新实例、新序号，管不到旧上传，
  // 旧上传完成后会把用户在新选择器里的选择覆盖掉
  useEffect(
    () => () => {
      selectionSeqRef.current += 1;
    },
    [],
  );

  const select = (icon: string) => {
    selectionSeqRef.current += 1;
    setUploading(false);
    onValueChange(icon);
  };

  // 上传只改表单里的值，点保存才写进库：取消编辑时什么都不会留下
  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    // 清空，同一个文件改完再选一次也能触发 change
    e.target.value = "";
    if (!file) return;
    const seq = ++selectionSeqRef.current;
    setUploading(true);
    try {
      const icon = await normalizeIconFile(file);
      if (seq === selectionSeqRef.current) onValueChange(icon);
    } catch (error) {
      if (seq !== selectionSeqRef.current) return;
      const code =
        error instanceof CustomIconError ? error.code : "decodeFailed";
      toast.error(t(`iconPicker.uploadError.${code}`));
    } finally {
      if (seq === selectionSeqRef.current) setUploading(false);
    }
  };

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
        <div className="mt-2 flex gap-2">
          <Input
            id="icon-search"
            type="text"
            placeholder={t("iconPicker.searchPlaceholder", {
              defaultValue: "输入图标名称...",
            })}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          <HoverTip content={t("iconPicker.uploadHint")}>
            <Button
              type="button"
              variant="outline"
              disabled={uploading}
              onClick={() => fileInputRef.current?.click()}
            >
              <Upload className="h-4 w-4" />
              {t("iconPicker.upload")}
            </Button>
          </HoverTip>
          <input
            ref={fileInputRef}
            type="file"
            accept={CUSTOM_ICON_ACCEPT}
            className="hidden"
            data-testid="icon-upload-input"
            onChange={handleFileChange}
          />
        </div>
      </div>

      {customIcon && (
        <div className="flex items-center gap-3 rounded-lg border-2 border-primary bg-primary/10 p-3">
          <ProviderIconBox icon={customIcon} name="" />
          <span className="flex-1 text-sm">{t("iconPicker.customIcon")}</span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => select("")}
          >
            <X className="h-4 w-4" />
            {t("iconPicker.removeCustom")}
          </Button>
        </div>
      )}

      <div className="max-h-[65vh] overflow-y-auto pr-1">
        <div className="grid grid-cols-6 sm:grid-cols-8 lg:grid-cols-10 gap-2">
          {filteredIcons.map((iconName) => {
            const meta = getIconMetadata(iconName);
            const isSelected = value === iconName;

            return (
              <button
                key={iconName}
                type="button"
                onClick={() => select(iconName)}
                className={cn(
                  "flex flex-col items-center gap-1 p-3 rounded-lg",
                  "border-2 transition-all duration-200",
                  "hover:bg-subtle hover:border-primary/50",
                  isSelected
                    ? "border-primary bg-primary/10"
                    : "border-transparent",
                )}
                title={meta?.displayName || iconName}
              >
                <ProviderIcon icon={iconName} name={iconName} size={32} />
                <span className="text-xs text-fg-2 truncate w-full text-center">
                  {meta?.displayName || iconName}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {filteredIcons.length === 0 && (
        <div className="text-center py-8 text-fg-2">
          {t("iconPicker.noResults", { defaultValue: "未找到匹配的图标" })}
        </div>
      )}
    </div>
  );
};
