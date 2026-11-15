import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { copyText } from "@/lib/clipboard";
import { isLinux, isMac, isWindows } from "@/lib/platform";
import { toast } from "@/lib/toast";
import {
  TOOL_NAMES,
  toolDisplayName,
  type ToolName,
  type ToolVersion,
} from "./useToolManagement";
import {
  UNINSTALL_METHODS,
  uninstallCommandsFor,
  type UninstallEnvironment,
  type UninstallMethod,
} from "./toolUninstallCommands";

export function ManualUninstallDialog({
  open,
  onOpenChange,
  toolVersionByName,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  toolVersionByName: ReadonlyMap<string, ToolVersion>;
}) {
  const { t } = useTranslation();
  const [method, setMethod] = useState<UninstallMethod>("npm");
  // Keep detection as an internal value so choosing the displayed environment
  // still registers as a manual override, even before detection finishes.
  const [environmentChoice, setEnvironmentChoice] = useState<
    UninstallEnvironment | "auto"
  >("auto");
  // Reuse the app page's per-tool detection, including the configured WSL distro.
  // Until a result is available, the host platform supplies the native fallback.
  const hostEnvironment: UninstallEnvironment | undefined = isWindows()
    ? "windows"
    : isMac()
      ? "macos"
      : isLinux()
        ? "linux"
        : undefined;
  const environmentForTool = (
    tool: ToolName,
  ): UninstallEnvironment | undefined => {
    if (environmentChoice !== "auto") return environmentChoice;
    const detected = toolVersionByName.get(tool)?.env_type;
    if (detected === "wsl") return "linux";
    return detected && detected !== "unknown" ? detected : hostEnvironment;
  };
  const environmentLabelForTool = (tool: ToolName) => {
    const info = toolVersionByName.get(tool);
    if (environmentChoice === "auto" && info?.env_type === "wsl") {
      return info.wsl_distro ? `WSL · ${info.wsl_distro}` : "WSL";
    }
    const environment = environmentForTool(tool);
    return environment
      ? { windows: "Windows", macos: "macOS", linux: "Linux" }[environment]
      : t("common.unknown");
  };
  const environmentLabels = [
    ...new Set(TOOL_NAMES.map(environmentLabelForTool)),
  ];
  const methods = (Object.keys(UNINSTALL_METHODS) as UninstallMethod[]).filter(
    (value) => uninstallCommandsFor(value, environmentForTool).length > 0,
  );
  // A refreshed detection can remove the selected method (e.g. WSL -> Windows).
  const selectedMethod = methods.includes(method) ? method : "npm";

  const copyCommand = async (command: string) => {
    try {
      await copyText(command);
      toast.success(t("settings.uninstallCommandCopied"), {
        closeButton: true,
      });
    } catch {
      toast.error(t("settings.uninstallCommandCopyFailed"));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("settings.manualUninstallCommands")}</DialogTitle>
          <DialogDescription>
            {t("settings.manualUninstallHint")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto px-6">
          <div className="flex flex-wrap items-center gap-3">
            <label
              htmlFor="manual-uninstall-environment"
              className="shrink-0 text-body text-fg-2"
            >
              {t("settings.manualUninstallEnvironment")}
            </label>
            <Select
              value={environmentChoice}
              onValueChange={(value) =>
                setEnvironmentChoice(value as UninstallEnvironment)
              }
            >
              <SelectTrigger
                id="manual-uninstall-environment"
                className="w-[200px]"
              >
                <SelectValue>
                  {environmentChoice === "auto"
                    ? environmentLabels.join(" / ")
                    : undefined}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="windows">Windows</SelectItem>
                <SelectItem value="macos">macOS</SelectItem>
                <SelectItem value="linux">Linux / WSL</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <label
              htmlFor="manual-uninstall-method"
              className="shrink-0 text-body text-fg-2"
            >
              {t("settings.manualUninstallMethod")}
            </label>
            <Select
              value={selectedMethod}
              onValueChange={(value) => setMethod(value as UninstallMethod)}
            >
              <SelectTrigger id="manual-uninstall-method" className="w-[200px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {methods.map((value) => (
                  <SelectItem key={value} value={value}>
                    {UNINSTALL_METHODS[value]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <p className="text-caption text-fg-2">
            {t("settings.manualUninstallEnvironmentHint")}
          </p>
          <ul className="space-y-3">
            {uninstallCommandsFor(selectedMethod, environmentForTool).map(
              ({ tool, command, variant }) => (
                <li key={command} className="space-y-1.5">
                  <div className="flex items-center justify-between gap-3">
                    <span className="text-strong text-fg-1">
                      {toolDisplayName(tool)}
                      {variant && ` (${variant})`}
                      {environmentLabels.length > 1 && (
                        <span className="ml-2 text-caption font-normal text-fg-2">
                          {environmentLabelForTool(tool)}
                        </span>
                      )}
                    </span>
                    <Button
                      variant="neutral"
                      size="compact"
                      aria-label={t("settings.copyToolUninstallCommand", {
                        tool: toolDisplayName(tool),
                      })}
                      onClick={() => void copyCommand(command)}
                    >
                      <Copy className="h-3.5 w-3.5" />
                      {t("common.copy")}
                    </Button>
                  </div>
                  {tool === "openclaw" && (
                    <p className="text-caption text-fg-2">
                      {t("settings.manualUninstallOpenClawHint")}
                    </p>
                  )}
                  {tool === "openclaw" && selectedMethod === "bun" && (
                    <p className="text-caption text-fg-2">
                      {t("settings.manualUninstallBunLauncherHint")}
                    </p>
                  )}
                  <pre className="overflow-x-auto rounded-control border border-border bg-subtle px-3 py-2.5 font-mono text-caption">
                    <code>{command}</code>
                  </pre>
                </li>
              ),
            )}
          </ul>
          <p className="text-caption text-fg-2">
            {t("settings.manualUninstallOtherMethods")}
          </p>
        </div>
        <DialogFooter>
          <Button
            variant="neutral"
            size="regular"
            onClick={() => onOpenChange(false)}
          >
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
