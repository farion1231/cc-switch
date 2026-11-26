import {
  createContext,
  useContext,
  useId,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DraggableAttributes,
  type DraggableSyntheticListeners,
  type DragEndEvent,
  type Modifier,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  AlertTriangle,
  ArrowUpCircle,
  Download,
  GripVertical,
  Info,
  LayoutGrid,
  ListOrdered,
  Loader2,
  MoreHorizontal,
  RefreshCw,
  Star,
  Trash2,
} from "lucide-react";
import { settingsApi, type AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import type {
  InstallMethod,
  ToolInstallation,
  ToolInstallationReport,
} from "@/lib/api/settings";
import type { VisibleApps } from "@/types";
import { DEFAULT_VISIBLE_APPS, orderAppIds } from "@/config/appConfig";
import { useSettings } from "@/hooks/useSettings";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isUpdateAvailable } from "@/lib/version";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { ToolUpgradeConfirmDialog } from "@/components/settings/ToolUpgradeConfirmDialog";
import {
  getToolInstallSourceLabel,
  ToolInstallRow,
} from "@/components/settings/ToolInstallRow";
import { ToolErrorMessage } from "@/components/settings/ToolErrorMessage";
import { ToolUninstallConfirmDialog } from "./ToolUninstallConfirmDialog";
import type { InstallApp } from "./installCommands";
import "./appsPage.css";
import { InstallCommandsDialog } from "./InstallCommandsDialog";
import { ManualUninstallDialog } from "./ManualUninstallDialog";
import {
  ENV_BADGE_CONFIG,
  TOOL_APP_IDS,
  TOOL_NAMES,
  toolDisplayName,
  availableInstallationUpdate,
  useToolManagement,
  isDesktopManagedTool,
  WSL_SHELL_FLAG_OPTIONS,
  WSL_SHELL_OPTIONS,
  type ToolLifecycleAction,
  type ToolName,
  type ToolVersion,
} from "./useToolManagement";

// WorkBuddy 是桌面应用、自己负责更新，这里和 Claude Desktop 一样只管侧栏显示
type Row =
  | { kind: "tool"; tool: ToolName }
  | { kind: "desktop" }
  | { kind: "workbuddy" };

const ROW_BY_APP = Object.fromEntries([
  ["claude-desktop", { kind: "desktop" }],
  ["workbuddy", { kind: "workbuddy" }],
  ...TOOL_NAMES.map((tool) => [TOOL_APP_IDS[tool], { kind: "tool", tool }]),
]) as Record<AppId, Row>;

const alphabeticalAppOrder = (Object.keys(ROW_BY_APP) as AppId[]).sort(
  (left, right) =>
    APP_DISPLAY_NAME[left].localeCompare(APP_DISPLAY_NAME[right], "en"),
);

type RowSection = { key: string; title: string; rows: Row[] };

const rowKey = (row: Row): AppId =>
  row.kind === "desktop"
    ? "claude-desktop"
    : row.kind === "workbuddy"
      ? "workbuddy"
      : TOOL_APP_IDS[row.tool];

// 只上下拖，且停在本组的列表框里：列表框裁掉超出的部分，拖出去整行会看不见
const verticalDragModifiers: Modifier[] = [
  ({ transform, draggingNodeRect, containerNodeRect }) => {
    if (!draggingNodeRect || !containerNodeRect) return { ...transform, x: 0 };
    const minY = containerNodeRect.top - draggingNodeRect.top;
    const maxY = containerNodeRect.bottom - draggingNodeRect.bottom;
    return {
      ...transform,
      x: 0,
      y: Math.min(Math.max(transform.y, minY), maxY),
    };
  },
];

type DragHandle = {
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  isDragging: boolean;
};

/** 可拖动的行把拖动把手交给里面的 `AppRow`（ToolRow 自己渲染 AppRow，不必逐层传参）。 */
const DragHandleContext = createContext<DragHandle | null>(null);

/** 本机有这个命令行应用：探到了版本、装了但跑不起来，或安装清单里有安装位置（和「卸载」出现的条件一致）。 */
function isToolPresent(
  info: ToolVersion | undefined,
  report: ToolInstallationReport | undefined,
): boolean {
  return Boolean(
    info?.version ||
      info?.installed_but_broken ||
      info?.desktop_installation ||
      report?.installs.length,
  );
}

// 表头和应用行共用列宽；操作列宽取各行按钮和表头量宽按钮的最大值，最后一列是行尾的「⋯」菜单。
const APP_ROW_GRID =
  "grid grid-cols-[minmax(0,1fr)_minmax(0,clamp(116px,21vw,208px))_var(--apps-actions-width,0px)_var(--apps-sidebar-width,56px)_28px] items-center gap-x-2.5 lg:gap-x-3";

function formatCheckedAt(timestamp: number, locale: string): string {
  const date = new Date(timestamp);
  const now = new Date();
  const time = date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return date.toDateString() === now.toDateString()
    ? time
    : `${date.toLocaleDateString(locale, { month: "numeric", day: "numeric" })} ${time}`;
}

/**
 * 「应用」页（v7）：每个应用一行 = 安装状态 / 版本 + 安装 / 升级 + 在侧栏显示的开关 + 「⋯」菜单（卸载）。
 * 隐藏一个应用只影响界面（侧栏、MCP / Skills 的列、提示词和会话的下拉），配置照常同步。
 */
export function AppsPage() {
  const { t, i18n } = useTranslation();
  const [expanded, setExpanded] = useState<Partial<Record<ToolName, boolean>>>(
    {},
  );
  const tools = useToolManagement(expanded);
  const [desktopInstalling, setDesktopInstalling] = useState(false);
  const { settings, updateSettings, autoSaveSettings } = useSettings();
  const tableRef = useRef<HTMLDivElement>(null);
  const {
    data: desktopStatus,
    isPending: isDesktopStatusPending,
    refetch: refreshClaudeDesktop,
  } = useQuery({
    queryKey: ["claude-desktop-status"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    staleTime: 30_000,
  });
  const { data: officialClaude, refetch: refreshOfficialClaude } = useQuery({
    queryKey: ["official-claude-desktop-installation"],
    queryFn: () => settingsApi.getOfficialClaudeDesktopInstallation(),
    staleTime: 30_000,
  });
  const {
    data: officialWorkbuddy,
    isPending: isOfficialWorkbuddyPending,
    refetch: refreshOfficialWorkbuddy,
  } = useQuery({
    queryKey: ["official-workbuddy-installation"],
    queryFn: () => settingsApi.getOfficialWorkbuddyInstallation(),
    staleTime: 30_000,
  });
  const {
    data: workbuddyStatus,
    isPending: isWorkbuddyStatusPending,
    refetch: refreshWorkBuddy,
  } = useQuery({
    queryKey: ["config-status", "workbuddy"],
    queryFn: () => settingsApi.getConfigStatus("workbuddy"),
    staleTime: 30_000,
  });

  const installFromChannel = async (app: InstallApp, method: InstallMethod) => {
    if (app === "workbuddy") return;
    if (app !== "claude-desktop") {
      await tools.handleRunToolAction([app], "install", {
        installMethod: method,
      });
      return;
    }
    setDesktopInstalling(true);
    try {
      await settingsApi.runToolLifecycleAction(
        [app],
        "install",
        undefined,
        undefined,
        undefined,
        method,
      );
      const [{ data }, { data: official }] = await Promise.all([
        refreshClaudeDesktop(),
        refreshOfficialClaude(),
      ]);
      if (!data?.installed && !official) {
        toast.warning(t("settings.toolActionInstalledNotRunnable"));
        return;
      }
      toast.success(
        t("settings.toolActionDone", {
          count: 1,
          action: t("settings.toolInstall"),
        }),
      );
    } catch (error) {
      const detail = extractErrorMessage(error);
      if (detail === "TOOL_ACTION_IN_PROGRESS") {
        toast.info(t("settings.toolActionInProgress"));
      } else {
        toast.error(t("settings.toolActionFailed"), { description: detail });
      }
    } finally {
      setDesktopInstalling(false);
    }
  };

  // 按「已安装 / 未安装」分两组。Claude Desktop 没有命令可探，用后端按数据目录 / 应用包推断的结果。
  // 首次探测还没出结果时不分组，免得各行探完一个跳一个；之后重新检查时沿用上次结果，位置不动。
  const rowInstalled = (row: Row): boolean | null => {
    if (row.kind === "desktop") {
      return officialClaude
        ? true
        : isDesktopStatusPending
          ? null
          : Boolean(desktopStatus?.installed);
    }
    // A signed desktop bundle is installed even before its first configuration is created.
    if (row.kind === "workbuddy") {
      return officialWorkbuddy || workbuddyStatus?.exists
        ? true
        : isWorkbuddyStatusPending || isOfficialWorkbuddyPending
          ? null
          : false;
    }
    const info = tools.toolVersionByName.get(row.tool);
    const report = tools.installReports?.[row.tool];
    // 安装清单（本机定位、不跑命令，比版本探测快得多）先定分组：有安装 → 已安装；
    // 清单为空 → 未安装（清单含登录 shell 解析到的默认入口，和版本探测看到的是同一处），
    // 版本探到「装了但跑不起来」时再归回已安装。跑在 WSL 里的工具本机清单说明不了什么，
    // 照旧等版本探测。
    const overview = tools.overviews?.[row.tool];
    if (overview && !overview.is_wsl)
      return (
        Boolean(overview.desktop_installed) ||
        overview.installs.length > 0 ||
        isToolPresent(info, tools.installReports?.[row.tool])
      );
    if (!info && tools.isLoadingTools) return null;
    return isToolPresent(info, report);
  };
  // 两组共用一份顺序（拖动后写进设置，侧栏也按它排），每组只是按装没装过滤
  const appOrder = orderAppIds(settings?.appOrder, alphabeticalAppOrder);
  const orderedRows = appOrder.map((app) => ROW_BY_APP[app]);
  const rowStates = orderedRows.map((row) => ({
    row,
    installed: rowInstalled(row),
  }));
  const sections: RowSection[] = rowStates.some((s) => s.installed === null)
    ? [{ key: "all", title: t("appsPage.columnApp"), rows: orderedRows }]
    : (
        [
          { key: "installed", installed: true },
          { key: "notInstalled", installed: false },
        ] as const
      )
        .map(({ key, installed }) => {
          const rows = rowStates
            .filter((s) => s.installed === installed)
            .map((s) => s.row);
          return {
            key,
            title: t(
              installed
                ? "appsPage.sectionInstalled"
                : "appsPage.sectionNotInstalled",
              { count: rows.length },
            ),
            rows,
          };
        })
        .filter((section) => section.rows.length > 0);
  // 行换组会重新挂载，操作列的量宽要跟着重新观察新节点
  const sectionLayoutKey = sections
    .map((section) => `${section.key}:${section.rows.map(rowKey).join(",")}`)
    .join("|");

  useLayoutEffect(() => {
    const table = tableRef.current;
    if (!table) return;
    const actions = Array.from(
      table.querySelectorAll<HTMLElement>("[data-tool-actions]"),
    );
    const sidebarHeading = table.querySelector<HTMLElement>(
      "[data-sidebar-heading]",
    );
    const updateColumnWidths = () => {
      const width = Math.max(
        0,
        ...actions.map((action) => action.getBoundingClientRect().width),
      );
      table.style.setProperty("--apps-actions-width", `${Math.ceil(width)}px`);
      if (sidebarHeading) {
        const sidebarWidth = Math.max(
          56,
          Math.ceil(sidebarHeading.getBoundingClientRect().width),
        );
        table.style.setProperty("--apps-sidebar-width", `${sidebarWidth}px`);
      }
    };
    updateColumnWidths();
    // 按钮增减、文案或字体变化时同步整列，避免各行自行收缩后版本列错位。
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateColumnWidths);
    actions.forEach((action) => observer.observe(action));
    if (sidebarHeading) observer.observe(sidebarHeading);
    return () => observer.disconnect();
  }, [sectionLayoutKey]);

  const [installCommandsFor, setInstallCommandsFor] = useState<{
    tool: InstallApp;
    manualUpdate?: boolean;
  } | null>(null);
  const [showUninstallCommands, setShowUninstallCommands] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<{
    tool: ToolName;
    path?: string;
  } | null>(null);
  // 「诊断安装冲突」（和升级后的自动补诊）有结果的行自动展开，否则点了诊断看起来没反应
  const diagnosedTools = Object.keys(tools.toolDiagnostics) as ToolName[];
  useEffect(() => {
    if (diagnosedTools.length === 0) return;
    setExpanded((prev) => {
      const next = { ...prev };
      for (const tool of diagnosedTools) next[tool] = true;
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tools.toolDiagnostics]);

  const visibleApps: VisibleApps = {
    ...DEFAULT_VISIBLE_APPS,
    ...settings?.visibleApps,
  };
  const visibleCount = Object.values(visibleApps).filter(Boolean).length;

  const setVisible = (app: AppId, visible: boolean) => {
    if (!settings) return;
    // 至少留一个应用在侧栏
    if (!visible && visibleCount <= 1) return;
    const next = { ...visibleApps, [app]: visible };
    const previous = settings.visibleApps;
    updateSettings({ visibleApps: next });
    void autoSaveSettings({ visibleApps: next }).catch(() =>
      updateSettings({ visibleApps: previous }),
    );
  };

  const visibilitySwitch = (app: AppId) => (
    <Switch
      size="sm"
      checked={visibleApps[app]}
      disabled={!settings || (visibleApps[app] && visibleCount <= 1)}
      onCheckedChange={(checked) => setVisible(app, checked)}
      aria-label={t("appsPage.showInSidebar", { name: APP_DISPLAY_NAME[app] })}
    />
  );

  // 检查更新时各工具逐个进入加载，可升级列表会先清零再逐个回来；页头沿用检查前的数，
  // 检查完再一次性更新，免得「全部升级」闪没又出现、数字逐个跳。
  const settledUpdatableCount = useRef(0);
  if (!tools.isLoadingTools) {
    settledUpdatableCount.current = tools.updatableCount;
  }
  const updatableCount = tools.isLoadingTools
    ? settledUpdatableCount.current
    : tools.updatableCount;

  const lastChecked = tools.lastCheckedAt
    ? t("appsPage.lastChecked", {
        time: formatCheckedAt(tools.lastCheckedAt, i18n.language),
      })
    : t("appsPage.neverChecked");

  const dragSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  // 在整份顺序里移动：组内两行之间夹着的别组应用相对次序不变
  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    if (!settings || !over || active.id === over.id) return;
    const oldIndex = appOrder.indexOf(active.id as AppId);
    const newIndex = appOrder.indexOf(over.id as AppId);
    if (oldIndex === -1 || newIndex === -1) return;
    saveAppOrder(arrayMove(appOrder, oldIndex, newIndex));
  };

  const saveAppOrder = (next: AppId[]) => {
    if (!settings) return;
    const previous = settings.appOrder;
    updateSettings({ appOrder: next });
    void autoSaveSettings({ appOrder: next }).catch(() =>
      updateSettings({ appOrder: previous }),
    );
  };

  const desktopInstallButton = (app: "workbuddy" | "claude-desktop") => (
    <Button
      variant="neutral"
      size="compact"
      disabled={app === "claude-desktop" && desktopInstalling}
      onClick={() => setInstallCommandsFor({ tool: app })}
    >
      {app === "claude-desktop" && desktopInstalling ? (
        <Loader2 className="size-3.5 motion-safe:animate-spin" />
      ) : (
        <Download className="size-3.5" />
      )}
      {t("settings.toolInstall")}
    </Button>
  );

  const renderRow = (row: Row) =>
    row.kind === "workbuddy" ? (
      <AppRow
        key="workbuddy"
        app="workbuddy"
        name={
          <>
            <span className="truncate">{APP_DISPLAY_NAME.workbuddy}</span>
            {officialWorkbuddy && <Pill>Official</Pill>}
          </>
        }
        version={
          officialWorkbuddy ? (
            <span className="font-mono text-body font-medium tabular-nums text-fg-1">
              {officialWorkbuddy.version ?? t("appsPage.detectedInstalled")}
            </span>
          ) : (
            <span className="inline-flex items-center gap-1 text-caption text-fg-2">
              {workbuddyStatus?.exists
                ? t("appsPage.desktopConnected")
                : t("appsPage.desktopNotConnected")}
              <HelpTip title={APP_DISPLAY_NAME.workbuddy} align="end">
                {t("appsPage.workbuddyHelp")}
              </HelpTip>
            </span>
          )
        }
        action={
          !isOfficialWorkbuddyPending &&
          !officialWorkbuddy &&
          desktopInstallButton("workbuddy")
        }
        visibility={visibilitySwitch("workbuddy")}
      />
    ) : row.kind === "desktop" ? (
      <AppRow
        key="claude-desktop"
        app="claude-desktop"
        name={
          <>
            <span className="truncate">
              {APP_DISPLAY_NAME["claude-desktop"]}
            </span>
            {officialClaude && <Pill>Official</Pill>}
          </>
        }
        meta={
          officialClaude && (
            <span className="truncate font-mono" title={officialClaude.path}>
              {officialClaude.path}
            </span>
          )
        }
        version={
          officialClaude ? (
            <span className="font-mono text-body font-medium tabular-nums text-fg-1">
              {officialClaude.version ?? t("appsPage.detectedInstalled")}
            </span>
          ) : (
            <span className="inline-flex items-center gap-1 text-caption text-fg-2">
              {desktopStatus?.supported === false
                ? t("appsPage.desktopUnsupported")
                : desktopStatus?.configured
                  ? t("appsPage.desktopConnected")
                  : t("appsPage.desktopNotConnected")}
              <HelpTip title={APP_DISPLAY_NAME["claude-desktop"]} align="end">
                {t("appsPage.desktopHelp")}
              </HelpTip>
            </span>
          )
        }
        action={
          rowInstalled(row) === false && desktopInstallButton("claude-desktop")
        }
        visibility={visibilitySwitch("claude-desktop")}
      />
    ) : (
      <ToolRow
        key={row.tool}
        tool={row.tool}
        tools={tools}
        onInstall={(manualUpdate) =>
          setInstallCommandsFor({ tool: row.tool, manualUpdate })
        }
        onUninstall={(path) => setPendingUninstall({ tool: row.tool, path })}
        expanded={expanded[row.tool] ?? false}
        onToggleExpanded={() =>
          setExpanded((prev) => ({
            ...prev,
            [row.tool]: !prev[row.tool],
          }))
        }
        visibility={visibilitySwitch(TOOL_APP_IDS[row.tool])}
      />
    );

  return (
    <>
      <AppPageHeader
        icon={<LayoutGrid className="h-5 w-5" strokeWidth={1.5} />}
        title={t("nav.apps")}
        titleExtra={
          <HelpTip title={t("appsPage.helpTitle")}>
            {t("appsPage.helpBody")}
          </HelpTip>
        }
        actions={
          <>
            <span className="whitespace-nowrap text-caption text-fg-3">
              {lastChecked}
            </span>
            {(updatableCount > 0 || tools.batchAction) && (
              <Button
                variant="neutral"
                size="regular"
                disabled={
                  tools.isLoadingTools ||
                  tools.checkingIndependentUpdates ||
                  Boolean(tools.batchAction)
                }
                aria-busy={Boolean(tools.batchAction)}
                onClick={() =>
                  void tools.handleRunToolAction(
                    tools.updatableToolNames,
                    "update",
                    {
                      fromBatchEntry: true,
                      selectedPreviews: tools.batchPreviews,
                      defaultTools: tools.updatableDefaultToolNames,
                    },
                  )
                }
              >
                {tools.batchAction === "update" ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <ArrowUpCircle className="h-3.5 w-3.5" />
                )}
                {t("settings.updateAllTools", { count: updatableCount })}
              </Button>
            )}
            <Button
              variant="neutral"
              size="regular"
              disabled={tools.isLoadingTools || tools.isAnyBusy}
              onClick={() =>
                void Promise.all([
                  tools.checkForUpdates(),
                  refreshClaudeDesktop(),
                  refreshOfficialClaude(),
                  refreshWorkBuddy(),
                  refreshOfficialWorkbuddy(),
                ])
              }
            >
              <RefreshCw
                className={cn(
                  "h-3.5 w-3.5",
                  tools.isLoadingTools && "animate-spin",
                )}
              />
              {/* 两种文案叠在同一格，按钮按较宽的那个定宽，切换时不改变宽度 */}
              <span className="grid justify-items-center [&>*]:col-start-1 [&>*]:row-start-1">
                <span
                  aria-hidden={tools.isLoadingTools}
                  className={cn(tools.isLoadingTools && "invisible")}
                >
                  {t("appsPage.checkUpdates")}
                </span>
                <span
                  aria-hidden={!tools.isLoadingTools}
                  className={cn(!tools.isLoadingTools && "invisible")}
                >
                  {t("appsPage.checking")}
                </span>
              </span>
            </Button>
            <DropdownMenu>
              <HoverTip content={t("common.more")}>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="quiet"
                    size="icon-compact"
                    className="h-8 w-8"
                    aria-label={t("appsPage.moreActions")}
                  >
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent align="end">
                <DropdownMenuItem
                  disabled={
                    tools.isLoadingTools ||
                    tools.isAnyBusy ||
                    tools.isDiagnosingAll
                  }
                  onSelect={() => void tools.handleDiagnoseAll()}
                >
                  {t("settings.toolDiagnose")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => setInstallCommandsFor({ tool: "claude" })}
                >
                  {t("appsPage.manualCommands")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  className="text-red-500 focus:text-red-500 dark:text-red-400 dark:focus:text-red-400"
                  onSelect={() => setShowUninstallCommands(true)}
                >
                  {t("settings.manualUninstallCommands")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable"
      >
        <div ref={tableRef} className="px-6 pb-10 pt-4">
          <div
            className={cn(
              APP_ROW_GRID,
              "mx-px h-9 px-4 text-caption font-medium text-fg-2",
            )}
          >
            <div className="flex min-w-0 items-center gap-2">
              <SectionTitle title={sections[0]?.title} />
              <Button
                variant="ghost"
                size="compact"
                disabled={!settings}
                onClick={() => saveAppOrder(alphabeticalAppOrder)}
              >
                <ListOrdered className="size-3.5" aria-hidden />
                {t("appsPage.sortByName")}
              </Button>
            </div>
            <span className="text-end">{t("appsPage.columnVersion")}</span>
            {/* 看不见的「安装 / 升级」按钮只用来量宽：操作列始终留出按钮的位置，
                版本号加载完或检查到新版本、按钮冒出来时，版本列不会被推开。 */}
            <div
              data-tool-actions
              aria-hidden
              className="invisible grid w-max justify-self-end [&>*]:col-start-1 [&>*]:row-start-1"
            >
              <Button variant="neutral" size="compact" tabIndex={-1}>
                <Download className="h-3.5 w-3.5" />
                {t("settings.toolInstall")}
              </Button>
              <Button variant="neutral" size="compact" tabIndex={-1}>
                <ArrowUpCircle className="h-3.5 w-3.5" />
                {t("settings.toolUpdate")}
              </Button>
              <Button variant="neutral" size="compact" tabIndex={-1}>
                {t("appsPage.repairInstallation")}
              </Button>
            </div>
            <span
              data-sidebar-heading
              className="flex w-max items-center gap-1 whitespace-nowrap justify-self-end"
            >
              {t("appsPage.columnVisible")}
              <HelpTip title={t("appsPage.columnVisible")} align="end">
                {t("appsPage.visibleHelp")}
              </HelpTip>
            </span>
            <span />
          </div>

          {sections.map((section, index) => (
            <section key={section.key} aria-label={section.title}>
              {index > 0 && (
                <div className="mx-px mt-5 flex h-9 items-center px-4">
                  <SectionTitle title={section.title} />
                </div>
              )}
              {section.key === "installed" ? (
                // 只有已安装的能拖：未安装的排在哪无所谓；首次探测时还不知道装没装，也先不给拖
                <DndContext
                  sensors={dragSensors}
                  modifiers={verticalDragModifiers}
                  collisionDetection={closestCenter}
                  onDragEnd={handleDragEnd}
                >
                  <SortableContext
                    items={section.rows.map(rowKey)}
                    strategy={verticalListSortingStrategy}
                  >
                    <div className="apps-group-frame">
                      {section.rows.map((row) => (
                        <SortableAppRow key={rowKey(row)} id={rowKey(row)}>
                          {renderRow(row)}
                        </SortableAppRow>
                      ))}
                    </div>
                  </SortableContext>
                </DndContext>
              ) : (
                <div className="apps-group-frame">
                  {section.rows.map(renderRow)}
                </div>
              )}
            </section>
          ))}
        </div>
      </div>

      {installCommandsFor && (
        <InstallCommandsDialog
          initialTool={installCommandsFor.tool}
          manualUpdateRequested={installCommandsFor.manualUpdate}
          onClose={() => setInstallCommandsFor(null)}
          toolVersionByName={tools.toolVersionByName}
          installationReports={tools.installReports}
          lastCheckedAt={tools.lastCheckedAt}
          officialClaudeDesktop={Boolean(officialClaude)}
          officialWorkbuddy={Boolean(officialWorkbuddy)}
          isAppInstalled={(app) => {
            const id =
              app === "workbuddy" || app === "claude-desktop"
                ? app
                : TOOL_APP_IDS[app];
            return rowInstalled(ROW_BY_APP[id]) === true;
          }}
          onInstall={installFromChannel}
          isInstallBusy={(app) =>
            app === "claude-desktop"
              ? desktopInstalling
              : app !== "workbuddy" && tools.busyTools.has(app)
          }
          isInstallQueued={(app) =>
            app !== "claude-desktop" &&
            app !== "workbuddy" &&
            tools.toolProgress.get(app)?.stage === "queued"
          }
        />
      )}

      <ManualUninstallDialog
        open={showUninstallCommands}
        onOpenChange={setShowUninstallCommands}
        toolVersionByName={tools.toolVersionByName}
      />

      {pendingUninstall && (
        <ToolUninstallConfirmDialog
          tool={pendingUninstall.tool}
          targetPath={pendingUninstall.path}
          wslShellByTool={tools.wslShellByTool}
          onConfirm={(uninstallPlan) => {
            void tools.handleRunToolAction(
              [pendingUninstall.tool],
              "uninstall",
              {
                uninstallPlan,
              },
            );
            setPendingUninstall(null);
          }}
          onCancel={() => setPendingUninstall(null)}
          onManualUninstall={() => {
            setPendingUninstall(null);
            setShowUninstallCommands(true);
          }}
        />
      )}

      <ToolUpgradeConfirmDialog
        isOpen={tools.pendingUpgrade !== null}
        plans={tools.pendingUpgrade?.plans ?? []}
        displayName={toolDisplayName}
        onConfirm={tools.handleConfirmUpgrade}
        onCancel={tools.handleCancelUpgrade}
      />
    </>
  );
}

function AppRow({
  app,
  name,
  meta,
  version,
  action,
  visibility,
  menu,
  footer,
}: {
  app: AppId;
  name: React.ReactNode;
  meta?: React.ReactNode;
  version?: React.ReactNode;
  action?: React.ReactNode;
  visibility: React.ReactNode;
  menu?: React.ReactNode;
  footer?: React.ReactNode;
}) {
  const { t } = useTranslation();
  const dragHandle = useContext(DragHandleContext);
  return (
    <div
      data-tool-row={app}
      className="group relative px-4 py-3 focus-within:bg-subtle"
    >
      {dragHandle && (
        // 放在行首的内边距里，不占列宽，表头和各列照旧对齐
        <button
          type="button"
          className={cn(
            "absolute start-0 top-3 flex h-10 w-4 cursor-grab items-center justify-center text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring active:cursor-grabbing",
            dragHandle.isDragging && "cursor-grabbing opacity-100",
          )}
          aria-label={t("appsPage.dragHandle", { name: APP_DISPLAY_NAME[app] })}
          {...dragHandle.attributes}
          {...dragHandle.listeners}
        >
          <GripVertical className="h-4 w-4" />
        </button>
      )}
      <div className={cn(APP_ROW_GRID, "min-h-10")}>
        <div className="flex min-w-0 items-center gap-4">
          <AppGlyph app={app} size={20} badgeClassName="bg-surface" />
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 flex-wrap items-center gap-1.5 text-strong text-fg-1">
              {name}
            </div>
            {meta && (
              <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-caption text-fg-2">
                {meta}
              </div>
            )}
          </div>
        </div>
        <div className="min-w-0 text-end">{version}</div>
        <div
          data-tool-actions
          className="flex w-max items-center justify-self-end gap-2 [&>button]:shrink-0"
        >
          {action}
        </div>
        <div className="flex justify-end">{visibility}</div>
        <div className="flex justify-end">{menu}</div>
      </div>
      {footer && <div className="mt-2 sm:ms-9">{footer}</div>}
    </div>
  );
}

function Pill({
  children,
  preferredLabel,
}: {
  children: React.ReactNode;
  preferredLabel?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-[18px] shrink-0 items-center gap-1 rounded-full border border-border-strong px-1.5 text-badge text-fg-2",
        preferredLabel && "border-primary/40 bg-primary/10 text-primary",
      )}
      title={preferredLabel}
    >
      {preferredLabel && <Star aria-hidden className="size-2.5" />}
      {children}
      {preferredLabel && <span className="sr-only"> ({preferredLabel})</span>}
    </span>
  );
}

function ToolRow({
  tool,
  tools,
  onInstall,
  onUninstall,
  expanded,
  onToggleExpanded,
  visibility,
}: {
  tool: ToolName;
  tools: ReturnType<typeof useToolManagement>;
  onInstall: (manualUpdate: boolean) => void;
  onUninstall: (path?: string) => void;
  expanded: boolean;
  onToggleExpanded: () => void;
  visibility: React.ReactNode;
}) {
  const { t } = useTranslation();
  const installationPanelId = useId();
  const queryClient = useQueryClient();
  const info = tools.toolVersionByName.get(tool);
  const report: ToolInstallationReport | undefined =
    tools.installReports?.[tool];
  const usesWsl = info?.env_type === "wsl";
  const preferenceKey = [
    "preferred-cli-installation",
    tool,
    info?.env_type,
    info?.wsl_distro,
    tools.lastCheckedAt,
    report?.installs.map(({ path, real_path }) => [path, real_path]),
  ] as const;
  const preferred = useQuery({
    queryKey: preferenceKey,
    queryFn: () => settingsApi.getPreferredToolInstallation(tool),
    enabled: !usesWsl && (expanded || isToolPresent(info, report)),
    staleTime: 30_000,
    retry: false,
  });
  const selectPreferred = useMutation({
    mutationFn: ({
      path,
      enabled,
    }: {
      path: string | null;
      enabled?: boolean;
    }) => settingsApi.setPreferredToolInstallation(tool, path, enabled),
    onMutate: () =>
      queryClient.cancelQueries({ queryKey: preferenceKey, exact: true }),
    onSuccess: (policy) => {
      queryClient.setQueryData(preferenceKey, policy);
    },
    onError: (error) =>
      toast.error(t("appsPage.cliPreferenceFailed"), {
        description: extractErrorMessage(error),
      }),
  });
  const nativeReport = usesWsl ? undefined : report;
  const defaultInstall =
    nativeReport?.installs.find((install) => install.is_path_default) ??
    nativeReport?.installs[0];
  const desktop = info?.desktop_installation;
  const {
    detectedInstalls,
    checkIndependentUpdates,
    updates: installationUpdates,
  } = tools.installationsByTool[tool];
  const inspected =
    !usesWsl && installationUpdates.data?.length
      ? installationUpdates.data.map((row) => row.installation)
      : (detectedInstalls ?? []);
  const installs = desktop
    ? inspected.filter((install) => install.source !== "desktop")
    : inspected;
  const canInspect = Boolean(desktop || installs.length > 0);

  const isVersionLoading =
    Boolean(tools.loadingTools[tool]) ||
    (tools.isLoadingTools && !tools.toolVersionByName.has(tool));
  const desktopManaged =
    Boolean(desktop) ||
    isDesktopManagedTool(tool, tools.overviews) ||
    info?.desktop_managed ||
    defaultInstall?.source === "desktop";
  const hasNewVersion = isUpdateAvailable(info?.version, info?.latest_version);
  const isOutdated =
    !desktopManaged && !nativeReport?.unmanaged && hasNewVersion;
  const copyNodeGap = installationUpdates.data?.find(
    ({ installation }) => installation.is_path_default,
  )?.update_blocked_by_node;
  const nodeGap = isOutdated
    ? (copyNodeGap ?? info?.update_blocked_by_node)
    : null;
  const manualUpdate = Boolean(info?.manual_update_only);
  const broken = Boolean(info?.installed_but_broken) && !desktopManaged;
  const isBusy = tools.busyTools.has(tool);
  const busyAction = tools.busyTools.get(tool);
  const installed = isToolPresent(info, report);
  const canUninstall = !desktopManaged && installed;
  const showManualUpdate =
    manualUpdate && (hasNewVersion || tool === "opencode");
  const action:
    | Exclude<ToolLifecycleAction, "uninstall">
    | "manual-update"
    | null =
    (busyAction !== "uninstall" ? busyAction : undefined) ??
    (isVersionLoading || broken || desktopManaged
      ? null
      : showManualUpdate
        ? "manual-update"
        : nativeReport?.unmanaged
          ? null
          : !installed
            ? "install"
            : isOutdated && !nodeGap && !manualUpdate
              ? "update"
              : null);
  const sourceLabel = desktop
    ? "Official"
    : desktopManaged
      ? tool === "opencode"
        ? "OpenCode Desktop"
        : t("appsPage.desktopInstallation")
      : defaultInstall
        ? getToolInstallSourceLabel(defaultInstall.source)
        : undefined;
  const channels = [
    ...new Set(
      [
        sourceLabel,
        ...installs.map(({ source }) => getToolInstallSourceLabel(source)),
      ].filter(Boolean),
    ),
  ];
  const envBadge =
    info?.env_type === "wsl" && ENV_BADGE_CONFIG.wsl
      ? `${t(ENV_BADGE_CONFIG.wsl.labelKey)}${info.wsl_distro ? ` · ${info.wsl_distro}` : ""}`
      : null;

  const detailsByPath = new Map(
    installationUpdates.data?.map((entry) => [entry.installation.path, entry]),
  );
  const rows: ToolInstallation[] = desktop
    ? [
        {
          path: desktop.path,
          source: "official",
          version: desktop.version,
          runnable: true,
          error: null,
          is_path_default: Boolean(
            installationUpdates.data?.some(
              ({ installation, real_path }) =>
                installation.source === "desktop" &&
                installation.is_path_default &&
                (real_path ?? installation.path).startsWith(`${desktop.path}/`),
            ),
          ),
        },
        ...installs,
      ]
    : installs;
  const fallbackVersion = isVersionLoading ? (
    <span
      role="status"
      aria-label={t("common.loading")}
      className="inline-flex justify-end"
    >
      <Loader2 className="h-4 w-4 animate-spin text-fg-3" />
    </span>
  ) : desktopManaged ? (
    <div className="font-mono text-body font-medium tabular-nums text-fg-1">
      {desktop?.version ?? info?.version ?? t("appsPage.detectedInstalled")}
    </div>
  ) : info?.version ? (
    <div>
      <div className="font-mono text-body font-medium tabular-nums text-fg-1">
        {info.version}
      </div>
      {(isOutdated || (manualUpdate && hasNewVersion)) &&
        info.latest_version && (
          <div className="text-caption text-fg-2">
            {t("appsPage.newVersion", { version: info.latest_version })}
          </div>
        )}
    </div>
  ) : broken ? (
    <div>
      {report?.package_version && (
        <div className="font-mono text-body font-medium tabular-nums text-fg-1">
          {report.package_version}
        </div>
      )}
      <span className="inline-flex items-center gap-1 text-caption text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5" />
        {t("appsPage.notRunnable")}
      </span>
    </div>
  ) : (
    <span className="text-caption text-fg-2">{t("common.notInstalled")}</span>
  );

  const actionButton = action ? (
    <Button
      variant="neutral"
      size="compact"
      onClick={
        action === "install" || action === "manual-update"
          ? () => onInstall(action === "manual-update")
          : () => void tools.handleRunToolAction([tool], action)
      }
      disabled={isVersionLoading || isBusy}
      aria-busy={isBusy}
    >
      {isBusy && busyAction !== "uninstall" ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
      ) : action === "install" ? (
        <Download className="h-3.5 w-3.5" />
      ) : (
        <ArrowUpCircle className="h-3.5 w-3.5" />
      )}
      {action === "install"
        ? t("settings.toolInstall")
        : action === "manual-update"
          ? t("appsPage.manualUpdateGuide")
          : t("settings.toolUpdate")}
    </Button>
  ) : null;

  const policy = usesWsl ? undefined : preferred.data;
  const selectedCopy =
    rows.find((install) => install.path === policy?.selected_path) ??
    rows.find(
      (install) =>
        install.real_path && install.real_path === policy?.selected_real_path,
    );
  const cliSourceLabel = (install: ToolInstallation) =>
    getToolInstallSourceLabel(install.source) ??
    t("appsPage.installSourceUnknown");
  const cliCopies = rows.filter(
    (install) => install.source !== "official" && install.source !== "desktop",
  );
  const isCliDisabled = (install: ToolInstallation) =>
    Boolean(
      policy?.disabled_paths.includes(install.path) ||
        (install.real_path &&
          (policy?.disabled_paths.includes(install.real_path) ||
            policy?.disabled_real_paths?.includes(install.real_path))),
    );
  const eligibleCopies = cliCopies.filter(
    (install) => install.runnable && !isCliDisabled(install),
  );
  const bundledAvailable = Boolean(desktop && policy?.bundled_available);
  const automaticSelection =
    policy?.selection === "automatic" || policy?.selection === "bundled";
  const explicitSelection = policy?.selection === "explicit";
  const usingBundled =
    policy?.selection === "bundled" ||
    (explicitSelection && policy?.path === "bundled");
  const selectedChannel = usingBundled
    ? "Official"
    : selectedCopy && (automaticSelection || explicitSelection)
      ? cliSourceLabel(selectedCopy)
      : undefined;
  const summaryChannel =
    selectedChannel ?? (channels.length === 1 ? channels[0] : undefined);
  let selectionMessage: string | null = null;
  if (
    policy?.supported &&
    !explicitSelection &&
    (!desktop || cliCopies.length > 0 || policy.selected_path)
  ) {
    if (automaticSelection) {
      if ((channels.length > 1 || policy.path) && selectedChannel) {
        selectionMessage = t("appsPage.cliUsingAutomatic", {
          channel: selectedChannel,
        });
      }
    } else if (policy.selection === "disabled") {
      selectionMessage = t(
        cliCopies.length > 1
          ? "appsPage.cliAllDisabled"
          : "appsPage.cliInstallationDisabled",
      );
    } else {
      selectionMessage = t(
        policy.selection === "ambiguous"
          ? "appsPage.cliSelectionAmbiguous"
          : policy.selection === "unavailable"
            ? "appsPage.cliSelectionUnavailable"
            : "appsPage.cliSelectionUnknown",
      );
    }
  }
  const preferredCopy = eligibleCopies.find(
    (install) => install.path === policy?.path,
  );
  const preferredBundled = bundledAvailable && policy?.path === "bundled";
  const selectedOptionLabel = preferredBundled
    ? "Official"
    : preferredCopy
      ? cliSourceLabel(preferredCopy)
      : policy?.path
        ? t("appsPage.cliDefaultUnavailable")
        : t("common.auto");
  const hasRepeatedChannel =
    new Set(eligibleCopies.map(cliSourceLabel)).size !== eligibleCopies.length;
  const versionSelector =
    policy?.supported &&
    (policy.path ||
      cliCopies.length > 1 ||
      (bundledAvailable && cliCopies.length > 0)) ? (
      <Select
        value={policy.path ?? "automatic"}
        disabled={isBusy || selectPreferred.isPending}
        onValueChange={(path) =>
          selectPreferred.mutate({ path: path === "automatic" ? null : path })
        }
      >
        <SelectTrigger
          aria-label={t("appsPage.cliVersionChoice")}
          title={selectedOptionLabel}
          className="h-7 w-max min-w-20 max-w-32 gap-1.5 px-2 text-caption [&>svg]:size-3 [&>svg]:shrink-0"
        >
          <SelectValue>{selectedOptionLabel}</SelectValue>
        </SelectTrigger>
        <SelectContent
          align="end"
          sideOffset={4}
          className={cn(
            "min-w-20",
            hasRepeatedChannel &&
              "w-[min(20rem,var(--radix-select-content-available-width))]",
          )}
        >
          <SelectItem value="automatic">{t("common.auto")}</SelectItem>
          {bundledAvailable && (
            <SelectItem value="bundled">Official</SelectItem>
          )}
          {policy.path &&
            !preferredBundled &&
            !eligibleCopies.some((install) => install.path === policy.path) && (
              <SelectItem value={policy.path} disabled>
                {t("appsPage.cliDefaultUnavailable")}
              </SelectItem>
            )}
          {eligibleCopies.map((install) => {
            const channel = cliSourceLabel(install);
            const repeated =
              hasRepeatedChannel &&
              eligibleCopies.some(
                (other) =>
                  other.path !== install.path &&
                  cliSourceLabel(other) === channel,
              );
            return (
              <SelectItem
                key={install.path}
                value={install.path}
                textValue={channel}
              >
                <span className="block min-w-0">
                  <span className="block truncate">{channel}</span>
                  {repeated && (
                    <span
                      className="block truncate text-caption text-fg-2"
                      title={install.path}
                    >
                      {install.path}
                    </span>
                  )}
                </span>
              </SelectItem>
            );
          })}
        </SelectContent>
      </Select>
    ) : null;

  const primaryPath =
    report?.update_plan?.path ??
    installs.find((install) => install.is_path_default)?.path ??
    defaultInstall?.path;
  const primaryUpdate = primaryPath
    ? detailsByPath.get(primaryPath)
    : undefined;
  const primaryUpdateShown =
    checkIndependentUpdates &&
    primaryUpdate?.latest_version === info?.latest_version &&
    isUpdateAvailable(
      primaryUpdate?.installation.version ?? primaryUpdate?.package_version,
      primaryUpdate?.latest_version,
    );
  const showCopyVersions =
    rows.length > 0 && info?.env_type !== "wsl" && !isVersionLoading;
  const version = showCopyVersions ? (
    <ul
      aria-label={t("appsPage.columnVersion")}
      className="space-y-1 tabular-nums"
    >
      {rows.map((install) => {
        const label =
          rows.length === 1
            ? undefined
            : install.source === "official"
              ? "Official"
              : install.source === "desktop"
                ? tool === "opencode"
                  ? "OpenCode Desktop"
                  : t("appsPage.desktopInstallation")
                : (getToolInstallSourceLabel(install.source) ??
                  t("appsPage.installSourceUnknown"));
        const detail = detailsByPath.get(install.path);
        const isDefaultCopy =
          report?.update_plan?.path === install.path ||
          install.is_path_default ||
          rows.length === 1;
        const installedVersion =
          install.version ??
          detail?.package_version ??
          (isDefaultCopy ? report?.package_version : null);
        return (
          <li
            key={install.path}
            className="flex min-w-0 items-center justify-end gap-x-2 whitespace-nowrap"
          >
            {label && (
              <span className="truncate text-caption text-fg-2" title={label}>
                {label}
              </span>
            )}
            {installedVersion && (
              <span className="shrink-0 font-mono text-body font-medium text-fg-1">
                {installedVersion}
              </span>
            )}
            {!install.runnable ? (
              <span className="truncate text-caption text-warning-text">
                {t("appsPage.notRunnable")}
              </span>
            ) : (
              !installedVersion && (
                <span className="truncate text-caption text-fg-2">
                  {t("appsPage.detectedInstalled")}
                </span>
              )
            )}
            {checkIndependentUpdates &&
              detail?.latest_version &&
              isUpdateAvailable(installedVersion, detail.latest_version) && (
                <span className="truncate text-caption text-fg-2">
                  {t("appsPage.newVersion", { version: detail.latest_version })}
                </span>
              )}
          </li>
        );
      })}
      {!primaryUpdateShown &&
        (isOutdated || (manualUpdate && hasNewVersion)) &&
        info?.latest_version && (
          <li className="truncate text-caption text-fg-2">
            {t("appsPage.newVersion", { version: info.latest_version })}
          </li>
        )}
    </ul>
  ) : (
    fallbackVersion
  );
  const repairPlan = rows
    .filter(
      (install) =>
        !install.runnable && (install.is_path_default || rows.length === 1),
    )
    .map((install) => {
      const detail = detailsByPath.get(install.path);
      return detail?.package_version || report?.package_version
        ? (detail?.plan ??
            (report?.update_plan?.path === install.path
              ? report.update_plan
              : null))
        : null;
    })
    .find((plan) => plan?.source === "npm");

  // 执行期间副行换成进度：排队中，或已用时间 + 命令最新一行输出
  const progress = isBusy ? tools.toolProgress.get(tool) : undefined;
  const progressMeta =
    progress?.stage === "queued" ? (
      <span className="truncate">{t("appsPage.toolQueued")}</span>
    ) : progress?.stage === "running" ? (
      <>
        <ElapsedTime since={progress.startedAt} />
        {progress.line && (
          <span className="min-w-0 truncate font-mono" title={progress.line}>
            {progress.line}
          </span>
        )}
      </>
    ) : null;
  const wslControls =
    info?.env_type === "wsl" || tools.overviews?.[tool]?.is_wsl === true ? (
      <div className="flex items-center gap-2 text-caption text-fg-2">
        <span>Shell</span>
        <Select
          value={tools.wslShellByTool[tool]?.wslShell || "auto"}
          onValueChange={(value) =>
            void tools.handleToolShellChange(tool, value)
          }
          disabled={isVersionLoading || isBusy}
        >
          <SelectTrigger className="h-7 w-[82px] text-caption">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="auto">{t("common.auto")}</SelectItem>
            {WSL_SHELL_OPTIONS.map((shell) => (
              <SelectItem key={shell} value={shell}>
                {shell}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span>{t("appsPage.wslFlag")}</span>
        <Select
          value={tools.wslShellByTool[tool]?.wslShellFlag || "auto"}
          onValueChange={(value) =>
            void tools.handleToolShellFlagChange(tool, value)
          }
          disabled={isVersionLoading || isBusy}
        >
          <SelectTrigger className="h-7 w-[82px] text-caption">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="auto">{t("common.auto")}</SelectItem>
            {WSL_SHELL_FLAG_OPTIONS.map((flag) => (
              <SelectItem key={flag} value={flag}>
                {flag}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    ) : null;

  const errorLine =
    !isVersionLoading && !desktopManaged && !info?.version && info?.error ? (
      <ToolErrorMessage message={info.error} />
    ) : null;

  const nodeLine =
    !isVersionLoading && nodeGap && !(expanded && copyNodeGap) ? (
      <p className="flex items-start gap-1 text-caption text-warning-text">
        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
        {t("appsPage.updateNeedsNode", {
          version: info?.latest_version,
          required: nodeGap.required,
          current: nodeGap.current,
        })}
      </p>
    ) : null;

  const loadingDetails = expanded && installationUpdates.isFetching;
  const installationList =
    expanded && canInspect ? (
      <section
        id={installationPanelId}
        aria-label={t("appsPage.inspectInstallations")}
        className="overflow-hidden rounded-control border border-border"
      >
        {installationUpdates.isError && (
          <div className="flex items-center gap-2 border-b border-border bg-subtle px-3 py-2">
            <Button
              variant="quiet"
              size="compact"
              onClick={() => void installationUpdates.refetch()}
            >
              {t("common.retry")}
            </Button>
          </div>
        )}
        <ul className="divide-y divide-border bg-surface">
          {rows.map((install) => {
            const detail = detailsByPath.get(install.path);
            const disabled = isCliDisabled(install);
            const updatePlan = checkIndependentUpdates
              ? availableInstallationUpdate(detail)
              : undefined;
            const isPreferred =
              explicitSelection &&
              (usingBundled
                ? install.source === "official"
                : selectedCopy?.path === install.path);
            const managedDesktop =
              install.source === "official" || install.source === "desktop";
            return (
              <li
                key={install.path}
                className="flex flex-wrap items-start gap-x-4 gap-y-1.5 px-3 py-2"
              >
                <div className="min-w-0 flex-1 basis-52">
                  <div className="flex min-h-7 min-w-0 items-center gap-2 whitespace-nowrap [&>div]:shrink-0">
                    <ToolInstallRow
                      inst={install}
                      packageVersion={detail?.package_version}
                      hidePath
                    />
                    {install.source === "official" &&
                      desktop?.bundled_cli_version && (
                        <span className="shrink-0 text-caption text-fg-2">
                          {t("appsPage.bundledCliVersion", {
                            version: desktop.bundled_cli_version,
                          })}
                        </span>
                      )}
                    {managedDesktop && (
                      <span
                        className="truncate text-caption text-fg-2"
                        title={t("appsPage.desktopSelfUpdates")}
                      >
                        {t("appsPage.desktopSelfUpdates")}
                      </span>
                    )}
                    {isPreferred && (
                      <span className="inline-flex h-[18px] shrink-0 items-center rounded-full border border-primary/30 bg-primary/10 px-1.5 text-badge font-medium text-primary">
                        {t("appsPage.cliPreferred")}
                      </span>
                    )}
                    {disabled && <Pill>{t("appsPage.cliDisabled")}</Pill>}
                  </div>
                  <p className="break-all whitespace-normal font-mono text-caption text-fg-2">
                    {install.path}
                  </p>
                  {install.error && !install.runnable && (
                    <ToolErrorMessage message={install.error} />
                  )}
                  {detail?.update_blocked_by_node && (
                    <p className="text-caption text-warning-text">
                      {t("appsPage.updateNeedsNode", {
                        version: detail.latest_version,
                        required: detail.update_blocked_by_node.required,
                        current: detail.update_blocked_by_node.current,
                      })}
                    </p>
                  )}
                </div>
                {!managedDesktop && (
                  <div className="ms-auto flex min-h-7 shrink-0 items-center gap-1">
                    {updatePlan && (
                      <Button
                        size="compact"
                        variant="quiet"
                        disabled={isBusy}
                        onClick={() =>
                          tools.handleSelectInstallUpdate(
                            tool,
                            rows,
                            updatePlan,
                          )
                        }
                      >
                        {t("settings.toolUpdate")}
                      </Button>
                    )}
                    {policy?.supported && (
                      <HoverTip content={t("appsPage.cliAvailability")}>
                        <label className="inline-flex items-center gap-2 whitespace-nowrap px-2 text-caption text-fg-2">
                          <span>{t("appsPage.cliEnable")}</span>
                          <Switch
                            size="sm"
                            aria-label={`${t("appsPage.cliEnable")} · ${cliSourceLabel(install)}`}
                            checked={!disabled}
                            disabled={isBusy || selectPreferred.isPending}
                            onCheckedChange={(enabled) =>
                              selectPreferred.mutate({
                                path: install.path,
                                enabled,
                              })
                            }
                          />
                        </label>
                      </HoverTip>
                    )}
                    <Button
                      size="compact"
                      variant="quiet"
                      disabled={isBusy}
                      className="text-danger-text"
                      onClick={() => onUninstall(install.path)}
                    >
                      {t("settings.toolUninstall")}
                    </Button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      </section>
    ) : null;

  const desktopLine =
    tool === "opencode" && desktopManaged && hasNewVersion ? (
      <p className="flex items-center gap-1 text-caption text-fg-2">
        <Info className="h-3.5 w-3.5" />
        {t("settings.toolUpgradeDesktopManagedDetail")}
      </p>
    ) : null;
  const footer =
    desktopLine || wslControls || errorLine || nodeLine || installationList ? (
      <div className="space-y-2">
        {errorLine}
        {nodeLine}
        {desktopLine}
        {wslControls}
        {installationList}
      </div>
    ) : undefined;

  return (
    <AppRow
      app={TOOL_APP_IDS[tool]}
      name={
        <>
          <span className="truncate">{toolDisplayName(tool)}</span>
          {summaryChannel && (
            <Pill
              preferredLabel={
                explicitSelection && selectedChannel
                  ? t("appsPage.cliPreferred")
                  : undefined
              }
            >
              {summaryChannel}
            </Pill>
          )}
          {envBadge && <Pill>{envBadge}</Pill>}
        </>
      }
      meta={
        progressMeta ??
        (selectionMessage && (
          <span
            role="status"
            className="max-w-full truncate"
            title={selectionMessage}
          >
            {selectionMessage}
          </span>
        ))
      }
      version={version}
      action={
        <>
          {versionSelector}
          {repairPlan && !actionButton ? (
            <Button
              size="compact"
              variant="neutral"
              disabled={isBusy}
              onClick={() =>
                tools.handleSelectInstallUpdate(tool, rows, repairPlan)
              }
            >
              {t("appsPage.repairInstallation")}
            </Button>
          ) : (
            actionButton
          )}
        </>
      }
      visibility={visibility}
      menu={
        canInspect ? (
          <HoverTip content={t("appsPage.manageInstallations")}>
            <Button
              size="icon-compact"
              variant="quiet"
              onClick={onToggleExpanded}
              aria-label={t("appsPage.manageInstallations")}
              aria-expanded={expanded}
              aria-controls={expanded ? installationPanelId : undefined}
              aria-busy={loadingDetails || undefined}
              className={cn(expanded && "bg-subtle text-fg-1")}
            >
              {loadingDetails ? (
                <Loader2
                  aria-hidden
                  className="size-4 motion-safe:animate-spin"
                />
              ) : (
                <MoreHorizontal
                  aria-hidden
                  className="size-4"
                  strokeWidth={1.5}
                />
              )}
            </Button>
          </HoverTip>
        ) : (
          (canUninstall || busyAction === "uninstall") && (
            <DropdownMenu>
              <HoverTip content={t("common.more")}>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="quiet"
                    size="icon-compact"
                    aria-label={t("appPage.moreActions", {
                      name: toolDisplayName(tool),
                    })}
                    disabled={isVersionLoading || isBusy}
                    aria-busy={busyAction === "uninstall"}
                  >
                    {busyAction === "uninstall" ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
                    )}
                  </Button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent align="end" className="min-w-[160px]">
                <DropdownMenuItem
                  onSelect={() => onUninstall()}
                  className="text-danger-text focus:text-danger-text"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  {t("settings.toolUninstall")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )
        )
      }
      footer={footer}
    />
  );
}

function formatElapsed(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

function ElapsedTime({ since }: { since: number }) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return (
    <span className="shrink-0 tabular-nums">
      {t("appsPage.toolElapsed", { time: formatElapsed(now - since) })}
    </span>
  );
}

function SortableAppRow({
  id,
  children,
}: {
  id: AppId;
  children: React.ReactNode;
}) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });
  const style: CSSProperties = {
    transform: CSS.Translate.toString(transform),
    transition,
    position: "relative",
    zIndex: isDragging ? 10 : undefined,
  };
  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(isDragging && "bg-surface ring-1 ring-inset ring-ring")}
    >
      <DragHandleContext.Provider value={{ attributes, listeners, isDragging }}>
        {children}
      </DragHandleContext.Provider>
    </div>
  );
}

function SectionTitle({ title }: { title?: string }) {
  return (
    <h2 className="m-0 truncate text-caption font-semibold text-fg-2">
      {title}
    </h2>
  );
}
