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
  };
  const appDisplayName: Record<AppId, string> = {
    claude: "Claude",
    codex: "Codex",
    gemini: "Gemini",
  };

  return (
    <div className="inline-flex bg-gray-100 dark:bg-gray-800 rounded-lg p-1 gap-1">
      <button
        type="button"
        onClick={() => handleSwitch("claude")}
        className={`group inline-flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "claude"
            ? "bg-white text-gray-900 shadow-sm dark:bg-gray-900 dark:text-gray-100"
            : "text-gray-500 hover:text-gray-900 hover:bg-white/50 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800/60"
        }`}
      >
        <ProviderIcon
          icon={appIconName.claude}
          name={appDisplayName.claude}
          size={iconSize}
          className={
            activeApp === "claude"
              ? "text-foreground"
              : "text-gray-500 dark:text-gray-400 group-hover:text-foreground transition-colors"
          }
        />
        <span>{appDisplayName.claude}</span>
      </button>

      <button
        type="button"
        onClick={() => handleSwitch("codex")}
        className={`inline-flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "codex"
            ? "bg-white text-gray-900 shadow-sm dark:bg-gray-900 dark:text-gray-100"
            : "text-gray-500 hover:text-gray-900 hover:bg-white/50 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800/60"
        }`}
      >
        <ProviderIcon
          icon={appIconName.codex}
          name={appDisplayName.codex}
          size={iconSize}
          className={
            activeApp === "codex"
              ? "text-foreground"
              : "text-gray-500 dark:text-gray-400 group-hover:text-foreground transition-colors"
          }
        />
        <span>{appDisplayName.codex}</span>
      </button>

      <button
        type="button"
        onClick={() => handleSwitch("gemini")}
        className={`inline-flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "gemini"
            ? "bg-white text-gray-900 shadow-sm dark:bg-gray-900 dark:text-gray-100"
            : "text-gray-500 hover:text-gray-900 hover:bg-white/50 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800/60"
        }`}
      >
        <ProviderIcon
          icon={appIconName.gemini}
          name={appDisplayName.gemini}
          size={iconSize}
          className={
            activeApp === "gemini"
              ? "text-foreground"
              : "text-gray-500 dark:text-gray-400 group-hover:text-foreground transition-colors"
          }
        />
        <span>{appDisplayName.gemini}</span>
      </button>
    </div>
  );
}
