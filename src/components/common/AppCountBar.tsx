import React from "react";
import { Badge } from "@/components/ui/badge";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";

interface AppCountBarProps {
  totalLabel: string;
  counts: Record<AppId, number>;
  appIds?: AppId[];
}

export const AppCountBar: React.FC<AppCountBarProps> = ({
  totalLabel,
  counts,
  appIds = APP_IDS,
}) => {
  return (
    <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6 flex items-center justify-between gap-4">
      <Badge variant="outline" className="bg-background/50 h-7 px-3">
        {totalLabel}
      </Badge>
      <div className="flex items-center gap-2 overflow-x-auto no-scrollbar">
        {appIds.map((app) => (
          <Badge
            key={app}
            variant="secondary"
            className={APP_ICON_MAP[app].badgeClass}
          >
            <span className="opacity-75">{APP_ICON_MAP[app].label}:</span>
            <span className="font-bold ml-1">{counts[app]}</span>
          </Badge>
        ))}
      </div>
    </div>
  );
};
