import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  AlertTriangle,
  ArrowUpCircle,
  ChevronDown,
  Copy,
  Download,
  LayoutGrid,
  Loader2,
  MoreHorizontal,
  RefreshCw,
  Trash2,
} from "lucide-react";
import type { AppId } from "@/lib/api";
import { providersApi, type ChatGptDesktopStatus } from "@/lib/api/providers";
import type { ToolInstallationReport } from "@/lib/api/settings";
import type { VisibleApps } from "@/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { useSettings } from "@/hooks/useSettings";
import { isMac } from "@/lib/platform";
import { extractErrorMessage } from "@/utils/errorUtils";
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
import {
  APP_DISPLAY_NAME,
  AppGlyph,
  CHATGPT_DESKTOP_NAME,
  type AppGlyphBadge,
} from "@/components/shell/AppGlyph";
import { ToolUpgradeConfirmDialog } from "@/components/settings/ToolUpgradeConfirmDialog";
import { ToolInstallRow } from "@/components/settings/ToolInstallRow";
import { ToolErrorMessage } from "@/components/settings/ToolErrorMessage";
import { ToolUninstallConfirmDialog } from "./ToolUninstallConfirmDialog";
import { ChatgptDesktopUninstallDialog } from "./ChatgptDesktopUninstallDialog";
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

type Row =
  | { kind: "tool"; tool: ToolName }
  | { kind: "desktop" }
  | { kind: "chatgpt-desktop" };

// 桌面版紧跟在同一家的命令行版后面：Claude Code → Claude Desktop，Codex CLI → ChatGPT Desktop
const ROWS: Row[] = TOOL_NAMES.flatMap((tool): Row[] => {
  const row: Row = { kind: "tool", tool };
  if (tool === "claude") return [row, { kind: "desktop" }];
  if (tool === "codex") return [row, { kind: "chatgpt-desktop" }];
  return [row];
});

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
  }, []);

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
  const { data: desktopStatus } = useQuery({
    queryKey: ["claude-desktop-status"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    staleTime: 30_000,
  });
  const chatgptDesktop = useQuery({
    queryKey: ["chatgpt-desktop-status"],
    queryFn: () => providersApi.getChatGptDesktopStatus(),
    staleTime: 30_000,
  });
  const queryClient = useQueryClient();
  const [confirmDesktopUninstall, setConfirmDesktopUninstall] = useState<{
    cleanupOnly: boolean;
    command: string | null;
  } | null>(null);
  const uninstallDesktop = useMutation({
    mutationFn: async (cleanupOnly: boolean) => {
      await providersApi.uninstallChatGptDesktop(cleanupOnly);
      return cleanupOnly;
    },
    onSuccess: (cleanupOnly) =>
      toast.success(
        t(
          cleanupOnly
            ? "appsPage.chatgptDesktop.cacheCleaned"
            : "appsPage.chatgptDesktop.uninstalled",
        ),
        {
          closeButton: true,
        },
      ),
    onError: (error) =>
      toast.error(t("settings.toolUninstallFailed"), {
        description: extractErrorMessage(error),
        closeButton: true,
      }),
    // 成败都重查一次：失败也可能卸掉了一部分
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["chatgpt-desktop-status"] }),
  });

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

  // 一个应用可以占几行（Codex CLI 和 ChatGPT Desktop 共用侧栏的 Codex 入口）：这几行的开关
  // 读写同一个设置，所以一起开、一起关。`name` 只是让每个开关的无障碍名称对上自己那一行。
  const visibilitySwitch = (app: AppId, name = APP_DISPLAY_NAME[app]) => (
    <Switch
      size="sm"
      checked={visibleApps[app]}
      disabled={!settings || (visibleApps[app] && visibleCount <= 1)}
      onCheckedChange={(checked) => setVisible(app, checked)}
      aria-label={t("appsPage.showInSidebar", { name })}
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
              onClick={() => {
                void tools.checkForUpdates();
                void chatgptDesktop.refetch();
              }}
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
            <span>{t("appsPage.columnApp")}</span>
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

          <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
            {ROWS.map((row) => {
              switch (row.kind) {
                case "desktop":
                  return (
                    <AppRow
                      key="claude-desktop"
                      app="claude-desktop"
                      name={
                        <span className="truncate">
                          {APP_DISPLAY_NAME["claude-desktop"]}
                        </span>
                      }
                      version={
                        <span className="inline-flex items-center gap-1 text-caption text-fg-2">
                          {desktopStatus?.supported === false
                            ? t("appsPage.desktopUnsupported")
                            : desktopStatus?.configured
                              ? t("appsPage.desktopConnected")
                              : t("appsPage.desktopNotConnected")}
                          <HelpTip
                            title={APP_DISPLAY_NAME["claude-desktop"]}
                            align="end"
                          >
                            {t("appsPage.desktopHelp")}
                          </HelpTip>
                        </span>
                      }
                      visibility={visibilitySwitch("claude-desktop")}
                    />
                  );
                case "chatgpt-desktop":
                  return (
                    <ChatgptDesktopRow
                      key="chatgpt-desktop"
                      status={chatgptDesktop.data}
                      isPending={chatgptDesktop.isPending}
                      isError={chatgptDesktop.isError}
                      uninstalling={uninstallDesktop.isPending}
                      onUninstall={setConfirmDesktopUninstall}
                      visibility={visibilitySwitch(
                        "codex",
                        CHATGPT_DESKTOP_NAME,
                      )}
                    />
                  );
                case "tool":
                  return (
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
                      visibility={visibilitySwitch(
                        TOOL_APP_IDS[row.tool],
                        toolDisplayName(row.tool),
                      )}
                    />
                  );
              }
            })}
          </div>
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

      {confirmDesktopUninstall && (
        <ChatgptDesktopUninstallDialog
          command={confirmDesktopUninstall.command}
          cleanupOnly={confirmDesktopUninstall.cleanupOnly}
          onConfirm={() => {
            const cleanupOnly = confirmDesktopUninstall.cleanupOnly;
            setConfirmDesktopUninstall(null);
            uninstallDesktop.mutate(cleanupOnly);
          }}
          onCancel={() => setConfirmDesktopUninstall(null)}
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
  badge,
  name,
  meta,
  version,
  action,
  visibility,
  menu,
  footer,
}: {
  app: AppId;
  /** 换掉应用默认的角标（Codex CLI / ChatGPT Desktop 两行共用 OpenAI 图标，靠它区分） */
  badge?: AppGlyphBadge;
  name: React.ReactNode;
  meta?: React.ReactNode;
  version?: React.ReactNode;
  action?: React.ReactNode;
  visibility?: React.ReactNode;
  menu?: React.ReactNode;
  footer?: React.ReactNode;
}) {
  return (
    <div data-tool-row className="px-4 py-3">
      <div className={cn(APP_ROW_GRID, "min-h-10")}>
        <div className="flex min-w-0 items-center gap-4">
          <AppGlyph
            app={app}
            badge={badge}
            size={20}
            badgeClassName="bg-surface"
          />
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

/**
 * ChatGPT Desktop：和 Codex CLI 共用配置和侧栏入口，所以显示开关也是同一个设置，两行的开关一起开、一起关；
 * 不能在这里安装或升级。
 * 它随附的 Codex CLI 只列在这一行（Codex CLI 那一行只算单独安装的），卸载桌面版时一起卸掉。
 */
function ChatgptDesktopRow({
  status,
  isPending,
  isError,
  uninstalling,
  onUninstall,
  visibility,
}: {
  status: ChatGptDesktopStatus | undefined;
  isPending: boolean;
  isError: boolean;
  uninstalling: boolean;
  onUninstall: (confirmation: {
    cleanupOnly: boolean;
    command: string | null;
  }) => void;
  visibility: React.ReactNode;
}) {
  const { t } = useTranslation();
  const help = (
    <HelpTip title={CHATGPT_DESKTOP_NAME} align="end">
      {t("appsPage.chatgptDesktop.help")}
    </HelpTip>
  );
  const cleanupOnly = status?.cleanupRequired === true && !status.installed;

  const version = isPending ? (
    <span
      role="status"
      aria-label={t("common.loading")}
      className="inline-flex justify-end"
    >
      <Loader2 className="h-4 w-4 animate-spin text-fg-3" />
    </span>
  ) : status?.installed ? (
    <span className="inline-flex items-center gap-1">
      {status.appVersion ? (
        <span className="font-mono text-body font-medium tabular-nums text-fg-1">
          {status.appVersion}
        </span>
      ) : (
        <span className="text-caption text-fg-2">
          {t("appsPage.chatgptDesktop.installed")}
        </span>
      )}
      {help}
    </span>
  ) : (
    <span className="inline-flex items-center gap-1 text-caption text-fg-2">
      {isError || !status
        ? t("common.unknown")
        : status.supported
          ? t("common.notInstalled")
          : t("appsPage.desktopUnsupported")}
      {help}
    </span>
  );

  // 不截断：随附 CLI 的版本和「不能单独升级」是这一行要说清楚的事，窄的时候换行
  const meta = cleanupOnly ? (
    <span>{t("appsPage.chatgptDesktop.cleanupRequiredHint")}</span>
  ) : status?.installed ? (
    <span title={status.bundledCliPath ?? undefined}>
      {status.bundledCliVersion
        ? t("appsPage.chatgptDesktop.bundledCli", {
            version: status.bundledCliVersion,
          })
        : t("appsPage.chatgptDesktop.bundledCliUnknown")}
    </span>
  ) : status?.supported ? (
    <span>
      {t(
        isMac()
          ? "appsPage.chatgptDesktop.installHintMac"
          : "appsPage.chatgptDesktop.installHint",
      )}
    </span>
  ) : null;

  return (
    <AppRow
      app="codex"
      badge="desktop"
      name={<span className="truncate">{CHATGPT_DESKTOP_NAME}</span>}
      meta={meta}
      version={version}
      visibility={visibility}
      menu={
        status?.uninstallCommand || status?.cleanupRequired || uninstalling ? (
          <RowUninstallMenu
            name={CHATGPT_DESKTOP_NAME}
            disabled={uninstalling}
            busy={uninstalling}
            actionLabel={
              cleanupOnly
                ? t("appsPage.chatgptDesktop.cleanupAction")
                : undefined
            }
            onUninstall={() =>
              onUninstall({
                cleanupOnly,
                command: status?.uninstallCommand ?? null,
              })
            }
          />
        ) : null
      }
    />
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
  const canUninstall = Boolean(info?.version || broken || defaultInstall);
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
      badge={tool === "codex" ? "terminal" : undefined}
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
          <RowUninstallMenu
            name={toolDisplayName(tool)}
            disabled={isVersionLoading || isBusy}
            busy={busyAction === "uninstall"}
            onUninstall={onUninstall}
          />
        )
      }
      footer={footer}
    />
  );
}

/** 行尾的「⋯」菜单，里面是「卸载」；卸载进行中按钮转圈。 */
function RowUninstallMenu({
  name,
  disabled,
  busy,
  actionLabel,
  onUninstall,
}: {
  name: string;
  disabled: boolean;
  busy: boolean;
  actionLabel?: string;
  onUninstall: () => void;
}) {
  const { t } = useTranslation();
  return (
    <DropdownMenu>
      <HoverTip content={t("common.more")}>
        <DropdownMenuTrigger asChild>
          <Button
            variant="quiet"
            size="icon-compact"
            aria-label={t("appPage.moreActions", { name })}
            disabled={disabled}
            aria-busy={busy}
          >
            {busy ? (
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
          {actionLabel ?? t("settings.toolUninstall")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
