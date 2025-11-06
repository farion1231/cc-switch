import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, FileText, Check, Download, ChevronDown, ChevronUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { type AppId } from "@/lib/api";
import { usePromptActions } from "@/hooks/usePromptActions";
import PromptListItem from "./PromptListItem";
import PromptFormModal from "./PromptFormModal";
import MarkdownEditor from "@/components/MarkdownEditor";
import { ConfirmDialog } from "../ConfirmDialog";

interface PromptPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
}

const PromptPanel: React.FC<PromptPanelProps> = ({
  open,
  onOpenChange,
  appId,
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [showCurrentFile, setShowCurrentFile] = useState(false);
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);
  const [isDarkMode, setIsDarkMode] = useState(false);

  useEffect(() => {
    // 检测初始暗色模式状态
    setIsDarkMode(document.documentElement.classList.contains("dark"));

    // 监听 html 元素的 class 变化以实时响应主题切换
    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  const {
    prompts,
    loading,
    currentFileContent,
    reload,
    savePrompt,
    deletePrompt,
    enablePrompt,
    importFromFile,
  } = usePromptActions(appId);

  useEffect(() => {
    if (open) reload();
  }, [open, reload]);

  const handleAdd = () => {
    setEditingId(null);
    setIsFormOpen(true);
  };

  const handleEdit = (id: string) => {
    setEditingId(id);
    setIsFormOpen(true);
  };

  const handleDelete = (id: string) => {
    setConfirmDialog({
      isOpen: true,
      title: t("prompts.confirm.deleteTitle"),
      message: t("prompts.confirm.deleteMessage"),
      onConfirm: async () => {
        try {
          await deletePrompt(id);
          setConfirmDialog(null);
        } catch (e) {
          // Error handled by hook
        }
      },
    });
  };

  const handleImport = async () => {
    await importFromFile();
  };

  const promptEntries = useMemo(
    () => Object.entries(prompts),
    [prompts]
  );

  const enabledPrompt = promptEntries.find(([_, p]) => p.enabled);

  const panelTitle =
    appId === "claude" ? t("prompts.claudeTitle") : t("prompts.codexTitle");

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-3xl max-h-[85vh] min-h-[600px] flex flex-col">
          <DialogHeader>
            <div className="flex items-center justify-between pr-8">
              <DialogTitle>{panelTitle}</DialogTitle>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleImport}
                >
                  <Download size={16} />
                  {t("prompts.import")}
                </Button>
                <Button type="button" variant="mcp" onClick={handleAdd}>
                  <Plus size={16} />
                  {t("prompts.add")}
                </Button>
              </div>
            </div>
          </DialogHeader>

          <div className="flex-shrink-0 px-6 py-4">
            <div className="text-sm text-gray-500 dark:text-gray-400">
              {t("prompts.count", { count: promptEntries.length })} ·{" "}
              {enabledPrompt
                ? t("prompts.enabledName", { name: enabledPrompt[1].name })
                : t("prompts.noneEnabled")}
            </div>
          </div>

          {currentFileContent && (
            <div className="flex-shrink-0 px-6 pb-4">
              <div className="border rounded-lg overflow-hidden bg-gray-50 dark:bg-gray-900">
                <button
                  onClick={() => setShowCurrentFile(!showCurrentFile)}
                  className="w-full flex items-center justify-between px-4 py-3 text-sm font-medium text-gray-900 dark:text-gray-100 hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
                >
                  <span>{t("prompts.currentFile")}</span>
                  {showCurrentFile ? (
                    <ChevronUp size={16} className="text-gray-500" />
                  ) : (
                    <ChevronDown size={16} className="text-gray-500" />
                  )}
                </button>
                {showCurrentFile && (
                  <div className="border-t border-gray-200 dark:border-gray-800">
                    <MarkdownEditor
                      value={currentFileContent}
                      readOnly
                      darkMode={isDarkMode}
                      minHeight="150px"
                      maxHeight="300px"
                      className="border-0 rounded-none"
                    />
                  </div>
                )}
              </div>
            </div>
          )}

          <div className="flex-1 overflow-y-auto px-6 pb-4">
            {loading ? (
              <div className="text-center py-12 text-gray-500 dark:text-gray-400">
                {t("prompts.loading")}
              </div>
            ) : promptEntries.length === 0 ? (
              <div className="text-center py-12">
                <div className="w-16 h-16 mx-auto mb-4 bg-gray-100 dark:bg-gray-800 rounded-full flex items-center justify-center">
                  <FileText
                    size={24}
                    className="text-gray-400 dark:text-gray-500"
                  />
                </div>
                <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
                  {t("prompts.empty")}
                </h3>
                <p className="text-gray-500 dark:text-gray-400 text-sm">
                  {t("prompts.emptyDescription")}
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {promptEntries.map(([id, prompt]) => (
                  <PromptListItem
                    key={id}
                    id={id}
                    prompt={prompt}
                    onEnable={enablePrompt}
                    onEdit={handleEdit}
                    onDelete={handleDelete}
                  />
                ))}
              </div>
            )}
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="mcp"
              onClick={() => onOpenChange(false)}
            >
              <Check size={16} />
              {t("common.done")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {isFormOpen && (
        <PromptFormModal
          appId={appId}
          editingId={editingId || undefined}
          initialData={editingId ? prompts[editingId] : undefined}
          onSave={savePrompt}
          onClose={() => setIsFormOpen(false)}
        />
      )}

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}
    </>
  );
};

export default PromptPanel;
