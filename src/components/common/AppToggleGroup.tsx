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
  appIds?: AppId[];
  pendingApps?: Partial<Record<AppId, boolean>>;
  disabled?: boolean;
}

export const AppToggleGroup: React.FC<AppToggleGroupProps> = ({
  apps,
  onToggle,
  appIds = APP_IDS,
  pendingApps,
  disabled = false,
}) => {
  return (
    <div className="flex items-center gap-1.5 flex-shrink-0">
      {appIds.map((app) => {
        const { label, icon, activeClass } = APP_ICON_MAP[app];
        const enabled = apps[app];
        const isPending = pendingApps?.[app] !== undefined;
        return (
          <Tooltip key={app}>
            <TooltipTrigger asChild>
              <button
                type="button"
                aria-label={label}
                onClick={() => onToggle(app, !enabled)}
                disabled={disabled}
                className={`relative w-7 h-7 rounded-lg flex items-center justify-center transition-all ${
                  enabled ? activeClass : "opacity-35 hover:opacity-70"
                } ${disabled ? "cursor-not-allowed opacity-60" : ""}`}
              >
                {icon}
                {isPending && (
                  <span
                    className="absolute right-0.5 top-0.5 h-1.5 w-1.5 rounded-full bg-sky-500 ring-2 ring-background"
                    aria-label={`${label} pending`}
                  />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              <p>
                {label}
                {enabled ? " ✓" : ""}
                {isPending ? " •" : ""}
              </p>
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
};
