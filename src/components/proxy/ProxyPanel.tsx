import { useState, useEffect } from "react";
import {
  Activity,
  Clock,
  TrendingUp,
  Server,
  ListOrdered,
  Save,
  Loader2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { toast } from "sonner";
import { useFailoverQueue } from "@/lib/query/failover";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { useProviderHealth } from "@/lib/query/failover";
import {
  useProxyTakeoverStatus,
  useSetProxyTakeoverForApp,
  useGlobalProxyConfig,
  useUpdateGlobalProxyConfig,
} from "@/lib/query/proxy";
import type { ProxyStatus } from "@/types/proxy";
import { useTranslation } from "react-i18next";

export function ProxyPanel() {
  const { t } = useTranslation();
  const { status, isRunning } = useProxyStatus();

  // 获取应用接管状态
  const { data: takeoverStatus } = useProxyTakeoverStatus();
  const setTakeoverForApp = useSetProxyTakeoverForApp();

  // 获取全局代理配置
  const { data: globalConfig } = useGlobalProxyConfig();
  const updateGlobalConfig = useUpdateGlobalProxyConfig();

  // 监听地址/端口的本地状态（端口用字符串以支持完全清空）
  const [listenAddress, setListenAddress] = useState("127.0.0.1");
  const [listenPort, setListenPort] = useState("15721");

  // 同步全局配置到本地状态
  useEffect(() => {
    if (globalConfig) {
      setListenAddress(globalConfig.listenAddress);
      setListenPort(String(globalConfig.listenPort));
    }
  }, [globalConfig]);

  // 获取所有三个应用类型的故障转移队列（不包含当前供应商）
  // 当前供应商始终优先，队列仅用于失败后的备用顺序
  const { data: claudeQueue = [] } = useFailoverQueue("claude");
  const { data: codexQueue = [] } = useFailoverQueue("codex");
  const { data: geminiQueue = [] } = useFailoverQueue("gemini");

  const handleTakeoverChange = async (appType: string, enabled: boolean) => {
    try {
      await setTakeoverForApp.mutateAsync({ appType, enabled });
      toast.success(
        enabled
          ? t("proxy.takeover.enabled", {
              app: appType,
              defaultValue: `${appType} 接管已启用`,
            })
          : t("proxy.takeover.disabled", {
              app: appType,
              defaultValue: `${appType} 接管已关闭`,
            }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("proxy.takeover.failed", {
          defaultValue: "切换接管状态失败",
        }),
      );
    }
  };

  const handleLoggingChange = async (enabled: boolean) => {
    if (!globalConfig) return;
    try {
      await updateGlobalConfig.mutateAsync({
        ...globalConfig,
        enableLogging: enabled,
      });
      toast.success(
        enabled
          ? t("proxy.logging.enabled", { defaultValue: "日志记录已启用" })
          : t("proxy.logging.disabled", { defaultValue: "日志记录已关闭" }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("proxy.logging.failed", { defaultValue: "切换日志状态失败" }),
      );
    }
  };

  const handleSaveBasicConfig = async () => {
    if (!globalConfig) return;

    // 校验地址格式（简单的 IP 地址或 localhost 校验）
    const addressTrimmed = listenAddress.trim();
    const ipv4Regex = /^(\d{1,3}\.){3}\d{1,3}$/;
    const isValidAddress =
      addressTrimmed === "localhost" ||
      addressTrimmed === "0.0.0.0" ||
      (ipv4Regex.test(addressTrimmed) &&
        addressTrimmed.split(".").every((n) => {
          const num = parseInt(n);
          return num >= 0 && num <= 255;
        }));
    if (!isValidAddress) {
      toast.error(
        t("proxy.settings.invalidAddress", {
          defaultValue:
            "地址无效，请输入有效的 IP 地址（如 127.0.0.1）或 localhost",
        }),
      );
      return;
    }

    // 严格校验端口：必须是纯数字
    const portTrimmed = listenPort.trim();
    if (!/^\d+$/.test(portTrimmed)) {
      toast.error(
        t("proxy.settings.invalidPort", {
          defaultValue: "端口无效，请输入 1024-65535 之间的数字",
        }),
      );
      return;
    }
    const port = parseInt(portTrimmed);
    if (isNaN(port) || port < 1024 || port > 65535) {
      toast.error(
        t("proxy.settings.invalidPort", {
          defaultValue: "端口无效，请输入 1024-65535 之间的数字",
        }),
      );
      return;
    }
    try {
      await updateGlobalConfig.mutateAsync({
        ...globalConfig,
        listenAddress: addressTrimmed,
        listenPort: port,
      });
      toast.success(
        t("proxy.settings.configSaved", { defaultValue: "代理配置已保存" }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("proxy.settings.configSaveFailed", { defaultValue: "保存配置失败" }),
      );
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

  // 格式化地址用于 URL（IPv6 需要方括号）
  const formatAddressForUrl = (address: string, port: number): string => {
    const isIPv6 = address.includes(":");
    const host = isIPv6 ? `[${address}]` : address;
    return `http://${host}:${port}`;
  };

  return (
    <>
      <section className="space-y-6">
        {isRunning && status ? (
          <div className="space-y-6">
            <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
              <div>
                <p className="text-xs text-muted-foreground mb-2">
                  {t("proxy.panel.serviceAddress", {
                    defaultValue: "服务地址",
                  })}
                </p>
                <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                  <code className="flex-1 text-sm bg-background px-3 py-2 rounded border border-border/60">
                    {formatAddressForUrl(status.address, status.port)}
                  </code>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => {
                      navigator.clipboard.writeText(
                        formatAddressForUrl(status.address, status.port),
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
                <p className="text-xs text-muted-foreground mt-2">
                  {t("proxy.settings.restartRequired", {
                    defaultValue: "修改监听地址/端口需要先停止代理服务",
                  })}
                </p>
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

              {/* 应用接管开关 */}
              <div className="pt-3 border-t border-border space-y-3">
                <p className="text-xs text-muted-foreground">
                  {t("proxyConfig.appTakeover", {
                    defaultValue: "应用接管",
                  })}
                </p>
                <div className="grid gap-2 sm:grid-cols-3">
                  {(["claude", "codex", "gemini"] as const).map((appType) => {
                    const isEnabled =
                      takeoverStatus?.[
                        appType as keyof typeof takeoverStatus
                      ] ?? false;
                    return (
                      <div
                        key={appType}
                        className="flex items-center justify-between rounded-md border border-border bg-background/60 px-3 py-2"
                      >
                        <span className="text-sm font-medium capitalize">
                          {appType}
                        </span>
                        <Switch
                          checked={isEnabled}
                          onCheckedChange={(checked) =>
                            handleTakeoverChange(appType, checked)
                          }
                          disabled={setTakeoverForApp.isPending}
                        />
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* 日志记录开关 */}
              <div className="pt-3 border-t border-border">
                <div className="flex items-center justify-between rounded-md border border-border bg-background/60 px-3 py-2">
                  <div className="space-y-0.5">
                    <Label className="text-sm font-medium">
                      {t("proxy.settings.fields.enableLogging.label", {
                        defaultValue: "启用日志记录",
                      })}
                    </Label>
                    <p className="text-xs text-muted-foreground">
                      {t("proxy.settings.fields.enableLogging.description", {
                        defaultValue: "记录所有代理请求，便于排查问题",
                      })}
                    </p>
                  </div>
                  <Switch
                    checked={globalConfig?.enableLogging ?? true}
                    onCheckedChange={handleLoggingChange}
                    disabled={updateGlobalConfig.isPending}
                  />
                </div>
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
                      targets={claudeQueue.map((item) => ({
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
                      targets={codexQueue.map((item) => ({
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
                      targets={geminiQueue.map((item) => ({
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
          <div className="space-y-6">
            {/* 空白区域避免冲突 */}
            <div className="h-4"></div>

            {/* 基础设置 - 监听地址/端口 */}
            <div className="rounded-lg border border-border bg-muted/40 p-4 space-y-4">
              <div>
                <h4 className="text-sm font-semibold">
                  {t("proxy.settings.basic.title", {
                    defaultValue: "基础设置",
                  })}
                </h4>
                <p className="text-xs text-muted-foreground">
                  {t("proxy.settings.basic.description", {
                    defaultValue: "配置代理服务监听的地址与端口。",
                  })}
                </p>
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="listen-address">
                    {t("proxy.settings.fields.listenAddress.label", {
                      defaultValue: "监听地址",
                    })}
                  </Label>
                  <Input
                    id="listen-address"
                    value={listenAddress}
                    onChange={(e) => setListenAddress(e.target.value)}
                    placeholder={t(
                      "proxy.settings.fields.listenAddress.placeholder",
                      {
                        defaultValue: "127.0.0.1",
                      },
                    )}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.settings.fields.listenAddress.description", {
                      defaultValue:
                        "代理服务器监听的 IP 地址（推荐 127.0.0.1）",
                    })}
                  </p>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="listen-port">
                    {t("proxy.settings.fields.listenPort.label", {
                      defaultValue: "监听端口",
                    })}
                  </Label>
                  <Input
                    id="listen-port"
                    type="number"
                    value={listenPort}
                    onChange={(e) => setListenPort(e.target.value)}
                    placeholder={t(
                      "proxy.settings.fields.listenPort.placeholder",
                      {
                        defaultValue: "15721",
                      },
                    )}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.settings.fields.listenPort.description", {
                      defaultValue: "代理服务器监听的端口号（1024 ~ 65535）",
                    })}
                  </p>
                </div>
              </div>

              <div className="flex justify-end">
                <Button
                  size="sm"
                  onClick={handleSaveBasicConfig}
                  disabled={updateGlobalConfig.isPending}
                >
                  {updateGlobalConfig.isPending ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      {t("common.saving", { defaultValue: "保存中..." })}
                    </>
                  ) : (
                    <>
                      <Save className="mr-2 h-4 w-4" />
                      {t("common.save", { defaultValue: "保存" })}
                    </>
                  )}
                </Button>
              </div>
            </div>

            {/* 代理服务已停止提示 */}
            <div className="text-center py-6 text-muted-foreground">
              <div className="mx-auto w-16 h-16 rounded-full bg-muted flex items-center justify-center mb-4">
                <Server className="h-8 w-8" />
              </div>
              <p className="text-base font-medium text-foreground mb-1">
                {t("proxy.panel.stoppedTitle", {
                  defaultValue: "代理服务已停止",
                })}
              </p>
              <p className="text-sm text-muted-foreground">
                {t("proxy.panel.stoppedDescription", {
                  defaultValue: "使用右上角开关即可启动服务",
                })}
              </p>
            </div>
          </div>
        )}
      </section>
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
