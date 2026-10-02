import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import {
  ArrowLeft,
  BookOpen,
  ChartColumn,
  ChevronsLeft,
  ChevronsRight,
  Database,
  Folder,
  Globe,
  History,
  Info,
  KeyRound,
  Layers,
  LayoutGrid,
  Route,
  Server,
  Settings,
  SlidersHorizontal,
} from "lucide-react";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { APP_IDS } from "@/config/appConfig";
import type { GlobalPage, SettingsSection, View } from "@/lib/navigation";
import { isAppPage } from "@/lib/navigation";
import { SkillsIcon } from "@/components/BrandIcons";
import { useUpdate } from "@/contexts/UpdateContext";
import { useSidebarStatus, type AppNavStatus } from "@/hooks/useSidebarStatus";
import { fmtUsd } from "@/components/usage/format";
import { DRAG_REGION_ATTR, DRAG_REGION_STYLE, isMac } from "@/lib/platform";
import { cn } from "@/lib/utils";
import { APP_DISPLAY_NAME, AppGlyph } from "./AppGlyph";

const NO_DRAG = { WebkitAppRegion: "no-drag" } as React.CSSProperties;

type IconComponent = ComponentType<{ className?: string }>;

interface SidebarProps {
  collapsed: boolean;
  onToggleCollapsed: () => void;
  activeApp: AppId;
  view: View;
  visibleApps: VisibleApps;
  settingsSection: SettingsSection;
  onSelectApp: (app: AppId) => void;
  onSelectPage: (page: GlobalPage | "settings") => void;
  onSelectSettingsSection: (section: SettingsSection) => void;
  onExitSettings: () => void;
  /** 打开了「启动时检查应用更新」并且查到了新版本 */
  appsUpdateAvailable?: boolean;
}

/**
 * 主导航（v7）：顶条 44 → 应用列表（唯一滚动的区域）→ 全局 6 项（贴底）→ 底栏「应用 · 设置」。
 * 进入设置后整条侧栏换成设置目录。收起时是 72px 的图标轨。
 */
export function Sidebar(props: SidebarProps) {
  const { t } = useTranslation();
  const { collapsed, onToggleCollapsed, view } = props;
  const inSettings = view === "settings";

  return (
    <nav
      aria-label={t("nav.mainLabel")}
      className={cn(
        "relative flex h-full shrink-0 flex-col border-e border-border bg-sidebar text-body text-fg-1",
        collapsed ? "w-[72px]" : "w-[200px]",
      )}
    >
      <a
        href="#main-content"
        className="absolute -start-[999px] top-2 z-10 rounded-control bg-surface px-2 py-1 text-caption shadow-v7-sm focus:start-2"
      >
        {t("nav.skipToContent")}
      </a>
      <SidebarTopBar collapsed={collapsed} onToggle={onToggleCollapsed} />
      {inSettings ? (
        <SettingsDirectory {...props} />
      ) : (
        <MainDirectory {...props} />
      )}
    </nav>
  );
}

function SidebarTopBar({
  collapsed,
  onToggle,
}: {
  collapsed: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  const label = collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar");
  const Icon = collapsed ? ChevronsRight : ChevronsLeft;
  const toggleButton = (
    <button
      type="button"
      onClick={onToggle}
      aria-label={label}
      title={label}
      style={NO_DRAG}
      className="flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-[background-color,color,scale] hover:bg-subtle hover:text-fg-1 active:scale-[0.96]"
    >
      <Icon className="h-4 w-4" strokeWidth={1.5} />
    </button>
  );

  // macOS 的红绿灯占着顶条左边约 70px：图标轨只有 72 宽，按钮挪到顶条下面一行
  if (collapsed && isMac()) {
    return (
      <>
        <div
          className="h-11 shrink-0"
          {...DRAG_REGION_ATTR}
          style={DRAG_REGION_STYLE as React.CSSProperties}
        />
        <div className="flex h-8 shrink-0 items-center justify-center">
          {toggleButton}
        </div>
      </>
    );
  }

  return (
    <div
      className={cn(
        "flex h-11 shrink-0 items-center gap-0.5 px-2",
        collapsed ? "justify-center" : "justify-end",
      )}
      {...DRAG_REGION_ATTR}
      style={DRAG_REGION_STYLE as React.CSSProperties}
    >
      {!collapsed && !isMac() && (
        <span className="me-auto ps-2 text-caption font-semibold text-fg-3">
          CC Switch
        </span>
      )}
      {toggleButton}
    </div>
  );
}

// ─── 主目录 ────────────────────────────────────────────────────────────────

function MainDirectory({
  collapsed,
  activeApp,
  view,
  visibleApps,
  onSelectApp,
  onSelectPage,
  appsUpdateAvailable = false,
}: SidebarProps) {
  const { t } = useTranslation();
  const { hasUpdate } = useUpdate();
  const { appStatus, todayCost, authNeedsAttention } = useSidebarStatus();
  const apps = APP_IDS.filter((app) => visibleApps[app]);
  const onAppPage = isAppPage(view);

  const todayLabel =
    todayCost > 0
      ? t("nav.todayCost", { cost: fmtUsd(todayCost, 2) })
      : undefined;

  const globals: {
    page: GlobalPage;
    label: string;
    icon: IconComponent;
    trailing?: string;
    alert?: string;
  }[] = [
    {
      page: "usage",
      label: t("nav.usage"),
      icon: ChartColumn,
      trailing: todayLabel,
    },
    {
      page: "auth",
      label: t("nav.auth"),
      icon: KeyRound,
      alert: authNeedsAttention ? t("nav.authNeedsReauth") : undefined,
    },
    { page: "mcp", label: "MCP", icon: Server },
    { page: "skills", label: "Skills", icon: SkillsIcon },
    { page: "prompts", label: t("nav.prompts"), icon: BookOpen },
    { page: "sessions", label: t("nav.sessions"), icon: History },
  ];

  const isGlobalSelected = (page: GlobalPage) =>
    view === page || (page === "skills" && view === "skillsDiscovery");

  return (
    <>
      <div
        className={cn(
          "flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden pb-2 pt-0.5",
          // 图标轨只有 72 宽，放不下滚动条
          collapsed && "no-scrollbar",
        )}
      >
        <div
          role="group"
          aria-label={t("nav.appsGroup")}
          className="flex shrink-0 flex-col"
        >
          {apps.map((app) => (
            <AppNavItem
              key={app}
              app={app}
              collapsed={collapsed}
              selected={onAppPage && activeApp === app}
              status={appStatus(app)}
              onSelect={() => onSelectApp(app)}
            />
          ))}
        </div>
      </div>

      <div
        className={cn(
          "h-px shrink-0 bg-border",
          collapsed ? "mx-[18px] my-1.5" : "mx-4 mb-1.5 mt-0.5",
        )}
      />

      <div className="flex shrink-0 flex-col">
        {globals.map((item) => (
          <NavItem
            key={item.page}
            collapsed={collapsed}
            selected={isGlobalSelected(item.page)}
            icon={item.icon}
            label={item.label}
            trailing={item.trailing}
            alert={item.alert}
            onClick={() => onSelectPage(item.page)}
          />
        ))}
      </div>

      <div
        className={cn(
          "flex shrink-0 border-t border-border pb-2 pt-1.5",
          collapsed ? "flex-col gap-0.5" : "mx-0 gap-1 px-2",
        )}
      >
        <NavItem
          collapsed={collapsed}
          compact={!collapsed}
          selected={view === "apps"}
          icon={LayoutGrid}
          label={t("nav.apps")}
          title={
            appsUpdateAvailable ? t("nav.appsHasUpdate") : t("nav.appsTitle")
          }
          dot={appsUpdateAvailable}
          onClick={() => onSelectPage("apps")}
        />
        <NavItem
          collapsed={collapsed}
          compact={!collapsed}
          selected={false}
          icon={Settings}
          label={t("nav.settings")}
          title={hasUpdate ? t("nav.settingsHasUpdate") : t("nav.settings")}
          dot={hasUpdate}
          onClick={() => onSelectPage("settings")}
        />
      </div>
    </>
  );
}

function modeTag(
  status: AppNavStatus,
  t: (key: string) => string,
): { label: string; className: string } | null {
  if (status.mapping) {
    return {
      label: t("nav.mode.mapping"),
      className: "bg-route-soft text-route-text",
    };
  }
  if (status.mode === "route") {
    return {
      label: t("nav.mode.route"),
      className: "bg-route-soft text-route-text",
    };
  }
  if (status.mode === "stack") {
    return {
      label: t("nav.mode.stack"),
      className: "bg-stack-soft text-stack-text",
    };
  }
  return null;
}

function AppNavItem({
  app,
  collapsed,
  selected,
  status,
  onSelect,
}: {
  app: AppId;
  collapsed: boolean;
  selected: boolean;
  status: AppNavStatus;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const name = APP_DISPLAY_NAME[app];
  const tag = modeTag(status, t);
  const modeName = status.mapping
    ? t("nav.mode.mappingFull")
    : status.mode === "route"
      ? t("nav.mode.route")
      : status.mode === "stack"
        ? t("nav.mode.stack")
        : null;
  const tip = status.alert
    ? `${name} · ${t("nav.needsAttention")}`
    : modeName
      ? `${name} · ${modeName}`
      : name;
  const badgeBg = selected ? "bg-selected" : "bg-sidebar";

  if (collapsed) {
    const marker = status.alert
      ? { className: "bg-danger text-action-fg", icon: AlertGlyph }
      : status.mode === "stack"
        ? { className: "bg-stack-solid text-stack-on", icon: Layers }
        : status.mode === "route" || status.mapping
          ? { className: "bg-route-solid text-route-on", icon: Route }
          : null;
    return (
      <button
        type="button"
        onClick={onSelect}
        aria-label={tip}
        aria-current={selected ? "page" : undefined}
        title={tip}
        className={cn(
          "ms-3 flex h-8 w-12 shrink-0 items-center justify-center rounded-control transition-colors hover:bg-subtle",
          selected && "bg-selected hover:bg-selected",
        )}
      >
        <span className="relative flex">
          <AppGlyph app={app} size={20} badgeClassName={badgeBg} />
          {marker && (
            <span
              aria-hidden="true"
              className={cn(
                "absolute -end-[7px] -top-1.5 flex h-3.5 w-3.5 items-center justify-center rounded-full border-2",
                selected ? "border-selected" : "border-sidebar",
                marker.className,
              )}
            >
              <marker.icon className="h-2 w-2" strokeWidth={3} />
            </span>
          )}
        </span>
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={selected ? "page" : undefined}
      title={tip}
      className={cn(
        "mx-2 flex h-7 w-[184px] shrink-0 items-center gap-2 rounded-control px-2 text-start transition-colors hover:bg-subtle",
        selected && "bg-selected font-medium hover:bg-selected",
      )}
    >
      <AppGlyph app={app} size={16} badgeClassName={badgeBg} />
      <span className="min-w-0 flex-1 truncate">{name}</span>
      {status.alert ? (
        <span
          role="img"
          aria-label={t("nav.needsAttention")}
          className="h-1.5 w-1.5 shrink-0 rounded-full bg-danger"
        />
      ) : (
        // 选中的应用不再显示模式标签：页头下面的模式行已经写明
        tag &&
        !selected && (
          <span
            className={cn(
              "h-[18px] whitespace-nowrap rounded-full px-1.5 text-badge leading-[18px]",
              tag.className,
            )}
          >
            {tag.label}
          </span>
        )
      )}
    </button>
  );
}

function AlertGlyph({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={3.5}
      strokeLinecap="round"
      className={className}
    >
      <path d="M12 4v10" />
      <path d="M12 20h.01" />
    </svg>
  );
}

function NavItem({
  collapsed,
  compact = false,
  selected,
  icon: Icon,
  label,
  title,
  trailing,
  alert,
  dot = false,
  endDot = false,
  onClick,
}: {
  collapsed: boolean;
  /** 底栏两格并排：宽度由父级平分 */
  compact?: boolean;
  selected: boolean;
  icon: IconComponent;
  label: string;
  title?: string;
  trailing?: string;
  /** 红点：需要处理，内容是给读屏的说明 */
  alert?: string;
  /** 中性圆点：有更新 */
  dot?: boolean;
  /** 圆点放在行尾（设置目录里的「关于」），而不是图标角上 */
  endDot?: boolean;
  onClick: () => void;
}) {
  const ring = selected ? "border-selected" : "border-sidebar";
  const accessibleName = [label, trailing, alert].filter(Boolean).join(" · ");
  const glyph = (size: number) => (
    <span aria-hidden="true" className="relative flex shrink-0 text-fg-2">
      <Icon className={size === 20 ? "h-5 w-5" : "h-[18px] w-[18px]"} />
      {dot && (collapsed || !endDot) && (
        <span
          className={cn(
            "absolute -end-1 -top-[3px] h-2.5 w-2.5 rounded-full border-2 bg-fg-1",
            ring,
          )}
        />
      )}
      {collapsed && alert && (
        <span
          className={cn(
            "absolute -end-[7px] -top-1.5 flex h-3.5 w-3.5 items-center justify-center rounded-full border-2 bg-danger text-action-fg",
            ring,
          )}
        >
          <AlertGlyph className="h-2 w-2" />
        </span>
      )}
    </span>
  );

  if (collapsed) {
    return (
      <button
        type="button"
        onClick={onClick}
        aria-label={title ?? accessibleName}
        aria-current={selected ? "page" : undefined}
        title={title ?? accessibleName}
        className={cn(
          "ms-3 flex h-8 w-12 shrink-0 items-center justify-center rounded-control transition-colors hover:bg-subtle",
          selected && "bg-selected hover:bg-selected",
        )}
      >
        {glyph(20)}
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={selected ? "page" : undefined}
      title={title}
      className={cn(
        "flex h-7 shrink-0 items-center gap-2 rounded-control px-2 text-start transition-colors hover:bg-subtle",
        compact ? "min-w-0 flex-1" : "mx-2 w-[184px]",
        selected && "bg-selected font-medium hover:bg-selected",
      )}
    >
      {glyph(18)}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing && (
        <span className="shrink-0 text-caption tabular-nums text-fg-3">
          {trailing}
        </span>
      )}
      {alert && (
        <span
          role="img"
          aria-label={alert}
          title={alert}
          className="h-1.5 w-1.5 shrink-0 rounded-full bg-danger"
        />
      )}
      {dot && endDot && (
        <span
          aria-hidden="true"
          className="h-1.5 w-1.5 shrink-0 rounded-full bg-fg-1"
        />
      )}
    </button>
  );
}

// ─── 设置目录 ──────────────────────────────────────────────────────────────

const SETTINGS_ITEMS: { section: SettingsSection; icon: IconComponent }[] = [
  { section: "general", icon: SlidersHorizontal },
  { section: "appConfig", icon: Folder },
  { section: "routing", icon: Route },
  { section: "network", icon: Globe },
  { section: "data", icon: Database },
  { section: "about", icon: Info },
];

function SettingsDirectory({
  collapsed,
  settingsSection,
  onSelectSettingsSection,
  onExitSettings,
}: SidebarProps) {
  const { t } = useTranslation();
  const { hasUpdate } = useUpdate();
  const { data: version } = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion(),
    staleTime: Infinity,
  });

  return (
    <>
      {collapsed ? (
        <NavItem
          collapsed
          selected={false}
          icon={ArrowLeft}
          label={t("nav.back")}
          onClick={onExitSettings}
        />
      ) : (
        <button
          type="button"
          onClick={onExitSettings}
          className="mx-2 flex h-7 w-[184px] shrink-0 items-center gap-2 rounded-control px-2 text-start text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
        >
          <ArrowLeft className="h-4 w-4" strokeWidth={1.5} aria-hidden="true" />
          {t("nav.back")}
        </button>
      )}
      {!collapsed && (
        <div className="mt-3 px-4 pb-1 text-badge text-fg-3">
          {t("nav.settings")}
        </div>
      )}
      <div
        role="group"
        aria-label={t("nav.settings")}
        className={cn("flex min-h-0 flex-1 flex-col", collapsed && "mt-2")}
      >
        {SETTINGS_ITEMS.map(({ section, icon }) => (
          <NavItem
            key={section}
            collapsed={collapsed}
            selected={settingsSection === section}
            icon={icon}
            label={t(`settings.sections.${section}`)}
            dot={section === "about" && hasUpdate}
            endDot
            title={
              section === "about" && hasUpdate
                ? t("nav.settingsHasUpdate")
                : undefined
            }
            onClick={() => onSelectSettingsSection(section)}
          />
        ))}
      </div>
      {!collapsed && version && (
        <div className="shrink-0 px-4 pb-4 text-caption text-fg-3">
          CC Switch v{version}
        </div>
      )}
    </>
  );
}

export type { SidebarProps };
