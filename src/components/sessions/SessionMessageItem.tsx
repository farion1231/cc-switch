import { memo, useState } from "react";
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

  const isLong = message.content.length > COLLAPSE_THRESHOLD;
  const hasSearchMatch =
    isLong &&
    !expanded &&
    !!searchQuery &&
    message.content.toLowerCase().includes(searchQuery.toLowerCase());
  const collapsed = isLong && !expanded && !hasSearchMatch;
  const displayContent = collapsed
    ? message.content.slice(0, COLLAPSED_LENGTH) + "…"
    : message.content;

  return (
    <div
      className={cn(
        "rounded-xl glass-card px-3 py-2.5 relative group transition-shadow min-w-0",
        message.role.toLowerCase() === "user"
          ? "!border-primary/25 bg-[color-mix(in_srgb,hsl(var(--primary))_8%,var(--glass-bg-strong))] ml-8"
          : message.role.toLowerCase() === "assistant"
            ? "!border-blue-500/25 bg-[color-mix(in_srgb,rgb(59_130_246)_8%,var(--glass-bg-strong))] mr-8"
            : "",
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
      <div className="whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-sm leading-relaxed min-w-0">
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
                ({Math.round(message.content.length / 1000)}k)
              </span>
            </>
          )}
        </button>
      )}
    </div>
  );
});
