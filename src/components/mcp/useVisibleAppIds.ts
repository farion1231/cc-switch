import { useMemo } from "react";
import type { AppId } from "@/lib/api/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { useSettingsQuery } from "@/lib/query";

/**
 * MCP / Skills 矩阵只列「应用」页里设为显示的应用。隐藏只影响界面，
 * 已经写进隐藏应用的配置照常保留、照常同步。DeepSeek Harness Desktop 没有自己的列
 * （MCP 同时写进两个 profile，Skills 共用 `$DSH_HOME/skills`），只显示 Desktop 时
 * 也列出 DeepSeek Harness 那一列。
 */
export function useVisibleAppIds<T extends AppId>(appIds: readonly T[]): T[] {
  const { data: settings } = useSettingsQuery();
  const visibleApps = settings?.visibleApps;
  return useMemo(() => {
    const visible = { ...DEFAULT_VISIBLE_APPS, ...visibleApps };
    return appIds.filter(
      (app) => visible[app] || (app === "dsh" && visible["dsh-desktop"]),
    );
  }, [appIds, visibleApps]);
}
