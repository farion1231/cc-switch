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
    <div className="inline-flex bg-muted rounded-xl p-1 gap-1">
      <button
        type="button"
        onClick={() => handleSwitch("claude")}
        className={`group inline-flex items-center gap-2 px-3 h-8 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "claude"
            ? "bg-background text-foreground shadow-sm"
            : "text-muted-foreground hover:text-foreground hover:bg-background/50"
        }`}
      >
        <ProviderIcon
          icon={appIconName.claude}
          name={appDisplayName.claude}
          size={iconSize}
          className={
            activeApp === "claude"
              ? "text-foreground"
              : "text-muted-foreground group-hover:text-foreground transition-colors"
          }
        />
        <span>{appDisplayName.claude}</span>
      </button>

      <button
        type="button"
        onClick={() => handleSwitch("codex")}
        className={`group inline-flex items-center gap-2 px-3 h-8 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "codex"
            ? "bg-background text-foreground shadow-sm"
            : "text-muted-foreground hover:text-foreground hover:bg-background/50"
        }`}
      >
        <ProviderIcon
          icon={appIconName.codex}
          name={appDisplayName.codex}
          size={iconSize}
          className={
            activeApp === "codex"
              ? "text-foreground"
              : "text-muted-foreground group-hover:text-foreground transition-colors"
          }
        />
        <span>{appDisplayName.codex}</span>
      </button>

      <button
        type="button"
        onClick={() => handleSwitch("gemini")}
        className={`group inline-flex items-center gap-2 px-3 h-8 rounded-md text-sm font-medium transition-all duration-200 ${
          activeApp === "gemini"
            ? "bg-background text-foreground shadow-sm"
            : "text-muted-foreground hover:text-foreground hover:bg-background/50"
        }`}
      >
        <ProviderIcon
          icon={appIconName.gemini}
          name={appDisplayName.gemini}
          size={iconSize}
          className={
            activeApp === "gemini"
              ? "text-foreground"
              : "text-muted-foreground group-hover:text-foreground transition-colors"
          }
        />
        <span>{appDisplayName.gemini}</span>
      </button>
    </div>
  );
}
