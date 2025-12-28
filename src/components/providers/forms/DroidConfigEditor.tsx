import { useTranslation } from "react-i18next";
import { useEffect, useState, useMemo } from "react";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";

interface DroidConfigEditorProps {
  // 输入框的值 (camelCase 格式)
  apiKey: string;
  baseUrl: string;
  model: string;
  provider: string;
  providerName: string; // 供应商名称，用于 model_display_name
  maxOutputTokens?: number;
  // 当配置 JSON 变化时的回调
  onConfigChange?: (config: {
    apiKey: string;
    baseUrl: string;
    model: string;
    provider: string;
  }) => void;
}

/**
 * Droid 配置编辑器
 * 
 * 显示 Droid config.json 中 custom_model 的格式 (snake_case)
 * 与输入框双向同步
 */
export function DroidConfigEditor({
  apiKey,
  baseUrl,
  model,
  provider,
  providerName,
  maxOutputTokens = 131072,
  onConfigChange,
}: DroidConfigEditorProps) {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);
  const [jsonError, setJsonError] = useState("");

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

  // 将输入框的值转换为 Droid config.json 格式 (snake_case)
  const configJsonValue = useMemo(() => {
    const customModel = {
      api_key: apiKey,
      base_url: baseUrl,
      model: model,
      model_display_name: providerName,
      provider: provider,
      max_tokens: maxOutputTokens,
    };
    return JSON.stringify(customModel, null, 2);
  }, [apiKey, baseUrl, model, provider, providerName, maxOutputTokens]);

  // 当用户编辑 JSON 时，解析并同步到输入框
  const handleJsonChange = (value: string) => {
    try {
      const parsed = JSON.parse(value);
      setJsonError("");
      
      // 从 snake_case 转换为 camelCase 并回调
      if (onConfigChange) {
        onConfigChange({
          apiKey: parsed.api_key ?? "",
          baseUrl: parsed.base_url ?? "",
          model: parsed.model ?? "",
          provider: parsed.provider ?? "anthropic",
        });
      }
    } catch (e) {
      setJsonError(t("provider.invalidJson", { defaultValue: "JSON 格式错误" }));
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label>{t("provider.configJson", { defaultValue: "配置 JSON" })}</Label>
        <span className="text-xs text-muted-foreground">
          {t("droid.configFormat", { defaultValue: "Droid config.json 格式" })}
        </span>
      </div>
      {jsonError && (
        <p className="text-xs text-red-500 dark:text-red-400">{jsonError}</p>
      )}
      <JsonEditor
        value={configJsonValue}
        onChange={handleJsonChange}
        placeholder={`{
  "api_key": "your-api-key",
  "base_url": "https://api.example.com",
  "model": "claude-sonnet-4-5-20250929",
  "model_display_name": "My Provider",
  "provider": "anthropic",
  "max_tokens": 131072
}`}
        darkMode={isDarkMode}
        rows={10}
        showValidation={true}
        language="json"
      />
    </div>
  );
}
