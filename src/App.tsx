import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import {
  BookOpen,
  Download,
  ExternalLink,
  FolderArchive,
  History,
  KeyRound,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Server,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Provider, VisibleApps } from "@/types";
import type { EnvConflict } from "@/types/env";
import { proxyKeys, useProvidersQuery, useSettingsQuery } from "@/lib/query";
import {
  piApi,
  providersApi,
  settingsApi,
  type AppId,
  type ProviderSwitchEvent,
} from "@/lib/api";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";
import { useProviderActions } from "@/hooks/useProviderActions";
import { openclawKeys, useOpenClawHealth } from "@/hooks/useOpenClaw";
import { hermesKeys, useOpenHermesWebUI } from "@/hooks/useHermes";
import { hermesApi } from "@/lib/api/hermes";
import type { ProviderEditorSave } from "@/lib/api/providers";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { useScanUnmanagedSkills } from "@/hooks/useSkills";
import { useSidebarCollapsed } from "@/hooks/useSidebarCollapsed";
import {
  extractErrorMessage,
  translatePiProviderMutationError,
} from "@/utils/errorUtils";
import { isTextEditableTarget } from "@/utils/domUtils";
import { deepClone } from "@/utils/deepClone";
import { isLinux } from "@/lib/platform";
import {
  APP_STORAGE_KEY,
  appPageBelongsTo,
  isAppPage,
  readStoredView,
  sharedFeatureAppOf,
  storeView,
  type GlobalPage,
  type SettingsSection,
  type View,
} from "@/lib/navigation";
import { Sidebar } from "@/components/shell/Sidebar";
import {
  AppPageHeader,
  WindowControlsContext,
} from "@/components/shell/AppPageHeader";
import { WindowControls } from "@/components/shell/WindowControls";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { ProviderList } from "@/components/providers/ProviderList";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import { AppsPage } from "@/components/apps/AppsPage";
import {
  checkToolUpdatesInBackground,
  useToolUpdatesAvailable,
} from "@/components/apps/useToolManagement";
import { UsagePage } from "@/components/usage/UsagePage";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import UsageScriptModal from "@/components/UsageScriptModal";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import PromptPanel, {
  type PromptPanelHandle,
  type PromptPrimaryAction,
} from "@/components/prompts/PromptPanel";
import {
  SkillsPage,
  getSkillsPageHeaderActions,
  type SkillsPageSource,
} from "@/components/skills/SkillsPage";
import UnifiedSkillsPanel, {
  type SkillsCheckUpdatesState,
} from "@/components/skills/UnifiedSkillsPanel";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import { SkillsIcon } from "@/components/BrandIcons";
import { SkillsStorageSheet } from "@/components/skills/SkillsStorageSheet";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";
import {
  useDisableCurrentOmo,
  useDisableCurrentOmoSlim,
} from "@/lib/query/omo";
import { invalidatePiProviderCaches, usePiCurrentState } from "@/lib/query/pi";
import WorkspaceFilesPanel from "@/components/workspace/WorkspaceFilesPanel";
import EnvPanel from "@/components/openclaw/EnvPanel";
import ToolsPanel from "@/components/openclaw/ToolsPanel";
import AgentsDefaultsPanel from "@/components/openclaw/AgentsDefaultsPanel";
import OpenClawHealthBanner from "@/components/openclaw/OpenClawHealthBanner";
import HermesMemoryPanel from "@/components/hermes/HermesMemoryPanel";
import {
  APP_IDS,
  DEFAULT_VISIBLE_APPS,
  isStackAppId,
  isProxyAppId,
} from "@/config/appConfig";

interface SyncStatusUpdatedPayload {
  source?: string;
  status?: string;
  error?: string;
}

type OpenClawConfigTab = "env" | "tools" | "agents";

/** 提示词页的应用下拉：有提示词文件的应用（Claude Desktop 和 Claude Code 共用）。 */
const PROMPT_APP_IDS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "pi",
  "mcode",
];

const getInitialApp = (): AppId => {
  const saved = localStorage.getItem(APP_STORAGE_KEY) as AppId | null;
  if (saved && APP_IDS.includes(saved)) {
    return saved;
  }
  return "claude";
};

function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [activeApp, setActiveApp] = useState<AppId>(getInitialApp);
  const sharedFeatureApp = sharedFeatureAppOf(activeApp);
  const [currentView, setCurrentView] = useState<View>(readStoredView);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("general");
  // 进设置前停留的页面：设置目录里的「← 返回」回到这里
  const settingsReturnViewRef = useRef<View>("providers");
  const [openclawConfigTab, setOpenclawConfigTab] =
    useState<OpenClawConfigTab>("env");
  const [promptsApp, setPromptsApp] = useState<AppId>(() =>
    PROMPT_APP_IDS.includes(sharedFeatureApp) ? sharedFeatureApp : "claude",
  );
  const { collapsed: sidebarCollapsed, toggle: toggleSidebar } =
    useSidebarCollapsed();
  const [skillsDiscoverySource, setSkillsDiscoverySource] =
    useState<SkillsPageSource>("repos");
  const [skillsStorageOpen, setSkillsStorageOpen] = useState(false);
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [mcpManagementBusy, setMcpManagementBusy] = useState(false);
  const [skillsManagementBusy, setSkillsManagementBusy] = useState(false);
  const [skillsNavigationBusy, setSkillsNavigationBusy] = useState(false);
  const [promptManagementBusy, setPromptManagementBusy] = useState(false);
  const [promptNavigationBusy, setPromptNavigationBusy] = useState(false);
  const [skillsCheckUpdatesState, setSkillsCheckUpdatesState] =
    useState<SkillsCheckUpdatesState>({
      isChecking: false,
      hasSkills: false,
    });

  useEffect(() => {
    storeView(currentView);
  }, [currentView]);

  const { data: settingsData } = useSettingsQuery();
  const useAppWindowControls =
    isLinux() && (settingsData?.useAppWindowControls ?? false);
  const visibleApps = useMemo<VisibleApps>(
    () => ({
      ...DEFAULT_VISIBLE_APPS,
      ...settingsData?.visibleApps,
    }),
    [settingsData?.visibleApps],
  );

  const getFirstVisibleApp = (): AppId => {
    return APP_IDS.find((app) => visibleApps[app]) ?? "claude";
  };

  useEffect(() => {
    if (!visibleApps[activeApp]) {
      setActiveApp(getFirstVisibleApp());
    }
  }, [visibleApps, activeApp]);

  // 「启动时检查应用更新」（默认关）：打开了才在后台查一次，查到新版本侧栏「应用」上出现圆点
  const checkToolUpdatesOnStartup =
    settingsData?.checkToolUpdatesOnStartup ?? false;
  const toolUpdatesAvailable = useToolUpdatesAvailable();
  const startupToolCheckDoneRef = useRef(false);
  useEffect(() => {
    if (!checkToolUpdatesOnStartup || startupToolCheckDoneRef.current) return;
    startupToolCheckDoneRef.current = true;
    void checkToolUpdatesInBackground();
  }, [checkToolUpdatesOnStartup]);

  // 应用专属页（工作区、记忆…）只属于它的应用；换了应用就回到供应商页
  useEffect(() => {
    if (isAppPage(currentView) && !appPageBelongsTo(currentView, activeApp)) {
      setCurrentView("providers");
    }
  }, [activeApp, currentView]);

  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    provider: Provider;
    action: "remove" | "delete";
  } | null>(null);
  const [envConflicts, setEnvConflicts] = useState<EnvConflict[]>([]);
  const [showEnvBanner, setShowEnvBanner] = useState(false);

  const effectiveEditingProvider = useLastValidValue(editingProvider);
  const effectiveUsageProvider = useLastValidValue(usageProvider);
  const mainScrollRef = useRef<HTMLElement>(null);
  const providerScrollContainerRef = useRef<HTMLDivElement>(null);

  useUsageCacheBridge();

  useLayoutEffect(() => {
    if (currentView !== "providers") return;

    for (const container of [
      mainScrollRef.current,
      providerScrollContainerRef.current,
    ]) {
      if (container) {
        container.scrollTop = 0;
        container.scrollLeft = 0;
      }
    }
  }, [activeApp, currentView]);

  const promptPanelRef = useRef<PromptPanelHandle>(null);
  const [promptPrimaryAction, setPromptPrimaryAction] =
    useState<PromptPrimaryAction>("prompt");
  const mcpPanelRef = useRef<any>(null);
  const skillsPageRef = useRef<any>(null);
  const unifiedSkillsPanelRef = useRef<any>(null);
  // 订阅未管理 Skill 的共享缓存（实际扫描由 UnifiedSkillsPanel 进入页面时触发）。
  // 这里 enabled 默认 false，仅用于「导入」按钮的绿点提示，不主动发起扫描。
  const { data: unmanagedSkills } = useScanUnmanagedSkills();
  const hasUnmanagedSkills = (unmanagedSkills?.length ?? 0) > 0;

  const {
    isRunning: isProxyRunning,
    takeoverStatus,
    status: proxyStatus,
  } = useProxyStatus();
  const proxyAppId = isProxyAppId(activeApp) ? activeApp : null;
  const currentAppUsesProxy =
    proxyAppId !== null || activeApp === "claude-desktop";
  const isCurrentAppTakeoverActive = proxyAppId
    ? takeoverStatus?.[proxyAppId] || false
    : false;
  const activeProviderId = useMemo(() => {
    if (!proxyAppId) return undefined;
    const target = proxyStatus?.active_targets?.find(
      (t) => t.app_type === proxyAppId,
    );
    return target?.provider_id;
  }, [proxyStatus?.active_targets, proxyAppId]);

  const { data, isLoading, refetch } = useProvidersQuery(activeApp, {
    isProxyRunning: currentAppUsesProxy && isProxyRunning,
  });
  const { data: piCurrentState } = usePiCurrentState(activeApp === "pi");
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";
  const isOpenClawView = activeApp === "openclaw" && isAppPage(currentView);
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
    isProxyRunning && isCurrentAppTakeoverActive,
  );
  const handleEnablePiProvider = async (provider: Provider) => {
    try {
      await providersApi.switch(provider.id, "pi");
      await invalidatePiProviderCaches(queryClient);
      await providersApi.updateTrayMenu().catch((error) => {
        console.error(
          "Failed to update tray menu after enabling Pi provider",
          error,
        );
      });
      toast.success(
        t("pi.provider.enabled", {
          defaultValue: "已在 Pi 中启用",
        }),
        { closeButton: true },
      );
    } catch (error) {
      const detail = extractErrorMessage(error);
      toast.error(
        t("pi.provider.enableFailed", {
          defaultValue: "无法在 Pi 中启用此供应商",
        }),
        {
          description:
            translatePiProviderMutationError(detail, t) || detail || undefined,
          closeButton: true,
        },
      );
    }
  };

  const disableOmoMutation = useDisableCurrentOmo();
  const handleDisableOmo = () => {
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
  };

  const disableOmoSlimMutation = useDisableCurrentOmoSlim();
  const handleDisableOmoSlim = () => {
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
  };

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;

    const setupListener = async () => {
      try {
        const off = await providersApi.onSwitched(
          async (event: ProviderSwitchEvent) => {
            if (event.appType === activeApp) {
              await refetch();
            }
            if (event.appType === "pi") {
              await invalidatePiProviderCaches(queryClient);
            }
          },
        );
        if (!active) {
          off();
          return;
        }
        unsubscribe = off;
      } catch (error) {
        console.error("[App] Failed to subscribe provider switch event", error);
      }
    };

    void setupListener();
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [activeApp, queryClient, refetch]);

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
    await queryClient.invalidateQueries({
      queryKey: proxyKeys.takeoverStatus,
    });
    await queryClient.invalidateQueries({ queryKey: proxyKeys.status });
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
      try {
        if (activeApp === "mcode") return;
        const conflicts = await checkEnvConflicts(activeApp);

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
  }, [activeApp]);

  const currentViewRef = useRef(currentView);
  const managementBusy =
    mcpManagementBusy || skillsNavigationBusy || promptNavigationBusy;
  const managementBusyRef = useRef(false);
  managementBusyRef.current = managementBusy;

  useEffect(() => {
    currentViewRef.current = currentView;
  }, [currentView]);

  // ─── 导航 ───────────────────────────────────────────────────────────────
  // MCP / Skills / 提示词在进行中的操作会锁住导航（卸载面板会丢掉进行中的状态）

  const openSettings = (section: SettingsSection = "general") => {
    if (managementBusyRef.current) return;
    if (currentViewRef.current !== "settings") {
      settingsReturnViewRef.current = currentViewRef.current;
    }
    setSettingsSection(section);
    setCurrentView("settings");
  };

  const exitSettings = () => {
    setCurrentView(settingsReturnViewRef.current);
  };

  const selectApp = (app: AppId) => {
    if (managementBusyRef.current) return;
    setActiveApp(app);
    localStorage.setItem(APP_STORAGE_KEY, app);
    setCurrentView("providers");
  };

  const openPage = (page: GlobalPage | "settings") => {
    if (page === "settings") {
      openSettings("general");
      return;
    }
    if (managementBusyRef.current) return;
    if (page === "prompts" && currentViewRef.current !== "prompts") {
      // 提示词页默认选中侧栏里最后选的那个应用
      setPromptsApp(
        PROMPT_APP_IDS.includes(sharedFeatureApp) ? sharedFeatureApp : "claude",
      );
    }
    setCurrentView(page);
  };

  const openSettingsRef = useRef(openSettings);
  openSettingsRef.current = openSettings;
  const toggleSidebarRef = useRef(toggleSidebar);
  toggleSidebarRef.current = toggleSidebar;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (mod && event.key === ",") {
        event.preventDefault();
        openSettingsRef.current("general");
        return;
      }

      if (mod && event.key === "\\") {
        event.preventDefault();
        toggleSidebarRef.current();
        return;
      }

      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (document.body.style.overflow === "hidden") return;
      if (managementBusyRef.current) return;
      if (isTextEditableTarget(event.target)) return;

      const view = currentViewRef.current;
      if (view === "skillsDiscovery") {
        event.preventDefault();
        setCurrentView("skills");
      } else if (view === "settings") {
        event.preventDefault();
        setCurrentView(settingsReturnViewRef.current);
      }
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

  const handleEditProvider = async ({
    provider,
    originalId,
    editorSave,
  }: {
    provider: Provider;
    originalId?: string;
    editorSave?: ProviderEditorSave;
  }) => {
    await updateProvider(provider, originalId, editorSave);
    setEditingProvider(null);
  };

  const handleConfirmAction = async () => {
    if (!confirmAction) return;
    const { provider, action } = confirmAction;

    if (action === "remove") {
      // Remove from live config only (for additive mode apps like OpenCode/OpenClaw)
      // Does NOT delete from database - provider remains in the list
      try {
        await providersApi.removeFromLiveConfig(provider.id, activeApp);
      } catch (error) {
        const detail = extractErrorMessage(error);
        const description =
          activeApp === "pi"
            ? translatePiProviderMutationError(detail, t) || detail
            : detail;
        if (activeApp === "pi") {
          void invalidatePiProviderCaches(queryClient).catch(() => undefined);
        }
        toast.error(t("notifications.removeFromConfigFailed"), {
          description: description || t("common.unknown"),
          closeButton: true,
        });
        return;
      }
      if (activeApp === "pi") {
        await invalidatePiProviderCaches(queryClient);
      }
      // Invalidate queries to refresh the isInConfig state
      if (activeApp === "opencode") {
        await queryClient.invalidateQueries({
          queryKey: ["opencodeLiveProviderIds"],
        });
      } else if (activeApp === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.liveProviderIds,
        });
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
      } else if (activeApp === "hermes") {
        await queryClient.invalidateQueries({
          queryKey: hermesKeys.liveProviderIds,
        });
      } else if (activeApp === "mcode") {
        await queryClient.invalidateQueries({
          queryKey: ["providers", "mcode"],
        });
      }
      toast.success(
        activeApp === "pi"
          ? t("pi.provider.removed", {
              defaultValue: "已从 Pi 移除",
            })
          : t("notifications.removeFromConfigSuccess", {
              defaultValue: "已从配置移除",
            }),
        { closeButton: true },
      );
    } else {
      await deleteProvider(provider.id);
    }
    setConfirmAction(null);
  };

  const generateUniqueProviderCopyKey = (
    originalKey: string,
    existingKeys: string[],
  ): string => {
    const baseKey = `${originalKey}-copy`;

    if (!existingKeys.includes(baseKey)) {
      return baseKey;
    }

    let counter = 2;
    while (existingKeys.includes(`${baseKey}-${counter}`)) {
      counter++;
    }
    return `${baseKey}-${counter}`;
  };

  const handleDuplicateProvider = async (provider: Provider) => {
    if (
      activeApp === "opencode" &&
      provider.category !== "omo" &&
      provider.category !== "omo-slim"
    ) {
      const { npm, models } = provider.settingsConfig;
      if (
        typeof npm !== "string" ||
        !npm.trim() ||
        !models ||
        typeof models !== "object" ||
        Array.isArray(models) ||
        Object.keys(models).length === 0
      ) {
        toast.error(t("opencode.duplicateRequiresDefinition"));
        return;
      }
    }

    const newSortIndex =
      provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined;

    const duplicatedProvider: Omit<Provider, "id" | "createdAt"> & {
      providerKey?: string;
      addToLive?: boolean;
    } = {
      name: `${provider.name} copy`,
      settingsConfig: deepClone(provider.settingsConfig),
      websiteUrl: provider.websiteUrl,
      category: provider.category,
      sortIndex: newSortIndex, // 复制原 sortIndex + 1
      meta: provider.meta ? deepClone(provider.meta) : undefined,
      icon: provider.icon,
      iconColor: provider.iconColor,
    };

    if (
      activeApp === "opencode" ||
      activeApp === "openclaw" ||
      activeApp === "hermes" ||
      activeApp === "pi"
    ) {
      let liveProviderIds: string[] = [];
      try {
        liveProviderIds =
          activeApp === "opencode"
            ? await queryClient.ensureQueryData({
                queryKey: ["opencodeLiveProviderIds"],
                queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
              })
            : activeApp === "openclaw"
              ? await queryClient.ensureQueryData({
                  queryKey: openclawKeys.liveProviderIds,
                  queryFn: () => providersApi.getOpenClawLiveProviderIds(),
                })
              : activeApp === "hermes"
                ? await queryClient.ensureQueryData({
                    queryKey: hermesKeys.liveProviderIds,
                    queryFn: () => providersApi.getHermesLiveProviderIds(),
                  })
                : (
                    await queryClient.ensureQueryData({
                      queryKey: ["pi", "currentState"],
                      queryFn: () => piApi.getCurrentState(),
                    })
                  ).enabledProviderIds;
      } catch (error) {
        console.error(
          "[App] Failed to load live provider IDs for duplication",
          error,
        );
        const errorMessage = extractErrorMessage(error);
        toast.error(
          t("provider.duplicateLiveIdsLoadFailed", {
            defaultValue: "读取配置中的供应商标识失败，请先修复配置后再试",
          }) + (errorMessage ? `: ${errorMessage}` : ""),
        );
        return;
      }
      const existingKeys = Array.from(
        new Set([...Object.keys(providers), ...liveProviderIds]),
      );
      duplicatedProvider.providerKey = generateUniqueProviderCopyKey(
        provider.id,
        existingKeys,
      );
      duplicatedProvider.addToLive = false;
    } else if (activeApp === "mcode") {
      // The MCode list already includes its live custom nodes; the backend
      // rejects a key that MCode itself owns.
      duplicatedProvider.providerKey = generateUniqueProviderCopyKey(
        provider.id,
        Object.keys(providers),
      );
    }

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

    await addProvider(duplicatedProvider);
  };

  const confirmActionMessage = useMemo(() => {
    if (!confirmAction) return "";

    const message =
      confirmAction.action === "remove"
        ? t("confirm.removeProviderMessage", {
            name: confirmAction.provider.name,
          })
        : t("confirm.deleteProviderMessage", {
            name: confirmAction.provider.name,
          });
    const isPiGlobalDefault =
      activeApp === "pi" &&
      piCurrentState?.defaultProviderId === confirmAction.provider.id;

    return isPiGlobalDefault
      ? `${message}\n\n${t("confirm.piDefaultProviderWarning")}`
      : message;
  }, [activeApp, confirmAction, piCurrentState?.defaultProviderId, t]);

  const handleOpenTerminal = async (provider: Provider) => {
    try {
      const selectedDir = await settingsApi.pickDirectory();
      if (!selectedDir) {
        return;
      }

      await providersApi.openTerminal(provider.id, activeApp, {
        cwd: selectedDir,
      });
      toast.success(
        t("provider.terminalOpened", {
          defaultValue: "终端已打开",
        }),
      );
    } catch (error) {
      console.error("[App] Failed to open terminal", error);
      const errorMessage = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed", {
          defaultValue: "打开终端失败",
        }) + (errorMessage ? `: ${errorMessage}` : ""),
      );
    }
  };

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
      await refetch();
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  };

  const handleOpenSkillsDiscovery = () => {
    setSkillsDiscoverySource("repos");
    setCurrentView("skillsDiscovery");
  };

  const handleHideActiveApp = async () => {
    const visibleCount = Object.values(visibleApps).filter(Boolean).length;
    if (visibleCount <= 1) return;
    try {
      const current = await settingsApi.get();
      await settingsApi.save({
        ...current,
        visibleApps: { ...visibleApps, [activeApp]: false },
      });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      toast.error(
        t("settings.saveFailedGeneric", { defaultValue: "保存失败，请重试" }),
        { description: extractErrorMessage(error) || undefined },
      );
    }
  };

  // ─── 应用页 ─────────────────────────────────────────────────────────────

  // 旧的路由开关（阶段 3 换成模式 tab），暂时放在页头
  const routeToggles =
    activeApp === "claude-desktop" ? (
      <ClaudeDesktopRouteToggle />
    ) : proxyAppId ? (
      // 设置里选了 Stack 模式：Claude Code、Codex 的开关换成 Stack 模式开关（不做
      // 故障转移），其余应用仍显示路由开关。
      settingsData?.enableStackMode && isStackAppId(proxyAppId) ? (
        <ProxyToggle activeApp={proxyAppId} stack />
      ) : (
        <>
          {(settingsData?.enableLocalProxy ||
            settingsData?.enableStackMode) && (
            <ProxyToggle activeApp={proxyAppId} />
          )}
          {settingsData?.enableFailoverToggle && (
            <FailoverToggle activeApp={proxyAppId} />
          )}
        </>
      )
    ) : null;

  const appMenu = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="quiet"
          size="icon-compact"
          className="h-8 w-8"
          aria-label={t("appPage.moreActions", {
            name: APP_DISPLAY_NAME[activeApp],
          })}
          title={t("common.more")}
        >
          <MoreHorizontal className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[180px]">
        <DropdownMenuItem onSelect={() => openPage("sessions")}>
          {t("appPage.viewSessions")}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => openSettings("appConfig")}>
          {t("appPage.configDirectory")}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => openPage("apps")}>
          {t("appPage.installAndUpgrade")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          disabled={Object.values(visibleApps).filter(Boolean).length <= 1}
          onSelect={() => void handleHideActiveApp()}
        >
          {t("appPage.hideFromSidebar")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  const appPageName =
    currentView === "workspace"
      ? t("appPage.workspace")
      : currentView === "openclawConfig"
        ? t("appPage.openclawConfig")
        : currentView === "hermesMemory"
          ? t("appPage.memory")
          : t("appPage.providers");

  const renderAppPageHeader = () => (
    <AppPageHeader
      variant="app"
      icon={<AppGlyph app={activeApp} size={20} badgeClassName="bg-app" />}
      title={APP_DISPLAY_NAME[activeApp]}
      subtitle={
        activeApp === "openclaw" || activeApp === "hermes"
          ? undefined
          : appPageName
      }
      actions={
        <>
          {currentView === "providers" && routeToggles}
          {currentView === "providers" &&
            activeApp !== "mcode" &&
            (settingsData?.showProfileSwitcher ?? true) && (
              <ProfileSwitcher activeApp={activeApp} />
            )}
          {activeApp === "hermes" && (
            <Button
              variant="quiet"
              size="regular"
              onClick={() => void openHermesWebUI()}
            >
              {t("appPage.webUi")}
              <ExternalLink className="h-3.5 w-3.5" />
            </Button>
          )}
          {currentView === "providers" && (
            <Button
              variant="solid"
              size="regular"
              onClick={() => setIsAddOpen(true)}
            >
              <Plus className="h-4 w-4" />
              {t("provider.addProvider")}
            </Button>
          )}
          {appMenu}
        </>
      }
    />
  );

  // OpenClaw / Hermes 没有模式 tab，那一行放它们自己的分段控件
  const renderAppSegments = () => {
    if (activeApp === "openclaw") {
      return (
        <div className="shrink-0 px-6 pt-3">
          <SegmentedControl
            aria-label={APP_DISPLAY_NAME.openclaw}
            value={currentView}
            onValueChange={(view) => setCurrentView(view)}
            items={[
              { value: "providers", label: t("appPage.providers") },
              { value: "workspace", label: t("appPage.workspace") },
              { value: "openclawConfig", label: t("appPage.openclawConfig") },
            ]}
          />
        </div>
      );
    }
    if (activeApp === "hermes") {
      return (
        <div className="shrink-0 px-6 pt-3">
          <SegmentedControl
            aria-label={APP_DISPLAY_NAME.hermes}
            value={currentView}
            onValueChange={(view) => setCurrentView(view)}
            items={[
              { value: "providers", label: t("appPage.providers") },
              { value: "hermesMemory", label: t("appPage.memory") },
            ]}
          />
        </div>
      );
    }
    return null;
  };

  const renderProviderList = () => (
    <div
      ref={providerScrollContainerRef}
      id="main-content"
      className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-6 pb-12 pt-4"
    >
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
            isProxyRunning={currentAppUsesProxy && isProxyRunning}
            isProxyTakeover={isProxyRunning && isCurrentAppTakeoverActive}
            activeProviderId={activeProviderId}
            onSwitch={
              activeApp === "pi" ? handleEnablePiProvider : switchProvider
            }
            onEdit={(provider) => {
              setEditingProvider(provider);
            }}
            onDelete={(provider) =>
              setConfirmAction({ provider, action: "delete" })
            }
            onRemoveFromConfig={
              activeApp === "opencode" ||
              activeApp === "openclaw" ||
              activeApp === "hermes" ||
              activeApp === "pi" ||
              activeApp === "mcode"
                ? (provider) => setConfirmAction({ provider, action: "remove" })
                : undefined
            }
            onDisableOmo={
              activeApp === "opencode" ? handleDisableOmo : undefined
            }
            onDisableOmoSlim={
              activeApp === "opencode" ? handleDisableOmoSlim : undefined
            }
            onDuplicate={handleDuplicateProvider}
            onConfigureUsage={setUsageProvider}
            onOpenWebsite={handleOpenWebsite}
            onOpenTerminal={
              activeApp === "claude" ? handleOpenTerminal : undefined
            }
            onCreate={() => setIsAddOpen(true)}
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
  );

  const renderAppPage = () => {
    const body = (() => {
      switch (currentView) {
        case "workspace":
          return <WorkspaceFilesPanel />;
        case "openclawConfig":
          return (
            <>
              <div className="shrink-0 px-6 pt-3">
                <SegmentedControl
                  size="sm"
                  aria-label={t("appPage.openclawConfig")}
                  value={openclawConfigTab}
                  onValueChange={setOpenclawConfigTab}
                  items={[
                    { value: "env", label: t("openclaw.env.title") },
                    { value: "tools", label: t("openclaw.tools.title") },
                    { value: "agents", label: t("openclaw.agents.title") },
                  ]}
                />
              </div>
              <div id="main-content" className="min-h-0 flex-1 overflow-y-auto">
                {openclawConfigTab === "env" ? (
                  <EnvPanel />
                ) : openclawConfigTab === "tools" ? (
                  <ToolsPanel />
                ) : (
                  <AgentsDefaultsPanel />
                )}
              </div>
            </>
          );
        case "hermesMemory":
          return (
            <div id="main-content" className="min-h-0 flex-1 overflow-y-auto">
              <HermesMemoryPanel />
            </div>
          );
        default:
          return renderProviderList();
      }
    })();

    return (
      <>
        {renderAppPageHeader()}
        {renderAppSegments()}
        {isOpenClawView && openclawHealthWarnings.length > 0 && (
          <OpenClawHealthBanner warnings={openclawHealthWarnings} />
        )}
        {body}
      </>
    );
  };

  // ─── 全局页 ─────────────────────────────────────────────────────────────

  const quietAction = (
    key: string,
    label: string,
    icon: React.ReactNode,
    onClick: () => void,
    options?: { disabled?: boolean; title?: string; dot?: boolean },
  ) => (
    <Button
      key={key}
      variant="quiet"
      size="regular"
      disabled={options?.disabled}
      onClick={onClick}
      title={options?.title}
      className="relative"
    >
      {icon}
      {label}
      {options?.dot && (
        <span
          className="absolute end-1 top-1 h-2 w-2 rounded-full bg-success"
          aria-hidden="true"
        />
      )}
    </Button>
  );

  const renderGlobalPage = () => {
    switch (currentView) {
      case "usage":
        return <UsagePage />;
      case "auth":
        return (
          <>
            <AppPageHeader
              icon={<KeyRound className="h-5 w-5" strokeWidth={1.5} />}
              title={t("nav.auth")}
              titleExtra={
                <>
                  <HelpTip title={t("nav.auth")}>
                    {t("settings.authCenter.description")}
                  </HelpTip>
                  <Badge
                    variant="outline"
                    className="ms-1 h-5 px-1.5 text-badge font-medium"
                  >
                    {t("settings.authCenter.beta", { defaultValue: "Beta" })}
                  </Badge>
                </>
              }
            />
            <div id="main-content" className="min-h-0 flex-1 overflow-y-auto">
              <div className="px-6 pb-10 pt-4">
                <AuthCenterPanel showIntro={false} />
              </div>
            </div>
          </>
        );
      case "mcp":
        return (
          <>
            <AppPageHeader
              icon={<Server className="h-5 w-5" strokeWidth={1.5} />}
              title="MCP"
              actions={
                <>
                  {quietAction(
                    "import",
                    t("mcp.importExisting"),
                    <Download className="h-4 w-4" />,
                    () => mcpPanelRef.current?.openImport(),
                    { disabled: mcpManagementBusy },
                  )}
                  <Button
                    variant="solid"
                    size="regular"
                    disabled={mcpManagementBusy}
                    onClick={() => mcpPanelRef.current?.openAdd()}
                  >
                    <Plus className="h-4 w-4" />
                    {t("mcp.addMcp")}
                  </Button>
                </>
              }
            />
            <div id="main-content" className="flex min-h-0 flex-1 flex-col">
              <UnifiedMcpPanel
                ref={mcpPanelRef}
                onOpenChange={() => setCurrentView("providers")}
                onInteractionBlockedChange={setMcpManagementBusy}
              />
            </div>
          </>
        );
      case "skills":
        return (
          <>
            <AppPageHeader
              icon={<SkillsIcon size={20} />}
              title="Skills"
              actions={
                <>
                  {quietAction(
                    "check",
                    skillsCheckUpdatesState.isChecking
                      ? t("skills.checkingUpdates")
                      : t("skills.checkUpdates"),
                    skillsCheckUpdatesState.isChecking ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <RefreshCw className="h-4 w-4" />
                    ),
                    () => unifiedSkillsPanelRef.current?.checkUpdates(),
                    {
                      disabled:
                        skillsManagementBusy ||
                        skillsCheckUpdatesState.isChecking ||
                        !skillsCheckUpdatesState.hasSkills,
                    },
                  )}
                  {quietAction(
                    "restore",
                    t("skills.restoreFromBackup.button"),
                    <History className="h-4 w-4" />,
                    () =>
                      unifiedSkillsPanelRef.current?.openRestoreFromBackup(),
                    { disabled: skillsManagementBusy },
                  )}
                  {quietAction(
                    "zip",
                    t("skills.installFromZip.button"),
                    <FolderArchive className="h-4 w-4" />,
                    () => unifiedSkillsPanelRef.current?.openInstallFromZip(),
                    { disabled: skillsManagementBusy },
                  )}
                  {quietAction(
                    "import",
                    t("skills.import"),
                    <Download className="h-4 w-4" />,
                    () => unifiedSkillsPanelRef.current?.openImport(),
                    {
                      disabled: skillsManagementBusy,
                      title: hasUnmanagedSkills
                        ? t("skills.unmanagedAvailable")
                        : undefined,
                      dot: hasUnmanagedSkills,
                    },
                  )}
                  <Button
                    variant="solid"
                    size="regular"
                    disabled={skillsManagementBusy}
                    onClick={() =>
                      unifiedSkillsPanelRef.current?.openDiscovery()
                    }
                  >
                    <Search className="h-4 w-4" />
                    {t("skills.discover")}
                  </Button>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="quiet"
                        size="icon-compact"
                        className="h-8 w-8"
                        aria-label={t("skills.moreActions")}
                        title={t("common.more")}
                      >
                        <MoreHorizontal className="h-4 w-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onSelect={() => setSkillsStorageOpen(true)}
                      >
                        {t("skills.storageSheet.open")}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </>
              }
            />
            <div id="main-content" className="flex min-h-0 flex-1 flex-col">
              <UnifiedSkillsPanel
                ref={unifiedSkillsPanelRef}
                onOpenDiscovery={handleOpenSkillsDiscovery}
                onInteractionBlockedChange={setSkillsManagementBusy}
                onNavigationBlockedChange={setSkillsNavigationBusy}
                onCheckUpdatesStateChange={setSkillsCheckUpdatesState}
                currentApp={
                  sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp
                }
              />
            </div>
          </>
        );
      case "skillsDiscovery":
        return (
          <>
            <AppPageHeader
              icon={<SkillsIcon size={20} />}
              title="Skills"
              subtitle={t("skills.discover")}
              actions={
                <>
                  {getSkillsPageHeaderActions(skillsDiscoverySource).map(
                    ({ key, labelKey, Icon, execute }) =>
                      quietAction(
                        key,
                        t(labelKey),
                        <Icon className="h-4 w-4" />,
                        () => execute(skillsPageRef.current),
                      ),
                  )}
                  <Button
                    variant="neutral"
                    size="regular"
                    onClick={() => setCurrentView("skills")}
                  >
                    {t("skills.backToInstalled")}
                  </Button>
                </>
              }
            />
            <div id="main-content" className="flex min-h-0 flex-1 flex-col">
              <SkillsPage
                ref={skillsPageRef}
                initialApp={
                  sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp
                }
                onSourceChange={setSkillsDiscoverySource}
              />
            </div>
          </>
        );
      case "prompts":
        return (
          <>
            <AppPageHeader
              icon={<BookOpen className="h-5 w-5" strokeWidth={1.5} />}
              title={t("nav.prompts")}
              actions={
                promptPrimaryAction ? (
                  <Button
                    variant="solid"
                    size="regular"
                    disabled={promptManagementBusy}
                    onClick={() => promptPanelRef.current?.openAdd()}
                  >
                    <Plus className="h-4 w-4" />
                    {t(
                      promptPrimaryAction === "template"
                        ? "pi.prompts.newTemplate"
                        : "prompts.add",
                    )}
                  </Button>
                ) : null
              }
            />
            <div className="shrink-0 px-6 pt-4">
              <Select
                value={promptsApp}
                disabled={promptNavigationBusy}
                onValueChange={(value) => setPromptsApp(value as AppId)}
              >
                <SelectTrigger
                  className="h-9 w-[200px]"
                  aria-label={t("prompts.appSelect")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PROMPT_APP_IDS.filter(
                    (app) =>
                      visibleApps[app] ||
                      (app === "claude" && visibleApps["claude-desktop"]),
                  ).map((app) => (
                    <SelectItem key={app} value={app}>
                      <span className="flex items-center gap-2">
                        <AppGlyph app={app} size={16} />
                        {APP_DISPLAY_NAME[app]}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div id="main-content" className="flex min-h-0 flex-1 flex-col">
              <PromptPanel
                key={promptsApp}
                ref={promptPanelRef}
                open={true}
                onOpenChange={() => setCurrentView("providers")}
                appId={promptsApp}
                onInteractionBlockedChange={setPromptManagementBusy}
                onNavigationBlockedChange={setPromptNavigationBusy}
                onPrimaryActionChange={setPromptPrimaryAction}
              />
            </div>
          </>
        );
      case "sessions":
        return (
          <>
            <AppPageHeader
              icon={<History className="h-5 w-5" strokeWidth={1.5} />}
              title={t("nav.sessions")}
            />
            <div id="main-content" className="flex min-h-0 flex-1 flex-col">
              <SessionManagerPage
                key={sharedFeatureApp}
                appId={sharedFeatureApp}
              />
            </div>
          </>
        );
      case "apps":
        return <AppsPage />;
      default:
        return null;
    }
  };

  const renderContent = () => {
    if (currentView === "settings") {
      return (
        <SettingsPage
          section={settingsSection}
          onImportSuccess={handleImportSuccess}
          onOpenApps={() => setCurrentView("apps")}
        />
      );
    }
    return isAppPage(currentView) ? renderAppPage() : renderGlobalPage();
  };

  return (
    <WindowControlsContext.Provider
      value={useAppWindowControls ? <WindowControls /> : null}
    >
      <div className="flex h-screen overflow-hidden bg-app text-fg-1 selection:bg-primary/30">
        <Sidebar
          collapsed={sidebarCollapsed}
          onToggleCollapsed={toggleSidebar}
          activeApp={activeApp}
          view={currentView}
          visibleApps={visibleApps}
          settingsSection={settingsSection}
          onSelectApp={selectApp}
          onSelectPage={openPage}
          onSelectSettingsSection={setSettingsSection}
          onExitSettings={exitSettings}
          appsUpdateAvailable={
            checkToolUpdatesOnStartup && toolUpdatesAvailable
          }
        />
        <main
          ref={mainScrollRef}
          className="flex min-w-0 flex-1 flex-col overflow-hidden"
        >
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
          <div
            key={
              currentView === "settings"
                ? "settings"
                : isAppPage(currentView)
                  ? "app"
                  : currentView
            }
            className="flex min-h-0 flex-1 flex-col"
          >
            {renderContent()}
          </div>
        </main>
      </div>

      <AddProviderDialog
        open={isAddOpen}
        onOpenChange={setIsAddOpen}
        appId={activeApp}
        onSubmit={addProvider}
      />

      <EditProviderDialog
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

      {effectiveUsageProvider && (
        <UsageScriptModal
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
      )}

      <ConfirmDialog
        isOpen={Boolean(confirmAction)}
        title={
          confirmAction?.action === "remove"
            ? t("confirm.removeProvider")
            : t("confirm.deleteProvider")
        }
        message={confirmActionMessage}
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

      <SkillsStorageSheet
        open={skillsStorageOpen}
        onOpenChange={setSkillsStorageOpen}
      />

      <DeepLinkImportDialog />
      <FirstRunNoticeDialog />
    </WindowControlsContext.Provider>
  );
}

export default App;
