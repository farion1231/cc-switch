import { memo, type ReactNode } from "react";

import { cn } from "@/lib/utils";
import type { StatusTone, StepMeta } from "./toolSummary";
import { formatCost, formatDuration, formatTokens } from "./toolSummary";
import { useReaderT } from "./i18n";

/**
 * 阅读页各行共用的样式片段：统一行高 32、glyph 列宽 16、状态色语义（§6.6「统一的部分」）。
 */

/** 可展开的一行：步骤、思考、折叠摘要 */
export const ROW_BUTTON =
  "group/row flex h-8 w-full min-w-0 items-center gap-2 rounded-control px-1.5 text-left text-caption transition-colors hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring";

/** 不可展开的一行（redacted 思考等） */
export const ROW_STATIC =
  "flex h-8 w-full min-w-0 items-center gap-2 px-1.5 text-caption";

export const TONE_CLASS: Record<StatusTone, string> = {
  success: "text-success-text",
  danger: "text-danger-text",
  warning: "text-warning-text",
  muted: "text-fg-3",
  neutral: "text-fg-2",
};

/** 行首符号列：固定宽度保证各行标题对齐；符号只是装饰，读屏靠文字 */
export const GlyphCell = ({
  glyph,
  className,
}: {
  glyph: string;
  className?: string;
}) => (
  <span
    aria-hidden
    className={cn(
      "inline-flex w-4 shrink-0 select-none justify-center font-mono leading-none",
      className,
    )}
  >
    {glyph}
  </span>
);

const Dot = () => (
  <span aria-hidden className="text-fg-3">
    ·
  </span>
);

/** 步骤元信息：exit N · 12s · 88 行 / +3 −1 · 28.5k tok · $0.012（tabular-nums） */
export const StepMetaText = memo(function StepMetaText({
  meta,
  className,
}: {
  meta: StepMeta;
  className?: string;
}) {
  const rt = useReaderT();
  const parts: ReactNode[] = [];
  if (meta.exitCode !== undefined) {
    parts.push(
      <span
        key="exit"
        className={meta.exitCode === 0 ? undefined : "text-danger-text"}
      >
        {rt("exitCode", { code: meta.exitCode })}
      </span>,
    );
  }
  if (meta.diff) {
    parts.push(
      <span key="diff">
        <span className="text-success-text">+{meta.diff.added}</span>{" "}
        <span className="text-danger-text">−{meta.diff.removed}</span>
      </span>,
    );
  }
  if (meta.durationMs !== undefined && meta.durationMs > 0) {
    parts.push(<span key="dur">{formatDuration(meta.durationMs)}</span>);
  }
  if (meta.lineCount !== undefined) {
    parts.push(
      <span key="lines">{rt("lines", { count: meta.lineCount })}</span>,
    );
  }
  if (meta.tokens !== undefined) {
    parts.push(
      <span key="tok">
        {rt("summaryTokens", { tokens: formatTokens(meta.tokens) })}
      </span>,
    );
  }
  if (meta.costUsd !== undefined) {
    parts.push(<span key="cost">{formatCost(meta.costUsd)}</span>);
  }
  if (parts.length === 0) return null;
  return (
    <span
      className={cn(
        "flex shrink-0 items-center gap-1 whitespace-nowrap tabular-nums text-fg-3",
        className,
      )}
    >
      {parts.flatMap((part, index) =>
        index === 0 ? [part] : [<Dot key={`dot${index}`} />, part],
      )}
    </span>
  );
});

/** 去掉 Codex reasoning 摘要里的 `**粗体**` 标记 */
export const stripBold = (text: string) =>
  text.replace(/\*\*(.+?)\*\*/g, "$1").trim();
