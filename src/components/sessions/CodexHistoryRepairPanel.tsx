import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
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
  CodexHistorySessionDetailOutcome,
  CodexHistorySessionListOutcome,
  CodexHistorySessionSummary,
  CodexHistoryValueCount,
  CodexHistoryVisibilityRepairOutcome,
} from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";
import { SessionMessageItem } from "./SessionMessageItem";

const AUTO_TARGET = "__auto__";
const DEFAULT_SOURCE_FILTER = "vscode";

interface SourceOption {
  value: string;
  label: string;
  detail: string;
}

interface CodexHistoryRepairPanelProps {
  initialProjectPath?: string | null;
  onClose?: () => void;
}

/// 在会话管理页中承载 Codex Desktop 历史可见性修复、SQLite 列表和单条 JSONL 详情。
export function CodexHistoryRepairPanel({
  initialProjectPath,
  onClose,
}: CodexHistoryRepairPanelProps) {
  const { t } = useTranslation();
  const [codexHome, setCodexHome] = useState("");
  const [stateDbPath, setStateDbPath] = useState("");
  const [projectPath, setProjectPath] = useState(initialProjectPath ?? "");
  const [targetProvider, setTargetProvider] = useState(AUTO_TARGET);
  const [sourceFilter, setSourceFilter] = useState(DEFAULT_SOURCE_FILTER);
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
  const [lastPreviewKey, setLastPreviewKey] = useState<string | null>(null);
  const [repairResult, setRepairResult] =
    useState<CodexHistoryVisibilityRepairOutcome | null>(null);
  const [repairError, setRepairError] = useState<string | null>(null);
  const [isPreviewingRepair, setIsPreviewingRepair] = useState(false);
  const [isApplyingRepair, setIsApplyingRepair] = useState(false);
  const didAutoLoadRef = useRef(false);

  const normalizedCodexHome = codexHome.trim();
  const normalizedStateDbPath = stateDbPath.trim();
  const normalizedProjectPath = projectPath.trim();
  const selectedSessionKey = useMemo(
    () => [...selectedSessionIds].sort().join("|"),
    [selectedSessionIds],
  );
  const currentRepairKey = useMemo(
    () =>
      JSON.stringify({
        codexHome: normalizedCodexHome,
        stateDbPath: normalizedStateDbPath,
        projectPath: normalizedProjectPath,
        targetProvider,
        sourceFilter,
        includeArchived,
        includeSubagents,
        sessions: selectedSessionKey,
      }),
    [
      includeArchived,
      includeSubagents,
      normalizedCodexHome,
      normalizedProjectPath,
      normalizedStateDbPath,
      selectedSessionKey,
      sourceFilter,
      targetProvider,
    ],
  );
  const canApplyRepair = Boolean(
    repairResult?.dryRun &&
      lastPreviewKey === currentRepairKey &&
      !isPreviewingRepair &&
      !isApplyingRepair,
  );
  const selectedSet = useMemo(
    () => new Set(selectedSessionIds),
    [selectedSessionIds],
  );
  const targetProviderOptions = useMemo(
    () => buildTargetProviderOptions(historyList),
    [historyList],
  );

  /// 修改输入后清掉旧 dry-run 锁，避免用户把过期预览直接写入。
  function invalidatePreview() {
    setLastPreviewKey(null);
    setRepairError(null);
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

  /// 选择当前加载页的全部 session，适合一次性拉回搜索结果。
  function selectAllLoadedSessions() {
    setSelectedSessionIds(historyList?.items.map((item) => item.id) ?? []);
    invalidatePreview();
  }

  /// 调用后端历史修复命令，dry-run 和 apply 共用同一组参数。
  async function runHistoryRepair(dryRun: boolean) {
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
      if (dryRun) {
        setLastPreviewKey(currentRepairKey);
      }
    } catch (error) {
      setRepairError(extractErrorMessage(error) || String(error));
    } finally {
      setIsPreviewingRepair(false);
      setIsApplyingRepair(false);
    }
  }

  /// 在写入前展示关键计数，确认后才执行真实修复。
  async function applyHistoryRepair() {
    if (!canApplyRepair || !repairResult) {
      setRepairError(t("codexHistoryRepair.previewRequired"));
      return;
    }
    const confirmed = window.confirm(
      [
        t("codexHistoryRepair.confirmApplyIntro"),
        "",
        t("codexHistoryRepair.confirmApplyActiveDb", {
          db: repairResult.stateDbPath ?? t("common.notSet"),
        }),
        t("codexHistoryRepair.confirmApplyTarget", {
          provider: repairResult.targetProvider,
        }),
        t("codexHistoryRepair.confirmApplyCodexHome", {
          path: repairResult.codexHome,
        }),
        t("codexHistoryRepair.confirmApplySelected", {
          count: selectedSessionIds.length,
        }),
        `provider rows: ${repairResult.providerRowsToUpdate}`,
        `session_index append: ${repairResult.sessionIndexMissingToAppend}`,
        `balanced rows: ${repairResult.balancedRecentWindowRows}`,
        `rollout mtimes: ${repairResult.rolloutMtimesToTouch}`,
        "",
        t("codexHistoryRepair.confirmApplyContinue"),
      ].join("\n"),
    );
    if (!confirmed) return;
    await runHistoryRepair(false);
  }

  /// 复制会话正文或路径到剪贴板。
  async function copyText(text: string, message: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(message);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) || t("codexHistoryRepair.copyFailed"),
      );
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
      ? t("codexHistoryRepair.repairScopeSelected", {
          count: selectedSessionIds.length,
        })
      : t("codexHistoryRepair.repairScopeBalanced");
  const activeDbLabel =
    historyList?.stateDbPath ||
    normalizedStateDbPath ||
    "~/.codex/sqlite/state_5.sqlite";

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-lg border bg-card">
      <div className="shrink-0 border-b px-4 py-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2 text-base font-semibold">
              <ProviderIcon icon="openai" name="Codex" size={20} />
              <span>{t("codexHistoryRepair.title")}</span>
              <Badge variant="secondary">
                {t("codexHistoryRepair.desktopHistory")}
              </Badge>
              <Badge variant="outline">source={sourceFilter}</Badge>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span className="font-mono">maxPerProject=10</span>
              <span className="font-mono">maxTotal=300</span>
              <span>{repairScopeLabel}</span>
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant={historyList ? "outline" : "default"}
              onClick={loadHistorySessions}
              disabled={isLoadingHistory}
              className="gap-2"
            >
              {isLoadingHistory ? (
                <RefreshCw className="size-4 animate-spin" />
              ) : (
                <Database className="size-4" />
              )}
              {t("codexHistoryRepair.loadHistory")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => runHistoryRepair(true)}
              disabled={isPreviewingRepair || isApplyingRepair}
              className="gap-2"
            >
              {isPreviewingRepair ? (
                <RefreshCw className="size-4 animate-spin" />
              ) : (
                <FileClock className="size-4" />
              )}
              {t("codexHistoryRepair.previewRepair")}
            </Button>
            <Button
              size="sm"
              onClick={applyHistoryRepair}
              disabled={!canApplyRepair}
              className="gap-2"
            >
              {isApplyingRepair ? (
                <RefreshCw className="size-4 animate-spin" />
              ) : (
                <ShieldCheck className="size-4" />
              )}
              {t("codexHistoryRepair.applyRepair")}
            </Button>
            {onClose ? (
              <Button size="sm" variant="ghost" onClick={onClose}>
                <X className="size-4" />
                {t("codexHistoryRepair.sessionBrowser")}
              </Button>
            ) : null}
          </div>
        </div>

        <div className="mt-3 grid gap-2 md:grid-cols-3">
          <RepairStatusTile
            icon={<Database className="size-4" />}
            label="Active DB"
            value={activeDbLabel}
          />
          <RepairStatusTile
            icon={<ListChecks className="size-4" />}
            label={t("codexHistoryRepair.loadedSelected")}
            value={`${historyList?.totalMatched ?? 0} / ${selectedSessionIds.length}`}
          />
          <RepairStatusTile
            icon={<ShieldCheck className="size-4" />}
            label={t("codexHistoryRepair.writeStatus")}
            value={
              canApplyRepair
                ? t("codexHistoryRepair.previewLocked")
                : repairResult?.dryRun
                  ? t("codexHistoryRepair.paramsChanged")
                  : t("codexHistoryRepair.waitingPreview")
            }
          />
        </div>
      </div>

      <div className="shrink-0 border-b bg-muted/20 px-4 py-3">
        <RepairSettings
          codexHome={codexHome}
          stateDbPath={stateDbPath}
          projectPath={projectPath}
          targetProvider={targetProvider}
          targetProviderOptions={targetProviderOptions}
          sourceFilter={sourceFilter}
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
          onTargetProviderChange={(value) => {
            setTargetProvider(value);
            invalidatePreview();
          }}
          onSourceFilterChange={(value) => {
            setSourceFilter(value);
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
        />
      </div>

      <div className="grid min-h-0 flex-1 xl:grid-cols-[400px_minmax(0,1fr)]">
        <div className="flex min-h-0 flex-col border-r">
          <div className="border-b px-3 py-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-sm font-semibold">
                <FileClock className="size-4" />
                {t("codexHistoryRepair.sqliteHistory")}
                <Badge variant="secondary">
                  {historyList?.totalMatched ?? 0}
                </Badge>
              </div>
              <Badge variant="outline">
                {t("codexHistoryRepair.selectedCount", {
                  count: selectedSessionIds.length,
                })}
              </Badge>
            </div>
            <div className="mt-2 flex gap-2">
              <div className="relative min-w-0 flex-1">
                <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={historyQuery}
                  onChange={(event) => setHistoryQuery(event.target.value)}
                  placeholder={t("codexHistoryRepair.searchPlaceholder")}
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
                {t("codexHistoryRepair.selectAllPage")}
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
                {t("codexHistoryRepair.clearSelection")}
              </Button>
            </div>
          </div>

          {historyListError ? (
            <div className="m-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive">
              {t("codexHistoryRepair.loadFailed", {
                error: historyListError,
              })}
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
                    ? t("codexHistoryRepair.noMatches")
                    : isLoadingHistory
                      ? t("codexHistoryRepair.loadingSqlite")
                      : t("codexHistoryRepair.loadThenSelect")}
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
  targetProvider: string;
  targetProviderOptions: string[];
  sourceFilter: string;
  includeArchived: boolean;
  includeSubagents: boolean;
  historyList: CodexHistorySessionListOutcome | null;
  onCodexHomeChange: (value: string) => void;
  onStateDbPathChange: (value: string) => void;
  onProjectPathChange: (value: string) => void;
  onTargetProviderChange: (value: string) => void;
  onSourceFilterChange: (value: string) => void;
  onIncludeArchivedChange: (checked: boolean) => void;
  onIncludeSubagentsChange: (checked: boolean) => void;
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
  targetProvider,
  targetProviderOptions,
  sourceFilter,
  includeArchived,
  includeSubagents,
  historyList,
  onCodexHomeChange,
  onStateDbPathChange,
  onProjectPathChange,
  onTargetProviderChange,
  onSourceFilterChange,
  onIncludeArchivedChange,
  onIncludeSubagentsChange,
}: RepairSettingsProps) {
  const { t } = useTranslation();
  const sourceOptions = useMemo(() => buildSourceOptions(t), [t]);

  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-2 text-sm font-semibold">
        <SlidersHorizontal className="size-4" />
        {t("codexHistoryRepair.pathsAndScope")}
      </div>
      <div className="grid gap-3 lg:grid-cols-3 2xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)_minmax(0,1fr)_260px_180px_180px]">
        <LabeledInput
          label={t("codexHistoryRepair.codexHome")}
          value={codexHome}
          placeholder={t("codexHistoryRepair.codexHomePlaceholder")}
          hint={historyList?.codexHome ?? t("codexHistoryRepair.codexHomeHint")}
          onChange={onCodexHomeChange}
        />
        <LabeledInput
          label="Active DB"
          value={stateDbPath}
          placeholder={t("codexHistoryRepair.activeDbPlaceholder")}
          hint={
            historyList?.stateDbPath ?? t("codexHistoryRepair.activeDbHint")
          }
          onChange={onStateDbPathChange}
        />
        <LabeledInput
          label={t("codexHistoryRepair.projectPath")}
          value={projectPath}
          placeholder={t("codexHistoryRepair.projectPathPlaceholder")}
          hint={projectPath.trim() || t("codexHistoryRepair.projectPathHint")}
          onChange={onProjectPathChange}
        />
        <label className="text-xs font-medium">
          {t("codexHistoryRepair.targetProvider")}
          <Select value={targetProvider} onValueChange={onTargetProviderChange}>
            <SelectTrigger className="mt-1 h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={AUTO_TARGET}>
                {t("codexHistoryRepair.autoTarget")}
              </SelectItem>
              {targetProviderOptions.map((provider) => (
                <SelectItem key={provider} value={provider}>
                  {provider}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <div className="mt-1 rounded-md bg-muted/60 px-2 py-1 font-mono text-[11px] text-muted-foreground">
            live:{" "}
            {historyList?.liveConfigModelProvider ??
              t("codexHistoryRepair.loadPending")}
          </div>
        </label>
        <label className="text-xs font-medium">
          {t("codexHistoryRepair.sourceLabel")}
          <Select value={sourceFilter} onValueChange={onSourceFilterChange}>
            <SelectTrigger className="mt-1 h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sourceOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <div className="mt-1 rounded-md bg-muted/60 px-2 py-1 text-[11px] text-muted-foreground">
            {
              sourceOptions.find((option) => option.value === sourceFilter)
                ?.detail
            }
          </div>
        </label>
        <div className="grid gap-2 self-end text-xs">
          <ToggleLine
            checked={includeArchived}
            label={t("codexHistoryRepair.includeArchived")}
            onChange={onIncludeArchivedChange}
          />
          <ToggleLine
            checked={includeSubagents}
            label={t("codexHistoryRepair.includeSubagents")}
            onChange={onIncludeSubagentsChange}
          />
        </div>
      </div>
    </div>
  );
}

interface LabeledInputProps {
  label: string;
  value: string;
  placeholder: string;
  hint: string;
  onChange: (value: string) => void;
}

/// 渲染带浅底默认提示的输入框，避免把空值误解为未配置。
function LabeledInput({
  label,
  value,
  placeholder,
  hint,
  onChange,
}: LabeledInputProps) {
  return (
    <label className="text-xs font-medium">
      {label}
      <Input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
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
  const { t, i18n } = useTranslation();

  return (
    <div
      className={cn(
        "grid grid-cols-[28px_1fr] gap-2 px-3 py-2 text-xs transition",
        active ? "bg-primary/10" : "hover:bg-muted/50",
      )}
    >
      <Checkbox
        checked={selected}
        aria-label={t("codexHistoryRepair.selectSessionAria", {
          title: session.title || session.id,
        })}
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
          {session.cwd ?? t("codexHistoryRepair.noCwd")}
        </div>
        <div className="mt-1 flex flex-wrap gap-1 text-[11px] text-muted-foreground">
          <Badge variant="outline">{session.modelProvider ?? "-"}</Badge>
          <Badge variant="outline">
            source={compactSource(session.source)}
          </Badge>
          <span>
            {formatHistorySessionTime(
              session.updatedAt,
              i18n.resolvedLanguage || i18n.language,
            )}
          </span>
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
  const { t } = useTranslation();
  const session = detail?.session;
  return (
    <div className="flex min-h-0 flex-col bg-card">
      <div className="flex flex-wrap items-start justify-between gap-2 border-b px-3 py-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Eye className="size-4" />
            {t("codexHistoryRepair.sessionContent")}
          </div>
          <div className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
            {session?.id ?? t("codexHistoryRepair.noSessionSelected")}
          </div>
        </div>
        {detail?.rolloutPath ? (
          <Button
            size="sm"
            variant="outline"
            className="gap-2"
            onClick={() =>
              onCopy(
                detail.rolloutPath!,
                t("codexHistoryRepair.rolloutPathCopied"),
              )
            }
          >
            <Copy className="size-3.5" />
            {t("codexHistoryRepair.path")}
          </Button>
        ) : null}
      </div>

      {isLoading ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
          <RefreshCw className="size-4 animate-spin" />
          {t("codexHistoryRepair.loadingSession")}
        </div>
      ) : error ? (
        <div className="m-3 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs text-destructive">
          {t("codexHistoryRepair.readFailed", { error })}
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
                onCopy={(content) =>
                  onCopy(content, t("codexHistoryRepair.messageCopied"))
                }
              />
            ))}
          </div>
        </ScrollArea>
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-8 text-center text-sm text-muted-foreground">
          <Eye className="size-9 opacity-35" />
          <span>{t("codexHistoryRepair.selectSessionToView")}</span>
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
  const { t } = useTranslation();

  return (
    <div className="flex min-h-0 flex-col border-t bg-card 2xl:border-t-0 2xl:border-l">
      <div className="border-b px-3 py-2">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <Info className="size-4" />
          {t("codexHistoryRepair.resultTitle")}
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
                <Badge>
                  {result.dryRun
                    ? t("codexHistoryRepair.preview")
                    : t("codexHistoryRepair.applied")}
                </Badge>
                <Badge variant="outline">target={result.targetProvider}</Badge>
                <Badge variant="outline">
                  source={result.sourceFilter || "interactive"}
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
                  backup=
                  {result.backupDir ?? t("codexHistoryRepair.backupPending")}
                </div>
              </div>
            </div>
          ) : (
            <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
              {t("codexHistoryRepair.previewFirst")}
            </div>
          )}

          <ValueCountPanel
            title={t("codexHistoryRepair.sourceCounts")}
            rows={sourceCounts}
          />
          <ValueCountPanel
            title={t("codexHistoryRepair.providerCounts")}
            rows={providerCounts}
          />
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
  const { t } = useTranslation();

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
        <div className="text-xs text-muted-foreground">
          {t("codexHistoryRepair.loadPending")}
        </div>
      )}
    </div>
  );
}

/// 生成带本地化说明的 source 选项，解释 threads.source 的筛选语义。
function buildSourceOptions(t: TFunction): SourceOption[] {
  return [
    {
      value: "vscode",
      label: "VS Code",
      detail: t("codexHistoryRepair.sourceOptions.vscode.detail"),
    },
    {
      value: "interactive",
      label: "CLI + VS Code",
      detail: t("codexHistoryRepair.sourceOptions.interactive.detail"),
    },
    {
      value: "cli",
      label: "CLI",
      detail: t("codexHistoryRepair.sourceOptions.cli.detail"),
    },
    {
      value: "exec",
      label: "Exec",
      detail: t("codexHistoryRepair.sourceOptions.exec.detail"),
    },
    {
      value: "all",
      label: t("codexHistoryRepair.sourceOptions.all.label"),
      detail: t("codexHistoryRepair.sourceOptions.all.detail"),
    },
  ];
}

/// 生成 provider 下拉候选并去重，避免把自动项和真实 provider 混在一起。
function buildTargetProviderOptions(
  historyList: CodexHistorySessionListOutcome | null,
): string[] {
  const values = [...(historyList?.targetProviderCandidates ?? []), "custom"];
  return values.filter(
    (value, index) =>
      value.trim() &&
      value !== AUTO_TARGET &&
      values.findIndex((item) => item === value) === index,
  );
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
function formatHistorySessionTime(
  value: string | null,
  locale: string,
): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(locale, {
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
