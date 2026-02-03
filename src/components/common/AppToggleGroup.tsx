import React from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";

interface AppToggleGroupProps {
  apps: Record<AppId, boolean>;
  onToggle: (app: AppId, enabled: boolean) => void;
}

export const AppToggleGroup: React.FC<AppToggleGroupProps> = ({ apps, onToggle }) => {
  return (
    <div className="flex items-center gap-1.5 flex-shrink-0">
      {APP_IDS.map((app) => {
        const { label, icon, activeClass } = APP_ICON_MAP[app];
        const enabled = apps[app];
        return (
          <Tooltip key={app}>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => onToggle(app, !enabled)}
                className={`w-7 h-7 rounded-lg flex items-center justify-center transition-all ${
                  enabled
                    ? activeClass
                    : "opacity-35 hover:opacity-70"
                }`}
              >
                {icon}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              <p>{label}{enabled ? " ✓" : ""}</p>
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
};
