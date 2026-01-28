import { useTranslation } from "react-i18next";
import { useEffect, useState } from "react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Save, Download, Loader2, Eye, EyeOff } from "lucide-react";
import JsonEditor from "@/components/JsonEditor";

interface CommonConfigEditorProps {
  value: string;
  onChange: (value: string) => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  commonConfigSnippet: string;
  onCommonConfigSnippetChange: (value: string) => void;
  commonConfigError: string;
  onEditClick: () => void;
  isModalOpen: boolean;
  onModalClose: () => void;
  onExtract?: () => void;
  isExtracting?: boolean;
  /** 最终合并后的配置（只读预览） */
  finalConfig?: string;
}

export function CommonConfigEditor({
  value,
  onChange,
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigSnippet,
  onCommonConfigSnippetChange,
  commonConfigError,
  onEditClick,
  isModalOpen,
  onModalClose,
  onExtract,
  isExtracting,
  finalConfig,
}: CommonConfigEditorProps) {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);
  const [showPreview, setShowPreview] = useState(false);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));

    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  // 当启用通用配置时，自动显示预览
  useEffect(() => {
    if (useCommonConfig && finalConfig) {
      setShowPreview(true);
    }
  }, [useCommonConfig, finalConfig]);

  return (
    <>
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
          <div className="flex items-center gap-2">
            <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
              <input
                type="checkbox"
                id="useCommonConfig"
                checked={useCommonConfig}
                onChange={(e) => onCommonConfigToggle(e.target.checked)}
                className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
              />
              <span>
                {t("claudeConfig.writeCommonConfig", {
                  defaultValue: "写入通用配置",
                })}
              </span>
            </label>
          </div>
        </div>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            {useCommonConfig && finalConfig && (
              <button
                type="button"
                onClick={() => setShowPreview(!showPreview)}
                className="inline-flex items-center gap-1 text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
              >
                {showPreview ? (
                  <>
                    <EyeOff className="w-3 h-3" />
                    {t("claudeConfig.hidePreview", {
                      defaultValue: "隐藏合并预览",
                    })}
                  </>
                ) : (
                  <>
                    <Eye className="w-3 h-3" />
                    {t("claudeConfig.showPreview", {
                      defaultValue: "显示合并预览",
                    })}
                  </>
                )}
              </button>
            )}
          </div>
          <button
            type="button"
            onClick={onEditClick}
            className="text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
          >
            {t("claudeConfig.editCommonConfig", {
              defaultValue: "编辑通用配置",
            })}
          </button>
        </div>
        {commonConfigError && !isModalOpen && (
          <p className="text-xs text-red-500 dark:text-red-400 text-right">
            {commonConfigError}
          </p>
        )}

        {/* 自定义配置编辑器 */}
        <div className="space-y-1">
          {useCommonConfig && showPreview && (
            <Label className="text-xs text-muted-foreground">
              {t("claudeConfig.customConfig", {
                defaultValue: "自定义配置（覆盖通用配置）",
              })}
            </Label>
          )}
          <JsonEditor
            value={value}
            onChange={onChange}
            placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com",
    "ANTHROPIC_AUTH_TOKEN": "your-api-key-here"
  }
}`}
            darkMode={isDarkMode}
            rows={useCommonConfig && showPreview ? 3 : 14}
            autoHeight={useCommonConfig && showPreview}
            showValidation={true}
            language="json"
          />
        </div>

        {/* 合并预览（只读）- 放在自定义配置下面 */}
        {useCommonConfig && showPreview && finalConfig && (
          <div className="space-y-1">
            <div className="flex items-center justify-between">
              <Label className="text-xs text-muted-foreground">
                {t("claudeConfig.mergedPreview", {
                  defaultValue: "合并预览（只读）",
                })}
              </Label>
              <span className="text-xs text-green-500 dark:text-green-400">
                {t("claudeConfig.mergedPreviewHint", {
                  defaultValue: "通用配置 + 自定义配置 = 最终配置",
                })}
              </span>
            </div>
            <div className="relative">
              <JsonEditor
                value={finalConfig}
                onChange={() => {}} // 只读
                darkMode={isDarkMode}
                rows={8}
                showValidation={false}
                language="json"
                readOnly={true}
              />
              <div className="absolute top-2 right-2 px-2 py-0.5 bg-green-500/10 text-green-600 dark:text-green-400 text-xs rounded">
                {t("common.readonly", { defaultValue: "只读" })}
              </div>
            </div>
          </div>
        )}
      </div>

      <FullScreenPanel
        isOpen={isModalOpen}
        title={t("claudeConfig.editCommonConfigTitle", {
          defaultValue: "编辑通用配置片段",
        })}
        onClose={onModalClose}
        footer={
          <>
            {onExtract && (
              <Button
                type="button"
                variant="outline"
                onClick={onExtract}
                disabled={isExtracting}
                className="gap-2"
              >
                {isExtracting ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Download className="w-4 h-4" />
                )}
                {t("claudeConfig.extractFromCurrent", {
                  defaultValue: "从编辑内容提取",
                })}
              </Button>
            )}
            <Button type="button" variant="outline" onClick={onModalClose}>
              {t("common.cancel")}
            </Button>
            <Button type="button" onClick={onModalClose} className="gap-2">
              <Save className="w-4 h-4" />
              {t("common.save")}
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <p className="text-sm text-muted-foreground">
            {t("claudeConfig.commonConfigHint", {
              defaultValue: "通用配置片段将合并到所有启用它的供应商配置中",
            })}
          </p>
          <JsonEditor
            value={commonConfigSnippet}
            onChange={onCommonConfigSnippetChange}
            placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com"
  }
}`}
            darkMode={isDarkMode}
            rows={16}
            showValidation={true}
            language="json"
          />
          {commonConfigError && (
            <p className="text-sm text-red-500 dark:text-red-400">
              {commonConfigError}
            </p>
          )}
        </div>
      </FullScreenPanel>
    </>
  );
}
