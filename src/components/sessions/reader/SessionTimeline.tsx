import { memo } from "react";
import { ChevronRight, Copy } from "lucide-react";

import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { useReaderT } from "./i18n";
import {
  formatCost,
  formatDuration,
  formatTokens,
  type TurnSummary,
} from "./toolSummary";

export interface SessionTimelineProps {
  turnKey: string;
  summary: TurnSummary;
  /** 本轮 steps 总数（只有思考 / 说明、没有工具时用它当步数） */
  stepTotal: number;
  expanded: boolean;
  /** 折叠态下没列出的失败步骤数（超过 5 条时） */
  hiddenFailures: number;
  /** 这一轮没有最终回复（且没中断） */
  noFinal: boolean;
  onToggle: (turnKey: string, expanded: boolean) => void;
  onCopyTurn: (turnKey: string) => void;
}

/**
 * 执行过程的折叠摘要行（规则 1）：
 * `执行过程 · 14 步 · 9 个命令 · 改了 2 个文件 · 1 个失败 · ⏱ 2m14s`，为 0 的项不写。
 * 受控 expanded；失败步骤由行模型在下面逐条常显。
 */
export const SessionTimeline = memo(function SessionTimeline({
  turnKey,
  summary,
  stepTotal,
  expanded,
  hiddenFailures,
  noFinal,
  onToggle,
  onCopyTurn,
}: SessionTimelineProps) {
  const rt = useReaderT();
  const parts: { key: string; text: string; danger?: boolean }[] = [];
  if (summary.commandCount > 0) {
    parts.push({
      key: "cmd",
      text: rt("summaryCommands", { count: summary.commandCount }),
    });
  }
  if (summary.filesChanged > 0) {
    parts.push({
      key: "files",
      text: rt("summaryFiles", { count: summary.filesChanged }),
    });
  }
  if (summary.errorCount > 0) {
    parts.push({
      key: "err",
      text: rt("summaryErrors", { count: summary.errorCount }),
      danger: true,
    });
  }
  if (summary.durationMs !== undefined && summary.durationMs > 0) {
    parts.push({
      key: "dur",
      text: rt("summaryDuration", {
        duration: formatDuration(summary.durationMs),
      }),
    });
  }
  if (summary.tokens !== undefined) {
    parts.push({
      key: "tok",
      text: rt("summaryTokens", { tokens: formatTokens(summary.tokens) }),
    });
  }
  if (summary.costUsd !== undefined) {
    parts.push({ key: "cost", text: formatCost(summary.costUsd) });
  }
  const steps = summary.stepCount || stepTotal;
  const actionLabel = expanded ? rt("collapseTimeline") : rt("expandTimeline");

  return (
    <div className="group/timeline flex min-w-0 items-center gap-1">
      <button
        type="button"
        aria-expanded={expanded}
        aria-label={`${actionLabel}：${rt("timelineSummary", { steps })}`}
        onClick={() => onToggle(turnKey, expanded)}
        className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-control px-1.5 text-left text-caption text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      >
        <ChevronRight
          aria-hidden
          className={cn(
            "h-4 w-4 shrink-0 text-fg-3 transition-transform duration-150 motion-reduce:transition-none",
            expanded && "rotate-90",
          )}
        />
        <span className="min-w-0 truncate tabular-nums">
          <span className="font-medium text-fg-1">
            {rt("timelineSummary", { steps })}
          </span>
          {parts.map((part) => (
            <span key={part.key}>
              <span aria-hidden className="mx-1.5 text-fg-3">
                ·
              </span>
              <span className={part.danger ? "text-danger-text" : undefined}>
                {part.text}
              </span>
            </span>
          ))}
          {hiddenFailures > 0 && (
            <span className="ms-2 text-fg-3">
              {rt("moreFailures", { count: hiddenFailures })}
            </span>
          )}
          {noFinal && (
            <span className="ms-2 text-fg-3">{rt("noFinalReply")}</span>
          )}
        </span>
      </button>
      <HoverTip content={rt("copyTurn")}>
        <button
          type="button"
          aria-label={rt("copyTurn")}
          onClick={() => onCopyTurn(turnKey)}
          className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-control text-fg-3 opacity-0 transition-[opacity,color] hover:bg-subtle hover:text-fg-1 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring group-hover/timeline:opacity-100"
        >
          <Copy aria-hidden className="h-3.5 w-3.5" strokeWidth={1.5} />
        </button>
      </HoverTip>
    </div>
  );
});
