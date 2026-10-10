import { formatMessageTime } from "../utils";
import type { AgentReaderStyle } from "./agentStyles";
import { describeEvent } from "./SessionEventRow";
import {
  formatCost,
  formatDuration,
  formatMergedTitle,
  formatStepMeta,
  formatStepTitle,
  formatTokens,
  isFailureStatus,
  type StepMeta,
  type Translate,
} from "./toolSummary";
import type { SessionTurn, TimelineStep, TurnEvent } from "./turns";

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
  includeThinking?: boolean;
}

const metaText = (meta: StepMeta, t: Translate) => {
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
      return [`${pad}- ${meta.glyph} ${title.text}${metaText(meta, t)}`];
    }
    case "merged": {
      const title = formatMergedTitle(step);
      const meta = formatStepMeta(step, style);
      return [
        `${pad}- ${meta.glyph} ${title.text}${metaText(meta, t)}`,
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
  const steps = turn.steps.flatMap((step) => stepLines(step, options));
  if (steps.length) sections.push(steps.join("\n"));
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
