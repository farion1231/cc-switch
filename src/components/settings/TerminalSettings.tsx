import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { isMac, isWindows, isLinux } from "@/lib/platform";
import { settingsApi } from "@/lib/api/settings";

// Terminal options per platform
const MACOS_TERMINALS = [
  { value: "terminal", labelKey: "settings.terminal.options.macos.terminal" },
  { value: "iterm2", labelKey: "settings.terminal.options.macos.iterm2" },
  { value: "alacritty", labelKey: "settings.terminal.options.macos.alacritty" },
  { value: "kitty", labelKey: "settings.terminal.options.macos.kitty" },
  { value: "ghostty", labelKey: "settings.terminal.options.macos.ghostty" },
  { value: "wezterm", labelKey: "settings.terminal.options.macos.wezterm" },
  { value: "cmux", labelKey: "settings.terminal.options.macos.cmux" },
] as const;

const WINDOWS_TERMINALS = [
  { value: "cmd", labelKey: "settings.terminal.options.windows.cmd" },
  {
    value: "powershell",
    labelKey: "settings.terminal.options.windows.powershell",
  },
  { value: "wt", labelKey: "settings.terminal.options.windows.wt" },
] as const;

const LINUX_TERMINALS = [
  {
    value: "gnome-terminal",
    labelKey: "settings.terminal.options.linux.gnomeTerminal",
  },
  { value: "konsole", labelKey: "settings.terminal.options.linux.konsole" },
  {
    value: "xfce4-terminal",
    labelKey: "settings.terminal.options.linux.xfce4Terminal",
  },
  { value: "alacritty", labelKey: "settings.terminal.options.linux.alacritty" },
  { value: "kitty", labelKey: "settings.terminal.options.linux.kitty" },
  { value: "ghostty", labelKey: "settings.terminal.options.linux.ghostty" },
] as const;

// Get terminals for the current platform
function getTerminalOptions() {
  if (isMac()) {
    return MACOS_TERMINALS;
  }
  if (isWindows()) {
    return WINDOWS_TERMINALS;
  }
  if (isLinux()) {
    return LINUX_TERMINALS;
  }
  // Fallback to macOS options
  return MACOS_TERMINALS;
}

// Get default terminal for the current platform
function getDefaultTerminal(): string {
  if (isMac()) {
    return "terminal";
  }
  if (isWindows()) {
    return "cmd";
  }
  if (isLinux()) {
    return "gnome-terminal";
  }
  return "terminal";
}

export interface TerminalSettingsProps {
  value?: string;
  onChange: (value: string) => void;
}

export function TerminalSettings({ value, onChange }: TerminalSettingsProps) {
  const { t } = useTranslation();
  const terminals = getTerminalOptions();
  const defaultTerminal = getDefaultTerminal();
  const [cmuxRestarting, setCmuxRestarting] = useState(false);

  // Use value or default
  const currentValue = value || defaultTerminal;

  const handleCmuxRestart = async () => {
    setCmuxRestarting(true);
    try {
      await settingsApi.restartCmuxForExternalAccess();
      toast.success(t("settings.terminal.cmuxRestartSuccess"));
    } catch (e) {
      toast.error(
        t("settings.terminal.cmuxRestartFailed", {
          message: e instanceof Error ? e.message : String(e),
        }),
      );
    } finally {
      setCmuxRestarting(false);
    }
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.terminal.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.terminal.description")}
        </p>
      </header>
      <Select value={currentValue} onValueChange={onChange}>
        <SelectTrigger className="w-[200px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {terminals.map((terminal) => (
            <SelectItem key={terminal.value} value={terminal.value}>
              {t(terminal.labelKey)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p className="text-xs text-muted-foreground">
        {t("settings.terminal.fallbackHint")}
      </p>
      {isMac() && currentValue === "cmux" ? (
        <div className="space-y-2 rounded-md border border-border/60 bg-muted/30 p-3">
          <p className="text-xs text-muted-foreground">
            {t("settings.terminal.cmuxSocketHint")}
          </p>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={cmuxRestarting}
            onClick={() => void handleCmuxRestart()}
          >
            {cmuxRestarting
              ? t("settings.terminal.cmuxRestarting")
              : t("settings.terminal.cmuxRestartButton")}
          </Button>
        </div>
      ) : null}
    </section>
  );
}
