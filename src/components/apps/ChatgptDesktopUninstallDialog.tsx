import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { CHATGPT_DESKTOP_NAME } from "@/components/shell/AppGlyph";

/** 卸载 ChatGPT Desktop 前的确认：说清会卸掉什么、留下什么，并给出要执行的命令。 */
export function ChatgptDesktopUninstallDialog({
  command,
  onConfirm,
  onCancel,
}: {
  command: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="max-w-2xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>
            {t("settings.toolUninstallConfirmTitle", {
              tool: CHATGPT_DESKTOP_NAME,
            })}
          </DialogTitle>
          <DialogDescription>
            {t("appsPage.chatgptDesktop.uninstallHint")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto px-6">
          <dl className="space-y-2 text-body">
            <dt className="text-fg-2">{t("settings.toolUpgradeWillRun")}</dt>
            <dd>
              <pre className="overflow-x-auto rounded-control border border-border bg-subtle px-3 py-2.5 font-mono text-caption">
                <code>{command}</code>
              </pre>
            </dd>
          </dl>
        </div>
        <DialogFooter>
          <Button variant="neutral" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            {t("settings.toolUninstall")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
