import React, { useEffect, useState } from "react";
import { Save, Download, Loader2, Package } from "lucide-react";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import JsonEditor from "@/components/JsonEditor";

interface CodexCommonConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  value: string;
  onSave: (value: string) => boolean;
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
  onSave,
  error,
  onExtract,
  isExtracting,
}) => {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);
  const [draftValue, setDraftValue] = useState(value);

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

  useEffect(() => {
    if (isOpen) {
      setDraftValue(value);
    }
  }, [isOpen, value]);

  const handleClose = () => {
    setDraftValue(value);
    onClose();
  };

  const handleSave = () => {
    if (onSave(draftValue)) {
      onClose();
    }
  };

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("codexConfig.editCommonConfigTitle")}
      onClose={handleClose}
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
          <Button type="button" variant="outline" onClick={handleClose}>
            {t("common.cancel")}
          </Button>
          <Button type="button" onClick={handleSave} className="gap-2">
            <Save className="w-4 h-4" />
            {t("common.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="rounded-lg border border-blue-200 dark:border-blue-800 bg-blue-50/50 dark:bg-blue-950/30 p-3 space-y-1.5">
          <p className="text-sm font-medium text-blue-800 dark:text-blue-300">
            {t("commonConfig.guideTitle")}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("commonConfig.guidePurpose")}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("commonConfig.guideUsage")}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("commonConfig.guideReExtract")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("commonConfig.guideReassurance")}
          </p>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("codexConfig.commonConfigHint")}
        </p>
        {(!draftValue || draftValue.trim() === "") && (
          <div className="flex flex-col items-center justify-center py-6 text-center text-muted-foreground">
            <Package className="h-8 w-8 mb-2 opacity-40" />
            <p className="text-sm font-medium">
              {t("commonConfig.emptyTitle")}
            </p>
            <p className="text-xs mt-1">{t("commonConfig.emptyHint")}</p>
          </div>
        )}

        <JsonEditor
          value={draftValue}
          onChange={setDraftValue}
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
