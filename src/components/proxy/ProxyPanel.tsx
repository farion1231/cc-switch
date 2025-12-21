import { useState } from "react";
import {
  Activity,
  Clock,
  TrendingUp,
  Server,
  ListOrdered,
  Settings,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { ProxySettingsDialog } from "./ProxySettingsDialog";
import { toast } from "sonner";
import { useFailoverQueue } from "@/lib/query/failover";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { useProviderHealth } from "@/lib/query/failover";
import type { ProxyStatus } from "@/types/proxy";
import { useTranslation } from "react-i18next";

export function ProxyPanel() {
  const { t } = useTranslation();
  const { status, isRunning } = useProxyStatus();
  const [showSettings, setShowSettings] = useState(false);

  // 获取所有三个应用类型的故障转移队列（不包含当前供应商）
  // 当前供应商始终优先，队列仅用于失败后的备用顺序
  const { data: claudeQueue = [] } = useFailoverQueue("claude");
  const { data: codexQueue = [] } = useFailoverQueue("codex");
  const { data: geminiQueue = [] } = useFailoverQueue("gemini");

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
                <div className="flex items-center justify-between mb-2">
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.panel.serviceAddress", {
                      defaultValue: "服务地址",
                    })}
                  </p>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setShowSettings(true)}
                    className="h-7 gap-1.5 text-xs"
                  >
                    <Settings className="h-3.5 w-3.5" />
                    {t("common.settings")}
                  </Button>
                </div>
                <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
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
                      toast.success(
                        t("proxy.panel.addressCopied", {
                          defaultValue: "地址已复制",
                        }),
                        { closeButton: true },
                      );
                    }}
                  >
                    {t("common.copy")}
                  </Button>
                </div>
              </div>

              <div className="pt-3 border-t border-border space-y-2">
                <p className="text-xs text-muted-foreground">
                  {t("provider.inUse")}
                </p>
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
                    {t("proxy.panel.currentProvider", {
                      defaultValue: "当前 Provider：",
                    })}{" "}
                    <span className="font-medium text-foreground">
                      {status.current_provider}
                    </span>
                  </p>
                ) : (
                  <p className="text-sm text-yellow-600 dark:text-yellow-400">
                    {t("proxy.panel.waitingFirstRequest", {
                      defaultValue: "当前 Provider：等待首次请求…",
                    })}
                  </p>
                )}
              </div>

              {/* 供应商队列 - 按应用类型分组展示 */}
              {(claudeQueue.length > 0 ||
                codexQueue.length > 0 ||
                geminiQueue.length > 0) && (
                <div className="pt-3 border-t border-border space-y-3">
                  <div className="flex items-center gap-2">
                    <ListOrdered className="h-3.5 w-3.5 text-muted-foreground" />
                    <p className="text-xs text-muted-foreground">
                      {t("proxy.failoverQueue.title")}
                    </p>
                  </div>

                  {/* Claude 队列 */}
                  {claudeQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="claude"
                      appLabel="Claude"
                      targets={claudeQueue
                        .filter((item) => item.enabled)
                        .sort((a, b) => a.queueOrder - b.queueOrder)
                        .map((item) => ({
                          id: item.providerId,
                          name: item.providerName,
                        }))}
                      status={status}
                    />
                  )}

                  {/* Codex 队列 */}
                  {codexQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="codex"
                      appLabel="Codex"
                      targets={codexQueue
                        .filter((item) => item.enabled)
                        .sort((a, b) => a.queueOrder - b.queueOrder)
                        .map((item) => ({
                          id: item.providerId,
                          name: item.providerName,
                        }))}
                      status={status}
                    />
                  )}

                  {/* Gemini 队列 */}
                  {geminiQueue.length > 0 && (
                    <ProviderQueueGroup
                      appType="gemini"
                      appLabel="Gemini"
                      targets={geminiQueue
                        .filter((item) => item.enabled)
                        .sort((a, b) => a.queueOrder - b.queueOrder)
                        .map((item) => ({
                          id: item.providerId,
                          name: item.providerName,
                        }))}
                      status={status}
                    />
                  )}
                </div>
              )}
            </div>

            <div className="grid gap-3 md:grid-cols-4">
              <StatCard
                icon={<Activity className="h-4 w-4" />}
                label={t("proxy.panel.stats.activeConnections", {
                  defaultValue: "活跃连接",
                })}
                value={status.active_connections}
              />
              <StatCard
                icon={<TrendingUp className="h-4 w-4" />}
                label={t("proxy.panel.stats.totalRequests", {
                  defaultValue: "总请求数",
                })}
                value={status.total_requests}
              />
              <StatCard
                icon={<Clock className="h-4 w-4" />}
                label={t("proxy.panel.stats.successRate", {
                  defaultValue: "成功率",
                })}
                value={`${status.success_rate.toFixed(1)}%`}
                variant={status.success_rate > 90 ? "success" : "warning"}
              />
              <StatCard
                icon={<Clock className="h-4 w-4" />}
                label={t("proxy.panel.stats.uptime", {
                  defaultValue: "运行时间",
                })}
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
              {t("proxy.panel.stoppedTitle", {
                defaultValue: "代理服务已停止",
              })}
            </p>
            <p className="text-sm text-muted-foreground mb-4">
              {t("proxy.panel.stoppedDescription", {
                defaultValue: "使用右上角开关即可启动服务",
              })}
            </p>
            <Button
              size="sm"
              variant="outline"
              onClick={() => setShowSettings(true)}
              className="gap-1.5"
            >
              <Settings className="h-4 w-4" />
              {t("proxy.panel.openSettings", {
                defaultValue: "配置代理服务",
              })}
            </Button>
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
  const { t } = useTranslation();
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
            {t("provider.inUse")}
          </span>
        )}
      </div>
      {/* 健康徽章 */}
      <ProviderHealthBadge
        consecutiveFailures={health?.consecutive_failures ?? 0}
      />
    </div>
  );
}
