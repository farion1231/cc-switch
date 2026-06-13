import {
  ListPlus,
  Minus,
  Plus,
  Search,
  Trash2,
  TestTube2,
  X,
  Rows3,
  LayoutGrid,
} from "lucide-react";
import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

export type ProviderViewMode = "cards" | "compact";

interface ProviderManagementToolbarProps {
  searchTerm: string;
  onSearchTermChange: (value: string) => void;
  searchInputRef?: RefObject<HTMLInputElement>;
  visibleCount: number;
  totalCount: number;
  selectedCount: number;
  viewMode: ProviderViewMode;
  onViewModeChange: (mode: ProviderViewMode) => void;
  onClearSelection: () => void;
  onBatchTest?: () => void;
  onBatchAddToConfig?: () => void;
  onBatchRemoveFromConfig?: () => void;
  onBatchAddToFailover?: () => void;
  onBatchRemoveFromFailover?: () => void;
  onBatchDelete?: () => void;
}

export function ProviderManagementToolbar({
  searchTerm,
  onSearchTermChange,
  searchInputRef,
  visibleCount,
  totalCount,
  selectedCount,
  viewMode,
  onViewModeChange,
  onClearSelection,
  onBatchTest,
  onBatchAddToConfig,
  onBatchRemoveFromConfig,
  onBatchAddToFailover,
  onBatchRemoveFromFailover,
  onBatchDelete,
}: ProviderManagementToolbarProps) {
  const { t } = useTranslation();
  const hasSelection = selectedCount > 0;

  return (
    <div className="rounded-lg border border-border bg-card/70 px-3 py-3">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={searchInputRef}
            value={searchTerm}
            onChange={(event) => onSearchTermChange(event.target.value)}
            placeholder={t("provider.searchPlaceholder", {
              defaultValue:
                "Search providers, URLs, models, or key fingerprints...",
            })}
            aria-label={t("provider.searchAriaLabel", {
              defaultValue: "Search providers",
            })}
            className="h-9 pl-9 pr-9"
          />
          {searchTerm && (
            <Button
              type="button"
              size="icon"
              variant="ghost"
              className="absolute right-1 top-1/2 h-7 w-7 -translate-y-1/2"
              onClick={() => onSearchTermChange("")}
              aria-label={t("common.clear", { defaultValue: "Clear" })}
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <div className="text-xs text-muted-foreground">
            {t("provider.management.resultCount", {
              visible: visibleCount,
              total: totalCount,
              defaultValue: `Showing ${visibleCount} of ${totalCount}`,
            })}
          </div>

          <div className="flex rounded-md border border-border p-0.5">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className={cn(
                "h-7 gap-1.5 px-2",
                viewMode === "cards" && "bg-muted",
              )}
              onClick={() => onViewModeChange("cards")}
            >
              <LayoutGrid className="h-3.5 w-3.5" />
              {t("provider.management.cards", { defaultValue: "Cards" })}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className={cn(
                "h-7 gap-1.5 px-2",
                viewMode === "compact" && "bg-muted",
              )}
              onClick={() => onViewModeChange("compact")}
            >
              <Rows3 className="h-3.5 w-3.5" />
              {t("provider.management.compact", { defaultValue: "Compact" })}
            </Button>
          </div>
        </div>
      </div>

      {hasSelection && (
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3">
          <span className="text-xs font-medium text-muted-foreground">
            {t("provider.management.selectedCount", {
              count: selectedCount,
              defaultValue: `${selectedCount} selected`,
            })}
          </span>
          <div className="flex flex-wrap items-center gap-2">
            {onBatchTest && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-8 gap-1.5"
                onClick={onBatchTest}
              >
                <TestTube2 className="h-3.5 w-3.5" />
                {t("provider.management.batchTest", {
                  defaultValue: "Test selected",
                })}
              </Button>
            )}
            {onBatchAddToConfig && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-8 gap-1.5"
                onClick={onBatchAddToConfig}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("provider.management.batchAddToConfig", {
                  defaultValue: "Add selected",
                })}
              </Button>
            )}
            {onBatchRemoveFromConfig && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-8 gap-1.5"
                onClick={onBatchRemoveFromConfig}
              >
                <Minus className="h-3.5 w-3.5" />
                {t("provider.management.batchRemoveFromConfig", {
                  defaultValue: "Remove selected",
                })}
              </Button>
            )}
            {onBatchAddToFailover && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-8 gap-1.5"
                onClick={onBatchAddToFailover}
              >
                <ListPlus className="h-3.5 w-3.5" />
                {t("provider.management.batchAddToFailover", {
                  defaultValue: "Add to queue",
                })}
              </Button>
            )}
            {onBatchRemoveFromFailover && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-8 gap-1.5"
                onClick={onBatchRemoveFromFailover}
              >
                <Minus className="h-3.5 w-3.5" />
                {t("provider.management.batchRemoveFromFailover", {
                  defaultValue: "Remove from queue",
                })}
              </Button>
            )}
            {onBatchDelete && (
              <Button
                type="button"
                size="sm"
                variant="destructive"
                className="h-8 gap-1.5"
                onClick={onBatchDelete}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("provider.management.batchDelete", {
                  defaultValue: "Delete selected",
                })}
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-8"
              onClick={onClearSelection}
            >
              {t("common.clear", { defaultValue: "Clear" })}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
