import { useState } from "react";
import { BadgeCheck, KeyRound, Loader2, Radio } from "lucide-react";
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
import { Switch } from "@/components/ui/switch";
import {
  useCursorProviders,
  useCursorRuntimeState,
  useInstallCursorCA,
  useStartCursorRuntime,
  useStopCursorRuntime,
} from "@/lib/query/cursor";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

interface CursorRuntimeToggleProps {
  className?: string;
}

const BUSY_PHASES = new Set(["starting", "restoring", "maintenance"]);

export function CursorRuntimeToggle({ className }: CursorRuntimeToggleProps) {
  const providersQuery = useCursorProviders();
  const runtimeQuery = useCursorRuntimeState();
  const startRuntime = useStartCursorRuntime();
  const stopRuntime = useStopCursorRuntime();
  const installCA = useInstallCursorCA();
  const [trustDialogOpen, setTrustDialogOpen] = useState(false);

  const state = runtimeQuery.data;
  const running = state?.phase === "running";
  const busy =
    BUSY_PHASES.has(state?.phase ?? "") ||
    startRuntime.isPending ||
    stopRuntime.isPending ||
    installCA.isPending;
  const enabledCount = Object.values(providersQuery.data ?? {}).filter(
    (provider) => provider.settingsConfig.enabled,
  ).length;
  const canStart = enabledCount > 0 && !providersQuery.isLoading;

  const reportError = (title: string, error: unknown) => {
    toast.error(title, {
      description: extractErrorMessage(error) || undefined,
    });
  };

  const start = async () => {
    try {
      await startRuntime.mutateAsync(undefined);
      toast.success("Cursor 模型转发已启动");
    } catch (error) {
      reportError("启动 Cursor 模型转发失败", error);
    }
  };

  const stop = async () => {
    try {
      await stopRuntime.mutateAsync(undefined);
      toast.success("Cursor 已恢复原始配置");
    } catch (error) {
      reportError("停止 Cursor 模型转发失败", error);
    }
  };

  const handleToggle = (checked: boolean) => {
    if (!checked) {
      void stop();
      return;
    }
    if (!state?.caInstalled) {
      setTrustDialogOpen(true);
      return;
    }
    void start();
  };

  const handleInstallCAAndStart = async () => {
    setTrustDialogOpen(false);
    try {
      await installCA.mutateAsync(undefined);
      await startRuntime.mutateAsync(undefined);
      toast.success("CA 已信任，Cursor 模型转发已启动");
    } catch (error) {
      reportError("启用 Cursor 模型转发失败", error);
    }
  };

  const tooltipText = running
    ? `Cursor 模型转发已开启 · ${enabledCount} 个模型`
    : canStart
      ? `开启 Cursor 模型转发 · ${enabledCount} 个模型`
      : "请先添加并启用至少一个 Cursor 模型";

  return (
    <>
      <div
        className={cn(
          "flex h-8 items-center gap-1 rounded-lg bg-muted/50 px-1.5 transition-all",
          className,
        )}
        title={tooltipText}
      >
        {busy || runtimeQuery.isLoading ? (
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        ) : (
          <Radio
            className={cn(
              "h-4 w-4 transition-colors",
              running
                ? "animate-pulse text-emerald-500"
                : "text-muted-foreground",
            )}
          />
        )}
        <Switch
          checked={running}
          onCheckedChange={handleToggle}
          disabled={busy || (!running && !canStart)}
          aria-label="Cursor 模型转发"
        />
      </div>

      <CursorTrustDialog
        open={trustDialogOpen}
        platform={state?.platform}
        busy={busy}
        onOpenChange={setTrustDialogOpen}
        onConfirm={() => void handleInstallCAAndStart()}
      />
    </>
  );
}

function CursorTrustDialog({
  open,
  platform,
  busy,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  platform?: string;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  const explanation =
    platform === "windows"
      ? "证书会安装到当前用户的 Windows Root 证书库，不需要管理员权限。"
      : platform === "linux"
        ? "系统会通过 pkexec 请求管理员授权，并安装到当前发行版的系统信任库。"
        : "证书会安装到当前用户的 macOS 登录钥匙串，系统可能要求确认。";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="h-5 w-5 text-blue-500" />
            首次启用 Cursor 转发
          </DialogTitle>
          <DialogDescription>
            Cursor 的 HTTPS 流量需要信任本机为本安装生成的独立 CA。
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 px-6 py-5 text-sm">
          <p>{explanation}</p>
          <div className="rounded-lg border border-border-default bg-muted/30 p-3 text-muted-foreground">
            私钥仅保存在本机 CC Switch 数据目录。停止服务会恢复 Cursor 配置，但
            CA 会保留；你可以稍后在 Cursor 页面独立移除。
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={onConfirm} disabled={busy}>
            {busy ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <BadgeCheck className="mr-2 h-4 w-4" />
            )}
            信任并启动
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
