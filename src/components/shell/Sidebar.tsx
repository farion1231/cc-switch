import { usageCostLabel } from "@/components/usage/format";
import { useRef, type ComponentType } from "react";
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
import { orderAppIds } from "@/config/appConfig";
import type { GlobalPage, SettingsSection, View } from "@/lib/navigation";
import { isAppPage } from "@/lib/navigation";
import { SkillsIcon } from "@/components/BrandIcons";
import { useUpdate } from "@/contexts/UpdateContext";
import { sidebarWidth, useSidebarCollapsed } from "@/hooks/useSidebarCollapsed";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useSettingsQuery } from "@/lib/query";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
  resolveSidebarToday,
  type SidebarTodayItem,
} from "@/components/usage/format";
import { useSidebarStatus, type AppNavStatus } from "@/hooks/useSidebarStatus";
import { HoverTip } from "@/components/ui/hover-tip";
import { DRAG_REGION_ATTR, DRAG_REGION_STYLE, isMac } from "@/lib/platform";
import { cn } from "@/lib/utils";
import ccswitchLogo from "@/assets/icons/logo.svg";
import { APP_DISPLAY_NAME, AppGlyph } from "./AppGlyph";

const NO_DRAG = { WebkitAppRegion: "no-drag" } as React.CSSProperties;

type IconComponent = ComponentType<{ className?: string }>;

interface SidebarProps {
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

type DirectoryProps = SidebarProps & { collapsed: boolean };

/**
 * 主导航（v7）：顶条 44 → 应用列表（唯一滚动的区域）→ 全局 6 项（贴底）→ 底栏「应用 · 设置」。
 * 进入设置后整条侧栏换成设置目录。收起时是 72px 的图标轨；
 * 展开 / 收起时宽度一步到位，边缘的滑动由盖板的 transform 动画画出（见 useSidebarCollapsed），文字不换行、被裁掉而不是挤成两行。
 * 收起时各行只剩图标，名字由 HoverTip 从右侧报；展开时文字可见，不再挂提示。
 */
export function Sidebar(props: SidebarProps) {
  const { t } = useTranslation();
  const navRef = useRef<HTMLElement>(null);
  const { collapsed, toggle } = useSidebarCollapsed(navRef);
  const { view } = props;
  const inSettings = view === "settings";

  return (
    <nav
      ref={navRef}
      aria-label={t("nav.mainLabel")}
      className={cn(
        "relative flex h-full shrink-0 flex-col overflow-hidden whitespace-nowrap border-e border-border bg-sidebar text-body text-fg-1",
      )}
      style={{ width: sidebarWidth(collapsed) }}
    >
      <a
        href="#main-content"
        className="absolute -start-[999px] top-2 z-10 rounded-control bg-surface px-2 py-1 text-caption shadow-v7-sm focus:start-2"
      >
        {t("nav.skipToContent")}
      </a>
      <SidebarTopBar collapsed={collapsed} onToggle={toggle} />
      {inSettings ? (
        <SettingsDirectory {...props} collapsed={collapsed} />
      ) : (
        <MainDirectory {...props} collapsed={collapsed} />
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
  const tipSide = collapsed ? "right" : "left";
  const label = collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar");
  const Icon = collapsed ? ChevronsRight : ChevronsLeft;
  const toggleButton = (
    <HoverTip content={label} side={tipSide} disableHoverableContent>
      <button
        type="button"
        onClick={onToggle}
        aria-label={label}
        style={NO_DRAG}
        className="flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-[background-color,color,scale] hover:bg-subtle hover:text-fg-1 active:scale-[0.96]"
      >
        <Icon className="h-4 w-4" strokeWidth={1.5} />
      </button>
    </HoverTip>
  );

  // macOS 展开：顶条只放红绿灯；下面一行贴近红绿灯放 logo + 名字，收起箭头放在这一行最右边
  if (!collapsed && isMac()) {
    return (
      <>
        <div
          className="h-11 shrink-0"
          {...DRAG_REGION_ATTR}
          style={DRAG_REGION_STYLE as React.CSSProperties}
        />
        {/* 品牌区与导航之间不画分隔线：靠侧栏底色和留白分开，免得和右侧页头下边框近似平行却差 11px */}
        <div
          className="-mt-[18px] mb-2 flex h-9 shrink-0 items-center gap-2 pe-2 ps-4"
          {...DRAG_REGION_ATTR}
          style={DRAG_REGION_STYLE as React.CSSProperties}
        >
          <img
            src={ccswitchLogo}
            alt=""
            draggable={false}
            className="pointer-events-none h-5 w-5 shrink-0"
          />
          <span className="pointer-events-none me-auto min-w-0 truncate text-strong font-semibold text-fg-1">
            CC Switch
          </span>
          {toggleButton}
        </div>
      </>
    );
  }

  // macOS 的红绿灯占着顶条左边约 70px：图标轨只有 72 宽，按钮挪到顶条下面一行
  if (collapsed && isMac()) {
    return (
      <>
        <div
          className="h-11 shrink-0"
          {...DRAG_REGION_ATTR}
          style={DRAG_REGION_STYLE as React.CSSProperties}
        />
        {/* 往上贴近红绿灯，和展开时品牌行同高（中心 y≈44） */}
        <div className="-mt-4 flex h-8 shrink-0 items-center justify-center">
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
        // 左上角品牌（Windows / Linux）：图标 + 名字。macOS 左上角留给红绿灯，不放。
        // 图片不接收指针事件，拖它也是在拖窗口
        <span className="pointer-events-none me-auto flex min-w-0 items-center gap-2 ps-1.5">
          <img
            src={ccswitchLogo}
            alt=""
            draggable={false}
            className="h-[22px] w-[22px] shrink-0"
          />
          <span className="truncate text-strong font-semibold text-fg-1">
            CC Switch
          </span>
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
}: DirectoryProps) {
  const { t, i18n } = useTranslation();
  const { moneyShort } = useMoneyFormat();
  const { data: settings } = useSettingsQuery();
  const { hasUpdate } = useUpdate();
  const {
    appStatus,
    todayCost,
    todayCostIncomplete,
    todayTokens,
    todayRequests,
    todayHitRate,
    authNeedsAttention,
  } = useSidebarStatus();
  // 顺序跟应用页拖出来的一致
  const apps = orderAppIds(settings?.appOrder).filter(
    (app) => visibleApps[app],
  );
  const onAppPage = isAppPage(view);

  // 选了的项里是 0 的不写（没配定价时金额总是 0）；命中率在今天还没有请求时不写；
  // 全都没有就整个不显示。两项都有时只写数、上下两行（「今日」放进悬停和读屏）：
  // 带上「今日」，换成新台币这类长币种后一行放不下，会把「用量统计」挤成省略号
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const todayValue = (item: SidebarTodayItem): string | null => {
    switch (item) {
      case "cost":
        return todayCostIncomplete
          ? usageCostLabel(
              todayCost,
              true,
              moneyShort(todayCost),
              t("usage.unpriced"),
            )
          : todayCost > 0
            ? moneyShort(todayCost)
            : null;
      case "tokens":
        return todayTokens > 0
          ? formatTokensCompact(todayTokens, locale)
          : null;
      case "requests":
        return todayRequests > 0
          ? t("nav.todayRequests", { value: fmtInt(todayRequests, locale) })
          : null;
      case "hitRate":
        return todayRequests > 0
          ? t("nav.todayHitRate", { value: formatHitRatePercent(todayHitRate) })
          : null;
    }
  };
  const todayValues = resolveSidebarToday(settings?.usageDisplay)
    .map(todayValue)
    .filter((part): part is string => part != null);
  const todayText =
    todayValues.length > 0
      ? t("nav.today", { value: todayValues.join(" · ") })
      : undefined;
  const todayTrailing = todayValues.length > 1 ? todayValues : todayText;

  const globals: {
    page: GlobalPage;
    label: string;
    icon: IconComponent;
    trailing?: string | string[];
    trailingLabel?: string;
    alert?: string;
  }[] = [
    { page: "mcp", label: "MCP", icon: Server },
    { page: "skills", label: "Skills", icon: SkillsIcon },
    { page: "prompts", label: t("nav.prompts"), icon: BookOpen },
    { page: "sessions", label: t("nav.sessions"), icon: History },
    {
      page: "auth",
      label: t("nav.auth"),
      icon: KeyRound,
      alert: authNeedsAttention ? t("nav.authNeedsReauth") : undefined,
    },
    {
      page: "usage",
      label: t("nav.usage"),
      icon: ChartColumn,
      trailing: todayTrailing,
      trailingLabel: todayText,
    },
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
            trailingLabel={item.trailingLabel}
            alert={item.alert}
            onClick={() => onSelectPage(item.page)}
          />
        ))}
      </div>

      {/* 和上面那条分隔线同样缩进、上下各留 6px，选中的最后一项不会贴着线 */}
      <div
        className={cn(
          "my-1.5 h-px shrink-0 bg-border",
          collapsed ? "mx-[18px]" : "mx-4",
        )}
      />

      <div
        className={cn(
          "flex shrink-0 pb-2",
          // 收起时竖排，和上面全局项一样无间隙
          collapsed ? "flex-col" : "gap-1 px-2",
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
          dotTone="success"
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
  const modeName =
    status.mode === "route"
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
        : status.mode === "route"
          ? { className: "bg-route-solid text-route-on", icon: Route }
          : null;
    return (
      <HoverTip content={tip} side="right" disableHoverableContent>
        <button
          type="button"
          onClick={onSelect}
          aria-label={tip}
          aria-current={selected ? "page" : undefined}
          className={cn(
            "mx-auto flex h-8 w-12 shrink-0 items-center justify-center rounded-control transition-colors hover:bg-subtle",
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
      </HoverTip>
    );
  }

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={selected ? "page" : undefined}
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
  trailingLabel,
  alert,
  dot = false,
  dotTone = "neutral",
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
  /** 行尾的小字；放不下时在空格处折成两行，给数组时直接上下排 */
  trailing?: string | string[];
  /** 行尾小字的完整说法（读屏和悬停用），行尾只写了数时给 */
  trailingLabel?: string;
  /** 红点：需要处理，内容是给读屏的说明 */
  alert?: string;
  /** 圆点：有更新 */
  dot?: boolean;
  /** 工具更新用中性色（很多人不想追新）；CC Switch 自己的更新用绿色，和工具更新分开 */
  dotTone?: "neutral" | "success";
  /** 圆点放在行尾（设置目录里的「关于」），而不是图标角上 */
  endDot?: boolean;
  onClick: () => void;
}) {
  const ring = selected ? "border-selected" : "border-sidebar";
  const dotFill = dotTone === "success" ? "bg-success" : "bg-fg-1";
  const trailingLines =
    trailing == null ? [] : Array.isArray(trailing) ? trailing : [trailing];
  const accessibleName = [
    label,
    trailingLabel ?? trailingLines.join(" · "),
    alert,
  ]
    .filter(Boolean)
    .join(" · ");
  const glyph = (size: number) => (
    <span aria-hidden="true" className="relative flex shrink-0 text-fg-2">
      <Icon className={size === 20 ? "h-5 w-5" : "h-[18px] w-[18px]"} />
      {dot && (collapsed || !endDot) && (
        <span
          className={cn(
            "absolute -end-1 -top-[3px] h-2.5 w-2.5 rounded-full border-2",
            dotFill,
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
      <HoverTip
        content={title ?? accessibleName}
        side="right"
        disableHoverableContent
      >
        <button
          type="button"
          onClick={onClick}
          aria-label={title ?? accessibleName}
          aria-current={selected ? "page" : undefined}
          className={cn(
            "mx-auto flex h-8 w-12 shrink-0 items-center justify-center rounded-control transition-colors hover:bg-subtle",
            selected && "bg-selected hover:bg-selected",
          )}
        >
          {glyph(20)}
        </button>
      </HoverTip>
    );
  }

  // 展开时名字可见；只有 title 比名字多说了点什么（如「有可用更新」），
  // 或行尾只写了数（完整说法在 trailingLabel）时才挂提示
  const hoverContent =
    title && title !== label
      ? title
      : trailingLabel
        ? accessibleName
        : undefined;
  return (
    <HoverTip
      content={hoverContent}
      side={compact ? "top" : "right"}
      disableHoverableContent
    >
      <button
        type="button"
        onClick={onClick}
        aria-label={trailingLabel ? accessibleName : undefined}
        aria-current={selected ? "page" : undefined}
        className={cn(
          "flex h-7 shrink-0 items-center gap-2 rounded-control px-2 text-start transition-colors hover:bg-subtle",
          compact ? "min-w-0 flex-1" : "mx-2 w-[184px]",
          selected && "bg-selected font-medium hover:bg-selected",
        )}
      >
        {glyph(18)}
        {trailingLines.length === 0 ? (
          <span className="min-w-0 flex-1 truncate">{label}</span>
        ) : (
          <>
            {/* 有行尾小字时名字不让：放不下就让小字折行（两行正好塞进 28px 的行高） */}
            <span className="shrink-0 grow">{label}</span>
            <span className="min-w-0 text-end text-caption leading-[13px] tabular-nums text-fg-3">
              {trailingLines.map((line) => (
                <span
                  key={line}
                  className={
                    trailingLines.length > 1
                      ? "block whitespace-nowrap"
                      : undefined
                  }
                >
                  {line}
                </span>
              ))}
            </span>
          </>
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
            className={cn("h-1.5 w-1.5 shrink-0 rounded-full", dotFill)}
          />
        )}
      </button>
    </HoverTip>
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
}: DirectoryProps) {
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
            dotTone="success"
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
