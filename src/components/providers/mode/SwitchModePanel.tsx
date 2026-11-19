import { useEffect, useMemo, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { Provider } from "@/types";
import type { AppMode, StartupAttachFailure } from "@/types/proxy";
import { authApi, settingsApi } from "@/lib/api";
import type { ManagedAuthProvider, ManagedAuthStatus } from "@/lib/api/auth";
import { providersApi } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
import { useSettingsQuery } from "@/lib/query";
import {
  proxyKeys,
  useAppMode,
  useProxyStack,
  useProxyStatusQuery,
  useSetProxyStackMember,
} from "@/lib/query/proxy";
import {
  useAddToFailoverQueue,
  useAutoFailoverEnabled,
  useFailoverQueue,
  useRemoveFromFailoverQueue,
  useSetAutoFailoverEnabled,
} from "@/lib/query/failover";
import { useModeActions } from "@/hooks/useModeActions";
import { getRoutingReason } from "@/utils/routingReason";
import {
  CODEX_OFFICIAL_PROVIDER_ID,
  isOfficialAccount,
} from "@/utils/providerCapabilities";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { routeSwitchNotification } from "@/utils/routeSwitchNotification";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderList } from "@/components/providers/ProviderList";
import { CodexStaleClientsNotice } from "@/components/providers/CodexStaleClientsNotice";
import {
  blockedFromRouting,
  queueMoveSortUpdates,
} from "@/components/providers/presentation";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { ModeDialog, type ModeDialogState } from "./ModeDialog";
import {
  sameStackSubagent,
  stackSubagentSettings,
  stackSubagentValue,
  type StackSubagentValue,
} from "./StackSubagentFields";
import { RouteSettingsSheet } from "./RouteSettingsSheet";
import { StackSettingsSheet } from "./StackSettingsSheet";
import { useCodexPauseOfficialLogin } from "./CodexPauseOfficialLogin";
import { useDesktopConfigMessages } from "./useDesktopConfigMessages";
import { type AppIdWith, appHas } from "@/config/appSpecs";

type ListCallbacks = Pick<
  React.ComponentProps<typeof ProviderList>,
  | "onEdit"
  | "onDelete"
  | "onDuplicate"
  | "onConfigureUsage"
  | "onOpenWebsite"
  | "onOpenTerminal"
  | "onCreate"
  | "searchOpen"
  | "onSearchOpenChange"
>;

interface SwitchModePanelProps extends ListCallbacks {
  app: AppIdWith<"routeMode">;
  providers: Record<string, Provider>;
  currentProviderId: string;
  isLoading: boolean;
  scrollRef?: RefObject<HTMLDivElement>;
  onSwitch: (
    provider: Provider,
    options?: { acknowledgedRouting?: boolean },
  ) => unknown;
  onOpenRoutingSettings: () => void;
  onOpenAuthCenter?: () => void;
  onOpenAggregation?: () => void;
  startupFailure?: StartupAttachFailure;
  onDismissStartupFailure?: () => void;
  /** 托盘里点了直连下需要路由的那家：到这页后弹同一个「需要路由」对话框 */
  needsRouteRequest?: { providerId: string; nonce: number };
  onNeedsRouteHandled?: () => void;
}

const SYNTHETIC_AUTH_PREFIX = "__auth_";

function isSyntheticAuthProviderId(id: string): boolean {
  return id.startsWith(SYNTHETIC_AUTH_PREFIX);
}

function buildSyntheticAuthProvider(
  app: AppIdWith<"routeMode">,
  authProvider: ManagedAuthProvider,
  accountId: string,
  login: string,
): Provider {
  const settingsConfig =
    app === "codex"
      ? {
          auth: { OPENAI_API_KEY: null },
          config: "",
        }
      : {};
  return {
    id: `${SYNTHETIC_AUTH_PREFIX}${authProvider}_${accountId}`,
    name: login,
    category: "official",
    settingsConfig,
    meta: {
      authBinding: {
        source: "managed_account",
        authProvider,
        accountId,
      },
    },
  };
}

/**
 * 切换式应用的供应商页（v7 S1–S3）：模式行 → 通知槽 → 按查看的模式分区的列表。
 * tab 只负责查看；要生效必须点写明后果的按钮（确认框），回到直连也一样。
 */
export function SwitchModePanel({
  app,
  providers,
  currentProviderId,
  isLoading,
  scrollRef,
  onSwitch,
  onOpenRoutingSettings,
  onOpenAuthCenter,
  onOpenAggregation,
  startupFailure,
  onDismissStartupFailure,
  needsRouteRequest,
  onNeedsRouteHandled,
  ...listCallbacks
}: SwitchModePanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const appName = APP_DISPLAY_NAME[app];

  const { data: modeView } = useAppMode(app);
  const active: AppMode =
    modeView &&
    (modeView.mode === "direct" ||
      modeView.mode === "route" ||
      modeView.mode === "stack")
      ? modeView.mode
      : "direct";
  const directId = modeView?.directProviderId ?? null;
  const routeId = modeView?.routeProviderId ?? null;
  const view: AppMode = "direct";

  const managedAuthProvider: ManagedAuthProvider | null =
    app === "claude"
      ? "claude_official"
      : app === "codex"
        ? "codex_oauth"
        : null;

  const { data: authStatus, isPending: isAuthPending } =
    useQuery<ManagedAuthStatus>({
      queryKey: ["managed-auth-status", managedAuthProvider],
      queryFn: () => authApi.authGetStatus(managedAuthProvider!),
      enabled: Boolean(managedAuthProvider),
      staleTime: 30_000,
    });

  const officialProviders = useMemo(() => {
    const out: Record<string, Provider> = {};
    if (managedAuthProvider) {
      const boundByAccountId = new Map<string, Provider>();
      for (const provider of Object.values(providers)) {
        if (!isOfficialAccount(app, provider)) continue;
        const accountId = resolveManagedAccountId(
          provider.meta,
          managedAuthProvider,
        );
        if (accountId && !boundByAccountId.has(accountId)) {
          boundByAccountId.set(accountId, provider);
        }
      }
      for (const account of authStatus?.accounts ?? []) {
        const existing = boundByAccountId.get(account.id);
        if (existing) {
          out[existing.id] = existing;
        } else {
          const synthetic = buildSyntheticAuthProvider(
            app,
            managedAuthProvider,
            account.id,
            account.login,
          );
          out[synthetic.id] = synthetic;
        }
      }
      return out;
    }

    for (const [id, provider] of Object.entries(providers)) {
      if (isOfficialAccount(app, provider)) {
        out[id] = provider;
      }
    }
    return out;
  }, [providers, app, managedAuthProvider, authStatus]);

  const allProvidersMap = useMemo(
    () => ({ ...providers, ...officialProviders }),
    [providers, officialProviders],
  );

  const fallbackDirectId = useMemo(() => {
    if (directId && officialProviders[directId]) return directId;
    const firstOfficial = Object.keys(officialProviders)[0];
    if (firstOfficial) return firstOfficial;
    if (directId && providers[directId]) return directId;
    return (
      Object.values(providers).find((provider) =>
        isOfficialAccount(app, provider),
      )?.id ?? directId
    );
  }, [directId, officialProviders, providers, app]);

  const { data: proxyStatus } = useProxyStatusQuery();
  const serviceRunning = proxyStatus?.running ?? false;
  const { data: settings } = useSettingsQuery();
  const pauseOfficialLogin = useCodexPauseOfficialLogin();
  // Claude Desktop 没有故障转移和路由设置（没有 proxy_config 行）
  const hasFailover = appHas(app, "localProxy");
  const { data: failoverOn = false } = useAutoFailoverEnabled(app, hasFailover);
  const { data: queue } = useFailoverQueue(app, hasFailover);
  const setFailover = useSetAutoFailoverEnabled();
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();
  const { data: stack } = useProxyStack(app, appHas(app, "stack"));
  const setStackMember = useSetProxyStackMember();
  const modeActions = useModeActions(app);
  const desktopMessages = useDesktopConfigMessages(app === "claude-desktop");

  const [dialog, setDialog] = useState<ModeDialogState | null>(null);
  const [routeSettingsOpen, setRouteSettingsOpen] = useState(false);
  const [stackSettingsOpen, setStackSettingsOpen] = useState(false);
  const [confirmFailover, setConfirmFailover] = useState(false);
  // 关掉的是哪一份提示（后端的 staleRevision）：目录或登录又变了就是新的一份，重新显示。
  const [staleDismissed, setStaleDismissed] = useState<string | null>(null);
  useEffect(() => {
    setStaleDismissed(null);
  }, [app, active, directId, routeId, stack?.staleClients?.auth]);
  const staleRevision = stack?.staleRevision ?? "";

  // 供应商还没加载完时先等着，到了再弹
  useEffect(() => {
    if (!needsRouteRequest) return;
    const provider = providers[needsRouteRequest.providerId];
    if (!provider) return;
    setDialog({
      kind: "needsRoute",
      providerId: provider.id,
      reason: getRoutingReason(app, provider, t) ?? "",
    });
    onNeedsRouteHandled?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [needsRouteRequest?.nonce, providers]);

  const providerList = useMemo(
    () => Object.values(allProvidersMap),
    [allProvidersMap],
  );
  const nameOf = (id: string | null) =>
    (id && allProvidersMap[id]?.name) || t("mode.noProvider");

  const materializeProvider = async (provider: Provider): Promise<Provider> => {
    if (!isSyntheticAuthProviderId(provider.id) || !managedAuthProvider) {
      return provider;
    }
    const accountId = resolveManagedAccountId(
      provider.meta,
      managedAuthProvider,
    );
    const realProvider: Provider = {
      ...provider,
      id: `${managedAuthProvider}-${accountId ?? provider.id.slice(SYNTHETIC_AUTH_PREFIX.length)}`,
    };
    await providersApi.add(realProvider, app, false);
    await queryClient.invalidateQueries({ queryKey: ["providers", app] });
    return realProvider;
  };

  const handleSwitchProvider = async (
    provider: Provider,
    options?: { acknowledgedRouting?: boolean },
  ) => {
    const real = await materializeProvider(provider);
    return await onSwitch(real, options);
  };

  const stackMembers = useMemo(() => {
    const members = new Map<string, number>();
    for (const member of stack?.members ?? []) {
      members.set(member.providerId, member.modelIds.length);
    }
    return members;
  }, [stack?.members]);

  const queueIds = useMemo(
    () => (queue ?? []).map((item) => item.providerId),
    [queue],
  );

  const exitAndUse = (provider: Provider) =>
    setDialog({ kind: "exit", useProviderId: provider.id });

  const queueMove = async (provider: Provider, delta: -1 | 1) => {
    const updates = queueMoveSortUpdates(
      queue ?? [],
      providers,
      provider.id,
      delta,
    );
    if (!updates) return;
    const target = updates.findIndex((update) => update.id === provider.id);
    try {
      await providersApi.updateSortOrder(updates, app);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["failoverQueue", app] }),
        queryClient.invalidateQueries({ queryKey: ["providers", app] }),
      ]);
      toast.success(
        t("mode.queueMoved", { name: provider.name, position: target + 1 }),
      );
    } catch (error) {
      toast.error(extractErrorMessage(error) || t("provider.sortUpdateFailed"));
    }
  };

  const rememberDefault = async (provider: Provider) => {
    try {
      const result = await proxyApi.setProxyRoute(app, provider.id);
      await queryClient.invalidateQueries({ queryKey: ["providers", app] });
      toast.success(routeSwitchNotification(result, app, provider.name, t), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(
        t("mode.toast.failed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    }
  };

  const switchMode = {
    active,
    view,
    directId,
    routeId,
    failoverOn,
    queue: queueIds,
    stackMembers,
    serviceRunning,
    routingReason: (provider: Provider) =>
      getRoutingReason(app, provider, t) ?? "",
    actions: {
      switchDirect: (provider: Provider) => void handleSwitchProvider(provider),
      needsRouteDialog: (provider: Provider) =>
        setDialog({
          kind: "needsRoute",
          providerId: provider.id,
          reason: getRoutingReason(app, provider, t) ?? "",
        }),
      exitAndUse,
      routeTo: (provider: Provider) => void handleSwitchProvider(provider),
      queueAdd: (provider: Provider) =>
        addToQueue.mutate({ appType: app, providerId: provider.id }),
      queueRemove: (provider: Provider) =>
        removeFromQueue.mutate({ appType: app, providerId: provider.id }),
      queueMove: (provider: Provider, delta: -1 | 1) =>
        void queueMove(provider, delta),
      stackAdd: (provider: Provider) => {
        setStackMember.mutate({
          appType: app,
          providerId: provider.id,
          enabled: true,
        });
      },
      stackRemove: (provider: Provider) => {
        setStackMember.mutate({
          appType: app,
          providerId: provider.id,
          enabled: false,
        });
      },
      // 聚合生效时换默认当场生效（同切换供应商）；没生效时只记下选择，切换时的确认框预选它
      stackSetDefault: (provider: Provider) =>
        active === "stack"
          ? void onSwitch(provider)
          : void rememberDefault(provider),
    },
  };

  const confirmFailoverOn = async () => {
    setConfirmFailover(false);
    try {
      const current = await settingsApi.get();
      await settingsApi.save({
        ...current,
        failoverConfirmed: true,
      });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error(
        "[SwitchModePanel] Failed to save failover confirmation",
        error,
      );
    }
    setFailover.mutate({ appType: app, enabled: true });
  };

  const enterMode = async (
    target: Exclude<AppMode, "direct">,
    pick: string,
    subagent?: StackSubagentValue,
  ) => {
    // 框里改了子代理设置就先存（聚合还没生效时后端只存不写），进入聚合时随契约一起写进去
    if (
      subagent &&
      !sameStackSubagent(subagent, stackSubagentValue(settings))
    ) {
      const current = await settingsApi.get();
      await settingsApi.save({
        ...current,
        ...stackSubagentSettings(subagent),
      });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }
    await modeActions.enter(target, pick, providers[pick]?.name);
  };

  const eligibleIds = providerList
    .filter((provider) => !blockedFromRouting(app, provider))
    .map((provider) => provider.id);

  const startService = async () => {
    try {
      await proxyApi.startProxyServer();
      await queryClient.invalidateQueries({ queryKey: proxyKeys.status });
    } catch (error) {
      toast.error(
        t("proxy.server.startFailed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    }
  };

  const notices: React.ReactNode[] = [];
  if (active !== "direct" && proxyStatus !== undefined && !serviceRunning) {
    notices.push(
      <Notice
        key="broken"
        tone="warning"
        title={t("mode.notice.serviceDown", {
          app: appName,
          mode: t(`mode.names.${active}`),
        })}
        actions={
          <>
            <Button
              variant="neutral"
              size="compact"
              onClick={() => void startService()}
            >
              {t("mode.notice.startService")}
            </Button>
            <Button
              variant="neutral"
              size="compact"
              onClick={() => setDialog({ kind: "exit" })}
            >
              {t("mode.notice.backToDirect")}
            </Button>
          </>
        }
      />,
    );
  }
  if (desktopMessages.length > 0) {
    notices.push(
      <Notice
        key="desktopConfig"
        tone="warning"
        title={t("claudeDesktop.statusTitle")}
      >
        {desktopMessages.map((message) => (
          <span key={message} className="block">
            {message}
          </span>
        ))}
      </Notice>,
    );
  }
  if (startupFailure) {
    notices.push(
      <Notice
        key="startup"
        tone="warning"
        title={t("mode.notice.attachFailed", {
          app: appName,
          mode: t(`mode.names.${startupFailure.stack ? "stack" : "route"}`),
        })}
        onDismiss={onDismissStartupFailure}
        dismissLabel={t("common.close")}
        actions={
          <Button
            variant="neutral"
            size="compact"
            onClick={() =>
              void modeActions
                .enter(
                  startupFailure.stack ? "stack" : "route",
                  routeId,
                  nameOf(routeId),
                )
                .then(() => onDismissStartupFailure?.())
                .catch((error) =>
                  toast.error(
                    t("mode.toast.failed", {
                      detail: extractErrorMessage(error) || t("common.unknown"),
                    }),
                  ),
                )
            }
          >
            {t("common.retry")}
          </Button>
        }
      >
        {startupFailure.error}
      </Notice>,
    );
  }
  if (
    app === "codex" &&
    stack?.staleClients &&
    (stack.staleClients.daemon || stack.staleClients.others) &&
    staleDismissed !== staleRevision
  ) {
    notices.push(
      <CodexStaleClientsNotice
        key="stale"
        staleClients={stack.staleClients}
        onDismiss={() => setStaleDismissed(staleRevision)}
      />,
    );
  }
  // 暂停使用官方登录只在路由 / 聚合下生效；开着就一直提示，免得额度恢复后忘了关
  if (app === "codex" && active !== "direct" && pauseOfficialLogin.paused) {
    notices.push(
      <Notice
        key="officialLoginPaused"
        tone="warning"
        title={t("mode.notice.officialLoginPaused")}
        actions={
          <Button
            variant="neutral"
            size="compact"
            disabled={pauseOfficialLogin.saving}
            onClick={() => void pauseOfficialLogin.setPaused(false)}
          >
            {t("mode.notice.resumeOfficialLogin")}
          </Button>
        }
      >
        {t("mode.notice.officialLoginPausedBody")}
      </Notice>,
    );
  }
  if (active !== "direct") {
    notices.push(
      <Notice
        key="globalStackNotice"
        tone="neutral"
        title={t("aggregation.appManagedNotice", {
          app: appName,
          defaultValue: `${appName} 当前由「聚合」接管外部供应商。`,
        })}
        actions={
          <>
            {onOpenAggregation && (
              <Button
                variant="stack"
                size="compact"
                onClick={onOpenAggregation}
              >
                {t("aggregation.goToPage", { defaultValue: "前往聚合页" })}
              </Button>
            )}
            <Button
              variant="neutral"
              size="compact"
              onClick={() => setDialog({ kind: "exit" })}
            >
              {t("mode.notice.backToDirect")}
            </Button>
          </>
        }
      />,
    );
  }

  return (
    <>
      <div
        ref={scrollRef}
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable overflow-x-hidden px-6 pb-12 pt-4"
      >
        {notices.length > 0 && (
          <div role="status" className="mb-3 flex flex-col gap-2">
            {notices}
          </div>
        )}
        <ProviderList
          {...listCallbacks}
          onOpenTerminal={
            listCallbacks.onOpenTerminal
              ? (provider) => {
                  void materializeProvider(provider).then((real) =>
                    listCallbacks.onOpenTerminal?.(real),
                  );
                }
              : undefined
          }
          providers={officialProviders}
          currentProviderId={currentProviderId}
          appId={app}
          isLoading={isLoading || (Boolean(managedAuthProvider) && isAuthPending)}
          onSwitch={(provider) => void handleSwitchProvider(provider)}
          switchMode={switchMode}
          officialOnly
          onOpenAuthCenter={onOpenAuthCenter}
          onOpenAggregation={onOpenAggregation}
        />
      </div>

      <ModeDialog
        app={app}
        state={dialog}
        active={active}
        providers={providerList}
        directId={fallbackDirectId}
        eligibleIds={eligibleIds}
        defaultPick={routeId ?? fallbackDirectId}
        stackMembers={(stack?.members ?? []).map((member) => ({
          id: member.providerId,
          name: nameOf(member.providerId),
          models: member.memberModels,
        }))}
        stackSubagent={
          app === "codex" && settings
            ? {
                models: stack?.subagentModels ?? [],
                value: stackSubagentValue(settings),
              }
            : undefined
        }
        onClose={() => setDialog(null)}
        onEnter={enterMode}
        onExit={async (useProviderId) => {
          const targetId = useProviderId ?? fallbackDirectId;
          const target = targetId ? allProvidersMap[targetId] : undefined;
          if (target && isSyntheticAuthProviderId(target.id)) {
            const real = await materializeProvider(target);
            await modeActions.exitToDirect({
              directProviderId: fallbackDirectId,
              useProviderId: real.id,
              providerName: real.name,
            });
            return;
          }
          await modeActions.exitToDirect({
            directProviderId: fallbackDirectId,
            useProviderId,
            providerName: nameOf(targetId),
          });
        }}
        onSwitchDirect={(providerId) => {
          const provider = allProvidersMap[providerId];
          if (provider) {
            void handleSwitchProvider(provider, { acknowledgedRouting: true });
          }
        }}
      />

      {appHas(app, "localProxy") && (
        <RouteSettingsSheet
          app={app}
          open={routeSettingsOpen}
          onOpenChange={setRouteSettingsOpen}
          onOpenSettings={onOpenRoutingSettings}
        />
      )}

      {app === "codex" && (
        <StackSettingsSheet
          open={stackSettingsOpen}
          onOpenChange={setStackSettingsOpen}
          // 默认那家不发布 `ccs-*` 模型
          models={(stack?.subagentModels ?? []).filter(
            (model) => model.providerId !== routeId,
          )}
          // 官方做默认那家时，合并目录里有官方的 codex-auto-review，自动审批默认用它
          routeOfficial={
            !!routeId &&
            (routeId === CODEX_OFFICIAL_PROVIDER_ID ||
              providers[routeId]?.category === "official")
          }
        />
      )}

      <ConfirmDialog
        isOpen={confirmFailover}
        variant="info"
        title={t("confirm.failover.title")}
        message={t("confirm.failover.message")}
        confirmText={t("confirm.failover.confirm")}
        onConfirm={() => void confirmFailoverOn()}
        onCancel={() => setConfirmFailover(false)}
      />
    </>
  );
}
