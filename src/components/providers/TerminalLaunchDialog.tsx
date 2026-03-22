import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Terminal, ShieldAlert } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";

interface TerminalLaunchDialogProps {
  isOpen: boolean;
  provider: Provider | null;
  onConfirm: (bypass: boolean, enableTelegramChannel: boolean) => void;
  onCancel: () => void;
}

export function TerminalLaunchDialog({
  isOpen,
  provider,
  onConfirm,
  onCancel,
}: TerminalLaunchDialogProps) {
  const { t } = useTranslation();
  const [enableTelegramChannel, setEnableTelegramChannel] = useState(false);
  const telegramCheckboxId = useId();

  useEffect(() => {
    if (isOpen) {
      setEnableTelegramChannel(false);
    }
  }, [isOpen, provider?.id]);

  if (!provider) return null;

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) {
          onCancel();
        }
      }}
    >
      <DialogContent className="max-w-md" zIndex="alert">
        <DialogHeader className="space-y-3">
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <Terminal className="h-5 w-5 text-primary" />
            {t("provider.terminalLaunchTitle", "启动终端")}
          </DialogTitle>
          <DialogDescription className="space-y-3 pt-2">
            <p className="text-sm leading-relaxed text-foreground/90">
              {t(
                "provider.terminalLaunchMessage",
                "您准备在终端中启动 Claude Code。是否启用安全跳过权限 (--dangerously-skip-permissions) 模式？",
              )}
            </p>
            <div className="flex items-start gap-3 rounded-lg border border-border/70 bg-muted/30 p-3">
              <Checkbox
                id={telegramCheckboxId}
                checked={enableTelegramChannel}
                onCheckedChange={(checked) =>
                  setEnableTelegramChannel(checked === true)
                }
              />
              <div className="space-y-1">
                <Label
                  htmlFor={telegramCheckboxId}
                  className="cursor-pointer text-sm font-medium leading-5"
                >
                  {t("provider.terminalLaunchTelegram", "TG 通信")}
                </Label>
                <p className="text-xs leading-normal text-muted-foreground">
                  {t(
                    "provider.terminalLaunchTelegramHint",
                    "勾选后会自动追加 --channels plugin:telegram@claude-plugins-official",
                  )}
                </p>
              </div>
            </div>
            <div className="flex items-start gap-2 rounded-lg bg-amber-500/10 p-3 text-amber-600 dark:text-amber-400">
              <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <p className="text-xs leading-normal whitespace-pre-wrap">
                {t(
                  "provider.dangerModeHint",
                  "开启此模式可以解决部分环境下 Claude Code 提示“非法请求”或“签名错误”的问题。\n注意：该模式会跳过所有权限确认，请务必确保项目环境安全。",
                )}
              </p>
            </div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="flex flex-col gap-2 pt-2 sm:flex-row sm:justify-end">
          <Button variant="outline" onClick={onCancel} className="sm:order-1">
            {t("common.cancel")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => onConfirm(false, enableTelegramChannel)}
            className="sm:order-2"
          >
            {t("provider.terminalLaunchNormal", "普通启动")}
          </Button>
          <Button
            variant="default"
            onClick={() => onConfirm(true, enableTelegramChannel)}
            className="bg-amber-600 hover:bg-amber-700 dark:bg-amber-700 dark:hover:bg-amber-800 sm:order-3"
          >
            {t("provider.terminalLaunchBypass", "跳过权限确认")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
