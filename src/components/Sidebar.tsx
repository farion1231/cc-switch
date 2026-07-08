import { memo, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  Plus,
  Settings,
  Book,
  Brain,
  Wrench,
  History,
  BarChart2,
  FolderOpen,
  KeyRound,
  Shield,
  Cpu,
  LayoutDashboard,
  Layers,
} from "lucide-react";
import type { VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { ProviderIcon } from "@/components/ProviderIcon";
import { McpIcon } from "@/components/BrandIcons";
import { cn } from "@/lib/utils";
import { DRAG_REGION_STYLE } from "@/lib/platform";
import { Monitor, Terminal } from "lucide-react";

/* ── App definitions ─────────────────────────── */

const ALL_APPS: AppId[] = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "opencode",
  "openclaw",
  "hermes",
];

const APP_ICON_NAME: Record<AppId, string> = {
  claude: "claude",
  "claude-desktop": "claude",
  codex: "openai",
  gemini: "gemini",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
};

const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

const APP_BADGE_ICON: Partial<
  Record<AppId, { icon: typeof Terminal; offsetY?: number }>
> = {
  claude: { icon: Terminal },
  "claude-desktop": { icon: Monitor, offsetY: 0.5 },
};

/* ── Types ────────────────────────────────────── */

import type { View } from "@/app/views";

interface SidebarProps {
  activeApp: AppId;
  onAppSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
  currentView: View;
  onViewChange: (view: View) => void;
  onOpenSettings: () => void;
  onOpenAddProvider: () => void;
  isProxyRunning?: boolean;
  isTakeoverActive?: boolean;
  onOpenUsage?: () => void;
  hasSkillsSupport: boolean;
  hasSessionSupport: boolean;
  hasUnmanagedSkills?: boolean;
  onOpenHermesWebUI?: () => void;
}

/* ── Navigation item type ─────────────────────── */

interface NavItem {
  id: View;
  labelKey: string;
  defaultLabel: string;
  icon: React.ComponentType<{ className?: string }>;
  iconSize?: number;
}

/* ── Sidebar Component ────────────────────────── */

export const Sidebar = memo(function Sidebar({
  activeApp,
  onAppSwitch,
  visibleApps,
  currentView,
  onViewChange,
  onOpenSettings,
  onOpenAddProvider,
  isProxyRunning = false,
  isTakeoverActive = false,
  onOpenUsage,
  hasSkillsSupport,
  hasSessionSupport,
  hasUnmanagedSkills = false,
  onOpenHermesWebUI,
}: SidebarProps) {
  const { t } = useTranslation();

  const appsToShow = useMemo(
    () => ALL_APPS.filter((app) => !visibleApps || visibleApps[app]),
    [visibleApps],
  );

  // Navigation items based on active app
  const navItems = useMemo((): NavItem[] => {
    if (activeApp === "openclaw") {
      return [
        {
          id: "workspace",
          labelKey: "workspace.title",
          defaultLabel: "Workspace",
          icon: FolderOpen,
        },
        {
          id: "openclawEnv",
          labelKey: "openclaw.env.title",
          defaultLabel: "Environment",
          icon: KeyRound,
        },
        {
          id: "openclawTools",
          labelKey: "openclaw.tools.title",
          defaultLabel: "Tools",
          icon: Shield,
        },
        {
          id: "openclawAgents",
          labelKey: "openclaw.agents.title",
          defaultLabel: "Agents",
          icon: Cpu,
        },
        {
          id: "sessions",
          labelKey: "sessionManager.title",
          defaultLabel: "Sessions",
          icon: History,
        },
      ];
    }

    if (activeApp === "hermes") {
      return [
        {
          id: "skills",
          labelKey: "skills.title",
          defaultLabel: "Skills",
          icon: Wrench,
        },
        {
          id: "hermesMemory",
          labelKey: "hermes.memory.title",
          defaultLabel: "Memory",
          icon: Brain,
        },
        {
          id: "mcp",
          labelKey: "mcp.title",
          defaultLabel: "MCP Servers",
          icon: McpIcon,
        },
      ];
    }

    // Default: Claude, Codex, Gemini, OpenCode
    const items: NavItem[] = [];

    if (hasSkillsSupport) {
      items.push({
        id: "skills",
        labelKey: "skills.title",
        defaultLabel: "Skills",
        icon: Wrench,
      });
    }

    items.push(
      {
        id: "prompts",
        labelKey: "prompts.title",
        defaultLabel: "Prompts",
        icon: Book,
      },
    );

    if (hasSessionSupport) {
      items.push({
        id: "sessions",
        labelKey: "sessionManager.title",
        defaultLabel: "Sessions",
        icon: History,
      });
    }

    items.push({
      id: "mcp",
      labelKey: "mcp.title",
      defaultLabel: "MCP Servers",
      icon: McpIcon,
    });

    return items;
  }, [activeApp, hasSkillsSupport, hasSessionSupport]);

  const handleAppSwitch = useCallback(
    (app: AppId) => {
      if (app === activeApp) return;
      onAppSwitch(app);
    },
    [activeApp, onAppSwitch],
  );

  const handleNavClick = useCallback(
    (view: View) => {
      if (view === currentView) return;
      onViewChange(view);
    },
    [currentView, onViewChange],
  );

  const isNavActive = (navId: View): boolean => {
    if (navId === currentView) return true;
    // Group related views
    if (navId === "skills" && currentView === "skillsDiscovery") return true;
    return false;
  };

  return (
    <aside
      className="app-sidebar flex flex-col h-full overflow-hidden select-none"
      style={{ width: "var(--sidebar-width)", minWidth: "var(--sidebar-width)" }}
    >
      {/* ── Drag Region & Branding ─────────── */}
      <div
        className="flex items-center gap-2 px-4 shrink-0"
        data-tauri-drag-region
        style={{
          ...DRAG_REGION_STYLE,
          height: 52,
        } as any}
      >
        <div
          className="flex items-center gap-2.5 no-drag"
          style={{ WebkitAppRegion: "no-drag" } as any}
        >
          {/* Signal mark: two routing nodes joined by a live wire */}
          <span
            className={cn(
              "relative flex h-7 w-7 items-center justify-center rounded-[0.5625rem] border shadow-[inset_0_1px_0_rgba(255,255,255,0.35),0_2px_6px_hsl(var(--shadow-tint)/0.12)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_2px_6px_hsl(var(--shadow-tint)/0.4)]",
              isProxyRunning && isTakeoverActive
                ? "border-emerald-500/40 bg-gradient-to-br from-emerald-500/20 to-emerald-500/5"
                : "border-primary/35 bg-gradient-to-br from-primary/20 to-primary/5",
            )}
            aria-hidden="true"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M2 3.5h3.2c2 0 2.6 7 4.6 7H13M2 10.5h3.2M9.8 3.5H13"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                className={
                  isProxyRunning && isTakeoverActive
                    ? "text-emerald-600 dark:text-emerald-400"
                    : "text-primary"
                }
              />
            </svg>
            {isProxyRunning && isTakeoverActive && (
              <span className="absolute -right-0.5 -top-0.5 h-2 w-2 rounded-full bg-emerald-500 shadow-[0_0_6px_rgba(16,185,129,0.8)]" />
            )}
          </span>
          <span className="flex flex-col leading-none">
            <span
              className={cn(
                "font-display text-[0.9375rem] font-bold tracking-[-0.02em]",
                isProxyRunning && isTakeoverActive
                  ? "text-emerald-600 dark:text-emerald-400"
                  : "text-foreground",
              )}
            >
              CC Switch
            </span>
            <span className="mt-0.5 font-mono text-[0.5625rem] font-medium uppercase tracking-[0.18em] text-muted-foreground/70">
              Signal Deck
            </span>
          </span>
        </div>
      </div>

      {/* ── Scrollable Navigation ──────────── */}
      <nav className="flex-1 overflow-y-auto px-3 pb-2 space-y-5">
        {/* Apps Section */}
        <div className="space-y-0.5">
          <div className="sidebar-section-label py-1">
            {t("sidebar.apps", { defaultValue: "Applications" })}
          </div>
          {appsToShow.map((app, index) => {
            const isActive = activeApp === app;
            const badgeConfig = APP_BADGE_ICON[app];
            const BadgeIcon = badgeConfig?.icon;

            return (
              <button
                key={app}
                type="button"
                onClick={() => handleAppSwitch(app)}
                className={cn(
                  "sidebar-item sidebar-reveal relative",
                  isActive && "active",
                )}
                style={{ animationDelay: `${40 + index * 28}ms` }}
              >
                <span className="relative inline-flex shrink-0">
                  <ProviderIcon
                    icon={APP_ICON_NAME[app]}
                    name={APP_DISPLAY_NAME[app]}
                    size={18}
                  />
                  {BadgeIcon && (
                    <span
                      className={cn(
                        "absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-[3px] border h-[10px] w-[10px]",
                        isActive
                          ? "bg-white/90 border-white/70 dark:bg-slate-900 dark:border-white/10"
                          : "bg-white/60 border-white/70 dark:bg-slate-900/60 dark:border-white/10",
                      )}
                      aria-hidden="true"
                    >
                      <BadgeIcon
                        className="h-[7px] w-[7px]"
                        strokeWidth={2.5}
                        style={
                          badgeConfig?.offsetY
                            ? { transform: `translateY(${badgeConfig.offsetY}px)` }
                            : undefined
                        }
                      />
                    </span>
                  )}
                </span>
                <span className="truncate">{APP_DISPLAY_NAME[app]}</span>
              </button>
            );
          })}
        </div>

        {/* Navigation Section */}
        <div className="space-y-0.5">
          <div className="sidebar-section-label py-1">
            {t("sidebar.tools", { defaultValue: "Tools" })}
          </div>
          {navItems.map((item, index) => {
            const active = isNavActive(item.id);
            const Icon = item.icon;

            return (
              <button
                key={item.id}
                type="button"
                onClick={() => handleNavClick(item.id)}
                className={cn(
                  "sidebar-item sidebar-reveal relative",
                  active && "active",
                )}
                style={{
                  animationDelay: `${140 + index * 28}ms`,
                }}
              >
                <Icon className="w-[18px] h-[18px] shrink-0" />
                <span className="truncate">
                  {t(item.labelKey, { defaultValue: item.defaultLabel })}
                </span>
                {item.id === "skills" && hasUnmanagedSkills && (
                  <span
                    className="ml-auto h-2 w-2 rounded-full bg-green-500 shrink-0"
                    aria-hidden="true"
                  />
                )}
              </button>
            );
          })}
        </div>

        {/* Universal Provider (always available) */}
        <div className="space-y-0.5">
          <div className="sidebar-section-label py-1">
            {t("sidebar.general", { defaultValue: "General" })}
          </div>
          <button
            type="button"
            onClick={() => handleNavClick("universal")}
            className={cn(
              "sidebar-item",
              isNavActive("universal") && "active",
            )}
          >
            <Layers className="w-[18px] h-[18px] shrink-0" />
            <span className="truncate">
              {t("universalProvider.title", { defaultValue: "Universal" })}
            </span>
          </button>
        </div>
      </nav>

      {/* ── Bottom Section ─────────────────── */}
      <div className="px-3 pb-3 space-y-0.5 shrink-0">
        {isTakeoverActive && onOpenUsage && (
          <button
            type="button"
            onClick={onOpenUsage}
            className="sidebar-item"
          >
            <BarChart2 className="w-[18px] h-[18px] shrink-0" />
            <span className="truncate">
              {t("usage.title", { defaultValue: "Usage" })}
            </span>
          </button>
        )}

        {activeApp === "hermes" && onOpenHermesWebUI && (
          <button
            type="button"
            onClick={onOpenHermesWebUI}
            className="sidebar-item"
          >
            <LayoutDashboard className="w-[18px] h-[18px] shrink-0" />
            <span className="truncate">
              {t("hermes.webui.open", { defaultValue: "Web UI" })}
            </span>
          </button>
        )}

        <div className="h-px bg-border/40 my-1.5" />

        <button
          type="button"
          onClick={onOpenSettings}
          className={cn(
            "sidebar-item",
            currentView === "settings" && "active",
          )}
        >
          <Settings className="w-[18px] h-[18px] shrink-0" />
          <span className="truncate">
            {t("common.settings", { defaultValue: "Settings" })}
          </span>
        </button>
      </div>

      {/* ── Add Provider FAB ───────────────── */}
      {currentView === "providers" && (
        <div className="px-3 pb-4 shrink-0">
          <button
            type="button"
            onClick={onOpenAddProvider}
            className={cn(
              "w-full flex items-center justify-center gap-2 h-9 rounded-[0.5625rem]",
              "bg-gradient-to-b from-primary to-primary/90 text-primary-foreground font-medium text-sm",
              "hover:brightness-105 active:scale-[0.98]",
              "transition-all duration-150",
              "shadow-[inset_0_1px_0_rgba(255,255,255,0.25),0_1px_2px_rgba(0,0,0,0.12),0_3px_10px_hsl(var(--primary)/0.3)]",
              "hover:shadow-[inset_0_1px_0_rgba(255,255,255,0.25),0_1px_2px_rgba(0,0,0,0.12),0_4px_14px_hsl(var(--primary)/0.45)]",
            )}
          >
            <Plus className="w-4 h-4" />
            <span>
              {t("provider.add", { defaultValue: "Add Provider" })}
            </span>
          </button>
        </div>
      )}
    </aside>
  );
});

Sidebar.displayName = "Sidebar";
