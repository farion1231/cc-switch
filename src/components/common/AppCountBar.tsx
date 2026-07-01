import React from "react";
import { Badge } from "@/components/ui/badge";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";

interface AppCountBarProps {
  totalLabel: string;
  counts: Partial<Record<AppId, number>>;
  appIds?: AppId[];
  selectedApps?: ReadonlySet<AppId>;
  onToggleApp?: (app: AppId) => void;
}

export const AppCountBar: React.FC<AppCountBarProps> = ({
  totalLabel,
  counts,
  appIds = APP_IDS,
  selectedApps,
  onToggleApp,
}) => {
  const hasSelectableCounts = Boolean(onToggleApp && selectedApps);

  return (
    <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6 flex items-center justify-between gap-4">
      <Badge variant="outline" className="bg-background/50 h-7 px-3">
        {totalLabel}
      </Badge>
      <div className="flex items-center gap-2 overflow-x-auto no-scrollbar">
        {appIds.map((app) => {
          const selected = Boolean(selectedApps?.has(app));
          const content = (
            <>
              <span className="opacity-75">{APP_ICON_MAP[app].label}:</span>
              <span className="font-bold ml-1">{counts[app] ?? 0}</span>
            </>
          );

          if (hasSelectableCounts) {
            return (
              <button
                key={app}
                type="button"
                aria-pressed={selected}
                onClick={() => onToggleApp?.(app)}
                className={`inline-flex h-7 items-center rounded-full px-2.5 text-xs font-semibold transition-all ${APP_ICON_MAP[app].badgeClass} ${
                  selected
                    ? ""
                    : hasSelectableCounts
                      ? "opacity-35 hover:opacity-70"
                      : ""
                }`}
              >
                {content}
              </button>
            );
          }

          return (
            <Badge
              key={app}
              variant="secondary"
              className={APP_ICON_MAP[app].badgeClass}
            >
              {content}
            </Badge>
          );
        })}
      </div>
    </div>
  );
};
