import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { ProviderIcon } from "@/components/ProviderIcon";

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
}

const ALL_APPS: AppId[] = ["claude", "codex", "gemini", "opencode"];

export function AppSwitcher({ activeApp, onSwitch, visibleApps }: AppSwitcherProps) {
  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    onSwitch(app);
  };
  const iconSize = 20;
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

  // Filter apps based on visibility settings (default all visible)
  const appsToShow = ALL_APPS.filter((app) => {
    if (!visibleApps) return true;
    return visibleApps[app];
  });

  return (
    <div className="inline-flex bg-muted rounded-xl p-1 gap-1">
      {appsToShow.map((app) => (
        <button
          key={app}
          type="button"
          onClick={() => handleSwitch(app)}
          className={`group inline-flex items-center gap-2 px-3 h-8 rounded-md text-sm font-medium transition-all duration-200 ${
            activeApp === app
              ? "bg-background text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground hover:bg-background/50"
          }`}
        >
          <ProviderIcon
            icon={appIconName[app]}
            name={appDisplayName[app]}
            size={iconSize}
            className={
              activeApp === app
                ? "text-foreground"
                : "text-muted-foreground group-hover:text-foreground transition-colors"
            }
          />
          <span>{appDisplayName[app]}</span>
        </button>
      ))}
    </div>
  );
}
