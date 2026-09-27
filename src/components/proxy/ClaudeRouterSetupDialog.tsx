import { AlertTriangle, Copy, Network } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export function buildClaudeRouterBaseUrl(
  listenAddress: string,
  port: number,
): string {
  const trimmedAddress = listenAddress.trim();
  const normalizedAddress =
    trimmedAddress === "localhost" ? "127.0.0.1" : trimmedAddress;
  const clientAddress =
    normalizedAddress === "0.0.0.0"
      ? "127.0.0.1"
      : normalizedAddress === "::"
        ? "::1"
        : normalizedAddress;
  const host = clientAddress.includes(":")
    ? `[${clientAddress}]`
    : clientAddress;
  return `http://${host}:${port}/claude-router`;
}

export function buildClaudeRouterVsCodeSettings(
  port: number,
  listenAddress = "127.0.0.1",
): string {
  const baseUrl = buildClaudeRouterBaseUrl(listenAddress, port);
  return JSON.stringify(
    {
      "claudeCode.environmentVariables": [
        { name: "ANTHROPIC_BASE_URL", value: baseUrl },
        { name: "ANTHROPIC_AUTH_TOKEN", value: "PROXY_MANAGED" },
        {
          name: "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
          value: "1",
        },
      ],
      "claudeCode.disableLoginPrompt": true,
    },
    null,
    2,
  );
}

interface ClaudeRouterSetupDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  listenAddress: string;
  port: number;
  exposedModelCount: number;
}

export function ClaudeRouterSetupDialog({
  open,
  onOpenChange,
  listenAddress,
  port,
  exposedModelCount,
}: ClaudeRouterSetupDialogProps) {
  const { t } = useTranslation();
  const baseUrl = buildClaudeRouterBaseUrl(listenAddress, port);
  const settings = buildClaudeRouterVsCodeSettings(port, listenAddress);

  const copySettings = async () => {
    try {
      await navigator.clipboard.writeText(settings);
      toast.success(
        t("claudeRouter.setup.copySuccess", {
          defaultValue: "Claude Code settings copied.",
        }),
      );
    } catch (error) {
      console.error("[ClaudeRouter] Failed to copy VS Code settings", error);
      toast.error(
        t("claudeRouter.setup.copyFailed", {
          defaultValue: "Failed to copy Claude Code settings.",
        }),
      );
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex max-h-[85vh] max-w-2xl flex-col"
        zIndex="top"
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Network className="h-5 w-5 text-emerald-500" />
            {t("claudeRouter.setup.title", {
              defaultValue: "Claude Code Gateway setup",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("claudeRouter.setup.description", {
              defaultValue:
                "Expose router-enabled providers in the Claude Code model picker without changing the active provider.",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 space-y-4 overflow-y-auto pr-1">
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="rounded-md border border-border bg-muted/30 p-3">
              <p className="text-xs text-muted-foreground">
                {t("claudeRouter.setup.baseUrl", {
                  defaultValue: "Gateway base URL",
                })}
              </p>
              <code className="mt-1 block break-all text-sm">{baseUrl}</code>
            </div>
            <div className="rounded-md border border-border bg-muted/30 p-3">
              <p className="text-xs text-muted-foreground">
                {t("claudeRouter.setup.exposedModels", {
                  defaultValue: "Exposed models",
                })}
              </p>
              <p className="mt-1 text-sm font-medium">
                {t("claudeRouter.setup.modelCount", {
                  count: exposedModelCount,
                  defaultValue: "{{count}} models exposed",
                })}
              </p>
            </div>
          </div>

          <div className="space-y-2">
            <p className="text-sm font-medium">
              {t("claudeRouter.setup.settingsTitle", {
                defaultValue: "VS Code settings.json merge snippet",
              })}
            </p>
            <pre className="max-h-72 overflow-auto rounded-md border border-border bg-muted/50 p-3 text-xs leading-relaxed">
              <code>{settings}</code>
            </pre>
          </div>

          <div className="flex gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-800 dark:text-amber-200">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <p>
              {t("claudeRouter.setup.mergeWarning", {
                defaultValue:
                  "If claudeCode.environmentVariables already exists, merge these entries into the existing array; do not replace it.",
              })}
            </p>
          </div>

          <p className="text-xs text-muted-foreground">
            {t("claudeRouter.setup.reloadHint", {
              defaultValue:
                "Start a new Claude session. If models remain cached, run Developer: Reload Window.",
            })}
          </p>
        </div>

        <DialogFooter>
          <Button type="button" onClick={copySettings}>
            <Copy className="h-4 w-4" />
            {t("claudeRouter.setup.copy", { defaultValue: "Copy settings" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
