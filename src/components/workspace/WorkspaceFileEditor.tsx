import React, { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
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
  const [loadedFile, setLoadedFile] = useState<string | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
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
    setLoadedFile(null);
    if (!isOpen || !filename) return;

    let cancelled = false;
    workspaceApi
      .readFile(filename)
      .then((data) => {
        if (cancelled) return;
        setContent(data ?? "");
        setLoadedFile(filename);
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("Failed to read workspace file:", err);
        toast.error(t("workspace.loadFailed"));
        onCloseRef.current();
      });
    return () => {
      cancelled = true;
    };
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
      // Keep the file list visible until the editor content is ready.
      isOpen={isOpen && loadedFile === filename}
      title={t("workspace.editing", { filename })}
      onClose={onClose}
      footer={
        <Button onClick={handleSave} disabled={saving}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      }
    >
      <MarkdownEditor
        value={content}
        onChange={setContent}
        darkMode={isDarkMode}
        placeholder={`# ${filename}\n\n...`}
        minHeight="calc(100vh - 240px)"
      />
    </FullScreenPanel>
  );
};

export default WorkspaceFileEditor;
