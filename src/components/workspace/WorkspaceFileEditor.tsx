import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import MarkdownEditor from "@/components/MarkdownEditor";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { workspaceApi } from "@/lib/api/workspace";

interface WorkspaceFileEditorProps {
  filename: string;
  isOpen: boolean;
  onClose: () => void;
}

const WorkspaceFileEditor: React.FC<WorkspaceFileEditorProps> = ({
  filename,
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
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

  useEffect(() => {
    if (!isOpen || !filename) return;

    setLoading(true);
    workspaceApi
      .readFile(filename)
      .then((data) => {
        setContent(data ?? "");
      })
      .catch((err) => {
        console.error("Failed to read workspace file:", err);
        toast.error(t("workspace.loadFailed"));
      })
      .finally(() => setLoading(false));
  }, [isOpen, filename, t]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await workspaceApi.writeFile(filename, content);
      toast.success(t("workspace.saveSuccess"));
    } catch (err) {
      console.error("Failed to save workspace file:", err);
      toast.error(t("workspace.saveFailed"));
    } finally {
      setSaving(false);
    }
  }, [filename, content, t]);

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={t("workspace.editing", { filename })}
      onClose={onClose}
      footer={
        <Button onClick={handleSave} disabled={saving || loading}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      }
    >
      {loading ? (
        <div className="flex items-center justify-center h-64 text-muted-foreground">
          {t("prompts.loading")}
        </div>
      ) : (
        <MarkdownEditor
          value={content}
          onChange={setContent}
          darkMode={isDarkMode}
          placeholder={`# ${filename}\n\n...`}
          minHeight="calc(100vh - 240px)"
        />
      )}
    </FullScreenPanel>
  );
};

export default WorkspaceFileEditor;
