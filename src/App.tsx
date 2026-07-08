import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import {
  Plus,
  ArrowLeft,
  Minus,
  Maximize2,
  Minimize2,
  X,
  Download,
  FolderArchive,
  Search,
  History,
} from "lucide-react";
import type { Provider, VisibleApps } from "@/types";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { useProviderActions } from "@/hooks/useProviderActions";
import { useOpenClawHealth } from "@/hooks/useOpenClaw";
import { useOpenHermesWebUI } from "@/hooks/useHermes";
import { hermesApi } from "@/lib/api/hermes";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { useScanUnmanagedSkills } from "@/hooks/useSkills";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isWindows, isLinux } from "@/lib/platform";
import { useDisableCurrentOmo, useDisableCurrentOmoSlim } from "@/lib/query/omo";

import { useAppShellState } from "@/app/useAppShellState";
import { useWindowChrome } from "@/app/useWindowChrome";
import { useStartupNotices } from "@/app/useStartupNotices";
import { useGlobalShortcuts } from "@/app/useGlobalShortcuts";
import { useAppEvents } from "@/app/useAppEvents";
import { useProviderWorkflow } from "@/app/useProviderWorkflow";
import { ViewFallback } from "@/app/ViewFallback";
import {
  LazySettingsPage,
  LazyPromptPanel,
  LazyUnifiedMcpPanel,
  LazyUnifiedSkillsPanel,
  LazySkillsPage,
  LazyAgentsPanel,
  LazyUniversalProviderPanel,
  LazySessionManagerPage,
  LazyWorkspaceFilesPanel,
  LazyEnvPanel,
  LazyToolsPanel,
  LazyAgentsDefaultsPanel,
  LazyHermesMemoryPanel,
  LazyAddProviderDialog,
  LazyEditProviderDialog,
  LazyUsageScriptModal,
} from "@/app/lazyViews";

import { Sidebar } from "@/components/Sidebar";
import { ProviderList } from "@/components/providers/ProviderList";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { UpdateBadge } from "@/components/UpdateBadge";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import {
  getSkillsPageHeaderActions,
  type SkillsPageSource,
} from "@/components/skills/skillsPageHeaderActions";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import OpenClawHealthBanner from "@/components/openclaw/OpenClawHealthBanner";
import { Button } from "@/components/ui/button";

const DEFAULT_DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px
const HEADER_HEIGHT = 64; // px

const DEFAULT_VISIBLE_APPS: VisibleApps = {
  claude: true,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  opencode: true,
  openclaw: true,
  hermes: true,
};

/** 首次条件满足后保持 true —— 用于懒加载对话框「首次打开才挂载，之后常驻」 */
function useMountWhen(condition: boolean): boolean {
  const [mounted, setMounted] = useState(condition);
  useEffect(() => {
    if (condition) {
      setMounted(true);
    }
  }, [condition]);
  return mounted;
}

function App() {
  const { t } = useTranslation();

  const { data: settingsData } = useSettingsQuery();
  const useAppWindowControls =
    isLinux() && (settingsData?.useAppWindowControls ?? false);
  const dragBarHeight = useAppWindowControls ? 32 : DEFAULT_DRAG_BAR_HEIGHT;
  const visibleApps: VisibleApps =
    settingsData?.visibleApps ?? DEFAULT_VISIBLE_APPS;

  /* ── 外壳状态 / 窗口 / 启动通知 / 快捷键 ───────── */
  const {
    activeApp,
    setActiveApp,
    sharedFeatureApp,
    currentView,
    setCurrentView,
    hasSkillsSupport,
    hasSessionSupport,
  } = useAppShellState(visibleApps);

  const { isWindowMaximized, minimize, toggleMaximize, close } =
    useWindowChrome(useAppWindowControls, Boolean(settingsData));

  const { envConflicts, showEnvBanner, dismissEnvBanner, recheckEnvConflicts } =
    useStartupNotices(activeApp);

  useGlobalShortcuts(currentView, setCurrentView);
  useUsageCacheBridge();

  /* ── 头部辅助状态 ───────────────────────────── */
  const [skillsDiscoverySource, setSkillsDiscoverySource] =
    useState<SkillsPageSource>("repos");
  const [settingsDefaultTab, setSettingsDefaultTab] = useState("general");
  const [isAddOpen, setIsAddOpen] = useState(false);

  const promptPanelRef = useRef<any>(null);
  const mcpPanelRef = useRef<any>(null);
  const skillsPageRef = useRef<any>(null);
  const unifiedSkillsPanelRef = useRef<any>(null);
  // 订阅未管理 Skill 的共享缓存（实际扫描由 UnifiedSkillsPanel 进入页面时触发）。
  // 这里 enabled 默认 false，仅用于「导入」按钮的绿点提示，不主动发起扫描。
  const { data: unmanagedSkills } = useScanUnmanagedSkills();
  const hasUnmanagedSkills = (unmanagedSkills?.length ?? 0) > 0;

  /* ── 代理 / 供应商数据 ──────────────────────── */
  const {
    isRunning: isProxyRunning,
    takeoverStatus,
    status: proxyStatus,
  } = useProxyStatus();
  const isCurrentAppTakeoverActive = takeoverStatus?.[activeApp] || false;
  const activeProviderId = useMemo(() => {
    const target = proxyStatus?.active_targets?.find(
      (t) => t.app_type === activeApp,
    );
    return target?.provider_id;
  }, [proxyStatus?.active_targets, activeApp]);

  const { data, isLoading, refetch } = useProvidersQuery(activeApp, {
    isProxyRunning,
  });
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";

  const isOpenClawView =
    activeApp === "openclaw" &&
    (currentView === "providers" ||
      currentView === "workspace" ||
      currentView === "sessions" ||
      currentView === "openclawEnv" ||
      currentView === "openclawTools" ||
      currentView === "openclawAgents");
  const { data: openclawHealthWarnings = [] } =
    useOpenClawHealth(isOpenClawView);

  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
  } = useProviderActions(
    activeApp,
    isProxyRunning,
    isProxyRunning && isCurrentAppTakeoverActive,
  );

  useAppEvents(activeApp, refetch);

  /* ── 供应商工作流（编辑/删除/复制/终端/导入） ── */
  const {
    editingProvider,
    setEditingProvider,
    usageProvider,
    setUsageProvider,
    confirmAction,
    setConfirmAction,
    handleEditProvider,
    handleConfirmAction,
    handleDuplicateProvider,
    handleOpenTerminal,
    handleOpenWebsite,
    handleImportSuccess,
  } = useProviderWorkflow({
    activeApp,
    providers,
    addProvider,
    updateProvider,
    deleteProvider,
    refetch,
  });

  const effectiveEditingProvider = useLastValidValue(editingProvider);
  const effectiveUsageProvider = useLastValidValue(usageProvider);

  const addDialogMounted = useMountWhen(isAddOpen);
  const editDialogMounted = useMountWhen(Boolean(editingProvider));

  /* ── OMO 停用 ──────────────────────────────── */
  const disableOmoMutation = useDisableCurrentOmo();
  const handleDisableOmo = useCallback(() => {
    disableOmoMutation.mutate(undefined, {
      onSuccess: () => {
        toast.success(t("omo.disabled", { defaultValue: "OMO 已停用" }));
      },
      onError: (error: Error) => {
        toast.error(
          t("omo.disableFailed", {
            defaultValue: "停用 OMO 失败: {{error}}",
            error: extractErrorMessage(error),
          }),
        );
      },
    });
  }, [disableOmoMutation, t]);

  const disableOmoSlimMutation = useDisableCurrentOmoSlim();
  const handleDisableOmoSlim = useCallback(() => {
    disableOmoSlimMutation.mutate(undefined, {
      onSuccess: () => {
        toast.success(t("omo.disabled", { defaultValue: "OMO 已停用" }));
      },
      onError: (error: Error) => {
        toast.error(
          t("omo.disableFailed", {
            defaultValue: "停用 OMO 失败: {{error}}",
            error: extractErrorMessage(error),
          }),
        );
      },
    });
  }, [disableOmoSlimMutation, t]);

  /* ── Hermes Web UI ─────────────────────────── */
  const [launchDashboardOpen, setLaunchDashboardOpen] = useState(false);
  const openHermesWebUI = useOpenHermesWebUI(() =>
    setLaunchDashboardOpen(true),
  );
  const handleOpenHermesWebUI = useCallback(() => {
    void openHermesWebUI();
  }, [openHermesWebUI]);

  /* ── 导航 handlers ─────────────────────────── */
  const handleOpenSettings = useCallback(() => {
    setSettingsDefaultTab("general");
    setCurrentView("settings");
  }, [setCurrentView]);
  const handleOpenUsageSettings = useCallback(() => {
    setSettingsDefaultTab("usage");
    setCurrentView("settings");
  }, [setCurrentView]);
  const handleOpenAboutSettings = useCallback(() => {
    setSettingsDefaultTab("about");
    setCurrentView("settings");
  }, [setCurrentView]);
  const handleOpenAddProvider = useCallback(() => {
    setIsAddOpen(true);
  }, []);
  const handleStartEditingProvider = useCallback(
    (provider: Provider) => {
      setEditingProvider(provider);
    },
    [setEditingProvider],
  );
  const handleRequestDeleteProvider = useCallback(
    (provider: Provider) => {
      setConfirmAction({ provider, action: "delete" });
    },
    [setConfirmAction],
  );
  const handleRequestRemoveFromConfig = useCallback(
    (provider: Provider) => {
      setConfirmAction({ provider, action: "remove" });
    },
    [setConfirmAction],
  );
  const handleOpenSkillsDiscovery = useCallback(() => {
    setSkillsDiscoverySource("repos");
    setCurrentView("skillsDiscovery");
  }, [setCurrentView]);

  /* ── 视图渲染 ──────────────────────────────── */
  const renderContent = () => {
    const content = (() => {
      switch (currentView) {
        case "settings":
          return (
            <LazySettingsPage
              open={true}
              onOpenChange={() => setCurrentView("providers")}
              onImportSuccess={handleImportSuccess}
              defaultTab={settingsDefaultTab}
            />
          );
        case "prompts":
          return (
            <LazyPromptPanel
              ref={promptPanelRef}
              open={true}
              onOpenChange={() => setCurrentView("providers")}
              appId={sharedFeatureApp}
            />
          );
        case "hermesMemory":
          return <LazyHermesMemoryPanel />;
        case "skills":
          return (
            <LazyUnifiedSkillsPanel
              ref={unifiedSkillsPanelRef}
              onOpenDiscovery={handleOpenSkillsDiscovery}
              currentApp={
                sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp
              }
            />
          );
        case "skillsDiscovery":
          return (
            <LazySkillsPage
              ref={skillsPageRef}
              initialApp={
                sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp
              }
              onSourceChange={setSkillsDiscoverySource}
            />
          );
        case "mcp":
          return (
            <LazyUnifiedMcpPanel
              ref={mcpPanelRef}
              onOpenChange={() => setCurrentView("providers")}
            />
          );
        case "agents":
          return (
            <LazyAgentsPanel onOpenChange={() => setCurrentView("providers")} />
          );
        case "universal":
          return (
            <div className="mx-auto w-full max-w-[1480px] px-5 pt-4 md:px-6">
              <LazyUniversalProviderPanel />
            </div>
          );
        case "sessions":
          return (
            <LazySessionManagerPage
              key={sharedFeatureApp}
              appId={sharedFeatureApp}
            />
          );
        case "workspace":
          return <LazyWorkspaceFilesPanel />;
        case "openclawEnv":
          return <LazyEnvPanel />;
        case "openclawTools":
          return <LazyToolsPanel />;
        case "openclawAgents":
          return <LazyAgentsDefaultsPanel />;
        default:
          return (
            <div className="mx-auto flex w-full max-w-[1480px] flex-col flex-1 min-h-0 overflow-hidden px-5 md:px-6">
              <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1">
                <AnimatePresence mode="wait">
                  <motion.div
                    key={activeApp}
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -4 }}
                    transition={{ duration: 0.18, ease: [0.32, 0.72, 0, 1] }}
                    className="space-y-4"
                  >
                    <ProviderList
                      providers={providers}
                      currentProviderId={currentProviderId}
                      appId={activeApp}
                      isLoading={isLoading}
                      isProxyRunning={isProxyRunning}
                      isProxyTakeover={
                        isProxyRunning && isCurrentAppTakeoverActive
                      }
                      activeProviderId={activeProviderId}
                      onSwitch={switchProvider}
                      onEdit={handleStartEditingProvider}
                      onDelete={handleRequestDeleteProvider}
                      onRemoveFromConfig={
                        activeApp === "opencode" ||
                        activeApp === "openclaw" ||
                        activeApp === "hermes"
                          ? handleRequestRemoveFromConfig
                          : undefined
                      }
                      onDisableOmo={
                        activeApp === "opencode" ? handleDisableOmo : undefined
                      }
                      onDisableOmoSlim={
                        activeApp === "opencode"
                          ? handleDisableOmoSlim
                          : undefined
                      }
                      onDuplicate={handleDuplicateProvider}
                      onConfigureUsage={setUsageProvider}
                      onOpenWebsite={handleOpenWebsite}
                      onOpenTerminal={
                        activeApp === "claude" ? handleOpenTerminal : undefined
                      }
                      onCreate={handleOpenAddProvider}
                      onSetAsDefault={
                        activeApp === "openclaw"
                          ? setAsDefaultModel
                          : activeApp === "hermes"
                            ? switchProvider
                            : undefined
                      }
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
          className="flex-1 min-h-0"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
        >
          <Suspense fallback={<ViewFallback />}>{content}</Suspense>
        </motion.div>
      </AnimatePresence>
    );
  };

  return (
    <div className="app-liquid-shell flex h-screen overflow-hidden bg-background text-foreground selection:bg-primary/30">
      {/* ── Tauri Drag Region (full-width, transparent overlay) ── */}
      {(dragBarHeight > 0 || useAppWindowControls) && (
        <div
          className="fixed top-0 left-0 right-0 z-[70] flex items-center justify-end px-2"
          data-tauri-drag-region
          style={{ WebkitAppRegion: "drag", height: dragBarHeight } as any}
        >
          {useAppWindowControls && (
            <div
              className="flex items-center gap-1"
              style={{ WebkitAppRegion: "no-drag" } as any}
            >
              <Button
                variant="ghost"
                size="icon"
                onClick={() => void minimize()}
                title={t("header.windowMinimize")}
                className="h-7 w-7"
              >
                <Minus className="w-4 h-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => void toggleMaximize()}
                title={
                  isWindowMaximized
                    ? t("header.windowRestore")
                    : t("header.windowMaximize")
                }
                className="h-7 w-7"
              >
                {isWindowMaximized ? (
                  <Minimize2 className="w-4 h-4" />
                ) : (
                  <Maximize2 className="w-4 h-4" />
                )}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => void close()}
                title={t("header.windowClose")}
                className="h-7 w-7 hover:bg-red-500/15 hover:text-red-500"
              >
                <X className="w-4 h-4" />
              </Button>
            </div>
          )}
        </div>
      )}

      {/* ── Sidebar ────────────────────────────────────────── */}
      <Sidebar
        activeApp={activeApp}
        onAppSwitch={setActiveApp}
        visibleApps={visibleApps}
        currentView={currentView}
        onViewChange={setCurrentView}
        onOpenSettings={handleOpenSettings}
        onOpenAddProvider={handleOpenAddProvider}
        isProxyRunning={isProxyRunning}
        isTakeoverActive={isCurrentAppTakeoverActive}
        onOpenUsage={
          isCurrentAppTakeoverActive ? handleOpenUsageSettings : undefined
        }
        hasSkillsSupport={hasSkillsSupport}
        hasSessionSupport={hasSessionSupport}
        hasUnmanagedSkills={hasUnmanagedSkills}
        onOpenHermesWebUI={handleOpenHermesWebUI}
      />

      {/* ── Main Content Area ──────────────────────────────── */}
      <div className="flex-1 flex flex-col min-w-0 h-full">
        {showEnvBanner && envConflicts.length > 0 && (
          <EnvWarningBanner
            conflicts={envConflicts}
            onDismiss={dismissEnvBanner}
            onDeleted={() => void recheckEnvConflicts()}
          />
        )}

        {isOpenClawView && openclawHealthWarnings.length > 0 && (
          <OpenClawHealthBanner warnings={openclawHealthWarnings} />
        )}

        {/* ── Content Header ─────────────────────────────────── */}
        <header
          className="content-header shrink-0 flex items-center justify-between px-5 gap-3"
          style={{
            height: HEADER_HEIGHT,
            paddingTop: dragBarHeight || undefined,
          }}
        >
          {/* Left: title / back */}
          <div className="flex items-center gap-2 min-w-0">
            {currentView !== "providers" ? (
              <>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() =>
                    setCurrentView(
                      currentView === "skillsDiscovery"
                        ? "skills"
                        : "providers",
                    )
                  }
                  className="h-8 w-8 shrink-0"
                >
                  <ArrowLeft className="w-4 h-4" />
                </Button>
                <h1 className="text-[0.9375rem] font-semibold tracking-[-0.02em] truncate">
                  {currentView === "settings" && t("settings.title")}
                  {currentView === "prompts" &&
                    t("prompts.title", {
                      appName: t(`apps.${sharedFeatureApp}`),
                    })}
                  {currentView === "skills" && t("skills.title")}
                  {currentView === "skillsDiscovery" && t("skills.title")}
                  {currentView === "mcp" && t("mcp.unifiedPanel.title")}
                  {currentView === "agents" && t("agents.title")}
                  {currentView === "universal" &&
                    t("universalProvider.title", {
                      defaultValue: "统一供应商",
                    })}
                  {currentView === "sessions" && t("sessionManager.title")}
                  {currentView === "workspace" && t("workspace.title")}
                  {currentView === "openclawEnv" && t("openclaw.env.title")}
                  {currentView === "openclawTools" && t("openclaw.tools.title")}
                  {currentView === "openclawAgents" &&
                    t("openclaw.agents.title")}
                  {currentView === "hermesMemory" && t("hermes.memory.title")}
                </h1>
              </>
            ) : (
              <h1 className="text-[0.9375rem] font-semibold tracking-[-0.02em]">
                {t(`apps.${activeApp}`, {
                  defaultValue: activeApp,
                })}
              </h1>
            )}

            <UpdateBadge onClick={handleOpenAboutSettings} />
          </div>

          {/* Right: context-specific toolbar actions */}
          <div className="flex items-center gap-2 shrink-0">
            {currentView === "providers" &&
              activeApp !== "opencode" &&
              activeApp !== "openclaw" &&
              activeApp !== "hermes" && (
                <div className="toolbar-cluster flex items-center gap-1">
                  {activeApp === "claude-desktop" ? (
                    <ClaudeDesktopRouteToggle />
                  ) : (
                    settingsData?.enableLocalProxy && (
                      <ProxyToggle activeApp={activeApp} />
                    )
                  )}
                  {activeApp !== "claude-desktop" &&
                    settingsData?.enableFailoverToggle && (
                      <FailoverToggle activeApp={activeApp} />
                    )}
                </div>
              )}

            {currentView === "prompts" && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => promptPanelRef.current?.openAdd()}
              >
                <Plus className="mr-1.5 h-4 w-4" />
                {t("prompts.add")}
              </Button>
            )}

            {currentView === "mcp" && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => mcpPanelRef.current?.openImport()}
                >
                  <Download className="mr-1.5 h-4 w-4" />
                  {t("mcp.importExisting")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => mcpPanelRef.current?.openAdd()}
                >
                  <Plus className="mr-1.5 h-4 w-4" />
                  {t("mcp.addMcp")}
                </Button>
              </>
            )}

            {currentView === "skills" && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    unifiedSkillsPanelRef.current?.openRestoreFromBackup()
                  }
                >
                  <History className="mr-1.5 h-4 w-4" />
                  {t("skills.restoreFromBackup.button")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    unifiedSkillsPanelRef.current?.openInstallFromZip()
                  }
                >
                  <FolderArchive className="mr-1.5 h-4 w-4" />
                  {t("skills.installFromZip.button")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => unifiedSkillsPanelRef.current?.openImport()}
                  className="relative"
                  title={
                    hasUnmanagedSkills
                      ? t("skills.unmanagedAvailable")
                      : undefined
                  }
                >
                  <Download className="mr-1.5 h-4 w-4" />
                  {t("skills.import")}
                  {hasUnmanagedSkills && (
                    <span
                      className="absolute right-1 top-1 h-2 w-2 rounded-full bg-green-500"
                      aria-hidden="true"
                    />
                  )}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleOpenSkillsDiscovery}
                >
                  <Search className="mr-1.5 h-4 w-4" />
                  {t("skills.discover")}
                </Button>
              </>
            )}

            {currentView === "skillsDiscovery" && (
              <>
                {getSkillsPageHeaderActions(skillsDiscoverySource).map(
                  ({ key, labelKey, Icon, execute }) => (
                    <Button
                      key={key}
                      variant="ghost"
                      size="sm"
                      onClick={() => execute(skillsPageRef.current)}
                    >
                      <Icon className="mr-1.5 h-4 w-4" />
                      {t(labelKey)}
                    </Button>
                  ),
                )}
              </>
            )}
          </div>
        </header>

        {/* ── Scrollable Content ─────────────────────────────── */}
        <main className="flex-1 min-h-0 overflow-y-auto animate-fade-in">
          {renderContent()}
        </main>
      </div>

      {/* ── Dialogs（懒加载：首次打开时才挂载） ─────────────── */}
      {addDialogMounted && (
        <Suspense fallback={null}>
          <LazyAddProviderDialog
            open={isAddOpen}
            onOpenChange={setIsAddOpen}
            appId={activeApp}
            onSubmit={addProvider}
          />
        </Suspense>
      )}

      {editDialogMounted && (
        <Suspense fallback={null}>
          <LazyEditProviderDialog
            open={Boolean(editingProvider)}
            provider={effectiveEditingProvider}
            onOpenChange={(open) => {
              if (!open) {
                setEditingProvider(null);
              }
            }}
            onSubmit={handleEditProvider}
            appId={activeApp}
            isProxyTakeover={isCurrentAppTakeoverActive}
          />
        </Suspense>
      )}

      {effectiveUsageProvider && (
        <Suspense fallback={null}>
          <LazyUsageScriptModal
            key={effectiveUsageProvider.id}
            provider={effectiveUsageProvider}
            appId={activeApp}
            isOpen={Boolean(usageProvider)}
            onClose={() => setUsageProvider(null)}
            onSave={(script) => {
              if (usageProvider) {
                void saveUsageScript(usageProvider, script);
              }
            }}
          />
        </Suspense>
      )}

      <ConfirmDialog
        isOpen={Boolean(confirmAction)}
        title={
          confirmAction?.action === "remove"
            ? t("confirm.removeProvider")
            : t("confirm.deleteProvider")
        }
        message={
          confirmAction
            ? confirmAction.action === "remove"
              ? t("confirm.removeProviderMessage", {
                  name: confirmAction.provider.name,
                })
              : t("confirm.deleteProviderMessage", {
                  name: confirmAction.provider.name,
                })
            : ""
        }
        onConfirm={() => void handleConfirmAction()}
        onCancel={() => setConfirmAction(null)}
      />

      <ConfirmDialog
        isOpen={launchDashboardOpen}
        title={t("hermes.webui.launchConfirmTitle")}
        message={t("hermes.webui.launchConfirmMessage")}
        confirmText={t("hermes.webui.launchConfirmAction")}
        variant="info"
        onConfirm={() => {
          setLaunchDashboardOpen(false);
          void (async () => {
            try {
              await hermesApi.launchDashboard();
              toast.success(t("hermes.webui.launching"));
            } catch (error) {
              toast.error(t("hermes.webui.launchFailed"), {
                description: extractErrorMessage(error) || undefined,
              });
            }
          })();
        }}
        onCancel={() => setLaunchDashboardOpen(false)}
      />

      <DeepLinkImportDialog />
      <FirstRunNoticeDialog />
    </div>
  );
}

export default App;
