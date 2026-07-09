import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import { AlertTriangle, Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { useDragSort } from "@/hooks/useDragSort";
import {
  useOpenClawLiveProviderIds,
  useOpenClawDefaultModel,
} from "@/hooks/useOpenClaw";
import {
  useHermesLiveProviderIds,
  useHermesModelConfig,
} from "@/hooks/useHermes";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
} from "@/lib/query/failover";
import {
  useCurrentOmoProviderId,
  useCurrentOmoSlimProviderId,
} from "@/lib/query/omo";
import { useCallback } from "react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api/settings";
import { useSetProxyTakeoverForApp } from "@/lib/query/proxy";
import { useSettingsQuery } from "@/lib/query/queries";
import { getProxyRequirement } from "@/utils/providerRouting";
import { decideSwitchAction } from "@/utils/switchDecision";
import { isTextEditableTarget } from "@/utils/domUtils";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (
    provider: Provider,
    opts?: { fromRoutingGuard?: boolean },
  ) => void | Promise<boolean>;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onCreate?: () => void;
  isLoading?: boolean;
  isProxyRunning?: boolean; // 代理服务运行状态
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管）
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
  onSetAsDefault?: (provider: Provider) => void; // OpenClaw: set as default model
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onCreate,
  isLoading = false,
  isProxyRunning = false,
  isProxyTakeover = false,
  activeProviderId,
  onSetAsDefault,
}: ProviderListProps) {
  const { t } = useTranslation();
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const { sortedProviders, sensors, handleDragEnd } = useDragSort(
    providers,
    appId,
  );

  const { data: opencodeLiveIds } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });

  // OpenClaw: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: openclawLiveIds } = useOpenClawLiveProviderIds(
    appId === "openclaw",
  );

  // Hermes: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: hermesLiveIds } = useHermesLiveProviderIds(appId === "hermes");

  // Hermes: 读取当前 model.provider，用于判断哪个供应商是"当前激活"（高亮）
  const { data: hermesModelConfig } = useHermesModelConfig(appId === "hermes");
  const hermesCurrentProviderId = hermesModelConfig?.provider;

  // 判断供应商是否已添加到配置（累加模式应用：OpenCode/OpenClaw/Hermes）
  const isProviderInConfig = useCallback(
    (providerId: string): boolean => {
      if (appId === "opencode") {
        return opencodeLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "openclaw") {
        return openclawLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "hermes") {
        return hermesLiveIds?.includes(providerId) ?? false;
      }
      return true; // 其他应用始终返回 true
    },
    [appId, opencodeLiveIds, openclawLiveIds, hermesLiveIds],
  );

  // OpenClaw: query default model to determine which provider is default
  const { data: openclawDefaultModel } = useOpenClawDefaultModel(
    appId === "openclaw",
  );

  const isProviderDefaultModel = useCallback(
    (providerId: string): boolean => {
      if (appId !== "openclaw" || !openclawDefaultModel?.primary) return false;
      return openclawDefaultModel.primary.startsWith(providerId + "/");
    },
    [appId, openclawDefaultModel],
  );

  // 故障转移相关
  const { data: isAutoFailoverEnabled } = useAutoFailoverEnabled(appId);
  const { data: failoverQueue } = useFailoverQueue(appId);
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();

  const isFailoverModeActive =
    isProxyTakeover === true && isAutoFailoverEnabled === true;

  const isOpenCode = appId === "opencode";
  const { data: currentOmoId } = useCurrentOmoProviderId(isOpenCode);
  const { data: currentOmoSlimId } = useCurrentOmoSlimProviderId(isOpenCode);

  const getFailoverPriority = useCallback(
    (providerId: string): number | undefined => {
      if (!isFailoverModeActive || !failoverQueue) return undefined;
      const index = failoverQueue.findIndex(
        (item) => item.providerId === providerId,
      );
      return index >= 0 ? index + 1 : undefined;
    },
    [isFailoverModeActive, failoverQueue],
  );

  const isInFailoverQueue = useCallback(
    (providerId: string): boolean => {
      if (!isFailoverModeActive || !failoverQueue) return false;
      return failoverQueue.some((item) => item.providerId === providerId);
    },
    [isFailoverModeActive, failoverQueue],
  );

  const handleToggleFailover = useCallback(
    (providerId: string, enabled: boolean) => {
      if (enabled) {
        addToQueue.mutate({ appType: appId, providerId });
      } else {
        removeFromQueue.mutate({ appType: appId, providerId });
      }
    },
    [appId, addToQueue, removeFromQueue],
  );

  const [searchTerm, setSearchTerm] = useState("");
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  // 路由自动开关 guard 状态
  const [showRoutingConfirm, setShowRoutingConfirm] = useState<
    "enable" | "disable" | null
  >(null);
  const [pendingSwitchProvider, setPendingSwitchProvider] =
    useState<Provider | null>(null);
  // 覆盖整个 toggleTakeoverThenSwitch 流程的「进行中」标记。不能只看
  // setProxyTakeover.isPending——「先切后开」时切换在途、接管 mutation 还没开始的
  // 窗口里它仍为 false，会被重复触发。
  const [routingSwitchInFlight, setRoutingSwitchInFlight] = useState(false);
  const setProxyTakeover = useSetProxyTakeoverForApp();
  // 路由 guard 需读取 autoEnable/autoDisable 偏好，并在「记住选择」时写回。
  const { data: settings } = useSettingsQuery();
  const { data: claudeDesktopStatus } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    enabled: appId === "claude-desktop",
    refetchInterval: appId === "claude-desktop" ? 5000 : false,
  });

  // 连通性检查不发真实请求、无封号/计费风险，直接执行（无需确认弹窗）。
  const handleTest = useCallback(
    (provider: Provider) => {
      checkProvider(provider.id, provider.name);
    },
    [checkProvider],
  );

  // 写入「记住选择」到对应设置位（点确认即写，独立于后续 takeover 成败）。
  const persistRoutingPreference = async (direction: "enable" | "disable") => {
    if (!settings) return;
    const { webdavSync: _webdavSync, ...rest } = settings;
    await settingsApi.save({
      ...rest,
      ...(direction === "enable"
        ? { autoEnableForNeedsRouting: true }
        : { autoDisableForNoRouting: true }),
    });
    await queryClient.invalidateQueries({ queryKey: ["settings"] });
  };

  // 接管开关 + 切换。两个方向顺序相反，都是为了「官方流量绝不在接管下」：
  // - enable（切到需路由 provider）：先切后开——仅当切换成功才开接管，开接管时
  //   current 已非官方，无「官方被接管」窗口；切换失败绝不开接管（防封号）。开接管
  //   失败则 provider 已切但未接管（不工作，非封号）→ 提示手动开路由。
  // - disable（切到官方 provider）：先关后切——先关接管再切，任何时刻官方都不在
  //   接管下；关接管失败则中止不切。
  // onSwitch(p, { fromRoutingGuard: true })：guard 已处理路由意图，让 switchProvider
  // 跳过基于闭包（可能滞后一帧）的需路由提示与官方硬阻断；返回值表示切换是否成功。
  const toggleTakeoverThenSwitch = async (
    provider: Provider,
    enabled: boolean,
  ): Promise<void> => {
    // 标记整个流程进行中（含切换在途窗口），防重入；finally 必清。
    setRoutingSwitchInFlight(true);
    try {
      if (enabled) {
        // 仅当切换**明确**返回 true 才开接管：非 true（含 false / void 实现的
        // undefined）一律中止，绝不在切换未确认下开接管（防封号）。切换失败的 toast
        // 由 switchProvider 自身弹出。
        const switched = await onSwitch(provider, { fromRoutingGuard: true });
        if (switched !== true) return;
        try {
          await setProxyTakeover.mutateAsync({ appType: appId, enabled: true });
        } catch {
          toast.error(
            t("notifications.routingEnableFailed", {
              name: provider.name,
              defaultValue:
                "已切换到 {{name}}，但开启本地路由失败，请在代理面板手动开启",
            }),
          );
          return;
        }
        toast.success(
          appId === "codex"
            ? t("notifications.routingAutoEnabledRestart", {
                name: provider.name,
                defaultValue:
                  "已切换到 {{name}} 并自动开启本地路由，请重启客户端以生效",
              })
            : t("notifications.routingAutoEnabled", {
                name: provider.name,
                defaultValue: "已切换到 {{name}} 并自动开启本地路由",
              }),
          { closeButton: true, duration: 5000 },
        );
        await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
        return;
      }

      // disable：先关接管（失败则中止不切换），再切到官方。
      try {
        await setProxyTakeover.mutateAsync({ appType: appId, enabled: false });
      } catch {
        toast.error(
          t("notifications.routingDisableFailed", {
            defaultValue: "关闭本地路由失败，已取消切换",
          }),
        );
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      // await：保持 in-flight 直到切换完成，否则 finally 会在切换在途时提前清标记 →
      // 按钮重新可点，用户可重复触发。
      const switched = await onSwitch(provider, { fromRoutingGuard: true });
      if (switched !== true) {
        // 路由已关但切换失败 — 当前 provider 需要路由却已失去路由，告知用户恢复
        toast.error(
          t("notifications.routingDisabledButSwitchFailed", {
            defaultValue:
              "本地路由已关闭，但切换失败。当前供应商需要本地路由才能使用，请手动开启路由或重试切换",
          }),
          { id: "switch-provider-error", duration: 6000 },
        );
        return;
      }
      toast.success(
        appId === "codex"
          ? t("notifications.routingAutoDisabledRestart", {
              defaultValue: "当前应用本地路由已自动关闭，请重启客户端以生效",
            })
          : t("notifications.routingAutoDisabled", {
              defaultValue: "当前应用本地路由已自动关闭",
            }),
        { closeButton: true, duration: 5000 },
      );
    } finally {
      setRoutingSwitchInFlight(false);
    }
  };

  // 切换 guard：decideSwitchAction 分流为直接切 / 弹确认 / 静默切。claude-desktop
  // 排除：其 proxy 模式依赖代理服务运行，非 per-app takeover（后端仅 claude/codex/
  // gemini）；其徽章与 switchProvider 既有 toast 不依赖 takeover，不受此排除影响。
  const handleSwitchWithGuard = async (provider: Provider) => {
    if (routingSwitchInFlight || setProxyTakeover.isPending) return;

    if (appId === "claude-desktop") {
      onSwitch(provider);
      return;
    }

    const requirement = getProxyRequirement(provider, appId);
    // 官方判定只认显式 category === "official"，不用空字段启发式：base_url/key 缺失
    // 区分不了「官方直连」和「自定义但还没填完」，封号保护这类高代价决策不能建立在
    // 这种脆弱信号上（与执行层 useProviderActions 的硬阻断判定保持一致）。
    const action = decideSwitchAction({
      needsRouting: requirement.required,
      isProxyTakeover,
      isOfficial: provider.category === "official",
      autoEnable: settings?.autoEnableForNeedsRouting ?? false,
      autoDisable: settings?.autoDisableForNoRouting ?? false,
    });

    switch (action) {
      case "confirmEnable":
        setPendingSwitchProvider(provider);
        setShowRoutingConfirm("enable");
        return;
      case "confirmDisable":
        setPendingSwitchProvider(provider);
        setShowRoutingConfirm("disable");
        return;
      case "directEnable":
        await toggleTakeoverThenSwitch(provider, true);
        return;
      case "directDisable":
        await toggleTakeoverThenSwitch(provider, false);
        return;
      case "direct":
        onSwitch(provider);
    }
  };

  // ConfirmDialog 通过 onConfirm 回传 checkbox 勾选值（rememberRouting）；不再
  // 维护单独的本地受控状态——避免 dialog 与父级各持一份导致漂移。
  const handleRoutingConfirm = async (rememberRouting: boolean) => {
    const direction = showRoutingConfirm;
    const provider = pendingSwitchProvider;
    setShowRoutingConfirm(null);
    setPendingSwitchProvider(null);
    if (!direction || !provider) return;

    if (rememberRouting) {
      try {
        await persistRoutingPreference(direction);
      } catch (error) {
        console.error("Failed to persist routing preference:", error);
      }
    }

    await toggleTakeoverThenSwitch(provider, direction === "enable");
  };

  const handleRoutingCancel = () => {
    setShowRoutingConfirm(null);
    setPendingSwitchProvider(null);
  };

  // Import current live config as default provider
  const queryClient = useQueryClient();
  const importMutation = useMutation({
    mutationFn: async (): Promise<boolean> => {
      if (appId === "opencode") {
        const count = await providersApi.importOpenCodeFromLive();
        return count > 0;
      }
      if (appId === "openclaw") {
        const count = await providersApi.importOpenClawFromLive();
        return count > 0;
      }
      if (appId === "hermes") {
        const count = await providersApi.importHermesFromLive();
        return count > 0;
      }
      if (appId === "claude-desktop") {
        const count = await providersApi.importClaudeDesktopFromClaude();
        return count > 0;
      }
      return providersApi.importDefault(appId);
    },
    onSuccess: (imported) => {
      if (imported) {
        queryClient.invalidateQueries({ queryKey: ["providers", appId] });
        if (appId === "claude-desktop") {
          queryClient.invalidateQueries({ queryKey: ["claudeDesktopStatus"] });
        }
        toast.success(t("provider.importCurrentDescription"));
      } else {
        toast.info(t("provider.noProviders"));
      }
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;

      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && key === "f") {
        // 正在输入框/可编辑区域中时不抢占 Ctrl+F（例如添加供应商表单里
        // ProviderPresetSelector 的搜索框），避免与其同名快捷键冲突。
        if (isTextEditableTarget(document.activeElement)) return;
        event.preventDefault();
        setIsSearchOpen(true);
        return;
      }

      if (key === "escape") {
        setIsSearchOpen(false);
      }
    };

    globalThis.addEventListener("keydown", handleKeyDown);
    return () => globalThis.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    if (isSearchOpen) {
      const frame = requestAnimationFrame(() => {
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      });
      return () => cancelAnimationFrame(frame);
    }
  }, [isSearchOpen]);

  const filteredProviders = useMemo(() => {
    const keyword = searchTerm.trim().toLowerCase();
    if (!keyword) return sortedProviders;
    return sortedProviders.filter((provider) => {
      const fields = [provider.name, provider.notes, provider.websiteUrl];
      return fields.some((field) =>
        field?.toString().toLowerCase().includes(keyword),
      );
    });
  }, [searchTerm, sortedProviders]);

  const claudeDesktopStatusMessages = useMemo(() => {
    if (appId !== "claude-desktop" || !claudeDesktopStatus) return [];

    const messages: string[] = [];
    if (!claudeDesktopStatus.supported) {
      messages.push(
        t("claudeDesktop.statusUnsupported", {
          defaultValue: "当前平台暂不支持 Claude Desktop 3P 配置写入。",
        }),
      );
      return messages;
    }

    if (claudeDesktopStatus.staleRawModels) {
      messages.push(
        t("claudeDesktop.statusStaleRawModels", {
          defaultValue:
            "Claude Desktop profile 中存在非 claude-* 模型名，新版 Claude Desktop 可能拒绝加载；重新切换当前供应商可修复。",
        }),
      );
    }
    if (claudeDesktopStatus.missingRouteMappings) {
      messages.push(
        t("claudeDesktop.statusMissingRouteMappings", {
          defaultValue:
            "当前供应商启用了模型映射，但没有有效路由；请编辑供应商并补全至少一个模型映射。",
        }),
      );
    }
    if (
      claudeDesktopStatus.mode === "proxy" &&
      !claudeDesktopStatus.gatewayTokenConfigured
    ) {
      messages.push(
        t("claudeDesktop.statusGatewayTokenMissing", {
          defaultValue:
            "当前本地路由 token 尚未生成；重新切换该供应商会写入新的本地 token。",
        }),
      );
    }

    const expected = claudeDesktopStatus.expectedBaseUrl?.replace(/\/+$/, "");
    const actual = claudeDesktopStatus.actualBaseUrl?.replace(/\/+$/, "");
    if (expected && actual && expected !== actual) {
      messages.push(
        t("claudeDesktop.statusBaseUrlMismatch", {
          expected,
          actual,
          defaultValue:
            "Claude Desktop profile 指向的地址与当前供应商不一致；当前为 {{actual}}，应为 {{expected}}。重新切换当前供应商可修复。",
        }),
      );
    }

    return messages;
  }, [appId, claudeDesktopStatus, t]);

  if (isLoading) {
    return (
      <div className="space-y-3">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="w-full border border-dashed rounded-lg h-28 border-muted-foreground/40 bg-muted/40"
          />
        ))}
      </div>
    );
  }

  if (sortedProviders.length === 0) {
    return (
      <ProviderEmptyState
        appId={appId}
        onCreate={onCreate}
        onImport={() => importMutation.mutate()}
      />
    );
  }

  const renderProviderList = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={filteredProviders.map((provider) => provider.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className="space-y-3">
          {filteredProviders.map((provider) => {
            const isOmo = provider.category === "omo";
            const isOmoSlim = provider.category === "omo-slim";
            const isOmoCurrent = isOmo && provider.id === (currentOmoId || "");
            const isOmoSlimCurrent =
              isOmoSlim && provider.id === (currentOmoSlimId || "");
            const isHermesCurrent =
              appId === "hermes" && hermesCurrentProviderId === provider.id;
            return (
              <SortableProviderCard
                key={provider.id}
                provider={provider}
                isCurrent={
                  isOmo
                    ? isOmoCurrent
                    : isOmoSlim
                      ? isOmoSlimCurrent
                      : appId === "hermes"
                        ? isHermesCurrent
                        : provider.id === currentProviderId
                }
                appId={appId}
                isInConfig={isProviderInConfig(provider.id)}
                isOmo={isOmo}
                isOmoSlim={isOmoSlim}
                onSwitch={handleSwitchWithGuard}
                onEdit={onEdit}
                onDelete={onDelete}
                onRemoveFromConfig={onRemoveFromConfig}
                onDisableOmo={onDisableOmo}
                onDisableOmoSlim={onDisableOmoSlim}
                onDuplicate={onDuplicate}
                onConfigureUsage={onConfigureUsage}
                onOpenWebsite={onOpenWebsite}
                onOpenTerminal={onOpenTerminal}
                onTest={handleTest}
                isTesting={isChecking(provider.id)}
                isProxyRunning={isProxyRunning}
                isProxyTakeover={isProxyTakeover}
                isRoutingSwitchPending={
                  routingSwitchInFlight || setProxyTakeover.isPending
                }
                isAutoFailoverEnabled={isFailoverModeActive}
                failoverPriority={getFailoverPriority(provider.id)}
                isInFailoverQueue={isInFailoverQueue(provider.id)}
                onToggleFailover={(enabled) =>
                  handleToggleFailover(provider.id, enabled)
                }
                activeProviderId={activeProviderId}
                // OpenClaw: default model / Hermes: model.provider === provider.id
                isDefaultModel={
                  appId === "hermes"
                    ? isHermesCurrent
                    : isProviderDefaultModel(provider.id)
                }
                onSetAsDefault={
                  onSetAsDefault ? () => onSetAsDefault(provider) : undefined
                }
              />
            );
          })}
        </div>
      </SortableContext>
    </DndContext>
  );

  return (
    <div className="mt-4 space-y-4">
      {claudeDesktopStatusMessages.length > 0 && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200">
          <div className="flex items-center gap-2 font-medium">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            {t("claudeDesktop.statusTitle", {
              defaultValue: "Claude Desktop 配置需要检查",
            })}
          </div>
          <ul className="mt-2 space-y-1 text-xs leading-relaxed">
            {claudeDesktopStatusMessages.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        </div>
      )}
      <AnimatePresence>
        {isSearchOpen && (
          <motion.div
            key="provider-search"
            initial={{ opacity: 0, y: -8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="fixed left-1/2 top-[6.5rem] z-40 w-[min(90vw,26rem)] -translate-x-1/2 sm:right-6 sm:left-auto sm:translate-x-0"
          >
            <div className="p-4 space-y-3 border shadow-md rounded-2xl border-white/10 bg-background/95 shadow-black/20 backdrop-blur-md">
              <div className="relative flex items-center gap-2">
                <Search className="absolute w-4 h-4 -translate-y-1/2 pointer-events-none left-3 top-1/2 text-muted-foreground" />
                <Input
                  ref={searchInputRef}
                  value={searchTerm}
                  onChange={(event) => setSearchTerm(event.target.value)}
                  placeholder={t("provider.searchPlaceholder", {
                    defaultValue: "Search name, notes, or URL...",
                  })}
                  aria-label={t("provider.searchAriaLabel", {
                    defaultValue: "Search providers",
                  })}
                  className="pr-16 pl-9"
                />
                {searchTerm && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="absolute text-xs -translate-y-1/2 right-11 top-1/2"
                    onClick={() => setSearchTerm("")}
                  >
                    {t("common.clear", { defaultValue: "Clear" })}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="ml-auto"
                  onClick={() => setIsSearchOpen(false)}
                  aria-label={t("provider.searchCloseAriaLabel", {
                    defaultValue: "Close provider search",
                  })}
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
                <span>
                  {t("provider.searchScopeHint", {
                    defaultValue: "Matches provider name, notes, and URL.",
                  })}
                </span>
                <span>
                  {t("provider.searchCloseHint", {
                    defaultValue: "Press Esc to close",
                  })}
                </span>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {filteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : (
        renderProviderList()
      )}

      <ConfirmDialog
        isOpen={showRoutingConfirm !== null}
        variant={showRoutingConfirm === "disable" ? "destructive" : "info"}
        title={
          showRoutingConfirm === "disable"
            ? t("confirm.routingDisable.title", {
                defaultValue: "关闭本地路由并切换？",
              })
            : t("confirm.routingEnable.title", {
                defaultValue: "开启本地路由并启用？",
              })
        }
        message={
          showRoutingConfirm === "disable"
            ? t("confirm.routingDisable.message", {
                defaultValue:
                  "该供应商为官方直连，不能在本地路由接管下使用（可能导致账号被封禁）。\n将关闭当前应用的本地路由，然后切换到该供应商。",
              })
            : t("confirm.routingEnable.message", {
                defaultValue:
                  "该供应商需要本地路由才能正常工作。\n将开启当前应用的本地路由，然后启用该供应商。",
              })
        }
        confirmText={
          showRoutingConfirm === "disable"
            ? t("confirm.routingDisable.confirm", {
                defaultValue: "关闭路由并切换",
              })
            : t("confirm.routingEnable.confirm", {
                defaultValue: "开启路由并启用",
              })
        }
        checkboxLabel={
          showRoutingConfirm === "disable"
            ? t("confirm.routing.rememberDisable", {
                defaultValue: "以后都自动关闭本地路由，不再询问",
              })
            : t("confirm.routing.rememberEnable", {
                defaultValue: "以后都自动开启本地路由，不再询问",
              })
        }
        onConfirm={(remember) => void handleRoutingConfirm(remember)}
        onCancel={handleRoutingCancel}
      />
    </div>
  );
}

interface SortableProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig: boolean;
  isOmo: boolean;
  isOmoSlim: boolean;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting: boolean;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  isRoutingSwitchPending: boolean;
  isAutoFailoverEnabled: boolean;
  failoverPriority?: number;
  isInFailoverQueue: boolean;
  onToggleFailover: (enabled: boolean) => void;
  activeProviderId?: string;
  // OpenClaw: default model
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
}

function SortableProviderCard({
  provider,
  isCurrent,
  appId,
  isInConfig,
  isOmo,
  isOmoSlim,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onTest,
  isTesting,
  isProxyRunning,
  isProxyTakeover,
  isRoutingSwitchPending,
  isAutoFailoverEnabled,
  failoverPriority,
  isInFailoverQueue,
  onToggleFailover,
  activeProviderId,
  isDefaultModel,
  onSetAsDefault,
}: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: provider.id });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div ref={setNodeRef} style={style}>
      <ProviderCard
        provider={provider}
        isCurrent={isCurrent}
        appId={appId}
        isInConfig={isInConfig}
        isOmo={isOmo}
        isOmoSlim={isOmoSlim}
        onSwitch={onSwitch}
        onEdit={onEdit}
        onDelete={onDelete}
        onRemoveFromConfig={onRemoveFromConfig}
        onDisableOmo={onDisableOmo}
        onDisableOmoSlim={onDisableOmoSlim}
        onDuplicate={onDuplicate}
        onConfigureUsage={
          onConfigureUsage ? (item) => onConfigureUsage(item) : () => undefined
        }
        onOpenWebsite={onOpenWebsite}
        onOpenTerminal={onOpenTerminal}
        onTest={onTest}
        isTesting={isTesting}
        isProxyRunning={isProxyRunning}
        isProxyTakeover={isProxyTakeover}
        isRoutingSwitchPending={isRoutingSwitchPending}
        dragHandleProps={{
          attributes,
          listeners,
          isDragging,
        }}
        isAutoFailoverEnabled={isAutoFailoverEnabled}
        failoverPriority={failoverPriority}
        isInFailoverQueue={isInFailoverQueue}
        onToggleFailover={onToggleFailover}
        activeProviderId={activeProviderId}
        // OpenClaw: default model
        isDefaultModel={isDefaultModel}
        onSetAsDefault={onSetAsDefault}
      />
    </div>
  );
}
