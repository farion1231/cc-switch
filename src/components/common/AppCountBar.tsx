import React from "react";
import { Badge } from "@/components/ui/badge";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { cn } from "@/lib/utils";

export type AppCountBarFilter = AppId | "all";

const interactiveBadgeClass =
  "inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-all outline-none focus:outline-none focus:ring-0 focus:ring-offset-0 focus-visible:outline-none focus-visible:ring-0 focus-visible:ring-offset-0";

interface AppCountBarProps {
  totalLabel: string;
  counts: Partial<Record<AppId, number>>;
  appIds?: AppId[];
  selectedApp?: AppCountBarFilter;
  onAppSelect?: (app: AppCountBarFilter) => void;
}

export const AppCountBar: React.FC<AppCountBarProps> = ({
  totalLabel,
  counts,
  appIds = APP_IDS,
  selectedApp = "all",
  onAppSelect,
}) => {
  const isInteractive = Boolean(onAppSelect);

  return (
    <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6 flex items-center justify-between gap-4">
      {isInteractive ? (
        <button
          type="button"
          className={cn(
            interactiveBadgeClass,
            "bg-background/50 h-7 px-3 cursor-pointer hover:bg-foreground/10 hover:text-foreground active:bg-foreground/15 active:scale-[0.98]",
          )}
          onClick={() => onAppSelect?.("all")}
        >
          {totalLabel}
        </button>
      ) : (
        <Badge variant="outline" className="bg-background/50 h-7 px-3">
          {totalLabel}
        </Badge>
      )}
      <div className="flex items-center gap-2 overflow-x-auto no-scrollbar">
        {appIds.map((app) =>
          isInteractive ? (
            <button
              key={app}
              type="button"
              className={cn(
                interactiveBadgeClass,
                APP_ICON_MAP[app].badgeClass,
                "cursor-pointer active:brightness-95 active:scale-[0.98] active:opacity-100",
                selectedApp === app
                  ? "font-semibold opacity-100"
                  : "opacity-60 hover:opacity-100",
              )}
              onClick={() => onAppSelect?.(app)}
              aria-pressed={selectedApp === app}
              aria-label={`Filter by ${APP_ICON_MAP[app].label}`}
            >
              <span className="opacity-75">{APP_ICON_MAP[app].label}:</span>
              <span className="font-bold ml-1">{counts[app] ?? 0}</span>
            </button>
          ) : (
            <Badge
              key={app}
              variant="secondary"
              className={APP_ICON_MAP[app].badgeClass}
            >
              <span className="opacity-75">{APP_ICON_MAP[app].label}:</span>
              <span className="font-bold ml-1">{counts[app] ?? 0}</span>
            </Badge>
          ),
        )}
      </div>
    </div>
  );
};
