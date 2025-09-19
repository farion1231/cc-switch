import { AppType } from "../lib/tauri-api";
import { ClaudeIcon, CodexIcon } from "./BrandIcons";

interface AppSwitcherProps {
  activeApp: AppType;
  onSwitch: (app: AppType) => void;
}

export function AppSwitcher({ activeApp, onSwitch }: AppSwitcherProps) {
  const handleSwitch = (app: AppType) => {
    if (app === activeApp) return;
    onSwitch(app);
  };

  return (
    <div className="inline-flex bg-gray-100 dark:bg-gray-800 rounded-lg p-1 gap-1 border border-transparent dark:border-gray-700">
      <button
        type="button"
        onClick={() => handleSwitch("claude")}
        className={`group inline-flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "claude"
            ? "bg-white text-gray-900 shadow-sm dark:bg-gray-900 dark:text-gray-100 dark:shadow-none"
            : "text-gray-500 hover:text-gray-900 hover:bg-white/50 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800/60"
        }`}
      >
        <ClaudeIcon
          size={16}
          className={
            activeApp === "claude"
              ? "text-[#D97757] dark:text-[#D97757] transition-colors duration-200"
              : "text-gray-500 dark:text-gray-400 group-hover:text-[#D97757] dark:group-hover:text-[#D97757] transition-colors duration-200"
          }
        />
        <span>Claude</span>
      </button>

      <button
        type="button"
        onClick={() => handleSwitch("codex")}
        className={`inline-flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "codex"
            ? "bg-white text-gray-900 shadow-sm dark:bg-gray-900 dark:text-gray-100 dark:shadow-none"
            : "text-gray-500 hover:text-gray-900 hover:bg-white/50 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800/60"
        }`}
      >
        <CodexIcon size={16} />
        <span>Codex</span>
      </button>
    </div>
  );
}
