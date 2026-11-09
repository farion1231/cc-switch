import { memo, useMemo } from "react";
import { ChevronRight } from "lucide-react";

import { cn } from "@/lib/utils";
import { highlightText } from "../utils";
import type { AgentReaderStyle } from "./agentStyles";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import { GlyphCell, ROW_BUTTON, StepMetaText, TONE_CLASS } from "./rowStyles";
import { SessionEventRow } from "./SessionEventRow";
import { SessionImage } from "./SessionImage";
import { SessionNoteRow } from "./SessionNoteRow";
import { SessionStepDetails } from "./SessionStepDetails";
import { SessionThinkingRow } from "./SessionThinkingRow";
import {
  formatMergedTitle,
  formatStepMeta,
  formatStepTitle,
  shortenPath,
  type StepTitle,
  type Translate,
} from "./toolSummary";
import type { MergedStep, TimelineStep, ToolStep } from "./turns";

export type StepToggle = (id: string, expanded: boolean) => void;

/** 标题：动作（品牌字面）+ 对象；call 格式包括号，icon 格式前置符号 */
const TitleText = ({
  title,
  style,
  query,
}: {
  title: StepTitle;
  style: AgentReaderStyle;
  query?: string;
}) => {
  const target = query ? highlightText(title.target, query) : title.target;
  const targetClass = cn("text-fg-1", style.monoTitles ? "" : "font-mono");
  return (
    <span
      className={cn(
        "min-w-0 truncate",
        style.monoTitles ? "font-mono" : "font-sans",
      )}
    >
      {title.icon && <span className="me-1.5 text-fg-3">{title.icon}</span>}
      <span className="font-semibold text-fg-1">{title.verb}</span>
      {title.target &&
        (style.titleFormat === "call" ? (
          <>
            <span className="text-fg-3">(</span>
            <span className={targetClass}>{target}</span>
            <span className="text-fg-3">)</span>
          </>
        ) : (
          <>
            {" "}
            <span className={targetClass}>{target}</span>
          </>
        ))}
    </span>
  );
};

const mergedDetail = (step: MergedStep, rt: Translate) => {
  if (step.mergeKind !== "explored") return undefined;
  const title = formatMergedTitle(step);
  return rt("merged.explored", {
    files: title.reads,
    searches: title.searches,
  });
};

interface ToolRowProps {
  step: ToolStep | MergedStep;
  expanded: boolean;
  previewLines: number;
  onToggle: StepToggle;
}

/** 工具步骤 / 合并步骤的一行：glyph、标题、detail、状态、元信息；点开看详情 */
const ToolRow = memo(function ToolRow({
  step,
  expanded,
  previewLines,
  onToggle,
}: ToolRowProps) {
  const rt = useReaderT();
  const { style, projectDir, searchQuery } = useReaderContext();
  const title = useMemo(
    () =>
      step.kind === "merged"
        ? formatMergedTitle(step)
        : formatStepTitle(step, style, { projectDir, t: rt }),
    [projectDir, rt, step, style],
  );
  const meta = useMemo(() => formatStepMeta(step, style), [step, style]);
  let detail = step.kind === "merged" ? mergedDetail(step, rt) : title.detail;
  if (detail && step.kind === "tool") {
    // workdir 之类的路径缩短；就是项目目录本身时不再重复
    const short = shortenPath(detail, projectDir);
    detail = short === "." ? undefined : short;
  }
  const statusText = rt(`status.${step.status}`);
  const regionId = `reader-step-${step.id}`;
  const fullTitle = [title.text, detail].filter(Boolean).join(" · ");
  const failure = step.status === "error" || step.status === "interrupted";

  return (
    <div className="min-w-0">
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={expanded && step.kind === "tool" ? regionId : undefined}
        aria-label={
          expanded
            ? `${rt("stepCollapse")} ${fullTitle}（${statusText}）`
            : rt("stepExpand", { title: `${fullTitle}（${statusText}）` })
        }
        title={fullTitle}
        onClick={() => onToggle(step.id, expanded)}
        className={ROW_BUTTON}
      >
        <GlyphCell glyph={meta.glyph} className={TONE_CLASS[meta.tone]} />
        <TitleText title={title} style={style} query={searchQuery} />
        {detail && (
          <span className="hidden min-w-0 max-w-[40%] shrink truncate text-fg-3 sm:inline">
            {detail}
          </span>
        )}
        <span className="ms-auto flex shrink-0 items-center gap-1.5 ps-2">
          {failure && (
            <span className={cn("font-medium", TONE_CLASS[meta.tone])}>
              {statusText}
            </span>
          )}
          <StepMetaText meta={meta} />
          <ChevronRight
            aria-hidden
            className={cn(
              "h-3.5 w-3.5 text-fg-3 opacity-0 transition-[opacity,transform] group-hover/row:opacity-100 group-focus-visible/row:opacity-100 motion-reduce:transition-none",
              expanded && "rotate-90 opacity-100",
            )}
          />
        </span>
      </button>
      {expanded && step.kind === "tool" && (
        <SessionStepDetails
          id={regionId}
          label={fullTitle}
          step={step}
          style={style}
          previewLines={previewLines}
        />
      )}
    </div>
  );
});

export interface SessionStepProps {
  step: TimelineStep;
  expanded: boolean;
  previewLines: number;
  onToggle: StepToggle;
}

/** 执行过程里的一步：按类型分派到工具行 / 思考 / 说明 / 图片 / 事件 */
export const SessionStep = memo(function SessionStep({
  step,
  expanded,
  previewLines,
  onToggle,
}: SessionStepProps) {
  const rt = useReaderT();
  switch (step.kind) {
    case "tool":
    case "merged":
      return (
        <ToolRow
          step={step}
          expanded={expanded}
          previewLines={previewLines}
          onToggle={onToggle}
        />
      );
    case "thinking":
      return (
        <SessionThinkingRow
          step={step}
          expanded={expanded}
          onToggle={onToggle}
        />
      );
    case "note":
      return (
        <SessionNoteRow step={step} expanded={expanded} onToggle={onToggle} />
      );
    case "image":
      return (
        <div className="flex min-w-0 gap-2 px-1.5 py-1.5">
          <GlyphCell glyph="▣" className="h-6 text-fg-3" />
          <SessionImage
            image={step.image}
            variant="result"
            alt={step.image.alt || rt("image.alt", { index: 1 })}
          />
        </div>
      );
    case "event":
      return (
        <SessionEventRow block={step.block} body={step.body} variant="inline" />
      );
  }
});
