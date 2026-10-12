import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
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
} from "lucide-react";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import type { ToolInstallationReport } from "@/lib/api/settings";
import type { VisibleApps } from "@/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
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

const ROWS: Row[] = [
  { kind: "tool", tool: "claude" },
  { kind: "desktop" },
  ...TOOL_NAMES.filter((tool) => tool !== "claude").map(
    (tool): Row => ({ kind: "tool", tool }),
  ),
];

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
 * 「应用」页（v7）：每个应用一行 = 安装状态 / 版本 + 安装 / 升级 + 在侧栏显示的开关。
 * 隐藏一个应用只影响界面（侧栏、MCP / Skills 的列、提示词和会话的下拉），配置照常同步。
 */
export function AppsPage() {
  const { t, i18n } = useTranslation();
  const tools = useToolManagement();
  const { settings, updateSettings, autoSaveSettings } = useSettings();
  const [showCommands, setShowCommands] = useState(false);
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
            {(tools.updatableToolNames.length > 0 || tools.batchAction) && (
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
                {t("settings.updateAllTools", {
                  count: tools.updatableToolNames.length,
                })}
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
              {tools.isLoadingTools
                ? t("appsPage.checking")
                : t("appsPage.checkUpdates")}
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
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable"
      >
        <div className="px-6 pb-10 pt-4">
          <div className="flex h-8 items-center px-4 text-caption font-medium text-fg-2">
            <span className="flex-1">{t("appsPage.columnApp")}</span>
            <span className="w-[168px] text-end">
              {t("appsPage.columnVersion")}
            </span>
            <span className="w-[104px]" />
            <span className="flex w-[56px] items-center justify-end gap-0.5">
              {t("appsPage.columnVisible")}
              <HelpTip title={t("appsPage.columnVisible")} align="end">
                {t("appsPage.visibleHelp")}
              </HelpTip>
            </span>
          </div>

          <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
            {ROWS.map((row) =>
              row.kind === "desktop" ? (
                <AppRow
                  key="claude-desktop"
                  app="claude-desktop"
                  name={
                    <span className="flex items-center gap-1">
                      {APP_DISPLAY_NAME["claude-desktop"]}
                      <HelpTip title={APP_DISPLAY_NAME["claude-desktop"]}>
                        {t("appsPage.desktopHelp")}
                      </HelpTip>
                    </span>
                  }
                  version={
                    <span className="text-caption text-fg-2">
                      {desktopStatus?.supported === false
                        ? t("appsPage.desktopUnsupported")
                        : desktopStatus?.configured
                          ? t("appsPage.desktopConnected")
                          : t("appsPage.desktopNotConnected")}
                    </span>
                  }
                  visibility={visibilitySwitch("claude-desktop")}
                />
              ) : (
                <ToolRow
                  key={row.tool}
                  tool={row.tool}
                  tools={tools}
                  expanded={expanded[row.tool] ?? false}
                  onToggleExpanded={() =>
                    setExpanded((prev) => ({
                      ...prev,
                      [row.tool]: !prev[row.tool],
                    }))
                  }
                  visibility={visibilitySwitch(TOOL_APP_IDS[row.tool])}
                />
              ),
            )}
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
  footer,
}: {
  app: AppId;
  name: React.ReactNode;
  meta?: React.ReactNode;
  version?: React.ReactNode;
  action?: React.ReactNode;
  visibility: React.ReactNode;
  footer?: React.ReactNode;
}) {
  return (
    <div data-tool-row className="px-4 py-3">
      <div className="flex items-center gap-4">
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
        <div className="w-[168px] shrink-0 text-end">{version}</div>
        <div className="flex w-[104px] shrink-0 justify-end">{action}</div>
        <div className="flex w-[56px] shrink-0 justify-end">{visibility}</div>
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
  expanded,
  onToggleExpanded,
  visibility,
}: {
  tool: ToolName;
  tools: ReturnType<typeof useToolManagement>;
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
  const broken = Boolean(info?.installed_but_broken);
  const isBusy = tools.busyTools.has(tool);
  const action: ToolLifecycleAction | null =
    tools.busyTools.get(tool) ??
    (isVersionLoading || broken
      ? null
      : !info?.version
        ? "install"
        : isOutdated
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
      {isBusy ? (
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
    <span>{t(`appsPage.installHint.${tool}`, { defaultValue: "" })}</span>
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
    wslControls || errorLine || conflictList ? (
      <div className="space-y-2">
        {errorLine}
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
      footer={footer}
    />
  );
}
