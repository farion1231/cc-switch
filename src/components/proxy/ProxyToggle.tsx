/**
 * 代理模式切换开关组件
 *
 * 放置在主界面头部，用于一键启用/关闭代理模式
 * 启用时自动接管 Live 配置，关闭时恢复原始配置
 */

import { Radio, Loader2 } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";

interface ProxyToggleProps {
  className?: string;
}

export function ProxyToggle({ className }: ProxyToggleProps) {
  const {
    isRunning,
    isTakeoverActive,
    startWithTakeover,
    stopWithRestore,
    isPending,
    status,
  } = useProxyStatus();

  const handleToggle = async (checked: boolean) => {
    if (checked) {
      await startWithTakeover();
    } else {
      await stopWithRestore();
    }
  };

  const isActive = isRunning && isTakeoverActive;

  const tooltipText = isActive
    ? `代理模式运行中 - ${status?.address}:${status?.port}\n切换供应商为热切换`
    : "开启代理模式\n启用后自动接管 Live 配置";

  return (
    <div
      className={cn(
        "flex items-center gap-2 px-3 py-1.5 rounded-lg transition-all cursor-default",
        isActive
          ? "bg-emerald-500/10 border border-emerald-500/30"
          : "bg-muted/50 hover:bg-muted",
        className,
      )}
      title={tooltipText}
    >
      {isPending ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            isActive
              ? "text-emerald-500 animate-pulse"
              : "text-muted-foreground",
          )}
        />
      )}
      <span
        className={cn(
          "text-sm font-medium transition-colors select-none",
          isActive
            ? "text-emerald-600 dark:text-emerald-400"
            : "text-muted-foreground",
        )}
      >
        Proxy
      </span>
      <Switch
        checked={isActive}
        onCheckedChange={handleToggle}
        disabled={isPending}
        className="ml-1"
      />
    </div>
  );
}
