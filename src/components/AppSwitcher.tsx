import type { AppId } from "@/lib/api";
import { ProviderIcon } from "@/components/ProviderIcon";

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
}

export function AppSwitcher({ activeApp, onSwitch }: AppSwitcherProps) {
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

  const apps: AppId[] = ["claude", "codex", "gemini", "opencode"];

  return (
    <div className="inline-flex items-center gap-1 rounded-full bg-gray-100/80 dark:bg-gray-800/50 p-1">
      {apps.map((app) => (
        <button
          key={app}
          type="button"
          onClick={() => handleSwitch(app)}
          className={`group inline-flex items-center gap-1.5 px-3 h-7 rounded-full text-sm transition-all duration-200 ${
            activeApp === app
              ? "bg-white dark:bg-gray-700 text-foreground font-medium shadow-sm"
              : "text-gray-500 dark:text-gray-400 hover:text-foreground"
          }`}
        >
          <ProviderIcon
            icon={appIconName[app]}
            name={appDisplayName[app]}
            size={iconSize}
            className={
              activeApp === app
                ? "text-foreground"
                : "text-gray-400 dark:text-gray-500 group-hover:text-foreground transition-colors"
            }
          />
          <span>{appDisplayName[app]}</span>
        </button>
      ))}
    </div>
  );
}
