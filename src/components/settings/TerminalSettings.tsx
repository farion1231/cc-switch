import { useTranslation } from "react-i18next";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { RuntimeOs } from "@/lib/api";

// Terminal options per platform
const MACOS_TERMINALS = [
  { value: "terminal", labelKey: "settings.terminal.options.macos.terminal" },
  { value: "iterm2", labelKey: "settings.terminal.options.macos.iterm2" },
  { value: "alacritty", labelKey: "settings.terminal.options.macos.alacritty" },
  { value: "kitty", labelKey: "settings.terminal.options.macos.kitty" },
  { value: "ghostty", labelKey: "settings.terminal.options.macos.ghostty" },
  { value: "wezterm", labelKey: "settings.terminal.options.macos.wezterm" },
  { value: "kaku", labelKey: "settings.terminal.options.macos.kaku" },
  { value: "warp", labelKey: "settings.terminal.options.macos.warp" },
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
function getTerminalOptions(serverOs: RuntimeOs) {
  if (serverOs === "macos") {
    return MACOS_TERMINALS;
  }
  if (serverOs === "windows") {
    return WINDOWS_TERMINALS;
  }
  if (serverOs === "linux") {
    return LINUX_TERMINALS;
  }
  // Fallback to macOS options
  return MACOS_TERMINALS;
}

// Get default terminal for the current platform
function getDefaultTerminal(serverOs: RuntimeOs): string {
  if (serverOs === "macos") {
    return "terminal";
  }
  if (serverOs === "windows") {
    return "cmd";
  }
  if (serverOs === "linux") {
    return "gnome-terminal";
  }
  return "terminal";
}

export interface TerminalSettingsProps {
  value?: string;
  onChange: (value: string) => void;
  serverOs: RuntimeOs;
}

export function TerminalSettings({
  value,
  onChange,
  serverOs,
}: TerminalSettingsProps) {
  const { t } = useTranslation();
  const terminals = getTerminalOptions(serverOs);
  const defaultTerminal = getDefaultTerminal(serverOs);

  // Use value or default
  const currentValue = value || defaultTerminal;

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
    </section>
  );
}
