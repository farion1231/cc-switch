import { AppType } from "../../lib/tauri-api";
import "./index.css";

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
    <div className="switcher-pills">
      <button
        type="button"
        className={`switcher-pill ${activeApp === "claude" ? "active" : ""}`}
        onClick={() => handleSwitch("claude")}
      >
        <span className="pill-dot" />
        <span>Claude Code</span>
      </button>
      <div className="pills-divider" />
      <button
        type="button"
        className={`switcher-pill ${activeApp === "codex" ? "active" : ""}`}
        onClick={() => handleSwitch("codex")}
      >
        <span className="pill-dot" />
        <span>Codex</span>
      </button>
    </div>
  );
}
