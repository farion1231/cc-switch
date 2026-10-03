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
  formatTimestamp,
  getRoleLabel,
  getRoleTone,
  highlightText,
} from "./utils";

const COLLAPSE_THRESHOLD = 3000;
const COLLAPSED_LENGTH = 1500;

// 工具消息在会话里占了绝大部分篇幅，而它们多是命令输出、文件内容与日志，
// 扫读对话时并不需要看全文，只要知道「这里调了个工具、大致返回了什么」。
// 按行折叠而不是按字符：工具输出的排版本来就是按行的，同样的字符数可能是
// 一行也可能是十几行，按行才能得到可预期的视觉高度；字符上限只是单行超长
// 时的兜底。
const TOOL_PREVIEW_LINES = 2;
const TOOL_PREVIEW_MAX_CHARS = 300;

const clampToLines = (content: string, maxLines: number, maxChars: number) => {
  const lines = content.split("\n");
  // 尾部空行不带信息量，去掉可以让预览更紧凑
  const head = lines.slice(0, maxLines).join("\n").trimEnd();
  if (head.length > maxChars) {
    return { text: head.slice(0, maxChars), truncated: true };
  }
  // 命令输出常以换行收尾，被裁掉的部分若只剩空白，展开后并不会多出内容，
  // 这类消息不应该显示省略号和展开按钮
  const rest = lines.slice(maxLines).join("\n");
  return { text: head, truncated: rest.trim().length > 0 };
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

  const isTool = message.role.toLowerCase() === "tool";
  const toolPreview = useMemo(
    () =>
      isTool
        ? clampToLines(
            message.content,
            TOOL_PREVIEW_LINES,
            TOOL_PREVIEW_MAX_CHARS,
          )
        : { text: "", truncated: false },
    [isTool, message.content],
  );
  const isLong = isTool
    ? toolPreview.truncated
    : message.content.length > COLLAPSE_THRESHOLD;
  const hasSearchMatch =
    isLong &&
    !expanded &&
    !!searchQuery &&
    message.content.toLowerCase().includes(searchQuery.toLowerCase());
  const collapsed = isLong && !expanded && !hasSearchMatch;
  const displayContent = collapsed
    ? isTool
      ? `${toolPreview.text}…`
      : message.content.slice(0, COLLAPSED_LENGTH) + "…"
    : message.content;

  return (
    <div
      className={cn(
        "rounded-lg border px-3 py-2.5 relative group transition-shadow min-w-0",
        message.role.toLowerCase() === "user"
          ? "bg-primary/5 border-primary/20 ml-8"
          : message.role.toLowerCase() === "assistant"
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
      <div
        className={cn(
          "whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-sm leading-relaxed min-w-0",
          // 预览取的是前两个源码行，其中的长行折行后视觉上仍可能占三四行；
          // line-clamp 兜住这种情况，让每条折叠的工具消息高度一致。
          isTool && collapsed && "line-clamp-2",
        )}
      >
        {searchQuery
          ? highlightText(displayContent, searchQuery)
          : displayContent}
      </div>
      {isLong && !hasSearchMatch && (
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
                (
                {isTool
                  ? t("sessionManager.lineCount", {
                      lines: message.content.trimEnd().split("\n").length,
                      defaultValue: "{{lines}} 行",
                    })
                  : `${Math.round(message.content.length / 1000)}k`}
                )
              </span>
            </>
          )}
        </button>
      )}
    </div>
  );
});
