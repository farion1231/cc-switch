import { memo, useCallback, useMemo, useState } from "react";
import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";

import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { formatClock, highlightText } from "../utils";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import {
  COLLAPSE_THRESHOLD,
  COLLAPSED_LENGTH,
  CollapseToggle,
  rowIconButton,
} from "./SessionQuestion";
import { SessionImage, SessionImageGrid } from "./SessionImage";
import {
  createCollapsedMarkdownPreview,
  hasHighlightableMarkdownMatch,
  SessionMarkdown,
} from "./SessionMarkdown";
import { formatCost, formatTokens } from "./toolSummary";
import type { TurnFinal } from "./turns";

const SNIPPET_RADIUS = 60;

/** 原文中命中处前后各 60 字（命中藏在链接地址等不可见位置时兜底展示） */
const rawSnippets = (text: string, query: string, limit = 3) => {
  const haystack = text.toLowerCase();
  const needle = query.trim().toLowerCase();
  const snippets: string[] = [];
  let at = haystack.indexOf(needle);
  while (at >= 0 && snippets.length < limit) {
    const start = Math.max(0, at - SNIPPET_RADIUS);
    const end = Math.min(text.length, at + needle.length + SNIPPET_RADIUS);
    snippets.push(
      `${start > 0 ? "…" : ""}${text.slice(start, end).replace(/\s+/g, " ")}${end < text.length ? "…" : ""}`,
    );
    at = haystack.indexOf(needle, end);
  }
  return snippets;
};

export interface SessionFinalReplyProps {
  final: TurnFinal;
  /** 查找命中：长回复自动展开（规则 11） */
  forceExpanded: boolean;
}

/**
 * 最终回复：角色行（glyph + Agent 名 + 时间 + 模型）、Markdown 正文（视觉重点，无底色）、
 * 超过 3000 字折叠到 1500 字、复制。
 */
export const SessionFinalReply = memo(function SessionFinalReply({
  final,
  forceExpanded,
}: SessionFinalReplyProps) {
  const { t } = useTranslation();
  const rt = useReaderT();
  const { style, appName, projectDir, searchQuery, onCopy } =
    useReaderContext();
  const [expanded, setExpanded] = useState(false);
  const text = final.text;
  const long = text.length > COLLAPSE_THRESHOLD;
  const open = expanded || forceExpanded;
  const shown =
    long && !open
      ? createCollapsedMarkdownPreview(text, COLLAPSED_LENGTH)
      : text;

  const hiddenMatches = useMemo(() => {
    const query = searchQuery?.trim();
    if (!query || !text.toLowerCase().includes(query.toLowerCase())) return [];
    if (hasHighlightableMarkdownMatch(text, query, projectDir)) return [];
    return rawSnippets(text, query);
  }, [projectDir, searchQuery, text]);

  const renderLocalImage = useCallback(
    (path: string, alt: string) => (
      <SessionImage
        image={{
          source: { kind: "local_file", path },
          mediaType: "",
          size: 0,
          alt,
        }}
        alt={alt || path}
        variant="result"
        className="my-1"
      />
    ),
    [],
  );

  const cost = style.showStepCost ? final.cost : undefined;

  return (
    <div className="group/final min-w-0 px-1.5 py-1">
      <div className="flex h-6 min-w-0 items-center gap-1.5 text-caption">
        {style.assistantGlyph && (
          <span
            aria-hidden
            className="inline-flex w-4 shrink-0 justify-center font-mono text-[var(--reader-accent)]"
          >
            {style.assistantGlyph}
          </span>
        )}
        <span className="shrink-0 font-semibold text-fg-1">{appName}</span>
        {final.ts ? (
          <time
            dateTime={new Date(final.ts).toISOString()}
            title={new Date(final.ts).toLocaleString()}
            className="shrink-0 tabular-nums text-fg-3"
          >
            {formatClock(final.ts)}
          </time>
        ) : null}
        {final.model && (
          <span className="min-w-0 truncate font-mono text-fg-3">
            · {final.model}
          </span>
        )}
        {cost && (cost.tokens !== undefined || cost.costUsd !== undefined) && (
          <span className="shrink-0 tabular-nums text-fg-3">
            {cost.tokens !== undefined &&
              ` · ${rt("summaryTokens", { tokens: formatTokens(cost.tokens) })}`}
            {cost.costUsd !== undefined && ` · ${formatCost(cost.costUsd)}`}
          </span>
        )}
        <div className="flex-1" />
        <HoverTip
          content={t("sessionManager.copyShort", { defaultValue: "复制" })}
        >
          <button
            type="button"
            aria-label={t("sessionManager.copyReply", {
              defaultValue: "复制这条回复",
            })}
            onClick={() =>
              onCopy(
                text,
                t("sessionManager.messageCopied", {
                  defaultValue: "已复制这条消息",
                }),
              )
            }
            className={cn(rowIconButton, "-me-1 group-hover/final:opacity-100")}
          >
            <Copy aria-hidden className="h-3.5 w-3.5" strokeWidth={1.5} />
          </button>
        </HoverTip>
      </div>
      <div className={cn("mt-1", style.assistantGlyph && "ps-[22px]")}>
        {text && (
          <SessionMarkdown
            content={shown}
            searchQuery={searchQuery}
            projectDir={projectDir}
            renderLocalImage={renderLocalImage}
          />
        )}
        {long && !forceExpanded && (
          <CollapseToggle
            expanded={expanded}
            total={text.length}
            onToggle={() => setExpanded((value) => !value)}
          />
        )}
        <SessionImageGrid
          images={final.images}
          variant="result"
          className="mt-2"
        />
        {hiddenMatches.length > 0 && searchQuery && (
          <div className="mt-2 rounded-[8px] border border-border px-3 py-2 text-caption text-fg-2">
            <div className="mb-1 font-medium text-fg-3">{rt("rawMatch")}</div>
            {hiddenMatches.map((snippet, index) => (
              <p
                key={index}
                className="m-0 break-words font-mono [overflow-wrap:anywhere]"
              >
                {highlightText(snippet, searchQuery.trim())}
              </p>
            ))}
          </div>
        )}
      </div>
    </div>
  );
});
