import { useMemo } from "react";
import { FileText, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { Prompt } from "@/lib/api";
import PromptListItem from "./PromptListItem";

interface PromptLibraryProps {
  prompts: Record<string, Prompt>;
  loading: boolean;
  searchQuery: string;
  statusText: string;
  disabled?: boolean;
  onSearchQueryChange: (value: string) => void;
  onToggle: (id: string, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
  isDeleteDisabled?: (id: string, prompt: Prompt) => boolean;
  getDeleteTitle?: (id: string, prompt: Prompt) => string;
}

export function PromptLibrary({
  prompts,
  loading,
  searchQuery,
  statusText,
  disabled = false,
  onSearchQueryChange,
  onToggle,
  onEdit,
  onDelete,
  isDeleteDisabled,
  getDeleteTitle,
}: PromptLibraryProps) {
  const { t } = useTranslation();
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

  return (
    <>
      <div className="mb-4 flex-shrink-0 rounded-xl border border-white/10 px-6 py-4 glass">
        <div className="text-sm text-muted-foreground">
          {t("prompts.count", { count: promptEntries.length })} · {statusText}
        </div>
      </div>

      <ManagementListSearch
        value={searchQuery}
        onValueChange={onSearchQueryChange}
        placeholder={t("prompts.searchPlaceholder")}
        ariaLabel={t("prompts.searchAriaLabel")}
        clearLabel={t("common.clear")}
      />

      <ScrollArea className="-mr-3 min-h-0 flex-1" type="auto">
        <div className="pb-16 pr-3">
          {loading ? (
            <div className="py-12 text-center text-muted-foreground">
              {t("prompts.loading")}
            </div>
          ) : promptEntries.length === 0 ? (
            <div className="py-12 text-center">
              <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted">
                <FileText size={24} className="text-muted-foreground" />
              </div>
              <h3 className="mb-2 text-lg font-medium text-foreground">
                {t("prompts.empty")}
              </h3>
              <p className="text-sm text-muted-foreground">
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
                  onToggle={onToggle}
                  onEdit={onEdit}
                  onDelete={onDelete}
                  disabled={disabled}
                  deleteDisabled={isDeleteDisabled?.(id, prompt)}
                  deleteTitle={getDeleteTitle?.(id, prompt)}
                />
              ))}
            </div>
          )}
        </div>
      </ScrollArea>
    </>
  );
}
