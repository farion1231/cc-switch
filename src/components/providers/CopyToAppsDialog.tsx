import { useEffect, useMemo, useState } from "react";
import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
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
import { APP_ICON_MAP, APP_IDS } from "@/config/appConfig";
import type { AppId } from "@/lib/api/types";
import type { Provider } from "@/types";

interface CopyToAppsDialogProps {
  isOpen: boolean;
  onClose: () => void;
  provider: Provider | null;
  sourceApp: AppId;
  onCopy: (provider: Provider, targetApps: AppId[]) => Promise<void>;
}

export function CopyToAppsDialog({
  isOpen,
  onClose,
  provider,
  sourceApp,
  onCopy,
}: CopyToAppsDialogProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<ReadonlySet<AppId>>(new Set());
  const [isCopying, setIsCopying] = useState(false);

  // Claude Desktop 仅接受 Claude 形状的配置：非 Claude 源不提供该目标。
  const targetApps = useMemo(
    () =>
      APP_IDS.filter(
        (appId) =>
          appId !== sourceApp &&
          !(appId === "claude-desktop" && sourceApp !== "claude"),
      ),
    [sourceApp],
  );

  // 每次打开都回到"全不选"，由用户显式决定复制目标。
  useEffect(() => {
    if (isOpen) {
      setSelected(new Set());
      setIsCopying(false);
    }
  }, [isOpen]);

  const allSelected =
    targetApps.length > 0 && targetApps.every((appId) => selected.has(appId));

  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(targetApps));
  };

  const toggleApp = (appId: AppId) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(appId)) {
        next.delete(appId);
      } else {
        next.add(appId);
      }
      return next;
    });
  };

  const handleCopy = async () => {
    if (!provider || selected.size === 0 || isCopying) return;
    setIsCopying(true);
    try {
      await onCopy(
        provider,
        targetApps.filter((appId) => selected.has(appId)),
      );
      // 成功后关闭；选择状态在下次打开时重置。
      onClose();
    } catch {
      // 失败时保持对话框打开，用户可直接重试或取消。
    } finally {
      setIsCopying(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !isCopying) {
          onClose();
        }
      }}
    >
      <DialogContent className="max-w-md" zIndex="top">
        <DialogHeader>
          <DialogTitle>{t("provider.copyToApps.title")}</DialogTitle>
          <DialogDescription>
            {t("provider.copyToApps.description", {
              name: provider?.name ?? "",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 overflow-y-auto px-6 py-4">
          <div className="flex items-center justify-between rounded-md bg-muted/40 px-2 py-1.5">
            <label className="flex cursor-pointer items-center gap-2.5">
              <Checkbox
                checked={allSelected}
                onCheckedChange={toggleAll}
                aria-label={t("provider.copyToApps.selectAll")}
              />
              <span className="text-sm font-medium">
                {t("provider.copyToApps.selectAll")}
              </span>
            </label>
            <span className="text-xs text-muted-foreground">
              {t("provider.copyToApps.selectedCount", {
                count: selected.size,
                total: targetApps.length,
              })}
            </span>
          </div>

          <div className="grid grid-cols-1 gap-0.5">
            {targetApps.map((appId) => {
              const app = APP_ICON_MAP[appId];
              return (
                <label
                  key={appId}
                  className="flex cursor-pointer items-center gap-2.5 rounded-md px-2 py-1.5 hover:bg-muted/60"
                >
                  <Checkbox
                    checked={selected.has(appId)}
                    onCheckedChange={() => toggleApp(appId)}
                    aria-label={app.label}
                  />
                  {app.icon}
                  <span className="text-sm">{app.label}</span>
                </label>
              );
            })}
          </div>
        </div>

        <DialogFooter className="gap-3 sm:items-center">
          <p className="mr-auto text-left text-xs leading-relaxed text-muted-foreground sm:max-w-[55%]">
            {t("provider.copyToApps.modelHint")}
          </p>
          <Button variant="outline" onClick={onClose} disabled={isCopying}>
            {t("common.cancel")}
          </Button>
          <Button
            onClick={handleCopy}
            disabled={isCopying || selected.size === 0}
          >
            {isCopying && <Loader2 className="h-4 w-4 animate-spin" />}
            {isCopying
              ? t("provider.copyToApps.copying")
              : t("provider.copyToApps.copy", { count: selected.size })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
