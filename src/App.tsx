import { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Settings,
  ArrowLeft,
  Minus,
  Maximize2,
  Minimize2,
  X,
  Book,
  Brain,
  Wrench,
  History,
  BarChart2,
  Download,
  FolderArchive,
  Search,
  FolderOpen,
  KeyRound,
  Shield,
  Cpu,
  LayoutDashboard,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { VisibleApps } from "@/types";
import type { EnvConflict } from "@/types/env";
import { useSettingsQuery } from "@/lib/query";
import { providersApi, type AppId } from "@/lib/api";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";
import { useOpenClawHealth } from "@/hooks/useOpenClaw";
import { useOpenHermesWebUI } from "@/hooks/useHermes";
import { hermesApi } from "@/lib/api/hermes";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useAutoCompact } from "@/hooks/useAutoCompact";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useScanUnmanagedSkills } from "@/hooks/useSkills";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isTextEditableTarget } from "@/utils/domUtils";
import { cn } from "@/lib/utils";
import {
  isWindows,
  isLinux,
  DRAG_REGION_ATTR,
  DRAG_REGION_STYLE,
} from "@/lib/platform";
import { AppSwitcher } from "@/components/AppSwitcher";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ManagedProviderWorkspace } from "@/components/providers/ManagedProviderWorkspace";
import type { ProviderCatalogHandle } from "@/components/providers/ProviderCatalogHandle";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { UpdateBadge } from "@/components/UpdateBadge";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import PromptPanel from "@/components/prompts/PromptPanel";
import {
  SkillsPage,
  getSkillsPageHeaderActions,
  type SkillsPageSource,
} from "@/components/skills/SkillsPage";
import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import { AgentsPanel } from "@/components/agents/AgentsPanel";
import { UniversalProviderPanel } from "@/components/universal";
import { McpIcon } from "@/components/BrandIcons";
import { Button } from "@/components/ui/button";
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";
import {
  APP_CAPABILITIES,
  APP_IDS,
  canShowUsageDashboard,
} from "@/config/appConfig";
import WorkspaceFilesPanel from "@/components/workspace/WorkspaceFilesPanel";
import EnvPanel from "@/components/openclaw/EnvPanel";
import ToolsPanel from "@/components/openclaw/ToolsPanel";
import AgentsDefaultsPanel from "@/components/openclaw/AgentsDefaultsPanel";
import OpenClawHealthBanner from "@/components/openclaw/OpenClawHealthBanner";
import HermesMemoryPanel from "@/components/hermes/HermesMemoryPanel";
import { CursorModelPanel } from "@/components/cursor/CursorModelPanel";
import { CursorRuntimeToggle } from "@/components/cursor/CursorRuntimeToggle";

type View =
  | "providers"
  | "settings"
  | "prompts"
  | "skills"
  | "skillsDiscovery"
  | "mcp"
  | "agents"
  | "universal"
  | "sessions"
  | "workspace"
  | "openclawEnv"
  | "openclawTools"
  | "openclawAgents"
  | "hermesMemory";

interface SyncStatusUpdatedPayload {
  source?: string;
  status?: string;
  error?: string;
}

const DEFAULT_DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px
const HEADER_HEIGHT = 64; // px

const STORAGE_KEY = "cc-switch-last-app";

const getInitialApp = (): AppId => {
  const saved = localStorage.getItem(STORAGE_KEY) as AppId | null;
  if (saved && APP_IDS.includes(saved)) {
    return saved;
  }
  return "claude";
};

const VIEW_STORAGE_KEY = "cc-switch-last-view";
const VALID_VIEWS: View[] = [
  "providers",
  "settings",
  "prompts",
  "skills",
  "skillsDiscovery",
  "mcp",
  "agents",
  "universal",
  "sessions",
  "workspace",
  "openclawEnv",
  "openclawTools",
  "openclawAgents",
  "hermesMemory",
];

const getInitialView = (): View => {
  const saved = localStorage.getItem(VIEW_STORAGE_KEY) as View | null;
  if (saved && VALID_VIEWS.includes(saved)) {
    return saved;
  }
  return "providers";
};

const supportsView = (appId: AppId, view: View): boolean => {
  const capabilities = APP_CAPABILITIES[appId];
  switch (view) {
    case "prompts":
      return capabilities.prompts && capabilities.sharedFeatureAppId !== null;
    case "skills":
    case "skillsDiscovery":
      return capabilities.skills && capabilities.sharedFeatureAppId !== null;
    case "mcp":
      return capabilities.mcp;
    case "sessions":
      return capabilities.sessions && capabilities.sharedFeatureAppId !== null;
    case "workspace":
    case "openclawEnv":
    case "openclawTools":
    case "openclawAgents":
      return appId === "openclaw";
    case "hermesMemory":
      return appId === "hermes";
    default:
      return true;
  }
};

function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [activeApp, setActiveApp] = useState<AppId>(getInitialApp);
  const capabilities = APP_CAPABILITIES[activeApp];
  const managedApp = capabilities.managedAppId;
  const sharedFeatureApp = capabilities.sharedFeatureAppId;
  const hasProviderToolbarActions =
    activeApp === "hermes" ||
    activeApp === "openclaw" ||
    capabilities.skills ||
    capabilities.prompts ||
    capabilities.sessions ||
    capabilities.mcp;
  const [currentView, setCurrentView] = useState<View>(getInitialView);
  const [skillsDiscoverySource, setSkillsDiscoverySource] =
    useState<SkillsPageSource>("repos");
  const [settingsDefaultTab, setSettingsDefaultTab] = useState("general");
  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

  useEffect(() => {
    localStorage.setItem(VIEW_STORAGE_KEY, currentView);
  }, [currentView]);

  const { data: settingsData } = useSettingsQuery();
  const useAppWindowControls =
    isLinux() && (settingsData?.useAppWindowControls ?? false);
  const dragBarHeight = useAppWindowControls ? 32 : DEFAULT_DRAG_BAR_HEIGHT;
  const contentTopOffset = dragBarHeight + HEADER_HEIGHT;
  const visibleApps: VisibleApps = settingsData?.visibleApps ?? {
    claude: true,
    "claude-desktop": true,
    codex: true,
    gemini: true,
    grokbuild: true,
    opencode: true,
    openclaw: true,
    hermes: true,
    cursor: true,
  };

  const getFirstVisibleApp = (): AppId => {
    if (visibleApps.claude) return "claude";
    if (visibleApps["claude-desktop"]) return "claude-desktop";
    if (visibleApps.codex) return "codex";
    if (visibleApps.gemini) return "gemini";
    if (visibleApps.grokbuild) return "grokbuild";
    if (visibleApps.opencode) return "opencode";
    if (visibleApps.openclaw) return "openclaw";
    if (visibleApps.hermes) return "hermes";
    if (visibleApps.cursor) return "cursor";
    return "claude"; // fallback
  };

  useEffect(() => {
    if (!visibleApps[activeApp]) {
      setActiveApp(getFirstVisibleApp());
    }
  }, [visibleApps, activeApp]);

  useEffect(() => {
    if (!supportsView(activeApp, currentView)) {
      setCurrentView("providers");
    }
  }, [activeApp, currentView]);

  const [envConflicts, setEnvConflicts] = useState<EnvConflict[]>([]);
  const [showEnvBanner, setShowEnvBanner] = useState(false);

  const toolbarRef = useRef<HTMLDivElement>(null);
  const isToolbarCompact = useAutoCompact(toolbarRef);

  useUsageCacheBridge();

  const promptPanelRef = useRef<any>(null);
  const providerCatalogRef = useRef<ProviderCatalogHandle>(null);
  const mcpPanelRef = useRef<any>(null);
  const skillsPageRef = useRef<any>(null);
  const unifiedSkillsPanelRef = useRef<any>(null);
  // 订阅未管理 Skill 的共享缓存（实际扫描由 UnifiedSkillsPanel 进入页面时触发）。
  // 这里 enabled 默认 false，仅用于「导入」按钮的绿点提示，不主动发起扫描。
  const { data: unmanagedSkills } = useScanUnmanagedSkills();
  const hasUnmanagedSkills = (unmanagedSkills?.length ?? 0) > 0;
  const addActionButtonClass =
    "bg-orange-500 hover:bg-orange-600 dark:bg-orange-500 dark:hover:bg-orange-600 text-white shadow-lg shadow-orange-500/30 dark:shadow-orange-500/40 rounded-full w-8 h-8";

  const {
    isRunning: isProxyRunning,
    takeoverStatus,
    status: proxyStatus,
  } = useProxyStatus();
  const isCurrentAppTakeoverActive =
    managedApp !== null && (takeoverStatus?.[managedApp] || false);
  const showUsageDashboard = canShowUsageDashboard(
    capabilities,
    isCurrentAppTakeoverActive,
  );
  const activeProviderId =
    managedApp === null
      ? undefined
      : proxyStatus?.active_targets?.find(
          (target) => target.app_type === managedApp,
        )?.provider_id;
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

  useTauriEvent("universal-provider-synced", async () => {
    await queryClient.invalidateQueries({ queryKey: ["providers"] });
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to update tray menu", error);
    }
  });

  // 应用项目后刷新相关缓存（providers 由既有 provider-switched 监听承接；
  // proxy 状态由后端直接改 DB，不走 mutation，必须显式刷新）
  useTauriEvent("profile-applied", async () => {
    await queryClient.invalidateQueries({ queryKey: ["profiles"] });
    await queryClient.invalidateQueries({ queryKey: ["mcp", "all"] });
    await queryClient.invalidateQueries({ queryKey: ["skills"] });
    await queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
    await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    await queryClient.invalidateQueries({
      queryKey: ["providers", "claude-desktop"],
    });
  });

  useTauriEvent<SyncStatusUpdatedPayload | null | undefined>(
    "webdav-sync-status-updated",
    async (payload) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t("settings.webdavSync.autoSyncFailedToast", {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    },
  );

  useTauriEvent<SyncStatusUpdatedPayload | null | undefined>(
    "s3-sync-status-updated",
    async (payload) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t("settings.s3Sync.autoSyncFailedToast", {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    },
  );

  useTauriEvent<{ appType: string; providerName: string }>(
    "proxy-official-warning",
    (payload) => {
      toast.warning(
        t("notifications.proxyOfficialWarning", {
          name: payload.providerName,
          defaultValue: `当前供应商 ${payload.providerName} 是官方供应商，建议切换到第三方供应商后再使用代理接管`,
        }),
        { duration: 8000 },
      );
    },
  );

  useEffect(() => {
    let active = true;
    let unlistenResize: (() => void) | undefined;

    const setupWindowStateSync = async () => {
      try {
        const currentWindow = getCurrentWindow();
        const syncWindowMaximizedState = async () => {
          const maximized = await currentWindow.isMaximized();
          if (active) {
            setIsWindowMaximized(maximized);
          }
        };

        await syncWindowMaximizedState();
        unlistenResize = await currentWindow.onResized(() => {
          void syncWindowMaximizedState();
        });
      } catch (error) {
        console.error("[App] Failed to sync window maximized state", error);
      }
    };

    void setupWindowStateSync();
    return () => {
      active = false;
      unlistenResize?.();
    };
  }, []);

  useEffect(() => {
    // settingsData 未加载时跳过，避免用 fallback false 覆盖 Rust 侧已设好的装饰状态
    if (!settingsData) return;

    const syncWindowDecorations = async () => {
      try {
        await getCurrentWindow().setDecorations(!useAppWindowControls);
      } catch (error) {
        console.error("[App] Failed to update window decorations", error);
      }
    };

    void syncWindowDecorations();
  }, [useAppWindowControls, settingsData]);

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

  useEffect(() => {
    const checkSkillsMigration = async () => {
      try {
        const result = await invoke<{ count: number; error?: string } | null>(
          "get_skills_migration_result",
        );
        if (result?.error) {
          toast.error(t("migration.skillsFailed"), {
            description: t("migration.skillsFailedDescription"),
            closeButton: true,
          });
          console.error("[App] Skills SSOT migration failed:", result.error);
          return;
        }
        if (result && result.count > 0) {
          toast.success(t("migration.skillsSuccess", { count: result.count }), {
            closeButton: true,
          });
          await queryClient.invalidateQueries({ queryKey: ["skills"] });
        }
      } catch (error) {
        console.error("[App] Failed to check skills migration result:", error);
      }
    };

    checkSkillsMigration();
  }, [t, queryClient]);

  useEffect(() => {
    const checkEnvOnSwitch = async () => {
      if (!capabilities.environmentConflictCheck || !managedApp) return;
      try {
        const conflicts = await checkEnvConflicts(managedApp);

        if (conflicts.length > 0) {
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
  }, [capabilities.environmentConflictCheck, managedApp]);

  const currentViewRef = useRef(currentView);

  useEffect(() => {
    currentViewRef.current = currentView;
  }, [currentView]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "," && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setCurrentView("settings");
        return;
      }

      if (event.key !== "Escape" || event.defaultPrevented) return;

      if (document.body.style.overflow === "hidden") return;

      const view = currentViewRef.current;
      if (view === "providers") return;

      if (isTextEditableTarget(event.target)) return;

      event.preventDefault();
      setCurrentView(view === "skillsDiscovery" ? "skills" : "providers");
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  const [launchDashboardOpen, setLaunchDashboardOpen] = useState(false);
  const openHermesWebUI = useOpenHermesWebUI(() =>
    setLaunchDashboardOpen(true),
  );

  const handleImportSuccess = async () => {
    try {
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
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  };

  const notifyWindowControlError = (error: unknown) => {
    toast.error(
      t("notifications.windowControlFailed", {
        defaultValue: "窗口控制失败：{{error}}",
        error: extractErrorMessage(error),
      }),
    );
  };

  const handleWindowMinimize = async () => {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      console.error("[App] Failed to minimize window", error);
      notifyWindowControlError(error);
    }
  };

  const handleWindowToggleMaximize = async () => {
    try {
      const currentWindow = getCurrentWindow();
      await currentWindow.toggleMaximize();
      setIsWindowMaximized(await currentWindow.isMaximized());
    } catch (error) {
      console.error("[App] Failed to toggle maximize", error);
      notifyWindowControlError(error);
    }
  };

  const handleWindowClose = async () => {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      console.error("[App] Failed to close window", error);
      notifyWindowControlError(error);
    }
  };

  const handleOpenSkillsDiscovery = () => {
    setSkillsDiscoverySource("repos");
    setCurrentView("skillsDiscovery");
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
              defaultTab={settingsDefaultTab}
            />
          );
        case "prompts":
          return sharedFeatureApp ? (
            <PromptPanel
              ref={promptPanelRef}
              open={true}
              onOpenChange={() => setCurrentView("providers")}
              appId={sharedFeatureApp}
            />
          ) : null;
        case "hermesMemory":
          return <HermesMemoryPanel />;
        case "skills":
          return sharedFeatureApp ? (
            <UnifiedSkillsPanel
              ref={unifiedSkillsPanelRef}
              onOpenDiscovery={handleOpenSkillsDiscovery}
              currentApp={
                sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp
              }
            />
          ) : null;
        case "skillsDiscovery":
          return sharedFeatureApp ? (
            <SkillsPage
              ref={skillsPageRef}
              initialApp={
                sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp
              }
              onSourceChange={setSkillsDiscoverySource}
            />
          ) : null;
        case "mcp":
          return (
            <UnifiedMcpPanel
              ref={mcpPanelRef}
              onOpenChange={() => setCurrentView("providers")}
            />
          );
        case "agents":
          return (
            <AgentsPanel onOpenChange={() => setCurrentView("providers")} />
          );
        case "universal":
          return (
            <div className="px-6 pt-4">
              <UniversalProviderPanel />
            </div>
          );

        case "sessions":
          return sharedFeatureApp ? (
            <SessionManagerPage
              key={sharedFeatureApp}
              appId={sharedFeatureApp}
            />
          ) : null;
        case "workspace":
          return <WorkspaceFilesPanel />;
        case "openclawEnv":
          return <EnvPanel />;
        case "openclawTools":
          return <ToolsPanel />;
        case "openclawAgents":
          return <AgentsDefaultsPanel />;
        default:
          if (!capabilities.providerCatalog) return null;

          return capabilities.providerFlow === "cursor-runtime" ? (
            <CursorModelPanel ref={providerCatalogRef} />
          ) : managedApp ? (
            <ManagedProviderWorkspace
              ref={providerCatalogRef}
              appId={managedApp}
              isProxyRunning={isProxyRunning}
              isProxyTakeover={isProxyRunning && isCurrentAppTakeoverActive}
              activeProviderId={activeProviderId}
            />
          ) : null;
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
          {content}
        </motion.div>
      </AnimatePresence>
    );
  };

  return (
    <div
      className="flex flex-col h-screen overflow-hidden bg-background text-foreground selection:bg-primary/30 pb-4"
      style={{ overflowX: "hidden", paddingTop: contentTopOffset }}
    >
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
                onClick={() => void handleWindowMinimize()}
                title={t("header.windowMinimize")}
                className="h-7 w-7"
              >
                <Minus className="w-4 h-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => void handleWindowToggleMaximize()}
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
                onClick={() => void handleWindowClose()}
                title={t("header.windowClose")}
                className="h-7 w-7 hover:bg-red-500/15 hover:text-red-500"
              >
                <X className="w-4 h-4" />
              </Button>
            </div>
          )}
        </div>
      )}
      {showEnvBanner && envConflicts.length > 0 && (
        <EnvWarningBanner
          conflicts={envConflicts}
          onDismiss={() => {
            setShowEnvBanner(false);
            sessionStorage.setItem("env_banner_dismissed", "true");
          }}
          onDeleted={async () => {
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
        {...DRAG_REGION_ATTR}
        style={
          {
            ...DRAG_REGION_STYLE,
            top: dragBarHeight,
            height: HEADER_HEIGHT,
          } as any
        }
      >
        <div
          className="flex h-full items-center justify-between gap-2 px-6"
          {...DRAG_REGION_ATTR}
          style={{ ...DRAG_REGION_STYLE } as any}
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
                  onClick={() =>
                    setCurrentView(
                      currentView === "skillsDiscovery"
                        ? "skills"
                        : "providers",
                    )
                  }
                  className="mr-2 rounded-lg"
                >
                  <ArrowLeft className="w-4 h-4" />
                </Button>
                <h1 className="text-lg font-semibold">
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
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <div className="relative inline-flex items-center">
                  <a
                    href="https://ccswitch.io"
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
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => {
                    setSettingsDefaultTab("general");
                    setCurrentView("settings");
                  }}
                  title={t("common.settings")}
                  className="hover:bg-black/5 dark:hover:bg-white/5"
                >
                  <Settings className="w-4 h-4" />
                </Button>
                <UpdateBadge
                  onClick={() => {
                    setSettingsDefaultTab("about");
                    setCurrentView("settings");
                  }}
                />
                {showUsageDashboard && (
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => {
                      setSettingsDefaultTab("usage");
                      setCurrentView("settings");
                    }}
                    title={t("usage.title", {
                      defaultValue: "使用统计",
                    })}
                    className="hover:bg-black/5 dark:hover:bg-white/5"
                  >
                    <BarChart2 className="w-4 h-4" />
                  </Button>
                )}
              </div>
            )}
          </div>

          <div className="flex flex-1 min-w-0 items-center justify-end gap-1.5">
            {currentView === "providers" &&
              capabilities.routingControl === "local-runtime" && (
                <div
                  className="flex shrink-0 items-center gap-1.5"
                  style={{ WebkitAppRegion: "no-drag" } as any}
                >
                  <CursorRuntimeToggle />
                </div>
              )}
            {currentView === "providers" &&
              (capabilities.routingControl === "generic-proxy" ||
                capabilities.routingControl === "claude-desktop") &&
              managedApp && (
                <div
                  className="flex shrink-0 items-center gap-1.5"
                  style={{ WebkitAppRegion: "no-drag" } as any}
                >
                  {capabilities.routingControl === "claude-desktop" ? (
                    <ClaudeDesktopRouteToggle />
                  ) : (
                    settingsData?.enableLocalProxy && (
                      <ProxyToggle activeApp={managedApp} />
                    )
                  )}
                  {capabilities.failover &&
                    settingsData?.enableFailoverToggle && (
                      <FailoverToggle activeApp={managedApp} />
                    )}
                </div>
              )}
            {currentView === "providers" &&
              capabilities.profiles &&
              (settingsData?.showProfileSwitcher ?? true) && (
                <div
                  className="flex shrink-0 items-center"
                  style={{ WebkitAppRegion: "no-drag" } as any}
                >
                  <ProfileSwitcher activeApp={activeApp} />
                </div>
              )}
            <div
              ref={toolbarRef}
              className="flex flex-1 min-w-0 overflow-x-hidden items-center py-4 pr-2"
            >
              <div
                className="flex shrink-0 items-center gap-1.5 ml-auto"
                style={{ WebkitAppRegion: "no-drag" } as any}
              >
                {currentView === "prompts" && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => promptPanelRef.current?.openAdd()}
                    className="hover:bg-black/5 dark:hover:bg-white/5"
                  >
                    <Plus className="w-4 h-4 mr-2" />
                    {t("prompts.add")}
                  </Button>
                )}
                {currentView === "mcp" && (
                  <>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => mcpPanelRef.current?.openImport()}
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <Download className="w-4 h-4 mr-2" />
                      {t("mcp.importExisting")}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => mcpPanelRef.current?.openAdd()}
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <Plus className="w-4 h-4 mr-2" />
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
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <History className="w-4 h-4 mr-2" />
                      {t("skills.restoreFromBackup.button")}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() =>
                        unifiedSkillsPanelRef.current?.openInstallFromZip()
                      }
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <FolderArchive className="w-4 h-4 mr-2" />
                      {t("skills.installFromZip.button")}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() =>
                        unifiedSkillsPanelRef.current?.openImport()
                      }
                      className="relative hover:bg-black/5 dark:hover:bg-white/5"
                      title={
                        hasUnmanagedSkills
                          ? t("skills.unmanagedAvailable")
                          : undefined
                      }
                    >
                      <Download className="w-4 h-4 mr-2" />
                      {t("skills.import")}
                      {hasUnmanagedSkills && (
                        <span
                          className="absolute top-1 right-1 h-2 w-2 rounded-full bg-green-500"
                          aria-hidden="true"
                        />
                      )}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={handleOpenSkillsDiscovery}
                      className="hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      <Search className="w-4 h-4 mr-2" />
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
                          className="hover:bg-black/5 dark:hover:bg-white/5"
                        >
                          <Icon className="w-4 h-4 mr-2" />
                          {t(labelKey)}
                        </Button>
                      ),
                    )}
                  </>
                )}
                {currentView === "providers" && (
                  <>
                    <AppSwitcher
                      activeApp={activeApp}
                      onSwitch={setActiveApp}
                      visibleApps={visibleApps}
                      compact={isToolbarCompact}
                    />

                    {hasProviderToolbarActions && (
                      <div className="flex items-center gap-1 p-1 bg-muted rounded-xl">
                        <AnimatePresence mode="wait">
                          <motion.div
                            key={
                              activeApp === "openclaw"
                                ? "openclaw"
                                : activeApp === "hermes"
                                  ? "hermes"
                                  : activeApp === "grokbuild"
                                    ? "grokbuild"
                                    : "default"
                            }
                            className="flex items-center gap-1"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            transition={{ duration: 0.15 }}
                          >
                            {activeApp === "hermes" ? (
                              <>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => setCurrentView("skills")}
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("skills.manage")}
                                >
                                  <Wrench className="w-4 h-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => setCurrentView("hermesMemory")}
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("hermes.memory.title")}
                                >
                                  <Brain className="w-4 h-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => void openHermesWebUI()}
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("hermes.webui.open")}
                                >
                                  <LayoutDashboard className="w-4 h-4" />
                                </Button>
                                {capabilities.mcp && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setCurrentView("mcp")}
                                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                    title={t("mcp.title")}
                                  >
                                    <McpIcon size={16} />
                                  </Button>
                                )}
                              </>
                            ) : activeApp === "openclaw" ? (
                              <>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => setCurrentView("workspace")}
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("workspace.manage")}
                                >
                                  <FolderOpen className="w-4 h-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => setCurrentView("openclawEnv")}
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("openclaw.env.title")}
                                >
                                  <KeyRound className="w-4 h-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() =>
                                    setCurrentView("openclawTools")
                                  }
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("openclaw.tools.title")}
                                >
                                  <Shield className="w-4 h-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() =>
                                    setCurrentView("openclawAgents")
                                  }
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("openclaw.agents.title")}
                                >
                                  <Cpu className="w-4 h-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => setCurrentView("sessions")}
                                  className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                  title={t("sessionManager.title")}
                                >
                                  <History className="w-4 h-4" />
                                </Button>
                              </>
                            ) : (
                              <>
                                {capabilities.skills && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setCurrentView("skills")}
                                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                    title={t("skills.manage")}
                                  >
                                    <Wrench className="w-4 h-4" />
                                  </Button>
                                )}
                                {capabilities.prompts && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setCurrentView("prompts")}
                                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                    title={t("prompts.manage")}
                                  >
                                    <Book className="w-4 h-4" />
                                  </Button>
                                )}
                                {capabilities.sessions && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setCurrentView("sessions")}
                                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                    title={t("sessionManager.title")}
                                  >
                                    <History className="w-4 h-4" />
                                  </Button>
                                )}
                                {capabilities.mcp && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setCurrentView("mcp")}
                                    className="text-muted-foreground hover:text-foreground hover:bg-black/5 dark:hover:bg-white/5 w-8 px-2"
                                    title={t("mcp.title")}
                                  >
                                    <McpIcon size={16} />
                                  </Button>
                                )}
                              </>
                            )}
                          </motion.div>
                        </AnimatePresence>
                      </div>
                    )}

                    {capabilities.providerCatalog && (
                      <Button
                        onClick={() => providerCatalogRef.current?.openCreate()}
                        size="icon"
                        title={
                          capabilities.providerFlow === "cursor-runtime"
                            ? "添加 Cursor Endpoint"
                            : undefined
                        }
                        className={`ml-2 ${addActionButtonClass}`}
                      >
                        <Plus className="w-5 h-5" />
                      </Button>
                    )}
                  </>
                )}
              </div>
            </div>
          </div>
        </div>
      </header>

      <main className="flex-1 min-h-0 flex flex-col overflow-y-auto animate-fade-in">
        {isOpenClawView && openclawHealthWarnings.length > 0 && (
          <OpenClawHealthBanner warnings={openclawHealthWarnings} />
        )}
        {renderContent()}
      </main>

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
