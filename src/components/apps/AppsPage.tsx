import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
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
  ChevronDown,
  Copy,
  Download,
  GripVertical,
  LayoutGrid,
  Loader2,
  MoreHorizontal,
  RefreshCw,
  Trash2,
} from "lucide-react";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import type { ToolInstallationReport } from "@/lib/api/settings";
import type { VisibleApps } from "@/types";
import { DEFAULT_VISIBLE_APPS, orderAppIds } from "@/config/appConfig";
import { useSettings } from "@/hooks/useSettings";
import { isUpdateAvailable } from "@/lib/version";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import { ToolInstallRow } from "@/components/settings/ToolInstallRow";
import { ToolErrorMessage } from "@/components/settings/ToolErrorMessage";
import { ToolUninstallConfirmDialog } from "./ToolUninstallConfirmDialog";
import { ManualUninstallDialog } from "./ManualUninstallDialog";
import {
  ENV_BADGE_CONFIG,
  ONE_CLICK_INSTALL_COMMANDS,
  TOOL_APP_IDS,
  TOOL_NAMES,
  toolDisplayName,
  useToolManagement,
  WSL_SHELL_FLAG_OPTIONS,
  WSL_SHELL_OPTIONS,
  type ToolLifecycleAction,
  type ToolName,
} from "./useToolManagement";

/** 安装来源的显示名：只标认得出的，认不出（system）就不标，免得标错。 */
const SOURCE_LABEL: Record<string, string> = {
  homebrew: "Homebrew",
  nvm: "npm",
  volta: "npm",
  fnm: "npm",
  mise: "npm",
  bun: "bun",
  pnpm: "pnpm",
  scoop: "Scoop",
  pip: "pip",
};

type Row = { kind: "tool"; tool: ToolName } | { kind: "desktop" };

const ROW_BY_APP = Object.fromEntries([
  ["claude-desktop", { kind: "desktop" }],
  ...TOOL_NAMES.map((tool) => [TOOL_APP_IDS[tool], { kind: "tool", tool }]),
]) as Record<AppId, Row>;

type RowSection = { key: string; title: string; rows: Row[] };

const rowKey = (row: Row): AppId =>
  row.kind === "desktop" ? "claude-desktop" : TOOL_APP_IDS[row.tool];

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

/** 本机有这个命令行应用：探到了版本、装了但跑不起来，或探测报告里有安装位置（和「卸载」出现的条件一致）。 */
function isToolPresent(
  info: { version?: string | null; installed_but_broken?: boolean } | undefined,
  report: ToolInstallationReport | undefined,
): boolean {
  return Boolean(
    info?.version || info?.installed_but_broken || report?.installs.length,
  );
}

// 表头和应用行共用列宽；操作列宽取各行按钮和表头量宽按钮的最大值，最后一列是行尾的「⋯」菜单。
const APP_ROW_GRID =
  "grid grid-cols-[minmax(0,1fr)_168px_var(--apps-actions-width,0px)_56px_28px] items-center gap-x-4";

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
  const tools = useToolManagement();
  const { settings, updateSettings, autoSaveSettings } = useSettings();
  const tableRef = useRef<HTMLDivElement>(null);
  const { data: desktopStatus, isPending: isDesktopStatusPending } = useQuery({
    queryKey: ["claude-desktop-status"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    staleTime: 30_000,
  });

  // 按「已安装 / 未安装」分两组。Claude Desktop 没有命令可探，用后端按数据目录 / 应用包推断的结果。
  // 首次探测还没出结果时不分组，免得各行探完一个跳一个；之后重新检查时沿用上次结果，位置不动。
  const rowInstalled = (row: Row): boolean | null => {
    if (row.kind === "desktop") {
      return isDesktopStatusPending ? null : Boolean(desktopStatus?.installed);
    }
    const info = tools.toolVersionByName.get(row.tool);
    if (!info && tools.isLoadingTools) return null;
    return isToolPresent(info, tools.installReports?.[row.tool]);
  };
  // 两组共用一份顺序（拖动后写进设置，侧栏也按它排），每组只是按装没装过滤
  const appOrder = orderAppIds(settings?.appOrder);
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
    const updateActionWidth = () => {
      const width = Math.max(
        0,
        ...actions.map((action) => action.getBoundingClientRect().width),
      );
      table.style.setProperty("--apps-actions-width", `${width}px`);
    };
    updateActionWidth();
    // 按钮增减、文案或字体变化时同步整列，避免各行自行收缩后版本列错位。
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateActionWidth);
    actions.forEach((action) => observer.observe(action));
    return () => observer.disconnect();
  }, [sectionLayoutKey]);

  const [showCommands, setShowCommands] = useState(false);
  const [showUninstallCommands, setShowUninstallCommands] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<ToolName | null>(
    null,
  );
  const [expanded, setExpanded] = useState<Partial<Record<ToolName, boolean>>>(
    {},
  );
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
    settledUpdatableCount.current = tools.updatableToolNames.length;
  }
  const updatableCount = tools.isLoadingTools
    ? settledUpdatableCount.current
    : tools.updatableToolNames.length;

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
    const next = arrayMove(appOrder, oldIndex, newIndex);
    const previous = settings.appOrder;
    updateSettings({ appOrder: next });
    void autoSaveSettings({ appOrder: next }).catch(() =>
      updateSettings({ appOrder: previous }),
    );
  };

  const renderRow = (row: Row) =>
    row.kind === "desktop" ? (
      <AppRow
        key="claude-desktop"
        app="claude-desktop"
        name={
          <span className="truncate">{APP_DISPLAY_NAME["claude-desktop"]}</span>
        }
        version={
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
        }
        visibility={visibilitySwitch("claude-desktop")}
      />
    ) : (
      <ToolRow
        key={row.tool}
        tool={row.tool}
        tools={tools}
        onUninstall={() => setPendingUninstall(row.tool)}
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
                disabled={tools.isLoadingTools || Boolean(tools.batchAction)}
                aria-busy={Boolean(tools.batchAction)}
                onClick={() =>
                  void tools.handleRunToolAction(
                    tools.updatableToolNames,
                    "update",
                    { fromBatchEntry: true },
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
              onClick={() => void tools.checkForUpdates()}
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
                <DropdownMenuItem onSelect={() => setShowCommands(true)}>
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
              "mx-px h-8 px-4 text-caption font-medium text-fg-2",
            )}
          >
            <SectionTitle title={sections[0]?.title} />
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
            </div>
            <span className="flex items-center justify-end gap-0.5">
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
                <div className="mx-px mt-4 flex h-8 items-center px-4">
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
                    <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
                      {section.rows.map((row) => (
                        <SortableAppRow key={rowKey(row)} id={rowKey(row)}>
                          {renderRow(row)}
                        </SortableAppRow>
                      ))}
                    </div>
                  </SortableContext>
                </DndContext>
              ) : (
                <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
                  {section.rows.map(renderRow)}
                </div>
              )}
            </section>
          ))}
        </div>
      </div>

      <Dialog open={showCommands} onOpenChange={setShowCommands}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("settings.manualInstallCommands")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 px-6">
            <p className="text-body text-fg-2">
              {t("settings.oneClickInstallHint")}
            </p>
            <pre className="max-h-[50vh] overflow-auto rounded-control border border-border bg-subtle px-3 py-2.5 font-mono text-caption">
              {ONE_CLICK_INSTALL_COMMANDS}
            </pre>
          </div>
          <DialogFooter>
            <Button
              variant="neutral"
              size="regular"
              onClick={() => setShowCommands(false)}
            >
              {t("common.close")}
            </Button>
            <Button
              variant="solid"
              size="regular"
              onClick={() => void tools.handleCopyInstallCommands()}
            >
              <Copy className="h-3.5 w-3.5" />
              {t("common.copy")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ManualUninstallDialog
        open={showUninstallCommands}
        onOpenChange={setShowUninstallCommands}
        toolVersionByName={tools.toolVersionByName}
      />

      {pendingUninstall && (
        <ToolUninstallConfirmDialog
          tool={pendingUninstall}
          wslShellByTool={tools.wslShellByTool}
          onConfirm={(uninstallPlan) => {
            void tools.handleRunToolAction([pendingUninstall], "uninstall", {
              uninstallPlan,
            });
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
    <div data-tool-row={app} className="group relative px-4 py-3">
      {dragHandle && (
        // 放在行首的内边距里，不占列宽，表头和各列照旧对齐
        <button
          type="button"
          className={cn(
            "absolute start-0 top-3 flex h-10 w-4 cursor-grab items-center justify-center text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 active:cursor-grabbing",
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
            <div className="flex min-w-0 items-center gap-1.5 text-strong text-fg-1">
              {name}
            </div>
            {meta && (
              <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-caption text-fg-2">
                {meta}
              </div>
            )}
          </div>
        </div>
        <div className="text-end">{version}</div>
        <div
          data-tool-actions
          className="flex w-max items-center justify-self-end gap-2 [&>button]:shrink-0"
        >
          {action}
        </div>
        <div className="flex justify-end">{visibility}</div>
        <div className="flex justify-end">{menu}</div>
      </div>
      {footer && <div className="ms-9 mt-2">{footer}</div>}
    </div>
  );
}

function Pill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex h-[18px] shrink-0 items-center rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
      {children}
    </span>
  );
}

function ToolRow({
  tool,
  tools,
  onUninstall,
  expanded,
  onToggleExpanded,
  visibility,
}: {
  tool: ToolName;
  tools: ReturnType<typeof useToolManagement>;
  onUninstall: () => void;
  expanded: boolean;
  onToggleExpanded: () => void;
  visibility: React.ReactNode;
}) {
  const { t } = useTranslation();
  const info = tools.toolVersionByName.get(tool);
  const report: ToolInstallationReport | undefined =
    tools.installReports?.[tool];
  const defaultInstall =
    report?.installs.find((install) => install.is_path_default) ??
    report?.installs[0];
  const otherInstalls = (report?.installs.length ?? 0) - 1;
  // 「诊断安装冲突」的结果优先（升级后会重新诊断），否则用打开页面时探测到的安装分布
  const installs = tools.toolDiagnostics[tool] ?? report?.installs;

  const isVersionLoading =
    Boolean(tools.loadingTools[tool]) ||
    (tools.isLoadingTools && !tools.toolVersionByName.has(tool));
  const isOutdated = isUpdateAvailable(info?.version, info?.latest_version);
  const nodeGap = isOutdated ? info?.update_blocked_by_node : null;
  const broken = Boolean(info?.installed_but_broken);
  const isBusy = tools.busyTools.has(tool);
  const busyAction = tools.busyTools.get(tool);
  const canUninstall = isToolPresent(info, report);
  const action: Exclude<ToolLifecycleAction, "uninstall"> | null =
    (busyAction !== "uninstall" ? busyAction : undefined) ??
    (isVersionLoading || broken
      ? null
      : !info?.version
        ? "install"
        : isOutdated && !nodeGap
          ? "update"
          : null);
  const sourceLabel = defaultInstall
    ? SOURCE_LABEL[defaultInstall.source]
    : undefined;
  const envBadge =
    info?.env_type === "wsl" && ENV_BADGE_CONFIG.wsl
      ? `${t(ENV_BADGE_CONFIG.wsl.labelKey)}${info.wsl_distro ? ` · ${info.wsl_distro}` : ""}`
      : null;

  const version = isVersionLoading ? (
    <span
      role="status"
      aria-label={t("common.loading")}
      className="inline-flex justify-end"
    >
      <Loader2 className="h-4 w-4 animate-spin text-fg-3" />
    </span>
  ) : info?.version ? (
    <div>
      <div className="font-mono text-body font-medium tabular-nums text-fg-1">
        {info.version}
      </div>
      {isOutdated && info.latest_version && (
        <div className="text-caption text-fg-2">
          {t("appsPage.newVersion", { version: info.latest_version })}
        </div>
      )}
    </div>
  ) : broken ? (
    <span className="inline-flex items-center gap-1 text-caption font-medium text-warning-text">
      <AlertTriangle className="h-3.5 w-3.5" />
      {t("appsPage.notRunnable")}
    </span>
  ) : (
    <span className="text-caption text-fg-2">{t("common.notInstalled")}</span>
  );

  const actionButton = action ? (
    <Button
      variant="neutral"
      size="compact"
      onClick={() => void tools.handleRunToolAction([tool], action)}
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
        : t("settings.toolUpdate")}
    </Button>
  ) : null;

  const installHint = t(`appsPage.installHint.${tool}`, { defaultValue: "" });
  const meta = defaultInstall ? (
    <>
      <span className="min-w-0 truncate font-mono" title={defaultInstall.path}>
        {defaultInstall.path}
      </span>
      {otherInstalls > 0 && (
        <button
          type="button"
          onClick={onToggleExpanded}
          aria-expanded={expanded}
          className="inline-flex shrink-0 items-center gap-0.5 font-medium text-warning-text hover:underline"
        >
          {t("appsPage.otherInstalls", { count: otherInstalls })}
          <ChevronDown
            className={cn(
              "h-3 w-3 transition-transform",
              expanded && "rotate-180",
            )}
          />
        </button>
      )}
    </>
  ) : !info?.version && !isVersionLoading ? (
    <span className="truncate" title={installHint}>
      {installHint}
    </span>
  ) : null;

  const wslControls =
    info?.env_type === "wsl" ? (
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
    !isVersionLoading && !info?.version && info?.error ? (
      <ToolErrorMessage message={info.error} />
    ) : null;

  const nodeLine =
    !isVersionLoading && nodeGap ? (
      <p className="flex items-start gap-1 text-caption text-warning-text">
        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
        {t("appsPage.updateNeedsNode", {
          version: info?.latest_version,
          required: nodeGap.required,
          current: nodeGap.current,
        })}
      </p>
    ) : null;

  const conflictList =
    expanded && installs && installs.length > 1 ? (
      <div className="space-y-1.5 rounded-control bg-warning-soft p-2.5">
        <div className="text-caption font-medium text-warning-text">
          {t("appsPage.installsFound", { count: installs.length })}
        </div>
        <ul className="space-y-1">
          {installs.map((install) => (
            <li key={install.path}>
              <ToolInstallRow inst={install} />
            </li>
          ))}
        </ul>
      </div>
    ) : null;

  const footer =
    wslControls || errorLine || nodeLine || conflictList ? (
      <div className="space-y-2">
        {errorLine}
        {nodeLine}
        {wslControls}
        {conflictList}
      </div>
    ) : undefined;

  return (
    <AppRow
      app={TOOL_APP_IDS[tool]}
      name={
        <>
          <span className="truncate">{toolDisplayName(tool)}</span>
          {sourceLabel && <Pill>{sourceLabel}</Pill>}
          {envBadge && <Pill>{envBadge}</Pill>}
        </>
      }
      meta={meta}
      version={version}
      action={actionButton}
      visibility={visibility}
      menu={
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
                onSelect={onUninstall}
                className="text-danger-text focus:text-danger-text"
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("settings.toolUninstall")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )
      }
      footer={footer}
    />
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
      className={cn(isDragging && "bg-surface shadow-v7-md")}
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
