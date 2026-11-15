import { memo } from "react";

import { cn } from "@/lib/utils";
import { useReaderContext } from "./context";
import { useReaderT } from "./i18n";
import { GlyphCell } from "./rowStyles";
import { SessionMarkdown } from "./SessionMarkdown";
import type { NoteStep } from "./turns";

/** 超过 3 行或 240 字才折叠（规则 10） */
const isLongNote = (text: string) =>
  text.length > 240 || text.split("\n").length > 3;

export interface SessionNoteRowProps {
  step: NoteStep;
  expanded: boolean;
  onToggle: (id: string, expanded: boolean) => void;
}

/** 过程中的助手说明：小字次级色 Markdown，默认前 3 行，点开全文 */
export const SessionNoteRow = memo(function SessionNoteRow({
  step,
  expanded,
  onToggle,
}: SessionNoteRowProps) {
  const rt = useReaderT();
  const { projectDir, searchQuery } = useReaderContext();
  const long = isLongNote(step.text);
  const clamped = long && !expanded;
  const regionId = `reader-note-${step.id}`;

  return (
    <div className="flex min-w-0 gap-2 px-1.5 py-1.5">
      <GlyphCell glyph="◦" className="h-[18px] text-fg-3" />
      <div className="min-w-0 flex-1">
        <span className="sr-only">{rt("note")}：</span>
        <div
          id={regionId}
          className={cn(
            "relative",
            clamped &&
              "max-h-[54px] overflow-hidden [mask-image:linear-gradient(to_bottom,black_60%,transparent)]",
          )}
        >
          <SessionMarkdown
            content={step.text}
            variant="note"
            searchQuery={searchQuery}
            projectDir={projectDir}
            className="[&_p]:my-0.5"
          />
        </div>
        {long && (
          <button
            type="button"
            aria-expanded={expanded}
            aria-controls={regionId}
            onClick={() => onToggle(step.id, expanded)}
            className="mt-0.5 rounded-[2px] text-caption text-fg-3 underline decoration-border-strong underline-offset-[3px] hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {expanded ? rt("collapseContent") : rt("noteExpand")}
          </button>
        )}
      </div>
    </div>
  );
});
