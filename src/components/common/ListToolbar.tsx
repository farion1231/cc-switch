import React from "react";
import { useTranslation } from "react-i18next";
import {
  List,
  LayoutGrid,
  Search,
  ArrowUpDown,
  ArrowUp,
  ArrowDown,
  EyeOff,
  Eye,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ViewMode, SortField, SortOrder } from "@/hooks/useListControls";
import { cn } from "@/lib/utils";

interface ListToolbarProps {
  viewMode: ViewMode;
  sortField: SortField;
  sortOrder: SortOrder;
  isSearchOpen: boolean;
  isLoading?: boolean;
  showViewSwitcher?: boolean;
  showSortControl?: boolean;
  showSearchTrigger?: boolean;
  // 匿名模式
  isAnonymousMode?: boolean;
  onAnonymousModeToggle?: () => void;
  showAnonymousToggle?: boolean;

  onViewModeChange: (mode: ViewMode) => void;
  onSortFieldChange: (field: SortField) => void;
  onSortOrderToggle: () => void;
  onSearchOpen: () => void;
}

// Detect platform for keyboard shortcut display
const isMac =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);
const modKey = isMac ? "⌘" : "Ctrl";

export const ListToolbar: React.FC<ListToolbarProps> = ({
  viewMode,
  sortField,
  sortOrder,
  isSearchOpen,
  isLoading = false,
  showViewSwitcher = true,
  showSortControl = true,
  showSearchTrigger = true,
  isAnonymousMode = false,
  onAnonymousModeToggle,
  showAnonymousToggle = true,
  onViewModeChange,
  onSortFieldChange,
  onSortOrderToggle,
  onSearchOpen,
}) => {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between gap-2 py-2">
      <div className="flex items-center gap-2">
        {/* View Switcher */}
        {showViewSwitcher && (
          <div className="flex items-center rounded-lg border border-border bg-muted/50 p-0.5">
            <Button
              variant="ghost"
              size="sm"
              className={cn(
                "h-7 w-7 p-0 rounded-md hover:bg-muted hover:text-foreground",
                viewMode === "list" && "bg-muted shadow-sm",
              )}
              onClick={() => onViewModeChange("list")}
              disabled={isLoading}
              title={t("listToolbar.listView", { defaultValue: "List View" })}
            >
              <List className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className={cn(
                "h-7 w-7 p-0 rounded-md hover:bg-muted hover:text-foreground",
                viewMode === "card" && "bg-muted shadow-sm",
              )}
              onClick={() => onViewModeChange("card")}
              disabled={isLoading}
              title={t("listToolbar.cardView", { defaultValue: "Card View" })}
            >
              <LayoutGrid className="h-4 w-4" />
            </Button>
          </div>
        )}

        {/* Search Trigger */}
        {showSearchTrigger && (
          <Button
            variant="outline"
            size="sm"
            className={cn(
              "h-8 gap-1.5 text-muted-foreground hover:text-foreground",
              isSearchOpen && "bg-accent text-accent-foreground",
            )}
            onClick={onSearchOpen}
            disabled={isLoading}
          >
            <Search className="h-3.5 w-3.5" />
            <span className="text-xs">{modKey}K</span>
          </Button>
        )}
        {/* Anonymous Mode Toggle */}
        {showAnonymousToggle && onAnonymousModeToggle && (
          <Button
            variant="outline"
            size="sm"
            className={cn(
              "h-8 w-8 p-0",
              isAnonymousMode
                ? "bg-amber-500/10 text-amber-600 border-amber-500/30 hover:bg-amber-500/20 hover:text-amber-600"
                : "text-muted-foreground hover:text-foreground",
            )}
            onClick={onAnonymousModeToggle}
            disabled={isLoading}
            title={
              isAnonymousMode
                ? t("listToolbar.anonymousModeOn", {
                    defaultValue: "隐私模式已开启，点击关闭",
                  })
                : t("listToolbar.anonymousModeOff", {
                    defaultValue: "开启隐私模式，隐藏敏感信息",
                  })
            }
          >
            {isAnonymousMode ? (
              <EyeOff className="h-4 w-4" />
            ) : (
              <Eye className="h-4 w-4" />
            )}
          </Button>
        )}
      </div>

      {/* Sort Control */}
      {showSortControl && (
        <div className="flex items-center gap-1.5">
          <Select
            value={sortField}
            onValueChange={(value) => onSortFieldChange(value as SortField)}
            disabled={isLoading}
          >
            <SelectTrigger className="h-8 w-[120px] text-xs">
              <ArrowUpDown className="h-3.5 w-3.5 mr-1.5 text-muted-foreground" />
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="custom">
                {t("listToolbar.sortCustom", { defaultValue: "Custom" })}
              </SelectItem>
              <SelectItem value="name">
                {t("listToolbar.sortName", { defaultValue: "Name" })}
              </SelectItem>
              <SelectItem value="createdAt">
                {t("listToolbar.sortCreatedAt", { defaultValue: "Date Added" })}
              </SelectItem>
            </SelectContent>
          </Select>

          <Button
            variant="ghost"
            size="sm"
            className="h-8 w-8 p-0"
            onClick={onSortOrderToggle}
            disabled={isLoading}
            title={
              sortOrder === "asc"
                ? t("listToolbar.sortAsc", { defaultValue: "Ascending" })
                : t("listToolbar.sortDesc", { defaultValue: "Descending" })
            }
          >
            {sortOrder === "asc" ? (
              <ArrowUp className="h-4 w-4" />
            ) : (
              <ArrowDown className="h-4 w-4" />
            )}
          </Button>
        </div>
      )}
    </div>
  );
};

export default ListToolbar;
