import { formatMessageTime } from "../utils";
import type { AgentReaderStyle } from "./agentStyles";
import { describeEvent } from "./SessionEventRow";
import {
  formatDuration,
  formatMergedTitle,
  formatStepMeta,
  formatStepTitle,
  formatTokens,
  isFailureStatus,
  type StepMeta,
  type Translate,
} from "./toolSummary";
import type {
  SessionTurn,
  TimelineStep,
  TurnChapter,
  TurnEvent,
} from "./turns";

/**
 * 「复制这一轮 / 复制整段为 Markdown」（§3.4 旧功能落点）：提问原文、步骤写成
 * `- ⏺ Bash(cargo build) — 失败 · exit 101 · 12s` 列表、最终回复原文；思考默认不含（决策 D14）。
 */

export interface MarkdownExportOptions {
  style: AgentReaderStyle;
  appName: string;
  youLabel: string;
  projectDir?: string;
  t: Translate;
  /** 费用按用户选的显示币种写，和界面上看到的一致 */
  formatCost: (usd: number) => string;
  includeThinking?: boolean;
}

const metaText = (meta: StepMeta, { t, formatCost }: MarkdownExportOptions) => {
  const parts: string[] = [];
  if (isFailureStatus(meta.status)) parts.push(t(`status.${meta.status}`));
  if (meta.exitCode !== undefined) {
    parts.push(t("exitCode", { code: meta.exitCode }));
  }
  if (meta.diff) parts.push(`+${meta.diff.added} −${meta.diff.removed}`);
  if (meta.durationMs) parts.push(formatDuration(meta.durationMs));
  if (meta.tokens !== undefined) {
    parts.push(t("summaryTokens", { tokens: formatTokens(meta.tokens) }));
  }
  if (meta.costUsd !== undefined) parts.push(formatCost(meta.costUsd));
  return parts.length ? ` — ${parts.join(" · ")}` : "";
};

/** 列表项里的多行文本：续行缩进两格 */
const indentBody = (text: string, prefix = "  ") =>
  text
    .trim()
    .split("\n")
    .map((line, index) => (index === 0 ? line : `${prefix}${line}`))
    .join("\n");

const stepLines = (
  step: TimelineStep,
  options: MarkdownExportOptions,
  depth = 0,
): string[] => {
  const { style, t, projectDir } = options;
  const pad = "  ".repeat(depth);
  switch (step.kind) {
    case "tool": {
      const title = formatStepTitle(step, style, { projectDir, t });
      const meta = formatStepMeta(step, style);
      return [`${pad}- ${meta.glyph} ${title.text}${metaText(meta, options)}`];
    }
    case "merged": {
      const title = formatMergedTitle(step);
      const meta = formatStepMeta(step, style);
      return [
        `${pad}- ${meta.glyph} ${title.text}${metaText(meta, options)}`,
        ...step.children.flatMap((child) =>
          stepLines(child, options, depth + 1),
        ),
      ];
    }
    case "thinking": {
      if (!options.includeThinking) return [];
      const label = t(style.thinkingLabelKey);
      const body = step.block.text || step.block.summary || "";
      const glyph = style.thinkingGlyph ? `${style.thinkingGlyph} ` : "";
      return [
        `${pad}- ${glyph}${label}${body ? `: ${indentBody(body, `${pad}  `)}` : ""}`,
      ];
    }
    case "note":
      return [`${pad}- ${indentBody(step.text, `${pad}  `)}`];
    case "image":
      return [`${pad}- [${step.image.alt || t("image.alt", { index: 1 })}]`];
    case "event": {
      const { label, detail } = describeEvent(step.block, t, step.body);
      return [`${pad}- ${label}${detail ? `: ${indentBody(detail)}` : ""}`];
    }
  }
};

const eventLine = (event: TurnEvent, t: Translate) => {
  const { label, detail } = describeEvent(event.block, t, event.body);
  const url = event.block.kind === "pr_link" && event.block.url;
  return `> ${label}${url ? ` (${url})` : ""}${detail ? `\n>\n> ${detail.replace(/\n/g, "\n> ")}` : ""}`;
};

/** 章节写成小标题，摘要作为引用跟在后面 */
const chapterSection = (chapter: TurnChapter) =>
  `### ${chapter.title}${chapter.summary ? `\n\n> ${chapter.summary}` : ""}`;

const heading = (label: string, ts?: number) =>
  `**${label}**${ts ? ` ${formatMessageTime(ts)}` : ""}`;

export const turnToMarkdown = (
  turn: SessionTurn,
  options: MarkdownExportOptions,
): string => {
  const { t } = options;
  const sections: string[] = [];
  turn.leadingEvents.forEach((event) => sections.push(eventLine(event, t)));
  if (turn.question) {
    const images = turn.question.images.map(
      (image, index) =>
        `[${image.alt || t("image.alt", { index: index + 1 })}]`,
    );
    sections.push(
      [
        heading(options.youLabel, turn.question.ts),
        turn.question.text,
        images.join(" "),
      ]
        .filter(Boolean)
        .join("\n\n"),
    );
  }
  // 步骤列表在章节处断开：章节小标题插在它后面第一个步骤之前
  let lines: string[] = [];
  const flush = () => {
    if (lines.length) sections.push(lines.join("\n"));
    lines = [];
  };
  const pushChapters = (stepId?: string) =>
    turn.chapters
      .filter((chapter) => chapter.beforeStepId === stepId)
      .forEach((chapter) => {
        flush();
        sections.push(chapterSection(chapter));
      });
  turn.steps.forEach((step) => {
    pushChapters(step.id);
    lines.push(...stepLines(step, options));
  });
  pushChapters();
  flush();
  if (turn.final) {
    sections.push(
      `${heading(options.appName, turn.final.ts)}\n\n${turn.final.text}`,
    );
  }
  turn.trailingEvents.forEach((event) => sections.push(eventLine(event, t)));
  return sections.join("\n\n");
};

export const transcriptToMarkdown = (
  title: string,
  turns: SessionTurn[],
  options: MarkdownExportOptions,
): string => {
  const body = turns
    .map((turn) => turnToMarkdown(turn, options))
    .filter(Boolean)
    .join("\n\n---\n\n");
  return `# ${title}\n\n${body}\n`;
};
