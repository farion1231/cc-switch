import { useTranslation } from "react-i18next";
import { Search, X, CheckSquare, XSquare } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import {
  LOCAL_SOURCE_KEY,
  type GroupKey,
  type SortKey,
} from "./useSkillsFilterSort";

interface SkillsToolbarProps {
  searchQuery: string;
  onSearchChange: (q: string) => void;

  sourceOptions: string[];
  filterSources: Set<string>;
  onToggleSource: (key: string) => void;

  filterUpdateOnly: boolean;
  onToggleUpdateOnly: () => void;
  updateAvailableCount: number;

  sortKey: SortKey;
  onSortChange: (k: SortKey) => void;

  groupKey: GroupKey;
  onGroupChange: (k: GroupKey) => void;

  selectionMode: boolean;
  onToggleSelectionMode: () => void;

  hasFilters: boolean;
  onClearFilters: () => void;

  total: number;
  filteredCount: number;
}

const SORT_OPTIONS: SortKey[] = [
  "nameAsc",
  "nameDesc",
  "installedNewest",
  "installedOldest",
  "sourceAsc",
];

const GROUP_OPTIONS: GroupKey[] = ["none", "source", "app"];

export function SkillsToolbar({
  searchQuery,
  onSearchChange,
  sourceOptions,
  filterSources,
  onToggleSource,
  filterUpdateOnly,
  onToggleUpdateOnly,
  updateAvailableCount,
  sortKey,
  onSortChange,
  groupKey,
  onGroupChange,
  selectionMode,
  onToggleSelectionMode,
  hasFilters,
  onClearFilters,
  total,
  filteredCount,
}: SkillsToolbarProps) {
  const { t } = useTranslation();
  const showSecondRow =
    sourceOptions.length > 0 || updateAvailableCount > 0 || hasFilters;

  return (
    <div className="flex flex-col gap-2 pb-2">
      {/* 第一行：搜索 + 计数 + 排序 + 分组 + 多选 */}
      <div className="flex flex-col gap-2 md:flex-row md:items-center">
        <div className="relative flex-1 min-w-0">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
          <Input
            type="text"
            placeholder={t("skills.toolbar.searchPlaceholder")}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            className="pl-9 pr-9"
          />
          {searchQuery && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => onSearchChange("")}
              className="absolute right-1 top-1/2 -translate-y-1/2 h-7 w-7"
              aria-label={t("common.clear")}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>

        <span className="text-xs text-muted-foreground whitespace-nowrap min-w-[3.5rem] text-right">
          {t("skills.toolbar.showing", {
            filtered: filteredCount,
            total,
          })}
        </span>

        <div className="flex items-center gap-2">
          <div className="w-40">
            <Select
              value={sortKey}
              onValueChange={(v) => onSortChange(v as SortKey)}
            >
              <SelectTrigger className="bg-card border shadow-sm text-foreground">
                <SelectValue
                  placeholder={t("skills.toolbar.sortBy")}
                  className="text-left truncate"
                />
              </SelectTrigger>
              <SelectContent className="bg-card text-foreground shadow-lg">
                {SORT_OPTIONS.map((k) => (
                  <SelectItem key={k} value={k}>
                    {t(`skills.sort.${k}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="w-32">
            <Select
              value={groupKey}
              onValueChange={(v) => onGroupChange(v as GroupKey)}
            >
              <SelectTrigger className="bg-card border shadow-sm text-foreground">
                <SelectValue
                  placeholder={t("skills.toolbar.groupBy")}
                  className="text-left truncate"
                />
              </SelectTrigger>
              <SelectContent className="bg-card text-foreground shadow-lg">
                {GROUP_OPTIONS.map((k) => (
                  <SelectItem key={k} value={k}>
                    <span className="flex flex-col items-start">
                      <span>{t(`skills.group.${k}`)}</span>
                      {k === "app" && (
                        <span className="text-[10px] text-muted-foreground">
                          {t("skills.group.appHint")}
                        </span>
                      )}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <Button
            type="button"
            variant={selectionMode ? "default" : "outline"}
            size="sm"
            className="h-9 gap-1 shrink-0"
            onClick={onToggleSelectionMode}
            title={
              selectionMode
                ? t("skills.toolbar.exitMultiSelect")
                : t("skills.toolbar.multiSelectMode")
            }
          >
            {selectionMode ? (
              <XSquare className="h-4 w-4" />
            ) : (
              <CheckSquare className="h-4 w-4" />
            )}
            <span className="hidden md:inline">
              {selectionMode
                ? t("skills.toolbar.exitMultiSelect")
                : t("skills.toolbar.multiSelectMode")}
            </span>
          </Button>
        </div>
      </div>

      {/* 第二行：来源 chips + 仅有更新 + 清除筛选（仅当有内容时显示） */}
      {showSecondRow && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs">
          {sourceOptions.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-muted-foreground mr-0.5">
                {t("skills.filter.source")}:
              </span>
              {sourceOptions.map((src) => {
                const active = filterSources.has(src);
                return (
                  <button
                    key={src}
                    type="button"
                    onClick={() => onToggleSource(src)}
                    className="focus:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded-full"
                  >
                    <Badge
                      variant={active ? "default" : "outline"}
                      className={cn(
                        "cursor-pointer font-normal",
                        !active && "hover:bg-muted",
                      )}
                    >
                      {src === LOCAL_SOURCE_KEY
                        ? t("skills.filter.local")
                        : src}
                    </Badge>
                  </button>
                );
              })}
            </div>
          )}

          {updateAvailableCount > 0 && (
            <button
              type="button"
              onClick={onToggleUpdateOnly}
              className="focus:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded-full"
            >
              <Badge
                variant={filterUpdateOnly ? "default" : "outline"}
                className={cn(
                  "cursor-pointer font-normal",
                  !filterUpdateOnly && "hover:bg-muted",
                )}
              >
                {t("skills.filter.updateOnly", {
                  count: updateAvailableCount,
                })}
              </Badge>
            </button>
          )}

          {hasFilters && (
            <Button
              type="button"
              variant="link"
              onClick={onClearFilters}
              className="ml-auto h-auto p-0 text-xs text-muted-foreground hover:text-foreground"
            >
              {t("skills.toolbar.clearFilters")}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
