import { resolveVisibleApps } from "@/config/qoder";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api/types";
import { useSettingsQuery } from "@/lib/query";

/**
 * MCP / Skills 矩阵只列「应用」页里设为显示的应用。隐藏只影响界面，
 * 已经写进隐藏应用的配置照常保留、照常同步。
 */
export function useVisibleAppIds<T extends AppId>(appIds: readonly T[]): T[] {
  const { data: settings } = useSettingsQuery();
  const { i18n } = useTranslation();
  return useMemo(() => {
    const visible = resolveVisibleApps(settings, i18n.language);
    return appIds.filter((app) => visible[app]);
  }, [appIds, settings, i18n.language]);
}
