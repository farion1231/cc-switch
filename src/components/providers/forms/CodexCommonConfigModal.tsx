import React, { useEffect, useState } from "react";
import { Save, Download, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import JsonEditor from "@/components/JsonEditor";

interface CodexCommonConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  value: string;
  onChange: (value: string) => void;
  error?: string;
  onExtract?: () => void;
  isExtracting?: boolean;
}

/**
 * CodexCommonConfigModal - Common Codex configuration editor modal
 * Allows editing of common TOML configuration shared across providers
 */
export const CodexCommonConfigModal: React.FC<CodexCommonConfigModalProps> = ({
  isOpen,
  onClose,
  value,
  onChange,
  error,
  onExtract,
  isExtracting,
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
    <FullScreenPanel
      isOpen={isOpen}
      title={t("codexConfig.editCommonConfigTitle")}
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
              {t("codexConfig.extractFromCurrent", {
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
          {t("codexConfig.commonConfigHint")}
        </p>

        <JsonEditor
          value={value}
          onChange={onChange}
          placeholder={`# Common Codex config

# Add your common TOML configuration here`}
          darkMode={isDarkMode}
          rows={16}
          showValidation={false}
          language="javascript"
        />

        {error && (
          <p className="text-sm text-red-500 dark:text-red-400">{error}</p>
        )}
      </div>
    </FullScreenPanel>
  );
};
