import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";

interface GeminiEnvSectionProps {
  isAgy?: boolean;
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  error?: string;
}

/**
 * GeminiEnvSection - .env editor section for Gemini environment variables
 */
export const GeminiEnvSection: React.FC<GeminiEnvSectionProps> = ({
  isAgy = false,
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
        htmlFor="geminiEnv"
        className="block text-sm font-medium text-fg-1"
      >
        {isAgy
          ? t("provider.form.agy.envFile")
          : t("geminiConfig.envFile", { defaultValue: "环境变量 (.env)" })}
      </label>

      <JsonEditor
        value={value}
        onChange={handleChange}
        placeholder={`GOOGLE_GEMINI_BASE_URL=https://your-api-endpoint.com/
GEMINI_API_KEY=sk-your-api-key-here
GEMINI_MODEL=gemini-3.6-flash`}
        darkMode={isDarkMode}
        rows={3}
        showValidation={false}
        language="javascript"
      />

      {error && <p className="text-xs text-danger-text">{error}</p>}

      {!error && (
        <p className="text-xs text-fg-2">
          {isAgy
            ? t("provider.form.agy.envFileHint")
            : t("geminiConfig.envFileHint", {
                defaultValue: "使用 .env 格式配置 Gemini 环境变量",
              })}
        </p>
      )}
    </div>
  );
};

interface GeminiConfigSectionProps {
  isAgy?: boolean;
  value: string;
  onChange: (value: string) => void;
  configError?: string;
}

/**
 * GeminiConfigSection - settings.json editor section
 */
export const GeminiConfigSection: React.FC<GeminiConfigSectionProps> = ({
  isAgy = false,
  value,
  onChange,
  configError,
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

  return (
    <div className="space-y-2">
      <label
        htmlFor="geminiConfig"
        className="block text-sm font-medium text-fg-1"
      >
        {isAgy
          ? t("provider.form.agy.configJson")
          : t("geminiConfig.configJson", {
              defaultValue: "配置文件 (settings.json)",
            })}
      </label>

      <JsonEditor
        value={value}
        onChange={onChange}
        placeholder={
          isAgy
            ? '{"model":{"name":"gemini-3.8-flash"}}'
            : `{
  "timeout": 30000,
  "maxRetries": 3
}`
        }
        darkMode={isDarkMode}
        rows={3}
        showValidation={true}
        language="json"
      />

      {configError && <p className="text-xs text-danger-text">{configError}</p>}

      {!configError && (
        <p className="text-xs text-fg-2">
          {isAgy
            ? t("provider.form.agy.configJsonHint")
            : t("geminiConfig.configJsonHint", {
                defaultValue:
                  "Gemini CLI 的 settings.json（MCP 服务器在 MCP 面板里管理）",
              })}
        </p>
      )}
    </div>
  );
};
