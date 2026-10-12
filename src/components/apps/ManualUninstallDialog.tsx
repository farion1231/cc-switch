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
import { isWindows } from "@/lib/platform";
import { toast } from "@/lib/toast";
import { toolDisplayName, type ToolName } from "./useToolManagement";

// Package names match the backend's npm_package_for. Hermes uses Python.
const NPM_PACKAGES = {
  claude: "@anthropic-ai/claude-code",
  codex: "@openai/codex",
  gemini: "@google/gemini-cli",
  grok: "@xai-official/grok",
  opencode: "opencode-ai",
  openclaw: "openclaw",
  pi: "@earendil-works/pi-coding-agent",
  mcode: "@minimax-ai/code",
} satisfies Partial<Record<ToolName, string>>;

const METHODS = {
  npm: "npm",
  pnpm: "pnpm",
  bun: "bun",
  volta: "Volta",
  "brew-formula": "Homebrew (formula)",
  "brew-cask": "Homebrew (cask)",
  pip: "pip",
};
type InstallMethod = keyof typeof METHODS;

function commandsFor(method: InstallMethod): Partial<Record<ToolName, string>> {
  switch (method) {
    case "brew-formula":
      return {
        claude: "brew uninstall --formula claude-code",
        codex: "brew uninstall --formula codex",
        gemini: "brew uninstall --formula gemini-cli",
        opencode: "brew uninstall --formula opencode",
      };
    case "brew-cask":
      return {
        claude: "brew uninstall --cask claude-code",
        codex: "brew uninstall --cask codex",
      };
    case "pip":
      return {
        hermes: `${isWindows() ? "python" : "python3"} -m pip uninstall hermes-agent`,
      };
    default: {
      const prefix = {
        npm: "npm uninstall -g",
        pnpm: "pnpm remove -g",
        bun: "bun remove -g",
        volta: "volta uninstall",
      }[method];
      return Object.fromEntries(
        Object.entries(NPM_PACKAGES).map(([tool, pkg]) => [
          tool,
          `${prefix} ${pkg}`,
        ]),
      );
    }
  }
}

export function ManualUninstallDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  const [method, setMethod] = useState<InstallMethod>("npm");

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
          <div className="flex items-center gap-3">
            <label
              htmlFor="manual-uninstall-method"
              className="shrink-0 text-body text-fg-2"
            >
              {t("settings.manualUninstallMethod")}
            </label>
            <Select
              value={method}
              onValueChange={(value) => setMethod(value as InstallMethod)}
            >
              <SelectTrigger id="manual-uninstall-method" className="w-[200px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(METHODS).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <p className="text-caption text-fg-2">
            {t("settings.manualUninstallEnvironmentHint")}
          </p>
          <ul className="space-y-3">
            {Object.entries(commandsFor(method)).map(([tool, command]) => (
              <li key={tool} className="space-y-1.5">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-strong text-fg-1">
                    {toolDisplayName(tool)}
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
                <pre className="overflow-x-auto rounded-control border border-border bg-subtle px-3 py-2.5 font-mono text-caption">
                  <code>{command}</code>
                </pre>
              </li>
            ))}
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
