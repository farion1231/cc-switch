import React from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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
    <TooltipProvider delayDuration={300}>
      <div className="flex-shrink-0 flex items-center gap-3 overflow-x-auto no-scrollbar">
        <span className="text-sm font-medium text-foreground whitespace-nowrap">
          {totalLabel}
        </span>
        <div className="flex items-center gap-1">
          {appIds.map((app) => {
            const count = counts[app] ?? 0;
            const { icon, label } = APP_ICON_MAP[app];
            return (
              <Tooltip key={app}>
                <TooltipTrigger asChild>
                  <div
                    className={`flex items-center gap-1 px-1.5 h-6 rounded-md text-xs transition-opacity ${
                      count > 0
                        ? "font-medium"
                        : "opacity-30"
                    }`}
                  >
                    {icon}
                    <span className="tabular-nums">{count}</span>
                  </div>
                </TooltipTrigger>
                <TooltipContent side="bottom">
                  <p>{label}</p>
                </TooltipContent>
              </Tooltip>
            );
          })}
        </div>
      </div>
    </TooltipProvider>
  );
};
