import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { ShieldAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { OhMyPiAgentDiscoveryProvider } from "@/lib/api/ohmypi";

interface OhMyPiAutoDiscoveryConfirmDialogProps {
  isOpen: boolean;
  providers: OhMyPiAgentDiscoveryProvider[];
  /** Whether the disable write-back is in flight (disables buttons). */
  pending?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * 启用 Oh My Pi 管理前的「自动发现确认门卫」。当 omp 用户级 `config.yml`
 * 的 `disabledProviders` 尚未包含全部其他 Agent 发现源时弹出：列出 omp 会
 * 自动发现 MCP/技能的 12 个 Agent、说明状态不一致原因、写回范围（保留
 * omp 自身与 `.mcp.json`/SSH 来源）与生效时机。确认→并入缺失来源后启用；
 * 取消→不启用、不改配置。
 */
export function OhMyPiAutoDiscoveryConfirmDialog({
  isOpen,
  providers,
  pending = false,
  onConfirm,
  onCancel,
}: OhMyPiAutoDiscoveryConfirmDialogProps) {
  const { t } = useTranslation();

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      <DialogContent className="max-w-lg" zIndex="alert">
        <DialogHeader className="space-y-2 border-b-0 bg-transparent pb-0">
          <DialogTitle className="flex items-center gap-2 text-base font-semibold">
            <ShieldAlert className="h-5 w-5 text-amber-500" />
            {t("ohmypi.autoDiscovery.dialog.title")}
          </DialogTitle>
          <DialogDescription className="text-xs leading-relaxed text-muted-foreground">
            {t("ohmypi.autoDiscovery.dialog.reason")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 px-6">
          <div className="rounded-lg border border-amber-500/20 bg-amber-500/5 p-3">
            <div className="mb-2 text-xs font-medium">
              {t("ohmypi.autoDiscovery.dialog.agentListHeading")}
            </div>
            <ul className="grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-2">
              {providers.map((p) => (
                <li key={p.id} className="text-xs leading-snug text-foreground">
                  {p.displayName}
                </li>
              ))}
            </ul>
          </div>

          <div className="text-xs leading-relaxed text-muted-foreground">
            <p>{t("ohmypi.autoDiscovery.dialog.scope")}</p>
            <p>{t("ohmypi.autoDiscovery.dialog.effectTiming")}</p>
          </div>
        </div>

        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={onCancel} disabled={pending}>
            {t("common.cancel")}
          </Button>
          <Button onClick={onConfirm} disabled={pending}>
            {t("ohmypi.autoDiscovery.dialog.confirmBtn")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
