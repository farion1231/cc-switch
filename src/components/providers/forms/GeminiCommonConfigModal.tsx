import React, { useEffect, useState } from "react";
import { Save } from "lucide-react";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import JsonEditor from "@/components/JsonEditor";

interface GeminiCommonConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  value: string;
  onChange: (value: string) => void;
  error?: string;
}

/**
 * GeminiCommonConfigModal - Common Gemini configuration editor modal
 * Allows editing of common JSON configuration shared across Gemini providers
 */
export const GeminiCommonConfigModal: React.FC<
  GeminiCommonConfigModalProps
> = ({ isOpen, onClose, value, onChange, error }) => {
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
    <FullScreenPanel
      isOpen={isOpen}
      title={t("geminiConfig.editCommonConfigTitle", {
        defaultValue: "编辑 Gemini 通用配置片段",
      })}
      onClose={onClose}
      footer={
        <>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="button" onClick={onClose} className="gap-2">
            <Save className="w-4 h-4" />
            {t("common.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <p className="text-sm text-muted-foreground">
          {t("geminiConfig.commonConfigHint", {
            defaultValue:
              "通用配置片段将合并到所有启用它的 Gemini 供应商配置中",
          })}
        </p>

        <JsonEditor
          value={value}
          onChange={onChange}
          placeholder={`{
  "timeout": 30000,
  "maxRetries": 3,
  "customField": "value"
}`}
          darkMode={isDarkMode}
          rows={16}
          showValidation={true}
          language="json"
        />

        {error && (
          <p className="text-sm text-red-500 dark:text-red-400">{error}</p>
        )}
      </div>
    </FullScreenPanel>
  );
};
