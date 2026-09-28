import { useMemo, useState } from "react";
import { AlertTriangle, Loader2, Play, Wifi, WifiOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import {
  useGatewayEndpoints,
  useGatewayHealth,
  useInstallKaixuanBundle,
  useStartLocalGateway,
} from "@/lib/query/kaixuan";
import type { GatewayHealth } from "@/lib/api/kaixuan";

/**
 * kaixuan Bundle 单卡组件
 *
 * 功能：
 * 1. 显示两个端点（公网 kxpms.cn / 本机 8782）的健康状态灯
 * 2. 一键安装/重新安装 bundle（输入 primary API key，可选 $ENV 占位符）
 * 3. 一键启动本机 8782 网关
 *
 * 集成位置：在 Codex ProviderList 上方作为一个特殊「一键装」卡片。
 *
 * 设计取舍：
 * - 健康探针每 15s 轮询（[`useGatewayHealth`]），状态变红立即可见。
 * - 不重复渲染已存在的 ProviderCard；本卡只关注「bundle 入口 + 健康」。
 * - 安装后不主动 invalidate 当前 provider 列表，让用户继续操作；点击
 *   provider 列表的 refresh 才会刷新。
 */
export function KaixuanBundleCard() {
  const { data: endpoints } = useGatewayEndpoints();
  const { data: health, isLoading: healthLoading } = useGatewayHealth();
  const install = useInstallKaixuanBundle();
  const startLocal = useStartLocalGateway();

  // 用户输入的 primary API key（占位符形式：$MY_KEY 或 sk-raw-...）。
  const [primaryKey, setPrimaryKey] = useState("");
  // 用户输入的 secondary API key（一般与 primary 相同，留空则复用）
  const [secondaryKey, setSecondaryKey] = useState("");

  const healthById = useMemo(() => {
    const map = new Map<string, GatewayHealth>();
    for (const h of health ?? []) {
      map.set(h.endpointId, h);
    }
    return map;
  }, [health]);

  // 实际探测的本机端口。后端从 `KAIXUAN_LOCAL_GATEWAY_PORT` 推导（默认 8782），
  // 端点还没加载出来时先退回 8782 —— 只是文案占位，端点到达后立刻被真值替换。
  //
  // 为什么不用字面量：端口是**运行时**值。用户在 ~/.zshenv 里改了 env var 后
  // 界面仍写「本地 8782」会让人以为「改 env var 没生效」，其实探针早就打在新
  // 端口上了，只是文案没跟上。端点的 `localPort` 由后端权威给出。
  const localPort =
    endpoints?.find((ep) => ep.role === "secondary")?.localPort ?? 8782;

  const onInstall = () => {
    const apiKeys: Record<string, string> = {};
    if (primaryKey.trim()) apiKeys.primary = primaryKey.trim();
    if (secondaryKey.trim()) apiKeys.secondary = secondaryKey.trim();
    install.mutate({
      bundleId: "kaixuan",
      appType: "codex",
      apiKeys,
    });
  };

  return (
    <Card
      data-testid="kaixuan-bundle-card"
      className="border-dashed border-cyan-500/40 bg-cyan-50/30 dark:bg-cyan-950/20"
    >
      <CardContent className="p-4 space-y-3">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-base font-semibold">kaixuan bundle</h3>
              <span className="inline-flex items-center rounded-md bg-cyan-100 px-1.5 py-0.5 text-[10px] font-semibold text-cyan-700 dark:bg-cyan-900/40 dark:text-cyan-300">
                一键装双网关
              </span>
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              装好后开轩 kxpms.cn 是主端点，本地 {localPort} 自动接管故障；
              也支持 <code className="bg-muted px-1 rounded">$MY_ENV_VAR</code>{" "}
              占位符从环境变量读密钥。
            </p>
          </div>
          <Button
            size="sm"
            variant="default"
            onClick={onInstall}
            disabled={install.isPending}
          >
            {install.isPending ? (
              <>
                <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
                安装中
              </>
            ) : (
              "安装 / 重装"
            )}
          </Button>
        </div>

        {/* 子端点健康灯 */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
          {(endpoints ?? []).map((ep) => {
            const h = healthById.get(ep.id);
            const isPrimary = ep.role === "primary";
            return (
              <div
                key={ep.id}
                className="flex items-center justify-between rounded-md border bg-background/60 px-3 py-2"
              >
                <div className="flex items-center gap-2 min-w-0">
                  <HealthDot
                    reachable={h?.reachable ?? false}
                    loading={healthLoading && !h}
                  />
                  <div className="min-w-0">
                    <div className="text-sm font-medium truncate">
                      {ep.label}
                      {isPrimary && (
                        <span className="ml-1.5 text-[10px] text-muted-foreground">
                          · 主
                        </span>
                      )}
                    </div>
                    <div className="text-[11px] text-muted-foreground truncate">
                      {/* 端口徽章：把「我现在探的是 X 端口」摆在最显眼处。
                          探针 URL 也一并展示——用户怀疑「到底打的是哪个口」时，
                          这里给的是完整答案而不是一个需要推断的数字。 */}
                      {ep.localPort ? (
                        <span
                          data-testid={`gateway-port-${ep.id}`}
                          className="font-mono text-foreground/80"
                        >
                          127.0.0.1:{ep.localPort}
                        </span>
                      ) : null}
                      {ep.localPort ? " · " : ""}
                      {h
                        ? h.reachable
                          ? `${h.httpStatus ?? "200"} · ${h.latencyMs ?? 0}ms`
                          : (h.error ?? "不可达")
                        : "探测中…"}
                    </div>
                  </div>
                </div>
                {!isPrimary && (
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 px-2 text-xs"
                    onClick={() => startLocal.mutate()}
                    disabled={startLocal.isPending}
                    title={`重启本机 ${localPort} 网关`}
                  >
                    {startLocal.isPending ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Play className="h-3.5 w-3.5" />
                    )}
                  </Button>
                )}
              </div>
            );
          })}
        </div>

        {/* API key 输入（可选） */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
          <div className="space-y-1">
            <Label
              htmlFor="kaixuan-key-primary"
              className="text-xs text-muted-foreground"
            >
              主端点 API Key（kxpms.cn）
            </Label>
            <Input
              id="kaixuan-key-primary"
              value={primaryKey}
              onChange={(e) => setPrimaryKey(e.target.value)}
              placeholder="$KAIXUAN_KXPMS_KEY 或 sk-..."
              className="h-8 text-sm font-mono"
            />
          </div>
          <div className="space-y-1">
            <Label
              htmlFor="kaixuan-key-secondary"
              className="text-xs text-muted-foreground"
            >
              备用端点 API Key（本地 {localPort}，留空复用）
            </Label>
            <Input
              id="kaixuan-key-secondary"
              value={secondaryKey}
              onChange={(e) => setSecondaryKey(e.target.value)}
              placeholder="$KAIXUAN_LOCAL_KEY 或 sk-..."
              className="h-8 text-sm font-mono"
            />
          </div>
        </div>

        {/* 安装结果提示 */}
        {install.isSuccess && (
          <div
            className={cn(
              "rounded-md px-3 py-2 text-xs flex items-start gap-2",
              install.data.missingEnvVars.length === 0
                ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-950/30 dark:text-emerald-300"
                : "bg-amber-50 text-amber-700 dark:bg-amber-950/30 dark:text-amber-300",
            )}
          >
            {install.data.missingEnvVars.length === 0 ? null : (
              <AlertTriangle className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
            )}
            <div>
              <div>
                P1 = <code>{install.data.primaryProviderId}</code>， 已装{" "}
                {install.data.installedProviderIds.length} 个端点，
                auto_failover={String(install.data.autoFailoverEnabled)}
              </div>
              {install.data.missingEnvVars.length > 0 && (
                <div className="mt-1">
                  缺失环境变量：
                  {install.data.missingEnvVars.map((v) => (
                    <code
                      key={v}
                      className="ml-1 bg-amber-100 dark:bg-amber-900/50 px-1 rounded"
                    >
                      ${v}
                    </code>
                  ))}
                  ；启动前请在 ~/.zshenv 写入。
                </div>
              )}
            </div>
          </div>
        )}

        {install.isError && (
          <div className="rounded-md bg-red-50 px-3 py-2 text-xs text-red-700 dark:bg-red-950/30 dark:text-red-300">
            安装失败：{(install.error as Error).message}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function HealthDot({
  reachable,
  loading,
}: {
  reachable: boolean;
  loading?: boolean;
}) {
  if (loading) {
    return (
      <span
        className="h-2.5 w-2.5 rounded-full bg-muted-foreground/40 animate-pulse flex-shrink-0"
        aria-label="健康状态：探测中"
      />
    );
  }
  return reachable ? (
    <Wifi
      className="h-4 w-4 text-emerald-600 flex-shrink-0"
      aria-label="健康状态：可达"
    />
  ) : (
    <WifiOff
      className="h-4 w-4 text-red-500 flex-shrink-0"
      aria-label="健康状态：不可达"
    />
  );
}
