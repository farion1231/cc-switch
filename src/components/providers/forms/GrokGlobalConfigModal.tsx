import { useEffect, useState } from "react";
import { Loader2, Save, ShieldCheck } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import JsonEditor from "@/components/JsonEditor";
import { configApi } from "@/lib/api";

interface GrokGlobalConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function GrokGlobalConfigModal({
  isOpen,
  onClose,
}: GrokGlobalConfigModalProps) {
  const { t } = useTranslation();
  const [content, setContent] = useState("");
  const [path, setPath] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isDarkMode, setIsDarkMode] = useState(false);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));
    const observer = new MutationObserver(() =>
      setIsDarkMode(document.documentElement.classList.contains("dark")),
    );
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    setIsLoading(true);
    configApi
      .readGrokGlobalConfig()
      .then((result) => {
        setContent(result.content);
        setPath(result.path);
      })
      .catch((error) => toast.error(String(error)))
      .finally(() => setIsLoading(false));
  }, [isOpen]);

  const handleSave = async () => {
    setIsSaving(true);
    try {
      await configApi.writeGrokGlobalConfig(content);
      toast.success(t("grokConfig.globalConfigSaved"));
      onClose();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsSaving(false);
    }
  };

  const handlePrivacy = async () => {
    setIsSaving(true);
    try {
      const next = await configApi.applyGrokPrivacyProtection();
      setContent(next);
      toast.success(t("grokConfig.privacyApplied"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("grokConfig.editGlobalConfig")}
      onClose={onClose}
      footer={
        <>
          <Button
            type="button"
            variant="outline"
            onClick={handlePrivacy}
            disabled={isLoading || isSaving}
            className="gap-2"
          >
            <ShieldCheck className="h-4 w-4" />
            {t("grokConfig.applyPrivacy")}
          </Button>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            onClick={handleSave}
            disabled={isLoading || isSaving}
            className="gap-2"
          >
            {isSaving ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Save className="h-4 w-4" />
            )}
            {t("common.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="rounded-lg border border-blue-200 bg-blue-50/50 p-3 text-sm dark:border-blue-800 dark:bg-blue-950/30">
          <p className="font-medium text-blue-800 dark:text-blue-300">
            {t("grokConfig.globalConfigTitle")}
          </p>
          <p className="mt-1 text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("grokConfig.globalConfigHint")}
          </p>
          <p className="mt-1 break-all text-xs text-muted-foreground">{path}</p>
        </div>
        {isLoading ? (
          <div className="flex justify-center py-12">
            <Loader2 className="h-6 w-6 animate-spin" />
          </div>
        ) : (
          <JsonEditor
            value={content}
            onChange={setContent}
            placeholder="# ~/.grok/config.toml"
            darkMode={isDarkMode}
            rows={22}
            showValidation={false}
            language="javascript"
          />
        )}
      </div>
    </FullScreenPanel>
  );
}
