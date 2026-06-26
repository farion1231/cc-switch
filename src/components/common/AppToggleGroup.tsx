import React from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { Lock } from "lucide-react";

interface AppToggleGroupProps {
  apps: Partial<Record<AppId, boolean>>;
  onToggle: (app: AppId, enabled: boolean) => void;
  appIds?: AppId[];
  lockedApps?: AppId[];
}

export const AppToggleGroup: React.FC<AppToggleGroupProps> = ({
  apps,
  onToggle,
  appIds = APP_IDS,
  lockedApps = [],
}) => {
  return (
    <div className="flex items-center gap-1.5 flex-shrink-0">
      {appIds.map((app) => {
        const { label, icon, activeClass } = APP_ICON_MAP[app];
        const enabled = apps[app];
        const isLocked = lockedApps.includes(app);
        // Locked apps show as enabled (with background) + lock icon
        const isVisuallyEnabled = enabled || isLocked;
        return (
          <Tooltip key={app}>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => !isLocked && onToggle(app, !enabled)}
                className={`w-7 h-7 rounded-lg flex items-center justify-center transition-all relative ${
                  isLocked
                    ? "cursor-not-allowed"
                    : isVisuallyEnabled
                      ? activeClass
                      : "opacity-35 hover:opacity-70"
                } ${isVisuallyEnabled ? activeClass : ""}`}
              >
                {icon}
                {isLocked && (
                  <Lock
                    size={8}
                    className="absolute -bottom-0.5 -right-0.5 text-muted-foreground"
                  />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              <p>
                {label}
                {enabled ? " ✓" : ""}
                {isLocked ? " (🔒)" : ""}
              </p>
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
};
