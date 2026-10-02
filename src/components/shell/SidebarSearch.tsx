import {
  forwardRef,
  useEffect,
  useMemo,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import {
  BookOpen,
  ChartColumn,
  Database,
  Folder,
  Globe,
  History,
  Info,
  KeyRound,
  LayoutGrid,
  Route,
  Search,
  Server,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { SkillsIcon } from "@/components/BrandIcons";
import { HoverTip } from "@/components/ui/hover-tip";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { APP_IDS } from "@/config/appConfig";
import type { GlobalPage, SettingsSection } from "@/lib/navigation";
import { isMac } from "@/lib/platform";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { APP_DISPLAY_NAME, AppGlyph } from "./AppGlyph";

type IconComponent = ComponentType<{ className?: string }>;
type SearchGroup = "apps" | "pages" | "settings";

const PAGES: {
  page: GlobalPage;
  labelKey?: string;
  label?: string;
  icon: IconComponent;
}[] = [
  { page: "usage", labelKey: "nav.usage", icon: ChartColumn },
  { page: "auth", labelKey: "nav.auth", icon: KeyRound },
  { page: "mcp", label: "MCP", icon: Server },
  { page: "skills", label: "Skills", icon: SkillsIcon },
  { page: "prompts", labelKey: "nav.prompts", icon: BookOpen },
  { page: "sessions", labelKey: "nav.sessions", icon: History },
  { page: "apps", labelKey: "nav.apps", icon: LayoutGrid },
];

const SETTINGS: { section: SettingsSection; icon: IconComponent }[] = [
  { section: "general", icon: SlidersHorizontal },
  { section: "appConfig", icon: Folder },
  { section: "routing", icon: Route },
  { section: "network", icon: Globe },
  { section: "data", icon: Database },
  { section: "about", icon: Info },
];

interface SearchItem {
  id: string;
  group: SearchGroup;
  label: string;
  /** 参与匹配但不显示的词（英文 id），中文界面里输 codex / mcp 也能搜到 */
  keywords: string;
  icon: ReactNode;
  run: () => void;
}

export interface SidebarSearchOptions {
  visibleApps: VisibleApps;
  onSelectApp: (app: AppId) => void;
  onSelectPage: (page: GlobalPage) => void;
  onOpenSettings?: (section: SettingsSection) => void;
}

export type SidebarSearchState = ReturnType<typeof useSidebarSearch>;

/**
 * 侧栏搜索（取代原来的 ⌘K 弹窗）：输入时侧栏列表原地换成匹配结果，
 * ↑ / ↓ 移动、回车跳转、Esc 清空。
 */
export function useSidebarSearch({
  visibleApps,
  onSelectApp,
  onSelectPage,
  onOpenSettings,
}: SidebarSearchOptions) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  const items = useMemo<SearchItem[]>(() => {
    const apps: SearchItem[] = APP_IDS.filter((app) => visibleApps[app]).map(
      (app) => ({
        id: `app-${app}`,
        group: "apps",
        label: APP_DISPLAY_NAME[app],
        keywords: app,
        icon: <AppGlyph app={app} size={18} />,
        run: () => onSelectApp(app),
      }),
    );
    const pages: SearchItem[] = PAGES.map(
      ({ page, labelKey, label, icon: Icon }) => ({
        id: `page-${page}`,
        group: "pages",
        label: labelKey ? t(labelKey) : (label ?? page),
        keywords: page,
        icon: <Icon className="h-[18px] w-[18px] text-fg-2" />,
        run: () => onSelectPage(page),
      }),
    );
    const settings: SearchItem[] = onOpenSettings
      ? SETTINGS.map(({ section, icon: Icon }) => ({
          id: `settings-${section}`,
          group: "settings",
          label: t(`settings.sections.${section}`),
          keywords: `settings ${section}`,
          icon: <Icon className="h-[18px] w-[18px] text-fg-2" />,
          run: () => onOpenSettings(section),
        }))
      : [];
    return [...apps, ...pages, ...settings];
  }, [t, visibleApps, onSelectApp, onSelectPage, onOpenSettings]);

  const needle = query.trim().toLowerCase();
  const results = needle
    ? items.filter((item) =>
        `${item.label} ${item.keywords}`.toLowerCase().includes(needle),
      )
    : [];

  useEffect(() => setActive(0), [needle]);

  return {
    query,
    setQuery,
    searching: needle.length > 0,
    results,
    active,
    setActive,
    run: (item: SearchItem) => {
      setQuery("");
      item.run();
    },
  };
}

interface SidebarSearchInputProps {
  search: SidebarSearchState;
  listId: string;
}

/** 列表上方的搜索框（⌘K 聚焦）。 */
export const SidebarSearchInput = forwardRef<
  HTMLInputElement,
  SidebarSearchInputProps
>(function SidebarSearchInput({ search, listId }, ref) {
  const { t } = useTranslation();
  const shortcut = isMac() ? "⌘K" : "Ctrl+K";
  const { query, setQuery, searching, results, active, setActive, run } =
    search;
  const activeItem = searching ? results[active] : undefined;

  return (
    <div className="shrink-0 px-2 pb-2">
      <div className="relative">
        <Search
          aria-hidden="true"
          strokeWidth={1.5}
          className="pointer-events-none absolute start-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-3"
        />
        <input
          ref={ref}
          type="text"
          role="combobox"
          aria-label={t("commandPalette.open", { shortcut })}
          aria-expanded={searching}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={
            activeItem ? `${listId}-${activeItem.id}` : undefined
          }
          placeholder={t("commandPalette.searchField")}
          value={query}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            const count = results.length;
            if (event.key === "ArrowDown" && count > 0) {
              event.preventDefault();
              setActive((active + 1) % count);
            } else if (event.key === "ArrowUp" && count > 0) {
              event.preventDefault();
              setActive((active - 1 + count) % count);
            } else if (event.key === "Enter" && activeItem) {
              event.preventDefault();
              run(activeItem);
              event.currentTarget.blur();
            } else if (event.key === "Escape") {
              if (query) {
                event.preventDefault();
                setQuery("");
              } else {
                event.currentTarget.blur();
              }
            }
          }}
          className={cn(fieldClass, "h-7 pe-12 ps-7")}
        />
        {query ? (
          <HoverTip content={t("common.clear")}>
            <button
              type="button"
              aria-label={t("common.clear")}
              onClick={() => setQuery("")}
              className="absolute end-1 top-1/2 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded-full text-fg-3 transition-colors hover:bg-subtle hover:text-fg-1"
            >
              <X className="h-3 w-3" strokeWidth={2} />
            </button>
          </HoverTip>
        ) : (
          <kbd
            aria-hidden="true"
            className="pointer-events-none absolute end-2 top-1/2 -translate-y-1/2 font-sans text-badge font-normal text-fg-3"
          >
            {shortcut}
          </kbd>
        )}
      </div>
    </div>
  );
});

const GROUP_LABEL: Record<SearchGroup, string> = {
  apps: "commandPalette.apps",
  pages: "commandPalette.pages",
  settings: "commandPalette.settings",
};

/** 搜索时代替侧栏列表的结果区：按「应用 / 页面 / 设置」分组。 */
export function SidebarSearchResults({
  search,
  listId,
}: SidebarSearchInputProps) {
  const { t } = useTranslation();
  const { results, active, setActive, run } = search;

  if (results.length === 0) {
    return (
      <div id={listId} role="listbox" className="flex-1 px-4 py-2">
        <p className="text-caption text-fg-3">{t("commandPalette.empty")}</p>
      </div>
    );
  }

  const groups = (["apps", "pages", "settings"] as const)
    .map((group) => ({
      group,
      items: results.filter((item) => item.group === group),
    }))
    .filter(({ items }) => items.length > 0);

  return (
    <div
      id={listId}
      role="listbox"
      aria-label={t("commandPalette.title")}
      className="flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden pb-2"
    >
      {groups.map(({ group, items }) => (
        <div key={group} role="group" aria-label={t(GROUP_LABEL[group])}>
          <div
            aria-hidden="true"
            className="px-4 pb-1 pt-2 text-caption text-fg-3"
          >
            {t(GROUP_LABEL[group])}
          </div>
          {items.map((item) => {
            const index = results.indexOf(item);
            const selected = index === active;
            return (
              <div
                key={item.id}
                id={`${listId}-${item.id}`}
                role="option"
                aria-selected={selected}
                // 不抢输入框的焦点，点完直接跳
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setActive(index)}
                onClick={() => run(item)}
                className={cn(
                  "mx-2 flex h-7 w-[184px] cursor-pointer items-center gap-2 rounded-control px-2",
                  selected && "bg-subtle",
                )}
              >
                <span aria-hidden="true" className="flex shrink-0">
                  {item.icon}
                </span>
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}
