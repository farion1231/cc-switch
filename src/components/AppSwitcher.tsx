import { useState } from "react";
import type { AppId } from "@/lib/api";
import { ProviderIcon } from "@/components/ProviderIcon";
import { isWindows, isLinux } from "@/lib/platform";
import { cn } from "@/lib/utils";

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
}

// 获取快捷键修饰符显示文本
const getModifierKey = () => (isWindows() || isLinux() ? "Ctrl" : "⌘");

export const APP_LIST: AppId[] = ["claude", "codex", "gemini", "opencode"];

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

export function AppSwitcher({ activeApp, onSwitch }: AppSwitcherProps) {
  const [hoveredApp, setHoveredApp] = useState<AppId | null>(null);

  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    onSwitch(app);
  };

  return (
    <div className="inline-flex items-center gap-0.5 rounded-xl bg-muted/60 p-1">
      {APP_LIST.map((app, index) => (
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
              {getModifierKey()}+{index + 1}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
