import { useCallback, useEffect, useMemo, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Settings,
  ArrowLeft,
  // Bot, // TODO: Agents 功能开发中，暂时不需要
  Book,
  Wrench,
  Server,
  RefreshCw,
  Globe2,
  ChevronDown,
  Loader2,
  Check,
} from "lucide-react";
import type { Provider } from "@/types";
import type { EnvConflict } from "@/types/env";
import { useProvidersQuery } from "@/lib/query";
import {
  providersApi,
  settingsApi,
  type AppId,
  type ProviderSwitchEvent,
} from "@/lib/api";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";
import { useProviderActions } from "@/hooks/useProviderActions";
import {
  useConfigSets,
  type ActivateConfigSetOptions,
} from "@/hooks/useConfigSets";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import { AppSwitcher } from "@/components/AppSwitcher";
import { ProviderList } from "@/components/providers/ProviderList";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { UpdateBadge } from "@/components/UpdateBadge";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import UsageScriptModal from "@/components/UsageScriptModal";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import PromptPanel from "@/components/prompts/PromptPanel";
import { SkillsPage } from "@/components/skills/SkillsPage";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { AgentsPanel } from "@/components/agents/AgentsPanel";
import { UniversalProviderPanel } from "@/components/universal";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

type View =
  | "providers"
  | "settings"
  | "prompts"
  | "skills"
  | "mcp"
  | "agents"
  | "universal";

const DRAG_BAR_HEIGHT = 28; // px
const HEADER_HEIGHT = 64; // px
const CONTENT_TOP_OFFSET = DRAG_BAR_HEIGHT + HEADER_HEIGHT;

function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [activeApp, setActiveApp] = useState<AppId>("claude");
  const [currentView, setCurrentView] = useState<View>("providers");
  const [isAddOpen, setIsAddOpen] = useState(false);

  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const [usageProviderId, setUsageProviderId] = useState<string | null>(null);
  const [editingProviderSnapshot, setEditingProviderSnapshot] =
    useState<Provider | null>(null);
  const [usageProviderSnapshot, setUsageProviderSnapshot] =
    useState<Provider | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Provider | null>(null);
  const [envConflicts, setEnvConflicts] = useState<EnvConflict[]>([]);
  const [showEnvBanner, setShowEnvBanner] = useState(false);

  const handleOpenEditProvider = useCallback((provider: Provider) => {
    setEditingProviderSnapshot(provider);
    setEditingProviderId(provider.id);
  }, []);

  const handleOpenUsageModal = useCallback((provider: Provider) => {
    setUsageProviderSnapshot(provider);
    setUsageProviderId(provider.id);
  }, []);

  const promptPanelRef = useRef<any>(null);
  const mcpPanelRef = useRef<any>(null);
  const skillsPageRef = useRef<any>(null);
  const addActionButtonClass =
    "bg-orange-500 hover:bg-orange-600 dark:bg-orange-500 dark:hover:bg-orange-600 text-white shadow-lg shadow-orange-500/30 dark:shadow-orange-500/40 rounded-full w-8 h-8";

  // 获取代理服务状态
  const {
    isRunning: isProxyRunning,
    takeoverStatus,
    status: proxyStatus,
  } = useProxyStatus();
  // 当前应用的代理是否开启
  const isCurrentAppTakeoverActive = takeoverStatus?.[activeApp] || false;
  // 当前应用代理实际使用的供应商 ID（从 active_targets 中获取）
  const activeProviderId = useMemo(() => {
    const target = proxyStatus?.active_targets?.find(
      (t) => t.app_type === activeApp,
    );
    return target?.provider_id;
  }, [proxyStatus?.active_targets, activeApp]);

  // 获取供应商列表，当代理服务运行时自动刷新
  const { data, isLoading, refetch } = useProvidersQuery(activeApp, {
    isProxyRunning,
  });
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";
  const editingProviderData =
    (editingProviderId ? providers[editingProviderId] : null) ??
    editingProviderSnapshot;
  const usageProviderData =
    (usageProviderId ? providers[usageProviderId] : null) ??
    usageProviderSnapshot;
  // 使用 Hook 保存最后有效值，用于动画退出期间保持内容显示
  const effectiveEditingProvider = useLastValidValue(editingProviderData);
  const effectiveUsageProvider = useLastValidValue(usageProviderData);
  const isEditingDialogOpen = editingProviderId !== null;
  const isUsageModalOpen = usageProviderId !== null;
  // Skills 功能仅支持 Claude 和 Codex
  const hasSkillsSupport = activeApp === "claude" || activeApp === "codex";

  // 🎯 使用 useProviderActions Hook 统一管理所有 Provider 操作
  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    isLoading: isProviderActionPending,
  } = useProviderActions(activeApp);

  const {
    configSets,
    activeConfigSetId,
    isActivating: isConfigSetActivating,
    activateConfigSet,
  } = useConfigSets();

  // 监听来自托盘菜单的切换事件
  useEffect(() => {
    let unsubscribe: (() => void) | undefined;

    const setupListener = async () => {
      try {
        unsubscribe = await providersApi.onSwitched(
          async (event: ProviderSwitchEvent) => {
            if (event.appType === activeApp) {
              await refetch();
            }
          },
        );
      } catch (error) {
        console.error("[App] Failed to subscribe provider switch event", error);
      }
    };

    setupListener();
    return () => {
      unsubscribe?.();
    };
  }, [activeApp, refetch]);

  // 监听统一供应商同步事件，刷新所有应用的供应商列表
  useEffect(() => {
    let unsubscribe: (() => void) | undefined;

    const setupListener = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unsubscribe = await listen("universal-provider-synced", async () => {
          // 统一供应商同步后刷新所有应用的供应商列表
          // 使用 invalidateQueries 使所有 providers 查询失效
          await queryClient.invalidateQueries({ queryKey: ["providers"] });
          // 同时更新托盘菜单
          try {
            await providersApi.updateTrayMenu();
          } catch (error) {
            console.error("[App] Failed to update tray menu", error);
          }
        });
      } catch (error) {
        console.error(
          "[App] Failed to subscribe universal-provider-synced event",
          error,
        );
      }
    };

    setupListener();
    return () => {
      unsubscribe?.();
    };
  }, [queryClient]);

  // 应用启动时检测所有应用的环境变量冲突
  useEffect(() => {
    const checkEnvOnStartup = async () => {
      try {
        const allConflicts = await checkAllEnvConflicts();
        const flatConflicts = Object.values(allConflicts).flat();

        if (flatConflicts.length > 0) {
          setEnvConflicts(flatConflicts);
          const dismissed = sessionStorage.getItem("env_banner_dismissed");
          if (!dismissed) {
            setShowEnvBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on startup:",
          error,
        );
      }
    };

    checkEnvOnStartup();
  }, []);

  // 应用启动时检查是否刚完成了配置迁移
  useEffect(() => {
    const checkMigration = async () => {
      try {
        const migrated = await invoke<boolean>("get_migration_result");
        if (migrated) {
          toast.success(
            t("migration.success", { defaultValue: "配置迁移成功" }),
            { closeButton: true },
          );
        }
      } catch (error) {
        console.error("[App] Failed to check migration result:", error);
      }
    };

    checkMigration();
  }, [t]);

  // 切换应用时检测当前应用的环境变量冲突
  useEffect(() => {
    const checkEnvOnSwitch = async () => {
      try {
        const conflicts = await checkEnvConflicts(activeApp);

        if (conflicts.length > 0) {
          // 合并新检测到的冲突
          setEnvConflicts((prev) => {
            const existingKeys = new Set(
              prev.map((c) => `${c.varName}:${c.sourcePath}`),
            );
            const newConflicts = conflicts.filter(
              (c) => !existingKeys.has(`${c.varName}:${c.sourcePath}`),
            );
            return [...prev, ...newConflicts];
          });
          const dismissed = sessionStorage.getItem("env_banner_dismissed");
          if (!dismissed) {
            setShowEnvBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on app switch:",
          error,
        );
      }
    };

    checkEnvOnSwitch();
  }, [activeApp]);

  const isSwitchingProvider = isProviderActionPending || isConfigSetActivating;

  const activeEnvironment = useMemo(
    () =>
      configSets.find((set) => set.id === activeConfigSetId) ?? configSets[0],
    [configSets, activeConfigSetId],
  );
  const environmentLabel =
    activeEnvironment?.name ??
    t("settings.configSetDefaultName", { defaultValue: "默认环境" });
  const hasMultipleConfigSets = configSets.length > 1;

  const activateEnvironment = useCallback(
    async (setId: string, options?: ActivateConfigSetOptions) => {
      const activated = await activateConfigSet(setId, options);
      if (activated) {
        await Promise.allSettled([
          queryClient.invalidateQueries({ queryKey: ["providers"] }),
          queryClient.invalidateQueries({ queryKey: ["mcp"] }),
        ]);
        await refetch();
      }
      return activated;
    },
    [activateConfigSet, queryClient, refetch],
  );

  const handleSwitchProvider = useCallback(
    async (provider: Provider, targetSetId?: string) => {
      const fallbackSetId =
        activeConfigSetId ?? (configSets.length > 0 ? configSets[0].id : undefined);
      const desiredSetId = targetSetId ?? fallbackSetId;

      if (desiredSetId && desiredSetId !== activeConfigSetId) {
        const activated = await activateEnvironment(desiredSetId, {
          silent: true,
        });
        if (!activated) {
          return;
        }
      }

      await switchProvider(provider);
    },
    [
      activateEnvironment,
      activeConfigSetId,
      configSets,
      switchProvider,
    ],
  );

  useEffect(() => {
    const handleGlobalShortcut = (event: KeyboardEvent) => {
      if (event.key !== "," || !(event.metaKey || event.ctrlKey)) {
        return;
      }
      event.preventDefault();
      setCurrentView("settings");
    };

    window.addEventListener("keydown", handleGlobalShortcut);
    return () => {
      window.removeEventListener("keydown", handleGlobalShortcut);
    };
  }, []);

  const EnvironmentSwitcher = () => {
    if (!configSets.length) return null;

    if (!hasMultipleConfigSets) {
      return (
        <Button
          variant="outline"
          size="sm"
          disabled
          className="flex items-center gap-2 rounded-lg border-border-default/50 px-3 text-sm"
        >
          <Globe2 className="h-4 w-4" />
          <span className="max-w-[9rem] truncate text-left">
            {environmentLabel}
          </span>
        </Button>
      );
    }

    return (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            disabled={isConfigSetActivating}
            className="flex items-center gap-2 rounded-lg border-border-default/50 px-3 text-sm"
          >
            {isConfigSetActivating ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Globe2 className="h-4 w-4" />
            )}
            <span className="max-w-[9rem] truncate text-left">
              {environmentLabel}
            </span>
            <ChevronDown className="h-3 w-3 opacity-70" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-52">
          <DropdownMenuLabel className="text-xs text-muted-foreground">
            {t("provider.selectEnvironment", {
              defaultValue: "选择环境",
            })}
          </DropdownMenuLabel>
          <DropdownMenuSeparator />
          {configSets.map((set) => (
            <DropdownMenuItem
              key={set.id}
              onSelect={(event) => {
                event.preventDefault();
                if (set.id === activeConfigSetId) return;
                void activateEnvironment(set.id);
              }}
              className="flex items-center justify-between gap-2"
            >
              <span className="truncate">{set.name}</span>
              {set.id === activeConfigSetId ? (
                <Check className="h-3 w-3 text-primary" />
              ) : null}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    );
  };

  // 打开网站链接
  const handleOpenWebsite = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("notifications.openLinkFailed", {
          defaultValue: "链接打开失败",
        });
      toast.error(detail);
    }
  };

  // 编辑供应商
  const handleEditProvider = async (provider: Provider) => {
    await updateProvider(provider);
    setEditingProviderId(null);
  };

  // 确认删除供应商
  const handleConfirmDelete = async () => {
    if (!confirmDelete) return;
    await deleteProvider(confirmDelete.id);
    setConfirmDelete(null);
  };

  // 复制供应商
  const handleDuplicateProvider = async (provider: Provider) => {
    // 1️⃣ 计算新的 sortIndex：如果原供应商有 sortIndex，则复制它
    const newSortIndex =
      provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined;

    const duplicatedProvider: Omit<Provider, "id" | "createdAt"> = {
      name: `${provider.name} copy`,
      settingsConfig: JSON.parse(JSON.stringify(provider.settingsConfig)), // 深拷贝
      websiteUrl: provider.websiteUrl,
      category: provider.category,
      sortIndex: newSortIndex, // 复制原 sortIndex + 1
      meta: provider.meta
        ? JSON.parse(JSON.stringify(provider.meta))
        : undefined, // 深拷贝
      icon: provider.icon,
      iconColor: provider.iconColor,
    };

    // 2️⃣ 如果原供应商有 sortIndex，需要将后续所有供应商的 sortIndex +1
    if (provider.sortIndex !== undefined) {
      const updates = Object.values(providers)
        .filter(
          (p) =>
            p.sortIndex !== undefined &&
            p.sortIndex >= newSortIndex! &&
            p.id !== provider.id,
        )
        .map((p) => ({
          id: p.id,
          sortIndex: p.sortIndex! + 1,
        }));

      // 先更新现有供应商的 sortIndex，为新供应商腾出位置
      if (updates.length > 0) {
        try {
          await providersApi.updateSortOrder(updates, activeApp);
        } catch (error) {
          console.error("[App] Failed to update sort order", error);
          toast.error(
            t("provider.sortUpdateFailed", {
              defaultValue: "排序更新失败",
            }),
          );
          return; // 如果排序更新失败，不继续添加
        }
      }
    }

    // 3️⃣ 添加复制的供应商
    await addProvider(duplicatedProvider);
  };

  // 导入配置成功后刷新
  const handleImportSuccess = async () => {
    try {
      // 导入会影响所有应用的供应商数据：刷新所有 providers 缓存
      await queryClient.invalidateQueries({
        queryKey: ["providers"],
        refetchType: "all",
      });
      await queryClient.refetchQueries({
        queryKey: ["providers"],
        type: "all",
      });
    } catch (error) {
      console.error("[App] Failed to refresh providers after import", error);
      await refetch();
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  };

  const renderContent = () => {
    const content = (() => {
      switch (currentView) {
        case "settings":
          return (
            <SettingsPage
              open={true}
              onOpenChange={() => setCurrentView("providers")}
              onImportSuccess={handleImportSuccess}
            />
          );
        case "prompts":
          return (
            <PromptPanel
              ref={promptPanelRef}
              open={true}
              onOpenChange={() => setCurrentView("providers")}
              appId={activeApp}
            />
          );
        case "skills":
          return (
            <SkillsPage
              ref={skillsPageRef}
              onClose={() => setCurrentView("providers")}
              initialApp={activeApp}
            />
          );
        case "mcp":
          return (
            <UnifiedMcpPanel
              ref={mcpPanelRef}
              onOpenChange={() => setCurrentView("providers")}
              activeConfigSetId={activeConfigSetId}
            />
          );
        case "agents":
          return (
            <AgentsPanel onOpenChange={() => setCurrentView("providers")} />
          );
        case "universal":
          return (
            <div className="mx-auto max-w-[56rem] px-5 pt-4">
              <UniversalProviderPanel />
            </div>
          );
        default:
          return (
            <div className="mx-auto max-w-[56rem] px-5 flex flex-col h-[calc(100vh-8rem)] overflow-hidden">
              {/* 独立滚动容器 - 解决 Linux/Ubuntu 下 DndContext 与滚轮事件冲突 */}
              <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1">
                <AnimatePresence mode="wait">
                  <motion.div
                    key={activeApp}
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={{ duration: 0.15 }}
                    className="space-y-4"
                  >
                    <ProviderList
                      providers={providers}
                      currentProviderId={currentProviderId}
                      appId={activeApp}
                      isLoading={isLoading}
                      onSwitch={handleSwitchProvider}
                      isProxyRunning={isProxyRunning}
                      isProxyTakeover={
                        isProxyRunning && isCurrentAppTakeoverActive
                      }
                      activeProviderId={activeProviderId}
                      onEdit={handleOpenEditProvider}
                      onDelete={setConfirmDelete}
                      onDuplicate={handleDuplicateProvider}
                      onConfigureUsage={handleOpenUsageModal}
                      onOpenWebsite={handleOpenWebsite}
                      onCreate={() => setIsAddOpen(true)}
                      configSets={configSets}
                      activeConfigSetId={activeConfigSetId}
                      isSwitching={isSwitchingProvider}
                    />
                  </motion.div>
                </AnimatePresence>
              </div>
            </div>
          );
      }
    })();

    return (
      <AnimatePresence mode="wait">
        <motion.div
          key={currentView}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
        >
          {content}
        </motion.div>
      </AnimatePresence>
    );
  };

  return (
    <div
      className="flex flex-col h-screen overflow-hidden bg-background text-foreground selection:bg-primary/30"
      style={{ overflowX: "hidden", paddingTop: CONTENT_TOP_OFFSET }}
    >
      {/* 全局拖拽区域（顶部 28px），避免上边框无法拖动 */}
      <div
        className="fixed top-0 left-0 right-0 z-[60]"
        data-tauri-drag-region
        style={{ WebkitAppRegion: "drag", height: DRAG_BAR_HEIGHT } as any}
      />
      {/* 环境变量警告横幅 */}
      {showEnvBanner && envConflicts.length > 0 && (
        <EnvWarningBanner
          conflicts={envConflicts}
          onDismiss={() => {
            setShowEnvBanner(false);
            sessionStorage.setItem("env_banner_dismissed", "true");
          }}
          onDeleted={async () => {
            // 删除后重新检测
            try {
              const allConflicts = await checkAllEnvConflicts();
              const flatConflicts = Object.values(allConflicts).flat();
              setEnvConflicts(flatConflicts);
              if (flatConflicts.length === 0) {
                setShowEnvBanner(false);
              }
            } catch (error) {
              console.error(
                "[App] Failed to re-check conflicts after deletion:",
                error,
              );
            }
          }}
        />
      )}

      <header
        className="fixed z-50 w-full transition-all duration-300 bg-background/80 backdrop-blur-md"
        data-tauri-drag-region
        style={
          {
            WebkitAppRegion: "drag",
            top: DRAG_BAR_HEIGHT,
            height: HEADER_HEIGHT,
          } as any
        }
      >
        <div
          className="mx-auto flex h-full max-w-[56rem] flex-wrap items-center justify-between gap-2 px-6"
          data-tauri-drag-region
          style={{ WebkitAppRegion: "drag" } as any}
        >
          <div
            className="flex items-center gap-1"
            style={{ WebkitAppRegion: "no-drag" } as any}
          >
            {currentView !== "providers" ? (
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="icon"
                  onClick={() => setCurrentView("providers")}
                  className="mr-2 rounded-lg"
                >
                  <ArrowLeft className="w-4 h-4" />
                </Button>
                <h1 className="text-lg font-semibold">
                  {currentView === "settings" && t("settings.title")}
                  {currentView === "prompts" &&
                    t("prompts.title", { appName: t(`apps.${activeApp}`) })}
                  {currentView === "skills" && t("skills.title")}
                  {currentView === "mcp" && t("mcp.unifiedPanel.title")}
                  {currentView === "agents" && t("agents.title")}
                  {currentView === "universal" &&
                    t("universalProvider.title", {
                      defaultValue: "统一供应商",
                    })}
                </h1>
              </div>
            ) : (
              <>
                <div className="flex items-center gap-2">
                  <a
                    href="https://github.com/farion1231/cc-switch"
                    target="_blank"
                    rel="noreferrer"
                    className={cn(
                      "text-xl font-semibold transition-colors",
                      isProxyRunning && isCurrentAppTakeoverActive
                        ? "text-emerald-500 hover:text-emerald-600 dark:text-emerald-400 dark:hover:text-emerald-300"
                        : "text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300",
                    )}
                  >
                    CC Switch
                  </a>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => setCurrentView("settings")}
                    title={t("common.settings")}
                    className="hover:bg-black/5 dark:hover:bg-white/5"
                  >
                    <Settings className="w-4 h-4" />
                  </Button>
                </div>
                <UpdateBadge onClick={() => setCurrentView("settings")} />
              </>
            )}
          </div>

          <div
            className="flex items-center gap-2 h-[32px]"
            style={{ WebkitAppRegion: "no-drag" } as any}
          >
            {currentView === "prompts" && (
              <Button
                size="icon"
                onClick={() => promptPanelRef.current?.openAdd()}
                className={`ml-auto ${addActionButtonClass}`}
                title={t("prompts.add")}
              >
                <Plus className="w-5 h-5" />
              </Button>
            )}
            {currentView === "skills" && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => skillsPageRef.current?.refresh()}
                  className="hover:bg-black/5 dark:hover:bg-white/5"
                >
                  <RefreshCw className="w-4 h-4 mr-2" />
                  {t("skills.refresh")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => skillsPageRef.current?.openRepoManager()}
                  className="hover:bg-black/5 dark:hover:bg-white/5"
                >
                  <Settings className="w-4 h-4 mr-2" />
                  {t("skills.repoManager")}
                </Button>
              </>
            )}
            {currentView === "providers" && (
              <>
                <div className="flex items-center gap-2">
                  <ProxyToggle activeApp={activeApp} />
                  <AppSwitcher activeApp={activeApp} onSwitch={setActiveApp} />
                  <EnvironmentSwitcher />
                </div>

                <div className="flex items-center gap-1 p-1 bg-muted rounded-xl">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setCurrentView("skills")}
                    className={cn(
                      "text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5",
                      "transition-all duration-200 ease-in-out overflow-hidden",
                      hasSkillsSupport
                        ? "opacity-100 w-8 scale-100 px-2"
                        : "opacity-0 w-0 scale-75 pointer-events-none px-0 -ml-1",
                    )}
                    title={t("skills.manage")}
                  >
                    <Wrench className="flex-shrink-0 w-4 h-4" />
                  </Button>
                  {/* TODO: Agents 功能开发中，暂时隐藏入口 */}
                  {/* {isClaudeApp && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setCurrentView("agents")}
                        className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5"
                        title="Agents"
                      >
                        <Bot className="w-4 h-4" />
                      </Button>
                    )} */}
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setCurrentView("prompts")}
                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5"
                    title={t("prompts.manage")}
                  >
                    <Book className="w-4 h-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setCurrentView("mcp")}
                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5"
                    title={t("mcp.title")}
                  >
                    <Server className="w-4 h-4" />
                  </Button>
                </div>

                <Button
                  onClick={() => setIsAddOpen(true)}
                  size="icon"
                  className={`ml-2 ${addActionButtonClass}`}
                >
                  <Plus className="w-5 h-5" />
                </Button>
              </>
            )}
            {currentView === "mcp" && (
              <div className="flex items-center gap-2">
                <EnvironmentSwitcher />
                <Button
                  size="icon"
                  onClick={() => mcpPanelRef.current?.openAdd()}
                  className={`ml-auto ${addActionButtonClass}`}
                  title={t("mcp.unifiedPanel.addServer")}
                >
                  <Plus className="w-5 h-5" />
                </Button>
              </div>
            )}
          </div>
        </div>
      </header>

      <main className="flex-1 pb-12 animate-fade-in ">
        <div className="pb-12">{renderContent()}</div>
      </main>

      <AddProviderDialog
        open={isAddOpen}
        onOpenChange={setIsAddOpen}
        appId={activeApp}
        onSubmit={addProvider}
      />

      <EditProviderDialog
        open={isEditingDialogOpen}
        provider={effectiveEditingProvider}
        onOpenChange={(open) => {
          if (!open) {
            setEditingProviderId(null);
          }
        }}
        onSubmit={handleEditProvider}
        appId={activeApp}
        isProxyTakeover={isProxyRunning && isCurrentAppTakeoverActive}
      />

      {effectiveUsageProvider && (
        <UsageScriptModal
          provider={effectiveUsageProvider}
          appId={activeApp}
          isOpen={isUsageModalOpen}
          onClose={() => setUsageProviderId(null)}
          onSave={(script) => {
            if (effectiveUsageProvider) {
              void saveUsageScript(effectiveUsageProvider, script);
            }
          }}
        />
      )}

      <ConfirmDialog
        isOpen={Boolean(confirmDelete)}
        title={t("confirm.deleteProvider")}
        message={
          confirmDelete
            ? t("confirm.deleteProviderMessage", {
                name: confirmDelete.name,
              })
            : ""
        }
        onConfirm={() => void handleConfirmDelete()}
        onCancel={() => setConfirmDelete(null)}
      />

      <DeepLinkImportDialog />
    </div>
  );
}

export default App;
