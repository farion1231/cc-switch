import { memo, useState } from "react";
import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";

import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { formatClock } from "../utils";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import {
  createCollapsedMarkdownPreview,
  SessionPlainText,
} from "./SessionMarkdown";
import { SessionImageGrid } from "./SessionImage";
import type { TurnQuestion } from "./turns";

/** 超过 3000 字折叠到 1500 字（§6.4 提问 / 规则 11） */
export const COLLAPSE_THRESHOLD = 3000;
export const COLLAPSED_LENGTH = 1500;

export const formatCharCount = (count: number) =>
  count >= 1000 ? `${(count / 1000).toFixed(1)}k` : String(count);

export const rowIconButton =
  "inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-control text-fg-3 opacity-0 transition-[opacity,color,background-color] hover:bg-selected hover:text-fg-1 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

/** 长内容的「展开完整内容 / 收起」 */
export const CollapseToggle = ({
  expanded,
  total,
  onToggle,
}: {
  expanded: boolean;
  total: number;
  onToggle: () => void;
}) => {
  const rt = useReaderT();
  return (
    <button
      type="button"
      aria-expanded={expanded}
      onClick={onToggle}
      className="mt-1.5 inline-flex items-center gap-1 rounded-[2px] text-caption text-fg-2 transition-colors hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {expanded ? (
        rt("collapseContent")
      ) : (
        <>
          {rt("expandContent")}
          <span className="tabular-nums text-fg-3">
            ({formatCharCount(total)})
          </span>
        </>
      )}
    </button>
  );
};

export interface SessionQuestionProps {
  question: TurnQuestion;
  /** 查找命中：长提问自动展开 */
  forceExpanded: boolean;
}

/**
 * 提问气泡：靠右显示（左右对话布局），底色是 Agent 主题色的淡色，右上角收小做出气泡尖角；
 * 纯文本 + 围栏代码（决策 D7），贴图缩略图，复制按钮悬停出现。
 */
export const SessionQuestion = memo(function SessionQuestion({
  question,
  forceExpanded,
}: SessionQuestionProps) {
  const { t } = useTranslation();
  const { style, searchQuery, onCopy } = useReaderContext();
  const [expanded, setExpanded] = useState(false);
  const text = question.text;
  const long = text.length > COLLAPSE_THRESHOLD;
  const open = expanded || forceExpanded;
  const shown =
    long && !open
      ? createCollapsedMarkdownPreview(text, COLLAPSED_LENGTH)
      : text;

  return (
    <div
      className="group/question relative min-w-0 rounded-[14px] rounded-se-[4px] border border-border px-3.5 py-2.5"
      style={{
        // 主题色淡淡铺一层，深浅色下都能和 Agent 一侧的无底色内容区分开
        background:
          "color-mix(in srgb, var(--reader-accent) 10%, var(--bg-card))",
      }}
    >
      <div className="flex h-6 items-center justify-end gap-1.5">
        <HoverTip
          content={t("sessionManager.copyShort", { defaultValue: "复制" })}
        >
          <button
            type="button"
            aria-label={t("sessionManager.copyQuestion", {
              defaultValue: "复制这条提问",
            })}
            onClick={() =>
              onCopy(
                text,
                t("sessionManager.messageCopied", {
                  defaultValue: "已复制这条消息",
                }),
              )
            }
            className={cn(
              rowIconButton,
              "-ms-1 group-hover/question:opacity-100",
            )}
          >
            <Copy aria-hidden className="h-3.5 w-3.5" strokeWidth={1.5} />
          </button>
        </HoverTip>
        <div className="flex-1" />
        {style.userGlyph && (
          <span
            aria-hidden
            className="font-mono text-caption font-semibold text-[var(--reader-accent)]"
          >
            {style.userGlyph}
          </span>
        )}
        <span className="text-caption font-semibold text-fg-1">
          {t("sessionManager.you", { defaultValue: "你" })}
        </span>
        {question.ts ? (
          <time
            dateTime={new Date(question.ts).toISOString()}
            title={new Date(question.ts).toLocaleString()}
            className="text-caption tabular-nums text-fg-3"
          >
            {formatClock(question.ts)}
          </time>
        ) : null}
      </div>
      {text && (
        <SessionPlainText
          content={shown}
          searchQuery={searchQuery}
          className="mt-0.5"
        />
      )}
      {long && !forceExpanded && (
        <CollapseToggle
          expanded={expanded}
          total={text.length}
          onToggle={() => setExpanded((value) => !value)}
        />
      )}
      <SessionImageGrid images={question.images} className="mt-2" />
    </div>
  );
});
