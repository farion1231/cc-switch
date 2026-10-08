import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useQueries } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import { ChevronRight, Loader2 } from "lucide-react";
import type { AppId } from "@/lib/api";
import { proxyApi } from "@/lib/api/proxy";
import { useProvidersQuery } from "@/lib/query";
import {
  useGlobalProxyConfig,
  useProxyStatusQuery,
  useUpdateGlobalProxyConfig,
} from "@/lib/query/proxy";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { PROXY_APP_IDS, type ProxyAppId } from "@/config/appConfig";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { HelpTip, DisabledReason } from "@/components/ui/help-tip";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { MODE_TONE } from "@/components/providers/mode/ModeTabs";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";
import type { AppMode } from "@/types/proxy";
import { cn } from "@/lib/utils";

interface RoutingSectionProps {
  /** 「前往」打开这个应用的供应商页 */
  onOpenApp: (app: AppId) => void;
}

const isValidListenAddress = (address: string): boolean => {
  if (address === "localhost" || address === "0.0.0.0") return true;
  if (/^(\d{1,3}\.){3}\d{1,3}$/.test(address)) {
    return address.split(".").every((part) => Number(part) <= 255);
  }
  // IPv6 字面量：必须含 `:` 且能在 [..] 包装后被 URL 解析器接受（后端会把 `::` 改写成 `::1`）
  if (!address.includes(":")) return false;
  try {
    new URL(`http://[${address}]/`);
    return true;
  } catch {
    return false;
  }
};

function formatUptime(seconds: number, t: TFunction) {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return hours > 0
    ? t("routingSettings.uptimeHours", { hours, minutes })
    : t("routingSettings.uptimeMinutes", { minutes: Math.max(minutes, 0) });
}

/**
 * 设置 → 本地路由（v7）：服务（状态、统计、启停、监听地址、请求日志）→ 正在使用路由的应用
 * → 全部回到直连 → 故障转移参数 → 整流与优化。进出路由在各应用页的模式 tab 里。
 */
export function RoutingSection({ onOpenApp }: RoutingSectionProps) {
  const { t } = useTranslation();
  const { data: status } = useProxyStatusQuery();
  const running = status?.running ?? false;
  const { startProxyServer, stopProxyServer, stopWithRestore, isPending } =
    useProxyStatus();
  const { data: config } = useGlobalProxyConfig();
  const updateConfig = useUpdateGlobalProxyConfig();
  const [address, setAddress] = useState("127.0.0.1");
  const [port, setPort] = useState("15721");
  const [addressError, setAddressError] = useState<string | null>(null);
  const [portError, setPortError] = useState<string | null>(null);
  const [confirmExitAll, setConfirmExitAll] = useState(false);
  const [failoverApp, setFailoverApp] = useState<ProxyAppId>("claude");

  useEffect(() => {
    if (config) {
      setAddress(config.listenAddress);
      setPort(String(config.listenPort));
    }
  }, [config]);

  const modeQueries = useQueries({
    queries: PROXY_APP_IDS.map((app) => ({
      queryKey: ["providers", app, "mode"],
      queryFn: () => proxyApi.getAppMode(app),
    })),
  });
  const providerQueries = {
    claude: useProvidersQuery("claude").data,
    codex: useProvidersQuery("codex").data,
    gemini: useProvidersQuery("gemini").data,
    grokbuild: useProvidersQuery("grokbuild").data,
  };
  const { data: desktop } = useProvidersQuery("claude-desktop");
  const desktopCurrent = desktop?.providers[desktop.currentProviderId];
  const desktopMapping = desktopCurrent
    ? providerNeedsRouting("claude-desktop", desktopCurrent)
    : false;

  const usingApps = PROXY_APP_IDS.flatMap((app, index) => {
    const view = modeQueries[index]?.data;
    if (!view || view.mode === "direct") return [];
    const routeName =
      (view.routeProviderId &&
        providerQueries[app]?.providers[view.routeProviderId]?.name) ||
      "—";
    return [{ app, mode: view.mode as AppMode, routeName }];
  });
  const usingCount = usingApps.length + (desktopMapping ? 1 : 0);

  const saveListen = async () => {
    if (!config) return;
    const trimmed = address.trim();
    const portValue = Number(port.trim());
    const addressOk = isValidListenAddress(trimmed);
    const portOk =
      /^\d+$/.test(port.trim()) && portValue >= 1024 && portValue <= 65535;
    setAddressError(addressOk ? null : t("proxy.settings.invalidAddress"));
    setPortError(portOk ? null : t("proxy.settings.invalidPort"));
    if (!addressOk || !portOk) return;
    try {
      await updateConfig.mutateAsync({
        ...config,
        listenAddress: trimmed === "localhost" ? "127.0.0.1" : trimmed,
        listenPort: portValue,
      });
    } catch {
      // useUpdateGlobalProxyConfig 的 onSuccess / onError 已经弹过 toast
    }
  };

  const listenDirty =
    !!config &&
    (address.trim() !== config.listenAddress ||
      port.trim() !== String(config.listenPort));

  const toggleLogging = async (enabled: boolean) => {
    if (!config) return;
    try {
      await updateConfig.mutateAsync({ ...config, enableLogging: enabled });
    } catch {
      toast.error(t("proxy.logging.failed"));
    }
  };

  const stopReason =
    running && usingCount > 0 ? t("routingSettings.stopBlocked") : undefined;

  return (
    <>
      <SettingsBlock title={t("routingSettings.service")}>
        <SettingsCard>
          <div className="flex flex-wrap items-center gap-x-6 gap-y-3 px-4 py-3.5">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1 text-body font-medium text-fg-1">
                {t("routingSettings.serviceName")}
                <HelpTip title={t("routingSettings.serviceName")}>
                  {t("routingSettings.serviceHelp")}
                </HelpTip>
              </div>
              <div className="mt-0.5 flex items-center gap-1.5 text-caption text-fg-2">
                <span
                  aria-hidden="true"
                  className={cn(
                    "h-1.5 w-1.5 rounded-full",
                    running ? "bg-route" : "bg-fg-3",
                  )}
                />
                {running
                  ? t("routingSettings.runningFor", {
                      uptime: formatUptime(status?.uptime_seconds ?? 0, t),
                    })
                  : t("routingSettings.stopped")}
              </div>
            </div>
            {running && (
              <dl className="flex gap-6 text-end">
                {[
                  [
                    t("routingSettings.totalRequests"),
                    status?.total_requests ?? 0,
                  ],
                  [
                    t("routingSettings.successRate"),
                    `${(status?.success_rate ?? 0).toFixed(1)}%`,
                  ],
                  [
                    t("routingSettings.activeConnections"),
                    status?.active_connections ?? 0,
                  ],
                ].map(([label, value]) => (
                  <div key={String(label)}>
                    <dt className="text-caption text-fg-2">{label}</dt>
                    <dd className="m-0 text-strong font-semibold tabular-nums text-fg-1">
                      {typeof value === "number"
                        ? value.toLocaleString()
                        : value}
                    </dd>
                  </div>
                ))}
              </dl>
            )}
            {running ? (
              <DisabledReason reason={stopReason} align="end">
                <Button
                  variant="neutral"
                  size="regular"
                  disabled={isPending}
                  onClick={() => void stopProxyServer()}
                >
                  {t("routingSettings.stop")}
                </Button>
              </DisabledReason>
            ) : (
              <Button
                variant="neutral"
                size="regular"
                disabled={isPending}
                onClick={() => void startProxyServer()}
              >
                {t("routingSettings.start")}
              </Button>
            )}
          </div>

          <SettingsRow
            label={t("routingSettings.listen")}
            control={
              <div className="flex items-start gap-3">
                <label className="space-y-1">
                  <span className="block text-caption font-medium text-fg-2">
                    {t("routingSettings.address")}
                  </span>
                  <Input
                    value={address}
                    onChange={(event) => setAddress(event.target.value)}
                    aria-invalid={addressError ? true : undefined}
                    className="w-[150px]"
                  />
                  <span
                    className={cn(
                      "block text-caption",
                      addressError ? "text-danger-text" : "text-fg-3",
                    )}
                  >
                    {addressError ?? t("routingSettings.addressHint")}
                  </span>
                </label>
                <label className="space-y-1">
                  <span className="block text-caption font-medium text-fg-2">
                    {t("routingSettings.port")}
                  </span>
                  <Input
                    value={port}
                    inputMode="numeric"
                    onChange={(event) => setPort(event.target.value)}
                    aria-invalid={portError ? true : undefined}
                    className="w-[96px]"
                  />
                  <span
                    className={cn(
                      "block text-caption",
                      portError ? "text-danger-text" : "text-fg-3",
                    )}
                  >
                    {portError ?? "1024–65535"}
                  </span>
                </label>
                <Button
                  variant="neutral"
                  size="regular"
                  // 和输入框顶边对齐：上面的小标题 18 + 间距 4
                  className="mt-[22px]"
                  disabled={!listenDirty || updateConfig.isPending}
                  onClick={() => void saveListen()}
                >
                  {updateConfig.isPending && (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  )}
                  {t("routingSettings.saveAndRestart")}
                </Button>
              </div>
            }
          />
          <SettingsSwitchRow
            label={t("routingSettings.logging")}
            help={{
              title: t("routingSettings.logging"),
              body: t("routingSettings.loggingHelp"),
            }}
            checked={config?.enableLogging ?? false}
            disabled={!config}
            onCheckedChange={(value) => void toggleLogging(value)}
          />
        </SettingsCard>
      </SettingsBlock>

      <SettingsBlock
        title={t("routingSettings.usingApps", { count: usingCount })}
      >
        {usingCount === 0 ? (
          <div className="rounded-panel border border-dashed border-border px-4 py-3 text-caption text-fg-3">
            {t("routingSettings.noApps")}
          </div>
        ) : (
          <SettingsCard>
            {usingApps.map(({ app, mode, routeName }) => (
              <UsingAppRow
                key={app}
                app={app}
                modeLabel={t(`mode.names.${mode}`)}
                dotClass={MODE_TONE[mode].dot}
                detail={
                  mode === "stack"
                    ? t("routingSettings.stackDetail", { name: routeName })
                    : t("routingSettings.routeDetail", { name: routeName })
                }
                onOpen={() => onOpenApp(app)}
              />
            ))}
            {desktopMapping && desktopCurrent && (
              <UsingAppRow
                app="claude-desktop"
                modeLabel={t("routingSettings.desktopMode")}
                dotClass="bg-route"
                detail={t("routingSettings.desktopDetail", {
                  name: desktopCurrent.name,
                })}
                onOpen={() => onOpenApp("claude-desktop")}
              />
            )}
          </SettingsCard>
        )}
        {usingApps.length > 0 && (
          <Button
            variant="neutral"
            size="regular"
            onClick={() => setConfirmExitAll(true)}
          >
            {t("routingSettings.exitAll")}
          </Button>
        )}
      </SettingsBlock>

      <SettingsBlock
        title={t("settings.advanced.failover.title")}
        help={{
          title: t("settings.advanced.failover.title"),
          body: t("routingSettings.failoverHelp"),
        }}
      >
        <div className="space-y-4 rounded-panel border border-border bg-surface p-5">
          <SegmentedControl
            size="sm"
            aria-label={t("settings.advanced.failover.title")}
            value={failoverApp}
            onValueChange={setFailoverApp}
            items={PROXY_APP_IDS.map((app) => ({
              value: app,
              label: APP_DISPLAY_NAME[app],
            }))}
          />
          <AutoFailoverConfigPanel key={failoverApp} appType={failoverApp} />
        </div>
      </SettingsBlock>

      <SettingsBlock
        title={t("settings.advanced.rectifier.title")}
        help={{
          title: t("settings.advanced.rectifier.title"),
          body: t("settings.advanced.rectifier.description"),
        }}
      >
        <div className="rounded-panel border border-border bg-surface p-5">
          <RectifierConfigPanel />
        </div>
      </SettingsBlock>

      {/* 可恢复的操作（路由目标、队列、聚合名单都保留），不用红色 */}
      <ConfirmDialog
        isOpen={confirmExitAll}
        variant="info"
        title={t("routingSettings.exitAllTitle")}
        message={t("routingSettings.exitAllMessage")}
        confirmText={t("routingSettings.exitAllConfirm")}
        onConfirm={() => {
          setConfirmExitAll(false);
          void stopWithRestore();
        }}
        onCancel={() => setConfirmExitAll(false)}
      />
    </>
  );
}

function UsingAppRow({
  app,
  modeLabel,
  dotClass,
  detail,
  onOpen,
}: {
  app: AppId;
  modeLabel: string;
  dotClass: string;
  detail: string;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-3 px-4 py-2.5">
      <AppGlyph app={app} size={18} badgeClassName="bg-surface" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-body font-medium text-fg-1">
          {APP_DISPLAY_NAME[app]}
          <span className="text-fg-3">·</span>
          <span
            aria-hidden="true"
            className={cn("h-1.5 w-1.5 rounded-full", dotClass)}
          />
          <span className="font-normal text-fg-2">{modeLabel}</span>
        </div>
        <div className="truncate text-caption text-fg-2">{detail}</div>
      </div>
      <Button variant="quiet" size="compact" onClick={onOpen}>
        {t("routingSettings.goTo")}
        <ChevronRight className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
