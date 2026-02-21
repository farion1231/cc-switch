import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { SessionMessage } from "@/types";
import { formatTimestamp, getRoleLabel, getRoleTone } from "./utils";
import { MarkdownContent } from "./MarkdownContent";

interface SessionMessageItemProps {
  message: SessionMessage;
  index: number;
  isActive: boolean;
  setRef: (el: HTMLDivElement | null) => void;
  onCopy: (content: string) => void;
  renderMarkdown: boolean;
}

export function SessionMessageItem({
  message,
  isActive,
  setRef,
  onCopy,
  renderMarkdown,
}: SessionMessageItemProps) {
  const { t } = useTranslation();

  return (
    <div
      ref={setRef}
      className={cn(
        "rounded-lg border px-3 py-2.5 relative group transition-all min-w-0",
        message.role.toLowerCase() === "user"
          ? "bg-primary/5 border-primary/20 ml-4"
          : message.role.toLowerCase() === "assistant"
            ? "bg-blue-500/5 border-blue-500/20 mr-4"
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
      {renderMarkdown ? (
        <MarkdownContent content={message.content} />
      ) : (
        <div className="whitespace-pre-wrap text-sm leading-relaxed break-words w-0 min-w-full overflow-x-auto">
          {message.content}
        </div>
      )}
    </div>
  );
}
