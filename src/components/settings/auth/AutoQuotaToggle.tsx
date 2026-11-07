import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query/queries";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { ToggleCard } from "./ToggleCard";

/**
 * 「自动查询额度」（#8990）：关掉后官方账号的额度只在点 ↻ /「全部刷新」时查，打开授权中心、
 * 供应商卡片定时刷新、悬停托盘都不再自动发请求——网络代理还没就绪时不会意外直连官方服务。
 * 存在设置里（托盘在后端也要读）。
 */
export function AutoQuotaToggle() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [saving, setSaving] = useState(false);
  const save = async (autoQuery: boolean) => {
    setSaving(true);
    try {
      const current = await settingsApi.get();
      await settingsApi.save({
        ...current,
        officialQuotaManualOnly: !autoQuery,
      });
    } catch (error) {
      toast.error(t("common.error"), {
        description: extractErrorMessage(error) || String(error),
      });
    } finally {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      setSaving(false);
    }
  };
  return (
    <ToggleCard
      title={t("authCenter.autoQuota.title", {
        defaultValue: "自动查询额度",
      })}
      description={t("authCenter.autoQuota.description", {
        defaultValue:
          "关掉后，官方账号的额度只在你点刷新时查询：打开授权中心、卡片定时刷新和悬停托盘都不会自动向官方服务发请求。",
      })}
      checked={settings?.officialQuotaManualOnly !== true}
      disabled={!settings || saving}
      onCheckedChange={(checked) => void save(checked)}
    />
  );
}
