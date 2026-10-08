import { useEffect, useState, type MouseEvent, type ReactNode } from "react";
import { Check, Clock, RefreshCw, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  cardRows,
  FAILED_LINE_KEY,
  countdownStr,
  formatRelativeTime,
  lineHint,
  resetText,
  type QuotaLine,
  type QuotaTone,
} from "./quotaRules";
import { QuotaBreakdownChevron, QuotaBreakdownRow } from "./QuotaBreakdown";

/**
 * 快用完只加深加粗、不用橙色：浅色模式的警告文字和可点击文字（主题橙）几乎同色，
 * 额度列本身又能点，橙色会被读成「这里可以点」。展开的额度条仍用琥珀色填充
 */
export const TONE_TEXT: Record<QuotaTone, string> = {
  normal: "text-fg-2",
  muted: "text-fg-3",
  warning: "font-medium text-fg-1",
  danger: "font-medium text-danger-text",
};

export const TONE_FILL: Record<QuotaTone, string> = {
  normal: "bg-chart-1",
  muted: "bg-chart-1",
  warning: "bg-warning",
  danger: "bg-danger",
};

/** 每 30 秒刷新一次「x 分钟前」和重置倒计时 */
export function useNow(active: boolean) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}

/** 一行里最早到点的那次重置（合并行几档各有各的，写最近的那个）；都没有或都过了为 null */
function rowCountdown(row: QuotaLine[], now: number) {
  let earliest: string | null = null;
  for (const line of row) {
    if (!countdownStr(line.resetsAt, now)) continue;
    if (!earliest || Date.parse(line.resetsAt!) < Date.parse(earliest)) {
      earliest = line.resetsAt!;
    }
  }
  return countdownStr(earliest, now);
}

/**
 * 额度行后面的「⏱ 2h30m」（卡片、授权中心）：定宽、内容靠左，几行的时钟上下对齐；
 * 这一行没有重置时间时留空占位，免得各行右边缘错开
 */
export function ResetSlot({
  countdown,
  className = "ms-1.5",
}: {
  countdown: string | null;
  className?: string;
}) {
  const { t } = useTranslation();
  return (
    <span
      className={cn(
        "inline-flex w-[52px] shrink-0 items-center gap-0.5 text-fg-3",
        className,
      )}
    >
      {countdown && (
        <>
          <Clock aria-hidden className="h-2.5 w-2.5 shrink-0" strokeWidth={2} />
          <span className="sr-only">
            {t("subscription.resetsIn", { time: countdown })}
          </span>
          <span aria-hidden>{countdown}</span>
        </>
      )}
    </span>
  );
}

/** 点一下重查后图标至少转这么久：请求常常不到一秒，太短了看不出来查过 */
export const MIN_REFRESH_SPIN_MS = 600;
/** 查完后 ✓ / ✗ 停留多久 */
export const REFRESH_RESULT_MS = 1000;

type RefreshPhase =
  | { kind: "idle" }
  | {
      kind: "spinning";
      since: number;
      /** onRefresh 返回的 Promise 有结果了（没返回 Promise 时一开始就算有） */
      settled: boolean;
      /** 这次请求本身失败了 */
      rejected: boolean;
    }
  | { kind: "succeeded" }
  | { kind: "failed" };

/** react-query refetch() 的结果：请求失败时 isError 为真（即使界面还留着上次成功的值） */
function isRejectedResult(result: unknown): boolean {
  return (
    typeof result === "object" &&
    result !== null &&
    (result as { isError?: unknown }).isError === true
  );
}

/**
 * 点击触发的重查：从点下去开始转，直到查完且满 MIN_REFRESH_SPIN_MS，再按结果换成 ✓ 或 ✗
 * 停一会儿。✗ 不能省：本来就没查到、再点又失败时，红字前后一模一样，没有它就像没点过。
 *
 * 成败两头看：额度列换成了「额度没查到」，或 refetch() 的结果是失败——后者管的是
 * 「保留上次成功值」窗口里的瞬时失败（界面照旧显示旧值，只看额度行会误打 ✓）。
 * 要等 Promise 有结果且 loading 落下（新的额度行已渲染）两件事都到，才下结论。
 *
 * 只认点击，后台轮询 / 窗口聚焦的重查不转（照旧只变淡），免得几张卡片一起闪
 */
function useClickRefreshFeedback(loading: boolean, failed: boolean) {
  const [phase, setPhase] = useState<RefreshPhase>({ kind: "idle" });
  useEffect(() => {
    if (phase.kind === "idle") return;
    if (phase.kind === "spinning") {
      if (loading || !phase.settled) return;
      const wait = Math.max(0, phase.since + MIN_REFRESH_SPIN_MS - Date.now());
      const outcome = failed || phase.rejected ? "failed" : "succeeded";
      const timer = setTimeout(() => setPhase({ kind: outcome }), wait);
      return () => clearTimeout(timer);
    }
    const timer = setTimeout(
      () => setPhase({ kind: "idle" }),
      REFRESH_RESULT_MS,
    );
    return () => clearTimeout(timer);
  }, [phase, loading, failed]);

  const start = (result: unknown) => {
    const since = Date.now();
    const thenable =
      typeof (result as PromiseLike<unknown> | undefined)?.then === "function";
    setPhase({ kind: "spinning", since, settled: !thenable, rejected: false });
    if (!thenable) return;
    const settle = (rejected: boolean) =>
      setPhase((current) =>
        current.kind === "spinning" && current.since === since
          ? { ...current, settled: true, rejected }
          : current,
      );
    Promise.resolve(result).then(
      (value) => settle(isRejectedResult(value)),
      () => settle(true),
    );
  };

  return { phase: phase.kind, start };
}

interface QuotaLinesProps {
  lines: QuotaLine[];
  max?: number;
  queriedAt?: number | null;
  loading?: boolean;
  /** 返回 refetch() 的 Promise 时，点击后的 ✓ / ✗ 也认请求本身的成败 */
  onRefresh?: () => unknown;
}

/**
 * 卡片右侧的额度列（v7）：最多两行、右对齐、平时灰色；档数多时第一行写窗口最短的那档，
 * 其余并成一行、每段各自上色（cardRows）。点一下重查，悬停说明每档、更新时间和重置时间。
 * 能重查时，悬停 / 键盘聚焦在第一行左边露出 ↻，点了之后它转到查完，再按结果换成 ✓ / ✗ 停一秒。
 */
export function QuotaLines({
  lines,
  max = 2,
  queriedAt,
  loading = false,
  onRefresh,
}: QuotaLinesProps) {
  const { t } = useTranslation();
  const now = useNow(Boolean(queriedAt) || lines.some((line) => line.resetsAt));
  const refresh = useClickRefreshFeedback(
    loading,
    lines.some((line) => line.key === FAILED_LINE_KEY),
  );
  const spinning = refresh.phase === "spinning";
  // 点过之后，鼠标移出 / 焦点离开之前不再露 ↻：否则 ✓ 一消失，还停在上面的鼠标又把 ↻ 叫出来
  const [quiet, setQuiet] = useState(false);
  // 拆开布局时刷新区不止一块，悬停改用状态记（见下）
  const [hovered, setHovered] = useState(false);
  const ResultIcon = refresh.phase === "failed" ? X : Check;
  const rows = cardRows(lines, max);
  if (rows.length === 0) return null;

  const title = [
    ...lines.map((line) => lineHint(t, line, now)),
    queriedAt
      ? t("quota.updatedAt", { time: formatRelativeTime(queriedAt, now, t) })
      : null,
    onRefresh ? t("quota.clickToRefresh") : null,
  ]
    .filter(Boolean)
    .join("\n");

  // 有一档带重置时间，每行后面就都留出倒计时那一格（v7 原来只放进悬停说明，看不到）
  const showReset = rows.some((row) => rowCountdown(row, now));
  const resetSlot = (row: QuotaLine[]) =>
    showReset ? <ResetSlot countdown={rowCountdown(row, now)} /> : null;

  const rowNodes = rows.map((row) => (
    <span
      key={row.map((line) => line.key).join("+")}
      className="flex max-w-full items-center justify-end"
    >
      {row.length === 1 ? (
        <span className={cn("min-w-0 truncate", TONE_TEXT[row[0].tone])}>
          {row[0].text}
        </span>
      ) : (
        <span className="min-w-0 truncate text-fg-2">
          {row.map((line, index) => (
            <span key={line.key}>
              {index > 0 && " · "}
              <span className={TONE_TEXT[line.tone]}>{line.short}</span>
            </span>
          ))}
        </span>
      )}
      {resetSlot(row)}
    </span>
  ));

  const className = cn(
    "flex shrink-0 flex-col items-end text-caption leading-[18px] tabular-nums whitespace-nowrap",
    // 文字仍是 136 宽，倒计时那格（52 + 间距）另加
    showReset ? "w-[194px]" : "w-[136px]",
    loading && !spinning && "opacity-60",
  );

  if (!onRefresh) {
    return (
      <span className={className} title={title}>
        {rowNodes}
      </span>
    );
  }

  // 用 aria-disabled 不用 disabled：禁用的按钮在 Chromium 里可能收不到 mouseleave，
  // 转圈时把鼠标移走，quiet 就解不开了
  const busy = loading || spinning;
  const refreshProps = {
    "aria-busy": busy,
    "aria-disabled": busy,
    onClick: (event: MouseEvent) => {
      event.stopPropagation();
      if (busy) return;
      setQuiet(true);
      refresh.start(onRefresh());
    },
    onMouseEnter: () => setHovered(true),
    onMouseLeave: () => {
      setHovered(false);
      setQuiet(false);
    },
    onBlur: () => setQuiet(false),
  };
  const split = rows.some((row) => row.some((line) => line.breakdown));
  const icon =
    refresh.phase === "succeeded" || refresh.phase === "failed" ? (
      <ResultIcon
        aria-hidden
        data-testid={`quota-refresh-${refresh.phase}`}
        className={cn(
          "h-[11px] w-[11px] shrink-0 motion-safe:animate-in motion-safe:fade-in-0 motion-safe:zoom-in-75",
          refresh.phase === "failed" ? "text-danger-text" : "text-fg-3",
        )}
        strokeWidth={2}
      />
    ) : (
      <RefreshCw
        aria-hidden
        data-testid="quota-refresh-icon"
        className={cn(
          "h-[11px] w-[11px] shrink-0 text-fg-3 group-hover:text-fg-2",
          spinning
            ? "motion-safe:animate-spin"
            : quiet
              ? "hidden"
              : split
                ? hovered
                  ? "block"
                  : "hidden group-focus-visible:block"
                : "hidden group-hover:block group-focus-visible:block",
        )}
        strokeWidth={1.75}
      />
    );

  if (split) {
    return (
      <SplitQuotaColumn
        rows={rows}
        className={className}
        title={title}
        icon={icon}
        showReset={showReset}
        resetSlot={resetSlot}
        refreshProps={refreshProps}
      />
    );
  }

  // ↻ 放在第一行左边：列是右对齐的，它出现 / 消失只占左侧空白，文字不挪位置
  const [firstRow, ...restRows] = rowNodes;
  return (
    <button
      type="button"
      className={cn(
        className,
        "group rounded-control text-end transition-opacity focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
      )}
      title={title}
      {...refreshProps}
    >
      <span className="flex max-w-full items-center justify-end gap-1">
        {icon}
        <span className="flex min-w-0 flex-col items-end">{firstRow}</span>
      </span>
      {restRows}
    </button>
  );
}

interface RefreshProps {
  "aria-busy": boolean;
  "aria-disabled": boolean;
  onClick: (event: MouseEvent) => void;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onBlur: () => void;
}

/**
 * 有一段能点开明细时（「每周 64% · 重置 3 次 ⌄」）：那一段单独做成下拉，其余照旧点了重查。
 * 按钮里不能再套按钮，所以整列拆成几块：第一块刷新是真按钮（键盘、读屏都认它），
 * 后面的刷新块只接鼠标点击，下拉段自己是按钮。
 * 下拉段独占一行且留了倒计时格时，⌄ 放进那一格，和上一行的 ⏱ 竖着对齐。
 */
function SplitQuotaColumn({
  rows,
  className,
  title,
  icon,
  showReset,
  resetSlot,
  refreshProps,
}: {
  rows: QuotaLine[][];
  className: string;
  title: string;
  icon: ReactNode;
  showReset: boolean;
  resetSlot: (row: QuotaLine[]) => ReactNode;
  refreshProps: RefreshProps;
}) {
  const { t } = useTranslation();
  const refreshText = rows
    .flat()
    .filter((line) => !line.breakdown)
    .map((line) => line.text)
    .join(", ");
  let mainPlaced = false;

  return (
    <div className={className}>
      {rows.map((row, rowIndex) => {
        const merged = row.length > 1;
        // 这一行只有下拉段：⌄ 占倒计时格的图标位，余下的宽度留空
        const chevronInSlot = showReset && !merged && Boolean(row[0].breakdown);
        // 连续的普通段并成一块刷新区，带明细的段单独成块
        const parts: { breakdown: boolean; lines: QuotaLine[] }[] = [];
        for (const line of row) {
          const last = parts[parts.length - 1];
          if (!line.breakdown && last && !last.breakdown) last.lines.push(line);
          else
            parts.push({ breakdown: Boolean(line.breakdown), lines: [line] });
        }
        let segment = 0;
        const segmentText = (line: QuotaLine) => {
          const node = (
            <span key={line.key}>
              {segment > 0 && " · "}
              <span className={TONE_TEXT[line.tone]}>
                {merged ? line.short : line.text}
              </span>
            </span>
          );
          segment += 1;
          return node;
        };

        return (
          <span
            key={row.map((line) => line.key).join("+")}
            className="flex max-w-full items-center justify-end text-fg-2"
          >
            {parts.map((part) => {
              if (part.breakdown) {
                const line = part.lines[0];
                const separator = segment > 0;
                segment += 1;
                return (
                  <span key={line.key} className="flex shrink-0 items-center">
                    {separator && <span>&nbsp;·&nbsp;</span>}
                    <QuotaBreakdownRow
                      line={line}
                      breakdown={line.breakdown!}
                      align="end"
                      className={cn(
                        "flex items-center gap-0.5",
                        TONE_TEXT[line.tone],
                      )}
                    >
                      {merged ? line.short : line.text}
                      {chevronInSlot ? (
                        <span className="ms-1 flex">
                          <QuotaBreakdownChevron />
                        </span>
                      ) : (
                        <QuotaBreakdownChevron />
                      )}
                    </QuotaBreakdownRow>
                    {/* 和上一行的「ms-1.5 + 52 宽倒计时格」等宽：gap-0.5 + ms-1 + ⌄ 12 + 40 */}
                    {chevronInSlot && <span className="w-10" />}
                  </span>
                );
              }
              const key = part.lines.map((line) => line.key).join("+");
              const text = part.lines.map(segmentText);
              if (!mainPlaced) {
                mainPlaced = true;
                return (
                  <button
                    key={key}
                    type="button"
                    title={title}
                    aria-label={`${refreshText} · ${t("quota.clickToRefresh")}`}
                    className="group flex min-w-0 items-center gap-1 rounded-control text-end focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    {...refreshProps}
                  >
                    {rowIndex === 0 && icon}
                    <span className="min-w-0 truncate">{text}</span>
                  </button>
                );
              }
              return (
                <span
                  key={key}
                  aria-hidden
                  title={title}
                  className="min-w-0 cursor-pointer truncate"
                  onClick={refreshProps.onClick}
                  onMouseEnter={refreshProps.onMouseEnter}
                  onMouseLeave={refreshProps.onMouseLeave}
                >
                  {text}
                </span>
              );
            })}
            {!chevronInSlot && resetSlot(row)}
          </span>
        );
      })}
    </div>
  );
}

interface QuotaBarsProps {
  /** 每档的名字和那一行（名字单独一列） */
  rows: { label: string; line: QuotaLine; note?: string }[];
  title?: ReactNode;
  queriedAt?: number | null;
  loading?: boolean;
  onRefresh?: () => void;
  footer?: ReactNode;
  className?: string;
}

const BAR_ROW_CLASS = "flex h-[18px] items-center gap-2 text-caption";

/** 展开的额度条（授权中心、多套餐展开）：条越短剩得越少 */
export function QuotaBars({
  rows,
  title,
  queriedAt,
  loading = false,
  onRefresh,
  footer,
  className,
}: QuotaBarsProps) {
  const { t } = useTranslation();
  const now = useNow(
    Boolean(queriedAt) || rows.some(({ line }) => line.resetsAt),
  );

  return (
    <div
      className={cn(
        "rounded-panel border border-border bg-surface px-3.5 py-2.5",
        className,
      )}
    >
      {(title || queriedAt || onRefresh) && (
        <div className="mb-1.5 flex items-center justify-between gap-2 text-caption">
          <span className="font-medium text-fg-2">{title}</span>
          <span className="flex items-center gap-1 text-fg-3">
            {queriedAt
              ? t("quota.updatedAt", {
                  time: formatRelativeTime(queriedAt, now, t),
                })
              : null}
            {onRefresh && (
              <HoverTip content={t("subscription.refresh")}>
                <button
                  type="button"
                  onClick={onRefresh}
                  disabled={loading}
                  className="rounded-control p-1 text-fg-3 hover:bg-subtle hover:text-fg-1 disabled:opacity-50"
                  aria-label={t("subscription.refresh")}
                >
                  <RefreshCw
                    className={cn("h-3 w-3", loading && "animate-spin")}
                  />
                </button>
              </HoverTip>
            )}
          </span>
        </div>
      )}
      <div className="flex flex-col gap-1">
        {rows.map(({ label, line, note }) => {
          const width = Number.isFinite(line.left)
            ? Math.max(0, Math.min(100, line.left))
            : 100;
          // 展开时地方够，重置时间直接写在数值后面（卡片上只在悬停说明里）
          const trailing = [resetText(t, line, now), note]
            .filter(Boolean)
            .join(" · ");
          const cells = (
            <>
              <span className="w-[72px] shrink-0 truncate text-fg-2">
                {label}
              </span>
              {line.caption ? (
                <span className="flex w-[120px] shrink-0 items-center gap-0.5 text-fg-3">
                  <span className="truncate">{line.caption}</span>
                  {line.breakdown && <QuotaBreakdownChevron />}
                </span>
              ) : (
                <span
                  role="meter"
                  aria-label={`${label}: ${line.value ?? line.text}`}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(width)}
                  className="relative h-1 w-[120px] shrink-0 overflow-hidden rounded-full bg-chart-grid"
                >
                  <span
                    className={cn(
                      "absolute inset-y-0 start-0 rounded-full",
                      TONE_FILL[line.tone],
                    )}
                    style={{ width: `${width}%` }}
                  />
                </span>
              )}
              <span
                className={cn(
                  "shrink-0 text-end tabular-nums whitespace-nowrap",
                  line.tone === "normal" ? "text-fg-1" : TONE_TEXT[line.tone],
                )}
              >
                {line.value ?? line.text}
              </span>
              {trailing && (
                <span className="min-w-0 truncate text-fg-3">{trailing}</span>
              )}
            </>
          );
          return line.breakdown ? (
            <QuotaBreakdownRow
              key={line.key}
              line={line}
              breakdown={line.breakdown}
              className={BAR_ROW_CLASS}
            >
              {cells}
            </QuotaBreakdownRow>
          ) : (
            <div key={line.key} className={BAR_ROW_CLASS} title={line.detail}>
              {cells}
            </div>
          );
        })}
      </div>
      {footer}
    </div>
  );
}
