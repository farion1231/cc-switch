import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, FileText, Check } from "lucide-react";
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
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    titleKey: string;
    messageKey: string;
    messageParams?: Record<string, unknown>;
    onConfirm: () => void;
  } | null>(null);

  const {
    prompts,
    loading,
    reload,
    savePrompt,
    deletePrompt,
    toggleEnabled,
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
    const prompt = prompts[id];
    setConfirmDialog({
      isOpen: true,
      titleKey: "prompts.confirm.deleteTitle",
      messageKey: "prompts.confirm.deleteMessage",
      messageParams: { name: prompt?.name },
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


  const promptEntries = useMemo(
    () => Object.entries(prompts),
    [prompts]
  );

  const enabledPrompt = promptEntries.find(([_, p]) => p.enabled);

  const appName = t(`apps.${appId}`);
  const panelTitle = t("prompts.title", { appName });

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-3xl max-h-[85vh] min-h-[600px] flex flex-col">
          <DialogHeader>
            <div className="flex items-center justify-between pr-8">
              <DialogTitle>{panelTitle}</DialogTitle>
              <Button type="button" variant="mcp" onClick={handleAdd}>
                <Plus size={16} />
                {t("prompts.add")}
              </Button>
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
                    onToggle={toggleEnabled}
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
          title={t(confirmDialog.titleKey)}
          message={t(confirmDialog.messageKey, confirmDialog.messageParams)}
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}
    </>
  );
};

export default PromptPanel;
