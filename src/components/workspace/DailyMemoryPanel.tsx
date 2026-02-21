import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Calendar, Trash2, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import MarkdownEditor from "@/components/MarkdownEditor";
import { workspaceApi, type DailyMemoryFileInfo } from "@/lib/api/workspace";

interface DailyMemoryPanelProps {
  isOpen: boolean;
  onClose: () => void;
}

function getTodayFilename(): string {
  const now = new Date();
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}.md`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const DailyMemoryPanel: React.FC<DailyMemoryPanelProps> = ({
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();

  // List state
  const [files, setFiles] = useState<DailyMemoryFileInfo[]>([]);
  const [loadingList, setLoadingList] = useState(false);

  // Edit state
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [loadingContent, setLoadingContent] = useState(false);
  const [saving, setSaving] = useState(false);

  // Delete state
  const [deletingFile, setDeletingFile] = useState<string | null>(null);

  // Dark mode
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

  // Load file list
  const loadFiles = useCallback(async () => {
    setLoadingList(true);
    try {
      const list = await workspaceApi.listDailyMemoryFiles();
      setFiles(list);
    } catch (err) {
      console.error("Failed to load daily memory files:", err);
      toast.error(t("workspace.dailyMemory.loadFailed"));
    } finally {
      setLoadingList(false);
    }
  }, [t]);

  useEffect(() => {
    if (isOpen) {
      void loadFiles();
    }
  }, [isOpen, loadFiles]);

  // Open file for editing
  const openFile = useCallback(
    async (filename: string) => {
      setLoadingContent(true);
      setEditingFile(filename);
      try {
        const data = await workspaceApi.readDailyMemoryFile(filename);
        setContent(data ?? "");
      } catch (err) {
        console.error("Failed to read daily memory file:", err);
        toast.error(t("workspace.dailyMemory.loadFailed"));
        setEditingFile(null);
      } finally {
        setLoadingContent(false);
      }
    },
    [t],
  );

  // Create today's note (deferred — file is only persisted on save)
  const handleCreateToday = useCallback(async () => {
    const filename = getTodayFilename();
    // Check if already exists in the list
    const existing = files.find((f) => f.filename === filename);
    if (existing) {
      // Just open it
      await openFile(filename);
      return;
    }
    // Open editor with empty content — no file created until user saves
    setEditingFile(filename);
    setContent("");
  }, [files, openFile]);

  // Save current file
  const handleSave = useCallback(async () => {
    if (!editingFile) return;
    setSaving(true);
    try {
      await workspaceApi.writeDailyMemoryFile(editingFile, content);
      toast.success(t("workspace.saveSuccess"));
    } catch (err) {
      console.error("Failed to save daily memory file:", err);
      toast.error(t("workspace.saveFailed"));
    } finally {
      setSaving(false);
    }
  }, [editingFile, content, t]);

  // Delete file
  const handleDelete = useCallback(async () => {
    if (!deletingFile) return;
    try {
      await workspaceApi.deleteDailyMemoryFile(deletingFile);
      toast.success(t("workspace.dailyMemory.deleteSuccess"));
      setDeletingFile(null);
      // If we were editing this file, go back to list
      if (editingFile === deletingFile) {
        setEditingFile(null);
      }
      await loadFiles();
    } catch (err) {
      console.error("Failed to delete daily memory file:", err);
      toast.error(t("workspace.dailyMemory.deleteFailed"));
      setDeletingFile(null);
    }
  }, [deletingFile, editingFile, loadFiles, t]);

  // Back from edit mode to list mode
  const handleBackToList = useCallback(() => {
    setEditingFile(null);
    setContent("");
    void loadFiles();
  }, [loadFiles]);

  // Close panel entirely
  const handleClose = useCallback(() => {
    setEditingFile(null);
    setContent("");
    onClose();
  }, [onClose]);

  // --- Edit mode ---
  if (editingFile) {
    return (
      <>
        <FullScreenPanel
          isOpen={isOpen}
          title={t("workspace.editing", { filename: editingFile })}
          onClose={handleBackToList}
          footer={
            <Button onClick={handleSave} disabled={saving || loadingContent}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          }
        >
          {loadingContent ? (
            <div className="flex items-center justify-center h-64 text-muted-foreground">
              {t("prompts.loading")}
            </div>
          ) : (
            <MarkdownEditor
              value={content}
              onChange={setContent}
              darkMode={isDarkMode}
              placeholder={`# ${editingFile}\n\n...`}
              minHeight="calc(100vh - 240px)"
            />
          )}
        </FullScreenPanel>

        <ConfirmDialog
          isOpen={!!deletingFile}
          title={t("workspace.dailyMemory.confirmDeleteTitle")}
          message={t("workspace.dailyMemory.confirmDeleteMessage", {
            date: deletingFile?.replace(".md", "") ?? "",
          })}
          onConfirm={handleDelete}
          onCancel={() => setDeletingFile(null)}
        />
      </>
    );
  }

  // --- List mode ---
  return (
    <>
      <FullScreenPanel
        isOpen={isOpen}
        title={t("workspace.dailyMemory.title")}
        onClose={handleClose}
      >
        <div className="space-y-4">
          {/* Header with path and create button */}
          <div className="flex items-center justify-between">
            <p className="text-sm text-muted-foreground">
              ~/.openclaw/workspace/memory/
            </p>
            <Button
              variant="outline"
              size="sm"
              onClick={handleCreateToday}
              className="gap-1.5"
            >
              <Plus className="w-3.5 h-3.5" />
              {t("workspace.dailyMemory.createToday")}
            </Button>
          </div>

          {/* File list */}
          {loadingList ? (
            <div className="flex items-center justify-center h-48 text-muted-foreground">
              {t("prompts.loading")}
            </div>
          ) : files.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 text-muted-foreground gap-3">
              <Calendar className="w-10 h-10 opacity-40" />
              <p className="text-sm">{t("workspace.dailyMemory.empty")}</p>
            </div>
          ) : (
            <div className="space-y-2">
              {files.map((file) => (
                <button
                  key={file.filename}
                  onClick={() => openFile(file.filename)}
                  className="w-full flex items-start gap-3 p-4 rounded-xl border border-border bg-card hover:bg-accent/50 transition-colors text-left group"
                >
                  <div className="mt-0.5 text-muted-foreground group-hover:text-foreground transition-colors">
                    <Calendar className="w-4 h-4" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-sm text-foreground">
                        {file.date}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {formatFileSize(file.sizeBytes)}
                      </span>
                    </div>
                    {file.preview && (
                      <p className="text-xs text-muted-foreground mt-1 line-clamp-2">
                        {file.preview}
                      </p>
                    )}
                  </div>
                  <div
                    className="opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0"
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeletingFile(file.filename);
                    }}
                  >
                    <Trash2 className="w-4 h-4 text-muted-foreground hover:text-destructive transition-colors" />
                  </div>
                </button>
              ))}
            </div>
          )}
        </div>
      </FullScreenPanel>

      <ConfirmDialog
        isOpen={!!deletingFile}
        title={t("workspace.dailyMemory.confirmDeleteTitle")}
        message={t("workspace.dailyMemory.confirmDeleteMessage", {
          date: deletingFile?.replace(".md", "") ?? "",
        })}
        onConfirm={handleDelete}
        onCancel={() => setDeletingFile(null)}
      />
    </>
  );
};

export default DailyMemoryPanel;
