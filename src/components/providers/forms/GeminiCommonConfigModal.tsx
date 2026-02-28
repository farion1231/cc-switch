import React, { useEffect, useState } from "react";
import { Save, Download, Loader2 } from "lucide-react";
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
  onExtract?: () => void;
  isExtracting?: boolean;
}

/**
 * GeminiCommonConfigModal - Common Gemini configuration editor modal
 * Allows editing of common env snippet shared across Gemini providers
 */
export const GeminiCommonConfigModal: React.FC<
  GeminiCommonConfigModalProps
> = ({ isOpen, onClose, value, onChange, error, onExtract, isExtracting }) => {
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
              {t("geminiConfig.extractFromCurrent", {
                defaultValue: "从编辑内容提取",
              })}
            </Button>
          )}
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
              "该片段会写入 Gemini 的 .env（不允许包含 GOOGLE_GEMINI_BASE_URL、GEMINI_API_KEY）",
          })}
        </p>

        <JsonEditor
          value={value}
          onChange={onChange}
          placeholder={`{
  "GEMINI_MODEL": "gemini-3-pro-preview"
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
