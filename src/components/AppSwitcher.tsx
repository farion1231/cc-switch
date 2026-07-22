import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";
import { Monitor, Terminal } from "lucide-react";

const APP_BADGE_ICON: Partial<
  Record<AppId, { icon: typeof Terminal; offsetY?: number }>
> = {
  claude: { icon: Terminal },
  "claude-desktop": { icon: Monitor, offsetY: 0.5 },
};

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
  compact?: boolean;
}

const ALL_APPS: AppId[] = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
  "pi",
];
const STORAGE_KEY = "cc-switch-last-app";

export function AppSwitcher({
  activeApp,
  onSwitch,
  visibleApps,
  compact,
}: AppSwitcherProps) {
  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    localStorage.setItem(STORAGE_KEY, app);
    onSwitch(app);
  };
  const iconSize = 20;
  const appIconName: Record<AppId, string> = {
    claude: "claude",
    "claude-desktop": "claude",
    codex: "openai",
    gemini: "gemini",
    grokbuild: "grok",
    opencode: "opencode",
    openclaw: "openclaw",
    hermes: "hermes",
    pi: "pi",
  };
  const appDisplayName: Record<AppId, string> = {
    claude: "Claude Code",
    "claude-desktop": "Claude Desktop",
    codex: "Codex",
    gemini: "Gemini",
    grokbuild: "Grok Build",
    opencode: "OpenCode",
    openclaw: "OpenClaw",
    hermes: "Hermes",
    pi: "Pi",
  };

  // Filter apps based on visibility settings (default all visible)
  const appsToShow = ALL_APPS.filter((app) => {
    if (!visibleApps) return true;
    return visibleApps[app];
  });

  return (
    <div className="inline-flex glass-pill rounded-xl p-1 gap-1">
      {appsToShow.map((app) => {
        const badgeConfig = APP_BADGE_ICON[app];
        const BadgeIcon = badgeConfig?.icon;
        const isActive = activeApp === app;
        return (
          <button
            key={app}
            type="button"
            onClick={() => handleSwitch(app)}
            className={cn(
              "group inline-flex items-center px-3 h-8 rounded-lg text-sm font-medium transition-all duration-200",
              isActive
                ? "bg-white/70 text-foreground shadow-sm backdrop-blur-md dark:bg-white/15"
                : "text-muted-foreground hover:text-foreground hover:bg-white/35 dark:hover:bg-white/10",
            )}
          >
            <span className="relative inline-flex shrink-0">
              <ProviderIcon
                icon={appIconName[app]}
                name={appDisplayName[app]}
                size={iconSize}
              />
              {BadgeIcon && (
                <span
                  className={cn(
                    "absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-[3px] border h-[11px] w-[11px]",
                    isActive
                      ? "bg-white/80 border-white/50 text-foreground dark:bg-white/15 dark:border-white/20"
                      : "bg-white/40 border-white/30 text-muted-foreground group-hover:bg-white/60 dark:bg-white/5 dark:border-white/10 dark:group-hover:bg-white/10 dark:group-hover:text-foreground",
                  )}
                  aria-hidden="true"
                >
                  <BadgeIcon
                    className="h-[8px] w-[8px]"
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
            <span
              className={cn(
                "transition-all duration-200 whitespace-nowrap overflow-hidden",
                compact
                  ? "max-w-0 opacity-0 ml-0"
                  : "max-w-[120px] opacity-100 ml-2",
              )}
            >
              {appDisplayName[app]}
            </span>
          </button>
        );
      })}
    </div>
  );
}
