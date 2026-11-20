import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, Download, ExternalLink, Loader2, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { settingsApi } from "@/lib/api";
import type { InstallMethod } from "@/lib/api/settings";
import { copyText } from "@/lib/clipboard";
import { isLinux, isWindows } from "@/lib/platform";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  getInstallChannels,
  OFFICIAL_INSTALL_PAGES,
  type InstallApp,
} from "./installCommands";
import {
  TOOL_APP_IDS,
  TOOL_NAMES,
  toolDisplayName,
  type ToolVersion,
} from "./useToolManagement";

const PLATFORM_LABELS = { macos: "macOS", windows: "Windows", linux: "Linux" };

const installAppId = (app: InstallApp) =>
  app === "workbuddy" || app === "claude-desktop" ? app : TOOL_APP_IDS[app];
const installAppName = (app: InstallApp) =>
  app === "workbuddy" || app === "claude-desktop"
    ? APP_DISPLAY_NAME[app]
    : toolDisplayName(app);
const INSTALL_APPS: InstallApp[] = [
  ...TOOL_NAMES,
  "workbuddy",
  "claude-desktop",
];
INSTALL_APPS.sort((left, right) =>
  installAppName(left).localeCompare(installAppName(right), "en"),
);

export function InstallCommandsDialog({
  initialTool,
  onClose,
  toolVersionByName,
  onInstall,
  isInstallBusy,
  isInstallQueued,
}: {
  initialTool: InstallApp;
  onClose: () => void;
  toolVersionByName: ReadonlyMap<string, ToolVersion>;
  onInstall: (app: InstallApp, method: InstallMethod) => Promise<void>;
  isInstallBusy: (app: InstallApp) => boolean;
  isInstallQueued: (app: InstallApp) => boolean;
}) {
  const { t } = useTranslation();
  const [tool, setTool] = useState(initialTool);
  const [installation, setInstallation] = useState<{
    app: InstallApp;
    method: InstallMethod;
  } | null>(null);
  const install = async (method: InstallMethod) => {
    setInstallation({ app: tool, method });
    try {
      await onInstall(tool, method);
    } finally {
      setInstallation(null);
    }
  };
  const [search, setSearch] = useState("");
  const [manualEnvironment, setManualEnvironment] = useState<
    "windows" | "wsl" | null
  >(null);
  const query = search.trim().toLowerCase();
  const visibleTools = INSTALL_APPS.filter((name) =>
    installAppName(name).toLowerCase().includes(query),
  );
  const environment = toolVersionByName.get(tool);
  const canSelectEnvironment =
    isWindows() && tool !== "workbuddy" && tool !== "claude-desktop";
  const waitingForEnvironment =
    canSelectEnvironment && !environment && !manualEnvironment;
  const isWsl = manualEnvironment
    ? manualEnvironment === "wsl"
    : environment?.env_type === "wsl";
  const platform =
    isWsl || (!isWindows() && isLinux())
      ? "linux"
      : isWindows()
        ? "windows"
        : "macos";
  const platformLabel = isWsl
    ? environment?.wsl_distro
      ? `WSL · ${environment.wsl_distro}`
      : "WSL"
    : PLATFORM_LABELS[platform];

  const selectedEnvironment = isWsl ? "wsl" : platform;
  const manualUpdate =
    environment?.manual_update_only &&
    environment.env_type === selectedEnvironment;
  const channels = manualUpdate ? [] : getInstallChannels(tool, platform);
  const pages =
    tool === "claude-desktop" && platform === "linux"
      ? "https://code.claude.com/docs/en/desktop-linux#install"
      : OFFICIAL_INSTALL_PAGES[tool];
  const wslSetupRequired =
    canSelectEnvironment && isWsl && environment?.env_type !== "wsl";
  const downloadLinks =
    typeof pages === "string"
      ? [{ url: pages, label: t("appsPage.installDownload") }]
      : [
          { url: pages.china, label: t("appsPage.installDownloadChina") },
          { url: pages.global, label: t("appsPage.installDownloadGlobal") },
        ];

  const copyCommand = async (command: string) => {
    try {
      await copyText(command);
      toast.success(t("settings.installCommandsCopied"));
    } catch {
      toast.error(t("settings.installCommandsCopyFailed"));
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("appsPage.manualCommands")}</DialogTitle>
          <DialogDescription>
            {t("settings.oneClickInstallHint")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid min-h-0 gap-4 px-6 sm:grid-cols-[12rem_minmax(0,1fr)]">
          <div className="min-w-0 overflow-hidden rounded-panel border border-border">
            <label className="flex items-center gap-2 border-b border-border px-3 py-2 text-fg-2 focus-within:bg-subtle focus-within:text-fg">
              <Search className="size-4 shrink-0" aria-hidden="true" />
              <input
                className="w-full min-w-0 bg-transparent text-body placeholder:text-fg-3 focus-visible:outline-none"
                type="search"
                aria-label={t("appsPage.installCommandApp")}
                placeholder={t("appsPage.installCommandApp")}
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
            <ul
              aria-label={t("appsPage.manualCommands")}
              className="max-h-28 space-y-1 overflow-y-auto p-1.5 sm:max-h-[48vh]"
            >
              {visibleTools.map((name) => (
                <li key={name}>
                  <button
                    type="button"
                    aria-pressed={name === tool}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-control px-2 py-2 text-left text-body outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                      name === tool
                        ? "bg-accent font-medium text-fg"
                        : "text-fg-2 hover:bg-subtle hover:text-fg",
                    )}
                    onClick={() => {
                      if (tool !== name) setManualEnvironment(null);
                      setTool(name);
                    }}
                  >
                    <AppGlyph app={installAppId(name)} size={16} />
                    <span className="min-w-0">{installAppName(name)}</span>
                  </button>
                </li>
              ))}
              {visibleTools.length === 0 && (
                <li role="status" className="p-2 text-caption text-fg-2">
                  {t("appsPage.installAppNotFound")}
                </li>
              )}
            </ul>
          </div>
          <section aria-labelledby="install-selected-app" className="min-w-0">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
              <div className="min-w-0">
                <h3
                  id="install-selected-app"
                  className="text-body font-semibold"
                >
                  {installAppName(tool)}
                </h3>
                {!waitingForEnvironment && (
                  <p className="text-caption text-fg-2">{platformLabel}</p>
                )}
              </div>
              <div className="flex flex-wrap gap-2">
                {downloadLinks.map(({ url, label }) => (
                  <Button
                    key={url}
                    variant="neutral"
                    size="compact"
                    onClick={() =>
                      void settingsApi
                        .openExternal(url)
                        .catch(() =>
                          toast.error(t("settings.installDocsOpenFailed")),
                        )
                    }
                  >
                    <ExternalLink className="size-3.5" />
                    {label}
                  </Button>
                ))}
              </div>
            </div>
            {canSelectEnvironment && (
              <div className="mb-3 flex flex-wrap gap-2">
                {(["windows", "wsl"] as const).map((target) => {
                  const selected =
                    !waitingForEnvironment &&
                    (isWsl ? "wsl" : "windows") === target;
                  return (
                    <Button
                      key={target}
                      variant={selected ? "solid" : "neutral"}
                      size="compact"
                      aria-pressed={selected}
                      onClick={() => setManualEnvironment(target)}
                    >
                      {target === "windows" ? "Windows" : "WSL"}
                    </Button>
                  );
                })}
              </div>
            )}
            {wslSetupRequired && (
              <p className="mb-3 text-caption text-fg-2">
                {t("appsPage.installWslDetectionHint")}
              </p>
            )}
            {waitingForEnvironment ? (
              <p
                role="status"
                className="rounded-panel border border-border bg-subtle p-3 text-body text-fg-2"
              >
                {t("appsPage.installEnvironmentPending")}
              </p>
            ) : channels.length ? (
              <ul className="max-h-[30vh] divide-y divide-border overflow-y-auto rounded-panel border border-border sm:max-h-[42vh]">
                {channels.map((channel) => {
                  const method = channel.method;
                  const installing =
                    installation?.app === tool &&
                    installation.method === method;
                  return (
                    <li
                      key={channel.name}
                      className="space-y-2.5 px-3 py-3"
                      aria-busy={installing}
                    >
                      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
                        <span className="text-body font-medium">
                          {channel.name}
                        </span>
                        <div className="ml-auto flex items-center gap-1.5">
                          <Button
                            variant="neutral"
                            size="compact"
                            aria-label={`${t("common.copy")} ${channel.name}`}
                            onClick={() => void copyCommand(channel.command)}
                          >
                            <Copy className="size-3.5" />
                            {t("common.copy")}
                          </Button>
                          {method && !isWindows() && (
                            <Button
                              variant="solid"
                              size="compact"
                              disabled={
                                Boolean(installation) || isInstallBusy(tool)
                              }
                              aria-label={`${t("settings.toolInstall")} ${channel.name}`}
                              onClick={() => void install(method)}
                            >
                              {installing ? (
                                <Loader2 className="size-3.5 motion-safe:animate-spin" />
                              ) : (
                                <Download className="size-3.5" />
                              )}
                              {t("settings.toolInstall")}
                            </Button>
                          )}
                        </div>
                      </div>
                      <pre className="select-text whitespace-pre-wrap break-words rounded-control bg-subtle px-3 py-2 font-mono text-caption">
                        {channel.command}
                      </pre>
                      {installing && isInstallQueued(tool) && (
                        <p role="status" className="text-caption text-fg-2">
                          {t("appsPage.toolQueued")}
                        </p>
                      )}
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="rounded-panel border border-border bg-subtle p-3 text-body text-fg-2">
                {t(
                  manualUpdate
                    ? "appsPage.installManualUpdate"
                    : "appsPage.installDownloadRequired",
                )}
              </p>
            )}
          </section>
        </div>
        <DialogFooter>
          <Button variant="neutral" size="regular" onClick={onClose}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
