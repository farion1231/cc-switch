import { useDeferredValue, useEffect, useMemo, useState } from "react";
import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Command as CommandPrimitive } from "cmdk";
import {
  BookOpen,
  ChartColumn,
  History,
  KeyRound,
  LayoutGrid,
  Loader2,
  Search,
  Server,
  Settings,
} from "lucide-react";
import { SkillsIcon } from "@/components/BrandIcons";
import { AppGlyph } from "@/components/shell/AppGlyph";
import {
  searchItems,
  type SearchItem,
  type SearchResultGroup,
  type SearchTarget,
} from "./searchIndex";
import {
  useGlobalSearchItems,
  type GlobalSearchScope,
} from "./useGlobalSearchItems";

type IconComponent = ComponentType<{
  className?: string;
  strokeWidth?: number | string;
}>;

const PAGE_ICON: Record<string, IconComponent> = {
  mcp: Server,
  skills: SkillsIcon,
  skillsDiscovery: SkillsIcon,
  prompts: BookOpen,
  sessions: History,
  auth: KeyRound,
  usage: ChartColumn,
  apps: LayoutGrid,
  settings: Settings,
};

function ItemIcon({ item }: { item: SearchItem }) {
  if ("app" in item.icon) {
    return (
      <AppGlyph
        app={item.icon.app}
        size={16}
        badgeClassName="bg-surface group-data-[selected=true]:bg-selected"
      />
    );
  }
  const Icon = PAGE_ICON[item.icon.page] ?? Search;
  return <Icon className="h-4 w-4 text-fg-2" strokeWidth={1.5} />;
}

export interface GlobalSearchDialogProps extends GlobalSearchScope {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSelect: (target: SearchTarget) => void;
}

/**
 * 全局搜索（⌘K / Ctrl+K）：一处搜页面、设置项和各页条目，选中后跳过去并定位到那一项。
 * 不输入时只列页面，当作快速导航；输入后各组按匹配程度排，每组最多 8 条。
 */
export function GlobalSearchDialog({
  open,
  onOpenChange,
  onSelect,
  ...scope
}: GlobalSearchDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const { items, loading } = useGlobalSearchItems(open, scope);

  // 每次打开都是空的：上一次的词留着反而要先删
  useEffect(() => {
    if (open) setQuery("");
  }, [open]);

  const trimmed = deferredQuery.trim();
  const groups = useMemo<SearchResultGroup[]>(() => {
    if (!trimmed) {
      const pages = items.filter((item) => item.group === "pages");
      return pages.length ? [{ group: "pages", items: pages, hidden: 0 }] : [];
    }
    return searchItems(items, trimmed);
  }, [items, trimmed]);

  const select = (item: SearchItem) => {
    onOpenChange(false);
    onSelect(item.target);
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-overlay data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0" />
        <DialogPrimitive.Content
          aria-describedby={undefined}
          data-global-search=""
          className="fixed left-1/2 top-[12vh] z-50 flex max-h-[min(560px,76vh)] w-[min(600px,calc(100vw-48px))] -translate-x-1/2 flex-col overflow-hidden rounded-dialog border border-border bg-surface text-fg-1 shadow-v7-lg duration-150 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95"
        >
          <DialogPrimitive.Title className="sr-only">
            {t("globalSearch.title")}
          </DialogPrimitive.Title>
          <CommandPrimitive
            shouldFilter={false}
            loop
            label={t("globalSearch.title")}
            className="flex min-h-0 flex-1 flex-col"
          >
            <div className="flex h-12 shrink-0 items-center gap-2.5 border-b border-border px-4">
              <Search
                aria-hidden="true"
                className="h-4 w-4 shrink-0 text-fg-3"
                strokeWidth={1.5}
              />
              <CommandPrimitive.Input
                value={query}
                onValueChange={setQuery}
                placeholder={t("globalSearch.placeholder")}
                aria-label={t("globalSearch.title")}
                spellCheck={false}
                autoComplete="off"
                className="h-full min-w-0 flex-1 bg-transparent text-body text-fg-1 outline-none placeholder:text-fg-3 focus-visible:outline-none"
              />
              {loading && (
                <Loader2
                  role="status"
                  aria-label={t("globalSearch.loading")}
                  className="h-3.5 w-3.5 shrink-0 animate-spin text-fg-3"
                />
              )}
              <kbd className="shrink-0 rounded-[4px] border border-border px-1.5 py-px font-sans text-badge text-fg-3">
                Esc
              </kbd>
            </div>

            <CommandPrimitive.List className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-1.5 scroll-stable">
              {groups.length === 0 && (
                <div className="px-4 py-10 text-center text-body text-fg-2">
                  {trimmed
                    ? t("globalSearch.empty", { query: trimmed })
                    : t("globalSearch.loading")}
                </div>
              )}
              {groups.map(({ group, items: groupItems, hidden }) => (
                <CommandPrimitive.Group
                  key={group}
                  heading={t(`globalSearch.groups.${group}`)}
                  className="[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:text-badge [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-fg-3"
                >
                  {groupItems.map((item) => (
                    <CommandPrimitive.Item
                      key={item.id}
                      value={item.id}
                      onSelect={() => select(item)}
                      className="group flex h-9 cursor-default select-none items-center gap-2.5 rounded-control px-2.5 text-body outline-none data-[selected=true]:bg-selected"
                    >
                      <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                        <ItemIcon item={item} />
                      </span>
                      <span className="min-w-0 flex-1 truncate">
                        {item.title}
                      </span>
                      {item.subtitle && (
                        <span className="min-w-0 max-w-[45%] shrink truncate text-caption text-fg-3">
                          {item.subtitle}
                        </span>
                      )}
                    </CommandPrimitive.Item>
                  ))}
                  {hidden > 0 && (
                    <div className="px-2.5 py-1 text-caption text-fg-3">
                      {t("globalSearch.more", { count: hidden })}
                    </div>
                  )}
                </CommandPrimitive.Group>
              ))}
            </CommandPrimitive.List>

            <div
              aria-hidden="true"
              className="flex h-9 shrink-0 items-center gap-4 border-t border-border px-4 text-caption text-fg-3"
            >
              <span>
                <kbd className="font-sans">↑↓</kbd> {t("globalSearch.hintMove")}
              </span>
              <span>
                <kbd className="font-sans">↵</kbd> {t("globalSearch.hintOpen")}
              </span>
              <span>
                <kbd className="font-sans">Esc</kbd>{" "}
                {t("globalSearch.hintClose")}
              </span>
            </div>
          </CommandPrimitive>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
