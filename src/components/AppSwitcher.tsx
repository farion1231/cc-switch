import { useState } from "react";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { ProviderIcon } from "@/components/ProviderIcon";
import { isWindows, isLinux } from "@/lib/platform";
import { cn } from "@/lib/utils";

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
}

// 获取快捷键修饰符显示文本
const getModifierKey = () => (isWindows() || isLinux() ? "Ctrl" : "⌘");

export const APP_LIST: AppId[] = ["claude", "codex", "gemini", "opencode"];
const STORAGE_KEY = "cc-switch-last-app";

const appIconName: Record<AppId, string> = {
  claude: "claude",
  codex: "openai",
  gemini: "gemini",
  opencode: "opencode",
};

const appDisplayName: Record<AppId, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  opencode: "OpenCode",
};

export function AppSwitcher({ activeApp, onSwitch, visibleApps }: AppSwitcherProps) {
  const [hoveredApp, setHoveredApp] = useState<AppId | null>(null);

  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    localStorage.setItem(STORAGE_KEY, app);
    onSwitch(app);
  };

  // Filter apps based on visibility settings (default all visible)
  const appsToShow = APP_LIST.filter((app) => {
    if (!visibleApps) return true;
    return visibleApps[app];
  });

  return (
    <div className="inline-flex items-center gap-0.5 rounded-xl bg-muted/60 p-1">
      {appsToShow.map((app) => {
        // Get original index for keyboard shortcut display
        const originalIndex = APP_LIST.indexOf(app);
        return (
          <div key={app} className="relative">
            <button
              type="button"
              onClick={() => handleSwitch(app)}
              onMouseEnter={() => setHoveredApp(app)}
              onMouseLeave={() => setHoveredApp(null)}
              className={cn(
                "inline-flex items-center justify-center w-8 h-8 rounded-lg transition-all duration-200",
                activeApp === app
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground hover:bg-background/50",
              )}
              title={appDisplayName[app]}
            >
              <ProviderIcon
                icon={appIconName[app]}
                name={appDisplayName[app]}
                size={20}
                className={cn(
                  "transition-colors",
                  activeApp === app
                    ? "text-foreground"
                    : "text-muted-foreground group-hover:text-foreground",
                )}
              />
            </button>
            {/* 快捷键提示 tooltip */}
            {hoveredApp === app && (
              <div className="absolute left-1/2 -translate-x-1/2 -bottom-6 px-1.5 py-0.5 text-[10px] text-muted-foreground bg-popover border border-border rounded shadow-sm whitespace-nowrap z-50 pointer-events-none">
                {getModifierKey()}+{originalIndex + 1}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
