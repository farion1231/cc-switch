import { AppType } from "../lib/query";
import { ClaudeIcon, CodexIcon } from "./BrandIcons";
import { Button } from "@/components/ui/button";

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
    <div className="inline-flex bg-muted rounded-lg p-1 gap-1 border border-transparent">
      <Button
        type="button"
        variant={activeApp === "claude" ? "default" : "ghost"}
        size="sm"
        onClick={() => handleSwitch("claude")}
        className="group"
      >
        <ClaudeIcon
          size={16}
          className={
            activeApp === "claude"
              ? "text-[#D97757] dark:text-[#D97757] transition-colors duration-200"
              : "text-muted-foreground group-hover:text-[#D97757] dark:group-hover:text-[#D97757] transition-colors duration-200"
          }
        />
        <span>Claude</span>
      </Button>

      <Button
        type="button"
        variant={activeApp === "codex" ? "default" : "ghost"}
        size="sm"
        onClick={() => handleSwitch("codex")}
        className="group"
      >
        <CodexIcon size={16} />
        <span>Codex</span>
      </Button>
    </div>
  );
}
