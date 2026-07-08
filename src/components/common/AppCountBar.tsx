import React from "react";
import { Badge } from "@/components/ui/badge";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";

interface AppCountBarProps {
  totalLabel: string;
  counts: Partial<Record<AppId, number>>;
  appIds?: AppId[];
}

export const AppCountBar: React.FC<AppCountBarProps> = ({
  totalLabel,
  counts,
  appIds = APP_IDS,
}) => {
  return (
    <div className="rounded-xl border border-border bg-card shadow-sm mb-4 flex flex-shrink-0 items-center justify-between gap-4 px-5 py-3.5">
      <Badge
        variant="outline"
        className="h-7 shrink-0 whitespace-nowrap border-border bg-background px-3 text-foreground"
      >
        {totalLabel}
      </Badge>
      <div className="flex min-w-0 items-center gap-2 overflow-x-auto no-scrollbar">
        {appIds.map((app) => (
          <Badge
            key={app}
            variant="secondary"
            className={APP_ICON_MAP[app].badgeClass}
          >
            <span className="opacity-75">{APP_ICON_MAP[app].label}:</span>
            <span className="font-bold ml-1">{counts[app] ?? 0}</span>
          </Badge>
        ))}
      </div>
    </div>
  );
};
