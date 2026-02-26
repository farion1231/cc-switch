import { useTranslation } from "react-i18next";
import { useEffect, useState } from "react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Save, FolderInput, Loader2 } from "lucide-react";
import JsonEditor from "@/components/JsonEditor";
import {
  OmoGlobalConfigFields,
  type OmoGlobalConfigFieldsRef,
} from "./OmoGlobalConfigFields";
import type { OmoGlobalConfig } from "@/types/omo";

interface OmoCommonConfigEditorProps {
  previewValue: string;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  isModalOpen: boolean;
  onEditClick: () => void;
  onModalClose: () => void;
  onSave: () => Promise<void>;
  isSaving: boolean;
  onGlobalConfigStateChange: (config: OmoGlobalConfig) => void;
  globalConfigRef: React.RefObject<OmoGlobalConfigFieldsRef | null>;
  fieldsKey: number;
  isSlim?: boolean;
}

export function OmoCommonConfigEditor({
  previewValue,
  useCommonConfig,
  onCommonConfigToggle,
  isModalOpen,
  onEditClick,
  onModalClose,
  onSave,
  isSaving,
  onGlobalConfigStateChange,
  globalConfigRef,
  fieldsKey,
  isSlim = false,
}: OmoCommonConfigEditorProps) {
  const { t } = useTranslation();
  const [isDarkMode, setIsDarkMode] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  useEffect(() => {
    const syncDarkMode = () =>
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    syncDarkMode();
    const observer = new MutationObserver(syncDarkMode);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);
  const handleImportLocal = async () => {
    if (!globalConfigRef.current) return;
    setIsImporting(true);
    try {
      await globalConfigRef.current.importFromLocal();
    } finally {
      setIsImporting(false);
    }
  };
  return (
    <>
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label>{t("provider.configJson")}</Label>
          <div className="flex items-center gap-2">
            <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
              <input
                type="checkbox"
                checked={useCommonConfig}
                onChange={(e) => onCommonConfigToggle(e.target.checked)}
                className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
              />
              <span>
                {t("omo.writeCommonConfig", {
                  defaultValue: "Write to common config",
                })}
              </span>
            </label>
          </div>
        </div>
        <div className="flex items-center justify-end">
          <button
            type="button"
            onClick={onEditClick}
            className="text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
          >
            {t("omo.editCommonConfig", { defaultValue: "Edit common config" })}
          </button>
        </div>
        <JsonEditor
          value={previewValue}
          onChange={() => {}}
          darkMode={isDarkMode}
          rows={14}
          showValidation={false}
          language="json"
        />
      </div>
      <FullScreenPanel
        isOpen={isModalOpen}
        title={t("omo.editCommonConfigTitle", {
          defaultValue: "Edit OMO Common Config",
        })}
        onClose={onModalClose}
        footer={
          <>
            <Button
              type="button"
              variant="outline"
              onClick={handleImportLocal}
              disabled={isImporting}
              className="gap-2"
            >
              {isImporting ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <FolderInput className="w-4 h-4" />
              )}
              {t("common.import", { defaultValue: "Import" })}
            </Button>
            <Button type="button" variant="outline" onClick={onModalClose}>
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              onClick={onSave}
              disabled={isSaving}
              className="gap-2"
            >
              {isSaving ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Save className="w-4 h-4" />
              )}
              {t("common.save")}
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <p className="text-sm text-muted-foreground">
            {t("omo.commonConfigHint", {
              defaultValue:
                "OMO common config will be merged into all OMO configs that enable it",
            })}
          </p>
          <OmoGlobalConfigFields
            key={fieldsKey}
            ref={globalConfigRef as React.Ref<OmoGlobalConfigFieldsRef>}
            onStateChange={onGlobalConfigStateChange}
            hideSaveButtons
            isSlim={isSlim}
          />
        </div>
      </FullScreenPanel>
    </>
  );
}
