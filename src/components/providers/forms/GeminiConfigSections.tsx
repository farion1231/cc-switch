import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff } from "lucide-react";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { useDarkMode } from "@/hooks/useDarkMode";

interface GeminiEnvSectionProps {
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  error?: string;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  onEditCommonConfig: () => void;
  commonConfigError?: string;
  /** 最终合并后的 env 配置（只读预览） */
  finalEnv?: string;
}

/**
 * GeminiEnvSection - .env editor section for Gemini environment variables
 */
export const GeminiEnvSection: React.FC<GeminiEnvSectionProps> = ({
  value,
  onChange,
  onBlur,
  error,
  useCommonConfig,
  onCommonConfigToggle,
  onEditCommonConfig,
  commonConfigError,
  finalEnv,
}) => {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const [showPreview, setShowPreview] = useState(false);

  // 当启用通用配置时，自动显示预览
  useEffect(() => {
    if (useCommonConfig && finalEnv) {
      setShowPreview(true);
    }
  }, [useCommonConfig, finalEnv]);

  const handleChange = (newValue: string) => {
    onChange(newValue);
    if (onBlur) {
      onBlur();
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label
          htmlFor="geminiEnv"
          className="block text-sm font-medium text-foreground"
        >
          {t("geminiConfig.envFile", { defaultValue: "环境变量 (.env)" })}
        </label>

        <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
          <input
            type="checkbox"
            checked={useCommonConfig}
            onChange={(e) => onCommonConfigToggle(e.target.checked)}
            className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
          />
          {t("geminiConfig.writeCommonConfig", {
            defaultValue: "写入通用配置",
          })}
        </label>
      </div>

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          {useCommonConfig && finalEnv && (
            <button
              type="button"
              onClick={() => setShowPreview(!showPreview)}
              className="inline-flex items-center gap-1 text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
            >
              {showPreview ? (
                <>
                  <EyeOff className="w-3 h-3" />
                  {t("geminiConfig.hidePreview", {
                    defaultValue: "隐藏合并预览",
                  })}
                </>
              ) : (
                <>
                  <Eye className="w-3 h-3" />
                  {t("geminiConfig.showPreview", {
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
          {t("geminiConfig.editCommonConfig", {
            defaultValue: "编辑通用配置",
          })}
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
            {t("geminiConfig.customConfig", {
              defaultValue: "自定义配置（覆盖通用配置）",
            })}
          </Label>
        )}
        <JsonEditor
          value={value}
          onChange={handleChange}
          placeholder={`GOOGLE_GEMINI_BASE_URL=https://your-api-endpoint.com/
GEMINI_API_KEY=sk-your-api-key-here
GEMINI_MODEL=gemini-3-pro-preview`}
          darkMode={isDarkMode}
          rows={useCommonConfig && showPreview ? 3 : 6}
          autoHeight={useCommonConfig && showPreview}
          showValidation={false}
          language="javascript"
        />
      </div>

      {/* 合并预览（只读）- 放在自定义配置下面 */}
      {useCommonConfig && showPreview && finalEnv && (
        <div className="space-y-1">
          <div className="flex items-center justify-between">
            <Label className="text-xs text-muted-foreground">
              {t("geminiConfig.mergedPreview", {
                defaultValue: "合并预览（只读）",
              })}
            </Label>
            <span className="text-xs text-green-500 dark:text-green-400">
              {t("geminiConfig.mergedPreviewHint", {
                defaultValue: "通用配置 + 自定义配置 = 最终配置",
              })}
            </span>
          </div>
          <div className="relative">
            <JsonEditor
              value={finalEnv}
              onChange={() => {}} // 只读
              darkMode={isDarkMode}
              rows={4}
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

      {error && (
        <p className="text-xs text-red-500 dark:text-red-400">{error}</p>
      )}

      {!error && (
        <p className="text-xs text-muted-foreground">
          {t("geminiConfig.envFileHint", {
            defaultValue: "使用 .env 格式配置 Gemini 环境变量",
          })}
        </p>
      )}
    </div>
  );
};

interface GeminiConfigSectionProps {
  value: string;
  onChange: (value: string) => void;
  configError?: string;
}

/**
 * GeminiConfigSection - Config JSON editor section with common config support
 */
export const GeminiConfigSection: React.FC<GeminiConfigSectionProps> = ({
  value,
  onChange,
  configError,
}) => {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();

  return (
    <div className="space-y-2">
      <label
        htmlFor="geminiConfig"
        className="block text-sm font-medium text-foreground"
      >
        {t("geminiConfig.configJson", {
          defaultValue: "配置文件 (config.json)",
        })}
      </label>

      <JsonEditor
        value={value}
        onChange={onChange}
        placeholder={`{
  "timeout": 30000,
  "maxRetries": 3
}`}
        darkMode={isDarkMode}
        rows={8}
        showValidation={true}
        language="json"
      />

      {configError && (
        <p className="text-xs text-red-500 dark:text-red-400">{configError}</p>
      )}

      {!configError && (
        <p className="text-xs text-muted-foreground">
          {t("geminiConfig.configJsonHint", {
            defaultValue: "使用 JSON 格式配置 Gemini 扩展参数（可选）",
          })}
        </p>
      )}
    </div>
  );
};
