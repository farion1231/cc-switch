import { createPortal } from "react-dom";
import { badgeVariants } from "@/components/ui/badge";
import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { useEffect, useMemo, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { Provider } from "@/types";
import type { AppMode, StartupAttachFailure } from "@/types/proxy";
import type { ModeAppId } from "@/config/appConfig";
import { isProxyAppId, isStackAppId } from "@/config/appConfig";
import { settingsApi } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
import { useSettingsQuery } from "@/lib/query";
import {
  proxyKeys,
  useAdoptCodexStackCatalog,
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
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { getRoutingReason } from "@/utils/routingReason";
import { CODEX_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
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
import { providerNeedsCursorSetup } from "@/utils/providerCapabilities";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { ModeTabs, MODE_TONE } from "./ModeTabs";
import { ModeDialog, type ModeDialogState } from "./ModeDialog";
import {
  sameStackSubagent,
  stackSubagentSettings,
  stackSubagentValue,
  type StackSubagentValue,
} from "./StackSubagentFields";
import { CursorSetupDialog } from "./CursorSetupDialog";
import { RouteSettingsSheet } from "./RouteSettingsSheet";
import { StackSettingsSheet } from "./StackSettingsSheet";
import { useDesktopConfigMessages } from "./useDesktopConfigMessages";

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
  app: ModeAppId;
  statusSlot?: HTMLDivElement | null;
  providers: Record<string, Provider>;
  currentProviderId: string;
  isLoading: boolean;
  scrollRef?: RefObject<HTMLDivElement>;
  onSwitch: (
    provider: Provider,
    options?: { acknowledgedRouting?: boolean },
  ) => unknown;
  onOpenRoutingSettings: () => void;
  /** 打开设置里的**公网路由**那一栏（与本地路由 `routing` 是两栏，别混用） */
  onOpenPublicRouteSettings: () => void;
  startupFailure?: StartupAttachFailure;
  onDismissStartupFailure?: () => void;
  /** 托盘里点了直连下需要路由的那家：到这页后弹同一个「需要路由」对话框 */
  needsRouteRequest?: { providerId: string; nonce: number };
  onNeedsRouteHandled?: () => void;
  /** 正在看的那格变了：新增 / 编辑供应商按它选表单布局。需要是稳定的回调 */
  onViewChange?: (app: ModeAppId, view: AppMode) => void;
}

/** Gemini CLI、Grok Build、Claude Desktop 没有聚合模式（Q6）：那一格隐藏，前两格位置不变。 */
const modesFor = (app: ModeAppId): AppMode[] =>
  isStackAppId(app) ? ["direct", "route", "stack"] : ["direct", "route"];

/**
 * 切换式应用的供应商页（v7 S1–S3）：模式行 → 通知槽 → 按查看的模式分区的列表。
 * tab 只负责查看；要生效必须点写明后果的按钮（确认框），回到直连也一样。
 */
export function SwitchModePanel({
  statusSlot,
  app,
  providers,
  currentProviderId,
  isLoading,
  scrollRef,
  onSwitch,
  onOpenRoutingSettings,
  onOpenPublicRouteSettings,
  startupFailure,
  onDismissStartupFailure,
  needsRouteRequest,
  onNeedsRouteHandled,
  onViewChange,
  ...listCallbacks
}: SwitchModePanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const appName = APP_DISPLAY_NAME[app];
  const modes = modesFor(app);

  const { data: modeView } = useAppMode(app);
  const active: AppMode =
    modeView && modes.includes(modeView.mode) ? modeView.mode : "direct";
  const directId = modeView?.directProviderId ?? null;
  const routeId = modeView?.routeProviderId ?? null;

  // 离开页面再回来、或生效的模式变了，一律落在生效的那格（「查看中」不持久化）
  const [view, setView] = useState<AppMode>(active);
  const [searchSlot, setSearchSlot] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    setView(active);
  }, [app, active]);
  useEffect(() => {
    onViewChange?.(app, view);
  }, [app, view, onViewChange]);

  const { data: proxyStatus } = useProxyStatusQuery();
  const serviceRunning = proxyStatus?.running ?? false;
  const { data: settings } = useSettingsQuery();
  // Claude Desktop 没有故障转移和路由设置（没有 proxy_config 行）
  const hasFailover = isProxyAppId(app);
  const { data: failoverOn = false } = useAutoFailoverEnabled(app, hasFailover);
  const { data: queue } = useFailoverQueue(app, hasFailover);
  const setFailover = useSetAutoFailoverEnabled();
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();
  const { data: stack } = useProxyStack(app, isStackAppId(app));
  const setStackMember = useSetProxyStackMember();
  const adoptCatalog = useAdoptCodexStackCatalog();
  const modeActions = useModeActions(app);
  const desktopMessages = useDesktopConfigMessages(app === "claude-desktop");
  const { setTakeoverForApp } = useProxyStatus();

  const [dialog, setDialog] = useState<ModeDialogState | null>(null);
  // Cursor 非官方供应商的「点启用时按需引导」框：非空即打开（只引导，不常驻）
  const [cursorSetup, setCursorSetup] = useState<Provider | null>(null);
  const [routeSettingsOpen, setRouteSettingsOpen] = useState(false);
  const [stackSettingsOpen, setStackSettingsOpen] = useState(false);
  const [confirmFailover, setConfirmFailover] = useState(false);
  // 关掉的是哪一份提示（后端的 staleRevision）：目录或登录又变了就是新的一份，重新显示。
  const [staleDismissed, setStaleDismissed] = useState<string | null>(null);
  useEffect(() => {
    setStaleDismissed(null);
  }, [app, active, directId, routeId, stack?.staleClients?.auth]);
  const staleRevision = stack?.staleRevision ?? "";

  /**
   * 「让某家供应商在某应用下生效」的统一闸门。入口矩阵按**动作**列，不按**按钮位置**列：
   * 切换（switchDirect）、路由到此（routeTo）、设为默认（stackSetDefault）、确认框里仍然直连
   * （onSwitchDirect）、回到直连并使用（exitAndUse）、托盘深链（needsRouteRequest）——
   * 每一个都从这里过。加新入口时先问「它做了什么」，别问「它挂在哪个页签」：上一轮就是按位置给
   * 卡片按钮加闸，漏掉了路由视图，接着又漏掉了「回到直连并使用」这条并列入口（日志里走的就是它）。
   *
   * ⚠️ **`enterMode`（进路由/聚合）刻意不在这里**：卡 A 的按钮就是让用户「去开启路由」，用户真去
   * 开启时走的正是那条动作——把它也拦下来会让用户被弹回同一张卡 A、点多少次都出不去
   * （2026-10-13 用户实测报的死循环）。那条动作本身就是用户在显式做「前提 1」，放行后由引导框
   * 接着报下一个缺口即可。
   */
  const guard = (provider: Provider | undefined, proceed: () => unknown) => {
    if (provider && providerNeedsCursorSetup(app, provider)) {
      setCursorSetup(provider);
      return;
    }
    return proceed();
  };

  // 供应商还没加载完时先等着，到了再弹
  useEffect(() => {
    if (!needsRouteRequest) return;
    const provider = providers[needsRouteRequest.providerId];
    if (!provider) return;
    // 托盘点了 Cursor 非官方那家：和界面上的入口走同一套接入引导（不是「需要路由」框）
    guard(provider, () =>
      setDialog({
        kind: "needsRoute",
        providerId: provider.id,
        reason: getRoutingReason(app, provider, t) ?? "",
      }),
    );
    onNeedsRouteHandled?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [needsRouteRequest?.nonce, providers]);

  const providerList = useMemo(() => Object.values(providers), [providers]);
  const nameOf = (id: string | null) =>
    (id && providers[id]?.name) || t("mode.noProvider");

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
    guard(provider, () =>
      setDialog({ kind: "exit", useProviderId: provider.id }),
    );

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
      // 下面每个会「启用某家」的动作都过同一个 guard（见上面的入口矩阵注释）：
      // 命中断言就开 CursorSetupDialog、不执行 proceed，别只给其中一个入口加闸
      switchDirect: (provider: Provider) =>
        guard(provider, () => void onSwitch(provider)),
      needsRouteDialog: (provider: Provider) =>
        setDialog({
          kind: "needsRoute",
          providerId: provider.id,
          reason: getRoutingReason(app, provider, t) ?? "",
        }),
      exitAndUse,
      routeTo: (provider: Provider) =>
        guard(provider, () => void onSwitch(provider)),
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
      stackSetDefault: (provider: Provider) => {
        if (active !== "stack") {
          void rememberDefault(provider);
          return;
        }
        guard(provider, () => void onSwitch(provider));
      },
    },
  };

  // 页头始终显示实际生效的连接，不跟随正在浏览的模式页签。
  const routeTarget = failoverOn ? t("mode.status.byQueue") : nameOf(routeId);
  const otherMembers = (stack?.members ?? []).filter(
    (member) => member.providerId !== routeId,
  );
  const otherModels = otherMembers.reduce(
    (sum, member) => sum + member.modelIds.length,
    0,
  );
  const status =
    active === "direct"
      ? { lead: t("mode.status.directLead"), value: nameOf(directId) }
      : active === "route"
        ? {
            lead: t("mode.status.routeLead"),
            value: routeTarget,
          }
        : {
            lead: t("mode.status.stackLead"),
            value: t("mode.status.stackValue", {
              name: nameOf(routeId),
              count: otherMembers.length,
              models: otherModels,
            }),
          };

  const handleFailoverChange = (enabled: boolean) => {
    if (enabled && !settings?.failoverConfirmed) {
      setConfirmFailover(true);
      return;
    }
    setFailover.mutate({ appType: app, enabled });
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
    const provider = providers[pick];
    // 这条**不过 guard**：见上面闸门注释里那段「enterMode 刻意不在这里」——拦它会把用户卡在
    // 卡 A 的死循环里。进路由本身就是用户在显式做「前提 1」，放行；进来之后再把剩下的缺口
    // （公网路由 / 退出 Cursor / 写 BYOK）接着引导完，用户不会以为「点了没反应」。
    await modeActions.enter(target, pick, provider?.name);
    if (provider && providerNeedsCursorSetup(app, provider)) {
      setCursorSetup(provider);
    }
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
              guard(
                routeId ? providers[routeId] : undefined,
                () =>
                  // 重试 = 把已经配好的路由/聚合目标再 attach 一次：也是「让某家生效」，同样过闸
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
                          detail:
                            extractErrorMessage(error) || t("common.unknown"),
                        }),
                      ),
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
  if (view === "stack" && active === "stack" && stack?.notice) {
    notices.push(
      <Notice
        key="stackNotice"
        tone="warning"
        title={t(`provider.${stack.notice}`)}
        actions={
          stack.notice === "routeOwnsCatalog" ? (
            <Button
              variant="neutral"
              size="compact"
              disabled={adoptCatalog.isPending}
              onClick={() => adoptCatalog.mutate()}
            >
              {t("provider.adoptCatalog")}
            </Button>
          ) : undefined
        }
      >
        {stack.noticeReasons?.length
          ? stack.noticeReasons.map((reason) => (
              <div key={reason}>
                {t(`provider.officialModelsReason.${reason}`)}
              </div>
            ))
          : undefined}
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
  if (view !== active) {
    const cta =
      view === "route"
        ? t("mode.activate.route")
        : view === "stack"
          ? t("mode.activate.stack")
          : t("mode.activate.direct");
    notices.push(
      <Notice
        key="activate"
        tone="neutral"
        title={t("mode.activate.viewing", {
          view: t(`mode.names.${view}`),
          app: appName,
          active: t(`mode.names.${active}`),
        })}
        actions={
          <Button
            variant={view}
            size="regular"
            disabled={modeActions.pending}
            onClick={() =>
              setDialog(
                view === "direct"
                  ? { kind: "exit" }
                  : { kind: "enter", target: view },
              )
            }
          >
            {cta}
          </Button>
        }
      />,
    );
  }

  return (
    <>
      {statusSlot &&
        createPortal(
          <HoverTip content={`${status.lead} ${status.value}`}>
            <span
              tabIndex={0}
              aria-label={`${status.lead} ${status.value}`}
              className={cn(
                badgeVariants({ variant: "outline" }),
                "max-w-full gap-1 border-border bg-surface px-2.5 py-1 text-body font-normal",
              )}
            >
              <span
                aria-hidden="true"
                className={cn(
                  "me-1 h-1.5 w-1.5 shrink-0 rounded-full",
                  (
                    active === "direct"
                      ? Boolean(directId && providers[directId])
                      : serviceRunning &&
                        modeView?.attached &&
                        Boolean(routeId && providers[routeId])
                  )
                    ? MODE_TONE[active].dot
                    : "bg-fg-3",
                )}
              />
              <span className="shrink-0 text-fg-2">
                {t(`mode.names.${active}`)}
              </span>
              <span aria-hidden="true" className="text-fg-3">
                ·
              </span>
              <span className="min-w-0 truncate font-medium text-fg-1">
                {active === "stack" ? nameOf(routeId) : status.value}
              </span>
            </span>
          </HoverTip>,
          statusSlot,
        )}
      <ModeTabs
        modes={modes}
        active={active}
        view={view}
        onView={setView}
        failover={
          hasFailover && view === "route" && active === "route"
            ? {
                enabled: failoverOn,
                disabled: setFailover.isPending,
                onChange: handleFailoverChange,
              }
            : undefined
        }
        onOpenRouteSettings={
          hasFailover && view === "route" && active === "route"
            ? () => setRouteSettingsOpen(true)
            : undefined
        }
        onOpenStackSettings={
          app === "codex" && view === "stack" && active === "stack"
            ? () => setStackSettingsOpen(true)
            : undefined
        }
        searchSlotRef={setSearchSlot}
      />
      <div
        ref={scrollRef}
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable overflow-x-hidden px-6 pb-12 pt-3"
      >
        {notices.length > 0 && (
          <div role="status" className="mb-3 flex flex-col gap-2">
            {notices}
          </div>
        )}
        <ProviderList
          {...listCallbacks}
          providers={providers}
          currentProviderId={currentProviderId}
          appId={app}
          isLoading={isLoading}
          onSwitch={(provider) => void onSwitch(provider)}
          switchMode={switchMode}
          searchSlot={searchSlot}
        />
      </div>

      <ModeDialog
        app={app}
        state={dialog}
        active={active}
        providers={providerList}
        directId={directId}
        eligibleIds={eligibleIds}
        defaultPick={routeId ?? directId}
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
        onExit={(useProviderId) =>
          modeActions.exitToDirect({
            directProviderId: directId,
            useProviderId,
            providerName: nameOf(useProviderId ?? directId),
          })
        }
        onSwitchDirect={(providerId) => {
          const provider = providers[providerId];
          if (!provider) return;
          guard(provider, () =>
            onSwitch(provider, { acknowledgedRouting: true }),
          );
        }}
      />

      {isProxyAppId(app) && (
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

      {cursorSetup && (
        <CursorSetupDialog
          provider={cursorSetup}
          onClose={() => setCursorSetup(null)}
          onSwitch={(provider) => onSwitch(provider)}
          onEnableLocalRoute={() =>
            void setTakeoverForApp({ appType: app, enabled: true })
          }
          onOpenPublicRouteSettings={onOpenPublicRouteSettings}
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
