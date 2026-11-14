import { useMemo } from "react";
import type { AppId } from "@/lib/api/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { useSettingsQuery } from "@/lib/query";

/**
 * MCP / Skills 矩阵只列「应用」页里设为显示的应用。隐藏只影响界面，
 * 已经写进隐藏应用的配置照常保留、照常同步。
 */
export function useVisibleAppIds<T extends AppId>(appIds: readonly T[]): T[] {
  const { data: settings } = useSettingsQuery();
  const visibleApps = settings?.visibleApps;
  return useMemo(() => {
    const visible = { ...DEFAULT_VISIBLE_APPS, ...visibleApps };
    return appIds.filter((app) => visible[app]);
  }, [appIds, visibleApps]);
}
