import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  CheckCircle2,
  Copy,
  Database,
  Eye,
  FileClock,
  Info,
  ListChecks,
  RefreshCw,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ProviderIcon } from "@/components/ProviderIcon";
import { proxyApi } from "@/lib/api/proxy";
import { cn } from "@/lib/utils";
import type {
  CodexModelPickerUnlockResult,
  CodexHistorySessionDetailOutcome,
  CodexHistorySessionListOutcome,
  CodexHistorySessionSummary,
  CodexHistoryValueCount,
  CodexHistoryVisibilityRepairOutcome,
} from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";
import { SessionMessageItem } from "./SessionMessageItem";

const AUTO_TARGET = "__auto__";
const DEFAULT_SOURCE_FILTER = "all";

interface CodexHistoryRepairPanelProps {
  initialProjectPath?: string | null;
  showAutomationGuide?: boolean;
  onClose?: () => void;
  onRepairApplied?: () => void | Promise<void>;
}

/// 在会话管理页中承载 Codex Desktop 历史可见性修复、SQLite 列表和单条 JSONL 详情。
export function CodexHistoryRepairPanel({
  initialProjectPath,
  showAutomationGuide = false,
  onClose,
  onRepairApplied,
}: CodexHistoryRepairPanelProps) {
  const [codexHome, setCodexHome] = useState("");
  const [stateDbPath, setStateDbPath] = useState("");
  const [projectPath, setProjectPath] = useState("");
  const [limitToSingleProject, setLimitToSingleProject] = useState(false);
  const [targetProvider, setTargetProvider] = useState(AUTO_TARGET);
  const sourceFilter = DEFAULT_SOURCE_FILTER;
  const [includeArchived, setIncludeArchived] = useState(false);
  const [includeSubagents, setIncludeSubagents] = useState(false);
  const [historyQuery, setHistoryQuery] = useState("");
  const [historyList, setHistoryList] =
    useState<CodexHistorySessionListOutcome | null>(null);
  const [historyListError, setHistoryListError] = useState<string | null>(null);
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const [selectedSessionIds, setSelectedSessionIds] = useState<string[]>([]);
  const [activeHistoryId, setActiveHistoryId] = useState<string | null>(null);
  const [sessionDetail, setSessionDetail] =
    useState<CodexHistorySessionDetailOutcome | null>(null);
  const [sessionDetailError, setSessionDetailError] = useState<string | null>(
    null,
  );
  const [isLoadingDetail, setIsLoadingDetail] = useState(false);
  const [repairResult, setRepairResult] =
    useState<CodexHistoryVisibilityRepairOutcome | null>(null);
  const [repairError, setRepairError] = useState<string | null>(null);
  const [isPreviewingRepair, setIsPreviewingRepair] = useState(false);
  const [isApplyingRepair, setIsApplyingRepair] = useState(false);
  const [appRepairResult, setAppRepairResult] =
    useState<CodexModelPickerUnlockResult | null>(null);
  const [appRepairError, setAppRepairError] = useState<string | null>(null);
  const [isRepairingAppHistory, setIsRepairingAppHistory] = useState(false);
  const didAutoLoadRef = useRef(false);

  const normalizedCodexHome = codexHome.trim();
  const normalizedStateDbPath = stateDbPath.trim();
  const normalizedProjectPath = limitToSingleProject ? projectPath.trim() : "";
  const suggestedProjectPath = (initialProjectPath ?? "").trim();
  const canConfirmRepair = !isPreviewingRepair && !isApplyingRepair;
  const selectedSet = useMemo(
    () => new Set(selectedSessionIds),
    [selectedSessionIds],
  );
  const targetProviderOptions = useMemo(
    () => buildTargetProviderOptions(historyList),
    [historyList],
  );
  const selectedTargetProviderLabel =
    targetProvider === AUTO_TARGET
      ? autoTargetProviderLabel(historyList)
      : targetProvider;

  /// 修改输入后清掉旧结果，确保“确认修复”永远基于最新参数重新 dry-run。
  function invalidatePreview() {
    setRepairError(null);
    setRepairResult(null);
  }

  /// 安装新版 Codex App 兼容层，并调用 App 自己的本地线程目录同步服务。
  async function repairNewCodexAppHistory() {
    setIsRepairingAppHistory(true);
    setAppRepairError(null);
    try {
      const result = await proxyApi.unlockCodexModelPicker();
      setAppRepairResult(result);
      if (result.historySyncRequested) {
        toast.success("已触发新版 Codex App 原生历史目录重建");
      } else {
        toast.info(result.message);
      }
    } catch (error) {
      const message = extractErrorMessage(error) || String(error);
      setAppRepairError(message);
      toast.error(message);
    } finally {
      setIsRepairingAppHistory(false);
    }
  }

  /// 从后端 active SQLite 加载可修复会话摘要和 source/provider 分布。
  async function loadHistorySessions() {
    setIsLoadingHistory(true);
    setHistoryListError(null);
    try {
      const result = await proxyApi.listCodexHistorySessions({
        codexHome: normalizedCodexHome || null,
        stateDbPath: normalizedStateDbPath || null,
        projectPath: normalizedProjectPath || null,
        sourceFilter,
        query: historyQuery.trim() || null,
        limit: 120,
        includeArchived,
        includeSubagents,
      });
      setHistoryList(result);
      setSelectedSessionIds((current) => {
        const visibleIds = new Set(result.items.map((item) => item.id));
        return current.filter((id) => visibleIds.has(id));
      });
      if (!activeHistoryId && result.items[0]) {
        void openHistorySession(result.items[0]);
      }
    } catch (error) {
      setHistoryListError(extractErrorMessage(error) || String(error));
    } finally {
      setIsLoadingHistory(false);
    }
  }

  /// 读取单条历史的 JSONL 正文，供修复前确认内容。
  async function openHistorySession(session: CodexHistorySessionSummary) {
    setActiveHistoryId(session.id);
    setIsLoadingDetail(true);
    setSessionDetailError(null);
    try {
      const result = await proxyApi.readCodexHistorySession({
        codexHome: normalizedCodexHome || null,
        stateDbPath: normalizedStateDbPath || null,
        sessionId: session.id,
      });
      setSessionDetail(result);
    } catch (error) {
      setSessionDetail(null);
      setSessionDetailError(extractErrorMessage(error) || String(error));
    } finally {
      setIsLoadingDetail(false);
    }
  }

  /// 切换定向修复 session；未选择任何 session 时走 balanced recent-window 全局修复。
  function toggleHistorySession(sessionId: string) {
    setSelectedSessionIds((current) =>
      current.includes(sessionId)
        ? current.filter((id) => id !== sessionId)
        : [...current, sessionId],
    );
    invalidatePreview();
  }

  /// 手动把当前会话项目带入范围；默认保持空值，代表跨项目读取和修复。
  function applySuggestedProjectPath() {
    if (!suggestedProjectPath) return;
    setLimitToSingleProject(true);
    setProjectPath(suggestedProjectPath);
    invalidatePreview();
  }

  /// 切换项目范围；关闭时保留输入值但不参与本次查询或写入。
  function toggleProjectLimit(checked: boolean) {
    setLimitToSingleProject(checked);
    if (checked && !projectPath.trim() && suggestedProjectPath) {
      setProjectPath(suggestedProjectPath);
    }
    invalidatePreview();
  }

  /// 选择当前加载页的全部 session，适合一次性拉回搜索结果。
  function selectAllLoadedSessions() {
    setSelectedSessionIds(historyList?.items.map((item) => item.id) ?? []);
    invalidatePreview();
  }

  /// 调用后端历史修复命令，dry-run 和 apply 共用同一组参数。
  async function runHistoryRepair(
    dryRun: boolean,
  ): Promise<CodexHistoryVisibilityRepairOutcome | null> {
    if (dryRun) {
      setIsPreviewingRepair(true);
    } else {
      setIsApplyingRepair(true);
    }
    setRepairError(null);
    try {
      const result = await proxyApi.repairCodexHistoryVisibility({
        dryRun,
        codexHome: normalizedCodexHome || null,
        stateDbPath: normalizedStateDbPath || null,
        projectPath: normalizedProjectPath || null,
        targetProvider: targetProvider === AUTO_TARGET ? null : targetProvider,
        sessionIds: selectedSessionIds.length > 0 ? selectedSessionIds : null,
        count: 30,
        windowLimit: 80,
        balanceRecentWindow: true,
        maxPerProject: 10,
        maxTotal: 300,
        sourceFilter,
        includeArchived,
        includeSubagents,
      });
      setRepairResult(result);
      if (!dryRun) {
        try {
          await onRepairApplied?.();
        } catch (callbackError) {
          toast.error(
            `历史修复已完成，但后续引导失败：${extractErrorMessage(callbackError) || String(callbackError)}`,
          );
        }
      }
      return result;
    } catch (error) {
      const message = historyRepairErrorMessage(
        extractErrorMessage(error) || String(error),
      );
      setRepairError(message);
      toast.error(message);
      return null;
    } finally {
      setIsPreviewingRepair(false);
      setIsApplyingRepair(false);
    }
  }

  /// 一个按钮完成预览、确认和写入，避免把 dry-run/apply 暴露成两套用户流程。
  async function confirmAndRepairHistory() {
    if (!canConfirmRepair) return;
    if (limitToSingleProject && !normalizedProjectPath) {
      const message = "已勾选只修复单个项目，请先填写项目路径。";
      setRepairError(message);
      toast.error(message);
      return;
    }
    const preview = await runHistoryRepair(true);
    if (!preview) return;
    const confirmed = window.confirm(
      [
        "确认修复前，请先完全退出 Codex / ChatGPT App。",
        "如果 App 还开着，CCSwitchMulti 会拒绝写入，避免运行中的 app-server 覆盖修复结果。",
        "",
        `active DB: ${preview.stateDbPath ?? "未找到"}`,
        `目标 provider: ${preview.targetProvider}`,
        `范围: ${repairScopeLabel}`,
        `已选 session: ${selectedSessionIds.length}`,
        `rollout 发现: ${preview.rolloutThreadsDiscovered}`,
        `新增 thread-store 行: ${preview.threadRowsToInsert}`,
        `重建索引元数据: ${preview.threadMetadataRowsToRebuild}`,
        `backfill 状态: ${preview.backfillStateToUpdate ? "将更新为 complete" : "当前 DB 无此表"}`,
        `provider rows: ${preview.providerRowsToUpdate}`,
        `session_index append: ${preview.sessionIndexMissingToAppend}`,
        `recent rows: ${preview.balancedRecentWindowRows}`,
        `rollout mtimes: ${preview.rolloutMtimesToTouch}`,
        "",
        "写入成功后重新打开 Codex；新版 App 会从 active DB 重建侧边栏目录。若侧边栏仍未刷新，再使用“启动并刷新新版目录”。",
        "",
        "确认现在写入吗？",
      ].join("\n"),
    );
    if (!confirmed) return;
    const applied = await runHistoryRepair(false);
    if (applied) {
      toast.success("历史修复已写入。请重新启动 Codex / ChatGPT App。");
    }
  }

  /// 复制会话正文或路径到剪贴板。
  async function copyText(text: string, message: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(message);
    } catch (error) {
      toast.error(extractErrorMessage(error) || "复制失败");
    }
  }

  /// 首次打开修复主工作区时自动执行只读加载，避免默认画面停在空状态。
  useEffect(() => {
    if (didAutoLoadRef.current) return;
    if (!canUseTauriInvoke()) return;
    didAutoLoadRef.current = true;
    void loadHistorySessions();
  }, []);

  const repairScopeLabel =
    selectedSessionIds.length > 0
      ? `定向修复 ${selectedSessionIds.length} 个 session`
      : limitToSingleProject && normalizedProjectPath
        ? "只修复单个项目"
        : "修复所有项目";
  const activeDbLabel =
    historyList?.stateDbPath ||
    normalizedStateDbPath ||
    "~/.codex/state_5.sqlite";

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-lg border bg-card">
      <div className="shrink-0 border-b px-4 py-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2 text-base font-semibold">
              <ProviderIcon icon="openai" name="Codex" size={20} />
              <span>Codex 历史修复</span>
              <Badge variant="secondary">Desktop history</Badge>
              <Badge variant="outline">
                {includeSubagents ? "含 subagent" : "主线程"}
              </Badge>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span>{repairScopeLabel}</span>
              <span>目标：{selectedTargetProviderLabel}</span>
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={loadHistorySessions}
              disabled={isLoadingHistory}
              className="gap-2"
            >
              {isLoadingHistory ? (
                <RefreshCw className="size-4 animate-spin" />
              ) : (
                <Database className="size-4" />
              )}
              刷新记录
            </Button>
            <Button
              size="sm"
              onClick={confirmAndRepairHistory}
              disabled={!canConfirmRepair}
              className="gap-2"
            >
              {isPreviewingRepair || isApplyingRepair ? (
                <RefreshCw className="size-4 animate-spin" />
              ) : (
                <ShieldCheck className="size-4" />
              )}
              确认修复
            </Button>
            {onClose ? (
              <Button size="sm" variant="ghost" onClick={onClose}>
                <X className="size-4" />
                会话浏览
              </Button>
            ) : null}
          </div>
        </div>

        <div className="mt-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-3 text-sm text-amber-950 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-50">
          <div className="flex items-start gap-2">
            <ListChecks className="mt-0.5 size-4 shrink-0" />
            <div className="min-w-0 space-y-2">
              <div className="font-medium">
                {showAutomationGuide
                  ? "MultiRouter 已配置成功，修复前先完全退出 Codex / ChatGPT App"
                  : "修复前先完全退出 Codex / ChatGPT App"}
              </div>
              <p className="text-xs leading-5">
                选择记录、项目范围和目标 provider 后点击
                <span className="font-medium"> 确认修复</span>。写入会修改
                active DB、索引和 rollout 元数据；如果 Codex
                仍在运行，后端会拒绝写入。 写入成功后重新打开
                Codex；若新版侧边栏仍未刷新，再到高级设置里使用
                <span className="font-medium"> 启动并刷新新版目录</span>。
              </p>
            </div>
          </div>
        </div>

        {appRepairResult || appRepairError ? (
          <div
            className={cn(
              "mt-3 rounded-md border px-3 py-2 text-xs",
              appRepairError
                ? "border-destructive/40 bg-destructive/10 text-destructive"
                : appRepairResult?.historySyncRequested
                  ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-800 dark:text-emerald-200"
                  : "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-200",
            )}
          >
            {appRepairError ?? appRepairResult?.message}
          </div>
        ) : null}

        <div className="mt-3 grid gap-2 md:grid-cols-3">
          <RepairStatusTile
            icon={<Database className="size-4" />}
            label="Active DB"
            value={activeDbLabel}
          />
          <RepairStatusTile
            icon={<ListChecks className="size-4" />}
            label="已加载 / 已选"
            value={`${historyList?.totalMatched ?? 0} / ${selectedSessionIds.length}`}
          />
          <RepairStatusTile
            icon={<ShieldCheck className="size-4" />}
            label="目标 provider"
            value={selectedTargetProviderLabel}
          />
        </div>
      </div>

      <div className="shrink-0 border-b bg-muted/20 px-4 py-3">
        <RepairSettings
          codexHome={codexHome}
          stateDbPath={stateDbPath}
          projectPath={projectPath}
          limitToSingleProject={limitToSingleProject}
          suggestedProjectPath={suggestedProjectPath}
          targetProvider={targetProvider}
          targetProviderOptions={targetProviderOptions}
          includeArchived={includeArchived}
          includeSubagents={includeSubagents}
          historyList={historyList}
          onCodexHomeChange={(value) => {
            setCodexHome(value);
            invalidatePreview();
          }}
          onStateDbPathChange={(value) => {
            setStateDbPath(value);
            invalidatePreview();
          }}
          onProjectPathChange={(value) => {
            setProjectPath(value);
            invalidatePreview();
          }}
          onLimitToSingleProjectChange={toggleProjectLimit}
          onUseSuggestedProjectPath={applySuggestedProjectPath}
          onTargetProviderChange={(value) => {
            setTargetProvider(value);
            invalidatePreview();
          }}
          onIncludeArchivedChange={(checked) => {
            setIncludeArchived(checked);
            invalidatePreview();
          }}
          onIncludeSubagentsChange={(checked) => {
            setIncludeSubagents(checked);
            invalidatePreview();
          }}
          isRepairingAppHistory={isRepairingAppHistory}
          onRepairNewCodexAppHistory={repairNewCodexAppHistory}
        />
      </div>

      <div className="grid min-h-0 flex-1 xl:grid-cols-[400px_minmax(0,1fr)]">
        <div className="flex min-h-0 flex-col border-r">
          <div className="border-b px-3 py-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-sm font-semibold">
                <FileClock className="size-4" />
                SQLite 历史
                <Badge variant="secondary">
                  {historyList?.totalMatched ?? 0}
                </Badge>
              </div>
              <Badge variant="outline">已选 {selectedSessionIds.length}</Badge>
            </div>
            <div className="mt-2 flex gap-2">
              <div className="relative min-w-0 flex-1">
                <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={historyQuery}
                  onChange={(event) => setHistoryQuery(event.target.value)}
                  placeholder="搜索标题、路径、provider 或 session id"
                  className="h-8 pl-8"
                />
              </div>
              <Button
                size="sm"
                variant="outline"
                onClick={loadHistorySessions}
                disabled={isLoadingHistory}
              >
                <RefreshCw
                  className={cn("size-3.5", isLoadingHistory && "animate-spin")}
                />
              </Button>
            </div>
            <div className="mt-2 flex flex-wrap gap-2">
              <Button
                size="sm"
                variant="outline"
                onClick={selectAllLoadedSessions}
                disabled={!historyList?.items.length}
              >
                全选本页
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setSelectedSessionIds([]);
                  invalidatePreview();
                }}
                disabled={selectedSessionIds.length === 0}
              >
                清空选择
              </Button>
            </div>
          </div>

          {historyListError ? (
            <div className="m-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive">
              加载失败：{historyListError}
            </div>
          ) : null}

          <ScrollArea className="min-h-0 flex-1">
            {historyList?.items.length ? (
              <div className="divide-y">
                {historyList.items.map((session) => (
                  <HistoryRepairSessionRow
                    key={session.id}
                    session={session}
                    selected={selectedSet.has(session.id)}
                    active={activeHistoryId === session.id}
                    onToggle={() => toggleHistorySession(session.id)}
                    onOpen={() => void openHistorySession(session)}
                  />
                ))}
              </div>
            ) : (
              <div className="flex min-h-[220px] flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground">
                <Database className="size-8 opacity-40" />
                <span>
                  {historyList
                    ? "没有匹配的历史记录。"
                    : isLoadingHistory
                      ? "正在加载 active SQLite..."
                      : "加载 active SQLite 后选择需要修复的 session。"}
                </span>
              </div>
            )}
          </ScrollArea>
        </div>

        <div className="grid min-h-0 bg-background 2xl:grid-cols-[minmax(0,1fr)_340px]">
          <HistorySessionDetail
            detail={sessionDetail}
            error={sessionDetailError}
            isLoading={isLoadingDetail}
            onCopy={copyText}
          />
          <RepairResultPanel
            result={repairResult}
            error={repairError}
            sourceCounts={historyList?.sourceCounts ?? []}
            providerCounts={historyList?.providerCounts ?? []}
          />
        </div>
      </div>
    </div>
  );
}

interface RepairSettingsProps {
  codexHome: string;
  stateDbPath: string;
  projectPath: string;
  limitToSingleProject: boolean;
  suggestedProjectPath: string;
  targetProvider: string;
  targetProviderOptions: string[];
  includeArchived: boolean;
  includeSubagents: boolean;
  historyList: CodexHistorySessionListOutcome | null;
  onCodexHomeChange: (value: string) => void;
  onStateDbPathChange: (value: string) => void;
  onProjectPathChange: (value: string) => void;
  onLimitToSingleProjectChange: (checked: boolean) => void;
  onUseSuggestedProjectPath: () => void;
  onTargetProviderChange: (value: string) => void;
  onIncludeArchivedChange: (checked: boolean) => void;
  onIncludeSubagentsChange: (checked: boolean) => void;
  isRepairingAppHistory: boolean;
  onRepairNewCodexAppHistory: () => void;
}

interface RepairStatusTileProps {
  icon: ReactNode;
  label: string;
  value: string;
}

/// 渲染修复工作台顶部状态块，用短文本展示当前 DB、选择数量和写入锁定状态。
function RepairStatusTile({ icon, label, value }: RepairStatusTileProps) {
  return (
    <div className="min-w-0 rounded-md border bg-muted/25 px-3 py-2">
      <div className="flex items-center gap-2 text-[11px] font-medium uppercase text-muted-foreground">
        {icon}
        <span>{label}</span>
      </div>
      <div
        className="mt-1 truncate text-xs font-medium text-foreground"
        title={value}
      >
        {value}
      </div>
    </div>
  );
}

/// 渲染修复参数区，浅色默认值说明实际会读写的位置。
function RepairSettings({
  codexHome,
  stateDbPath,
  projectPath,
  limitToSingleProject,
  suggestedProjectPath,
  targetProvider,
  targetProviderOptions,
  includeArchived,
  includeSubagents,
  historyList,
  onCodexHomeChange,
  onStateDbPathChange,
  onProjectPathChange,
  onLimitToSingleProjectChange,
  onUseSuggestedProjectPath,
  onTargetProviderChange,
  onIncludeArchivedChange,
  onIncludeSubagentsChange,
  isRepairingAppHistory,
  onRepairNewCodexAppHistory,
}: RepairSettingsProps) {
  const providerCountsByValue = useMemo(
    () =>
      new Map(
        (historyList?.providerCounts ?? [])
          .filter((row) => row.value)
          .map((row) => [row.value as string, row.count]),
      ),
    [historyList?.providerCounts],
  );

  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-2 text-sm font-semibold">
        <SlidersHorizontal className="size-4" />
        修复范围
      </div>
      <div className="grid gap-3 xl:grid-cols-[minmax(0,1.35fr)_320px]">
        <div className="rounded-md border bg-background/70 p-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <ToggleLine
              checked={limitToSingleProject}
              label="只修复单个项目"
              onChange={onLimitToSingleProjectChange}
            />
            {suggestedProjectPath && limitToSingleProject ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={onUseSuggestedProjectPath}
              >
                带入当前项目
              </Button>
            ) : null}
          </div>
          <Input
            value={projectPath}
            onChange={(event) => onProjectPathChange(event.target.value)}
            placeholder="勾选后填写项目路径；不勾选则修复所有项目"
            disabled={!limitToSingleProject}
            className="mt-2 h-9"
          />
          <div className="mt-2 rounded-md bg-muted/60 px-2 py-1 text-[11px] text-muted-foreground">
            {limitToSingleProject && projectPath.trim()
              ? "当前只读取和修复这个项目路径。"
              : limitToSingleProject
                ? "已限制单个项目，请填写路径或带入当前项目。"
                : "未限制项目；会跨项目读取并修复所有匹配记录。"}
          </div>
        </div>
        <label className="text-xs font-medium xl:self-start">
          修复到 provider 桶
          <Select value={targetProvider} onValueChange={onTargetProviderChange}>
            <SelectTrigger className="mt-1 h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={AUTO_TARGET}>
                {autoTargetProviderLabel(historyList)}
              </SelectItem>
              {targetProviderOptions.map((provider) => (
                <SelectItem key={provider} value={provider}>
                  {targetProviderOptionLabel(
                    provider,
                    historyList,
                    providerCountsByValue,
                  )}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <div className="mt-1 rounded-md bg-muted/60 px-2 py-1 font-mono text-[11px] text-muted-foreground">
            live: {historyList?.liveConfigModelProvider ?? "加载后显示"}
          </div>
        </label>
      </div>

      <details className="rounded-md border bg-muted/20">
        <summary className="cursor-pointer px-3 py-2 text-xs font-medium text-muted-foreground">
          高级设置和新版目录兜底
        </summary>
        <div className="grid gap-3 border-t p-3 lg:grid-cols-2 2xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)_220px_240px]">
          <LabeledInput
            label="Codex 目录"
            value={codexHome}
            placeholder="默认 ~/.codex"
            hint={
              historyList?.codexHome ?? "将自动解析为当前用户的 .codex 目录"
            }
            onChange={onCodexHomeChange}
          />
          <LabeledInput
            label="Active DB"
            value={stateDbPath}
            placeholder="默认 ~/.codex/state_5.sqlite"
            hint={
              historyList?.stateDbPath ??
              "自动识别 sqlite_home / CODEX_SQLITE_HOME；默认优先 ~/.codex/state_5.sqlite"
            }
            onChange={onStateDbPathChange}
          />
          <div className="grid gap-2 text-xs">
            <ToggleLine
              checked={includeArchived}
              label="包含 archived"
              onChange={onIncludeArchivedChange}
            />
            <ToggleLine
              checked={includeSubagents}
              label="包含 subagent thread_source"
              onChange={onIncludeSubagentsChange}
            />
          </div>
          <div className="grid gap-2 text-xs text-muted-foreground">
            <Button
              size="sm"
              variant="outline"
              onClick={onRepairNewCodexAppHistory}
              disabled={isRepairingAppHistory}
              className="w-full gap-2"
            >
              {isRepairingAppHistory ? (
                <RefreshCw className="size-4 animate-spin" />
              ) : (
                <Eye className="size-4" />
              )}
              启动并刷新新版目录
            </Button>
            <div>
              仅在确认修复写入后，新版 Codex 侧边栏仍没有从 active DB
              重建时使用。
            </div>
          </div>
        </div>
      </details>
    </div>
  );
}

interface LabeledInputProps {
  label: string;
  value: string;
  placeholder: string;
  hint: string;
  disabled?: boolean;
  actionLabel?: string;
  onAction?: () => void;
  onChange: (value: string) => void;
}

/// 渲染带浅底默认提示的输入框，避免把空值误解为未配置。
function LabeledInput({
  label,
  value,
  placeholder,
  hint,
  disabled = false,
  actionLabel,
  onAction,
  onChange,
}: LabeledInputProps) {
  return (
    <label className="text-xs font-medium">
      <span className="flex items-center justify-between gap-2">
        <span>{label}</span>
        {actionLabel && onAction ? (
          <button
            type="button"
            className="text-[11px] font-medium text-primary hover:underline"
            onClick={(event) => {
              event.preventDefault();
              onAction();
            }}
          >
            {actionLabel}
          </button>
        ) : null}
      </span>
      <Input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        className="mt-1 h-9"
      />
      <div className="mt-1 truncate rounded-md bg-muted/60 px-2 py-1 font-mono text-[11px] text-muted-foreground">
        {hint}
      </div>
    </label>
  );
}

interface ToggleLineProps {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}

/// 渲染修复范围开关，和删除批量选择状态保持独立。
function ToggleLine({ checked, label, onChange }: ToggleLineProps) {
  return (
    <label className="flex h-8 items-center gap-2 rounded-md border bg-background/60 px-2">
      <Checkbox
        checked={checked}
        onCheckedChange={(value) => onChange(Boolean(value))}
      />
      <span>{label}</span>
    </label>
  );
}

interface HistoryRepairSessionRowProps {
  session: CodexHistorySessionSummary;
  selected: boolean;
  active: boolean;
  onToggle: () => void;
  onOpen: () => void;
}

/// 渲染单条 SQLite 历史候选；勾选用于修复，点正文区域用于预览内容。
function HistoryRepairSessionRow({
  session,
  selected,
  active,
  onToggle,
  onOpen,
}: HistoryRepairSessionRowProps) {
  return (
    <div
      className={cn(
        "grid grid-cols-[28px_1fr] gap-2 px-3 py-2 text-xs transition",
        active ? "bg-primary/10" : "hover:bg-muted/50",
      )}
    >
      <Checkbox
        checked={selected}
        aria-label={`选择 ${session.title || session.id}`}
        onCheckedChange={() => onToggle()}
        className="mt-1"
      />
      <button type="button" onClick={onOpen} className="min-w-0 text-left">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium">
            {session.title || session.id}
          </span>
          {session.hasUserEvent ? (
            <CheckCircle2 className="size-3.5 shrink-0 text-emerald-500" />
          ) : null}
        </div>
        <div className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
          {session.cwd ?? "no cwd"}
        </div>
        <div className="mt-1 flex flex-wrap gap-1 text-[11px] text-muted-foreground">
          <Badge variant="outline">{session.modelProvider ?? "-"}</Badge>
          <Badge variant="outline">
            source={compactSource(session.source)}
          </Badge>
          <span>{formatHistorySessionTime(session.updatedAt)}</span>
        </div>
      </button>
    </div>
  );
}

interface HistorySessionDetailProps {
  detail: CodexHistorySessionDetailOutcome | null;
  error: string | null;
  isLoading: boolean;
  onCopy: (text: string, message: string) => void;
}

/// 展示单条修复候选的 JSONL 正文，便于写入前核对 session 内容。
function HistorySessionDetail({
  detail,
  error,
  isLoading,
  onCopy,
}: HistorySessionDetailProps) {
  const session = detail?.session;
  return (
    <div className="flex min-h-0 flex-col bg-card">
      <div className="flex flex-wrap items-start justify-between gap-2 border-b px-3 py-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Eye className="size-4" />
            Session 内容
          </div>
          <div className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
            {session?.id ?? "未选择 session"}
          </div>
        </div>
        {detail?.rolloutPath ? (
          <Button
            size="sm"
            variant="outline"
            className="gap-2"
            onClick={() => onCopy(detail.rolloutPath!, "已复制 rollout 路径")}
          >
            <Copy className="size-3.5" />
            路径
          </Button>
        ) : null}
      </div>

      {isLoading ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
          <RefreshCw className="size-4 animate-spin" />
          加载会话内容中...
        </div>
      ) : error ? (
        <div className="m-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive">
          读取失败：{error}
        </div>
      ) : detail?.skippedReason ? (
        <div className="m-3 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-200">
          {detail.skippedReason}
        </div>
      ) : detail?.messages.length ? (
        <ScrollArea className="min-h-0 flex-1">
          <div className="space-y-3 p-3">
            {detail.messages.map((message, index) => (
              <SessionMessageItem
                key={`${index}-${message.role}-${message.ts ?? "no-ts"}`}
                message={message}
                isActive={false}
                onCopy={(content) => onCopy(content, "已复制消息内容")}
              />
            ))}
          </div>
        </ScrollArea>
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-8 text-center text-sm text-muted-foreground">
          <Eye className="size-9 opacity-35" />
          <span>选择左侧 session 后查看本地 JSONL 内容。</span>
        </div>
      )}
    </div>
  );
}

interface RepairResultPanelProps {
  result: CodexHistoryVisibilityRepairOutcome | null;
  error: string | null;
  sourceCounts: CodexHistoryValueCount[];
  providerCounts: CodexHistoryValueCount[];
}

/// 展示 dry-run/apply 证据和当前 DB 的 source/provider 分布。
function RepairResultPanel({
  result,
  error,
  sourceCounts,
  providerCounts,
}: RepairResultPanelProps) {
  return (
    <div className="flex min-h-0 flex-col border-t bg-card 2xl:border-t-0 2xl:border-l">
      <div className="border-b px-3 py-2">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <Info className="size-4" />
          修复结果
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-3 p-3">
          {error ? (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive">
              {error}
            </div>
          ) : result ? (
            <div className="space-y-3">
              <div className="flex flex-wrap items-center gap-2">
                <Badge>{result.dryRun ? "预览" : "已写入"}</Badge>
                <Badge variant="outline">target={result.targetProvider}</Badge>
                <Badge variant="outline">
                  source={result.sourceFilter || "all"}
                </Badge>
              </div>
              <div className="grid grid-cols-2 gap-2">
                <RepairMetric
                  label="provider"
                  value={result.providerRowsToUpdate}
                />
                <RepairMetric
                  label="user-event"
                  value={result.userEventRowsToUpdate}
                />
                <RepairMetric
                  label="index append"
                  value={result.sessionIndexMissingToAppend}
                />
                <RepairMetric label="focus" value={result.focusSelectedCount} />
                <RepairMetric
                  label="balanced"
                  value={result.balancedRecentWindowRows}
                />
                <RepairMetric
                  label="rollout mtime"
                  value={result.rolloutMtimesToTouch}
                />
              </div>
              <div className="space-y-1 rounded-md bg-muted/60 p-2 font-mono text-[11px] text-muted-foreground">
                <div className="truncate">db={result.stateDbPath ?? "-"}</div>
                <div className="truncate">
                  live={result.liveConfigModelProvider ?? "-"}
                </div>
                <div className="truncate">
                  backup={result.backupDir ?? "写入后显示"}
                </div>
              </div>
            </div>
          ) : (
            <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
              选择范围和目标 provider 后，点击“确认修复”。
            </div>
          )}

          <ValueCountPanel title="source 分布" rows={sourceCounts} />
          <ValueCountPanel title="provider 桶分布" rows={providerCounts} />
        </div>
      </ScrollArea>
    </div>
  );
}

interface RepairMetricProps {
  label: string;
  value: number;
}

/// 渲染单个 dry-run 指标，保持结果区紧凑。
function RepairMetric({ label, value }: RepairMetricProps) {
  return (
    <div className="rounded-md border bg-muted/30 px-2 py-1.5">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <div className="text-sm font-semibold">{value.toLocaleString()}</div>
    </div>
  );
}

interface ValueCountPanelProps {
  title: string;
  rows: CodexHistoryValueCount[];
}

/// 渲染 active SQLite 字段分布，帮助判断 source 和 provider 桶的真实含义。
function ValueCountPanel({ title, rows }: ValueCountPanelProps) {
  return (
    <div className="rounded-md border bg-muted/20 p-3">
      <div className="mb-2 text-sm font-semibold">{title}</div>
      {rows.length ? (
        <div className="space-y-1">
          {rows.slice(0, 8).map((row) => (
            <div
              key={`${title}-${row.value ?? "null"}`}
              className="flex items-center justify-between gap-2 text-xs"
            >
              <span className="min-w-0 truncate font-mono">
                {compactSource(row.value) || "(null)"}
              </span>
              <Badge variant="secondary">{row.count}</Badge>
            </div>
          ))}
        </div>
      ) : (
        <div className="text-xs text-muted-foreground">加载后显示</div>
      )}
    </div>
  );
}

/// 生成 provider 下拉候选并去重，避免把自动项和真实 provider 混在一起。
function buildTargetProviderOptions(
  historyList: CodexHistorySessionListOutcome | null,
): string[] {
  const values = [
    historyList?.liveConfigModelProvider,
    ...(historyList?.targetProviderCandidates ?? []),
    ...(historyList?.providerCounts ?? []).map((row) => row.value),
    "openai",
    "custom",
    "codex_model_router_v2",
  ];
  const output: string[] = [];
  for (const value of values) {
    const normalized = value?.trim();
    if (!normalized || normalized === AUTO_TARGET) continue;
    if (!output.includes(normalized)) output.push(normalized);
  }
  return output;
}

/// 生成自动目标项文案，说明 apply 时会把 null 交给后端按 live config 解析。
function autoTargetProviderLabel(
  historyList: CodexHistorySessionListOutcome | null,
): string {
  const live = historyList?.liveConfigModelProvider?.trim();
  return live
    ? `当前 provider：${live}`
    : "当前 provider：live config 或 codex_model_router_v2";
}

/// 为 provider 候选追加来源和计数，避免用户误以为下拉只读了当前项目。
function targetProviderOptionLabel(
  provider: string,
  historyList: CodexHistorySessionListOutcome | null,
  providerCountsByValue: Map<string, number>,
): string {
  const badges = [];
  const count = providerCountsByValue.get(provider);
  if (provider === historyList?.liveConfigModelProvider) badges.push("live");
  if (typeof count === "number") badges.push(`${count} 条`);
  if (provider === "codex_model_router_v2" && typeof count !== "number") {
    badges.push("稳定 MultiRouter 桶");
  }
  return badges.length ? `${provider} (${badges.join(" / ")})` : provider;
}

/// 把后端并发保护错误改写成用户可执行的关 App 指引。
function historyRepairErrorMessage(message: string): string {
  const text = message.trim();
  if (
    /Codex|ChatGPT|app-server|running|进程|运行/i.test(text) &&
    !text.includes("完全退出")
  ) {
    return `${text}\n请完全退出 Codex / ChatGPT App 后再点“确认修复”；写入成功后重新打开 Codex 等新版目录重建。`;
  }
  return text;
}

/// 检测当前是否运行在 Tauri 环境，避免浏览器预览时自动触发后端 invoke 错误。
function canUseTauriInvoke(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean(
    (window as unknown as { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__,
  );
}

/// 格式化历史时间，异常时保留原始字符串便于排查。
function formatHistorySessionTime(value: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/// 压缩 subagent JSON source，避免长 JSON 撑破列表。
function compactSource(value: string | null): string {
  if (!value) return "";
  if (value.startsWith("{") && value.includes("subagent")) return "subagent";
  return value;
}
