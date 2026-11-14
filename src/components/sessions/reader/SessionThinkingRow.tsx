import { memo } from "react";

import { cn } from "@/lib/utils";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import { GlyphCell, ROW_BUTTON, ROW_STATIC, stripBold } from "./rowStyles";
import { BlockText } from "./SessionStepDetails";
import { formatDuration, summarizeThinking } from "./toolSummary";
import type { ThinkingStep } from "./turns";

/** 字数显示：860 / 1.2k */
const formatChars = (count: number) =>
  count >= 1000 ? `${Number((count / 1000).toFixed(1))}k` : String(count);

export interface SessionThinkingRowProps {
  step: ThinkingStep;
  expanded: boolean;
  onToggle: (id: string, expanded: boolean) => void;
}

/**
 * 思考行（规则 8、决策 D6）：默认一行「✻ 思考中… · 1.2k 字 · 8s」，有 summary 直接显示；
 * 点开斜体全文，预览之外按需取全文。redacted 只显示「思考内容不可见」，不可展开。
 */
export const SessionThinkingRow = memo(function SessionThinkingRow({
  step,
  expanded,
  onToggle,
}: SessionThinkingRowProps) {
  const rt = useReaderT();
  const { style } = useReaderContext();
  const info = summarizeThinking(step.block);
  const label = rt(style.thinkingLabelKey);
  const summary = info.summary ? stripBold(info.summary) : "";
  const duration =
    info.durationMs !== undefined && info.durationMs > 0
      ? formatDuration(info.durationMs)
      : null;
  const glyph = <GlyphCell glyph={style.thinkingGlyph} className="text-fg-3" />;

  if (info.redacted || (!step.block.text && !step.block.full)) {
    return (
      <div className={ROW_STATIC}>
        {glyph}
        <span className="shrink-0 italic text-fg-2">{label}</span>
        <span className="min-w-0 truncate text-fg-3">
          {summary || rt("thinkingRedacted")}
        </span>
        {duration && (
          <span className="ms-auto shrink-0 tabular-nums text-fg-3">
            {duration}
          </span>
        )}
      </div>
    );
  }

  const chars = formatChars(info.chars);
  const meta = duration
    ? rt("thinkingMeta", { chars, duration })
    : rt("thinkingChars", { chars });
  const regionId = `reader-thinking-${step.id}`;

  return (
    <div className="min-w-0">
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={expanded ? regionId : undefined}
        onClick={() => onToggle(step.id, expanded)}
        className={ROW_BUTTON}
      >
        {glyph}
        <span className="shrink-0 italic text-fg-2">{label}</span>
        {summary && (
          <span className="min-w-0 truncate text-fg-2" title={summary}>
            {summary}
          </span>
        )}
        <span
          className={cn(
            "shrink-0 tabular-nums text-fg-3",
            summary ? "ms-auto" : "",
          )}
        >
          {summary ? meta : `· ${meta}`}
        </span>
      </button>
      {expanded && (
        <div
          id={regionId}
          role="region"
          aria-label={label}
          className="pb-2 pe-1.5 ps-8 pt-0.5"
        >
          <BlockText
            preview={step.block.text}
            truncated={Boolean(step.block.full)}
            full={step.block.full}
            previewLines={Number.MAX_SAFE_INTEGER}
            variant="thinking"
          />
        </div>
      )}
    </div>
  );
});
