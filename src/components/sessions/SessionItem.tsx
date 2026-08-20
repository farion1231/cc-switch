import { ChevronRight, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { SessionMeta, SessionSearchSnippet } from "@/types";
import {
  formatRelativeTime,
  formatSessionTitle,
  getProviderIconName,
  getProviderLabel,
  getSessionKey,
  highlightText,
} from "./utils";

interface SessionItemProps {
  session: SessionMeta;
  isSelected: boolean;
  selectionMode: boolean;
  isChecked: boolean;
  isCheckDisabled?: boolean;
  searchQuery?: string;
  snippets?: SessionSearchSnippet[];
  onSelect: (key: string) => void;
  onToggleChecked: (checked: boolean) => void;
  onSnippetSelect: (session: SessionMeta, messageIndex: number) => void;
}

export function SessionItem({
  session,
  isSelected,
  selectionMode,
  isChecked,
  isCheckDisabled = false,
  searchQuery,
  snippets,
  onSelect,
  onToggleChecked,
  onSnippetSelect,
}: SessionItemProps) {
  const { t } = useTranslation();
  const title = formatSessionTitle(session);
  const lastActive = session.lastActiveAt || session.createdAt || undefined;
  const sessionKey = getSessionKey(session);

  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-lg px-3 py-2.5 transition-all group",
        isSelected
          ? "bg-primary/10 border border-primary/30"
          : "hover:bg-muted/60 border border-transparent",
      )}
    >
      {selectionMode && (
        <div className="shrink-0 pt-0.5">
          <Checkbox
            checked={isChecked}
            disabled={isCheckDisabled}
            aria-label={t("sessionManager.selectForBatch", {
              defaultValue: "选择会话",
            })}
            onCheckedChange={(checked) => onToggleChecked(Boolean(checked))}
          />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <button
          type="button"
          onClick={() => onSelect(sessionKey)}
          className="w-full text-left"
        >
          <div className="flex items-center gap-2 mb-1">
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="shrink-0">
                  <ProviderIcon
                    icon={getProviderIconName(session.providerId)}
                    name={session.providerId}
                    size={18}
                  />
                </span>
              </TooltipTrigger>
              <TooltipContent>
                {getProviderLabel(session.providerId, t)}
              </TooltipContent>
            </Tooltip>
            <span className="text-sm font-medium line-clamp-2 flex-1">
              {searchQuery ? highlightText(title, searchQuery) : title}
            </span>
            <ChevronRight
              className={cn(
                "size-4 text-muted-foreground/50 shrink-0 transition-transform",
                isSelected && "text-primary rotate-90",
              )}
            />
          </div>

          <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
            <Clock className="size-3" />
            <span>
              {lastActive
                ? formatRelativeTime(lastActive, t)
                : t("common.unknown")}
            </span>
          </div>
        </button>

        {snippets && snippets.length > 0 && (
          <div className="mt-1.5 space-y-1">
            {snippets.map((snippet) => (
              <button
                key={snippet.messageIndex}
                type="button"
                onClick={() => onSnippetSelect(session, snippet.messageIndex)}
                title={t("sessionManager.jumpToMatch", {
                  defaultValue: "跳转到匹配位置",
                })}
                className="w-full rounded bg-muted/60 px-1.5 py-1 text-left text-[11px] leading-snug text-muted-foreground line-clamp-2 transition-colors hover:bg-muted hover:text-foreground"
              >
                {highlightText(snippet.text, searchQuery ?? "")}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
