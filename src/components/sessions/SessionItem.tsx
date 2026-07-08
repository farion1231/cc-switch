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
import type { SessionMeta } from "@/types";
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
  onSelect: (key: string) => void;
  onToggleChecked: (checked: boolean) => void;
}

export function SessionItem({
  session,
  isSelected,
  selectionMode,
  isChecked,
  isCheckDisabled = false,
  searchQuery,
  onSelect,
  onToggleChecked,
}: SessionItemProps) {
  const { t } = useTranslation();
  const title = formatSessionTitle(session);
  const lastActive = session.lastActiveAt || session.createdAt || undefined;
  const sessionKey = getSessionKey(session);

  return (
    <div
      className={cn(
        "group flex items-start gap-2 rounded-[1rem] px-3 py-2.5 transition-all",
        isSelected
          ? "border border-sky-300/60 bg-[linear-gradient(145deg,rgba(255,255,255,0.82),rgba(224,242,254,0.78))] shadow-[0_18px_34px_-26px_rgba(14,165,233,0.55)] dark:border-sky-400/20 dark:bg-[linear-gradient(145deg,rgba(14,116,144,0.2),rgba(8,47,73,0.18))]"
          : "border border-transparent hover:bg-white/35 dark:hover:bg-white/[0.04]",
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
      <button
        type="button"
        onClick={() => onSelect(sessionKey)}
        className="min-w-0 flex-1 text-left"
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
              "size-4 shrink-0 text-muted-foreground/50 transition-transform",
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
    </div>
  );
}
