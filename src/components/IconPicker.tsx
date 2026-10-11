import React, { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Upload, Trash2, Link2, Check } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { ProviderIconBox } from "@/components/ProviderIconBox";
import type { ProviderIconSource } from "@/components/ProviderIcon";
import {
  pickProviderIconFile,
  saveProviderIcon,
} from "@/lib/api/providerIcons";
import { iconList } from "@/icons/extracted";
import { searchIcons, getIconMetadata } from "@/icons/extracted/metadata";
import { cn } from "@/lib/utils";

const HTTP_URL_PATTERN = /^https?:\/\//i;

interface IconPickerProps {
  value?: string; // 当前选中的图标（内建名称 / 资产键 / URL）
  iconSource?: ProviderIconSource; // 当前图标来源
  onValueChange: (icon: string, source: ProviderIconSource) => void; // 选择回调
  color?: string; // 预览颜色
  /** 上传资产用的供应商 ID；新增草稿时由调用方传入临时 ID */
  providerId?: string;
  /** 移除自定义图标时的额外清理（如删除磁盘资产），先于状态重置执行 */
  onRemoveCustom?: () => void;
}

export const IconPicker: React.FC<IconPickerProps> = ({
  value,
  iconSource,
  onValueChange,
  providerId,
  onRemoveCustom,
}) => {
  const { t } = useTranslation();
  const [searchQuery, setSearchQuery] = useState("");
  const [urlDraft, setUrlDraft] = useState("");
  const [uploading, setUploading] = useState(false);
  const uploadLockRef = useRef(false);

  const isCustomSource = iconSource === "local" || iconSource === "url";

  // 过滤图标列表
  const filteredIcons = React.useMemo(() => {
    if (!searchQuery) return iconList;
    return searchIcons(searchQuery);
  }, [searchQuery]);

  const handleUpload = async () => {
    if (uploadLockRef.current) return;
    uploadLockRef.current = true;
    setUploading(true);
    try {
      const sourcePath = await pickProviderIconFile();
      if (!sourcePath) return;
      const saved = await saveProviderIcon(providerId ?? "draft", sourcePath);
      onValueChange(saved.key, "local");
    } catch (error) {
      // 后端已本地化错误文案；这里仅提示不中断 picker
      console.error("save_provider_icon failed:", error);
      const message =
        typeof error === "string" ? error : String(error ?? "upload failed");
      alert(message);
    } finally {
      uploadLockRef.current = false;
      setUploading(false);
    }
  };

  const handleApplyUrl = () => {
    const url = urlDraft.trim();
    if (!HTTP_URL_PATTERN.test(url)) return;
    onValueChange(url, "url");
    setUrlDraft("");
  };

  const handleRemoveCustom = () => {
    onRemoveCustom?.();
    onValueChange("", "builtin");
  };

  return (
    <div className="space-y-4">
      {/* 自定义图标（上传 / 粘贴 URL）— issue #8117 */}
      <div className="rounded-lg border border-border p-4 space-y-3">
        <Label>{t("iconPicker.custom", { defaultValue: "自定义图标" })}</Label>
        <div className="flex items-center gap-3">
          {isCustomSource && value ? (
            <ProviderIconBox
              icon={value}
              iconSource={iconSource}
              name={value}
            />
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={uploading}
            onClick={handleUpload}
          >
            <Upload className="h-4 w-4" />
            {uploading
              ? t("iconPicker.uploading", { defaultValue: "上传中…" })
              : t("iconPicker.upload", { defaultValue: "上传本地图片" })}
          </Button>
          {isCustomSource ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={handleRemoveCustom}
            >
              <Trash2 className="h-4 w-4" />
              {t("iconPicker.removeCustom", { defaultValue: "移除自定义图标" })}
            </Button>
          ) : null}
        </div>
        <p className="text-xs text-fg-2">
          {t("iconPicker.customHint", {
            defaultValue:
              "支持 PNG / JPG / WEBP / SVG，≤256×256，光栅图 ≤200KB，SVG ≤100KB",
          })}
        </p>
        <div className="flex items-center gap-2">
          <div className="relative flex-1">
            <Link2 className="absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-3" />
            <Input
              type="url"
              placeholder={t("iconPicker.urlPlaceholder", {
                defaultValue: "https://example.com/icon.png",
              })}
              value={urlDraft}
              onChange={(e) => setUrlDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  handleApplyUrl();
                }
              }}
              className="pl-7"
            />
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!HTTP_URL_PATTERN.test(urlDraft.trim())}
            onClick={handleApplyUrl}
          >
            <Check className="h-4 w-4" />
            {t("iconPicker.applyUrl", { defaultValue: "使用 URL" })}
          </Button>
        </div>
      </div>

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

      <div className="max-h-[55vh] overflow-y-auto pr-1">
        <div className="grid grid-cols-6 sm:grid-cols-8 lg:grid-cols-10 gap-2">
          {filteredIcons.map((iconName) => {
            const meta = getIconMetadata(iconName);
            const isSelected = iconSource !== "local" && value === iconName;

            return (
              <button
                key={iconName}
                type="button"
                onClick={() => onValueChange(iconName, "builtin")}
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
                <ProviderIconBox icon={iconName} name={iconName} />
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
