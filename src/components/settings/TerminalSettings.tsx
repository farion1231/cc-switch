import { useTranslation } from "react-i18next";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isMac, isWindows, isLinux } from "@/lib/platform";

// Terminal options per platform
const MACOS_TERMINALS = [
  { value: "terminal", labelKey: "settings.terminal.options.macos.terminal" },
  { value: "iterm2", labelKey: "settings.terminal.options.macos.iterm2" },
  { value: "alacritty", labelKey: "settings.terminal.options.macos.alacritty" },
  { value: "kitty", labelKey: "settings.terminal.options.macos.kitty" },
  { value: "ghostty", labelKey: "settings.terminal.options.macos.ghostty" },
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
