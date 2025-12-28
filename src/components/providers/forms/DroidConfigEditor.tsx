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
  providerName: string; // 供应商名称，用于 displayName
  maxOutputTokens?: number;
  noImageSupport?: boolean;
  // 当配置 JSON 变化时的回调
  onConfigChange?: (config: {
    apiKey: string;
    baseUrl: string;
    model: string;
    provider: string;
    maxOutputTokens?: number;
    noImageSupport?: boolean;
  }) => void;
}

/**
 * Droid 配置编辑器
 * 
 * 显示 Droid settings.json 中 customModels 的格式 (camelCase)
 * 直接写入 settings.json 实现热更新
 */
export function DroidConfigEditor({
  apiKey,
  baseUrl,
  model,
  provider,
  providerName,
  maxOutputTokens = 131072,
  noImageSupport = false,
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

  // 将输入框的值转换为 Droid settings.json 格式 (camelCase)
  const configJsonValue = useMemo(() => {
    const customModel = {
      model: model,
      baseUrl: baseUrl,
      apiKey: apiKey,
      displayName: providerName,
      maxOutputTokens: maxOutputTokens,
      noImageSupport: noImageSupport,
      provider: provider,
    };
    return JSON.stringify(customModel, null, 2);
  }, [apiKey, baseUrl, model, provider, providerName, maxOutputTokens, noImageSupport]);

  // 当用户编辑 JSON 时，解析并同步到输入框
  const handleJsonChange = (value: string) => {
    try {
      const parsed = JSON.parse(value);
      setJsonError("");
      
      if (onConfigChange) {
        onConfigChange({
          apiKey: parsed.apiKey ?? parsed.api_key ?? "",
          baseUrl: parsed.baseUrl ?? parsed.base_url ?? "",
          model: parsed.model ?? "",
          provider: parsed.provider ?? "anthropic",
          maxOutputTokens: parsed.maxOutputTokens ?? parsed.max_tokens ?? 131072,
          noImageSupport: parsed.noImageSupport ?? false,
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
          {t("droid.settingsFormat", { defaultValue: "Droid settings.json 格式" })}
        </span>
      </div>
      {jsonError && (
        <p className="text-xs text-red-500 dark:text-red-400">{jsonError}</p>
      )}
      <JsonEditor
        value={configJsonValue}
        onChange={handleJsonChange}
        placeholder={`{
  "model": "claude-sonnet-4-5-20250929",
  "baseUrl": "https://api.example.com",
  "apiKey": "your-api-key",
  "displayName": "My Provider",
  "maxOutputTokens": 131072,
  "noImageSupport": false,
  "provider": "anthropic"
}`}
        darkMode={isDarkMode}
        rows={10}
        showValidation={true}
        language="json"
      />
    </div>
  );
}
