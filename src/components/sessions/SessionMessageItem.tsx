import { memo, useState } from "react";
import { ChevronDown, ChevronRight, Copy, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  formatCharCount,
  formatClock,
  formatToolNames,
  highlightText,
  splitInlineCode,
  splitMarkdownBlocks,
  type SessionChatItem,
  type SessionToolItem,
} from "./utils";

const COLLAPSE_THRESHOLD = 3000;
const COLLAPSED_LENGTH = 1500;
const TOOL_OUTPUT_PREVIEW_LINES = 12;

const iconButton =
  "inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-control text-fg-3 transition-colors hover:bg-subtle hover:text-fg-1";

function InlineText({ text, query }: { text: string; query?: string }) {
  return (
    <>
      {splitInlineCode(text).map((part, index) =>
        part.code ? (
          <code
            key={index}
            className="rounded-[4px] border border-border bg-subtle px-1 font-mono text-caption"
          >
            {query ? highlightText(part.text, query) : part.text}
          </code>
        ) : (
          <span key={index}>
            {query ? highlightText(part.text, query) : part.text}
          </span>
        ),
      )}
    </>
  );
}

function CodeBlock({
  lang,
  code,
  query,
  onCopy,
}: {
  lang: string;
  code: string;
  query?: string;
  onCopy: (text: string, message: string) => void;
}) {
  const { t } = useTranslation();
  const label = lang || "text";
  return (
    <div className="my-1 overflow-hidden rounded-[8px] border border-border bg-subtle">
      <div className="flex h-[30px] items-center justify-between border-b border-border pe-1 ps-3">
        <span className="font-mono text-caption text-fg-2">{label}</span>
        <button
          type="button"
          aria-label={t("sessionManager.copyCode", {
            defaultValue: "复制代码",
          })}
          onClick={() =>
            onCopy(
              code,
              t("sessionManager.codeCopied", { defaultValue: "已复制代码" }),
            )
          }
          className="h-6 rounded-control px-2 text-caption text-fg-2 transition-colors hover:bg-selected hover:text-fg-1"
        >
          {t("sessionManager.copyShort", { defaultValue: "复制" })}
        </button>
      </div>
      <div
        tabIndex={0}
        role="region"
        aria-label={t("sessionManager.codeRegion", {
          defaultValue: "{{lang}} 代码",
          lang: label,
        })}
        className="overflow-x-auto px-3 py-2 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      >
        <pre className="m-0 whitespace-pre font-mono text-caption text-fg-1">
          {query ? highlightText(code, query) : code}
        </pre>
      </div>
    </div>
  );
}

interface SessionMessageItemProps {
  item: SessionChatItem;
  roleLabel: string;
  isActive: boolean;
  searchQuery?: string;
  onCopy: (text: string, message: string) => void;
}

export const SessionMessageItem = memo(function SessionMessageItem({
  item,
  roleLabel,
  isActive,
  searchQuery,
  onCopy,
}: SessionMessageItemProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const role = item.role.toLowerCase();
  const isUser = role === "user";

  const isLong = item.content.length > COLLAPSE_THRESHOLD;
  const hasSearchMatch =
    isLong &&
    !expanded &&
    !!searchQuery &&
    item.content.toLowerCase().includes(searchQuery.toLowerCase());
  const collapsed = isLong && !expanded && !hasSearchMatch;
  const displayContent = collapsed
    ? item.content.slice(0, COLLAPSED_LENGTH) + "…"
    : item.content;
  const blocks = splitMarkdownBlocks(displayContent);

  return (
    <div
      className={cn(
        "flex min-w-0 flex-col gap-1 rounded-panel px-3.5 transition-shadow",
        isUser ? "bg-subtle py-2.5" : role === "system" ? "py-1" : "",
        isActive && "ring-2 ring-ring ring-offset-2 ring-offset-app",
      )}
    >
      <div className="flex h-6 items-center gap-2">
        <span className="text-caption font-semibold text-fg-1">
          {roleLabel}
        </span>
        {item.ts ? (
          <span
            className="text-caption tabular-nums text-fg-3"
            title={new Date(item.ts).toLocaleString()}
          >
            {formatClock(item.ts)}
          </span>
        ) : null}
        <div className="flex-1" />
        <HoverTip
          content={t("sessionManager.copyShort", { defaultValue: "复制" })}
        >
          <button
            type="button"
            aria-label={
              isUser
                ? t("sessionManager.copyQuestion", {
                    defaultValue: "复制这条提问",
                  })
                : t("sessionManager.copyReply", {
                    defaultValue: "复制这条回复",
                  })
            }
            onClick={() =>
              onCopy(
                item.content,
                t("sessionManager.messageCopied", {
                  defaultValue: "已复制这条消息",
                }),
              )
            }
            className={cn(iconButton, "-me-1")}
          >
            <Copy className="h-3.5 w-3.5" strokeWidth={1.5} />
          </button>
        </HoverTip>
      </div>
      <div
        className={cn(
          "min-w-0 text-body text-fg-1",
          role === "system" && "text-fg-2",
        )}
      >
        {blocks.map((block, index) =>
          block.type === "code" ? (
            <CodeBlock
              key={index}
              lang={block.lang}
              code={block.code}
              query={searchQuery}
              onCopy={onCopy}
            />
          ) : (
            <p
              key={index}
              className="m-0 whitespace-pre-wrap break-words [overflow-wrap:anywhere] [&:not(:first-child)]:mt-2"
            >
              <InlineText text={block.text} query={searchQuery} />
            </p>
          ),
        )}
      </div>
      {isLong && !hasSearchMatch && (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
          className="mt-1 inline-flex items-center gap-1 self-start text-caption text-fg-2 transition-colors hover:text-fg-1"
        >
          {expanded ? (
            t("sessionManager.collapseContent", { defaultValue: "收起" })
          ) : (
            <>
              {t("sessionManager.expandContent", {
                defaultValue: "展开完整内容",
              })}
              <span className="text-fg-3">
                ({formatCharCount(item.content.length)})
              </span>
            </>
          )}
        </button>
      )}
    </div>
  );
});

interface SessionToolGroupProps {
  item: SessionToolItem;
  isActive: boolean;
  searchQuery?: string;
}

/** 合并后的工具调用 chip + 默认折叠的工具输出。 */
export const SessionToolGroup = memo(function SessionToolGroup({
  item,
  isActive,
  searchQuery,
}: SessionToolGroupProps) {
  const { t } = useTranslation();
  const [openByUser, setOpenByUser] = useState(false);
  const [showAllByUser, setShowAllByUser] = useState(false);
  const outputId = `session-tool-output-${item.key}`;
  const lines = item.output ? item.output.split(/\r?\n/) : [];
  const hidden = Math.max(0, lines.length - TOOL_OUTPUT_PREVIEW_LINES);
  // 查找命中在工具输出里：和长消息一样自动展开，否则高亮藏在折叠里看不见；
  // 命中不在预览行里就连「还有 N 行」一起展开
  const query = searchQuery?.toLowerCase() ?? "";
  const outputHit = !!query && !!item.output?.toLowerCase().includes(query);
  const previewHit =
    outputHit &&
    lines
      .slice(0, TOOL_OUTPUT_PREVIEW_LINES)
      .join("\n")
      .toLowerCase()
      .includes(query);
  const open = openByUser || outputHit;
  const showAll = showAllByUser || (outputHit && !previewHit);
  const shownLines =
    showAll || hidden === 0 ? lines : lines.slice(0, TOOL_OUTPUT_PREVIEW_LINES);
  const shownText = shownLines.join("\n");

  return (
    <div
      className={cn(
        "flex min-w-0 flex-col items-start gap-1.5 rounded-panel px-3.5",
        isActive && "ring-2 ring-ring ring-offset-2 ring-offset-app",
      )}
    >
      {item.names.length > 0 && (
        <span className="inline-flex h-6 max-w-full items-center gap-1.5 rounded-full bg-subtle pe-2.5 ps-2 text-caption text-fg-2">
          <Wrench className="h-3.5 w-3.5 shrink-0" strokeWidth={1.5} />
          <span className="sr-only">
            {t("sessionManager.toolCalls", { defaultValue: "工具调用：" })}
          </span>
          <span className="truncate">{formatToolNames(item.names)}</span>
        </span>
      )}
      {item.output && (
        <>
          <button
            type="button"
            aria-expanded={open}
            aria-controls={outputId}
            onClick={() => setOpenByUser(!open)}
            className="-ms-1 inline-flex h-6 items-center gap-1 rounded-control pe-2 ps-1 text-caption text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
          >
            {open ? (
              <ChevronDown className="h-3.5 w-3.5" strokeWidth={1.5} />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" strokeWidth={1.5} />
            )}
            {t("sessionManager.toolOutput", {
              defaultValue: "工具输出 · {{size}} 字",
              size: formatCharCount(item.output.length),
            })}
          </button>
          {open && (
            <div
              id={outputId}
              className="self-stretch overflow-hidden rounded-[8px] border border-border bg-subtle"
            >
              <div
                tabIndex={0}
                role="region"
                aria-label={t("sessionManager.toolOutputRegion", {
                  defaultValue: "工具输出",
                })}
                className="max-h-[480px] overflow-auto px-3 py-2 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
              >
                <pre className="m-0 whitespace-pre font-mono text-caption text-fg-2">
                  {searchQuery
                    ? highlightText(shownText, searchQuery)
                    : shownText}
                </pre>
              </div>
              {hidden > 0 && (
                <div className="flex items-center gap-2 border-t border-border px-3 py-1.5 text-caption text-fg-3">
                  {!showAll &&
                    t("sessionManager.toolOutputMore", {
                      defaultValue: "还有 {{count}} 行",
                      count: hidden,
                    })}
                  <button
                    type="button"
                    onClick={() => setShowAllByUser(!showAll)}
                    className="text-fg-2 underline underline-offset-2 hover:text-fg-1"
                  >
                    {showAll
                      ? t("sessionManager.collapseContent", {
                          defaultValue: "收起",
                        })
                      : t("sessionManager.showAll", {
                          defaultValue: "显示全部",
                        })}
                  </button>
                </div>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
});
