/**
 * 故障转移切换开关组件
 *
 * 放置在主界面头部，用于一键启用/关闭自动故障转移
 */

import { Shuffle, Loader2 } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import {
  useAutoFailoverEnabled,
  useSetAutoFailoverEnabled,
} from "@/lib/query/failover";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";

interface FailoverToggleProps {
  className?: string;
  activeApp: AppId;
}

export function FailoverToggle({ className, activeApp }: FailoverToggleProps) {
  const { t } = useTranslation();
  const { data: isEnabled = false, isLoading } =
    useAutoFailoverEnabled(activeApp);
  const setEnabled = useSetAutoFailoverEnabled();

  const handleToggle = (checked: boolean) => {
    setEnabled.mutate({ appType: activeApp, enabled: checked });
  };

  const appLabel =
    activeApp === "claude"
      ? "Claude"
      : activeApp === "codex"
        ? "Codex"
        : "Gemini";

  const tooltipText = isEnabled
    ? t("failover.tooltip.enabled", {
        app: appLabel,
        defaultValue: `${appLabel} 故障转移已启用\n按队列优先级（P1→P2→...）选择供应商`,
      })
    : t("failover.tooltip.disabled", {
        app: appLabel,
        defaultValue: `启用 ${appLabel} 故障转移\n将立即切换到队列 P1，并在失败时自动切换到下一个`,
      });

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 h-8 rounded-lg bg-muted/50 transition-all",
        className,
      )}
      title={tooltipText}
    >
      {setEnabled.isPending || isLoading ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Shuffle
          className={cn(
            "h-4 w-4 transition-colors",
            isEnabled
              ? "text-emerald-500 animate-pulse"
              : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={isEnabled}
        onCheckedChange={handleToggle}
        disabled={setEnabled.isPending || isLoading}
      />
    </div>
  );
}
