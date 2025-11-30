import { useState } from "react";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { Settings, Activity, Clock, TrendingUp, Server } from "lucide-react";
import { ProxySettingsDialog } from "./ProxySettingsDialog";
import { toast } from "sonner";

export function ProxyPanel() {
  const { status, isRunning, start, stop, isPending } = useProxyStatus();
  const [showSettings, setShowSettings] = useState(false);

  const handleToggle = async () => {
    try {
      if (isRunning) {
        await stop();
      } else {
        await start();
      }
    } catch (error) {
      console.error("Toggle proxy failed:", error);
    }
  };

  const formatUptime = (seconds: number): string => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;

    if (hours > 0) {
      return `${hours}h ${minutes}m ${secs}s`;
    } else if (minutes > 0) {
      return `${minutes}m ${secs}s`;
    } else {
      return `${secs}s`;
    }
  };

  return (
    <>
      <section className="space-y-6 rounded-xl border border-white/10 glass-card p-6">
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-primary/10 text-primary">
              <Server className="h-5 w-5" />
            </div>
            <div>
              <h3 className="text-base font-semibold text-foreground">
                本地代理服务
              </h3>
              <p className="text-sm text-muted-foreground">
                {isRunning
                  ? `运行中 · ${status?.address}:${status?.port}`
                  : "已停止"}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <Badge
              variant={isRunning ? "default" : "secondary"}
              className="gap-1.5"
            >
              <Activity
                className={`h-3 w-3 ${isRunning ? "animate-pulse" : ""}`}
              />
              {isRunning ? "运行中" : "已停止"}
            </Badge>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setShowSettings(true)}
              disabled={isPending}
              aria-label="打开代理设置"
            >
              <Settings className="h-4 w-4" />
            </Button>
            <Switch
              checked={isRunning}
              onCheckedChange={handleToggle}
              disabled={isPending}
              aria-label={isRunning ? "停止代理服务" : "启动代理服务"}
            />
          </div>
        </div>

        {isRunning && status ? (
          <div className="space-y-6">
            <div className="rounded-lg border border-white/10 bg-muted/40 p-4 space-y-4">
              <div>
                <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                  服务地址
                </p>
                <div className="mt-2 flex flex-col gap-2 sm:flex-row sm:items-center">
                  <code className="flex-1 text-sm bg-background px-3 py-2 rounded border border-border/60">
                    http://{status.address}:{status.port}
                  </code>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => {
                      navigator.clipboard.writeText(
                        `http://${status.address}:${status.port}`,
                      );
                      toast.success("地址已复制");
                    }}
                  >
                    复制
                  </Button>
                </div>
              </div>

              <div className="pt-3 border-t border-white/10 space-y-2">
                <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                  当前代理
                </p>
                {status.active_targets && status.active_targets.length > 0 ? (
                  <div className="grid gap-2 sm:grid-cols-2">
                    {status.active_targets.map((target) => (
                      <div
                        key={target.app_type}
                        className="flex items-center justify-between rounded-md border border-white/10 bg-background/60 px-2 py-1.5 text-xs"
                      >
                        <span className="text-muted-foreground">
                          {target.app_type}
                        </span>
                        <span
                          className="ml-2 font-medium truncate text-foreground"
                          title={target.provider_name}
                        >
                          {target.provider_name}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : status.current_provider ? (
                  <p className="text-sm text-muted-foreground">
                    当前 Provider：{" "}
                    <span className="font-medium text-foreground">
                      {status.current_provider}
                    </span>
                  </p>
                ) : (
                  <p className="text-sm text-yellow-600 dark:text-yellow-400">
                    当前 Provider：等待首次请求…
                  </p>
                )}
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-4">
              <StatCard
                icon={<Activity className="h-4 w-4" />}
                label="活跃连接"
                value={status.active_connections}
              />
              <StatCard
                icon={<TrendingUp className="h-4 w-4" />}
                label="总请求数"
                value={status.total_requests}
              />
              <StatCard
                icon={<Clock className="h-4 w-4" />}
                label="成功率"
                value={`${status.success_rate.toFixed(1)}%`}
                variant={status.success_rate > 90 ? "success" : "warning"}
              />
              <StatCard
                icon={<Clock className="h-4 w-4" />}
                label="运行时间"
                value={formatUptime(status.uptime_seconds)}
              />
            </div>
          </div>
        ) : (
          <div className="text-center py-10 text-muted-foreground">
            <div className="mx-auto w-16 h-16 rounded-full bg-muted flex items-center justify-center mb-4">
              <Server className="h-8 w-8" />
            </div>
            <p className="text-base font-medium text-foreground mb-1">
              代理服务已停止
            </p>
            <p className="text-sm text-muted-foreground">
              使用右上角开关即可启动服务
            </p>
          </div>
        )}
      </section>

      <ProxySettingsDialog open={showSettings} onOpenChange={setShowSettings} />
    </>
  );
}

interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  variant?: "default" | "success" | "warning";
}

function StatCard({ icon, label, value, variant = "default" }: StatCardProps) {
  const variantStyles = {
    default: "",
    success: "border-green-500/40 bg-green-500/5",
    warning: "border-yellow-500/40 bg-yellow-500/5",
  };

  return (
    <div
      className={`rounded-lg border border-white/10 bg-white/70 p-4 text-sm text-muted-foreground dark:bg-white/5 ${variantStyles[variant]}`}
    >
      <div className="flex items-center gap-2 text-muted-foreground mb-2">
        {icon}
        <span className="text-xs font-medium uppercase tracking-wide">
          {label}
        </span>
      </div>
      <p className="text-xl font-semibold text-foreground">{value}</p>
    </div>
  );
}
