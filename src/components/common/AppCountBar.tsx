import React from "react";
import { Badge } from "@/components/ui/badge";
import type { AppId } from "@/lib/api/types";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { cn } from "@/lib/utils";

interface AppCountBarProps {
  totalLabel: string;
  counts: Partial<Record<AppId, number>>;
  appIds?: AppId[];
  /** 已激活的 App 过滤集合；当提供 onAppClick 时启用 chip 高亮 */
  activeApps?: Set<AppId>;
  /** 给每个 App badge 添加点击行为；提供时 badge 变为按钮 */
  onAppClick?: (app: AppId) => void;
}

export const AppCountBar: React.FC<AppCountBarProps> = ({
  totalLabel,
  counts,
  appIds = APP_IDS,
  activeApps,
  onAppClick,
}) => {
  const interactive = !!onAppClick;
  return (
    <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6 flex items-center justify-between gap-4">
      <Badge variant="outline" className="bg-background/50 h-7 px-3">
        {totalLabel}
      </Badge>
      <div className="flex items-center gap-2 overflow-x-auto no-scrollbar">
        {appIds.map((app) => {
          const active = activeApps?.has(app) ?? false;
          const badge = (
            <Badge
              key={app}
              variant="secondary"
              className={cn(
                APP_ICON_MAP[app].badgeClass,
                interactive && "cursor-pointer transition-shadow",
                active &&
                  "ring-2 ring-primary ring-offset-1 ring-offset-background",
              )}
            >
              <span className="opacity-75">{APP_ICON_MAP[app].label}:</span>
              <span className="font-bold ml-1">{counts[app] ?? 0}</span>
            </Badge>
          );
          if (!interactive) return badge;
          return (
            <button
              key={app}
              type="button"
              onClick={() => onAppClick(app)}
              className="focus:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-md"
              aria-pressed={active}
            >
              {badge}
            </button>
          );
        })}
      </div>
    </div>
  );
};
