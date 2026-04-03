import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";

interface GeminiEnvSectionProps {
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  error?: string;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  onEditCommonConfig: () => void;
  commonConfigError?: string;
}

interface GeminiConfigSectionProps {
  value: string;
  onChange: (value: string) => void;
  configError?: string;
}

function ToggleChip({
  checked,
  label,
  onCheckedChange,
}: {
  checked: boolean;
  label: string;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <label className="inline-flex cursor-pointer items-center gap-2 rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:border-border-hover hover:text-foreground">
      <Checkbox
        checked={checked}
        onCheckedChange={(nextChecked) => onCheckedChange(nextChecked === true)}
      />
      <span>{label}</span>
    </label>
  );
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
    <div className="space-y-4 rounded-[calc(var(--radius)+0.25rem)] border border-border/70 bg-card/45 p-4 shadow-sm">
      <div className="flex items-center justify-between gap-3">
        <div className="space-y-1">
          <label
            htmlFor="geminiEnv"
            className="block text-sm font-medium text-foreground"
          >
            {t("geminiConfig.envFile", { defaultValue: "环境变量 (.env)" })}
          </label>
          <p className="text-xs text-muted-foreground">
            {t("geminiConfig.envFileHint", {
              defaultValue: "使用 .env 格式配置 Gemini 环境变量",
            })}
          </p>
        </div>

        <ToggleChip
          checked={useCommonConfig}
          onCheckedChange={onCommonConfigToggle}
          label={t("geminiConfig.writeCommonConfig", {
            defaultValue: "写入通用配置",
          })}
        />
      </div>

      <div className="flex items-center justify-end">
        <Button
          type="button"
          onClick={onEditCommonConfig}
          variant="link"
          size="sm"
          className="h-auto px-0 py-0 text-xs"
        >
          {t("geminiConfig.editCommonConfig", {
            defaultValue: "编辑通用配置",
          })}
        </Button>
      </div>

      {commonConfigError && (
        <p className="text-right text-xs text-[hsl(var(--destructive))]">
          {commonConfigError}
        </p>
      )}

      <JsonEditor
        value={value}
        onChange={handleChange}
        placeholder={`GOOGLE_GEMINI_BASE_URL=https://your-api-endpoint.com/
GEMINI_API_KEY=sk-your-api-key-here
GEMINI_MODEL=gemini-3-pro-preview`}
        darkMode={isDarkMode}
        rows={6}
        showValidation={false}
        language="javascript"
      />

      {error && (
        <p className="text-xs text-[hsl(var(--destructive))]">{error}</p>
      )}
    </div>
  );
};

/**
 * GeminiConfigSection - Config JSON editor section with common config support
 */
export const GeminiConfigSection: React.FC<GeminiConfigSectionProps> = ({
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
    <div className="space-y-4 rounded-[calc(var(--radius)+0.25rem)] border border-border/70 bg-card/45 p-4 shadow-sm">
      <div className="space-y-1">
        <label
          htmlFor="geminiConfig"
          className="block text-sm font-medium text-foreground"
        >
          {t("geminiConfig.configJson", {
            defaultValue: "配置文件 (config.json)",
          })}
        </label>
        <p className="text-xs text-muted-foreground">
          {t("geminiConfig.configJsonHint", {
            defaultValue: "使用 JSON 格式配置 Gemini 扩展参数（可选）",
          })}
        </p>
      </div>

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
        <p className="text-xs text-[hsl(var(--destructive))]">{configError}</p>
      )}
    </div>
  );
};
