import { memo, useState } from "react";
import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";

import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { formatMessageTime } from "../utils";
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
 * 提问气泡：靠右显示（左右对话布局），时间和复制按钮在气泡外上方；气泡只放正文，
 * 底色是 Agent 主题色的浅色，右上角收小做出尖角。纯文本 + 围栏代码（决策 D7）、贴图缩略图。
 */
export const SessionQuestion = memo(function SessionQuestion({
  question,
  forceExpanded,
}: SessionQuestionProps) {
  const { t } = useTranslation();
  const { searchQuery, onCopy } = useReaderContext();
  const [expanded, setExpanded] = useState(false);
  const text = question.text;
  const long = text.length > COLLAPSE_THRESHOLD;
  const open = expanded || forceExpanded;
  const shown =
    long && !open
      ? createCollapsedMarkdownPreview(text, COLLAPSED_LENGTH)
      : text;

  return (
    <div className="group/question flex min-w-0 flex-col items-end gap-1">
      {/* 时间与复制放在气泡外：气泡里只放正文；靠右本身就表示「你」，读屏另给文字 */}
      <div className="flex h-5 items-center gap-1 text-caption text-fg-3">
        <span className="sr-only">
          {t("sessionManager.you", { defaultValue: "你" })}
        </span>
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
              "h-5 w-5 group-hover/question:opacity-100",
            )}
          >
            <Copy aria-hidden className="h-3.5 w-3.5" strokeWidth={1.5} />
          </button>
        </HoverTip>
        {question.ts ? (
          <time
            dateTime={new Date(question.ts).toISOString()}
            title={new Date(question.ts).toLocaleString()}
            className="tabular-nums"
          >
            {formatMessageTime(question.ts)}
          </time>
        ) : null}
      </div>
      <div
        className="min-w-0 max-w-full rounded-[16px] rounded-se-[4px] px-3.5 py-2 text-fg-1"
        style={{
          // Agent 主题色铺一层浅色：深浅两套下都和左侧无底色的 Agent 输出区分开
          background:
            "color-mix(in srgb, var(--reader-accent) 14%, var(--bg-subtle))",
        }}
      >
        {text && <SessionPlainText content={shown} searchQuery={searchQuery} />}
        {long && !forceExpanded && (
          <CollapseToggle
            expanded={expanded}
            total={text.length}
            onToggle={() => setExpanded((value) => !value)}
          />
        )}
        <SessionImageGrid
          images={question.images}
          className={text ? "mt-2" : undefined}
        />
      </div>
    </div>
  );
});
