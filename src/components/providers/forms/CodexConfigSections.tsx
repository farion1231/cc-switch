import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff } from "lucide-react";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";

interface CodexAuthSectionProps {
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  error?: string;
}

/**
 * CodexAuthSection - Auth JSON editor section
 */
export const CodexAuthSection: React.FC<CodexAuthSectionProps> = ({
  value,
  onChange,
  onBlur,
  error,
}) => {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);

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

  const handleChange = (newValue: string) => {
    onChange(newValue);
    if (onBlur) {
      onBlur();
    }
  };

  return (
    <div className="space-y-2">
      <label
        htmlFor="codexAuth"
        className="block text-sm font-medium text-foreground"
      >
        {t("codexConfig.authJson")}
      </label>

      <JsonEditor
        value={value}
        onChange={handleChange}
        placeholder={t("codexConfig.authJsonPlaceholder")}
        darkMode={isDarkMode}
        rows={6}
        showValidation={true}
        language="json"
      />

      {error && (
        <p className="text-xs text-red-500 dark:text-red-400">{error}</p>
      )}

      {!error && (
        <p className="text-xs text-muted-foreground">
          {t("codexConfig.authJsonHint")}
        </p>
      )}
    </div>
  );
};

interface CodexConfigSectionProps {
  value: string;
  onChange: (value: string) => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  onEditCommonConfig: () => void;
  commonConfigError?: string;
  configError?: string;
  /** 最终合并后的配置（只读预览） */
  finalConfig?: string;
}

/**
 * CodexConfigSection - Config TOML editor section
 */
export const CodexConfigSection: React.FC<CodexConfigSectionProps> = ({
  value,
  onChange,
  useCommonConfig,
  onCommonConfigToggle,
  onEditCommonConfig,
  commonConfigError,
  configError,
  finalConfig,
}) => {
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
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label
          htmlFor="codexConfig"
          className="block text-sm font-medium text-foreground"
        >
          {t("codexConfig.configToml")}
        </label>

        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
          <input
            type="checkbox"
            checked={useCommonConfig}
            onChange={(e) => onCommonConfigToggle(e.target.checked)}
            className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default  rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
          />
          {t("codexConfig.writeCommonConfig")}
        </label>
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
                  {t("codexConfig.hidePreview", {
                    defaultValue: "隐藏合并预览",
                  })}
                </>
              ) : (
                <>
                  <Eye className="w-3 h-3" />
                  {t("codexConfig.showPreview", {
                    defaultValue: "显示合并预览",
                  })}
                </>
              )}
            </button>
          )}
        </div>
        <button
          type="button"
          onClick={onEditCommonConfig}
          className="text-xs text-blue-500 dark:text-blue-400 hover:underline"
        >
          {t("codexConfig.editCommonConfig")}
        </button>
      </div>

      {commonConfigError && (
        <p className="text-xs text-red-500 dark:text-red-400 text-right">
          {commonConfigError}
        </p>
      )}

      {/* 自定义配置编辑器 */}
      <div className="space-y-1">
        {useCommonConfig && showPreview && (
          <Label className="text-xs text-muted-foreground">
            {t("codexConfig.customConfig", {
              defaultValue: "自定义配置（覆盖通用配置）",
            })}
          </Label>
        )}
        <JsonEditor
          value={value}
          onChange={onChange}
          placeholder=""
          darkMode={isDarkMode}
          rows={useCommonConfig && showPreview ? 6 : 8}
          showValidation={false}
          language="javascript"
        />
      </div>

      {/* 合并预览（只读）- 放在自定义配置下面 */}
      {useCommonConfig && showPreview && finalConfig && (
        <div className="space-y-1">
          <div className="flex items-center justify-between">
            <Label className="text-xs text-muted-foreground">
              {t("codexConfig.mergedPreview", {
                defaultValue: "合并预览（只读）",
              })}
            </Label>
            <span className="text-xs text-green-500 dark:text-green-400">
              {t("codexConfig.mergedPreviewHint", {
                defaultValue: "通用配置 + 自定义配置 = 最终配置",
              })}
            </span>
          </div>
          <div className="relative">
            <JsonEditor
              value={finalConfig}
              onChange={() => {}} // 只读
              darkMode={isDarkMode}
              rows={6}
              showValidation={false}
              language="javascript"
              readOnly={true}
            />
            <div className="absolute top-2 right-2 px-2 py-0.5 bg-green-500/10 text-green-600 dark:text-green-400 text-xs rounded">
              {t("common.readonly", { defaultValue: "只读" })}
            </div>
          </div>
        </div>
      )}

      {configError && (
        <p className="text-xs text-red-500 dark:text-red-400">{configError}</p>
      )}

      {!configError && (
        <p className="text-xs text-muted-foreground">
          {t("codexConfig.configTomlHint")}
        </p>
      )}
    </div>
  );
};
