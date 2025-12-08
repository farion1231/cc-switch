import { useState } from "react";
import { Activity, Clock, TrendingUp, Server, ListOrdered } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { ProxySettingsDialog } from "./ProxySettingsDialog";
import { toast } from "sonner";
import { useProxyTargets } from "@/lib/query/failover";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { useProviderHealth } from "@/lib/query/failover";
import type { ProxyStatus } from "@/types/proxy";

export function ProxyPanel() {
  const { status, isRunning } = useProxyStatus();
  const [showSettings, setShowSettings] = useState(false);

  // 获取所有三个应用类型的代理目标列表
  const { data: claudeTargets = [] } = useProxyTargets("claude");
  const { data: codexTargets = [] } = useProxyTargets("codex");
  const { data: geminiTargets = [] } = useProxyTargets("gemini");

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
      <section className="space-y-6">
        {isRunning && status ? (
          <div className="space-y-6">
            <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
              <div>
                <p className="text-xs text-muted-foreground">服务地址</p>
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

              <div className="pt-3 border-t border-border space-y-2">
                <p className="text-xs text-muted-foreground">使用中</p>
                {status.active_targets && status.active_targets.length > 0 ? (
                  <div className="grid gap-2 sm:grid-cols-2">
                    {status.active_targets.map((target) => (
                      <div
                        key={target.app_type}
                        className="flex items-center justify-between rounded-md border border-border bg-background/60 px-2 py-1.5 text-xs"
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

              {/* 供应商队列 - 按应用类型分组展示 */}
              {(claudeTargets.length > 0 ||
                codexTargets.length > 0 ||
                geminiTargets.length > 0) && (
                <div className="pt-3 border-t border-border space-y-3">
                  <div className="flex items-center gap-2">
                    <ListOrdered className="h-3.5 w-3.5 text-muted-foreground" />
                    <p className="text-xs text-muted-foreground">
                      故障转移队列
                    </p>
                  </div>

                  {/* Claude 队列 */}
                  {claudeTargets.length > 0 && (
                    <ProviderQueueGroup
                      appType="claude"
                      appLabel="Claude"
                      targets={claudeTargets}
                      status={status}
                    />
                  )}

                  {/* Codex 队列 */}
                  {codexTargets.length > 0 && (
                    <ProviderQueueGroup
                      appType="codex"
                      appLabel="Codex"
                      targets={codexTargets}
                      status={status}
                    />
                  )}

                  {/* Gemini 队列 */}
                  {geminiTargets.length > 0 && (
                    <ProviderQueueGroup
                      appType="gemini"
                      appLabel="Gemini"
                      targets={geminiTargets}
                      status={status}
                    />
                  )}
                </div>
              )}
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
      className={`rounded-lg border border-border bg-card/60 p-4 text-sm text-muted-foreground ${variantStyles[variant]}`}
    >
      <div className="flex items-center gap-2 text-muted-foreground mb-2">
        {icon}
        <span className="text-xs">{label}</span>
      </div>
      <p className="text-xl font-semibold text-foreground">{value}</p>
    </div>
  );
}

interface ProviderQueueGroupProps {
  appType: string;
  appLabel: string;
  targets: Array<{
    id: string;
    name: string;
  }>;
  status: ProxyStatus;
}

function ProviderQueueGroup({
  appType,
  appLabel,
  targets,
  status,
}: ProviderQueueGroupProps) {
  // 查找该应用类型的当前活跃目标
  const activeTarget = status.active_targets?.find(
    (t) => t.app_type === appType,
  );

  return (
    <div className="space-y-2">
      {/* 应用类型标题 */}
      <div className="flex items-center gap-2 px-2">
        <span className="text-xs font-semibold text-foreground/80">
          {appLabel}
        </span>
        <div className="flex-1 h-px bg-border/50" />
      </div>

      {/* 供应商列表 */}
      <div className="space-y-1.5">
        {targets.map((target, index) => (
          <ProviderQueueItem
            key={target.id}
            provider={target}
            priority={index + 1}
            appType={appType}
            isCurrent={activeTarget?.provider_id === target.id}
          />
        ))}
      </div>
    </div>
  );
}

interface ProviderQueueItemProps {
  provider: {
    id: string;
    name: string;
  };
  priority: number;
  appType: string;
  isCurrent: boolean;
}

function ProviderQueueItem({
  provider,
  priority,
  appType,
  isCurrent,
}: ProviderQueueItemProps) {
  const { data: health } = useProviderHealth(provider.id, appType);

  return (
    <div
      className={`flex items-center justify-between rounded-md border px-3 py-2 text-sm transition-colors ${
        isCurrent
          ? "border-primary/40 bg-primary/10 text-primary font-medium"
          : "border-border bg-background/60"
      }`}
    >
      <div className="flex items-center gap-2">
        <span
          className={`flex-shrink-0 flex items-center justify-center w-5 h-5 rounded-full text-xs font-bold ${
            isCurrent
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground"
          }`}
        >
          {priority}
        </span>
        <span className={isCurrent ? "" : "text-foreground"}>
          {provider.name}
        </span>
        {isCurrent && (
          <span className="text-xs px-1.5 py-0.5 rounded bg-primary/20 text-primary">
            使用中
          </span>
        )}
      </div>
      {/* 健康徽章：队列中的代理目标始终显示，没有健康数据时默认为正常 */}
      <ProviderHealthBadge
        consecutiveFailures={health?.consecutive_failures ?? 0}
        isProxyTarget={true}
      />
    </div>
  );
}
