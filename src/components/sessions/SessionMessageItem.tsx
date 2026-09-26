import { memo, useMemo, useState } from "react";
import { ChevronDown, ChevronUp, Copy } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { SessionMessage } from "@/types";
import {
  createCollapsedMarkdownPreview,
  hasHighlightableMarkdownMatch,
  SessionMarkdown,
} from "./SessionMarkdown";
import {
  formatTimestamp,
  getRoleLabel,
  getRoleTone,
  getSearchSnippet,
  highlightText,
} from "./utils";

const COLLAPSE_THRESHOLD = 3000;
const COLLAPSED_LENGTH = 1500;

const getHiddenSearchSnippet = (
  content: string,
  collapsed: boolean,
  renderedAsMarkdown: boolean,
  displayContent: string,
  searchQuery?: string,
) => {
  if (!searchQuery) return null;

  // 折叠时，匹配可能整体位于折叠边界之后（含跨边界的情况）。
  if (collapsed) {
    const hiddenStart = Math.max(0, COLLAPSED_LENGTH - searchQuery.length + 1);
    const snippet = getSearchSnippet(content, searchQuery, hiddenStart);
    if (snippet) return snippet;
  }

  // Markdown 渲染会隐藏部分原文（链接 URL、标记符），还会把行内文本切成
  // 多个片段导致跨节点匹配无法高亮。此时展示原文上下文，避免“搜索命中
  // 却看不到匹配”的困惑。
  if (
    renderedAsMarkdown &&
    !hasHighlightableMarkdownMatch(displayContent, searchQuery)
  ) {
    return getSearchSnippet(content, searchQuery);
  }

  return null;
};

interface SessionMessageItemProps {
  message: SessionMessage;
  isActive: boolean;
  searchQuery?: string;
  onCopy: (content: string) => void;
}

export const SessionMessageItem = memo(function SessionMessageItem({
  message,
  isActive,
  searchQuery,
  onCopy,
}: SessionMessageItemProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  const role = message.role.toLowerCase();
  const shouldRenderMarkdown = role === "assistant";
  const isLong = message.content.length > COLLAPSE_THRESHOLD;
  const collapsed = isLong && !expanded;
  const displayContent = useMemo(() => {
    if (!collapsed) return message.content;
    return shouldRenderMarkdown
      ? createCollapsedMarkdownPreview(message.content, COLLAPSED_LENGTH)
      : `${message.content.slice(0, COLLAPSED_LENGTH)}…`;
  }, [collapsed, message.content, shouldRenderMarkdown]);
  const hiddenSearchSnippet = useMemo(
    () =>
      getHiddenSearchSnippet(
        message.content,
        collapsed,
        shouldRenderMarkdown,
        displayContent,
        searchQuery,
      ),
    [
      collapsed,
      displayContent,
      message.content,
      searchQuery,
      shouldRenderMarkdown,
    ],
  );

  return (
    <div
      className={cn(
        "rounded-lg border px-3 py-2.5 relative group transition-shadow min-w-0",
        role === "user"
          ? "bg-primary/5 border-primary/20 ml-8"
          : role === "assistant"
            ? "bg-blue-500/5 border-blue-500/20 mr-8"
            : "bg-muted/40 border-border/60",
        isActive && "ring-2 ring-primary ring-offset-2",
      )}
    >
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="absolute top-2 right-2 size-6 opacity-0 group-hover:opacity-100 transition-opacity"
            onClick={() => onCopy(message.content)}
          >
            <Copy className="size-3" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {t("sessionManager.copyMessage", {
            defaultValue: "复制内容",
          })}
        </TooltipContent>
      </Tooltip>
      <div className="flex items-center justify-between text-xs mb-1.5 pr-6">
        <span className={cn("font-semibold", getRoleTone(message.role))}>
          {getRoleLabel(message.role, t)}
        </span>
        {message.ts && (
          <span className="text-muted-foreground">
            {formatTimestamp(message.ts)}
          </span>
        )}
      </div>
      {shouldRenderMarkdown ? (
        <SessionMarkdown content={displayContent} searchQuery={searchQuery} />
      ) : (
        <div className="min-w-0 whitespace-pre-wrap break-words text-sm leading-relaxed [overflow-wrap:anywhere]">
          {searchQuery
            ? highlightText(displayContent, searchQuery)
            : displayContent}
        </div>
      )}
      {hiddenSearchSnippet && searchQuery && (
        <div className="mt-2 rounded-md border border-primary/20 bg-primary/5 px-2.5 py-2 text-xs">
          <div className="mb-1 font-medium text-muted-foreground">
            {t("sessionManager.hiddenSearchMatch", {
              defaultValue: "原文中的匹配",
            })}
          </div>
          <div className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
            {highlightText(hiddenSearchSnippet, searchQuery)}
          </div>
        </div>
      )}
      {isLong && (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
          className="flex items-center gap-1 mt-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {expanded ? (
            <>
              <ChevronUp className="size-3" />
              {t("sessionManager.collapseContent", {
                defaultValue: "收起",
              })}
            </>
          ) : (
            <>
              <ChevronDown className="size-3" />
              {t("sessionManager.expandContent", {
                defaultValue: "展开完整内容",
              })}
              <span className="text-muted-foreground/60">
                ({Math.round(message.content.length / 1000)}k)
              </span>
            </>
          )}
        </button>
      )}
    </div>
  );
});
