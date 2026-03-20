import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isMac, isWindows, isLinux } from "@/lib/platform";
import { terminalApi, type WtProfile } from "@/lib/api/terminal";

// Terminal options per platform
const MACOS_TERMINALS = [
  { value: "terminal", labelKey: "settings.terminal.options.macos.terminal" },
  { value: "iterm2", labelKey: "settings.terminal.options.macos.iterm2" },
  { value: "alacritty", labelKey: "settings.terminal.options.macos.alacritty" },
  { value: "kitty", labelKey: "settings.terminal.options.macos.kitty" },
  { value: "ghostty", labelKey: "settings.terminal.options.macos.ghostty" },
  { value: "wezterm", labelKey: "settings.terminal.options.macos.wezterm" },
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
  profileValue?: string;
  onProfileChange?: (value: string) => void;
}

export function TerminalSettings({
  value,
  onChange,
  profileValue,
  onProfileChange,
}: TerminalSettingsProps) {
  const { t } = useTranslation();
  const terminals = getTerminalOptions();
  const defaultTerminal = getDefaultTerminal();

  // Use value or default
  const currentValue = value || defaultTerminal;

  // WT Profiles 状态
  const [wtProfiles, setWtProfiles] = useState<WtProfile[]>([]);
  const [isLoadingProfiles, setIsLoadingProfiles] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);
  const hasLoadedProfiles = useRef(false);

  // 当选择 wt 时，加载 profiles（仅首次）
  useEffect(() => {
    if (
      isWindows() &&
      currentValue === "wt" &&
      onProfileChange &&
      !hasLoadedProfiles.current
    ) {
      hasLoadedProfiles.current = true;
      setIsLoadingProfiles(true);
      setProfileError(null);
      terminalApi
        .getWtProfiles()
        .then((profiles) => {
          setWtProfiles(profiles);
          // 如果没有已选中的 profile，自动选择第一项
          if (!profileValue && profiles.length > 0) {
            onProfileChange(profiles[0].guid);
          }
        })
        .catch(() => setProfileError(t("settings.terminal.options.windows.profileError")))
        .finally(() => setIsLoadingProfiles(false));
    }
  }, [currentValue, onProfileChange, t, profileValue]);

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.terminal.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.terminal.description")}
        </p>
      </header>

      <div className="flex items-center gap-3">
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

        {/* WT Profile 选择器 - 仅当选择 wt 且在 Windows 上时显示 */}
        {isWindows() && currentValue === "wt" && onProfileChange && (
          <>
            {isLoadingProfiles ? (
              <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
            ) : profileError ? (
              <span className="text-xs text-destructive">{profileError}</span>
            ) : wtProfiles.length === 0 ? (
              <span className="text-xs text-muted-foreground">
                {t("settings.terminal.options.windows.noProfiles")}
              </span>
            ) : (
              <Select
                value={profileValue || wtProfiles[0]?.guid || ""}
                onValueChange={onProfileChange}
              >
                <SelectTrigger className="w-[200px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {wtProfiles.map((profile) => (
                    <SelectItem key={profile.guid} value={profile.guid}>
                      {profile.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </>
        )}
      </div>

      <p className="text-xs text-muted-foreground">
        {t("settings.terminal.fallbackHint")}
      </p>
    </section>
  );
}
