import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileText } from "lucide-react";
import { type AppId } from "@/lib/api";
import { usePromptActions } from "@/hooks/usePromptActions";
import { useListControls } from "@/hooks/useListControls";
import { useSearchShortcut } from "@/components/common/SearchOverlay";
import { useSettingsQuery } from "@/lib/query";
import PromptListItem from "./PromptListItem";
import PromptCardCompact from "./PromptCardCompact";
import PromptFormPanel from "./PromptFormPanel";
import { ConfirmDialog } from "../ConfirmDialog";
import { ListToolbar } from "@/components/common/ListToolbar";
import { SearchOverlay } from "@/components/common/SearchOverlay";

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
    const [confirmDialog, setConfirmDialog] = useState<{
      isOpen: boolean;
      titleKey: string;
      messageKey: string;
      messageParams?: Record<string, unknown>;
      onConfirm: () => void;
    } | null>(null);

    // List controls (view mode, search, sort)
    const {
      viewMode,
      searchTerm,
      sortField,
      sortOrder,
      isSearchOpen,
      setViewMode,
      setSearchTerm,
      setSortField,
      toggleSortOrder,
      openSearch,
      closeSearch,
      clearSearch,
      filterItems,
      sortItems,
    } = useListControls({ panelId: `prompts-${appId}` });

    // Keyboard shortcut for search (only when panel is open/visible)
    const { data: settings } = useSettingsQuery();
    const searchShortcut = settings?.searchShortcut || "mod+k";
    useSearchShortcut(open ? openSearch : () => {}, searchShortcut);

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

    // Apply filtering and sorting
    const processedEntries = useMemo(() => {
      const items = promptEntries.map(([id, prompt]) => ({
        id,
        prompt,
        name: prompt.name,
        description: prompt.description,
        tags: undefined,
        createdAt: undefined,
        sortIndex: undefined,
      }));

      // Apply filter
      const filtered = filterItems(items);

      // Apply sort (for custom, keep original order)
      const sorted = sortField === "custom" ? filtered : sortItems(filtered);

      return sorted.map(
        (item) => [item.id, item.prompt] as [string, typeof item.prompt],
      );
    }, [promptEntries, filterItems, sortItems, sortField]);

    const enabledPrompt = processedEntries.find(([_, p]) => p.enabled);

    return (
      <div className="flex flex-col h-[calc(100vh-8rem)] px-6">
        <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6">
          <div className="text-sm text-muted-foreground">
            {t("prompts.count", { count: processedEntries.length })} ·{" "}
            {enabledPrompt
              ? t("prompts.enabledName", { name: enabledPrompt[1].name })
              : t("prompts.noneEnabled")}
          </div>
        </div>

        {/* Toolbar */}
        <div className="flex-shrink-0 mb-4">
          <ListToolbar
            viewMode={viewMode}
            sortField={sortField}
            sortOrder={sortOrder}
            isSearchOpen={isSearchOpen}
            isLoading={loading}
            showViewSwitcher={true}
            showAnonymousToggle={false}
            onViewModeChange={setViewMode}
            onSortFieldChange={setSortField}
            onSortOrderToggle={toggleSortOrder}
            onSearchOpen={openSearch}
          />
        </div>

        {/* Search Overlay */}
        <SearchOverlay
          isOpen={isSearchOpen}
          searchTerm={searchTerm}
          placeholder={t("prompts.searchPlaceholder", {
            defaultValue: "Search prompts...",
          })}
          scopeHint={t("search.scopeHint", {
            defaultValue: "Matches name, description, and tags.",
          })}
          onSearchChange={setSearchTerm}
          onClose={closeSearch}
          onClear={clearSearch}
        />

        <div className="flex-1 overflow-y-auto pb-16">
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
          ) : processedEntries.length === 0 ? (
            <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
              {t("search.noResults", {
                defaultValue: "No matching results",
              })}
            </div>
          ) : viewMode === "card" ? (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
              {processedEntries.map(([id, prompt]) => (
                <PromptCardCompact
                  key={id}
                  id={id}
                  prompt={prompt}
                  onToggle={toggleEnabled}
                  onEdit={handleEdit}
                  onDelete={handleDelete}
                />
              ))}
            </div>
          ) : (
            <div className="space-y-3">
              {processedEntries.map(([id, prompt]) => (
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
