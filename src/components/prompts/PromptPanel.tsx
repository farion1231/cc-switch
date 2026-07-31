import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileText, Search } from "lucide-react";
import { type AppId } from "@/lib/api";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { ScrollArea } from "@/components/ui/scroll-area";
import PromptListItem from "./PromptListItem";
import PromptFormPanel from "./PromptFormPanel";
import { ConfirmDialog } from "../ConfirmDialog";

interface PromptPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
}

export interface PromptPanelHandle {
  openAdd: () => void;
}

const PromptPanel = React.forwardRef<PromptPanelHandle, PromptPanelProps>(
  ({ open, appId }, ref) => {
    const { t } = useTranslation();
    const [isFormOpen, setIsFormOpen] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [searchQuery, setSearchQuery] = useState("");
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

    useEffect(() => {
      setSearchQuery("");
    }, [appId]);

    // Listen for prompt import events from deep link
    useEffect(() => {
      const handlePromptImported = (event: Event) => {
        const customEvent = event as CustomEvent;
        // Reload if the import is for this app
        if (customEvent.detail?.app === appId) {
          reload();
        }
      };

      window.addEventListener("prompt-imported", handlePromptImported);
      return () => {
        window.removeEventListener("prompt-imported", handlePromptImported);
      };
    }, [appId, reload]);

    // 应用项目 Profile 会切换激活的 prompt（prompts 非 react-query，需主动 reload）
    useTauriEvent("profile-applied", reload);

    const handleAdd = () => {
      setEditingId(null);
      setIsFormOpen(true);
    };

    React.useImperativeHandle(ref, () => ({
      openAdd: handleAdd,
    }));

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

    const promptEntries = useMemo(() => Object.entries(prompts), [prompts]);
    const normalizedSearchQuery = searchQuery.trim().toLocaleLowerCase();
    const filteredPromptEntries = useMemo(() => {
      if (!normalizedSearchQuery) return promptEntries;

      return promptEntries.filter(([recordId, prompt]) =>
        [
          recordId,
          prompt.id,
          prompt.name,
          prompt.description,
          prompt.content,
        ].some((value) =>
          value?.toLocaleLowerCase().includes(normalizedSearchQuery),
        ),
      );
    }, [normalizedSearchQuery, promptEntries]);

    const enabledPrompt = promptEntries.find(([_, p]) => p.enabled);

    return (
      <div className="flex flex-col flex-1 min-h-0 px-6">
        <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6">
          <div className="text-sm text-muted-foreground">
            {t("prompts.count", { count: promptEntries.length })} ·{" "}
            {enabledPrompt
              ? t("prompts.enabledName", { name: enabledPrompt[1].name })
              : t("prompts.noneEnabled")}
          </div>
        </div>

        <ManagementListSearch
          value={searchQuery}
          onValueChange={setSearchQuery}
          placeholder={t("prompts.searchPlaceholder")}
          ariaLabel={t("prompts.searchAriaLabel")}
          clearLabel={t("common.clear")}
        />

        <ScrollArea className="-mr-3 flex-1 min-h-0" type="auto">
          <div className="pb-16 pr-3">
            {loading ? (
              <div className="text-center py-12 text-muted-foreground">
                {t("prompts.loading")}
              </div>
            ) : promptEntries.length === 0 ? (
              <div className="text-center py-12">
                <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
                  <FileText size={24} className="text-muted-foreground" />
                </div>
                <h3 className="text-lg font-medium text-foreground mb-2">
                  {t("prompts.empty")}
                </h3>
                <p className="text-muted-foreground text-sm">
                  {t("prompts.emptyDescription")}
                </p>
              </div>
            ) : filteredPromptEntries.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-12 text-center text-muted-foreground">
                <Search className="mb-4 h-10 w-10 opacity-40" />
                <p className="text-sm">{t("prompts.noSearchResults")}</p>
              </div>
            ) : (
              <div className="space-y-3">
                {filteredPromptEntries.map(([id, prompt]) => (
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
        </ScrollArea>

        {isFormOpen && (
          <PromptFormPanel
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
      </div>
    );
  },
);

PromptPanel.displayName = "PromptPanel";

export default PromptPanel;
