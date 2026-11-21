import { memo } from "react";
import { Bookmark } from "lucide-react";

import { highlightText } from "../utils";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import type { TurnChapter } from "./turns";

export interface SessionChapterRowProps {
  chapter: TurnChapter;
}

/**
 * 章节行：Claude Code desktop 里模型用 `mark_chapter` 打的导航点。
 * 像小标题一样横在时间线里，不能展开；摘要跟在标题后面，放不下就截断。
 */
export const SessionChapterRow = memo(function SessionChapterRow({
  chapter,
}: SessionChapterRowProps) {
  const rt = useReaderT();
  const { searchQuery } = useReaderContext();
  const mark = (text: string) =>
    searchQuery ? highlightText(text, searchQuery) : text;

  return (
    <div className="flex min-w-0 items-center gap-2 border-b border-border px-1.5 pb-1.5 pt-2.5">
      <Bookmark
        aria-hidden
        className="h-3.5 w-3.5 shrink-0 text-[var(--reader-accent)]"
        strokeWidth={2}
      />
      <span className="sr-only">{rt("chapter")}：</span>
      <span className="min-w-0 shrink truncate text-body font-semibold text-fg-1">
        {mark(chapter.title)}
      </span>
      {chapter.summary && (
        <span
          className="min-w-0 flex-1 truncate text-caption text-fg-3"
          title={chapter.summary}
        >
          {mark(chapter.summary)}
        </span>
      )}
    </div>
  );
});
