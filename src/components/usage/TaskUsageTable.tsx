import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  ChevronsUpDown,
  TriangleAlert,
} from "lucide-react";
import {
  useAgentTaskUsage,
  useAgentTaskUsageFilterOptions,
  useAgentUsageCapabilities,
  type AgentTaskUsageQueryFilter,
} from "@/lib/query/usage";
import {
  type AgentTaskUsageRow,
  type AgentUsageCapability,
  type AgentUsageMeasure,
  type AgentUsageSourceDimension,
  type AgentUsageAppType,
  type UsageRangeSelection,
} from "@/types/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import {
  fmtInt,
  formatKnownTokenTotal,
  formatUsageCost,
  formatUsageCostWithStatus,
  isCodexReplayInProgress,
  resolveUsageCostStatusForMeasure,
  usageSourceDimensionsForScope,
} from "./format";
import { UsageCostTooltip, UsageQualityTooltip } from "./UsageQualityTooltip";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

const TASK_PAGE_SIZE = 20;
const TASK_TABLE_MIN_WIDTH = 1280;

interface TaskUsageTableProps {
  range: UsageRangeSelection;
  refreshIntervalMs: number;
  /** Keep the dashboard's app selector useful when the task tab is opened. */
  initialAppType?: AgentUsageAppType;
}

/** Convert the dashboard's date picker state to the canonical task query range. */
export function toAgentUsageRange(selection: UsageRangeSelection) {
  const { startDate, endDate } = resolveUsageRange(selection);
  return { startAt: startDate, endAt: endDate };
}

function agentLabel(
  appType: AgentUsageAppType,
  t: (key: string, options?: { defaultValue?: string }) => string,
) {
  const labels: Record<AgentUsageAppType, [string, string]> = {
    claude: ["usage.appFilter.claude", "Claude Code"],
    "claude-desktop": [
      "usage.appFilter.claudeDesktop",
      "Claude Desktop / Cowork",
    ],
    codex: ["usage.appFilter.codex", "Codex"],
    gemini: ["usage.appFilter.gemini", "Gemini CLI"],
    grokbuild: ["usage.appFilter.grokbuild", "Grok Build"],
    opencode: ["usage.appFilter.opencode", "OpenCode"],
    openclaw: ["usage.appFilter.openclaw", "OpenClaw"],
    hermes: ["usage.appFilter.hermes", "Hermes"],
    pi: ["usage.appFilter.pi", "Pi"],
  };
  const [key, fallback] = labels[appType];
  return t(key, { defaultValue: fallback });
}

function requestCountSemanticsLabel(
  semantics: AgentUsageMeasure["requestCountSemantics"],
  t: (key: string, options?: { defaultValue?: string }) => string,
) {
  const labels: Record<
    AgentUsageMeasure["requestCountSemantics"],
    [string, string]
  > = {
    http_request: ["usage.task.count.httpRequest", "HTTP requests"],
    assistant_message: [
      "usage.task.count.assistantMessage",
      "Assistant messages",
    ],
    agent_call: ["usage.task.count.agentCall", "Agent calls"],
    usage_event: ["usage.task.count.usageEvent", "Usage events"],
    unavailable: ["usage.task.count.unavailable", "Count unavailable"],
  };
  const [key, fallback] = labels[semantics];
  return t(key, { defaultValue: fallback });
}

function nullableInteger(value: number | null | undefined) {
  return value == null ? "—" : fmtInt(value);
}

function displayMeasureCost(
  measure: AgentUsageMeasure | null,
  sourceDimensions: AgentUsageSourceDimension[] | undefined,
) {
  return formatUsageCostWithStatus(
    measure,
    resolveUsageCostStatusForMeasure(measure, sourceDimensions),
  );
}

function shortSessionId(row: AgentTaskUsageRow) {
  const sessionId = row.rootSessionId || row.sessionId;
  return sessionId.length > 8 ? sessionId.slice(0, 8) : sessionId;
}

function taskTitle(
  row: AgentTaskUsageRow,
  t: (key: string, options?: { defaultValue?: string }) => string,
) {
  const title = row.root?.title?.trim();
  return (
    title ||
    `${t("usage.task.titleUnavailable", {
      defaultValue: "Task title not provided",
    })} · ${shortSessionId(row)}`
  );
}

function taskTitleTooltip(row: AgentTaskUsageRow) {
  return row.root?.title?.trim() || row.rootSessionId || row.sessionId;
}

function projectBasename(projectDir: string) {
  const trimmed = projectDir.trim();
  if (!trimmed) return "";
  const withoutTrailingSeparators = trimmed.replace(/[\\/]+$/, "");
  if (!withoutTrailingSeparators) return trimmed;
  if (/^[A-Za-z]:$/.test(withoutTrailingSeparators)) return trimmed;
  const separator = Math.max(
    withoutTrailingSeparators.lastIndexOf("/"),
    withoutTrailingSeparators.lastIndexOf("\\"),
  );
  return separator >= 0
    ? withoutTrailingSeparators.slice(separator + 1) || trimmed
    : withoutTrailingSeparators;
}

function taskProject(row: AgentTaskUsageRow) {
  const projectDir = row.root?.projectDir?.trim();
  if (!projectDir) return null;
  return projectBasename(projectDir);
}

function taskAgentProjectLabel(
  row: AgentTaskUsageRow,
  t: (key: string, options?: { defaultValue?: string }) => string,
) {
  const agent = agentLabel(row.appType, t);
  const project = taskProject(row);
  return project ? `${agent} - ${project}` : agent;
}

function rowKey(row: AgentTaskUsageRow) {
  return `${row.appType}:${row.rootSessionId || row.sessionId}`;
}

function taskTokenTotal(
  measure: AgentUsageMeasure | null,
  t: (key: string, options?: { defaultValue?: string }) => string,
) {
  return formatKnownTokenTotal(
    measure,
    undefined,
    t("usage.task.unavailable", { defaultValue: "Unavailable" }),
  );
}

function UnattributedUsageSummary({
  measure,
  t,
}: {
  measure: AgentUsageMeasure;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const tokenTotal = formatKnownTokenTotal(measure, undefined, "—");
  const cost = formatUsageCostWithStatus(
    measure,
    measure.totalCostUsd == null ? "unavailable" : "reported",
  );
  const requestCount =
    measure.requestCount == null ? "—" : fmtInt(measure.requestCount);
  const hint = t("usage.task.unattributedHint", {
    defaultValue:
      "These Codex proxy requests are included in the top cost total but have no verifiable native session event, so they are not assigned to a specific task.",
  });
  return (
    <TooltipProvider delayDuration={220}>
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            className="flex min-w-0 cursor-help items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-300"
            data-testid="unattributed-usage-summary"
            role="status"
            tabIndex={0}
            aria-label={hint}
          >
            <TriangleAlert className="size-4 shrink-0" aria-hidden="true" />
            <span className="shrink-0 font-medium">
              {t("usage.task.unattributed", {
                defaultValue: "Unattributed sessions",
              })}
            </span>
            <span className="min-w-0 truncate text-xs text-amber-800/80 dark:text-amber-200/80">
              {requestCount}{" "}
              {t("usage.task.count.httpRequest", {
                defaultValue: "HTTP requests",
              })}
              <span className="mx-1">·</span>
              {tokenTotal} {t("usage.tokens", { defaultValue: "tokens" })}
              <span className="mx-1">·</span>
              {t("usage.cost", { defaultValue: "Cost" })} {cost}
            </span>
          </div>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          align="start"
          collisionPadding={12}
          className="max-w-[420px] whitespace-normal text-left"
        >
          {hint}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function CodexReplayStatus({
  status,
  t,
}: {
  status: "rebuilding_with_snapshot" | "rebuilding";
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const message = t(
    status === "rebuilding_with_snapshot"
      ? "usage.task.rebuildingWithSnapshot"
      : "usage.task.rebuilding",
    {
      defaultValue:
        status === "rebuilding_with_snapshot"
          ? "Codex session statistics are updating; showing the last complete result. Unattributed sessions will be calculated after the rebuild finishes."
          : "Codex session statistics are being rebuilt. Unattributed sessions will not be calculated until the rebuild finishes.",
    },
  );
  return (
    <div
      className="flex min-w-0 items-center gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-300"
      data-testid="codex-replay-status"
      role="status"
    >
      <TriangleAlert className="size-4 shrink-0" aria-hidden="true" />
      <span className="min-w-0">{message}</span>
    </div>
  );
}

interface TaskFilterComboboxOption {
  value: string;
  label: string;
  description?: string;
}

function TaskFilterCombobox({
  label,
  placeholder,
  searchPlaceholder,
  loadingText,
  emptyText,
  clearText,
  value,
  options,
  loading,
  onChange,
}: {
  label: string;
  placeholder: string;
  searchPlaceholder: string;
  loadingText: string;
  emptyText: string;
  clearText: string;
  value: string;
  options: TaskFilterComboboxOption[];
  loading?: boolean;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const selected = options.find((option) => option.value === value);

  return (
    <Popover modal open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          aria-busy={loading || undefined}
          className="flex min-h-9 h-auto w-full min-w-0 items-center justify-between gap-2 rounded-md border border-border-default bg-background px-3 py-1.5 text-left text-sm text-foreground shadow-sm outline-none transition-colors focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
        >
          <span className="min-w-0 truncate">
            {selected ? (
              <>
                <span className="block truncate">{selected.label}</span>
                {selected.description && (
                  <span className="block truncate text-[11px] text-muted-foreground">
                    {selected.description}
                  </span>
                )}
              </>
            ) : (
              <span className="text-muted-foreground">{placeholder}</span>
            )}
          </span>
          <ChevronsUpDown className="h-4 w-4 shrink-0 text-muted-foreground" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="start"
        sideOffset={6}
        collisionPadding={8}
        className="z-[1000] w-[var(--radix-popover-trigger-width)] p-0"
      >
        <Command label={searchPlaceholder}>
          <CommandInput aria-label={label} placeholder={searchPlaceholder} />
          <CommandList>
            <CommandEmpty>{loading ? loadingText : emptyText}</CommandEmpty>
            <CommandGroup>
              {value && (
                <CommandItem
                  value="__clear_task_filter__"
                  onSelect={() => {
                    onChange("");
                    setOpen(false);
                  }}
                >
                  <Check className="mr-2 h-4 w-4 opacity-0" />
                  {clearText}
                </CommandItem>
              )}
              {options.map((option) => (
                <CommandItem
                  key={option.value}
                  value={option.value}
                  keywords={
                    option.description
                      ? [option.label, option.description]
                      : [option.label]
                  }
                  onSelect={() => {
                    onChange(option.value);
                    setOpen(false);
                  }}
                >
                  <Check
                    className={`mr-2 h-4 w-4 ${
                      value === option.value ? "opacity-100" : "opacity-0"
                    }`}
                  />
                  <span className="min-w-0">
                    <span className="block truncate">{option.label}</span>
                    {option.description && (
                      <span className="block truncate text-xs text-muted-foreground">
                        {option.description}
                      </span>
                    )}
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function useWideTaskLayout(
  containerRef: RefObject<HTMLElement>,
  minWidth: number,
) {
  const [isWide, setIsWide] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const update = () => setIsWide(container.clientWidth >= minWidth);
    update();

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }

    const observer = new ResizeObserver(update);
    observer.observe(container);
    return () => observer.disconnect();
  }, [containerRef, minWidth]);

  return isWide;
}

function hasUsageQualityDetails(row: AgentTaskUsageRow) {
  const measures = [row.totalUsage, row.selfUsage, row.descendantUsage];
  return Boolean(
    row.partial ||
      row.warnings.length > 0 ||
      measures.some(
        (measure) =>
          measure?.partial ||
          measure?.warnings.length ||
          (measure?.timeSemantics != null &&
            measure.timeSemantics !== "event_time"),
      ),
  );
}

function usageTimeSemantics(row: AgentTaskUsageRow) {
  const measures = [row.totalUsage, row.selfUsage, row.descendantUsage];
  return measures.find(
    (measure) =>
      measure?.timeSemantics && measure.timeSemantics !== "event_time",
  )?.timeSemantics;
}

function TaskUsageDetails({
  row,
  expanded,
  onToggle,
  t,
}: {
  row: AgentTaskUsageRow;
  expanded: boolean;
  onToggle: () => void;
  t: (key: string, options?: { defaultValue?: string }) => string;
}) {
  const key = rowKey(row);
  if (row.descendantSessionCount <= 0) return null;

  return (
    <div className="mt-2 border-t border-border/50 pt-2">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 px-1.5 text-xs"
        aria-expanded={expanded}
        aria-controls={`task-details-${key}`}
        onClick={onToggle}
      >
        {expanded ? (
          <ChevronUp className="h-3.5 w-3.5" />
        ) : (
          <ChevronDown className="h-3.5 w-3.5" />
        )}
        {t("usage.task.viewBreakdown", {
          defaultValue: "Self / descendants",
        })}
        <span className="text-muted-foreground">
          ({row.descendantSessionCount})
        </span>
      </Button>
      {expanded && (
        <div
          id={`task-details-${key}`}
          className="mt-2 space-y-2"
          data-testid={`task-details-${key}`}
        >
          <div data-testid={`task-breakdown-${key}`} className="space-y-2">
            <MeasureBreakdown
              label={t("usage.task.self", { defaultValue: "Self" })}
              measure={row.selfUsage}
              sourceDimensions={usageSourceDimensionsForScope(
                row.sourceDimensions,
                false,
              )}
              t={t}
            />
            <MeasureBreakdown
              label={t("usage.task.descendants", {
                defaultValue: "Descendants",
              })}
              measure={row.descendantUsage}
              emptyMessage={
                row.descendantUsageStatus === "no_activity_in_range"
                  ? t("usage.task.noDescendantActivity", {
                      defaultValue:
                        "No descendant activity in the selected time range",
                    })
                  : undefined
              }
              sourceDimensions={usageSourceDimensionsForScope(
                row.sourceDimensions,
                true,
              )}
              t={t}
            />
          </div>
        </div>
      )}
    </div>
  );
}

function MeasureBreakdown({
  label,
  measure,
  emptyMessage,
  sourceDimensions,
  t,
}: {
  label: string;
  measure: AgentUsageMeasure | null;
  emptyMessage?: string;
  sourceDimensions: AgentUsageSourceDimension[];
  t: (key: string, options?: { defaultValue?: string }) => string;
}) {
  if (!measure) {
    return (
      <div className="rounded-md border border-border/50 bg-muted/20 p-2 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">{label}:</span>{" "}
        {emptyMessage ??
          t("usage.task.unavailable", { defaultValue: "Usage unavailable" })}
      </div>
    );
  }

  const costStatus = resolveUsageCostStatusForMeasure(
    measure,
    sourceDimensions,
  );
  const replayInProgress = isCodexReplayInProgress(sourceDimensions);

  return (
    <div className="rounded-md border border-border/50 bg-muted/20 p-2 text-xs">
      <div className="mb-1 flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="font-medium text-foreground">{label}</span>
      </div>
      <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-muted-foreground sm:grid-cols-4">
        <span>
          {t("usage.inputTokens", { defaultValue: "Input" })}:{" "}
          {nullableInteger(measure.inputTokens)}
        </span>
        <span>
          {t("usage.outputTokens", { defaultValue: "Output" })}:{" "}
          {nullableInteger(measure.outputTokens)}
        </span>
        <span>
          {t("usage.cacheReadTokens", { defaultValue: "Cache read" })}:{" "}
          {nullableInteger(measure.cacheReadTokens)}
        </span>
        <span>
          {t("usage.cacheCreationTokens", {
            defaultValue: "Cache creation",
          })}
          : {nullableInteger(measure.cacheCreationTokens)}
        </span>
      </div>
      <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-muted-foreground">
        <span>
          {t("usage.cost", { defaultValue: "Cost" })}:{" "}
          <UsageCostTooltip
            status={costStatus}
            replayInProgress={replayInProgress}
          >
            {displayMeasureCost(measure, sourceDimensions)}
          </UsageCostTooltip>
        </span>
        <span>
          {measure.requestCount == null ? "—" : fmtInt(measure.requestCount)}{" "}
          {requestCountSemanticsLabel(measure.requestCountSemantics, t)}
        </span>
      </div>
    </div>
  );
}

function TaskUsageRowView({
  row,
  expanded,
  onToggle,
  t,
}: {
  row: AgentTaskUsageRow;
  expanded: boolean;
  onToggle: () => void;
  t: (key: string, options?: { defaultValue?: string }) => string;
}) {
  const key = rowKey(row);
  const title = taskTitle(row, t);
  const titleTooltip = taskTitleTooltip(row);
  const total = row.totalUsage;
  const costStatus = resolveUsageCostStatusForMeasure(
    total,
    row.sourceDimensions,
  );
  const replayInProgress = isCodexReplayInProgress(row.sourceDimensions);
  const countLabel = total
    ? requestCountSemanticsLabel(total.requestCountSemantics, t)
    : requestCountSemanticsLabel("unavailable", t);

  return (
    <TableRow data-testid={`task-row-${key}`}>
      <TableCell className="min-w-[320px] max-w-[560px] align-top">
        <div className="flex min-w-0 items-center gap-1.5">
          <div className="min-w-0 truncate font-medium" title={titleTooltip}>
            {title}
          </div>
          <UsageQualityTooltip
            partial={hasUsageQualityDetails(row)}
            timeSemantics={usageTimeSemantics(row)}
            costStatus={costStatus}
            replayInProgress={replayInProgress}
          />
        </div>
        <div className="mt-1 truncate text-xs text-muted-foreground">
          {taskAgentProjectLabel(row, t)}
        </div>
      </TableCell>
      <TableCell className="min-w-[170px] align-top">
        <div className="font-semibold tabular-nums">
          {taskTokenTotal(total, t)}{" "}
          <span className="text-xs font-normal text-muted-foreground">
            {t("usage.tokens", { defaultValue: "tokens" })}
          </span>
        </div>
        <div className="mt-1 text-xs text-muted-foreground">
          <UsageCostTooltip
            status={costStatus}
            replayInProgress={replayInProgress}
          >
            {formatUsageCost(total, row.sourceDimensions)}
          </UsageCostTooltip>
        </div>
        <TaskUsageDetails
          row={row}
          expanded={expanded}
          onToggle={onToggle}
          t={t}
        />
      </TableCell>
      <TableCell className="min-w-[120px] align-top text-right">
        {total?.requestCount == null ? "—" : fmtInt(total.requestCount)}
        <div className="mt-1 text-xs text-muted-foreground">{countLabel}</div>
      </TableCell>
    </TableRow>
  );
}

function TaskUsageCardView({
  row,
  expanded,
  onToggle,
  t,
}: {
  row: AgentTaskUsageRow;
  expanded: boolean;
  onToggle: () => void;
  t: (key: string, options?: { defaultValue?: string }) => string;
}) {
  const key = rowKey(row);
  const title = taskTitle(row, t);
  const titleTooltip = taskTitleTooltip(row);
  const total = row.totalUsage;
  const costStatus = resolveUsageCostStatusForMeasure(
    total,
    row.sourceDimensions,
  );
  const replayInProgress = isCodexReplayInProgress(row.sourceDimensions);
  const countLabel = total
    ? requestCountSemanticsLabel(total.requestCountSemantics, t)
    : requestCountSemanticsLabel("unavailable", t);

  return (
    <article
      data-testid={`task-row-${key}`}
      className="min-w-0 rounded-lg border border-border/60 bg-card/40 p-3 backdrop-blur-sm"
    >
      <div className="flex min-w-0 flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-1.5">
            <div
              className="line-clamp-2 min-w-0 break-words font-medium"
              title={titleTooltip}
            >
              {title}
            </div>
            <UsageQualityTooltip
              partial={hasUsageQualityDetails(row)}
              timeSemantics={usageTimeSemantics(row)}
              costStatus={costStatus}
              replayInProgress={replayInProgress}
            />
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground">
            {taskAgentProjectLabel(row, t)}
          </div>
        </div>
      </div>

      <div className="mt-3 grid min-w-0 grid-cols-3 gap-x-4 gap-y-2 border-t border-border/50 pt-3 text-xs">
        <div className="min-w-0">
          <div className="text-muted-foreground">
            {t("usage.task.total", { defaultValue: "Derived total" })}
          </div>
          <div className="mt-1 truncate font-semibold tabular-nums">
            {taskTokenTotal(total, t)}{" "}
            <span className="font-normal text-muted-foreground">
              {t("usage.tokens", { defaultValue: "tokens" })}
            </span>
          </div>
        </div>
        <div className="min-w-0">
          <div className="text-muted-foreground">
            {t("usage.cost", { defaultValue: "Cost" })}
          </div>
          <div className="mt-1 truncate font-semibold tabular-nums">
            <UsageCostTooltip
              status={costStatus}
              replayInProgress={replayInProgress}
            >
              {formatUsageCost(total, row.sourceDimensions)}
            </UsageCostTooltip>
          </div>
        </div>
        <div className="min-w-0">
          <div className="text-muted-foreground">
            {t("usage.task.count", { defaultValue: "Count" })}
          </div>
          <div className="mt-1 truncate tabular-nums">
            {total?.requestCount == null ? "—" : fmtInt(total.requestCount)}{" "}
            <span className="text-muted-foreground">{countLabel}</span>
          </div>
        </div>
      </div>

      <TaskUsageDetails
        row={row}
        expanded={expanded}
        onToggle={onToggle}
        t={t}
      />
    </article>
  );
}

export function TaskUsageTable({
  range,
  refreshIntervalMs,
  initialAppType,
}: TaskUsageTableProps) {
  const { t } = useTranslation();
  const layoutRef = useRef<HTMLDivElement>(null);
  const isWideLayout = useWideTaskLayout(layoutRef, TASK_TABLE_MIN_WIDTH);
  const [agentAppType, setAgentAppType] = useState<AgentUsageAppType | "all">(
    initialAppType ?? "all",
  );
  const [title, setTitle] = useState("");
  const [projectDir, setProjectDir] = useState("");
  const [page, setPage] = useState(0);
  const [expandedRows, setExpandedRows] = useState<string[]>([]);

  useEffect(() => {
    setAgentAppType(initialAppType ?? "all");
  }, [initialAppType]);

  const rangeSelection = useMemo<UsageRangeSelection>(
    () => ({
      preset: range.preset,
      customStartDate: range.customStartDate,
      customEndDate: range.customEndDate,
      liveEndTime: range.liveEndTime,
    }),
    [
      range.customEndDate,
      range.customStartDate,
      range.liveEndTime,
      range.preset,
    ],
  );

  const filter = useMemo<AgentTaskUsageQueryFilter>(
    () => ({
      appType: agentAppType === "all" ? undefined : agentAppType,
      titleExact: title.trim() || undefined,
      projectDirExact: projectDir.trim() || undefined,
      rangeSelection,
      limit: TASK_PAGE_SIZE,
      offset: page * TASK_PAGE_SIZE,
    }),
    [agentAppType, page, projectDir, rangeSelection, title],
  );

  const { data, isLoading, isError, error, isFetching } = useAgentTaskUsage(
    filter,
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );
  const capabilitiesQuery = useAgentUsageCapabilities({
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });
  const filterOptionsQuery = useAgentTaskUsageFilterOptions(
    {
      appType: agentAppType === "all" ? undefined : agentAppType,
      rangeSelection,
    },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const capabilities = capabilitiesQuery.data ?? [];
  const filterOptions = filterOptionsQuery.data;
  const titleOptions = (filterOptions?.titles ?? []).map((value) => ({
    value,
    label: value,
  }));
  const projectOptions = (filterOptions?.projects ?? []).map((option) => ({
    value: option.projectDir,
    label: projectBasename(option.projectDir),
  }));

  const rows = data?.items ?? [];
  const total = data?.total ?? 0;
  const totalPages = total > 0 ? Math.ceil(total / TASK_PAGE_SIZE) : 0;
  const dataStatus = data?.dataStatus ?? "ready";
  const codexRebuildingWithoutSnapshot =
    agentAppType === "codex" && dataStatus === "rebuilding";

  useEffect(() => {
    const lastPage = Math.max(0, totalPages - 1);
    if (page > lastPage) {
      setPage(lastPage);
    }
  }, [page, totalPages]);

  useEffect(() => {
    setPage(0);
    setExpandedRows([]);
  }, [
    agentAppType,
    projectDir,
    range.customEndDate,
    range.customStartDate,
    range.liveEndTime,
    range.preset,
    title,
  ]);

  useEffect(() => {
    setTitle("");
    setProjectDir("");
  }, [agentAppType, rangeSelection]);

  const toggleExpanded = (key: string) => {
    setExpandedRows((current) =>
      current.includes(key)
        ? current.filter((item) => item !== key)
        : [...current, key],
    );
  };

  return (
    <div
      ref={layoutRef}
      className="min-w-0 space-y-4"
      data-testid="task-usage-layout"
      data-layout={isWideLayout ? "table" : "cards"}
    >
      <div className="rounded-lg border border-border/50 bg-card/50 p-3 backdrop-blur-sm">
        <div className="flex min-w-0 flex-wrap items-end gap-x-3 gap-y-2">
          <label className="flex w-full min-w-0 flex-col gap-1 text-xs text-muted-foreground sm:w-40 sm:shrink-0">
            <span>{t("usage.task.agent", { defaultValue: "Agent" })}</span>
            <select
              aria-label={t("usage.task.agent", { defaultValue: "Agent" })}
              value={agentAppType}
              onChange={(event) =>
                setAgentAppType(event.target.value as AgentUsageAppType | "all")
              }
              className="h-9 w-full min-w-0 truncate rounded-md border border-border-default bg-background px-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20"
            >
              <option value="all">
                {t("usage.task.allAgents", { defaultValue: "All agents" })}
              </option>
              {capabilities.map((capability: AgentUsageCapability) => (
                <option key={capability.appType} value={capability.appType}>
                  {agentLabel(capability.appType, t)}
                </option>
              ))}
            </select>
          </label>
          <label className="flex w-full min-w-0 flex-col gap-1 text-xs text-muted-foreground sm:min-w-[220px] sm:flex-1">
            <span>{t("usage.task.title", { defaultValue: "Task title" })}</span>
            <TaskFilterCombobox
              label={t("usage.task.title", { defaultValue: "Task title" })}
              placeholder={t("usage.task.selectTitle", {
                defaultValue: "Select task title",
              })}
              searchPlaceholder={t("usage.task.searchTitle", {
                defaultValue: "Search task titles",
              })}
              loadingText={t("usage.task.loadingOptions", {
                defaultValue: "Loading options…",
              })}
              emptyText={t("usage.task.noFilterOptions", {
                defaultValue: "No matching options",
              })}
              clearText={t("usage.task.clearFilter", {
                defaultValue: "Clear selection",
              })}
              value={title}
              options={titleOptions}
              loading={filterOptionsQuery.isLoading}
              onChange={setTitle}
            />
          </label>
          <label className="flex w-full min-w-0 flex-col gap-1 text-xs text-muted-foreground sm:min-w-[220px] sm:flex-1">
            <span>{t("usage.task.project", { defaultValue: "Project" })}</span>
            <TaskFilterCombobox
              label={t("usage.task.project", { defaultValue: "Project" })}
              placeholder={t("usage.task.selectProject", {
                defaultValue: "Select project",
              })}
              searchPlaceholder={t("usage.task.searchProject", {
                defaultValue: "Search projects",
              })}
              loadingText={t("usage.task.loadingOptions", {
                defaultValue: "Loading options…",
              })}
              emptyText={t("usage.task.noFilterOptions", {
                defaultValue: "No matching options",
              })}
              clearText={t("usage.task.clearFilter", {
                defaultValue: "Clear selection",
              })}
              value={projectDir}
              options={projectOptions}
              loading={filterOptionsQuery.isLoading}
              onChange={setProjectDir}
            />
          </label>
        </div>
        {capabilitiesQuery.isError && (
          <p
            className="mt-2 text-xs text-amber-600 dark:text-amber-400"
            role="status"
          >
            {t("usage.task.capabilitiesUnavailable", {
              defaultValue: "Agent capability metadata is unavailable.",
            })}
          </p>
        )}
        {filterOptionsQuery.isError && (
          <p
            className="mt-2 text-xs text-amber-600 dark:text-amber-400"
            role="status"
          >
            {t("usage.task.filterOptionsUnavailable", {
              defaultValue: "Task title and project options are unavailable.",
            })}
          </p>
        )}
        {(isFetching || filterOptionsQuery.isFetching) && !isLoading && (
          <p className="mt-2 text-xs text-muted-foreground" role="status">
            {t("usage.task.refreshing", { defaultValue: "Refreshing…" })}
          </p>
        )}
      </div>

      {isLoading ? (
        <div
          className="h-[320px] animate-pulse rounded-lg border border-border/50 bg-muted/20"
          aria-busy="true"
          aria-label={t("usage.loading", { defaultValue: "Loading" })}
          role="status"
        />
      ) : isError ? (
        <div
          className="rounded-lg border border-destructive/30 bg-destructive/5 p-6 text-sm text-destructive"
          role="alert"
        >
          <div className="font-medium">
            {t("usage.task.loadError", {
              defaultValue: "Unable to load tasks",
            })}
          </div>
          <div className="mt-1 text-xs opacity-80">{String(error)}</div>
        </div>
      ) : (
        <>
          {dataStatus !== "ready" ? (
            <CodexReplayStatus status={dataStatus} t={t} />
          ) : null}
          {dataStatus === "ready" && data?.unattributedUsage ? (
            <UnattributedUsageSummary measure={data.unattributedUsage} t={t} />
          ) : null}
          {!codexRebuildingWithoutSnapshot &&
            (isWideLayout ? (
              <div
                data-testid="task-usage-table"
                className="overflow-hidden rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm"
              >
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>
                        {t("usage.task.task", { defaultValue: "Task" })}
                      </TableHead>
                      <TableHead>
                        {t("usage.task.total", {
                          defaultValue: "Derived total",
                        })}
                      </TableHead>
                      <TableHead className="text-right">
                        {t("usage.task.count", { defaultValue: "Count" })}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rows.length === 0 ? (
                      <TableRow>
                        <TableCell
                          colSpan={3}
                          className="h-32 text-center text-muted-foreground"
                        >
                          {t("usage.task.empty", {
                            defaultValue: "No root tasks match these filters.",
                          })}
                        </TableCell>
                      </TableRow>
                    ) : (
                      rows.map((row) => {
                        const key = rowKey(row);
                        return (
                          <TaskUsageRowView
                            key={key}
                            row={row}
                            expanded={expandedRows.includes(key)}
                            onToggle={() => toggleExpanded(key)}
                            t={t}
                          />
                        );
                      })
                    )}
                  </TableBody>
                </Table>
              </div>
            ) : (
              <div
                data-testid="task-usage-cards"
                className="grid min-w-0 gap-3"
              >
                {rows.length === 0 ? (
                  <div className="flex min-h-32 items-center justify-center rounded-lg border border-border/50 bg-card/40 p-4 text-center text-sm text-muted-foreground">
                    {t("usage.task.empty", {
                      defaultValue: "No root tasks match these filters.",
                    })}
                  </div>
                ) : (
                  rows.map((row) => {
                    const key = rowKey(row);
                    return (
                      <TaskUsageCardView
                        key={key}
                        row={row}
                        expanded={expandedRows.includes(key)}
                        onToggle={() => toggleExpanded(key)}
                        t={t}
                      />
                    );
                  })
                )}
              </div>
            ))}

          {!codexRebuildingWithoutSnapshot && (
            <div className="flex flex-wrap items-center justify-between gap-3 text-sm text-muted-foreground">
              <span>
                {t("usage.task.totalRecords", {
                  defaultValue: "{{total}} root tasks",
                  total,
                })}
              </span>
              <div className="flex items-center gap-2">
                <span aria-live="polite">
                  {totalPages > 0
                    ? t("usage.task.pageSummary", {
                        defaultValue: "Page {{page}} of {{pages}}",
                        page: page + 1,
                        pages: totalPages,
                      })
                    : t("usage.task.noPages", { defaultValue: "No pages" })}
                </span>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  aria-label={t("usage.previousPage", {
                    defaultValue: "Previous page",
                  })}
                  disabled={page === 0 || totalPages === 0}
                  onClick={() => setPage((current) => Math.max(0, current - 1))}
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  aria-label={t("usage.nextPage", {
                    defaultValue: "Next page",
                  })}
                  disabled={totalPages === 0 || page >= totalPages - 1}
                  onClick={() =>
                    setPage((current) =>
                      totalPages > 0
                        ? Math.min(totalPages - 1, current + 1)
                        : current,
                    )
                  }
                >
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
